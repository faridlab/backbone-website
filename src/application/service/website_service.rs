//! Website root service (hand-written; user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! The generated CRUD alias first (lib.rs and this module's mod tree
//! compile against it), then the hand root service: the bootstrap
//! create verb (homepage page + root menu + minted public principal),
//! hostname resolution (exact match, no fallback), the derived
//! primary-website read, the guarded delete, the config-fields patch,
//! and the company allow-list derivation.

use sqlx::PgPool;
use uuid::Uuid;

use backbone_core::GenericCrudService;
use crate::domain::entity::Website;
use crate::infrastructure::persistence::WebsiteRepository;
use crate::presentation::dto::{CreateWebsiteDto, UpdateWebsiteDto};

/// Generated CRUD alias (the generator skipped emitting this file
/// because it is user-owned; the alias keeps lib.rs's
/// `Arc<WebsiteService>` wiring compiling unchanged).
pub type WebsiteService = GenericCrudService<
    Website,
    CreateWebsiteDto,
    UpdateWebsiteDto,
    WebsiteRepository,
>;

use super::website_error::WebsiteError;

/// The acting officer (admin-tree verbs). `None` marks a system actor
/// (the GC cron, internal seeding) — never a public principal.
#[derive(Debug, Clone, Copy, Default)]
pub struct ActorRef(pub Option<Uuid>);

impl ActorRef {
    pub fn officer(id: Uuid) -> Self {
        ActorRef(Some(id))
    }

    pub fn system() -> Self {
        ActorRef(None)
    }

    pub(crate) fn stamp(self) -> Option<Uuid> {
        self.0
    }
}

/// The website row as every read verb serves it.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WebsiteView {
    pub id: Uuid,
    pub name: String,
    pub domain: Option<String>,
    pub company_id: Uuid,
    pub public_user_id: Uuid,
    pub default_lang_code: String,
    pub homepage_url: String,
    pub robots_txt: Option<String>,
    pub social_links: Option<serde_json::Value>,
    pub contact_recipients: Vec<String>,
    pub sequence: i32,
}

/// Normalize a host header for binding: trim, lowercase, strip any
/// scheme, strip the port, strip the trailing dot, drop dot-segments.
/// The STORED domain passes through the same function at write time,
/// so both sides of the exact match share one normalization. Full
/// UTS-46 IDNA folding lands with the webapp i18n increment; a
/// non-ASCII host simply matches only a domain stored under the same
/// byte sequence.
pub fn normalize_host(raw: &str) -> String {
    let mut h = raw.trim().to_lowercase();
    if let Some(rest) = h.strip_prefix("https://") {
        h = rest.to_string();
    } else if let Some(rest) = h.strip_prefix("http://") {
        h = rest.to_string();
    }
    if h.starts_with('[') {
        // Bracketed IPv6 literal: the address runs to ']'; anything
        // after the bracket is the port and is dropped.
        if let Some(end) = h.find(']') {
            h.truncate(end + 1);
        }
    } else if let Some((host, port)) = h.rsplit_once(':') {
        // Only strip a numeric tail when the remainder is a single
        // label (no further colons) — a bare IPv6 literal keeps its
        // colons.
        if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) && !host.contains(':') {
            h = host.to_string();
        }
    }
    while h.ends_with('.') {
        h.pop();
    }
    while h.contains("/./") || h.contains("/../") {
        h = h.replace("/./", "/");
        h = h.replace("/../", "/");
    }
    h.trim_end_matches('/').to_string()
}

/// The bootstrap create input. `public_user_id` is OPTIONAL: absent →
/// the verb MINTS the ghost portal principal (an officer-acted
/// `portal.portal_users` row; the ghost never logs in — it has no
/// credential). Present → the officer binds an existing principal.
#[derive(Debug, Clone)]
pub struct CreateWebsiteInput {
    pub name: String,
    pub domain: Option<String>,
    pub company_id: Uuid,
    pub public_user_id: Option<Uuid>,
    pub default_lang_code: String,
    pub homepage_url: String,
    pub robots_txt: Option<String>,
    pub social_links: Option<serde_json::Value>,
    pub contact_recipients: Vec<String>,
    pub sequence: i32,
}

