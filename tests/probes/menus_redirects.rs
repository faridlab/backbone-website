//! Menu tree probes (§4.4/WS-19 shape) and redirect table probes
//! (§8.2 case-7 backing store): depth ceiling, mega isolation, the
//! explicit fan-out verb, local-only delete, url_from uniqueness, and
//! the routing answers the matcher consumes.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use backbone_website::application::service::intake_engine::TurnstileConfig;
use backbone_website::application::service::menu_service::{
    CreateMenuInput, MenuAdminService, MenuPatch,
};
use backbone_website::application::service::redirect_service::{
    CreateRedirectInput, RedirectAdminService, RedirectPatch,
};
use backbone_website::application::service::website_error::WebsiteError;
use backbone_website::application::service::website_service::ActorRef;
use backbone_website::presentation::http::public_routes::{
    website_public_routes, WebsitePublicState,
};

use super::common::{host_of, make_website, TestDb, PROBE_PEPPER};

fn officer() -> ActorRef {
    ActorRef::officer(Uuid::new_v4())
}

async fn mk_menu(
    menus: &MenuAdminService,
    website: uuid::Uuid,
    parent: Option<uuid::Uuid>,
    name: &str,
) -> uuid::Uuid {
    menus
        .create_menu(
            officer(),
            CreateMenuInput {
                website_id: website,
                parent_id: parent,
                name: name.into(),
                page_id: None,
                url: Some(format!("https://{name}.example")),
                new_window: false,
                sequence: 5,
                visibility: "public".into(),
                required_member_roles: vec![],
                is_mega_menu: false,
            },
        )
        .await
        .unwrap_or_else(|e| panic!("PROBE-FAIL: create menu {name}: {e:?}"))
        .id
}

/// Depth ceiling: root → child → grandchild is allowed (two levels
/// below root); a great-grandchild is REFUSED with the typed error, and
/// a patch that would DEEPEN a subtree past the ceiling is refused
/// against the RESULTING position.
#[tokio::test]
async fn probe_menu_depth_ceiling() {
    let db = TestDb::new("menudepth").await;
    let website = make_website(&db.pool, "menudepth").await;
    let menus = MenuAdminService::new(db.pool.clone());

    let root = mk_menu(&menus, website.id, None, "root").await;
    let child = mk_menu(&menus, website.id, Some(root), "child").await;
    let grandchild = mk_menu(&menus, website.id, Some(child), "grandchild").await;

    // The great-grandchild crosses the ceiling.
    match menus
        .create_menu(
            officer(),
            CreateMenuInput {
                website_id: website.id,
                parent_id: Some(grandchild),
                name: "great".into(),
                page_id: None,
                url: None,
                new_window: false,
                sequence: 1,
                visibility: "public".into(),
                required_member_roles: vec![],
                is_mega_menu: false,
            },
        )
        .await
    {
        Err(WebsiteError::MenuDepthExceeded) => {}
        other => panic!("PROBE-FAIL: depth 3 must refuse, got {other:?}"),
    }

    // A patch re-parenting the ROOT under the grandchild (depth 2) is
    // refused against the RESULTING position — and self-parenting is
    // refused outright.
    match menus
        .patch_menu(
            officer(),
            root,
            MenuPatch { parent_id: Some(Some(grandchild)), ..Default::default() },
        )
        .await
    {
        Err(WebsiteError::MenuDepthExceeded) => {}
        other => panic!("PROBE-FAIL: deepening patch must refuse, got {other:?}"),
    }
    match menus
        .patch_menu(
            officer(),
            child,
            MenuPatch { parent_id: Some(Some(child)), ..Default::default() },
        )
        .await
    {
        Err(WebsiteError::InvalidInput(_)) => {}
        other => panic!("PROBE-FAIL: self-parenting must refuse, got {other:?}"),
    }

    // A LEGAL move within the ceiling still works.
    menus
        .patch_menu(
            officer(),
            grandchild,
            MenuPatch { parent_id: Some(Some(root)), ..Default::default() },
        )
        .await
        .unwrap();

    db.dispose().await;
}

