//! The website module's typed error surface (hand-written; user-owned;
//! see `metaphor.codegen.yaml`).
//!
//! Two rules shape every variant:
//!
//! 1. **Loud refusals, never silent degradation.** Every refusal in this
//!    module is a TYPED failure with a stable machine code and an HTTP
//!    status — the intake path never answers a silent `false`, the
//!    publish fence never drops a field quietly, and an unconfigured
//!    secret or unwired port refuses loudly.
//! 2. **Existence is hidden on the public tree.** Unpublished and
//!    off-website page reads share ONE not-found shape (404) — a public
//!    caller cannot distinguish "no such page" from "not for you" except
//!    where the tier rules deliberately say 403 (published, restricted).

use thiserror::Error;

/// The module error enum.
#[derive(Debug, Error)]
pub enum WebsiteError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("internal error: {0}")]
    Internal(String),

    // ── request binding / website resolution ───────────────────────────────

    /// The request's hostname binds to no live website (BTF-1: no session
    /// force flag, no first-website fallback — the loud 404).
    #[error("no live website is bound to this hostname")]
    WebsiteNotResolved,

    // ── specificity / versioning (WS-1/WS-2/WS-3) ─────────────────────────

    /// Two live rows collided on the (key, website) specificity grain or
    /// the per-website url scope — the sentinel unique indexes refused a
    /// concurrent duplicate (mapped from 23505).
    #[error("a row already occupies this key or url on this website")]
    SpecificityConflict { constraint: String },

    /// `fork_to_website` found no live generic row for the key.
    #[error("no live generic page carries this key")]
    ForkSourceMissing,

    /// A plain DELETE landed on a GENERIC page row — the fanout verb is
    /// the only generic deletion.
    #[error("deleting a generic page requires the fanout verb")]
    GenericRequiresFanoutVerb,

    // ── publish fence (WS-6) ───────────────────────────────────────────────

    /// A generic patch carried `is_published` — the publish/unpublish
    /// verbs are the only writers.
    #[error("the {field} field moves only through its dedicated verb: {verb}")]
    FieldNotPatchable { field: &'static str, verb: &'static str },

    /// The caller lacks the module-write authority for the publish verbs
    /// (the route layer maps the gate refusal; this arm is the service
    /// record of it).
    #[error("publishing requires the website write authority")]
    PublishPermissionRequired,

    // ── visibility (WS-7) ──────────────────────────────────────────────────

    /// The page exists, is published, and is tier-gated above the caller
    /// (restricted/connected without a qualifying principal).
    #[error("this page's visibility tier refuses this caller")]
    PageVisibilityRefused,

    // ── menus (WS-15) ──────────────────────────────────────────────────────

    /// A menu tree write violated the depth ceiling (<= 2 below root).
    #[error("menu depth exceeds the ceiling of 2 below root")]
    MenuDepthExceeded,

    /// A mega-menu carry validation failed (must have no parent and no
    /// children).
    #[error("a mega-menu cannot carry a parent or children")]
    MegaMenuIsolated,

    // ── redirects (WS-12) ──────────────────────────────────────────────────

    /// An `alias_308` redirect's target does not carry the same
    /// query-parameter names as its source (the param-parity rule).
    #[error("a 308 alias target must carry the same query parameters as its source")]
    RedirectParamParity,

    /// A redirect row is missing its target (required unless gone_404).
    #[error("this redirect type requires a target url")]
    RedirectTargetRequired,

    // ── visitors (WS-10) ───────────────────────────────────────────────────

    /// The visitor digest pepper (`WEBSITE_VISITOR_PEPPER`) is not
    /// configured — the first visitor verb refuses loudly rather than
    /// digesting under an empty key.
    #[error("visitor digest pepper is not configured")]
    VisitorPepperNotConfigured,

    // ── intake (WS-11) ─────────────────────────────────────────────────────

    /// Cloudflare rejected the presented turnstile token.
    #[error("turnstile refused this token")]
    TurnstileRefused,

    /// The declaration requires turnstile but no secret is configured —
    /// UNSET, distinguishable from a bad secret.
    #[error("turnstile is required by this verb but not configured")]
    TurnstileNotConfigured,

    /// Cloudflare rejected the SECRET (invalid-input-secret) — the host
    /// misconfigured it; distinguishable from unset and from a bad token.
    #[error("turnstile rejected the configured secret")]
    TurnstileMisconfigured,

    /// Transport failure reaching the turnstile verifier — fail-closed,
    /// never a passthrough.
    #[error("turnstile verifier is unreachable")]
    TurnstileUnreachable,

    /// The captcha provider selector (`WEBSITE_CAPTCHA_PROVIDER`) names
    /// no known provider — the gate stays shut rather than silently
    /// falling back to a default provider (docs/spec.md §6.3).
    #[error("the captcha provider selector names no known provider")]
    CaptchaProviderUnknown,

    /// A Tier B intake rate bucket fired (per identity or per IP).
    #[error("intake rate limited; retry after {retry_after_seconds}s")]
    IntakeRateLimited { retry_after_seconds: i64 },

    /// An unknown field reached the intake payload parse (the typed
    /// allowlist refused it — deny_unknown_fields).
    #[error("intake payload carries an unknown field")]
    IntakeFieldRejected,

    /// Typed validation failed (lengths, formats) on an intake payload.
    #[error("intake validation failed: {0}")]
    IntakeValidationFailed(String),

    /// A declaration-specific persist refusal.
    #[error("intake refused: {0}")]
    IntakeRefused(String),

    // ── generic shapes ─────────────────────────────────────────────────────

    /// Input validation refusal (officer verbs).
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// A requested record does not exist (officer verbs).
    #[error("not found: {0}")]
    NotFound(String),

    /// A write was refused by a recorded guard (e.g. deleting the
    /// company's derived primary website).
    #[error("refused: {0}")]
    Guarded(String),

    /// The delete verb landed on the company's DERIVED primary website
    /// — demote it first (sequence bump) and retry.
    #[error("this website is its company's derived primary website")]
    WebsiteIsPrimaryForCompany,
}

