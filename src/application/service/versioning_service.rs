//! Copy-on-write / copy-on-unlink as EXPLICIT versioning verbs
//! (hand-written; user-owned; see `metaphor.codegen.yaml`).
//!
//! No ambient `website_id`-in-context exists anywhere; no write path
//! sniffs a header to decide copy semantics. Forking and fanout
//! deletion are NAMED verbs with provenance columns (`forked_from`,
//! `forked_at`, `forked_by`), each one transaction, each audited.
//! The concurrent-first-edit race resolves at the DB: the sentinel
//! unique lets exactly one INSERT land; the loser's
//! `ON CONFLICT DO NOTHING` + re-select observes the winner's row.

use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::entity::Page;

use super::specificity::{resolve_generic, resolve_specific, resolve_specific_entity, Resolution};
use super::website_error::WebsiteError;
use super::website_service::{record_audit, ActorRef};

/// The fork verb's answer: which row won.
#[derive(Debug, Clone)]
pub enum ForkOutcome {
    /// This call created the specific row.
    Created(Page),
    /// A specific already existed (or a concurrent call won the race)
    /// — the same row, same id, no second copy.
    Existing(Page),
}

impl ForkOutcome {
    pub fn row(&self) -> &Page {
        match self {
            ForkOutcome::Created(p) | ForkOutcome::Existing(p) => p,
        }
    }
}

/// The fanout deletion's answer.
#[derive(Debug, Clone)]
pub struct FanoutDeletion {
    pub generic_id: Uuid,
    /// The specific rows force-forked before the generic fell (one per
    /// website that lacked a specific for the key).
    pub created: Vec<Uuid>,
    /// The websites whose menus were re-pointed.
    pub websites_touched: Vec<Uuid>,
}

async fn specific_entity(
    exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    key: &str,
    website_id: Uuid,
) -> Result<Option<Page>, WebsiteError> {
    resolve_specific_entity(exec, key, website_id).await
}

async fn generic_entity(
    exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    key: &str,
) -> Result<Option<Page>, WebsiteError> {
    resolve_generic(exec, key).await
}

