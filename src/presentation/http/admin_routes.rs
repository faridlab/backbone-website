//! The website module's ADMIN route surface (hand-written; user-owned;
//! see `metaphor.codegen.yaml`).
//!
//! The module DOES NOT SELF-MOUNT and DOES NOT SELF-GATE: it exports
//! [`website_admin_routes`], a plain `axum::Router` the composing host
//! nests under the schema name BEHIND `company_auth`, with
//! `ModuleWriteGate::new(pool, "website")` as the INNERMOST
//! `route_layer` (the foundation-ext pattern verbatim: the write gate
//! innermost, company_auth outside). Authority names resolve through
//! the host gate: `write:website` (POST/PUT/PATCH),
//! `delete:website` (DELETE), `admin:website` / `ADMIN` / `*:*`
//! supersets.
//!
//! The acting OFFICER id arrives through the [`WebsiteActor`]
//! request extension (the host's company_auth bridge inserts it);
//! without it the verbs run as the system actor — never a public
//! principal.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::Extensions,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::application::service::{
    menu_service::{CreateMenuInput, MenuAdminService, MenuPatch},
    page_service::{CreatePageInput, PageAdminService, PagePatch, PUBLISH_FENCED_FIELDS},
    redirect_service::{CreateRedirectInput, RedirectAdminService, RedirectPatch},
    visitor_gc::{sweep_partnerless_visitors, DEFAULT_GC_BATCH, DEFAULT_RETENTION_DAYS},
    visitor_service::VisitorEngine,
    website_error::WebsiteError,
    website_service::{
        ActorRef, CreateWebsiteInput, WebsiteConfigPatch, WebsiteRootService,
    },
};

/// The request extension carrying the acting officer id (the host's
/// company_auth bridge inserts it after authentication).
#[derive(Debug, Clone, Copy)]
pub struct WebsiteActor(pub Uuid);

/// The module's admin state — the hand services over one pool.
#[derive(Clone)]
pub struct WebsiteAdminState {
    pub websites: Arc<WebsiteRootService>,
    pub pages: Arc<PageAdminService>,
    pub menus: Arc<MenuAdminService>,
    pub redirects: Arc<RedirectAdminService>,
    pub visitors: Arc<VisitorEngine>,
    pub pool: sqlx::PgPool,
}

impl WebsiteAdminState {
    pub fn new(pool: sqlx::PgPool, pepper: String) -> Self {
        Self {
            websites: Arc::new(WebsiteRootService::new(pool.clone())),
            pages: Arc::new(PageAdminService::new(pool.clone())),
            menus: Arc::new(MenuAdminService::new(pool.clone())),
            redirects: Arc::new(RedirectAdminService::new(pool.clone())),
            visitors: Arc::new(VisitorEngine::new(pool.clone(), pepper)),
            pool,
        }
    }

    pub fn from_env(pool: sqlx::PgPool) -> Self {
        let pepper = std::env::var("WEBSITE_VISITOR_PEPPER").unwrap_or_default();
        Self::new(pool, pepper)
    }
}

fn actor_of(extensions: &Extensions) -> ActorRef {
    match extensions.get::<WebsiteActor>() {
        Some(WebsiteActor(id)) => ActorRef::officer(*id),
        None => ActorRef::system(),
    }
}

