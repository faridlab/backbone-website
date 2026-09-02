//! Redirect service (hand-written; user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! The generated CRUD alias first (keeps lib.rs's wiring compiling),
//! then the hand redirect verbs: validated CRUD (the `alias_308`
//! param-parity rule, target-required-unless-gone), the routing-table
//! answer the matcher consumes, and the exported `record_redirect`
//! seam blog's stale-slug 301s write through. One redirect answer per
//! path per website — the hardening partial unique holds that.

use sqlx::PgPool;
use uuid::Uuid;

use backbone_core::GenericCrudService;
use crate::domain::entity::Redirect;
use crate::infrastructure::persistence::RedirectRepository;
use crate::presentation::dto::{CreateRedirectDto, UpdateRedirectDto};

/// Generated CRUD alias (the generator skipped emitting this file
/// because it is user-owned).
pub type RedirectService = GenericCrudService<
    Redirect,
    CreateRedirectDto,
    UpdateRedirectDto,
    RedirectRepository,
>;

use super::lang_matcher::RedirectAnswer as MatcherRedirectAnswer;
use super::website_error::WebsiteError;
use super::website_service::{record_audit, ActorRef};

/// The closed redirect-type vocabulary.
pub const REDIRECT_TYPES: &[&str] = &["moved_301", "found_302", "alias_308", "gone_404"];

/// The query-parameter NAMES of a site-absolute url (order-insensitive,
/// value-insensitive).
fn param_names(url: &str) -> Vec<String> {
    match url.split_once('?') {
        Some((_, q)) => q
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|p| p.split('=').next().unwrap_or("").to_string())
            .collect(),
        None => Vec::new(),
    }
}

/// The `alias_308` parity rule: source and target carry the SAME query
/// parameter name SET (a 308 preserves method AND body; the target must
/// be able to accept every parameter the source accepted).
pub fn param_parity_holds(url_from: &str, url_to: &str) -> bool {
    let mut from = param_names(url_from);
    let mut to = param_names(url_to);
    from.sort();
    to.sort();
    from == to
}

fn validate_redirect(redirect_type: &str, url_from: &str, url_to: &Option<String>) -> Result<(), WebsiteError> {
    if !url_from.starts_with('/') {
        return Err(WebsiteError::InvalidInput(
            "url_from must be a site-absolute path starting with '/'".into(),
        ));
    }
    if !REDIRECT_TYPES.contains(&redirect_type) {
        return Err(WebsiteError::InvalidInput(format!(
            "unknown redirect type {redirect_type:?} ({REDIRECT_TYPES:?})"
        )));
    }
    match (redirect_type, url_to) {
        ("gone_404", Some(_)) => Err(WebsiteError::InvalidInput(
            "a gone_404 redirect carries no target".into(),
        )),
        ("gone_404", None) => Ok(()),
        (_, None) => Err(WebsiteError::RedirectTargetRequired),
        ("alias_308", Some(to)) => {
            if param_parity_holds(url_from, to) {
                Ok(())
            } else {
                Err(WebsiteError::RedirectParamParity)
            }
        }
        (_, Some(to)) => {
            if to.starts_with('/') {
                Ok(())
            } else {
                Err(WebsiteError::InvalidInput(
                    "url_to must be a site-absolute path starting with '/'".into(),
                ))
            }
        }
    }
}

/// Create input.
#[derive(Debug, Clone)]
pub struct CreateRedirectInput {
    pub website_id: Uuid,
    pub url_from: String,
    pub redirect_type: String,
    pub url_to: Option<String>,
    pub description: Option<String>,
}

/// The typed patch whitelist.
#[derive(Debug, Clone, Default)]
pub struct RedirectPatch {
    pub url_from: Option<String>,
    pub redirect_type: Option<String>,
    pub url_to: Option<Option<String>>,
    pub description: Option<Option<String>>,
}

impl RedirectPatch {
    pub fn is_empty(&self) -> bool {
        self.url_from.is_none()
            && self.redirect_type.is_none()
            && self.url_to.is_none()
            && self.description.is_none()
    }
}

/// The hand redirect verbs.
pub struct RedirectAdminService {
    pool: PgPool,
}