/// The INSERT..SELECT arm shared by the fork verb and the fanout loop:
/// copy the generic's content columns into a specific for `target`,
/// stamp provenance, `ON CONFLICT DO NOTHING` (the fence picks the
/// winner). Returns true when THIS call landed the row.
async fn insert_specific_copy(
    exec: &mut sqlx::PgConnection,
    generic_id: Uuid,
    target_website: Uuid,
    new_id: Uuid,
    actor: ActorRef,
) -> Result<bool, WebsiteError> {
    let inserted = sqlx::query(
        r#"
        INSERT INTO website.pages
            (id, key, website_id, url, title, seo_name, is_published, date_publish,
             website_indexed, visibility, required_member_roles,
             forked_from, forked_at, forked_by, metadata)
        SELECT $1, p.key, $2, p.url, p.title, p.seo_name, p.is_published, p.date_publish,
               p.website_indexed, p.visibility, p.required_member_roles,
               p.id, now(), $3,
               jsonb_build_object('created_at', now(), 'created_by', $3)
        FROM website.pages p
        WHERE p.id = $4 AND (p.metadata->>'deleted_at') IS NULL
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(new_id)
    .bind(target_website)
    .bind(actor.stamp())
    .bind(generic_id)
    .execute(&mut *exec)
    .await?
    .rows_affected()
        > 0;

    if inserted {
        // Full block set, positions preserved — only for a row this
        // call created (a raced loser copying again would duplicate).
        sqlx::query(
            r#"
            INSERT INTO website.page_blocks (id, page_id, kind, position, payload)
            SELECT gen_random_uuid(), $1, b.kind, b.position, b.payload
            FROM website.page_blocks b
            WHERE b.page_id = $2
            "#,
        )
        .bind(new_id)
        .bind(generic_id)
        .execute(&mut *exec)
        .await?;
    }
    Ok(inserted)
}

/// Fork the GENERIC page for `key` into a website-specific copy.
/// Idempotent: if a specific already exists for (key, target), return
/// it. One transaction; audited `page_forked`.
pub async fn fork_to_website(
    pool: &PgPool,
    actor: ActorRef,
    key: &str,
    target_website: Uuid,
) -> Result<ForkOutcome, WebsiteError> {
    let mut tx = pool.begin().await?;

    // 1. A live specific already answers → idempotent Existing.
    if let Resolution::Specific(_) = resolve_specific(&mut *tx, key, target_website).await? {
        let row = specific_entity(&mut *tx, key, target_website)
            .await?
            .ok_or_else(|| WebsiteError::Internal("specific vanished mid-fork".into()))?;
        tx.commit().await?;
        return Ok(ForkOutcome::Existing(row));
    }

    // 2. The generic must exist and be live.
    let generic = generic_entity(&mut *tx, key)
        .await?
        .ok_or(WebsiteError::ForkSourceMissing)?;

    // 3. INSERT..SELECT with the fence; the loser re-selects the winner.
    let new_id = Uuid::new_v4();
    let created = insert_specific_copy(&mut *tx, generic.id, target_website, new_id, actor).await?;
    let row = specific_entity(&mut *tx, key, target_website)
        .await?
        .ok_or_else(|| WebsiteError::Internal("fork landed but re-select found nothing".into()))?;

    // 4. Re-point this website's menus bound to the generic — exactly
    // once (only the call whose INSERT landed performs it).
    if created {
        sqlx::query(
            r#"
            UPDATE website.menus
            SET page_id = $1,
                metadata = jsonb_set(metadata, '{updated_at}', to_jsonb(now()))
            WHERE website_id = $2 AND page_id = $3
              AND (metadata->>'deleted_at') IS NULL
            "#,
        )
        .bind(row.id)
        .bind(target_website)
        .bind(generic.id)
        .execute(&mut *tx)
        .await?;
    }

    // 5. Audit.
    record_audit(
        &mut *tx,
        "page_forked",
        actor,
        Some("page"),
        Some(row.id),
        Some(serde_json::json!({
            "key": key,
            "target_website": target_website,
            "generic_id": generic.id,
            "created_by_this_call": created,
        })),
    )
    .await?;

    tx.commit().await?;
    Ok(if created {
        ForkOutcome::Created(row)
    } else {
        ForkOutcome::Existing(row)
    })
}

/// COU: before deleting a GENERIC page, force-fork it to every website
/// that has no specific yet, re-point those websites' menus, then
/// soft-delete the generic. All-or-nothing: any failure rolls the whole
/// fanout back — a partial fanout is never committed.
pub async fn delete_generic_with_fanout(
    pool: &PgPool,
    actor: ActorRef,
    key: &str,
) -> Result<FanoutDeletion, WebsiteError> {
    let mut tx = pool.begin().await?;

    let generic = generic_entity(&mut *tx, key)
        .await?
        .ok_or(WebsiteError::ForkSourceMissing)?;

    // Every live website lacking a specific for the key.
    let targets: Vec<(Uuid,)> = sqlx::query_as(
        r#"
        SELECT w.id FROM website.websites w
        WHERE (w.metadata->>'deleted_at') IS NULL
          AND NOT EXISTS (
              SELECT 1 FROM website.pages p
              WHERE p.key = $1 AND p.website_id = w.id
                AND (p.metadata->>'deleted_at') IS NULL
          )
        ORDER BY w.id
        "#,
    )
    .bind(key)
    .fetch_all(&mut *tx)
    .await?;

    let mut created: Vec<Uuid> = Vec::with_capacity(targets.len());
    let mut touched: Vec<Uuid> = Vec::with_capacity(targets.len());
    for (website,) in targets {
        let new_id = Uuid::new_v4();
        if insert_specific_copy(&mut *tx, generic.id, website, new_id, actor).await? {
            created.push(new_id);
            touched.push(website);
        }
    }

    // After the loop every website holds a specific: re-point each
    // website's generic-bound menus to ITS specific.
    let repointed = sqlx::query(
        r#"
        UPDATE website.menus m
        SET page_id = p.id,
            metadata = jsonb_set(m.metadata, '{updated_at}', to_jsonb(now()))
        FROM website.pages p
        WHERE p.key = $1 AND p.website_id = m.website_id
          AND p.website_id IS NOT NULL
          AND (p.metadata->>'deleted_at') IS NULL
          AND m.page_id = $2
          AND (m.metadata->>'deleted_at') IS NULL
        "#,
    )
    .bind(key)
    .bind(generic.id)
    .execute(&mut *tx)
    .await?
    .rows_affected();

    // Soft-delete the generic.
    sqlx::query(
        r#"
        UPDATE website.pages
        SET metadata = jsonb_set(jsonb_set(metadata, '{deleted_at}', to_jsonb(now())),
                                 '{deleted_by}', to_jsonb($2))
        WHERE id = $1
        "#,
    )
    .bind(generic.id)
    .bind(actor.stamp())
    .execute(&mut *tx)
    .await?;

    record_audit(
        &mut *tx,
        "generic_deleted_with_fanout",
        actor,
        Some("page"),
        Some(generic.id),
        Some(serde_json::json!({
            "key": key,
            "created_specifics": created,
            "menus_repointed": repointed,
        })),
    )
    .await?;

    tx.commit().await?;
    Ok(FanoutDeletion {
        generic_id: generic.id,
        created,
        websites_touched: touched,
    })
}
