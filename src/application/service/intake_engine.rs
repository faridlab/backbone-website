//! The intake engine (hand-written; user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! Intake verbs are DECLARED per target in Rust, never a
//! model-name-in-URL route: each declaration owns a fixed route
//! segment, a typed `Payload` allowlist (`deny_unknown_fields` makes
//! unknown keys unparseable at the edge — a 422, not a silent drop),
//! its turnstile requirement, and its Tier B limits. The executor
//! below runs the frozen order:
//!
//! 1. (website resolution happens in the route — hostname binding)
//! 2. the captcha verifier (turnstile default, the recaptcha sibling
//!    selectable via `WEBSITE_CAPTCHA_PROVIDER` — §6.3), FAIL-CLOSED,
//!    unset ≠ misconfigured ≠ refused ≠ unreachable — four typed
//!    answers, no passthrough on any path;
//! 3. Tier B fixed-window rate buckets, per-identity AND per-IP, both
//!    always armed (identity key = the visitor digest when a session
//!    is presented, else the connecting IP);
//! 4. the persist runs inside `SAVEPOINT intake_<name>` — a rejection
//!    rolls back to the savepoint and answers a loud typed 4xx
//!    (upstream's silent `json False` on IntegrityError is dead);
//! 5. the plain app role executes the write — no sudo anywhere, and
//!    the typed payload struct admits no identity column;
//! 6. NO anonymous mail: fixed-recipient notification rides the
//!    host-installed notifier port; unwired → WARN + `notified=false`,
//!    the write never refused by the port.

use std::collections::HashMap;
use std::sync::Mutex;

use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use super::notifier_port::IntakeNotifier;
use super::website_error::WebsiteError;
use super::website_service::{record_audit, ActorRef, WebsiteView};

/// One declared intake verb. Downstream consumers (blog comments, lead
/// capture, event registration funnels) declare their own
/// `IntakeDeclaration` in their own crates and mount through
/// `execute_intake` — never a second engine.
pub trait IntakeDeclaration: Send + Sync {
    /// The fixed route segment (`POST /public/intake/{NAME}`).
    const NAME: &'static str;
    /// Whether this verb requires a turnstile token.
    const REQUIRES_TURNSTILE: bool;
    /// Per-identity Tier B limit (fixed 1-hour window).
    const IDENTITY_LIMIT: u32;
    /// Per-IP Tier B limit (fixed 1-hour window).
    const IP_LIMIT: u32;
    /// The typed field allowlist. `deny_unknown_fields` at the parse
    /// edge is the whole point: there is no column list anywhere to
    /// drift, and an unknown key is unparseable.
    type Payload: serde::de::DeserializeOwned + Send;
    /// Typed validation (lengths, formats) — a refusal here is the
    /// loud 422 `website_intake_validation_failed`.
    fn validate(payload: &Self::Payload) -> impl std::future::Future<Output = Result<(), WebsiteError>> + Send;
    /// Persist inside the per-verb savepoint. A refusal here is the
    /// declaration-specific loud 422 `website_intake_refused`.
    fn persist(
        tx: &mut PgConnection,
        website_id: Uuid,
        payload: Self::Payload,
    ) -> impl std::future::Future<Output = Result<IntakeOutcome, WebsiteError>> + Send;
}

/// What a successful persist produced.
#[derive(Debug, Clone)]
pub struct IntakeOutcome {
    /// The persisted row's id, when the declaration has one.
    pub subject_id: Option<Uuid>,
    /// Free-form detail for the audit trail.
    pub detail: Option<serde_json::Value>,
    /// The fixed-recipient notification payload. The shipped port is
    /// contact-shaped (the reference declaration); a downstream
    /// declaration that ships its own notifier port leaves this None.
    pub contact: Option<super::intake_contact::ContactMessageView>,
}

