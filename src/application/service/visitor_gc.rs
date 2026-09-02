//! The 60-day partnerless visitor GC (hand-written; user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! ONE cron exists in this module and this is it (`visitor-gc`,
//! daily at 03:23, pull posture, commit-per-batch, pickup lock).
//! Hard DELETE (never archive) of anonymous visitors whose last
//! connection predates the retention window; tracks cascade at the
//! DB. Safe to run concurrently (batch claims lock-skip) and
//! re-runnable; nothing gates on swept state.

use sqlx::PgPool;
use tracing::info;

use super::website_error::WebsiteError;
use super::website_service::{record_audit, ActorRef};

/// The GC verb's progress report.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct SweepSummary {
    /// Visitor rows deleted.
    pub swept: u64,
    /// Batches committed (progress is durable per batch).
    pub batches: u64,
}

/// Default retention (days) — `WEBSITE_VISITOR_RETENTION_DAYS` overrides.
pub const DEFAULT_RETENTION_DAYS: i64 = 60;

/// Default batch size — `WEBSITE_VISITOR_GC_BATCH` overrides.
pub const DEFAULT_GC_BATCH: i64 = 1000;

/// Sweep anonymous (`portal_user_id IS NULL`) visitors whose last
/// connection is older than `retention_days`, in committed batches of
/// `batch`. Each batch is claimed `FOR UPDATE SKIP LOCKED` so a
/// concurrent sweep (cron + manual trigger) never double-deletes.
/// Audits `visitor_gc_swept` once per run with the totals.
pub async fn sweep_partnerless_visitors(
    pool: &PgPool,
    retention_days: i64,
    batch: i64,
) -> Result<SweepSummary, WebsiteError> {
    let mut summary = SweepSummary::default();
    loop {
        let mut tx = pool.begin().await?;
        let deleted = sqlx::query(
            r#"
            DELETE FROM website.visitors
            WHERE id IN (
                SELECT id FROM website.visitors
                WHERE portal_user_id IS NULL
                  AND last_connection_at < now() - make_interval(days => $1::int)
                ORDER BY last_connection_at
                LIMIT $2
                FOR UPDATE SKIP LOCKED
            )
            "#,
        )
        .bind(retention_days)
        .bind(batch)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        if deleted == 0 {
            break;
        }
        summary.swept += deleted;
        summary.batches += 1;
        info!(
            swept_total = summary.swept,
            batch = summary.batches,
            "visitor GC committed a batch"
        );
    }
    if summary.swept > 0 {
        record_audit(
            pool,
            "visitor_gc_swept",
            ActorRef::system(),
            Some("visitor"),
            None,
            Some(serde_json::json!({
                "swept": summary.swept,
                "batches": summary.batches,
                "retention_days": retention_days,
            })),
        )
        .await?;
    }
    Ok(summary)
}