/// Mega-menu isolation: a mega carries NO parent and NO children — at
/// create and at patch, against both directions.
#[tokio::test]
async fn probe_mega_isolation() {
    let db = TestDb::new("menumega").await;
    let website = make_website(&db.pool, "menumega").await;
    let menus = MenuAdminService::new(db.pool.clone());

    // A mega WITH a parent is refused.
    let root = mk_menu(&menus, website.id, None, "root").await;
    match menus
        .create_menu(
            officer(),
            CreateMenuInput {
                website_id: website.id,
                parent_id: Some(root),
                name: "mega-with-parent".into(),
                page_id: None,
                url: None,
                new_window: false,
                sequence: 1,
                visibility: "public".into(),
                required_member_roles: vec![],
                is_mega_menu: true,
            },
        )
        .await
    {
        Err(WebsiteError::MegaMenuIsolated) => {}
        other => panic!("PROBE-FAIL: mega-with-parent must refuse, got {other:?}"),
    }

    // A clean mega lands at root level and carries blocks.
    let mega = menus
        .create_menu(
            officer(),
            CreateMenuInput {
                website_id: website.id,
                parent_id: None,
                name: "mega".into(),
                page_id: None,
                url: None,
                new_window: false,
                sequence: 1,
                visibility: "public".into(),
                required_member_roles: vec![],
                is_mega_menu: true,
            },
        )
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO website.menu_blocks (id, menu_id, kind, position, payload) \
         VALUES ($1, $2, 'link', 0, '{\"label\":\"Home\"}')",
    )
    .bind(Uuid::new_v4())
    .bind(mega.id)
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(menus.menu_blocks(mega.id).await.unwrap().len(), 1);

    // Giving the mega a parent via patch is refused...
    match menus
        .patch_menu(
            officer(),
            mega.id,
            MenuPatch { parent_id: Some(Some(root)), ..Default::default() },
        )
        .await
    {
        Err(WebsiteError::MegaMenuIsolated) => {}
        other => panic!("PROBE-FAIL: mega-with-parent patch must refuse, got {other:?}"),
    }
    // ...and flipping a node WITH children to mega is refused.
    mk_menu(&menus, website.id, Some(root), "child-a").await;
    match menus
        .patch_menu(
            officer(),
            root,
            MenuPatch { is_mega_menu: Some(true), ..Default::default() },
        )
        .await
    {
        Err(WebsiteError::MegaMenuIsolated) => {}
        other => panic!("PROBE-FAIL: mega-with-children patch must refuse, got {other:?}"),
    }

    db.dispose().await;
}