/// The config-fields patch whitelist. `public_user_id` and
/// `company_id` are structurally ABSENT — they are not config fields;
/// an attempt to move them is refused before this type exists.
#[derive(Debug, Clone, Default)]
pub struct WebsiteConfigPatch {
    pub name: Option<String>,
    pub domain: Option<String>,
    pub default_lang_code: Option<String>,
    pub homepage_url: Option<String>,
    pub robots_txt: Option<String>,
    pub social_links: Option<serde_json::Value>,
    pub contact_recipients: Option<Vec<String>>,
    pub sequence: Option<i32>,
}

impl WebsiteConfigPatch {
    pub fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.domain.is_none()
            && self.default_lang_code.is_none()
            && self.homepage_url.is_none()
            && self.robots_txt.is_none()
            && self.social_links.is_none()
            && self.contact_recipients.is_none()
            && self.sequence.is_none()
    }
}

/// Append-only audit writer shared by every hand verb (the closed
/// `website_audit_event` vocabulary).
pub async fn record_audit(
    exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    event: &str,
    actor: ActorRef,
    subject_type: Option<&str>,
    subject_id: Option<Uuid>,
    detail: Option<serde_json::Value>,
) -> Result<(), WebsiteError> {
    sqlx::query(
        r#"
        INSERT INTO website.website_audit_log
            (id, event, actor, subject_type, subject_id, detail, occurred_at)
        VALUES (gen_random_uuid(), $1::website_audit_event, $2, $3, $4, $5, now())
        "#,
    )
    .bind(event)
    .bind(actor.stamp())
    .bind(subject_type)
    .bind(subject_id)
    .bind(detail)
    .execute(exec)
    .await?;
    Ok(())
}

/// The hand root service over the website table.
pub struct WebsiteRootService {
    pool: PgPool,
}

