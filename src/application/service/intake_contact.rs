//! The reference intake declaration: contact (hand-written;
//! user-owned; see `metaphor.codegen.yaml`).
//!
//! `POST /public/intake/contact` — turnstile-gated, Tier-B limited
//! (5 per identity / 20 per IP per hour), persisted into
//! `website.contact_messages` with `notified = false` until the
//! notifier port delivers to the website's FIXED officer recipients
//! (`websites.contact_recipients`). The payload struct carries ONLY
//! the fields this verb owns — a submitter can admit no identity or
//! routing column, and the module sends nothing to any submitter
//! address, ever.

use serde::Deserialize;
use sqlx::PgConnection;
use uuid::Uuid;

use super::intake_engine::{IntakeDeclaration, IntakeOutcome};
use super::website_error::WebsiteError;

/// The typed allowlist — unknown keys are UNPARSEABLE at the edge
/// (422), not silently dropped.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactPayload {
    /// Optional submitter-displayed name.
    pub name: Option<String>,
    /// The submitter's reply address — stored for OFFICER replies
    /// only; no automated mail ever leaves the module.
    pub email: String,
    pub message: String,
}

/// The message as the officer read and the notifier port see it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ContactMessageView {
    pub id: Uuid,
    pub website_id: Uuid,
    pub name: Option<String>,
    pub email: String,
    pub message: String,
    pub notified: bool,
}

/// The reference declaration.
pub struct ContactIntake;

impl IntakeDeclaration for ContactIntake {
    const NAME: &'static str = "contact";
    const REQUIRES_TURNSTILE: bool = true;
    const IDENTITY_LIMIT: u32 = 5;
    const IP_LIMIT: u32 = 20;

    type Payload = ContactPayload;

    async fn validate(payload: &Self::Payload) -> Result<(), WebsiteError> {
        if let Some(name) = payload.name.as_deref() {
            if name.len() > 120 {
                return Err(WebsiteError::IntakeValidationFailed(
                    "name exceeds 120 characters".into(),
                ));
            }
        }
        let email = payload.email.trim();
        if email.is_empty() || email.len() > 320 {
            return Err(WebsiteError::IntakeValidationFailed(
                "email is required (<= 320 characters)".into(),
            ));
        }
        if !email.contains('@') || email.starts_with('@') || email.ends_with('@') {
            return Err(WebsiteError::IntakeValidationFailed(
                "email is not a valid address".into(),
            ));
        }
        let message = payload.message.trim();
        if message.is_empty() || message.len() > 5000 {
            return Err(WebsiteError::IntakeValidationFailed(
                "message is required (<= 5000 characters)".into(),
            ));
        }
        Ok(())
    }

    async fn persist(
        tx: &mut PgConnection,
        website_id: Uuid,
        payload: Self::Payload,
    ) -> Result<IntakeOutcome, WebsiteError> {
        let id: Uuid = sqlx::query_scalar(
            r#"
            INSERT INTO website.contact_messages
                (id, website_id, name, email, message, notified, metadata)
            VALUES (gen_random_uuid(), $1, $2, $3, $4, FALSE,
                    jsonb_build_object('created_at', now()))
            RETURNING id
            "#,
        )
        .bind(website_id)
        .bind(payload.name.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .bind(payload.email.trim())
        .bind(payload.message.trim())
        .fetch_one(tx)
        .await
        .map_err(|e| {
            // Any persist-shape failure is the declaration's loud
            // refusal (the savepoint rolls the attempt back); the
            // typed surface stays closed, the source goes to tracing.
            tracing::debug!(error = ?e, "intake contact persist refused");
            WebsiteError::IntakeRefused(
                "the contact message could not be persisted".into(),
            )
        })?;
        Ok(IntakeOutcome {
            subject_id: Some(id),
            detail: Some(serde_json::json!({ "email": payload.email })),
            contact: Some(ContactMessageView {
                id,
                website_id,
                name: payload.name,
                email: payload.email,
                message: payload.message,
                notified: false,
            }),
        })
    }
}