fn err_response(err: WebsiteError) -> Response {
    use axum::http::StatusCode;
    let status =
        StatusCode::from_u16(err.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let body = match &err {
        WebsiteError::Db(e) => {
            tracing::error!(error = ?e, "website admin route internal error");
            json!({"error": "internal error", "code": err.code()})
        }
        WebsiteError::Internal(msg) => {
            tracing::error!(reason = %msg, "website admin route internal error");
            json!({"error": "internal error", "code": err.code()})
        }
        other => json!({"error": other.to_string(), "code": other.code()}),
    };
    (status, Json(body)).into_response()
}

// ── request DTOs (unknown keys dropped at the officer edge) ─────────────────

#[derive(Debug, Deserialize)]
struct WebsiteListQuery {
    company_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
struct CreateWebsiteBody {
    name: String,
    #[serde(default)]
    domain: Option<String>,
    company_id: Uuid,
    #[serde(default)]
    public_user_id: Option<Uuid>,
    #[serde(default)]
    default_lang_code: Option<String>,
    #[serde(default)]
    homepage_url: Option<String>,
    #[serde(default)]
    robots_txt: Option<String>,
    #[serde(default)]
    social_links: Option<serde_json::Value>,
    #[serde(default)]
    contact_recipients: Option<Vec<String>>,
    #[serde(default)]
    sequence: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct PageListQuery {
    website_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct CreatePageBody {
    key: String,
    website_id: Option<Uuid>,
    url: String,
    title: String,
    #[serde(default)]
    seo_name: Option<String>,
    #[serde(default)]
    visibility: Option<String>,
    #[serde(default)]
    required_member_roles: Option<Vec<String>>,
    #[serde(default)]
    website_indexed: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ForkBody {
    key: String,
    target_website_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct FanoutDeleteBody {
    key: String,
}

#[derive(Debug, Deserialize)]
struct RenameBody {
    url: String,
    #[serde(default)]
    create_redirect: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct MenuListQuery {
    website_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct CreateMenuBody {
    website_id: Uuid,
    #[serde(default)]
    parent_id: Option<Uuid>,
    name: String,
    #[serde(default)]
    page_id: Option<Uuid>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    new_window: Option<bool>,
    #[serde(default)]
    sequence: Option<i32>,
    #[serde(default)]
    visibility: Option<String>,
    #[serde(default)]
    required_member_roles: Option<Vec<String>>,
    #[serde(default)]
    is_mega_menu: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RedirectListQuery {
    website_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct CreateRedirectBody {
    website_id: Uuid,
    url_from: String,
    redirect_type: String,
    #[serde(default)]
    url_to: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VisitorListQuery {
    website_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct MergeVisitorsBody {
    website_id: Uuid,
    anonymous_visitor_id: Uuid,
    portal_user_id: Uuid,
}

#[derive(Debug, Deserialize)]
struct ContactMessageListQuery {
    website_id: Uuid,
}

// ── website handlers ────────────────────────────────────────────────────────

async fn list_websites(
    State(state): State<WebsiteAdminState>,
    Query(q): Query<WebsiteListQuery>,
) -> Response {
    match state.websites.list_websites(q.company_id).await {
        Ok(rows) => (
            axum::http::StatusCode::OK,
            Json(json!({
                "websites": rows.iter().map(admin_website_json).collect::<Vec<_>>(),
            })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn create_website(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Json(body): Json<CreateWebsiteBody>,
) -> Response {
    let actor = actor_of(&extensions);
    let input = CreateWebsiteInput {
        name: body.name,
        domain: body.domain,
        company_id: body.company_id,
        public_user_id: body.public_user_id,
        default_lang_code: body.default_lang_code.unwrap_or_else(|| "en".into()),
        homepage_url: body.homepage_url.unwrap_or_else(|| "/".into()),
        robots_txt: body.robots_txt,
        social_links: body.social_links,
        contact_recipients: body.contact_recipients.unwrap_or_default(),
        sequence: body.sequence.unwrap_or(10),
    };
    match state.websites.create_website(actor, input).await {
        Ok(_) => (
            axum::http::StatusCode::CREATED,
            Json(json!({ "created": true })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn get_website(
    State(state): State<WebsiteAdminState>,
    Path(id): Path<Uuid>,
) -> Response {
    match state.websites.website_by_id(id).await {
        Ok(w) => (axum::http::StatusCode::OK, Json(serde_json::to_value(&admin_website_json(&w)).unwrap_or_default())).into_response(),
        Err(e) => err_response(e),
    }
}

fn admin_website_json(w: &crate::application::service::website_service::WebsiteView) -> serde_json::Value {
    json!({
        "id": w.id,
        "name": w.name,
        "domain": w.domain,
        "company_id": w.company_id,
        "public_user_id": w.public_user_id,
        "default_lang_code": w.default_lang_code,
        "homepage_url": w.homepage_url,
        "robots_txt": w.robots_txt,
        "social_links": w.social_links,
        "contact_recipients": w.contact_recipients,
        "sequence": w.sequence,
    })
}

/// The config-fields patch. `public_user_id` and `company_id` are NOT
/// config fields: a body carrying either refuses the typed
/// `website_field_not_patchable` BEFORE anything is read.
async fn patch_website(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Path(id): Path<Uuid>,
    body: axum::body::Bytes,
) -> Response {
    let actor = actor_of(&extensions);
    let raw: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return err_response(WebsiteError::InvalidInput(format!("invalid JSON body: {e}"))),
    };
    for forbidden in ["public_user_id", "company_id"] {
        if raw.get(forbidden).is_some() {
            return err_response(WebsiteError::FieldNotPatchable {
                field: "public_user_id/company_id",
                verb: "website create (these columns are identity, not config)",
            });
        }
    }
    let patch = WebsiteConfigPatch {
        name: raw.get("name").and_then(|v| v.as_str()).map(str::to_string),
        domain: raw.get("domain").and_then(|v| v.as_str()).map(str::to_string),
        default_lang_code: raw
            .get("default_lang_code")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        homepage_url: raw
            .get("homepage_url")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        robots_txt: raw.get("robots_txt").and_then(|v| v.as_str()).map(str::to_string),
        social_links: raw.get("social_links").filter(|v| !v.is_null()).cloned(),
        contact_recipients: raw
            .get("contact_recipients")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()),
        sequence: raw.get("sequence").and_then(|v| v.as_i64()).map(|v| v as i32),
    };
    match state.websites.patch_config(actor, id, patch).await {
        Ok(w) => (
            axum::http::StatusCode::OK,
            Json(admin_website_json(&w)),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn delete_website(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Path(id): Path<Uuid>,
) -> Response {
    let actor = actor_of(&extensions);
    match state.websites.delete_website(actor, id).await {
        Ok(()) => (axum::http::StatusCode::NO_CONTENT).into_response(),
        Err(e) => err_response(e),
    }
}

// ── page handlers ───────────────────────────────────────────────────────────

async fn list_pages(
    State(state): State<WebsiteAdminState>,
    Query(q): Query<PageListQuery>,
) -> Response {
    match state.pages.list_pages(q.website_id).await {
        Ok(rows) => (
            axum::http::StatusCode::OK,
            Json(json!({
                "pages": rows.iter().map(|p| json!({
                    "id": p.id, "key": p.key, "website_id": p.website_id,
                    "url": p.url, "title": p.title, "is_published": p.is_published,
                    "date_publish": p.date_publish,
                    "visibility": p.visibility.to_string(),
                    "website_indexed": p.website_indexed,
                    "forked_from": p.forked_from,
                })).collect::<Vec<_>>(),
            })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn create_page(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Json(body): Json<CreatePageBody>,
) -> Response {
    let actor = actor_of(&extensions);
    let input = CreatePageInput {
        key: body.key,
        website_id: body.website_id,
        url: body.url,
        title: body.title,
        seo_name: body.seo_name,
        visibility: body.visibility.unwrap_or_else(|| "public".into()),
        required_member_roles: body.required_member_roles.unwrap_or_default(),
        website_indexed: body.website_indexed.unwrap_or(true),
    };
    match state.pages.create_page(actor, input).await {
        Ok(p) => (
            axum::http::StatusCode::CREATED,
            Json(json!({ "id": p.id, "key": p.key, "url": p.url })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

/// The typed page patch — the publish-fenced fields refuse loudly with
/// the verb to use, and the refusal is audited (`publish_refused`).
async fn patch_page(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Path(id): Path<Uuid>,
    body: axum::body::Bytes,
) -> Response {
    let actor = actor_of(&extensions);
    let raw: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return err_response(WebsiteError::InvalidInput(format!("invalid JSON body: {e}"))),
    };
    for field in PUBLISH_FENCED_FIELDS {
        if raw.get(*field).is_some() {
            let refusal = WebsiteError::FieldNotPatchable { field, verb: "publish/unpublish" };
            let _ = crate::application::service::website_service::record_audit(
                &state.pool,
                "publish_refused",
                actor,
                Some("page"),
                Some(id),
                Some(json!({ "attempted_field": field, "via": "generic_patch" })),
            )
            .await;
            return err_response(refusal);
        }
    }
    let patch = PagePatch {
        url: raw.get("url").and_then(|v| v.as_str()).map(str::to_string),
        title: raw.get("title").and_then(|v| v.as_str()).map(str::to_string),
        seo_name: raw.get("seo_name").and_then(|v| v.as_str()).map(str::to_string),
        visibility: raw.get("visibility").and_then(|v| v.as_str()).map(str::to_string),
        required_member_roles: raw
            .get("required_member_roles")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()),
        website_indexed: raw.get("website_indexed").and_then(|v| v.as_bool()),
    };
    match state.pages.patch_page(actor, id, patch).await {
        Ok(p) => (
            axum::http::StatusCode::OK,
            Json(json!({ "id": p.id, "url": p.url, "is_published": p.is_published })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn publish_page(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Path(id): Path<Uuid>,
) -> Response {
    let actor = actor_of(&extensions);
    match state.pages.publish(actor, id).await {
        Ok(p) => (
            axum::http::StatusCode::OK,
            Json(json!({ "id": p.id, "is_published": true, "date_publish": p.date_publish })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn unpublish_page(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Path(id): Path<Uuid>,
) -> Response {
    let actor = actor_of(&extensions);
    match state.pages.unpublish(actor, id).await {
        Ok(p) => (
            axum::http::StatusCode::OK,
            Json(json!({ "id": p.id, "is_published": false })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn fork_page(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Json(body): Json<ForkBody>,
) -> Response {
    let actor = actor_of(&extensions);
    match crate::application::service::versioning_service::fork_to_website(
        &state.pool,
        actor,
        &body.key,
        body.target_website_id,
    )
    .await
    {
        Ok(outcome) => {
            let row = outcome.row();
            (
                axum::http::StatusCode::CREATED,
                Json(json!({
                    "outcome": match outcome {
                        crate::application::service::versioning_service::ForkOutcome::Created(_) => "created",
                        crate::application::service::versioning_service::ForkOutcome::Existing(_) => "existing",
                    },
                    "id": row.id, "key": row.key, "website_id": row.website_id,
                })),
            )
                .into_response()
        }
        Err(e) => err_response(e),
    }
}

async fn fanout_delete_page(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Json(body): Json<FanoutDeleteBody>,
) -> Response {
    let actor = actor_of(&extensions);
    match crate::application::service::versioning_service::delete_generic_with_fanout(
        &state.pool,
        actor,
        &body.key,
    )
    .await
    {
        Ok(summary) => (
            axum::http::StatusCode::OK,
            Json(json!({
                "deleted_generic": summary.generic_id,
                "created_specifics": summary.created,
                "websites_touched": summary.websites_touched,
            })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn rename_page(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Path(id): Path<Uuid>,
    Json(body): Json<RenameBody>,
) -> Response {
    let actor = actor_of(&extensions);
    match state.pages.rename_page(actor, id, body.url, body.create_redirect).await {
        Ok(p) => (
            axum::http::StatusCode::OK,
            Json(json!({ "id": p.id, "url": p.url })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn delete_page(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Path(id): Path<Uuid>,
) -> Response {
    let actor = actor_of(&extensions);
    match state.pages.delete_specific(actor, id).await {
        Ok(()) => (axum::http::StatusCode::NO_CONTENT).into_response(),
        Err(e) => err_response(e),
    }
}

async fn page_blocks(
    State(state): State<WebsiteAdminState>,
    Path(page_id): Path<Uuid>,
) -> Response {
    match state.pages.page_blocks(page_id).await {
        Ok(blocks) => (axum::http::StatusCode::OK, Json(json!({ "blocks": blocks })))
            .into_response(),
        Err(e) => err_response(e),
    }
}

// ── menu handlers ───────────────────────────────────────────────────────────

async fn list_menus(
    State(state): State<WebsiteAdminState>,
    Query(q): Query<MenuListQuery>,
) -> Response {
    match state.menus.tree_admin(q.website_id).await {
        Ok(tree) => (axum::http::StatusCode::OK, Json(json!({ "menus": tree })))
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn create_menu(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Json(body): Json<CreateMenuBody>,
) -> Response {
    let actor = actor_of(&extensions);
    let input = CreateMenuInput {
        website_id: body.website_id,
        parent_id: body.parent_id,
        name: body.name,
        page_id: body.page_id,
        url: body.url,
        new_window: body.new_window.unwrap_or(false),
        sequence: body.sequence.unwrap_or(10),
        visibility: body.visibility.unwrap_or_else(|| "public".into()),
        required_member_roles: body.required_member_roles.unwrap_or_default(),
        is_mega_menu: body.is_mega_menu.unwrap_or(false),
    };
    match state.menus.create_menu(actor, input).await {
        Ok(m) => (
            axum::http::StatusCode::CREATED,
            Json(json!({ "id": m.id, "name": m.name, "parent_id": m.parent_id })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn fanout_menu(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Path(id): Path<Uuid>,
) -> Response {
    let actor = actor_of(&extensions);
    match state.menus.fanout_menu(actor, id).await {
        Ok(created) => (
            axum::http::StatusCode::CREATED,
            Json(json!({ "created": created })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn patch_menu(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Path(id): Path<Uuid>,
    body: axum::body::Bytes,
) -> Response {
    let actor = actor_of(&extensions);
    let raw: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return err_response(WebsiteError::InvalidInput(format!("invalid JSON body: {e}"))),
    };
    let patch = MenuPatch {
        parent_id: raw.get("parent_id").map(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok())),
        name: raw.get("name").and_then(|v| v.as_str()).map(str::to_string),
        page_id: raw.get("page_id").map(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok())),
        url: raw.get("url").map(|v| match v {
            serde_json::Value::Null => None,
            v => v.as_str().map(str::to_string),
        }),
        new_window: raw.get("new_window").and_then(|v| v.as_bool()),
        sequence: raw.get("sequence").and_then(|v| v.as_i64()).map(|v| v as i32),
        visibility: raw.get("visibility").and_then(|v| v.as_str()).map(str::to_string),
        required_member_roles: raw
            .get("required_member_roles")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()),
        is_mega_menu: raw.get("is_mega_menu").and_then(|v| v.as_bool()),
    };
    match state.menus.patch_menu(actor, id, patch).await {
        Ok(m) => (
            axum::http::StatusCode::OK,
            Json(json!({ "id": m.id, "name": m.name })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn delete_menu(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Path(id): Path<Uuid>,
) -> Response {
    let actor = actor_of(&extensions);
    match state.menus.delete_menu(actor, id).await {
        Ok(()) => (axum::http::StatusCode::NO_CONTENT).into_response(),
        Err(e) => err_response(e),
    }
}

// ── redirect handlers ───────────────────────────────────────────────────────

async fn list_redirects(
    State(state): State<WebsiteAdminState>,
    Query(q): Query<RedirectListQuery>,
) -> Response {
    match state.redirects.list(q.website_id).await {
        Ok(rows) => (
            axum::http::StatusCode::OK,
            Json(json!({
                "redirects": rows.iter().map(|r| json!({
                    "id": r.id, "url_from": r.url_from,
                    "redirect_type": r.redirect_type.to_string(),
                    "url_to": r.url_to, "description": r.description,
                })).collect::<Vec<_>>(),
            })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn create_redirect(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Json(body): Json<CreateRedirectBody>,
) -> Response {
    let actor = actor_of(&extensions);
    let input = CreateRedirectInput {
        website_id: body.website_id,
        url_from: body.url_from,
        redirect_type: body.redirect_type,
        url_to: body.url_to,
        description: body.description,
    };
    match state.redirects.create(actor, input).await {
        Ok(r) => (
            axum::http::StatusCode::CREATED,
            Json(json!({ "id": r.id, "url_from": r.url_from })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn patch_redirect(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Path(id): Path<Uuid>,
    body: axum::body::Bytes,
) -> Response {
    let actor = actor_of(&extensions);
    let raw: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return err_response(WebsiteError::InvalidInput(format!("invalid JSON body: {e}"))),
    };
    let patch = RedirectPatch {
        url_from: raw.get("url_from").and_then(|v| v.as_str()).map(str::to_string),
        redirect_type: raw.get("redirect_type").and_then(|v| v.as_str()).map(str::to_string),
        url_to: raw
            .get("url_to")
            .map(|v| v.as_str().map(str::to_string)),
        description: raw
            .get("description")
            .map(|v| v.as_str().map(str::to_string)),
    };
    match state.redirects.patch(actor, id, patch).await {
        Ok(r) => (
            axum::http::StatusCode::OK,
            Json(json!({ "id": r.id, "url_from": r.url_from })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

async fn delete_redirect(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Path(id): Path<Uuid>,
) -> Response {
    let actor = actor_of(&extensions);
    match state.redirects.delete(actor, id).await {
        Ok(()) => (axum::http::StatusCode::NO_CONTENT).into_response(),
        Err(e) => err_response(e),
    }
}

// ── visitor handlers ────────────────────────────────────────────────────────

async fn list_visitors(
    State(state): State<WebsiteAdminState>,
    Query(q): Query<VisitorListQuery>,
) -> Response {
    let (visitors, connected) = match (
        state.visitors.list(q.website_id).await,
        state.visitors.connected_count(q.website_id).await,
    ) {
        (Ok(v), Ok(c)) => (v, c),
        (Err(e), _) | (_, Err(e)) => return err_response(e),
    };
    (
        axum::http::StatusCode::OK,
        Json(json!({ "visitors": visitors, "connected_now": connected })),
    )
        .into_response()
}

async fn merge_visitors(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
    Json(body): Json<MergeVisitorsBody>,
) -> Response {
    let actor = actor_of(&extensions);
    match state
        .visitors
        .merge_visitor(actor, body.website_id, body.anonymous_visitor_id, body.portal_user_id)
        .await
    {
        Ok(v) => (
            axum::http::StatusCode::OK,
            Json(json!({ "visitor_id": v.id, "kind": v.kind, "portal_user_id": v.portal_user_id })),
        )
            .into_response(),
        Err(e) => err_response(e),
    }
}

/// Manual GC trigger — the SAME verb the cron runs.
async fn sweep_visitors(
    State(state): State<WebsiteAdminState>,
    extensions: Extensions,
) -> Response {
    let actor = actor_of(&extensions);
    let retention = std::env::var("WEBSITE_VISITOR_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_RETENTION_DAYS);
    let batch = std::env::var("WEBSITE_VISITOR_GC_BATCH")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_GC_BATCH);
    match sweep_partnerless_visitors(&state.pool, retention, batch).await {
        Ok(summary) => {
            let _ = crate::application::service::website_service::record_audit(
                &state.pool,
                "visitor_gc_swept",
                actor,
                Some("visitor"),
                None,
                Some(json!({ "swept": summary.swept, "batches": summary.batches, "origin": "manual" })),
            )
            .await;
            (axum::http::StatusCode::OK, Json(json!({ "swept": summary.swept, "batches": summary.batches })))
                .into_response()
        }
        Err(e) => err_response(e),
    }
}

// ── intake read handler ─────────────────────────────────────────────────────

async fn list_contact_messages(
    State(state): State<WebsiteAdminState>,
    Query(q): Query<ContactMessageListQuery>,
) -> Response {
    let rows: Vec<(Uuid, Option<String>, String, String, bool, chrono::DateTime<chrono::Utc>)> =
        match sqlx::query_as(
            r#"
            SELECT id, name, email, message, notified,
                   ((metadata->>'created_at')::timestamptz)
            FROM website.contact_messages
            WHERE website_id = $1
            ORDER BY (metadata->>'created_at') DESC
            LIMIT 200
            "#,
        )
        .bind(q.website_id)
        .fetch_all(&state.pool)
        .await
        {
            Ok(r) => r,
            Err(e) => return err_response(WebsiteError::Db(e)),
        };
    (
        axum::http::StatusCode::OK,
        Json(json!({
            "messages": rows.iter().map(|m| json!({
                "id": m.0, "name": m.1, "email": m.2,
                "message": m.3, "notified": m.4, "received_at": m.5,
            })).collect::<Vec<_>>(),
        })),
    )
        .into_response()
}

/// The exported admin router — the §9.2 table, exhaustive. The host
/// nests it BEHIND `company_auth` with the module write gate as the
/// INNERMOST route_layer.
pub fn website_admin_routes(state: WebsiteAdminState) -> Router {
    Router::new()
        // websites
        .route("/admin/websites", get(list_websites).post(create_website))
        .route("/admin/websites/:id", get(get_website).patch(patch_website).delete(delete_website))
        // pages
        .route("/admin/pages", get(list_pages).post(create_page))
        .route("/admin/pages/fork", axum::routing::post(fork_page))
        .route("/admin/pages/fanout-delete", axum::routing::post(fanout_delete_page))
        .route("/admin/pages/:id", axum::routing::patch(patch_page).delete(delete_page))
        .route("/admin/pages/:id/publish", axum::routing::post(publish_page))
        .route("/admin/pages/:id/unpublish", axum::routing::post(unpublish_page))
        .route("/admin/pages/:id/rename", axum::routing::post(rename_page))
        // blocks ride page DTOs; the single declared read:
        .route("/admin/blocks/page/:page_id", get(page_blocks))
        // menus
        .route("/admin/menus", get(list_menus).post(create_menu))
        .route("/admin/menus/:id", axum::routing::patch(patch_menu).delete(delete_menu))
        .route("/admin/menus/:id/fanout", axum::routing::post(fanout_menu))
        // redirects
        .route("/admin/redirects", get(list_redirects).post(create_redirect))
        .route("/admin/redirects/:id", axum::routing::patch(patch_redirect).delete(delete_redirect))
        // visitors
        .route("/admin/visitors", get(list_visitors))
        .route("/admin/visitors/merge", axum::routing::post(merge_visitors))
        .route("/admin/visitors/sweep", axum::routing::post(sweep_visitors))
        // intake reads
        .route("/admin/intake/contact-messages", get(list_contact_messages))
        .with_state(state)
}