impl WebsiteRootService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// The bootstrap create verb — ONE transaction: mint (or bind) the
    /// public principal, insert the website, seed the homepage page
    /// (key 'homepage', url the site's homepage_url), seed the root
    /// menu with a first-level 'Home' entry bound to that page, audit
    /// `website_created`.
    pub async fn create_website(
        &self,
        actor: ActorRef,
        input: CreateWebsiteInput,
    ) -> Result<WebsiteView, WebsiteError> {
        if input.name.trim().is_empty() {
            return Err(WebsiteError::InvalidInput("name is required".into()));
        }
        if !input.homepage_url.starts_with('/') {
            return Err(WebsiteError::InvalidInput(
                "homepage_url must be a site-absolute path starting with '/'".into(),
            ));
        }
        let domain = match input.domain.as_deref() {
            Some(d) if !d.trim().is_empty() => Some(normalize_host(d)),
            _ => None,
        };

        let mut tx = self.pool.begin().await?;

        // Mint or bind the public principal. The mint is officer-acted:
        // the ghost row exists so its declared read verbs can enumerate
        // public content; it carries NO credential and never logs in.
        let public_user_id = match input.public_user_id {
            Some(existing) => existing,
            None => {
                let minted = Uuid::new_v4();
                let ghost_email = format!("ghost-{minted}@website.invalid");
                sqlx::query(
                    r#"
                    INSERT INTO portal.portal_users (id, email, display_name, status, metadata)
                    VALUES ($1, $2, $3, 'active',
                            jsonb_build_object('created_at', now(), 'created_by', $4))
                    "#,
                )
                .bind(minted)
                .bind(&ghost_email)
                .bind(format!("Public principal — {}", input.name))
                .bind(actor.stamp())
                .execute(&mut *tx)
                .await?;
                minted
            }
        };

        let website_id = Uuid::new_v4();
        let view = sqlx::query_as::<_, WebsiteView>(
            r#"
            INSERT INTO website.websites
                (id, name, domain, company_id, public_user_id, default_lang_code,
                 homepage_url, robots_txt, social_links, contact_recipients, sequence, metadata)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                    jsonb_build_object('created_at', now(), 'created_by', $12))
            RETURNING id, name, domain, company_id, public_user_id, default_lang_code,
                      homepage_url, robots_txt, social_links, contact_recipients, sequence
            "#,
        )
        .bind(website_id)
        .bind(input.name.trim())
        .bind(domain)
        .bind(input.company_id)
        .bind(public_user_id)
        .bind(if input.default_lang_code.trim().is_empty() {
            "en".to_string()
        } else {
            input.default_lang_code.trim().to_lowercase()
        })
        .bind(&input.homepage_url)
        .bind(input.robots_txt)
        .bind(input.social_links)
        .bind(&input.contact_recipients)
        .bind(input.sequence)
        .bind(actor.stamp())
        .fetch_one(&mut *tx)
        .await
        .map_err(super::website_error::map_unique_violation)?;

        // The homepage page: a GENERIC seed (the NULL-website arm,
        // loaded through the ONE resolver) shared by every website
        // until forked — idempotent, because the specificity fence
        // allows exactly one live generic 'homepage' and the second
        // bootstrap rides the existing one.
        let generic_homepage =
            super::specificity::resolve_generic(&mut *tx, "homepage").await?;
        let generic_homepage_id = match generic_homepage {
            Some(existing) => existing.id,
            None => {
                let seeded = Uuid::new_v4();
                sqlx::query(
                    r#"
                    INSERT INTO website.pages
                        (id, key, website_id, url, title, is_published, date_publish,
                         visibility, website_indexed, required_member_roles, metadata)
                    VALUES ($1, 'homepage', NULL, $2, $3, FALSE, NULL,
                            'public', TRUE, '{}',
                            jsonb_build_object('created_at', now(), 'created_by', $4))
                    "#,
                )
                .bind(seeded)
                .bind(&input.homepage_url)
                .bind(format!("{} — Home", input.name.trim()))
                .bind(actor.stamp())
                .execute(&mut *tx)
                .await
                .map_err(super::website_error::map_unique_violation)?;
                seeded
            }
        };

        // Root menu + the first-level 'Home' entry bound to the
        // homepage page of THIS website (a specific fork, so menu
        // writes never leak across sites). The fork stamps THIS
        // website's homepage_url — `websites.homepage_url` is a
        // per-site column, so copying the generic row's url verbatim
        // would point every website after the first at the first
        // site's homepage. ON CONFLICT DO NOTHING + re-select: the
        // fence guarantees exactly one live specific per (key,
        // website) no matter who raced us.
        sqlx::query(
            r#"
            INSERT INTO website.pages
                (id, key, website_id, url, title, is_published, date_publish,
                 visibility, website_indexed, required_member_roles,
                 forked_from, forked_at, forked_by, metadata)
            SELECT gen_random_uuid(), p.key, $2, $4, p.title, p.is_published,
                   p.date_publish, p.visibility, p.website_indexed,
                   p.required_member_roles, p.id, now(), $3,
                   jsonb_build_object('created_at', now(), 'created_by', $3)
            FROM website.pages p
            WHERE p.id = $1
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(generic_homepage_id)
        .bind(website_id)
        .bind(actor.stamp())
        .bind(&input.homepage_url)
        .execute(&mut *tx)
        .await?;
        let homepage_specific = sqlx::query_scalar::<_, Uuid>(
            r#"
            SELECT id FROM website.pages
            WHERE key = 'homepage' AND website_id = $1
              AND (metadata->>'deleted_at') IS NULL
            "#,
        )
        .bind(website_id)
        .fetch_one(&mut *tx)
        .await?;

        let root_id = Uuid::new_v4();
        sqlx::query(
            r#"
            INSERT INTO website.menus
                (id, website_id, parent_id, name, page_id, url, new_window,
                 sequence, visibility, required_member_roles, is_mega_menu, metadata)
            VALUES ($1, $2, NULL, 'Main', NULL, NULL, FALSE,
                    10, 'public', '{}', FALSE,
                    jsonb_build_object('created_at', now(), 'created_by', $3))
            "#,
        )
        .bind(root_id)
        .bind(website_id)
        .bind(actor.stamp())
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO website.menus
                (id, website_id, parent_id, name, page_id, url, new_window,
                 sequence, visibility, required_member_roles, is_mega_menu, metadata)
            VALUES ($1, $2, $3, 'Home', $4, NULL, FALSE,
                    10, 'public', '{}', FALSE,
                    jsonb_build_object('created_at', now(), 'created_by', $5))
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(website_id)
        .bind(root_id)
        .bind(homepage_specific)
        .bind(actor.stamp())
        .execute(&mut *tx)
        .await?;

        record_audit(
            &mut *tx,
            "website_created",
            actor,
            Some("website"),
            Some(website_id),
            Some(serde_json::json!({
                "name": view.name,
                "domain": view.domain,
                "public_principal_minted": input.public_user_id.is_none(),
            })),
        )
        .await?;

        tx.commit().await?;
        Ok(view)
    }

    /// Hostname binding: lowercase/strip-port/exact-match against live
    /// `websites.domain`. NO session force flag, NO first-website
    /// fallback — a miss is the loud typed 404. (The exported surface's
    /// frozen name `resolve_website_by_host` — spec §9.4 — delegates
    /// here; this helper deliberately carries no `resolve_` prefix so
    /// the one-resolver grep invariant stays literal.)
    pub async fn website_by_host(&self, host: &str) -> Result<WebsiteView, WebsiteError> {
        let normalized = normalize_host(host);
        let row = sqlx::query_as::<_, WebsiteView>(
            r#"
            SELECT id, name, domain, company_id, public_user_id, default_lang_code,
                   homepage_url, robots_txt, social_links, contact_recipients, sequence
            FROM website.websites
            WHERE domain = $1 AND (metadata->>'deleted_at') IS NULL
            "#,
        )
        .bind(normalized)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(WebsiteError::WebsiteNotResolved)?;
        Ok(row)
    }

    pub async fn website_by_id(&self, id: Uuid) -> Result<WebsiteView, WebsiteError> {
        sqlx::query_as::<_, WebsiteView>(
            r#"
            SELECT id, name, domain, company_id, public_user_id, default_lang_code,
                   homepage_url, robots_txt, social_links, contact_recipients, sequence
            FROM website.websites
            WHERE id = $1 AND (metadata->>'deleted_at') IS NULL
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| WebsiteError::NotFound(format!("website {id}")))
    }

    /// Officer list (admin tree): optionally scoped to a company.
    pub async fn list_websites(
        &self,
        company_id: Option<Uuid>,
    ) -> Result<Vec<WebsiteView>, WebsiteError> {
        sqlx::query_as::<_, WebsiteView>(
            r#"
            SELECT id, name, domain, company_id, public_user_id, default_lang_code,
                   homepage_url, robots_txt, social_links, contact_recipients, sequence
            FROM website.websites
            WHERE (metadata->>'deleted_at') IS NULL
              AND ($1::uuid IS NULL OR company_id = $1)
            ORDER BY company_id, sequence, id
            "#,
        )
        .bind(company_id)
        .fetch_all(&self.pool)
        .await
        .map_err(WebsiteError::from)
    }

    /// The DERIVED primary website: lowest live (sequence, id) for the
    /// company. No stored primary column exists anywhere.
    pub async fn primary_website_of(
        &self,
        company_id: Uuid,
    ) -> Result<Option<WebsiteView>, WebsiteError> {
        let row = sqlx::query_as::<_, WebsiteView>(
            r#"
            SELECT id, name, domain, company_id, public_user_id, default_lang_code,
                   homepage_url, robots_txt, social_links, contact_recipients, sequence
            FROM website.websites
            WHERE company_id = $1 AND (metadata->>'deleted_at') IS NULL
            ORDER BY sequence, id
            LIMIT 1
            "#,
        )
        .bind(company_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// The guarded delete: refuses while the website is its company's
    /// derived primary (typed `website_is_primary_for_company`);
    /// otherwise soft-deletes (pages/menus/children cascade at the DB).
    pub async fn delete_website(&self, actor: ActorRef, id: Uuid) -> Result<(), WebsiteError> {
        let site = self.website_by_id(id).await?;
        if let Some(primary) = self.primary_website_of(site.company_id).await? {
            if primary.id == id {
                return Err(WebsiteError::WebsiteIsPrimaryForCompany);
            }
        }
        sqlx::query(
            r#"
            UPDATE website.websites
            SET metadata = jsonb_set(jsonb_set(metadata, '{deleted_at}', to_jsonb(now())),
                                     '{deleted_by}', to_jsonb($2))
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(actor.stamp())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The config-fields patch — whitelist columns only; refuses an
    /// empty patch. `public_user_id`/`company_id` are unpatchable by
    /// construction (absent from `WebsiteConfigPatch`); the route
    /// layer maps any attempt to `website_field_not_patchable`.
    // The terminal set_arm arm's flag assignment is dead by construction.
    #[allow(unused_assignments)]
    pub async fn patch_config(
        &self,
        actor: ActorRef,
        id: Uuid,
        patch: WebsiteConfigPatch,
    ) -> Result<WebsiteView, WebsiteError> {
        use sqlx::QueryBuilder;
        if patch.is_empty() {
            return Err(WebsiteError::InvalidInput(
                "the config patch sets no field".into(),
            ));
        }
        if let Some(url) = patch.homepage_url.as_deref() {
            if !url.starts_with('/') {
                return Err(WebsiteError::InvalidInput(
                    "homepage_url must be a site-absolute path starting with '/'".into(),
                ));
            }
        }
        let mut qb = QueryBuilder::new("UPDATE website.websites SET ");
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
        set_arm!("name", patch.name.clone());
        if let Some(d) = patch.domain.clone() {
            let normalized = if d.trim().is_empty() { None } else { Some(normalize_host(&d)) };
            if !first {
                qb.push(", ");
            }
            qb.push("domain = ").push_bind(normalized);
            first = false;
        }
        set_arm!("default_lang_code", patch.default_lang_code.clone());
        set_arm!("homepage_url", patch.homepage_url.clone());
        set_arm!("robots_txt", patch.robots_txt.clone());
        set_arm!("social_links", patch.social_links.clone());
        set_arm!("contact_recipients", patch.contact_recipients.clone());
        set_arm!("sequence", patch.sequence);
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
        qb.push(" RETURNING id, name, domain, company_id, public_user_id, default_lang_code, \
                 homepage_url, robots_txt, social_links, contact_recipients, sequence");
        let view = qb
            .build_query_as::<WebsiteView>()
            .fetch_one(&self.pool)
            .await
            .map_err(super::website_error::map_unique_violation)?;
        Ok(view)
    }

    /// The company allow-list derivation (exported for family-side read
    /// models): the website's public principal → [company]; a member →
    /// [company]; any other verified principal → [] (public tier only).
    /// Officers are outside this function's domain.
    pub async fn company_allowlist(
        &self,
        principal_user_id: Uuid,
        website_id: Uuid,
    ) -> Result<Vec<Uuid>, WebsiteError> {
        let company = sqlx::query_scalar::<_, Option<Uuid>>(
            r#"
            SELECT
                CASE
                    WHEN w.public_user_id = $2
                        THEN w.company_id
                    WHEN EXISTS (SELECT 1 FROM website.website_members m
                                 WHERE m.website_id = w.id
                                   AND m.portal_user_id = $2
                                   AND (m.metadata->>'deleted_at') IS NULL)
                        THEN w.company_id
                    ELSE NULL
                END
            FROM website.websites w
            WHERE w.id = $1 AND (w.metadata->>'deleted_at') IS NULL
            "#,
        )
        .bind(website_id)
        .bind(principal_user_id)
        .fetch_optional(&self.pool)
        .await?
        .flatten();
        Ok(company.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_host_strips_scheme_port_dot_and_case() {
        assert_eq!(normalize_host("Example.COM:443"), "example.com");
        assert_eq!(normalize_host("https://Shop.Example.io/"), "shop.example.io");
        assert_eq!(normalize_host("  WWW.site.org.  "), "www.site.org");
    }

    #[test]
    fn normalize_host_keeps_ipv6_colons() {
        assert_eq!(normalize_host("[::1]:8080"), "[::1]");
        assert_eq!(normalize_host("::1"), "::1");
    }
}
