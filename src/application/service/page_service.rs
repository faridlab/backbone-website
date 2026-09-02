//! Page service (hand-written; user-owned; see `metaphor.codegen.yaml`).
//!
//! The generated CRUD alias first (keeps lib.rs's wiring compiling),
//! then the hand page verbs: officer create/list/patch, the
//! publish/unpublish fence verbs, rename (the redirect generator),
//! specifics-only delete, and the PUBLIC page read (published + tier
//! rules). Every generic/specific read routes through the ONE
//! resolver in `specificity.rs`.

use sqlx::PgPool;
use uuid::Uuid;

use backbone_core::GenericCrudService;
use crate::domain::entity::Page;
use crate::infrastructure::persistence::PageRepository;
use crate::presentation::dto::{CreatePageDto, UpdatePageDto};

/// Generated CRUD alias (the generator skipped emitting this file
/// because it is user-owned).
pub type PageService = GenericCrudService<
    Page,
    CreatePageDto,
    UpdatePageDto,
    PageRepository,
>;

use super::specificity::{resolve_page_by_url, resolve_sitemap_page, Resolution, ResolvedPage};
use super::website_error::WebsiteError;
use super::website_service::{record_audit, ActorRef};

/// Fields NO generic patch may carry — the publish verbs are the only
/// writers (`website_field_not_patchable` names the verb to use).
pub const PUBLISH_FENCED_FIELDS: &[&str] = &["is_published", "date_publish"];

/// One structured block of a page's content list.
#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct BlockView {
    pub kind: String,
    pub position: i32,
    pub payload: serde_json::Value,
}

/// The public page answer: the resolved page plus its ordered blocks.
#[derive(Debug, Clone)]
pub struct PublicPage {
    pub page: ResolvedPage,
    pub blocks: Vec<BlockView>,
}

/// Officer create input. `key` is immutable after create; a missing
/// `website_id` creates the GENERIC row (the resolver's fallback).
#[derive(Debug, Clone)]
pub struct CreatePageInput {
    pub key: String,
    pub website_id: Option<Uuid>,
    pub url: String,
    pub title: String,
    pub seo_name: Option<String>,
    pub visibility: String,
    pub required_member_roles: Vec<String>,
    pub website_indexed: bool,
}

/// The typed patch whitelist. `key` (immutable), `website_id`
/// (provenance), and the publish-fenced fields are structurally
/// absent.
#[derive(Debug, Clone, Default)]
pub struct PagePatch {
    pub url: Option<String>,
    pub title: Option<String>,
    pub seo_name: Option<String>,
    pub visibility: Option<String>,
    pub required_member_roles: Option<Vec<String>>,
    pub website_indexed: Option<bool>,
}

impl PagePatch {
    pub fn is_empty(&self) -> bool {
        self.url.is_none()
            && self.title.is_none()
            && self.seo_name.is_none()
            && self.visibility.is_none()
            && self.required_member_roles.is_none()
            && self.website_indexed.is_none()
    }
}

fn validate_url(url: &str) -> Result<(), WebsiteError> {
    if !url.starts_with('/') {
        return Err(WebsiteError::InvalidInput(
            "url must be a site-absolute path starting with '/'".into(),
        ));
    }
    if url.len() > 1 && url.ends_with('/') {
        return Err(WebsiteError::InvalidInput(
            "url must not carry a trailing slash (the matcher 301s it away)".into(),
        ));
    }
    if url[1..].contains("//") {
        return Err(WebsiteError::InvalidInput(
            "url must not carry empty // segments (the matcher collapses them)".into(),
        ));
    }
    Ok(())
}

fn validate_visibility(v: &str) -> Result<(), WebsiteError> {
    match v {
        "public" | "connected" | "restricted" => Ok(()),
        other => Err(WebsiteError::InvalidInput(format!(
            "unknown visibility tier {other:?} (public | connected | restricted)"
        ))),
    }
}

