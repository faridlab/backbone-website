//! The ONE generic-vs-specific resolver (hand-written; user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! ONE resolver for the whole module: every generic/specific read in
//! every service routes through this file. The grep invariant holds by
//! construction — `website_id IS NULL` appears NOWHERE else in `src/`,
//! and `fn resolve_` is defined nowhere else.
//!
//! The shared core:
//!
//! ```sql
//! WHERE key = $1
//!   AND (website_id = $2 OR website_id IS NULL)
//!   AND (metadata->>'deleted_at') IS NULL
//! ORDER BY website_id NULLS LAST
//! LIMIT 1
//! ```
//!
//! A specific row for THIS website always wins; the generic backs it.
//! The partial sentinel uniques (the hardening migration) make two live
//! rows on one grain impossible, so the fold is deterministic; the
//! resolver never copies rows — copying is exclusively the fork verb.

use uuid::Uuid;

use crate::domain::entity::Page;

use super::website_error::WebsiteError;

/// A reusable SQL fragment (the fold's WHERE arm). Exposed so the
/// module's own reads compose the SAME grain text; nothing outside
/// this file may write the NULL arm by hand.
#[derive(Debug, Clone, Copy)]
pub struct ScopeFragment(pub &'static str);

impl std::fmt::Display for ScopeFragment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// The ORDER arm: the specific always wins.
#[derive(Debug, Clone, Copy)]
pub struct OrderFragment(pub &'static str);

impl std::fmt::Display for OrderFragment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// WHERE fragment binding the generic/specific grain: the website's own
/// row or the generic row. Parameter slot 2 assumes the fold's canonical
/// shape (`key = $1 AND <scope>`).
pub fn live_scope(_website_id: Uuid) -> ScopeFragment {
    ScopeFragment("(website_id = $2 OR website_id IS NULL)")
}

/// ORDER fragment: `website_id NULLS LAST` — the specific outranks the
/// generic because a non-NULL uuid sorts before NULL under NULLS LAST.
pub fn prefer_specific() -> OrderFragment {
    OrderFragment("website_id NULLS LAST")
}

/// Row shape returned by the resolver reads — the columns the public
/// page verb serves.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ResolvedPage {
    pub id: Uuid,
    pub key: String,
    pub website_id: Option<Uuid>,
    pub url: String,
    pub title: String,
    pub seo_name: Option<String>,
    pub is_published: bool,
    pub date_publish: Option<chrono::DateTime<chrono::Utc>>,
    pub visibility: String,
    pub website_indexed: bool,
    pub required_member_roles: Option<Vec<String>>,
}

impl ResolvedPage {
    /// True when this resolved row is the website's SPECIFIC row (not
    /// the generic fallback).
    pub fn is_specific(&self) -> bool {
        self.website_id.is_some()
    }
}

/// Which arm answered — callers never re-derive it.
#[derive(Debug, Clone)]
pub enum Resolution {
    Specific(ResolvedPage),
    Generic(ResolvedPage),
    /// Neither arm holds a live row.
    None,
}

impl Resolution {
    pub fn row(&self) -> Option<&ResolvedPage> {
        match self {
            Resolution::Specific(r) | Resolution::Generic(r) => Some(r),
            Resolution::None => None,
        }
    }
}

// `visibility::text` — the row shape carries the tier as its string
// label; the raw enum type would refuse the String decode.
const RESOLVE_COLUMNS: &str = "id, key, website_id, url, title, seo_name, \
     is_published, date_publish, visibility::text AS visibility, \
     website_indexed, required_member_roles";

/// Resolve THE most-specific live row for `key` on `website`:
/// Specific(key, W), else Generic(key, NULL), else `Resolution::None`.
/// Accepts a pool or a transaction (`&mut PgConnection`).
pub async fn resolve_specific(
    exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    key: &str,
    website_id: Uuid,
) -> Result<Resolution, WebsiteError> {
    let row = sqlx::query_as::<_, ResolvedPage>(&format!(
        "SELECT {RESOLVE_COLUMNS} FROM website.pages \
         WHERE key = $1 AND ({}) AND (metadata->>'deleted_at') IS NULL \
         ORDER BY {} LIMIT 1",
        live_scope(website_id),
        prefer_specific()
    ))
    .bind(key)
    .bind(website_id)
    .fetch_optional(exec)
    .await?;
    Ok(classify(row))
}

/// Resolve a page by routing url on a website — the SAME fold over the
/// url arm. Used by the public page read and the redirect/canonical
/// chain. Live rows only; publication and tier checks are the read
/// verbs' concern (the resolver is purely the specificity fold).
pub async fn resolve_page_by_url(
    exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    url: &str,
    website_id: Uuid,
) -> Result<Resolution, WebsiteError> {
    let row = sqlx::query_as::<_, ResolvedPage>(&format!(
        "SELECT {RESOLVE_COLUMNS} FROM website.pages \
         WHERE url = $1 AND ({}) AND (metadata->>'deleted_at') IS NULL \
         ORDER BY {} LIMIT 1",
        live_scope(website_id),
        prefer_specific()
    ))
    .bind(url)
    .bind(website_id)
    .fetch_optional(exec)
    .await?;
    Ok(classify(row))
}

fn classify(row: Option<ResolvedPage>) -> Resolution {
    match row {
        Some(r) if r.website_id.is_some() => Resolution::Specific(r),
        Some(r) => Resolution::Generic(r),
        None => Resolution::None,
    }
}

/// The sitemap read: the fold over the whole live published set — per
/// key, the specific-first pick for THIS website — then cursor-paginated
/// by url. `after_url` is the exclusive cursor (the last url served);
/// `limit` is the page size (the route caps at 45,000).
pub async fn resolve_sitemap_page(
    exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    website_id: Uuid,
    after_url: Option<&str>,
    limit: i64,
) -> Result<Vec<ResolvedPage>, WebsiteError> {
    let rows = sqlx::query_as::<_, ResolvedPage>(&format!(
        r#"
        SELECT {RESOLVE_COLUMNS} FROM (
            SELECT DISTINCT ON (key) {RESOLVE_COLUMNS}
            FROM website.pages
            WHERE ({0})
              AND is_published
              AND visibility = 'public'
              AND (date_publish IS NULL OR date_publish <= now())
              AND website_indexed
              AND (metadata->>'deleted_at') IS NULL
            ORDER BY key, {1}
        ) picked
        WHERE ($2::text IS NULL OR picked.url > $2)
        ORDER BY picked.url
        LIMIT $3
        "#,
        // Inner query numbers the website binding $1 (no key predicate
        // on the set read).
        "(website_id = $1 OR website_id IS NULL)",
        prefer_specific()
    ))
    .bind(website_id)
    .bind(after_url)
    .bind(limit)
    .fetch_all(exec)
    .await?;
    Ok(rows)
}

/// Live-row filter reused by the hand verbs (soft-delete aware).
pub fn live_filter() -> &'static str {
    "(metadata->>'deleted_at') IS NULL"
}

