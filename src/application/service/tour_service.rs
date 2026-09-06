//! Tour persistence — the `web_tour` port, persistence layer only
//! (hand-written; user-owned; see `metaphor.codegen.yaml`).
//!
//! Port scope (the register's WB-2 row): upstream `web_tour.tour`
//! (name unique, per-user consumption M2M) plus its thin Python —
//! the tour ENGINE (trigger/run/tooltip) lives in the client and
//! consumes definitions BY NAME. This service is the whole server
//! side: definitions keyed by a live-unique name, and per-principal
//! consumption as a set-membership fact.
//!
//! Fences applied by the port:
//!   * NAME uniqueness among LIVE rows is a partial unique index
//!     (`idx_website_tours_name_live`) — the module's live-row
//!     uniqueness family. Upsert is idempotent by name (the engine
//!     convention); a soft-deleted name is immediately re-creatable.
//!   * CONSUMPTION requires a verified portal principal (the logical
//!     portal-user row id — this module's established principal
//!     pattern; anonymous callers answer the typed 401). The pair
//!     (tour, principal) is unique, so re-consume is an idempotent
//!     no-op — never a counter, never a second row.
//!   * RESET (dropping consumption facts so a tour can run again) is
//!     an OFFICER verb, audited like every other officer mutation.
//!     Consumption itself is not audited: it is an idempotent
//!     per-principal fact whose trail IS the consumption table.
//!
//! Tours are NOT website-scoped (the upstream registry rows are
//! global) — the same posture as `website.website_audit_log`.

use serde::Serialize;
use uuid::Uuid;

use super::website_error::WebsiteError;
use super::website_service::{record_audit, ActorRef};

/// The tour-name length cap (the module's text-bound family).
const TOUR_NAME_MAX: usize = 120;
/// The display-name length cap.
const TOUR_DISPLAY_NAME_MAX: usize = 120;
/// The rainbow-man message cap.
const TOUR_MESSAGE_MAX: usize = 500;
/// The serialized steps-payload cap (256 KiB — a bound, not a parser:
/// the steps are the client engine's own vocabulary, opaque here).
const TOUR_STEPS_MAX_BYTES: usize = 256 * 1024;

/// One live tour definition as the surface reads it.
#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct TourView {
    pub id: Uuid,
    pub name: String,
    pub display_name: String,
    pub rainbow_man_message: Option<String>,
    pub steps: serde_json::Value,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// One consumed tour in a principal's consumed set.
#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct ConsumedTour {
    pub name: String,
    pub tour_id: Uuid,
    pub consumed_at: chrono::DateTime<chrono::Utc>,
}

/// The consume verb's outcome (both arms are successes — consumption
/// is idempotent set membership, and the caller may not care which
/// happened; `consumed_at` reports the ORIGINAL fact either way).
#[derive(Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ConsumeOutcome {
    Consumed { consumed_at: chrono::DateTime<chrono::Utc> },
    AlreadyConsumed { consumed_at: chrono::DateTime<chrono::Utc> },
}

/// Officer upsert input (create-or-update by name).
#[derive(Debug)]
pub struct UpsertTourInput {
    pub name: String,
    pub display_name: String,
    pub rainbow_man_message: Option<String>,
    pub steps: serde_json::Value,
}

/// The tour persistence service.
#[derive(Clone)]
pub struct TourService {
    pool: sqlx::PgPool,
}