/// The request context the executor rates on.
#[derive(Debug, Clone)]
pub struct IntakeContext<'a> {
    /// The caller's address, already resolved by the HTTP layer's
    /// trusted-proxy posture (rightmost forwarded hop when a trusted
    /// proxy is declared, else the socket address — see
    /// `public_routes::visitor_ip`). Rate shaping and digests only;
    /// never an authorization input.
    pub client_ip: &'a str,
    /// The webapp session id when one is presented — the identity key
    /// becomes the visitor digest.
    pub session_id: Option<&'a str>,
    pub user_agent: &'a str,
    /// The turnstile token (required when the declaration requires
    /// turnstile).
    pub turnstile_token: Option<&'a str>,
}

/// Turnstile knobs: the secret (`WEBSITE_TURNSTILE_SECRET`, unset ≠
/// misconfigured) and the siteverify URL
/// (`WEBSITE_TURNSTILE_VERIFY_URL`, probe-overridable).
#[derive(Debug, Clone)]
pub struct TurnstileConfig {
    pub secret: Option<String>,
    pub verify_url: String,
}

impl TurnstileConfig {
    pub fn from_env() -> Self {
        let secret = std::env::var("WEBSITE_TURNSTILE_SECRET")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let verify_url = std::env::var("WEBSITE_TURNSTILE_VERIFY_URL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "https://challenges.cloudflare.com/turnstile/v0/siteverify".to_string());
        Self { secret, verify_url }
    }
}

/// The savepoint identifier for a declaration name: a savepoint name
/// is a SQL identifier, so anything outside `[A-Za-z0-9_]` folds to
/// `_` before interpolation (declaration names are constants, but the
/// engine never formats a raw label into an identifier position).
fn savepoint_ident(name: &str) -> String {
    let folded: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    format!("intake_{folded}")
}

/// The turnstile verifier — the module's ONLY outbound HTTP family
/// (one call shape; the recaptcha sibling in `captcha_recaptcha.rs`
/// shares it — §6.3).
#[derive(Debug, Clone)]
pub struct TurnstileClient {
    config: TurnstileConfig,
    http: reqwest::Client,
}

/// The verifier's parsed answer.
#[derive(Debug, serde::Deserialize)]
struct SiteverifyBody {
    success: bool,
    #[serde(default, rename = "error-codes")]
    error_codes: Vec<String>,
}

impl TurnstileClient {
    pub fn new(config: TurnstileConfig) -> Self {
        Self { config, http: reqwest::Client::new() }
    }

    pub fn config(&self) -> &TurnstileConfig {
        &self.config
    }

    /// FAIL-CLOSED verify: the four typed answers, never a
    /// passthrough. Secret-rejection codes (`missing-input-secret`,
    /// `invalid-input-secret`, `bad-secret`) are the HOST's
    /// misconfiguration, distinguishable from a bad token and from
    /// transport failure.
    pub async fn verify(&self, token: &str) -> Result<(), WebsiteError> {
        let Some(secret) = self.config.secret.clone() else {
            return Err(WebsiteError::TurnstileNotConfigured);
        };
        if token.is_empty() {
            return Err(WebsiteError::TurnstileRefused);
        }
        let resp = self
            .http
            .post(&self.config.verify_url)
            .form(&[("secret", secret.as_str()), ("response", token)])
            .send()
            .await
            .map_err(|_| WebsiteError::TurnstileUnreachable)?;
        let body: SiteverifyBody = resp
            .json()
            .await
            .map_err(|_| WebsiteError::TurnstileUnreachable)?;
        if body.success {
            return Ok(());
        }
        const SECRET_FAULTS: &[&str] =
            &["missing-input-secret", "invalid-input-secret", "bad-secret"];
        if body.error_codes.iter().any(|c| SECRET_FAULTS.contains(&c.as_str())) {
            return Err(WebsiteError::TurnstileMisconfigured);
        }
        Err(WebsiteError::TurnstileRefused)
    }
}

/// Per-declaration Tier B policy (module consts).
#[derive(Debug, Clone, Copy)]
pub struct IntakeRatePolicy {
    pub identity_limit: u32,
    pub ip_limit: u32,
}

impl IntakeRatePolicy {
    pub fn of<D: IntakeDeclaration>() -> Self {
        Self { identity_limit: D::IDENTITY_LIMIT, ip_limit: D::IP_LIMIT }
    }
}

#[derive(Debug, Clone, Copy)]
struct WindowCount {
    hour: i64,
    count: u32,
}

/// The composing intake engine. In-memory Tier B books (the accepted
/// family trade: multi-instance hosts front a shared limiter).
pub struct IntakeEngine {
    pool: PgPool,
    captcha: super::captcha_recaptcha::CaptchaVerifier,
    pepper: String,
    notifier: std::sync::Arc<dyn IntakeNotifier>,
    books: Mutex<HashMap<String, WindowCount>>,
}

impl IntakeEngine {
    /// Compose over the DEFAULT provider (turnstile) — the original
    /// signature, kept for every existing composer; hosts selecting a
    /// provider via env use [`Self::with_verifier`] +
    /// [`super::captcha_recaptcha::CaptchaVerifier::from_env`].
    pub fn new(
        pool: PgPool,
        turnstile: TurnstileClient,
        pepper: String,
        notifier: std::sync::Arc<dyn IntakeNotifier>,
    ) -> Self {
        Self::with_verifier(
            pool,
            super::captcha_recaptcha::CaptchaVerifier::Turnstile(turnstile),
            pepper,
            notifier,
        )
    }

