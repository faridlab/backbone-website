//! Menu service (hand-written; user-owned; see `metaphor.codegen.yaml`).
//!
//! The generated CRUD alias first (keeps lib.rs's wiring compiling),
//! then the hand menu verbs: the tier-filtered tree read, the depth
//! ceiling (<= 2 below root), mega-menu isolation, explicit per-website
//! fan-out, and LOCAL-only deletion (the upstream cross-website
//! same-url cascade delete is not ported — its blast radius is a
//! recorded upstream latent risk).

use sqlx::PgPool;
use uuid::Uuid;

use backbone_core::GenericCrudService;
use crate::domain::entity::Menu;
use crate::infrastructure::persistence::MenuRepository;
use crate::presentation::dto::{CreateMenuDto, UpdateMenuDto};

/// Generated CRUD alias (the generator skipped emitting this file
/// because it is user-owned).
pub type MenuService = GenericCrudService<
    Menu,
    CreateMenuDto,
    UpdateMenuDto,
    MenuRepository,
>;

use super::page_service::tier_passes;
use super::website_error::WebsiteError;
use super::website_service::{record_audit, ActorRef};

/// The depth ceiling: at most two levels below the root node.
pub const MENU_MAX_DEPTH: i32 = 2;

/// One node of a hierarchy slice — the flat read shape the
/// `web_hierarchy` port serves (the same fields as [`MenuNode`] plus
/// the lazy-expansion child count, WITHOUT recursive nesting: the
/// view expands level by level through the slice verb).
#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct HierarchyNode {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub page_id: Option<Uuid>,
    pub url: Option<String>,
    pub new_window: bool,
    pub sequence: i32,
    pub visibility: String,
    pub required_member_roles: Vec<String>,
    pub is_mega_menu: bool,
    pub child_count: i64,
}

/// A hierarchy slice: the anchor row (None = the roots slice), its
/// ordered child level, and the truncation flag.
#[derive(Debug, serde::Serialize)]
pub struct HierarchySlice {
    pub website_id: Uuid,
    pub parent: Option<HierarchyNode>,
    pub children: Vec<HierarchyNode>,
    pub truncated: bool,
}

/// The menu tree node the reads serve.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MenuNode {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub page_id: Option<Uuid>,
    pub url: Option<String>,
    pub new_window: bool,
    pub sequence: i32,
    pub visibility: String,
    pub required_member_roles: Vec<String>,
    pub is_mega_menu: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<MenuNode>,
}

/// Officer/admin create input (`website_id` required — a menu with no
/// website is impossible; the create verb never fans out implicitly).
#[derive(Debug, Clone)]
pub struct CreateMenuInput {
    pub website_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub page_id: Option<Uuid>,
    pub url: Option<String>,
    pub new_window: bool,
    pub sequence: i32,
    pub visibility: String,
    pub required_member_roles: Vec<String>,
    pub is_mega_menu: bool,
}

/// The typed patch whitelist (depth and mega rules validated
/// service-side against the resulting tree).
#[derive(Debug, Clone, Default)]
pub struct MenuPatch {
    pub parent_id: Option<Option<Uuid>>,
    pub name: Option<String>,
    pub page_id: Option<Option<Uuid>>,
    pub url: Option<Option<String>>,
    pub new_window: Option<bool>,
    pub sequence: Option<i32>,
    pub visibility: Option<String>,
    pub required_member_roles: Option<Vec<String>>,
    pub is_mega_menu: Option<bool>,
}

impl MenuPatch {
    pub fn is_empty(&self) -> bool {
        self.parent_id.is_none()
            && self.name.is_none()
            && self.page_id.is_none()
            && self.url.is_none()
            && self.new_window.is_none()
            && self.sequence.is_none()
            && self.visibility.is_none()
            && self.required_member_roles.is_none()
            && self.is_mega_menu.is_none()
    }
}

/// The hand menu verbs.
pub struct MenuAdminService {
    pool: PgPool,
}

