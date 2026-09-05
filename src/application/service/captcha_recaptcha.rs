//! The captcha provider arm (hand-written; user-owned; see
//! `metaphor.codegen.yaml`; the contract of record is docs/spec.md §6.3).
//!
//! `TurnstileClient` (Cloudflare) is the shipped default verifier; this
//! file adds its SIBLING — Google reCAPTCHA's siteverify — plus the
//! minimal selection shape that lets a host compose either. Both are
//! body-dialect variants of one contract: form-POST `secret` +
//! `response`, parse the JSON answer, map onto the SAME four typed
//! verdicts (the frozen `website_turnstile_*` codes — the prefix is
//! historical and now means "the captcha verifier").
//!
//! Posture (WM-19 deliberately NOT ported): upstream's captcha seam
//! fails OPEN on a missing secret. Both arms here fail CLOSED — unset
//! secret, secret fault, bad token, transport failure, or a broken
//! selector each refuse the gated verb; nothing passes on any error
//! path. The configured secret and the presented token never appear in
//! any error text, audit row, or wire body.

use super::intake_engine::{TurnstileClient, TurnstileConfig};
use super::website_error::WebsiteError;

/// The env var selecting the captcha verifier (`WEBSITE_CAPTCHA_PROVIDER`).
pub const WEBSITE_CAPTCHA_PROVIDER_ENV: &str = "WEBSITE_CAPTCHA_PROVIDER";

/// The env var holding the reCAPTCHA secret (unset ≠ misconfigured).
pub const WEBSITE_RECAPTCHA_SECRET_ENV: &str = "WEBSITE_RECAPTCHA_SECRET";

/// The env var overriding the reCAPTCHA siteverify URL (probe seam).
pub const WEBSITE_RECAPTCHA_VERIFY_URL_ENV: &str = "WEBSITE_RECAPTCHA_VERIFY_URL";

/// Google's siteverify endpoint (the probe override replaces it).
const RECAPTCHA_DEFAULT_VERIFY_URL: &str = "https://www.google.com/recaptcha/api/siteverify";

/// Recaptcha knobs: the secret (`WEBSITE_RECAPTCHA_SECRET`, unset ≠
/// misconfigured) and the siteverify URL (`WEBSITE_RECAPTCHA_VERIFY_URL`,
/// probe-overridable) — the mirrored pair of `TurnstileConfig`.
#[derive(Debug, Clone)]
pub struct RecaptchaConfig {
    pub secret: Option<String>,
    pub verify_url: String,
}

impl RecaptchaConfig {
    pub fn from_env() -> Self {
        let secret = std::env::var(WEBSITE_RECAPTCHA_SECRET_ENV)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let verify_url = std::env::var(WEBSITE_RECAPTCHA_VERIFY_URL_ENV)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| RECAPTCHA_DEFAULT_VERIFY_URL.to_string());
        Self { secret, verify_url }
    }
}

/// Google's siteverify answer body. A v3 answer's `score`/`action`/
/// `hostname` fields are deliberately NOT read (docs/spec.md §13 BTF-14:
/// the verifier answers the boolean verdict only — threshold policy is
/// never modeled here).
#[derive(Debug, serde::Deserialize)]
struct RecaptchaSiteverifyBody {
    success: bool,
    #[serde(default, rename = "error-codes")]
    error_codes: Vec<String>,
}

/// Google's secret-fault codes (the host misconfigured its secret,
/// distinguishable from a bad token and from transport failure).
/// Cloudflare's `bad-secret` has no Google counterpart.
const RECAPTCHA_SECRET_FAULTS: &[&str] = &["missing-input-secret", "invalid-input-secret"];

/// The reCAPTCHA verifier — the turnstile client's sibling, the
/// module's only outbound HTTP family (one call shape, two dialects).
#[derive(Debug, Clone)]
pub struct RecaptchaClient {
    config: RecaptchaConfig,
    http: reqwest::Client,
}

impl RecaptchaClient {
    pub fn new(config: RecaptchaConfig) -> Self {
        Self { config, http: reqwest::Client::new() }
    }

    pub fn config(&self) -> &RecaptchaConfig {
        &self.config
    }