impl TourService {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    /// Create or update the live tour definition carrying `name`.
    /// Idempotent by name — the engine's consumption key never moves.
    pub async fn upsert_tour(
        &self,
        actor: ActorRef,
        input: UpsertTourInput,
    ) -> Result<TourView, WebsiteError> {
        let name = input.name.trim().to_string();
        if name.is_empty() || name.len() > TOUR_NAME_MAX {
            return Err(WebsiteError::InvalidInput(format!(
                "tour name must carry between 1 and {TOUR_NAME_MAX} characters"
            )));
        }
        let display_name = input.display_name.trim().to_string();
        if display_name.len() > TOUR_DISPLAY_NAME_MAX {
            return Err(WebsiteError::InvalidInput(format!(
                "tour display_name is capped at {TOUR_DISPLAY_NAME_MAX} characters"
            )));
        }
        let rainbow_man_message = input
            .rainbow_man_message
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(str::to_string);
        if rainbow_man_message.as_deref().map(str::len).unwrap_or(0) > TOUR_MESSAGE_MAX {
            return Err(WebsiteError::InvalidInput(format!(
                "tour rainbow_man_message is capped at {TOUR_MESSAGE_MAX} characters"
            )));
        }
        // Steps are the client engine's vocabulary — opaque here, but
        // still bounded and still an ARRAY (the engine iterates them).
        if !input.steps.is_array() {
            return Err(WebsiteError::InvalidInput(
                "tour steps must be a JSON array (the engine's step vocabulary)".to_string(),
            ));
        }
        let steps_text = input.steps.to_string();
        if steps_text.len() > TOUR_STEPS_MAX_BYTES {
            return Err(WebsiteError::InvalidInput(format!(
                "tour steps payload is capped at {TOUR_STEPS_MAX_BYTES} bytes"
            )));
        }

        let mut tx = self.pool.begin().await?;
        let tour = sqlx::query_as::<_, TourView>(
            r#"
            INSERT INTO website.tours (name, display_name, rainbow_man_message, steps)
            VALUES ($1, $2, $3, $4::jsonb)
            ON CONFLICT (name) WHERE (metadata->>'deleted_at') IS NULL
            DO UPDATE SET
                display_name = EXCLUDED.display_name,
                rainbow_man_message = EXCLUDED.rainbow_man_message,
                steps = EXCLUDED.steps,
                metadata = website.tours.metadata
                    || jsonb_build_object('updated_by', $5::uuid)
            RETURNING id, name, display_name, rainbow_man_message, steps,
                      (metadata->>'updated_at')::timestamptz AS updated_at
            "#,
        )
        .bind(&name)
        .bind(&display_name)
        .bind(&rainbow_man_message)
        .bind(&steps_text)
        .bind(actor.stamp())
        .fetch_one(&mut *tx)
        .await
        .map_err(super::website_error::map_unique_violation)?;
        record_audit(
            &mut *tx,
            "tour_upserted",
            actor,
            Some("tour"),
            Some(tour.id),
            Some(serde_json::json!({ "name": tour.name })),
        )
        .await?;
        tx.commit().await?;
        Ok(tour)
    }

    /// The live definition for one name (the engine's by-name lookup).
    pub async fn tour_by_name(&self, name: &str) -> Result<Option<TourView>, WebsiteError> {
        let tour = sqlx::query_as::<_, TourView>(
            r#"
            SELECT id, name, display_name, rainbow_man_message, steps,
                   (metadata->>'updated_at')::timestamptz AS updated_at
            FROM website.tours
            WHERE name = $1 AND (metadata->>'deleted_at') IS NULL
            "#,
        )
        .bind(name.trim())
        .fetch_optional(&self.pool)
        .await?;
        Ok(tour)
    }