impl WebsiteError {
    /// The HTTP status the route layer maps this error to.
    pub fn http_status(&self) -> u16 {
        match self {
            WebsiteError::Db(_) | WebsiteError::Internal(_) => 500,
            WebsiteError::WebsiteNotResolved => 404,
            WebsiteError::SpecificityConflict { .. } => 409,
            WebsiteError::ForkSourceMissing => 422,
            WebsiteError::GenericRequiresFanoutVerb => 422,
            WebsiteError::FieldNotPatchable { .. } => 422,
            WebsiteError::PublishPermissionRequired => 403,
            WebsiteError::PageVisibilityRefused => 403,
            WebsiteError::MenuDepthExceeded => 422,
            WebsiteError::MegaMenuIsolated => 422,
            WebsiteError::RedirectParamParity => 422,
            WebsiteError::RedirectTargetRequired => 422,
            WebsiteError::VisitorPepperNotConfigured => 500,
            WebsiteError::TurnstileRefused => 400,
            WebsiteError::TurnstileNotConfigured => 503,
            WebsiteError::TurnstileMisconfigured => 503,
            WebsiteError::TurnstileUnreachable => 503,
            WebsiteError::CaptchaProviderUnknown => 503,
            WebsiteError::IntakeRateLimited { .. } => 429,
            WebsiteError::IntakeFieldRejected => 422,
            WebsiteError::IntakeValidationFailed(_) => 422,
            WebsiteError::IntakeRefused(_) => 422,
            WebsiteError::InvalidInput(_) => 400,
            WebsiteError::NotFound(_) => 404,
            WebsiteError::Guarded(_) => 422,
            WebsiteError::WebsiteIsPrimaryForCompany => 422,
        }
    }

    /// The stable machine code the route layer emits.
    pub fn code(&self) -> &'static str {
        match self {
            WebsiteError::Db(_) => "website_internal_error",
            WebsiteError::Internal(_) => "website_internal_error",
            WebsiteError::WebsiteNotResolved => "website_not_resolved",
            WebsiteError::SpecificityConflict { .. } => "website_specificity_conflict",
            WebsiteError::ForkSourceMissing => "website_fork_source_missing",
            WebsiteError::GenericRequiresFanoutVerb => "website_generic_requires_fanout_verb",
            WebsiteError::FieldNotPatchable { .. } => "website_field_not_patchable",
            WebsiteError::PublishPermissionRequired => "website_publish_permission_required",
            WebsiteError::PageVisibilityRefused => "website_page_visibility_refused",
            WebsiteError::MenuDepthExceeded => "website_menu_depth_exceeded",
            WebsiteError::MegaMenuIsolated => "website_mega_menu_isolated",
            WebsiteError::RedirectParamParity => "website_redirect_param_parity",
            WebsiteError::RedirectTargetRequired => "website_redirect_target_required",
            WebsiteError::VisitorPepperNotConfigured => "website_visitor_pepper_not_configured",
            WebsiteError::TurnstileRefused => "website_turnstile_refused",
            WebsiteError::TurnstileNotConfigured => "website_turnstile_not_configured",
            WebsiteError::TurnstileMisconfigured => "website_turnstile_misconfigured",
            WebsiteError::TurnstileUnreachable => "website_turnstile_unreachable",
            WebsiteError::CaptchaProviderUnknown => "website_captcha_provider_unknown",
            WebsiteError::IntakeRateLimited { .. } => "website_intake_rate_limited",
            WebsiteError::IntakeFieldRejected => "website_intake_field_rejected",
            WebsiteError::IntakeValidationFailed(_) => "website_intake_validation_failed",
            WebsiteError::IntakeRefused(_) => "website_intake_refused",
            WebsiteError::InvalidInput(_) => "website_invalid_input",
            WebsiteError::NotFound(_) => "website_not_found",
            WebsiteError::Guarded(_) => "website_guarded_refusal",
            WebsiteError::WebsiteIsPrimaryForCompany => "website_is_primary_for_company",
        }
    }
}

/// Map a Postgres unique-violation (23505) on one of this module's
/// constraint names to the typed conflict arm; anything else stays a DB
/// error. The constraint names are the DB half of the resolver contract.
pub fn map_unique_violation(err: sqlx::Error) -> WebsiteError {
    if let sqlx::Error::Database(db) = &err {
        if db.code().as_deref() == Some("23505") {
            let msg = db.message().to_lowercase();
            let constraint = db
                .constraint()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            let _ = msg;
            return WebsiteError::SpecificityConflict { constraint };
        }
    }
    WebsiteError::Db(err)
}