impl RedirectAdminService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Officer create — validated (308 parity, target rules); a
    /// second answer for one path maps to the typed 409. Audits
    /// `redirect_created`.
    pub async fn create(&self, actor: ActorRef, input: CreateRedirectInput) -> Result<Redirect, WebsiteError> {
        validate_redirect(&input.redirect_type, &input.url_from, &input.url_to)?;
        let row = sqlx::query_as::<_, Redirect>(
            r#"
            INSERT INTO website.redirects
                (id, website_id, url_from, redirect_type, url_to, description, metadata)
            VALUES ($1, $2, $3, $4::website_redirect_type, $5, $6,
                    jsonb_build_object('created_at', now(), 'created_by', $7))
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(input.website_id)
        .bind(input.url_from.trim())
        .bind(&input.redirect_type)
        .bind(input.url_to)
        .bind(input.description)
        .bind(actor.stamp())
        .fetch_one(&self.pool)
        .await
        .map_err(super::website_error::map_unique_violation)?;
        record_audit(
            &self.pool,
            "redirect_created",
            actor,
            Some("redirect"),
            Some(row.id),
            Some(serde_json::json!({ "url_from": row.url_from, "kind": row.redirect_type.to_string() })),
        )
        .await?;
        Ok(row)
    }

    /// Officer patch — re-validated against the RESULTING row. Audits
    /// `redirect_updated`.
    // The terminal set_arm arm's flag assignment is dead by construction.
    #[allow(unused_assignments)]
    pub async fn patch(&self, actor: ActorRef, id: Uuid, patch: RedirectPatch) -> Result<Redirect, WebsiteError> {
        if patch.is_empty() {
            return Err(WebsiteError::InvalidInput(
                "the redirect patch sets no field".into(),
            ));
        }
        let current: Option<Redirect> = sqlx::query_as::<_, Redirect>(
            "SELECT * FROM website.redirects WHERE id = $1 \
             AND (metadata->>'deleted_at') IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(current) = current else {
            return Err(WebsiteError::NotFound(format!("redirect {id}")));
        };
        let url_from = patch.url_from.clone().unwrap_or_else(|| current.url_from.clone());
        let redirect_type = patch
            .redirect_type
            .clone()
            .unwrap_or_else(|| current.redirect_type.to_string());
        let url_to = match patch.url_to.clone() {
            Some(t) => t,
            None => current.url_to.clone(),
        };
        validate_redirect(&redirect_type, &url_from, &url_to)?;

        use sqlx::QueryBuilder;
        let mut qb = QueryBuilder::new("UPDATE website.redirects SET ");
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
        set_arm!("url_from", patch.url_from.clone());
        if let Some(v) = patch.redirect_type.clone() {
            if !first {
                qb.push(", ");
            }
            qb.push("redirect_type = ").push_bind(v).push("::website_redirect_type");
            first = false;
        }
        set_arm!("url_to", patch.url_to.clone());
        set_arm!("description", patch.description.clone());
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
        let row = qb
            .build_query_as::<Redirect>()
            .fetch_one(&self.pool)
            .await
            .map_err(super::website_error::map_unique_violation)?;
        record_audit(
            &self.pool,
            "redirect_updated",
            actor,
            Some("redirect"),
            Some(id),
            None,
        )
        .await?;
        Ok(row)
    }

    /// Officer delete. Audits `redirect_deleted`.
    pub async fn delete(&self, actor: ActorRef, id: Uuid) -> Result<(), WebsiteError> {
        let existing: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM website.redirects WHERE id = $1 \
             AND (metadata->>'deleted_at') IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        if existing.is_none() {
            return Err(WebsiteError::NotFound(format!("redirect {id}")));
        }
        sqlx::query(
            r#"
            UPDATE website.redirects
            SET metadata = jsonb_set(jsonb_set(metadata, '{deleted_at}', to_jsonb(now())),
                                     '{deleted_by}', to_jsonb($2))
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(actor.stamp())
        .execute(&self.pool)
        .await?;
        record_audit(&self.pool, "redirect_deleted", actor, Some("redirect"), Some(id), None).await?;
        Ok(())
    }

    /// Officer list for one website.
    pub async fn list(&self, website_id: Uuid) -> Result<Vec<Redirect>, WebsiteError> {
        let rows = sqlx::query_as::<_, Redirect>(
            r#"
            SELECT * FROM website.redirects
            WHERE website_id = $1 AND (metadata->>'deleted_at') IS NULL
            ORDER BY url_from
            "#,
        )
        .bind(website_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// The routing-table answer for one path on one website — the
    /// matcher's case-7 input. Fresh read, no cache.
    pub async fn answer(&self, website_id: Uuid, url: &str) -> Result<Option<MatcherRedirectAnswer>, WebsiteError> {
        let row = sqlx::query_as::<_, (String, Option<String>)>(
            r#"
            SELECT redirect_type::text, url_to
            FROM website.redirects
            WHERE website_id = $1 AND url_from = $2
              AND (metadata->>'deleted_at') IS NULL
            "#,
        )
        .bind(website_id)
        .bind(url)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|(redirect_type, url_to)| MatcherRedirectAnswer { redirect_type, url_to }))
    }

    /// The exported record seam (blog stale-slug 301s): idempotent
    /// upsert of one redirect answer per path per website.
    pub async fn record_redirect(
        &self,
        website_id: Uuid,
        url_from: &str,
        url_to: &str,
        kind: &str,
    ) -> Result<(), WebsiteError> {
        let target = if kind == "gone_404" { None } else { Some(url_to.to_string()) };
        validate_redirect(kind, url_from, &target)?;
        sqlx::query(
            r#"
            INSERT INTO website.redirects
                (id, website_id, url_from, redirect_type, url_to, metadata)
            VALUES (gen_random_uuid(), $1, $2, $3::website_redirect_type, $4,
                    jsonb_build_object('created_at', now()))
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(website_id)
        .bind(url_from)
        .bind(kind)
        .bind(target)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_parity_rules() {
        assert!(param_parity_holds("/a?x=1&y=2", "/b?y=9&x=3"));
        assert!(param_parity_holds("/a", "/b"));
        assert!(!param_parity_holds("/a?x=1", "/b?x=1&y=2"));
        assert!(!param_parity_holds("/a?x=1&z=3", "/b?x=1"));
    }

    #[test]
    fn gone_requires_no_target_and_others_do() {
        assert!(validate_redirect("gone_404", "/old", &None).is_ok());
        assert!(matches!(
            validate_redirect("moved_301", "/old", &None),
            Err(WebsiteError::RedirectTargetRequired)
        ));
        assert!(validate_redirect("alias_308", "/a?x=1", &Some("/b?x=2".into())).is_ok());
        assert!(matches!(
            validate_redirect("alias_308", "/a?x=1", &Some("/b?y=2".into())),
            Err(WebsiteError::RedirectParamParity)
        ));
    }
}