    /// FAIL-CLOSED verify — the same four typed answers as the turnstile
    /// arm, never a passthrough: unset secret → not-configured; empty
    /// token → refused; Google's secret-fault codes → misconfigured;
    /// any other failure answer → refused; transport or parse failure →
    /// unreachable. `success:true` is the only pass.
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
        let body: RecaptchaSiteverifyBody = resp
            .json()
            .await
            .map_err(|_| WebsiteError::TurnstileUnreachable)?;
        if body.success {
            return Ok(());
        }
        if body
            .error_codes
            .iter()
            .any(|c| RECAPTCHA_SECRET_FAULTS.contains(&c.as_str()))
        {
            return Err(WebsiteError::TurnstileMisconfigured);
        }
        Err(WebsiteError::TurnstileRefused)
    }
}

/// The parsed `WEBSITE_CAPTCHA_PROVIDER` selector. A PURE parse (the
/// `WEBSITE_TRUSTED_PROXY` tolerant-parse pattern): trimmed,
/// case-insensitive; unknown values are the caller's failure to resolve,
/// never a silent fallback to a default provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptchaProvider {
    Turnstile,
    Recaptcha,
}

impl CaptchaProvider {
    /// Parse one raw selector value. Unset/empty → the shipped turnstile
    /// default (existing deployments unchanged); `turnstile`/`recaptcha`
    /// in any case → that provider; anything else → `None` (fail-closed
    /// — the unknown arm refuses every gated verb).
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "turnstile" => Some(CaptchaProvider::Turnstile),
            "recaptcha" => Some(CaptchaProvider::Recaptcha),
            _ => None,
        }
    }
}

/// The config-SELECTED verifier behind the intake engine's captcha step
/// (docs/spec.md §6.3: selection is a deployment property, never a
/// per-declaration const). `UnknownProvider` is the fail-closed arm for
/// a selector value naming no known provider — it refuses every gated
/// verb with the typed `website_captcha_provider_unknown`; it NEVER
/// falls back to a default provider.
#[derive(Debug, Clone)]
pub enum CaptchaVerifier {
    Turnstile(TurnstileClient),
    Recaptcha(RecaptchaClient),
    UnknownProvider,
}

impl From<TurnstileClient> for CaptchaVerifier {
    fn from(client: TurnstileClient) -> Self {
        CaptchaVerifier::Turnstile(client)
    }
}

impl From<RecaptchaClient> for CaptchaVerifier {
    fn from(client: RecaptchaClient) -> Self {
        CaptchaVerifier::Recaptcha(client)
    }
}

impl CaptchaVerifier {
    /// Build the selected verifier from explicit per-provider configs
    /// plus the selector value (the pure-parse seam — probes pass
    /// crafted values, never the process environment).
    pub fn selected(
        selector: &str,
        turnstile: TurnstileConfig,
        recaptcha: RecaptchaConfig,
    ) -> Self {
        match CaptchaProvider::parse(selector) {
            Some(CaptchaProvider::Turnstile) => {
                CaptchaVerifier::Turnstile(TurnstileClient::new(turnstile))
            }
            Some(CaptchaProvider::Recaptcha) => {
                CaptchaVerifier::Recaptcha(RecaptchaClient::new(recaptcha))
            }
            None => CaptchaVerifier::UnknownProvider,
        }
    }

    /// Read the selector + both knob pairs from the environment. A host
    /// switching providers changes ONLY env vars — no code.
    pub fn from_env() -> Self {
        let selector = std::env::var(WEBSITE_CAPTCHA_PROVIDER_ENV).unwrap_or_default();
        Self::selected(&selector, TurnstileConfig::from_env(), RecaptchaConfig::from_env())
    }

    /// The selected provider, when the selector resolved. `None` is the
    /// unknown arm (composition surfaces answer
    /// `website_captcha_provider_unknown`).
    pub fn provider(&self) -> Option<CaptchaProvider> {
        match self {
            CaptchaVerifier::Turnstile(_) => Some(CaptchaProvider::Turnstile),
            CaptchaVerifier::Recaptcha(_) => Some(CaptchaProvider::Recaptcha),
            CaptchaVerifier::UnknownProvider => None,
        }
    }

    /// FAIL-CLOSED verify through the armed arm. Every path but a
    /// verified token refuses; the four verdicts are the turnstile
    /// family's (the frozen codes), the broken selector adds its own
    /// typed 503.
    pub async fn verify(&self, token: &str) -> Result<(), WebsiteError> {
        match self {
            CaptchaVerifier::Turnstile(client) => client.verify(token).await,
            CaptchaVerifier::Recaptcha(client) => client.verify(token).await,
            CaptchaVerifier::UnknownProvider => Err(WebsiteError::CaptchaProviderUnknown),
        }
    }
}
