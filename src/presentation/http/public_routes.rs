//! The website module's PUBLIC route surface (hand-written; user-owned;
//! see `metaphor.codegen.yaml`).
//!
//! The module DOES NOT SELF-MOUNT: it exports
//! [`website_public_routes`], a plain `axum::Router` the composing host
//! nests BARE of `company_auth` under the schema name —
//! `Router::new().nest("/api/v1/website", website_public_routes(..))`.
//! The module's own gates are the fence: hostname binding first, then
//! the §7.2 allowlist. NOTHING else answers unauthenticated — the
//! registered route table is the negative-enumeration probe's target.
//!
//! The allowlist (exhaustive):
//! - `GET /public/pages/{*url}`   the resolved published page + blocks
//! - `GET /public/menus`          the visible menu tree
//! - `GET /public/sitemap`        cursor-paginated, 45k/page, fresh
//! - `GET /public/robots.txt`     stored bytes, verbatim
//! - `GET /public/resolve`        the routing answer (matcher table)
//! - `POST /public/visitors/heartbeat`  the only mutating verb besides intake
//! - `POST /public/intake/{verb}` a DECLARATION name (fixed closed set)

use std::sync::Arc;

use axum::{
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;

use crate::application::service::{
    captcha_recaptcha::CaptchaVerifier,
    intake_contact::{ContactIntake, ContactPayload},
    intake_engine::{
        IntakeContext, IntakeDeclaration, IntakeEngine, TurnstileClient, TurnstileConfig,
    },
    lang_matcher::{apply_matcher, MatchInput},
    menu_service::MenuAdminService,
    page_service::PageAdminService,
    principal_port::{RefusingPrincipalVerifier, WebsitePrincipal, WebsitePrincipalVerifier},
    redirect_service::RedirectAdminService,
    website_error::WebsiteError,
    website_service::{normalize_host, WebsiteRootService},
};

/// The sitemap page cap (the upstream split threshold, generalized as
/// pagination: a bigger site pages, never truncates silently).
pub const SITEMAP_PAGE_CAP: i64 = 45_000;

/// The env var holding the visitor digest pepper.
pub const WEBSITE_PEPPER_ENV: &str = "WEBSITE_VISITOR_PEPPER";

/// The env var declaring whether this module's traffic arrives through a
/// trusted reverse proxy (the host's Caddy/nginx front). `true` → the
/// RIGHTMOST `X-Forwarded-For` hop is the caller's address; anything
/// else (the default) ignores the forwarded header entirely — every hop
/// of it is client-supplied text. Tolerant truth parsing: `true`, `1`,
/// `yes`, `on` (any case) arm the proxy posture; unset or any other
/// value keeps the direct-connection posture.
pub const WEBSITE_TRUSTED_PROXY_ENV: &str = "WEBSITE_TRUSTED_PROXY";

/// The shared public state (all cheap-to-clone service handles).
#[derive(Clone)]
pub struct WebsitePublicState {
    pub websites: Arc<WebsiteRootService>,
    pub pages: Arc<PageAdminService>,
    pub menus: Arc<MenuAdminService>,
    pub redirects: Arc<RedirectAdminService>,
    pub visitors: Arc<crate::application::service::visitor_service::VisitorEngine>,
    pub intake: Arc<IntakeEngine>,
    principal_port: Arc<dyn WebsitePrincipalVerifier>,
    /// Whether traffic arrives through a trusted reverse proxy (the
    /// rightmost forwarded hop is then the caller's address). Feeds
    /// [`visitor_ip`] only — rate shaping and digests, never
    /// authorization.
    trusted_proxy: bool,
}

impl WebsitePublicState {
    /// Compose the whole public surface over one pool with explicit
    /// config (the host's one-call wiring). The trusted-proxy posture
    /// is read from [`WEBSITE_TRUSTED_PROXY_ENV`]; hosts that need to
    /// control it in code use [`Self::compose_with_trusted_proxy`].
    pub fn compose(pool: sqlx::PgPool, pepper: String, turnstile: TurnstileConfig) -> Self {
        Self::compose_with_verifier(
            pool,
            pepper,
            CaptchaVerifier::Turnstile(TurnstileClient::new(turnstile)),
            trusted_proxy_from_env(),
        )
    }

    /// [`Self::compose`] with the trusted-proxy posture explicit — the
    /// probe entry (tests must not depend on process environment other
    /// tests mutate).
    pub fn compose_with_trusted_proxy(
        pool: sqlx::PgPool,
        pepper: String,
        turnstile: TurnstileConfig,
        trusted_proxy: bool,
    ) -> Self {
        Self::compose_with_verifier(
            pool,
            pepper,
            CaptchaVerifier::Turnstile(TurnstileClient::new(turnstile)),
            trusted_proxy,
        )
    }

    /// The provider-selection entry (§6.3): compose over the
    /// CONFIG-SELECTED captcha verifier — the turnstile default, the
    /// recaptcha sibling, or the fail-closed unknown arm (which refuses
    /// every gated verb with `website_captcha_provider_unknown`).
    pub fn compose_with_verifier(
        pool: sqlx::PgPool,
        pepper: String,
        verifier: CaptchaVerifier,
        trusted_proxy: bool,
    ) -> Self {
        let notifier: Arc<dyn crate::application::service::notifier_port::IntakeNotifier> =
            Arc::new(crate::application::service::notifier_port::UnwiredIntakeNotifier::new());
        Self {
            websites: Arc::new(WebsiteRootService::new(pool.clone())),
            pages: Arc::new(PageAdminService::new(pool.clone())),
            menus: Arc::new(MenuAdminService::new(pool.clone())),
            redirects: Arc::new(RedirectAdminService::new(pool.clone())),
            visitors: Arc::new(
                crate::application::service::visitor_service::VisitorEngine::new(
                    pool.clone(),
                    pepper.clone(),
                ),
            ),
            intake: Arc::new(IntakeEngine::with_verifier(
                pool,
                verifier,
                pepper,
                notifier,
            )),
            principal_port: Arc::new(RefusingPrincipalVerifier),
            trusted_proxy,
        }
    }

    /// [`Self::compose`] reading the pepper, the captcha provider
    /// knobs (`WEBSITE_CAPTCHA_PROVIDER` + the selected arm's secret
    /// and verify URL), and the trusted-proxy knob from the
    /// environment. An UNSET pepper stays empty and the first visitor
    /// verb refuses with the typed
    /// `website_visitor_pepper_not_configured` — fail-closed, never a
    /// zero-secret fallback. A host switching captcha providers changes
    /// ONLY env vars — no code.
    pub fn from_env(pool: sqlx::PgPool) -> Self {
        let pepper = std::env::var(WEBSITE_PEPPER_ENV).unwrap_or_default();
        Self::compose_with_verifier(
            pool,
            pepper,
            CaptchaVerifier::from_env(),
            trusted_proxy_from_env(),
        )
    }

    /// The principal-port install seam (the host bridges portal's
    /// verification surface here; unwired, every non-public tier
    /// reads 403 — fail-closed, never fail-open).
    pub fn install_principal_verifier(&mut self, verifier: Arc<dyn WebsitePrincipalVerifier>) {
        self.principal_port = verifier;
    }

    async fn principal(&self, headers: &HeaderMap) -> Option<WebsitePrincipal> {
        let presented = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|s| !s.is_empty())?;
        self.principal_port.verify(presented).await
    }
}