/// The tier predicate (§3): anonymous → public only; a verified
/// portal principal → connected, and restricted when a website_members
/// row exists for (principal, website) with role ∈ required roles;
/// officers (admin tree) bypass — they read their own trees.
pub async fn tier_passes(
    exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    visibility: &str,
    required_roles: &[String],
    website_id: Uuid,
    principal: Option<Uuid>,
) -> Result<bool, WebsiteError> {
    match visibility {
        "public" => Ok(true),
        "connected" => Ok(principal.is_some()),
        "restricted" => {
            let Some(p) = principal else { return Ok(false) };
            let roles: Option<Vec<String>> = sqlx::query_scalar(
                r#"
                SELECT array_agg(m.role::text)
                FROM website.website_members m
                WHERE m.website_id = $1 AND m.portal_user_id = $2
                  AND (m.metadata->>'deleted_at') IS NULL
                "#,
            )
            .bind(website_id)
            .bind(p)
            .fetch_optional(exec)
            .await?
            .flatten();
            let Some(roles) = roles else { return Ok(false) };
            Ok(required_roles.iter().any(|r| roles.contains(r)))
        }
        other => Err(WebsiteError::Internal(format!(
            "unknown stored visibility tier {other:?}"
        ))),
    }
}

/// The hand page verbs.
pub struct PageAdminService {
    pool: PgPool,
}