    /// Every live definition, ordered by name (deterministic).
    pub async fn list_tours(&self) -> Result<Vec<TourView>, WebsiteError> {
        let tours = sqlx::query_as::<_, TourView>(
            r#"
            SELECT id, name, display_name, rainbow_man_message, steps,
                   (metadata->>'updated_at')::timestamptz AS updated_at
            FROM website.tours
            WHERE (metadata->>'deleted_at') IS NULL
            ORDER BY name, id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(tours)
    }

    /// Soft-delete the live definition carrying `name` (the name is
    /// immediately re-creatable — the live-unique index ignores dead
    /// rows). A missing name is the typed 404.
    pub async fn delete_tour(&self, actor: ActorRef, name: &str) -> Result<(), WebsiteError> {
        let mut tx = self.pool.begin().await?;
        let deleted = sqlx::query_scalar::<_, Uuid>(
            r#"
            UPDATE website.tours
            SET metadata = metadata || jsonb_build_object(
                'deleted_at', to_jsonb(now()),
                'deleted_by', to_jsonb($2::uuid))
            WHERE name = $1 AND (metadata->>'deleted_at') IS NULL
            RETURNING id
            "#,
        )
        .bind(name.trim())
        .bind(actor.stamp())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(id) = deleted else {
            return Err(WebsiteError::TourNotFound);
        };
        record_audit(
            &mut *tx,
            "tour_deleted",
            actor,
            Some("tour"),
            Some(id),
            Some(serde_json::json!({ "name": name.trim() })),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Record this principal's consumption of the tour named `name`.
    /// Idempotent: the (tour, principal) pair is unique, a repeat is
    /// the `already_consumed` arm reporting the ORIGINAL timestamp.
    pub async fn consume_tour(
        &self,
        portal_user_id: Uuid,
        name: &str,
    ) -> Result<ConsumeOutcome, WebsiteError> {
        let mut tx = self.pool.begin().await?;
        let tour: Option<(Uuid,)> = sqlx::query_as(
            r#"
            SELECT id FROM website.tours
            WHERE name = $1 AND (metadata->>'deleted_at') IS NULL
            "#,
        )
        .bind(name.trim())
        .fetch_optional(&mut *tx)
        .await?;
        let Some((tour_id,)) = tour else {
            return Err(WebsiteError::TourNotFound);
        };
        // ON CONFLICT DO NOTHING keeps the original row when the fact
        // already exists. RETURNING yields a row ONLY when this call
        // inserted — the exact discriminator between the two arms, no
        // clock heuristics (a repeat within the same second is still
        // a repeat).
        let inserted: Option<chrono::DateTime<chrono::Utc>> =
            sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
                r#"
                INSERT INTO website.tour_consumptions (tour_id, portal_user_id)
                VALUES ($1, $2)
                ON CONFLICT (tour_id, portal_user_id) DO NOTHING
                RETURNING consumed_at
                "#,
            )
            .bind(tour_id)
            .bind(portal_user_id)
            .fetch_optional(&mut *tx)
            .await?;
        let outcome = match inserted {
            Some(consumed_at) => ConsumeOutcome::Consumed { consumed_at },
            None => {
                let consumed_at = sqlx::query_scalar::<_, chrono::DateTime<chrono::Utc>>(
                    r#"
                    SELECT consumed_at FROM website.tour_consumptions
                    WHERE tour_id = $1 AND portal_user_id = $2
                    "#,
                )
                .bind(tour_id)
                .bind(portal_user_id)
                .fetch_one(&mut *tx)
                .await?;
                ConsumeOutcome::AlreadyConsumed { consumed_at }
            }
        };
        tx.commit().await?;
        Ok(outcome)
    }

    /// The principal's consumed set (the boot read: which tours the
    /// engine must NOT replay), ordered by name — deterministic.
    pub async fn consumed_for(
        &self,
        portal_user_id: Uuid,
    ) -> Result<Vec<ConsumedTour>, WebsiteError> {
        let consumed = sqlx::query_as::<_, ConsumedTour>(
            r#"
            SELECT t.name, c.tour_id, c.consumed_at
            FROM website.tour_consumptions c
            JOIN website.tours t ON t.id = c.tour_id
            WHERE c.portal_user_id = $1 AND (t.metadata->>'deleted_at') IS NULL
            ORDER BY t.name, c.tour_id
            "#,
        )
        .bind(portal_user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(consumed)
    }

    /// Officer reset: drop every consumption fact for the tour named
    /// `name` so the tour runs again for everyone. Audited (the one
    /// tour verb that erases user-visible state). Returns the number
    /// of dropped facts.
    pub async fn reset_tour(&self, actor: ActorRef, name: &str) -> Result<u64, WebsiteError> {
        let mut tx = self.pool.begin().await?;
        let tour: Option<(Uuid,)> = sqlx::query_as(
            r#"
            SELECT id FROM website.tours
            WHERE name = $1 AND (metadata->>'deleted_at') IS NULL
            "#,
        )
        .bind(name.trim())
        .fetch_optional(&mut *tx)
        .await?;
        let Some((tour_id,)) = tour else {
            return Err(WebsiteError::TourNotFound);
        };
        let dropped = sqlx::query(
            r#"
            DELETE FROM website.tour_consumptions WHERE tour_id = $1
            "#,
        )
        .bind(tour_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        record_audit(
            &mut *tx,
            "tour_reset",
            actor,
            Some("tour"),
            Some(tour_id),
            Some(serde_json::json!({ "dropped": dropped, "name": name.trim() })),
        )
        .await?;
        tx.commit().await?;
        Ok(dropped)
    }
}