/// Map a typed service error to its HTTP shape (status + machine code;
/// the intake rate arm carries `Retry-After`).
pub fn website_error_response(err: WebsiteError) -> Response {
    let status = StatusCode::from_u16(err.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if let WebsiteError::IntakeRateLimited { retry_after_seconds } = &err {
        return (
            status,
            [
                ("retry-after", retry_after_seconds.to_string()),
                ("content-type", "application/json".into()),
            ],
            Json(json!({
                "error": err.to_string(),
                "code": err.code(),
                "retry_after_seconds": retry_after_seconds,
            })),
        )
            .into_response();
    }
    // Internal shapes never leak their text.
    let body = match &err {
        WebsiteError::Db(e) => {
            tracing::error!(error = ?e, "website public route internal error");
            json!({"error": "internal error", "code": err.code()})
        }
        WebsiteError::Internal(msg) => {
            tracing::error!(reason = %msg, "website public route internal error");
            json!({"error": "internal error", "code": err.code()})
        }
        other => json!({"error": other.to_string(), "code": other.code()}),
    };
    (status, Json(body)).into_response()
}

/// The trusted-proxy posture from [`WEBSITE_TRUSTED_PROXY_ENV`]:
/// tolerant truth (`true`/`1`/`yes`/`on`, any case) arms the proxy
/// posture; unset or any other value keeps direct connections (the
/// forwarded header is then client-controlled text and ignored).
fn trusted_proxy_from_env() -> bool {
    std::env::var(WEBSITE_TRUSTED_PROXY_ENV)
        .ok()
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on"))
        .unwrap_or(false)
}

/// The caller address for rate shaping and visitor digests only (never
/// authorization): `trusted_proxy` → the RIGHTMOST `X-Forwarded-For`
/// hop (the address the nearest trusted proxy recorded for the
/// connection it forwarded); otherwise the connection's socket address,
/// with the forwarded header ignored entirely (its hops are
/// client-controlled). Falls back to the socket address when a trusted
/// chain emits no header, and to `"unknown"` when no socket address is
/// available.
fn visitor_ip(
    headers: &HeaderMap,
    remote_addr: Option<std::net::SocketAddr>,
    trusted_proxy: bool,
) -> String {
    let socket = remote_addr.map(|a| a.to_string());
    if trusted_proxy {
        if let Some(hop) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            // RIGHTMOST hop: the entry the nearest trusted proxy
            // appended. Every hop to its left is client-supplied text
            // the caller could have written anything into.
            .and_then(|v| v.rsplit(',').next())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return hop.to_string();
        }
    }
    socket.unwrap_or_else(|| "unknown".to_string())
}