impl PageAdminService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Officer create (admin tree). Audits `page_created`. A url/key
    /// collision on the specificity grain maps to the typed 409.
    pub async fn create_page(
        &self,
        actor: ActorRef,
        input: CreatePageInput,
    ) -> Result<Page, WebsiteError> {
        if input.key.trim().is_empty() || input.key.len() > 120 {
            return Err(WebsiteError::InvalidInput(
                "key is required (1..=120 chars)".into(),
            ));
        }
        validate_url(&input.url)?;
        validate_visibility(&input.visibility)?;
        let page = sqlx::query_as::<_, Page>(
            r#"
            INSERT INTO website.pages
                (id, key, website_id, url, title, seo_name, is_published, date_publish,
                 website_indexed, visibility, required_member_roles, metadata)
            VALUES ($1, $2, $3, $4, $5, $6, FALSE, NULL,
                    $7, $8::website_visibility, $9,
                    jsonb_build_object('created_at', now(), 'created_by', $10))
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(input.key.trim())
        .bind(input.website_id)
        .bind(&input.url)
        .bind(&input.title)
        .bind(input.seo_name)
        .bind(input.website_indexed)
        .bind(&input.visibility)
        .bind(&input.required_member_roles)
        .bind(actor.stamp())
        .fetch_one(&self.pool)
        .await
        .map_err(super::website_error::map_unique_violation)?;

        record_audit(
            &self.pool,
            "page_created",
            actor,
            Some("page"),
            Some(page.id),
            Some(serde_json::json!({ "key": page.key, "website_id": page.website_id })),
        )
        .await?;
        Ok(page)
    }

    /// Officer list — the fold for one website (its specifics + the
    /// generics), unpublished included. Explicit website ids only; the
    /// admin tree never binds by host. The set fold lives in the ONE
    /// resolver.
    pub async fn list_pages(&self, website_id: Uuid) -> Result<Vec<Page>, WebsiteError> {
        super::specificity::resolve_effective_pages(&self.pool, website_id).await
    }

    /// All raw rows for a key (officer provenance sight: the generic
    /// and every specific together).
    pub async fn rows_for_key(&self, key: &str) -> Result<Vec<Page>, WebsiteError> {
        let rows = sqlx::query_as::<_, Page>(
            r#"
            SELECT * FROM website.pages
            WHERE key = $1 AND (metadata->>'deleted_at') IS NULL
            ORDER BY website_id NULLS LAST
            "#,
        )
        .bind(key)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// The typed patch — whitelist arms only. The publish-fenced
    /// fields never reach this type; the route layer refuses them with
    /// `website_field_not_patchable` before mapping the body here.
    /// Audits `page_updated`.
    // The terminal set_arm arm's flag assignment is dead by construction.
    #[allow(unused_assignments)]
    pub async fn patch_page(
        &self,
        actor: ActorRef,
        id: Uuid,
        patch: PagePatch,
    ) -> Result<Page, WebsiteError> {
        use sqlx::QueryBuilder;
        if patch.is_empty() {
            return Err(WebsiteError::InvalidInput(
                "the page patch sets no field".into(),
            ));
        }
        if let Some(url) = patch.url.as_deref() {
            validate_url(url)?;
        }
        if let Some(v) = patch.visibility.as_deref() {
            validate_visibility(v)?;
        }
        let mut qb = QueryBuilder::new("UPDATE website.pages SET ");
        let mut first = true;
        macro_rules! set_arm {
            ($col:literal, $value:expr) => {
                if let Some(v) = $value {
                    if !first {
                        qb.push(", ");
                    }
                    qb.push($col).push(" = ").push_bind(v);
                    first = false;
                }
            };
        }
        set_arm!("url", patch.url.clone());
        set_arm!("title", patch.title.clone());
        set_arm!("seo_name", patch.seo_name.clone());
        if let Some(v) = patch.visibility.clone() {
            if !first {
                qb.push(", ");
            }
            qb.push("visibility = ").push_bind(v).push("::website_visibility");
            first = false;
        }
        set_arm!("required_member_roles", patch.required_member_roles.clone());
        set_arm!("website_indexed", patch.website_indexed);
        // ONE metadata assignment: a second `metadata =` in the same
        // UPDATE is a syntax error, so the actor stamp chains inside
        // the same jsonb_set nest.
        qb.push(", metadata = jsonb_set(jsonb_set(metadata, '{updated_at}', to_jsonb(now()))");
        if let Some(by) = actor.stamp() {
            qb.push(", '{updated_by}', to_jsonb(")
                .push_bind(by)
                .push(")");
        }
        qb.push(")");
        qb.push(" WHERE id = ").push_bind(id);
        qb.push(" AND (metadata->>'deleted_at') IS NULL RETURNING *");
        let page = qb
            .build_query_as::<Page>()
            .fetch_one(&self.pool)
            .await
            .map_err(super::website_error::map_unique_violation)?;
        record_audit(
            &self.pool,
            "page_updated",
            actor,
            Some("page"),
            Some(id),
            Some(serde_json::json!({ "fields": patch_field_names(&patch) })),
        )
        .await?;
        Ok(page)
    }

    /// The publish fence verb: `is_published = true`, `date_publish`
    /// stays lazily at its first publish instant. Audits
    /// `page_published`.
    pub async fn publish(&self, actor: ActorRef, id: Uuid) -> Result<Page, WebsiteError> {
        let page = sqlx::query_as::<_, Page>(
            r#"
            UPDATE website.pages
            SET is_published = TRUE,
                date_publish = COALESCE(date_publish, now()),
                metadata = jsonb_set(metadata, '{updated_at}', to_jsonb(now()))
            WHERE id = $1 AND (metadata->>'deleted_at') IS NULL
            RETURNING *
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| WebsiteError::NotFound(format!("page {id}")))?;
        record_audit(&self.pool, "page_published", actor, Some("page"), Some(id), None).await?;
        Ok(page)
    }

    /// The unpublish verb: `is_published = false`; `date_publish` is
    /// kept (the first-publish instant is history). Audits
    /// `page_unpublished`.
    pub async fn unpublish(&self, actor: ActorRef, id: Uuid) -> Result<Page, WebsiteError> {
        let page = sqlx::query_as::<_, Page>(
            r#"
            UPDATE website.pages
            SET is_published = FALSE,
                metadata = jsonb_set(metadata, '{updated_at}', to_jsonb(now()))
            WHERE id = $1 AND (metadata->>'deleted_at') IS NULL
            RETURNING *
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| WebsiteError::NotFound(format!("page {id}")))?;
        record_audit(&self.pool, "page_unpublished", actor, Some("page"), Some(id), None).await?;
        Ok(page)
    }

    /// Rename — validates normalization, moves `pages.url`, cascades
    /// menu targets, syncs `homepage_url` when this page was the
    /// homepage, and optionally writes the redirect row. Audits
    /// `page_renamed`.
    pub async fn rename_page(
        &self,
        actor: ActorRef,
        id: Uuid,
        new_url: String,
        create_redirect: Option<u16>,
    ) -> Result<Page, WebsiteError> {
        validate_url(&new_url)?;
        let mut tx = self.pool.begin().await?;
        let old: Option<(String, Option<Uuid>, String)> = sqlx::query_as(
            r#"
            SELECT url, website_id,
                   coalesce((metadata->>'deleted_at'), '') AS live
            FROM website.pages WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let (old_url, website_id, live) =
            old.ok_or_else(|| WebsiteError::NotFound(format!("page {id}")))?;
        if !live.is_empty() {
            return Err(WebsiteError::NotFound(format!("page {id}")));
        }
        let page = sqlx::query_as::<_, Page>(
            r#"
            UPDATE website.pages
            SET url = $2,
                metadata = jsonb_set(metadata, '{updated_at}', to_jsonb(now()))
            WHERE id = $1
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(&new_url)
        .fetch_one(&mut *tx)
        .await
        .map_err(super::website_error::map_unique_violation)?;

        // Cascade explicit menu urls that referenced the old path (the
        // website-scoped arm only — an explicit url is a literal, and
        // page_id-bound menus follow the page row automatically). Menus
        // are ALWAYS per-website; a generic rename has no website-scoped
        // cascade arm.
        if let Some(w) = website_id {
            sqlx::query(
                r#"
                UPDATE website.menus
                SET url = $3,
                    metadata = jsonb_set(metadata, '{updated_at}', to_jsonb(now()))
                WHERE url = $2
                  AND website_id = $1
                  AND (metadata->>'deleted_at') IS NULL
                "#,
            )
            .bind(w)
            .bind(&old_url)
            .bind(&new_url)
            .execute(&mut *tx)
            .await?;
        }

        // Homepage sync: when the renamed page WAS the site's homepage
        // (its old url equalled homepage_url), the website follows.
        if let Some(w) = website_id {
            sqlx::query(
                r#"
                UPDATE website.websites
                SET homepage_url = $3,
                    metadata = jsonb_set(metadata, '{updated_at}', to_jsonb(now()))
                WHERE id = $1 AND homepage_url = $2
                "#,
            )
            .bind(w)
            .bind(&old_url)
            .bind(&new_url)
            .execute(&mut *tx)
            .await?;
        }

        // Optional redirect row (301 or 302 only — a rename is never a
        // 308 alias and never a 410).
        let redirect_kind = match create_redirect {
            Some(301) => Some("moved_301"),
            Some(302) => Some("found_302"),
            Some(other) => {
                return Err(WebsiteError::InvalidInput(format!(
                    "create_redirect accepts 301 or 302, not {other}"
                )))
            }
            None => None,
        };
        if let Some(kind) = redirect_kind {
            sqlx::query(
                r#"
                INSERT INTO website.redirects
                    (id, website_id, url_from, redirect_type, url_to, metadata)
                VALUES (gen_random_uuid(), $1, $2, $3::website_redirect_type, $4,
                        jsonb_build_object('created_at', now(), 'created_by', $5))
                ON CONFLICT DO NOTHING
                "#,
            )
            .bind(website_id)
            .bind(&old_url)
            .bind(kind)
            .bind(&new_url)
            .bind(actor.stamp())
            .execute(&mut *tx)
            .await?;
            record_audit(
                &mut *tx,
                "redirect_created",
                actor,
                Some("redirect"),
                None,
                Some(serde_json::json!({
                    "url_from": old_url, "url_to": new_url, "kind": kind,
                    "origin": "page_rename",
                })),
            )
            .await?;
        }

        record_audit(
            &mut *tx,
            "page_renamed",
            actor,
            Some("page"),
            Some(id),
            Some(serde_json::json!({ "from": old_url, "to": new_url })),
        )
        .await?;
        tx.commit().await?;
        Ok(page)
    }

    /// Plain DELETE — SPECIFICS ONLY: re-points the website's menus
    /// back to the generic (the resolver picks it up). A generic row
    /// REFUSES: the fanout verb is the only generic deletion.
    pub async fn delete_specific(&self, actor: ActorRef, id: Uuid) -> Result<(), WebsiteError> {
        let mut tx = self.pool.begin().await?;
        let row: Option<(String, Option<Uuid>)> = sqlx::query_as(
            "SELECT key, website_id FROM website.pages WHERE id = $1 \
             AND (metadata->>'deleted_at') IS NULL",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((key, website_id)) = row else {
            return Err(WebsiteError::NotFound(format!("page {id}")));
        };
        let Some(_website) = website_id else {
            return Err(WebsiteError::GenericRequiresFanoutVerb);
        };

        sqlx::query(
            r#"
            UPDATE website.pages
            SET metadata = jsonb_set(jsonb_set(metadata, '{deleted_at}', to_jsonb(now())),
                                     '{deleted_by}', to_jsonb($2))
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(actor.stamp())
        .execute(&mut *tx)
        .await?;

        // Menus bound to the dead specific re-point to the generic if
        // one survives (loaded through the ONE resolver); otherwise the
        // binding drops (menu still carries name/url for rendering).
        let generic = super::specificity::resolve_generic(&mut *tx, &key).await?;
        if let Some(g) = generic {
            sqlx::query(
                r#"
                UPDATE website.menus
                SET page_id = $2,
                    metadata = jsonb_set(metadata, '{updated_at}', to_jsonb(now()))
                WHERE page_id = $1
                  AND (metadata->>'deleted_at') IS NULL
                "#,
            )
            .bind(id)
            .bind(g.id)
            .execute(&mut *tx)
            .await?;
        }
        // Menus with no generic to fall back to lose the binding.
        sqlx::query(
            r#"
            UPDATE website.menus
            SET page_id = NULL,
                metadata = jsonb_set(metadata, '{updated_at}', to_jsonb(now()))
            WHERE page_id = $1
              AND (metadata->>'deleted_at') IS NULL
            "#,
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    /// Ordered blocks of a page (admin read; also the public read's
    /// block arm).
    pub async fn page_blocks(&self, page_id: Uuid) -> Result<Vec<BlockView>, WebsiteError> {
        let blocks = sqlx::query_as::<_, BlockView>(
            r#"
            SELECT kind::text AS kind, position, payload
            FROM website.page_blocks
            WHERE page_id = $1
            ORDER BY position
            "#,
        )
        .bind(page_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(blocks)
    }

    /// THE PUBLIC PAGE READ: resolve on the fold (live rows), then the
    /// §3 predicate — unpublished or future-dated → `Ok(None)` (the
    /// 404 hides existence); published but tier-refused → the typed
    /// 403. Off-website rows are structurally invisible: the fold
    /// never returns them.
    pub async fn visible_page(
        &self,
        website_id: Uuid,
        url: &str,
        principal: Option<Uuid>,
    ) -> Result<Option<PublicPage>, WebsiteError> {
        let resolution = resolve_page_by_url(&self.pool, url, website_id).await?;
        let row = match resolution {
            Resolution::None => return Ok(None),
            Resolution::Specific(r) => r,
            Resolution::Generic(r) => r,
        };
        if !row.is_published {
            return Ok(None);
        }
        if let Some(at) = row.date_publish {
            if at > chrono::Utc::now() {
                return Ok(None);
            }
        }
        if !tier_passes(
            &self.pool,
            &row.visibility,
            row.required_member_roles.as_deref().unwrap_or(&[]),
            website_id,
            principal,
        )
        .await?
        {
            return Err(WebsiteError::PageVisibilityRefused);
        }
        let blocks = self.page_blocks(row.id).await?;
        Ok(Some(PublicPage { page: row, blocks }))
    }

    /// The sitemap arm (cursor-paginated, capped by the caller).
    pub async fn sitemap_page(
        &self,
        website_id: Uuid,
        cursor: Option<&str>,
        limit: i64,
    ) -> Result<Vec<ResolvedPage>, WebsiteError> {
        resolve_sitemap_page(&self.pool, website_id, cursor, limit).await
    }
}

fn patch_field_names(patch: &PagePatch) -> Vec<&'static str> {
    let mut names = Vec::new();
    if patch.url.is_some() {
        names.push("url");
    }
    if patch.title.is_some() {
        names.push("title");
    }
    if patch.seo_name.is_some() {
        names.push("seo_name");
    }
    if patch.visibility.is_some() {
        names.push("visibility");
    }
    if patch.required_member_roles.is_some() {
        names.push("required_member_roles");
    }
    if patch.website_indexed.is_some() {
        names.push("website_indexed");
    }
    names
}