/// The explicit fan-out verb: one source menu copies to every OTHER
/// live website, re-parented under each target's root, page re-bound
/// through the ONE resolver to the TARGET's effective page, mega blocks
/// included — and LOCAL-only delete never cascades across websites.
#[tokio::test]
async fn probe_menu_fanout_and_local_delete() {
    let db = TestDb::new("menufan").await;
    let a = make_website(&db.pool, "fan-a").await;
    let b = make_website(&db.pool, "fan-b").await;
    let c = make_website(&db.pool, "fan-c").await;
    let menus = MenuAdminService::new(db.pool.clone());

    // A page-bound child on A (bound through A's homepage specific so
    // the re-bind is observable).
    let a_homepage_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM website.pages WHERE key = 'homepage' AND website_id = $1",
    )
    .bind(a.id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    let source = menus
        .create_menu(
            officer(),
            CreateMenuInput {
                website_id: a.id,
                parent_id: None,
                name: "source".into(),
                page_id: Some(a_homepage_id),
                url: None,
                new_window: false,
                sequence: 3,
                visibility: "public".into(),
                required_member_roles: vec![],
                is_mega_menu: false,
            },
        )
        .await
        .unwrap();

    let created = menus.fanout_menu(officer(), source.id).await.unwrap();
    assert_eq!(created.len(), 2, "exactly B and C receive copies");

    for created_id in &created {
        // Re-parented under the target's own root...
        let (parent_website, parent_of_copy): (uuid::Uuid, Option<uuid::Uuid>) = sqlx::query_as(
            "SELECT m.website_id, m.parent_id FROM website.menus m WHERE m.id = $1",
        )
        .bind(created_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert!(
            parent_website == b.id || parent_website == c.id,
            "the copy lands on one of the OTHER websites (got {parent_website})"
        );
        assert!(parent_of_copy.is_some(), "the copy re-roots under the target's root menu");
        // ...and page-bound to the TARGET's own row, never A's.
        let bound: Option<uuid::Uuid> = sqlx::query_scalar(
            "SELECT page_id FROM website.menus WHERE id = $1",
        )
        .bind(created_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        let bound_website: Option<uuid::Uuid> = match bound {
            Some(pid) => {
                sqlx::query_scalar("SELECT website_id FROM website.pages WHERE id = $1")
                    .bind(pid)
                    .fetch_one(&db.pool)
                    .await
                    .unwrap()
            }
            None => None,
        };
        assert_eq!(
            bound_website,
            Some(parent_website),
            "the copy's page binding must resolve to the TARGET's effective page"
        );
    }

    // LOCAL delete: A's source dies; B's and C's copies are untouched.
    menus.delete_menu(officer(), source.id).await.unwrap();
    let survivors: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.menus WHERE id = ANY($1) \
         AND (metadata->>'deleted_at') IS NULL",
    )
    .bind(&created)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(survivors, 2, "deleting the source is LOCAL — copies survive");

    // Deleting again is the typed 404 (already soft-deleted).
    match menus.delete_menu(officer(), source.id).await {
        Err(WebsiteError::NotFound(_)) => {}
        other => panic!("PROBE-FAIL: re-delete must 404, got {other:?}"),
    }

    db.dispose().await;
}

/// The redirect table: one answer per path per website (the partial
/// unique — a second is the typed 409), 308 parity validated on create
/// and patch, and the routing answers the matcher consumes.
#[tokio::test]
async fn probe_redirect_table_unique_and_answers() {
    let db = TestDb::new("redirtab").await;
    let a = make_website(&db.pool, "redir-a").await;
    let b = make_website(&db.pool, "redir-b").await;
    let redirects = RedirectAdminService::new(db.pool.clone());

    redirects
        .create(
            officer(),
            CreateRedirectInput {
                website_id: a.id,
                url_from: "/old-path".into(),
                redirect_type: "moved_301".into(),
                url_to: Some("/new-path".into()),
                description: None,
            },
        )
        .await
        .unwrap();

    // The SAME path on A is the typed conflict (the partial unique).
    match redirects
        .create(
            officer(),
            CreateRedirectInput {
                website_id: a.id,
                url_from: "/old-path".into(),
                redirect_type: "found_302".into(),
                url_to: Some("/other".into()),
                description: None,
            },
        )
        .await
    {
        Err(WebsiteError::SpecificityConflict { constraint }) => {
            assert!(constraint.contains("url_from_live"), "constraint {constraint}");
        }
        other => panic!("PROBE-FAIL: duplicate url_from must 409, got {other:?}"),
    }
    // The SAME path on B is legal (per-website scoping).
    redirects
        .create(
            officer(),
            CreateRedirectInput {
                website_id: b.id,
                url_from: "/old-path".into(),
                redirect_type: "moved_301".into(),
                url_to: Some("/b-new".into()),
                description: None,
            },
        )
        .await
        .unwrap();

    // 308 parity refused at create AND at patch (resulting row).
    match redirects
        .create(
            officer(),
            CreateRedirectInput {
                website_id: a.id,
                url_from: "/alias?x=1".into(),
                redirect_type: "alias_308".into(),
                url_to: Some("/alias-target?y=1".into()),
                description: None,
            },
        )
        .await
    {
        Err(WebsiteError::RedirectParamParity) => {}
        other => panic!("PROBE-FAIL: 308 parity must refuse, got {other:?}"),
    }
    let row = redirects
        .create(
            officer(),
            CreateRedirectInput {
                website_id: a.id,
                url_from: "/gone-path".into(),
                redirect_type: "gone_404".into(),
                url_to: None,
                description: None,
            },
        )
        .await
        .unwrap();
    match redirects
        .patch(
            officer(),
            row.id,
            RedirectPatch {
                redirect_type: Some("alias_308".into()),
                url_to: Some(Some("/alias-target?y=1".into())),
                ..Default::default()
            },
        )
        .await
    {
        Err(WebsiteError::RedirectParamParity) => {}
        other => panic!("PROBE-FAIL: 308 parity on patch must refuse, got {other:?}"),
    }

    // The stored answers the matcher reads: A's 301 and A's 410.
    let answer = redirects.answer(a.id, "/old-path").await.unwrap().unwrap();
    assert_eq!(answer.redirect_type, "moved_301");
    assert_eq!(answer.url_to.as_deref(), Some("/new-path"));
    let gone = redirects.answer(a.id, "/gone-path").await.unwrap().unwrap();
    assert_eq!(gone.redirect_type, "gone_404");
    assert!(gone.url_to.is_none());
    // B's same path answers DIFFERENTLY (per-website tables).
    let b_answer = redirects.answer(b.id, "/old-path").await.unwrap().unwrap();
    assert_eq!(b_answer.url_to.as_deref(), Some("/b-new"));
    assert!(redirects.answer(a.id, "/nothing").await.unwrap().is_none());

    db.dispose().await;
}

/// The public resolve verb answers from the redirect table: a 301 row
/// redirects, a gone_404 row answers not_found — through the ROUTER,
/// matcher case 7 end to end.
#[tokio::test]
async fn probe_public_resolve_redirect_answers() {
    let db = TestDb::new("reslv").await;
    let website = make_website(&db.pool, "reslv").await;
    let redirects = RedirectAdminService::new(db.pool.clone());
    redirects
        .create(
            officer(),
            CreateRedirectInput {
                website_id: website.id,
                url_from: "/legacy".into(),
                redirect_type: "moved_301".into(),
                url_to: Some("/modern".into()),
                description: None,
            },
        )
        .await
        .unwrap();
    redirects
        .create(
            officer(),
            CreateRedirectInput {
                website_id: website.id,
                url_from: "/buried".into(),
                redirect_type: "gone_404".into(),
                url_to: None,
                description: None,
            },
        )
        .await
        .unwrap();

    let state = WebsitePublicState::compose(
        db.pool.clone(),
        PROBE_PEPPER.to_string(),
        TurnstileConfig { secret: Some("stub".into()), verify_url: "http://127.0.0.1:9/x".to_string() },
    );
    let app = website_public_routes(state);
    let host = host_of(&website);

    for (path, want_action, want_status, want_location) in [
        ("/legacy", "redirect", 301u16, Some("/modern")),
        ("/buried", "not_found", 404, None),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/public/resolve?path={path}"))
                    .header("host", host.as_str())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "resolve {path}");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["action"], want_action, "resolve {path}: {body}");
        assert_eq!(body["status"], want_status, "resolve {path}: {body}");
        match want_location {
            Some(loc) => assert_eq!(body["location"], loc, "resolve {path}: {body}"),
            None => assert!(body["location"].is_null(), "resolve {path}: {body}"),
        }
    }

    db.dispose().await;
}