impl MenuAdminService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Depth of a node (root = 0), walking parents in SQL.
    async fn depth_of(
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
        node_id: Uuid,
    ) -> Result<i32, WebsiteError> {
        let (depth,): (i32,) = sqlx::query_as(
            r#"
            WITH RECURSIVE up AS (
                SELECT id, parent_id, 0 AS d FROM website.menus WHERE id = $1
                UNION ALL
                SELECT m.id, m.parent_id, up.d + 1
                FROM website.menus m JOIN up ON m.id = up.parent_id
                WHERE up.d < 16
            )
            SELECT MAX(d) FROM up
            "#,
        )
        .bind(node_id)
        .fetch_one(exec)
        .await?;
        Ok(depth)
    }

    async fn node_children_count(
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
        node_id: Uuid,
    ) -> Result<i64, WebsiteError> {
        let (n,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM website.menus \
             WHERE parent_id = $1 AND (metadata->>'deleted_at') IS NULL",
        )
        .bind(node_id)
        .fetch_one(exec)
        .await?;
        Ok(n)
    }

    /// Officer create — depth ceiling and mega isolation validated
    /// service-side; audits `menu_created`.
    pub async fn create_menu(&self, actor: ActorRef, input: CreateMenuInput) -> Result<Menu, WebsiteError> {
        if input.name.trim().is_empty() {
            return Err(WebsiteError::InvalidInput("menu name is required".into()));
        }
        if let Some(parent) = input.parent_id {
            if input.is_mega_menu {
                return Err(WebsiteError::MegaMenuIsolated);
            }
            let parent_depth = Self::depth_of(&self.pool, parent).await?;
            if parent_depth + 1 > MENU_MAX_DEPTH {
                return Err(WebsiteError::MenuDepthExceeded);
            }
        }
        let menu = sqlx::query_as::<_, Menu>(
            r#"
            INSERT INTO website.menus
                (id, website_id, parent_id, name, page_id, url, new_window,
                 sequence, visibility, required_member_roles, is_mega_menu, metadata)
            VALUES ($1, $2, $3, $4, $5, $6, $7,
                    $8, $9::website_visibility, $10, $11,
                    jsonb_build_object('created_at', now(), 'created_by', $12))
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(input.website_id)
        .bind(input.parent_id)
        .bind(input.name.trim())
        .bind(input.page_id)
        .bind(input.url)
        .bind(input.new_window)
        .bind(input.sequence)
        .bind(&input.visibility)
        .bind(&input.required_member_roles)
        .bind(input.is_mega_menu)
        .bind(actor.stamp())
        .fetch_one(&self.pool)
        .await
        .map_err(super::website_error::map_unique_violation)?;
        record_audit(
            &self.pool,
            "menu_created",
            actor,
            Some("menu"),
            Some(menu.id),
            Some(serde_json::json!({ "website_id": menu.website_id, "name": menu.name })),
        )
        .await?;
        Ok(menu)
    }

    /// The typed patch — depth and mega isolation re-validated against
    /// the RESULTING position; audits `menu_updated`.
    // The terminal set_arm arm's flag assignment is dead by construction.
    #[allow(unused_assignments)]
    pub async fn patch_menu(&self, actor: ActorRef, id: Uuid, patch: MenuPatch) -> Result<Menu, WebsiteError> {
        if patch.is_empty() {
            return Err(WebsiteError::InvalidInput(
                "the menu patch sets no field".into(),
            ));
        }
        let current: Option<Menu> = sqlx::query_as::<_, Menu>(
            "SELECT * FROM website.menus WHERE id = $1 \
             AND (metadata->>'deleted_at') IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(current) = current else {
            return Err(WebsiteError::NotFound(format!("menu {id}")));
        };

        let will_mega = patch.is_mega_menu.unwrap_or(current.is_mega_menu);
        let will_parent = match &patch.parent_id {
            Some(p) => *p,
            None => current.parent_id,
        };
        let depth = match will_parent {
            Some(p) => {
                if p == current.id {
                    return Err(WebsiteError::InvalidInput(
                        "a menu cannot be its own parent".into(),
                    ));
                }
                Self::depth_of(&self.pool, p).await? + 1
            }
            None => 0,
        };
        if depth > MENU_MAX_DEPTH {
            return Err(WebsiteError::MenuDepthExceeded);
        }
        if will_mega {
            // Isolation: a mega-menu carries NO parent and NO child
            // menu nodes (its content is menu_blocks, not children).
            if will_parent.is_some() {
                return Err(WebsiteError::MegaMenuIsolated);
            }
            if Self::node_children_count(&self.pool, id).await? > 0 {
                return Err(WebsiteError::MegaMenuIsolated);
            }
        }

        use sqlx::QueryBuilder;
        let mut qb = QueryBuilder::new("UPDATE website.menus SET ");
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
        set_arm!("parent_id", patch.parent_id.clone());
        set_arm!("name", patch.name.clone());
        set_arm!("page_id", patch.page_id.clone());
        set_arm!("url", patch.url.clone());
        set_arm!("new_window", patch.new_window);
        set_arm!("sequence", patch.sequence);
        if let Some(v) = patch.visibility.clone() {
            if !first {
                qb.push(", ");
            }
            qb.push("visibility = ").push_bind(v).push("::website_visibility");
            first = false;
        }
        set_arm!("required_member_roles", patch.required_member_roles.clone());
        set_arm!("is_mega_menu", patch.is_mega_menu);
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
        let menu = qb
            .build_query_as::<Menu>()
            .fetch_one(&self.pool)
            .await
            .map_err(super::website_error::map_unique_violation)?;
        record_audit(
            &self.pool,
            "menu_updated",
            actor,
            Some("menu"),
            Some(id),
            None,
        )
        .await?;
        Ok(menu)
    }

    /// LOCAL-only delete (its website, nothing else — the upstream
    /// cross-website cascade is not ported). Audits `menu_deleted`.
    pub async fn delete_menu(&self, actor: ActorRef, id: Uuid) -> Result<(), WebsiteError> {
        let row: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM website.menus WHERE id = $1 \
             AND (metadata->>'deleted_at') IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        if row.is_none() {
            return Err(WebsiteError::NotFound(format!("menu {id}")));
        }
        sqlx::query(
            r#"
            UPDATE website.menus
            SET metadata = jsonb_set(jsonb_set(metadata, '{deleted_at}', to_jsonb(now())),
                                     '{deleted_by}', to_jsonb($2))
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(actor.stamp())
        .execute(&self.pool)
        .await?;
        record_audit(&self.pool, "menu_deleted", actor, Some("menu"), Some(id), None).await?;
        Ok(())
    }

    /// Explicit per-website fan-out: copy this menu to every OTHER
    /// live website, re-parented under each target's root, binding the
    /// target's effective page for the source page's key (through the
    /// ONE resolver — never a cross-website page binding), mega blocks
    /// included. Audits `menu_fanout`. Returns the created-id list.
    pub async fn fanout_menu(&self, actor: ActorRef, id: Uuid) -> Result<Vec<Uuid>, WebsiteError> {
        let mut tx = self.pool.begin().await?;
        let source: Option<Menu> = sqlx::query_as::<_, Menu>(
            "SELECT * FROM website.menus WHERE id = $1 \
             AND (metadata->>'deleted_at') IS NULL",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(source) = source else {
            return Err(WebsiteError::NotFound(format!("menu {id}")));
        };

        // The source page's key — the cross-website re-bind needs the
        // TARGET's effective page for that key.
        let source_page_key: Option<String> = match source.page_id {
            Some(pid) => {
                sqlx::query_scalar::<_, String>(
                    "SELECT key FROM website.pages WHERE id = $1",
                )
                .bind(pid)
                .fetch_optional(&mut *tx)
                .await?
            }
            None => None,
        };

        let targets: Vec<(Uuid,)> = sqlx::query_as(
            r#"
            SELECT id FROM website.websites
            WHERE id <> $1 AND (metadata->>'deleted_at') IS NULL
            ORDER BY id
            "#,
        )
        .bind(source.website_id)
        .fetch_all(&mut *tx)
        .await?;

        let mut created = Vec::with_capacity(targets.len());
        for (target,) in targets {
            // The target's root: lowest-sequence root node.
            let target_root: Option<Uuid> = sqlx::query_scalar(
                r#"
                SELECT id FROM website.menus
                WHERE website_id = $1 AND parent_id IS NULL
                  AND (metadata->>'deleted_at') IS NULL
                ORDER BY sequence, id LIMIT 1
                "#,
            )
            .bind(target)
            .fetch_optional(&mut *tx)
            .await?;

            // Re-bind the page on the target through the resolver.
            let rebound_page = match source_page_key.as_deref() {
                Some(key) => {
                    super::specificity::resolve_specific(&mut *tx, key, target)
                        .await?
                        .row()
                        .map(|r| r.id)
                }
                None => None,
            };

            let new_id = Uuid::new_v4();
            let inserted = sqlx::query(
                r#"
                INSERT INTO website.menus
                    (id, website_id, parent_id, name, page_id, url, new_window,
                     sequence, visibility, required_member_roles, is_mega_menu, metadata)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8,
                        $9::website_visibility, $10, $11,
                        jsonb_build_object('created_at', now(), 'created_by', $12))
                ON CONFLICT DO NOTHING
                "#,
            )
            .bind(new_id)
            .bind(target)
            .bind(match (source.is_mega_menu, target_root) {
                (true, _) => None,           // a mega re-roots as a root
                (false, Some(r)) => Some(r), // re-parented under the target root
                (false, None) => None,
            })
            .bind(&source.name)
            .bind(rebound_page)
            .bind(source.url.clone())
            .bind(source.new_window)
            .bind(source.sequence)
            .bind(source.visibility.to_string())
            .bind(&source.required_member_roles)
            .bind(source.is_mega_menu)
            .bind(actor.stamp())
            .execute(&mut *tx)
            .await?
            .rows_affected()
                > 0;
            if inserted {
                sqlx::query(
                    r#"
                    INSERT INTO website.menu_blocks (id, menu_id, kind, position, payload)
                    SELECT gen_random_uuid(), $1, b.kind, b.position, b.payload
                    FROM website.menu_blocks b WHERE b.menu_id = $2
                    "#,
                )
                .bind(new_id)
                .bind(source.id)
                .execute(&mut *tx)
                .await?;
                created.push(new_id);
            }
        }

        record_audit(
            &mut *tx,
            "menu_fanout",
            actor,
            Some("menu"),
            Some(id),
            Some(serde_json::json!({ "created": created })),
        )
        .await?;
        tx.commit().await?;
        Ok(created)
    }

    /// The officer tree (admin read, everything).
    pub async fn tree_admin(&self, website_id: Uuid) -> Result<Vec<MenuNode>, WebsiteError> {
        let rows = sqlx::query_as::<_, Menu>(
            r#"
            SELECT * FROM website.menus
            WHERE website_id = $1 AND (metadata->>'deleted_at') IS NULL
            ORDER BY parent_id NULLS FIRST, sequence, id
            "#,
        )
        .bind(website_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(build_tree(rows))
    }

    /// The PUBLIC tree: live entries that pass the SAME tier rules as
    /// page reads, nested. `principal` = the verified portal principal
    /// (None = anonymous → public tier only).
    pub async fn tree_visible(
        &self,
        website_id: Uuid,
        principal: Option<Uuid>,
    ) -> Result<Vec<MenuNode>, WebsiteError> {
        let rows = sqlx::query_as::<_, Menu>(
            r#"
            SELECT * FROM website.menus
            WHERE website_id = $1 AND (metadata->>'deleted_at') IS NULL
            ORDER BY parent_id NULLS FIRST, sequence, id
            "#,
        )
        .bind(website_id)
        .fetch_all(&self.pool)
        .await?;
        let mut visible = Vec::with_capacity(rows.len());
        for m in rows {
            if tier_passes(
                &self.pool,
                &m.visibility.to_string(),
                &m.required_member_roles,
                website_id,
                principal,
            )
            .await?
            {
                visible.push(m);
            }
        }
        // Keep only nodes whose ENTIRE ancestor chain is visible (a
        // hidden parent hides its subtree on the public tree).
        let visible_ids: std::collections::HashSet<Uuid> =
            visible.iter().map(|m| m.id).collect();
        let fully = visible
            .into_iter()
            .filter(|m| match m.parent_id {
                Some(p) => visible_ids.contains(&p),
                None => true,
            })
            .collect();
        Ok(build_tree(fully))
    }

    /// The hierarchy slice read — the `web_hierarchy` port (one RPC
    /// returning a parent/child slice by spec, for lazily-expanding
    /// tree views). `parent = None` reads the ROOT slice. Ordering is
    /// deterministic everywhere: `(sequence, id)` within every level.
    /// The `limit` bound (1..=500) is part of the typed surface — a
    /// huge slice is a truncated answer plus the flag, never an
    /// unbounded read.
    pub async fn hierarchy_read(
        &self,
        website_id: Uuid,
        parent: Option<Uuid>,
        limit: i64,
    ) -> Result<HierarchySlice, WebsiteError> {
        const LIMIT_MAX: i64 = 500;
        if !(1..=LIMIT_MAX).contains(&limit) {
            return Err(WebsiteError::InvalidInput(format!(
                "hierarchy limit must lie between 1 and {LIMIT_MAX}"
            )));
        }
        // The anchor row (a parent outside this website is the closed
        // 404 — no cross-website oracle).
        let anchor = if let Some(parent_id) = parent {
            let row = sqlx::query_as::<_, HierarchyNode>(
                r#"
                SELECT m.id, m.parent_id, m.name, m.page_id, m.url, m.new_window,
                       m.sequence, m.visibility::text AS visibility,
                       m.required_member_roles, m.is_mega_menu,
                       (SELECT count(*) FROM website.menus gc
                        WHERE gc.parent_id = m.id
                          AND (gc.metadata->>'deleted_at') IS NULL) AS child_count
                FROM website.menus m
                WHERE m.id = $1 AND m.website_id = $2
                  AND (m.metadata->>'deleted_at') IS NULL
                "#,
            )
            .bind(parent_id)
            .bind(website_id)
            .fetch_optional(&self.pool)
            .await?;
            match row {
                Some(a) => Some(a),
                None => {
                    return Err(WebsiteError::NotFound(
                        "parent menu not found on this website".to_string(),
                    ))
                }
            }
        } else {
            None
        };
        // One level below the anchor (or the roots), limit+1 fetched so
        // truncation is observable without a second count query.
        let mut rows = sqlx::query_as::<_, HierarchyNode>(
            r#"
            SELECT m.id, m.parent_id, m.name, m.page_id, m.url, m.new_window,
                   m.sequence, m.visibility::text AS visibility,
                   m.required_member_roles, m.is_mega_menu,
                   (SELECT count(*) FROM website.menus gc
                    WHERE gc.parent_id = m.id
                      AND (gc.metadata->>'deleted_at') IS NULL) AS child_count
            FROM website.menus m
            WHERE m.website_id = $1
              AND m.parent_id IS NOT DISTINCT FROM $2
              AND (m.metadata->>'deleted_at') IS NULL
            ORDER BY m.sequence, m.id
            LIMIT $3
            "#,
        )
        .bind(website_id)
        .bind(parent)
        .bind(limit + 1)
        .fetch_all(&self.pool)
        .await?;
        let truncated = rows.len() as i64 > limit;
        if truncated {
            rows.truncate(limit as usize);
        }
        Ok(HierarchySlice {
            website_id,
            parent: anchor,
            children: rows,
            truncated,
        })
    }

    /// A mega-menu's ordered blocks.
    pub async fn menu_blocks(
        &self,
        menu_id: Uuid,
    ) -> Result<Vec<super::page_service::BlockView>, WebsiteError> {
        let blocks = sqlx::query_as::<_, super::page_service::BlockView>(
            r#"
            SELECT kind::text AS kind, position, payload
            FROM website.menu_blocks
            WHERE menu_id = $1
            ORDER BY position
            "#,
        )
        .bind(menu_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(blocks)
    }
}

fn build_tree(rows: Vec<Menu>) -> Vec<MenuNode> {
    use std::collections::HashMap;
    let nodes: HashMap<Uuid, MenuNode> = rows
        .into_iter()
        .map(|m| {
            (
                m.id,
                MenuNode {
                    id: m.id,
                    parent_id: m.parent_id,
                    name: m.name,
                    page_id: m.page_id,
                    url: m.url,
                    new_window: m.new_window,
                    sequence: m.sequence,
                    visibility: m.visibility.to_string(),
                    required_member_roles: m.required_member_roles,
                    is_mega_menu: m.is_mega_menu,
                    children: Vec::new(),
                },
            )
        })
        .collect();
    // A node is attachable only when its parent is also in the set
    // (the visible tree hides orphaned subtrees).
    let present: std::collections::HashSet<Uuid> = nodes.keys().copied().collect();
    let mut children: HashMap<Uuid, Vec<MenuNode>> = HashMap::new();
    let mut roots: Vec<MenuNode> = Vec::new();
    for (_, node) in nodes {
        match node.parent_id {
            Some(p) if present.contains(&p) => children.entry(p).or_default().push(node),
            _ => roots.push(node),
        }
    }

    fn attach(node: &mut MenuNode, children: &mut HashMap<Uuid, Vec<MenuNode>>) {
        if let Some(mut cs) = children.remove(&node.id) {
            for c in cs.iter_mut() {
                attach(c, children);
            }
            node.children = cs;
        }
    }
    let mut children = children;
    for r in roots.iter_mut() {
        attach(r, &mut children);
    }

    fn sort_recursive(nodes: &mut Vec<MenuNode>) {
        nodes.sort_by(|a, b| (a.sequence, a.id).cmp(&(b.sequence, b.id)));
        for n in nodes.iter_mut() {
            sort_recursive(&mut n.children);
        }
    }
    sort_recursive(&mut roots);
    roots
}