fn user_agent(headers: &HeaderMap) -> String {
    headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

/// Hostname binding: the Host header (normalized) → the live website.
/// Miss → the loud typed 404. NO fallback to any first website.
async fn bound_website(
    state: &WebsitePublicState,
    headers: &HeaderMap,
) -> Result<crate::application::service::website_service::WebsiteView, Response> {
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    match state.websites.website_by_host(&normalize_host(host)).await {
        Ok(w) => Ok(w),
        Err(e) => Err(website_error_response(e)),
    }
}

// ── request DTOs ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct HeartbeatRequest {
    pub session_id: String,
    pub url: Option<String>,
    pub page_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ResolveQuery {
    pub path: Option<String>,
    pub method: Option<String>,
    pub accept_language: Option<String>,
    pub bot: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SitemapQuery {
    pub cursor: Option<String>,
}

// ── handlers ────────────────────────────────────────────────────────────────

async fn public_page(
    State(state): State<WebsitePublicState>,
    headers: HeaderMap,
    Path(url): Path<String>,
) -> Response {
    let website = match bound_website(&state, &headers).await {
        Ok(w) => w,
        Err(resp) => return resp,
    };
    let principal = state.principal(&headers).await;
    // The path arrives without its leading slash depending on the
    // nest depth; normalize to the site-absolute form.
    let path = if url.starts_with('/') { url } else { format!("/{url}") };
    match state
        .pages
        .visible_page(website.id, &path, principal.as_ref().map(|p| p.user_uuid()))
        .await
    {
        Ok(Some(public)) => (
            StatusCode::OK,
            Json(json!({
                "visible": true,
                "page": {
                    "id": public.page.id,
                    "key": public.page.key,
                    "url": public.page.url,
                    "title": public.page.title,
                    "seo_name": public.page.seo_name,
                    "visibility": public.page.visibility,
                    "date_publish": public.page.date_publish,
                },
                "blocks": public.blocks,
            })),
        )
            .into_response(),
        Ok(None) => website_error_response(WebsiteError::NotFound(format!("page {path}"))),
        Err(e) => website_error_response(e),
    }
}

async fn public_menus(
    State(state): State<WebsitePublicState>,
    headers: HeaderMap,
) -> Response {
    let website = match bound_website(&state, &headers).await {
        Ok(w) => w,
        Err(resp) => return resp,
    };
    let principal = state.principal(&headers).await;
    match state
        .menus
        .tree_visible(website.id, principal.as_ref().map(|p| p.user_uuid()))
        .await
    {
        Ok(tree) => (StatusCode::OK, Json(json!({ "website_id": website.id, "menus": tree })))
            .into_response(),
        Err(e) => website_error_response(e),
    }
}

async fn public_sitemap(
    State(state): State<WebsitePublicState>,
    headers: HeaderMap,
    Query(q): Query<SitemapQuery>,
) -> Response {
    let website = match bound_website(&state, &headers).await {
        Ok(w) => w,
        Err(resp) => return resp,
    };
    match state
        .pages
        .sitemap_page(website.id, q.cursor.as_deref(), SITEMAP_PAGE_CAP)
        .await
    {
        Ok(rows) => {
            let urls: Vec<&str> = rows.iter().map(|r| r.url.as_str()).collect();
            let next_cursor =
                if (urls.len() as i64) >= SITEMAP_PAGE_CAP { rows.last().map(|r| r.url.clone()) } else { None };
            (
                StatusCode::OK,
                Json(json!({
                    "website_id": website.id,
                    "urls": urls,
                    "next_cursor": next_cursor,
                })),
            )
                .into_response()
        }
        Err(e) => website_error_response(e),
    }
}

async fn public_robots(
    State(state): State<WebsitePublicState>,
    headers: HeaderMap,
) -> Response {
    let website = match bound_website(&state, &headers).await {
        Ok(w) => w,
        Err(resp) => return resp,
    };
    // Served VERBATIM (sanitization is the webapp's concern). The
    // mismatched-host guard is structural: this website was RESOLVED
    // by exactly this host.
    (
        StatusCode::OK,
        [("content-type", "text/plain; charset=utf-8")],
        website.robots_txt.unwrap_or_default(),
    )
        .into_response()
}

async fn public_resolve(
    State(state): State<WebsitePublicState>,
    headers: HeaderMap,
    Query(q): Query<ResolveQuery>,
) -> Response {
    let website = match bound_website(&state, &headers).await {
        Ok(w) => w,
        Err(resp) => return resp,
    };
    let path = match q.path.as_deref() {
        Some(p) if p.starts_with('/') => p.to_string(),
        Some(p) => format!("/{p}"),
        None => "/".to_string(),
    };
    let method = q.method.unwrap_or_else(|| "GET".to_string()).to_uppercase();
    let bot = matches!(q.bot.as_deref(), Some("true") | Some("1"))
        || headers
            .get("x-bot")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("true"));

    let redirect = match state.redirects.answer(website.id, &path).await {
        Ok(a) => a,
        Err(e) => return website_error_response(e),
    };
    // The resolved page's stored url drives the canonical-301 row. A
    // tier-refused page counts as unresolved HERE (the routing answer
    // for it is the same 403 the page verb serves; the matcher never
    // leaks it as a serve).
    let resolved_url = match state
        .pages
        .visible_page(website.id, &path, None)
        .await
    {
        Ok(Some(public)) => Some(public.page.url.clone()),
        Ok(None) => None,
        Err(WebsiteError::PageVisibilityRefused) => None,
        Err(e) => return website_error_response(e),
    };

    let input = MatchInput {
        path: &path,
        method: &method,
        bot,
        default_lang_code: &website.default_lang_code,
        redirect,
        resolved_url: resolved_url.as_deref(),
    };
    let answer = apply_matcher(&input);
    let page = if answer.action == "serve" {
        match state.pages.visible_page(website.id, &path, None).await {
            Ok(Some(public)) => Some(json!({
                "id": public.page.id,
                "key": public.page.key,
                "url": public.page.url,
                "title": public.page.title,
            })),
            _ => None,
        }
    } else {
        None
    };
    (
        StatusCode::OK,
        Json(json!({
            "action": answer.action,
            "status": answer.status,
            "location": answer.location,
            "page": page,
        })),
    )
        .into_response()
}

async fn visitor_heartbeat(
    State(state): State<WebsitePublicState>,
    connect_info: Option<ConnectInfo<std::net::SocketAddr>>,
    headers: HeaderMap,
    Json(body): Json<HeartbeatRequest>,
) -> Response {
    // Declared no-op arms (not errors): bot suppression and the
    // tracking opt-out header.
    let disable = headers
        .get("x-disable-tracking")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| !v.is_empty() && v != "0");
    let bot = headers
        .get("x-bot")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("true"));
    if disable || bot {
        return (StatusCode::OK, Json(json!({ "tracked": false }))).into_response();
    }
    let website = match bound_website(&state, &headers).await {
        Ok(w) => w,
        Err(resp) => return resp,
    };
    let ip = visitor_ip(&headers, connect_info.map(|c| c.0), state.trusted_proxy);
    let ua = user_agent(&headers);
    let session = crate::application::service::visitor_service::SessionFacts {
        ip: &ip,
        user_agent: &ua,
        session_id: &body.session_id,
        country_code: None,
    };
    let page_id = match body.page_key.as_deref() {
        Some(key) => match crate::application::service::specificity::resolve_specific(
            state.pages.pool(),
            key,
            website.id,
        )
        .await
        {
            Ok(resolution) => resolution.row().map(|r| r.id),
            Err(e) => return website_error_response(e),
        },
        None => None,
    };
    match state
        .visitors
        .heartbeat(website.id, &session, body.url.as_deref(), page_id)
        .await
    {
        Ok(outcome) => (
            StatusCode::OK,
            Json(json!({
                "tracked": true,
                "visitor_id": outcome.visitor_id,
                "access_token": outcome.access_token,
                "kind": outcome.kind,
            })),
        )
            .into_response(),
        Err(e) => website_error_response(e),
    }
}