    /// Compose over the CONFIG-SELECTED verifier (§6.3): the turnstile
    /// default, the recaptcha sibling, or the fail-closed unknown arm.
    pub fn with_verifier(
        pool: PgPool,
        captcha: super::captcha_recaptcha::CaptchaVerifier,
        pepper: String,
        notifier: std::sync::Arc<dyn IntakeNotifier>,
    ) -> Self {
        Self { pool, captcha, pepper, notifier, books: Mutex::new(HashMap::new()) }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    fn current_hour() -> i64 {
        chrono::Utc::now().timestamp() / 3600
    }

    fn bump(book: &mut HashMap<String, WindowCount>, key: &str, limit: u32) -> Result<(), i64> {
        let hour = Self::current_hour();
        let entry = book.entry(key.to_string()).or_insert(WindowCount { hour, count: 0 });
        if entry.hour != hour {
            entry.hour = hour;
            entry.count = 0;
        }
        if entry.count >= limit {
            // Seconds until the window rolls over.
            let retry = (entry.hour + 1) * 3600 - chrono::Utc::now().timestamp();
            return Err(retry.max(1));
        }
        entry.count += 1;
        Ok(())
    }

    /// The Tier B gate: the IP window is ALWAYS armed; the identity
    /// window arms only when a session is presented (keyed by the
    /// visitor digest, computed under the same pepper construction as
    /// the heartbeat). An anonymous submission carries no identity to
    /// rate separately — arming the identity bucket on the IP key
    /// would double-count every request into one window and halve the
    /// declared limits; the IP window is its only arm.
    fn arm_rate_buckets(
        &self,
        declaration: &str,
        policy: IntakeRatePolicy,
        ctx: &IntakeContext<'_>,
    ) -> Result<(), WebsiteError> {
        let identity_key = match ctx.session_id {
            Some(session) => {
                let digest = super::visitor_service::visitor_digest(
                    &self.pepper,
                    ctx.client_ip,
                    ctx.user_agent,
                    session,
                )?;
                Some(format!("{declaration}:id:{digest}"))
            }
            None => None,
        };
        let ip_key = format!("{declaration}:ip:{}", ctx.client_ip);
        // A poisoned lock means a panic left the books mid-update; the
        // recovered data is still a valid (slightly stale) window book —
        // rate shaping fails CLOSED (keeps counting), never crashes the
        // intake verb.
        let mut books = self
            .books
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(key) = &identity_key {
            if let Err(retry) = Self::bump(&mut books, key, policy.identity_limit) {
                return Err(WebsiteError::IntakeRateLimited { retry_after_seconds: retry });
            }
        }
        if let Err(retry) = Self::bump(&mut books, &ip_key, policy.ip_limit) {
            return Err(WebsiteError::IntakeRateLimited { retry_after_seconds: retry });
        }
        Ok(())
    }

    /// The per-request executor, generic over the declaration.
    pub async fn execute_intake<D: IntakeDeclaration>(
        &self,
        website: &WebsiteView,
        payload: D::Payload,
        ctx: &IntakeContext<'_>,
    ) -> Result<IntakeReceipt, WebsiteError> {
        // 2. The captcha verifier (turnstile default, the recaptcha
        // sibling selectable), fail-closed.
        if D::REQUIRES_TURNSTILE {
            let token = ctx.turnstile_token.unwrap_or("");
            if let Err(e) = self.captcha.verify(token).await {
                record_audit(
                    &self.pool,
                    "intake_refused",
                    ActorRef::system(),
                    Some("intake"),
                    None,
                    Some(serde_json::json!({
                        "verb": D::NAME, "code": e.code(),
                    })),
                )
                .await?;
                return Err(e);
            }
        }

        // 3. Tier B books.
        if let Err(e) = self.arm_rate_buckets(D::NAME, IntakeRatePolicy::of::<D>(), ctx) {
            record_audit(
                &self.pool,
                "intake_refused",
                ActorRef::system(),
                Some("intake"),
                None,
                Some(serde_json::json!({ "verb": D::NAME, "code": e.code() })),
            )
            .await?;
            return Err(e);
        }

        // Typed validation.
        if let Err(e) = D::validate(&payload).await {
            record_audit(
                &self.pool,
                "intake_refused",
                ActorRef::system(),
                Some("intake"),
                None,
                Some(serde_json::json!({ "verb": D::NAME, "code": e.code() })),
            )
            .await?;
            return Err(e);
        }

        // 4. Per-verb savepoint; 5. the plain app role executes (no
        // sudo anywhere — the payload struct admits no identity field).
        let mut tx = self.pool.begin().await?;
        let savepoint = savepoint_ident(D::NAME);
        sqlx::query(&format!("SAVEPOINT {savepoint}"))
            .execute(&mut *tx)
            .await?;
        let outcome = match D::persist(&mut tx, website.id, payload).await {
            Ok(o) => o,
            Err(e) => {
                let _ = sqlx::query(&format!("ROLLBACK TO SAVEPOINT {savepoint}"))
                    .execute(&mut *tx)
                    .await;
                let _ = sqlx::query(&format!("RELEASE SAVEPOINT {savepoint}"))
                    .execute(&mut *tx)
                    .await;
                let _ = tx.rollback().await;
                record_audit(
                    &self.pool,
                    "intake_refused",
                    ActorRef::system(),
                    Some("intake"),
                    None,
                    Some(serde_json::json!({ "verb": D::NAME, "code": e.code() })),
                )
                .await?;
                return Err(e);
            }
        };
        sqlx::query(&format!("RELEASE SAVEPOINT {savepoint}"))
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;

        record_audit(
            &self.pool,
            "intake_received",
            ActorRef::system(),
            Some("intake"),
            outcome.subject_id,
            Some(serde_json::json!({ "verb": D::NAME, "detail": outcome.detail })),
        )
        .await?;

        // 6. Fixed-recipient notification through the port; the write
        // is already durable and the port never refuses it.
        let mut notified = false;
        if let Some(view) = &outcome.contact {
            self.notifier.notify_intake(website, view).await;
            if self.notifier.wired() {
                // A wired adapter has delivered (or queued) the officer
                // notification by the time notify_intake resolves.
                notified = true;
                if let Some(id) = outcome.subject_id {
                    let _ = sqlx::query(
                        "UPDATE website.contact_messages SET notified = TRUE WHERE id = $1",
                    )
                    .bind(id)
                    .execute(&self.pool)
                    .await;
                }
            }
        }

        Ok(IntakeReceipt {
            subject_id: outcome.subject_id,
            notified,
        })
    }
}

/// The executor's receipt.
#[derive(Debug, Clone)]
pub struct IntakeReceipt {
    pub subject_id: Option<Uuid>,
    pub notified: bool,
}