/// Load the GENERIC row (the NULL-website arm) for `key` as the full
/// generated entity — the fork verb's source loader. Lives here so the
/// NULL-arm predicate text appears nowhere else.
pub async fn resolve_generic(
    exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    key: &str,
) -> Result<Option<Page>, WebsiteError> {
    let row = sqlx::query_as::<_, Page>(
        "SELECT * FROM website.pages \
         WHERE key = $1 AND website_id IS NULL \
         AND (metadata->>'deleted_at') IS NULL",
    )
    .bind(key)
    .fetch_optional(exec)
    .await?;
    Ok(row)
}

/// Load the SPECIFIC row for (key, website) as the full entity — the
/// fork verb's idempotence re-select. Exact-arm lookup; no fold.
pub async fn resolve_specific_entity(
    exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    key: &str,
    website_id: Uuid,
) -> Result<Option<Page>, WebsiteError> {
    let row = sqlx::query_as::<_, Page>(
        "SELECT * FROM website.pages \
         WHERE key = $1 AND website_id = $2 \
         AND (metadata->>'deleted_at') IS NULL",
    )
    .bind(key)
    .bind(website_id)
    .fetch_optional(exec)
    .await?;
    Ok(row)
}

/// The fold as a SET read: the effective page per key for one website
/// (specific-first), full entities, unpublished included — the officer
/// list. One query; the fold text lives only here.
pub async fn resolve_effective_pages(
    exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    website_id: Uuid,
) -> Result<Vec<Page>, WebsiteError> {
    let rows = sqlx::query_as::<_, Page>(
        r#"
        SELECT DISTINCT ON (key) *
        FROM website.pages
        WHERE (website_id = $1 OR website_id IS NULL)
          AND (metadata->>'deleted_at') IS NULL
        ORDER BY key, website_id NULLS LAST
        "#,
    )
    .bind(website_id)
    .fetch_all(exec)
    .await?;
    Ok(rows)
}

/// Loader bridging the resolver's row shape to the generated entity
/// (officer verbs need the full entity, not just public columns).
pub async fn page_entity_by_id(
    exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    id: Uuid,
) -> Result<Option<Page>, WebsiteError> {
    let row = sqlx::query_as::<_, Page>(
        "SELECT * FROM website.pages WHERE id = $1 AND (metadata->>'deleted_at') IS NULL",
    )
    .bind(id)
    .fetch_optional(exec)
    .await?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_fragment_is_the_null_tolerant_grain() {
        assert_eq!(
            live_scope(Uuid::nil()).to_string(),
            "(website_id = $2 OR website_id IS NULL)"
        );
    }

    #[test]
    fn order_fragment_prefers_specific() {
        assert_eq!(prefer_specific().to_string(), "website_id NULLS LAST");
    }

    #[test]
    fn resolution_classifies_arms() {
        let mut r = ResolvedPage {
            id: Uuid::nil(),
            key: "k".into(),
            website_id: None,
            url: "/u".into(),
            title: "t".into(),
            seo_name: None,
            is_published: true,
            date_publish: None,
            visibility: "public".into(),
            website_indexed: true,
            required_member_roles: None,
        };
        assert!(matches!(classify(Some(r.clone())), Resolution::Generic(_)));
        r.website_id = Some(Uuid::nil());
        assert!(matches!(classify(Some(r)), Resolution::Specific(_)));
        assert!(matches!(classify(None), Resolution::None));
    }
}