async fn public_intake(
    State(state): State<WebsitePublicState>,
    connect_info: Option<ConnectInfo<std::net::SocketAddr>>,
    headers: HeaderMap,
    Path(verb): Path<String>,
    body: axum::body::Bytes,
) -> Response {
    let website = match bound_website(&state, &headers).await {
        Ok(w) => w,
        Err(resp) => return resp,
    };

    // A fixed closed set of DECLARATION names — never a model name.
    if verb != ContactIntake::NAME {
        return website_error_response(WebsiteError::NotFound(format!(
            "no intake verb {verb:?}"
        )));
    }

    // Parse through the typed allowlist: unknown keys are a LOUD 422,
    // not a silent drop (deny_unknown_fields at the edge).
    let payload: ContactPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            let code = if e.to_string().contains("unknown field") {
                WebsiteError::IntakeFieldRejected
            } else {
                WebsiteError::IntakeValidationFailed("payload is not valid JSON for this verb".into())
            };
            return website_error_response(code);
        }
    };

    let ip = visitor_ip(&headers, connect_info.map(|c| c.0), state.trusted_proxy);
    let ua = user_agent(&headers);
    let turnstile_token = headers
        .get("x-turnstile-token")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let ctx = IntakeContext {
        client_ip: &ip,
        session_id: None,
        user_agent: &ua,
        turnstile_token: turnstile_token.as_deref(),
    };

    match state.intake.execute_intake::<ContactIntake>(&website, payload, &ctx).await {
        Ok(receipt) => (
            StatusCode::CREATED,
            Json(json!({
                "received": true,
                "subject_id": receipt.subject_id,
                "notified": receipt.notified,
            })),
        )
            .into_response(),
        Err(e) => website_error_response(e),
    }
}

/// The exported public router — exactly the §7.2 allowlist; nothing
/// else answers unauthenticated.
pub fn website_public_routes(state: WebsitePublicState) -> Router {
    Router::new()
        .route("/public/pages/*url", get(public_page))
        .route("/public/menus", get(public_menus))
        .route("/public/sitemap", get(public_sitemap))
        .route("/public/robots.txt", get(public_robots))
        .route("/public/resolve", get(public_resolve))
        .route("/public/visitors/heartbeat", post(visitor_heartbeat))
        .route("/public/intake/:verb", post(public_intake))
        .with_state(state)
}
