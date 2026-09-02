//! The exported trait surface (hand-written; user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! `WebsiteSurface` is the artifact name downstream modules (blog's
//! stale-slug 301s, livechat / WB-8's visit seam, the event and
//! storefront surfaces) hold this module to — mirroring portal's
//! `PortalDocumentSurface`. Re-exported exactly once through the
//! generated-seam CUSTOM SERVICES block in `src/exports/services.rs`.

use uuid::Uuid;

use super::lang_matcher::RedirectAnswer as MatcherRedirectAnswer;
use super::menu_service::{MenuAdminService, MenuNode};
use super::page_service::PageAdminService;
use super::principal_port::WebsitePrincipal;
use super::redirect_service::RedirectAdminService;
use super::specificity::resolve_specific;
use super::visitor_gc::{sweep_partnerless_visitors, SweepSummary, DEFAULT_GC_BATCH, DEFAULT_RETENTION_DAYS};
use super::visitor_service::{SessionFacts, VisitorEngine};
use super::website_error::WebsiteError;
use super::website_service::{WebsiteRootService, WebsiteView};

/// The module's result alias on the exported surface.
pub type WebsiteResult<T> = Result<T, WebsiteError>;

/// The redirect kinds `record_redirect` accepts (the closed
/// `website_redirect_type` vocabulary).
pub type RedirectKind = &'static str;

/// The exported surface. Every method is a thin delegation to the ONE
/// hand service that owns the verb — the surface adds no logic.
#[async_trait::async_trait]
pub trait WebsiteSurface: Send + Sync {
    /// Hostname → the bound live website (no fallback; miss is the
    /// typed 404).
    async fn resolve_website_by_host(&self, host: &str) -> WebsiteResult<WebsiteView>;
    /// The public page read (published + tier rules) with blocks.
    async fn visible_page(
        &self,
        website_id: Uuid,
        url: &str,
        principal: Option<&WebsitePrincipal>,
    ) -> WebsiteResult<Option<super::page_service::PublicPage>>;
    /// The public menu tree (visible entries only).
    async fn menu_tree_visible(
        &self,
        website_id: Uuid,
        principal: Option<&WebsitePrincipal>,
    ) -> WebsiteResult<Vec<MenuNode>>;
    /// The redirect-table answer for one path (matcher case 7's input).
    async fn redirect_answer(&self, website_id: Uuid, url: &str)
        -> Option<MatcherRedirectAnswer>;
    /// Record a redirect (blog stale-slug 301s ride this seam).
    async fn record_redirect(
        &self,
        website_id: Uuid,
        url_from: &str,
        url_to: &str,
        kind: RedirectKind,
    ) -> WebsiteResult<()>;
    /// The company allow-list derivation for a verified principal on a
    /// website ([company] when public principal or member; else []).
    async fn company_allowlist(&self, principal: &WebsitePrincipal, website_id: Uuid) -> Vec<Uuid>;
    /// Record a visit (livechat / WB-8 seam) — the heartbeat upsert.
    async fn track_visit(
        &self,
        website_id: Uuid,
        session: &SessionFacts<'_>,
        url: &str,
        page_key: Option<&str>,
    ) -> WebsiteResult<()>;
    /// The visitor GC verb (cron + manual trigger share it).
    async fn sweep_visitors(&self) -> WebsiteResult<SweepSummary>;
}

/// The Postgres reference implementation, composing the hand services.
pub struct PgWebsiteSurface {
    websites: WebsiteRootService,
    pages: PageAdminService,
    menus: MenuAdminService,
    redirects: RedirectAdminService,
    visitors: VisitorEngine,
    retention_days: i64,
    gc_batch: i64,
}

impl PgWebsiteSurface {
    pub fn new(pool: sqlx::PgPool, pepper: String) -> Self {
        let retention_days = env_u64("WEBSITE_VISITOR_RETENTION_DAYS")
            .unwrap_or(DEFAULT_RETENTION_DAYS as u64) as i64;
        let gc_batch = env_u64("WEBSITE_VISITOR_GC_BATCH")
            .unwrap_or(DEFAULT_GC_BATCH as u64) as i64;
        Self {
            websites: WebsiteRootService::new(pool.clone()),
            pages: PageAdminService::new(pool.clone()),
            menus: MenuAdminService::new(pool.clone()),
            redirects: RedirectAdminService::new(pool.clone()),
            visitors: VisitorEngine::new(pool, pepper),
            retention_days,
            gc_batch,
        }
    }
}

fn env_u64(key: &str) -> Option<u64> {
    std::env::var(key).ok().and_then(|v| v.trim().parse::<u64>().ok())
}

#[async_trait::async_trait]
impl WebsiteSurface for PgWebsiteSurface {
    async fn resolve_website_by_host(&self, host: &str) -> WebsiteResult<WebsiteView> {
        self.websites.website_by_host(host).await
    }

    async fn visible_page(
        &self,
        website_id: Uuid,
        url: &str,
        principal: Option<&WebsitePrincipal>,
    ) -> WebsiteResult<Option<super::page_service::PublicPage>> {
        self.pages
            .visible_page(website_id, url, principal.map(|p| p.user_uuid()))
            .await
    }

    async fn menu_tree_visible(
        &self,
        website_id: Uuid,
        principal: Option<&WebsitePrincipal>,
    ) -> WebsiteResult<Vec<MenuNode>> {
        self.menus
            .tree_visible(website_id, principal.map(|p| p.user_uuid()))
            .await
    }

    async fn redirect_answer(
        &self,
        website_id: Uuid,
        url: &str,
    ) -> Option<MatcherRedirectAnswer> {
        self.redirects.answer(website_id, url).await.ok().flatten()
    }

    async fn record_redirect(
        &self,
        website_id: Uuid,
        url_from: &str,
        url_to: &str,
        kind: RedirectKind,
    ) -> WebsiteResult<()> {
        self.redirects
            .record_redirect(website_id, url_from, url_to, kind)
            .await
    }

    async fn company_allowlist(&self, principal: &WebsitePrincipal, website_id: Uuid) -> Vec<Uuid> {
        self.websites
            .company_allowlist(principal.user_uuid(), website_id)
            .await
            .unwrap_or_default()
    }

    async fn track_visit(
        &self,
        website_id: Uuid,
        session: &SessionFacts<'_>,
        url: &str,
        page_key: Option<&str>,
    ) -> WebsiteResult<()> {
        let page_id = match page_key {
            Some(key) => resolve_specific(self.pages.pool(), key, website_id)
                .await?
                .row()
                .map(|r| r.id),
            None => None,
        };
        self.visitors
            .heartbeat(website_id, session, Some(url), page_id)
            .await?;
        Ok(())
    }

    async fn sweep_visitors(&self) -> WebsiteResult<SweepSummary> {
        sweep_partnerless_visitors(self.websites.pool(), self.retention_days, self.gc_batch).await
    }
}
