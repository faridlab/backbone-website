//! Menu hierarchy_read probes (the web_hierarchy port): the slice
//! shape (anchor + ordered children + truncation flag), the
//! deterministic (sequence, id) child ordering, correlated child
//! counts, the reported (never silent) limit truncation, the typed
//! limit bounds, and the closed cross-website 404 (a parent from
//! another website is "not found on this website" — no oracle).

use uuid::Uuid;

use backbone_website::application::service::menu_service::{CreateMenuInput, MenuAdminService};
use backbone_website::application::service::website_error::WebsiteError;
use backbone_website::application::service::website_service::ActorRef;

use super::common::{make_website, TestDb};

fn officer() -> ActorRef {
    ActorRef::officer(Uuid::new_v4())
}

async fn mk_menu(
    menus: &MenuAdminService,
    website: Uuid,
    parent: Option<Uuid>,
    name: &str,
    sequence: i32,
) -> Uuid {
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
                sequence,
                visibility: "public".into(),
                required_member_roles: vec![],
                is_mega_menu: false,
            },
        )
        .await
        .unwrap_or_else(|e| panic!("PROBE-FAIL: create menu {name}: {e:?}"))
        .id
}

/// Three levels (the full legal depth): the roots slice anchors None,
/// children order by (sequence, id), child counts are correlated, the
/// grandchild slice is a leaf, truncation is REPORTED, and the limit
/// bounds refuse with the typed validation error.
#[tokio::test]
async fn probe_menu_hierarchy_slice_shape_and_ordering() {
    let db = TestDb::new("hiershape").await;
    let website = make_website(&db.pool, "hiershape").await;
    let menus = MenuAdminService::new(db.pool.clone());

    // (Bootstrap mints the site's own root menu; this probe reasons
    // about ITS root, robust to any siblings in the roots slice.)
    let root_a = mk_menu(&menus, website.id, None, "root-a", 10).await;
    let c30 = mk_menu(&menus, website.id, Some(root_a), "c-thirty", 30).await;
    let c10 = mk_menu(&menus, website.id, Some(root_a), "c-ten", 10).await;
    let c20 = mk_menu(&menus, website.id, Some(root_a), "c-twenty", 20).await;
    let _gc = mk_menu(&menus, website.id, Some(c10), "grandchild", 1).await;

    // Roots slice.
    let slice = menus.hierarchy_read(website.id, None, 100).await.unwrap();
    assert!(slice.parent.is_none());
    assert!(!slice.truncated);
    let root_a_node = slice
        .children
        .iter()
        .find(|c| c.id == root_a)
        .unwrap_or_else(|| panic!("PROBE-FAIL: root-a missing from the roots slice"));
    assert_eq!(root_a_node.child_count, 3);
    assert_eq!(root_a_node.parent_id, None);

    // Children slice: deterministic (sequence, id) ordering.
    let slice = menus
        .hierarchy_read(website.id, Some(root_a), 100)
        .await
        .unwrap();
    let anchor = slice
        .parent
        .as_ref()
        .unwrap_or_else(|| panic!("PROBE-FAIL: anchor missing"));
    assert_eq!(anchor.id, root_a);
    assert_eq!(anchor.child_count, 3);
    let ids: Vec<Uuid> = slice.children.iter().map(|c| c.id).collect();
    assert_eq!(ids, vec![c10, c20, c30], "children must order by (sequence, id)");

    // The grandchild slice under c10 is a leaf level.
    let slice = menus.hierarchy_read(website.id, Some(c10), 100).await.unwrap();
    assert_eq!(slice.children.len(), 1);
    assert_eq!(slice.children[0].name, "grandchild");
    assert_eq!(slice.children[0].child_count, 0);

    // Truncation is reported, never silent.
    let slice = menus
        .hierarchy_read(website.id, Some(root_a), 2)
        .await
        .unwrap();
    assert_eq!(slice.children.len(), 2);
    assert!(slice.truncated, "the third child must be REPORTED as truncated");

    // The limit bounds refuse with the typed validation error.
    for bad in [0i64, 501] {
        match menus.hierarchy_read(website.id, None, bad).await {
            Err(WebsiteError::InvalidInput(_)) => {}
            other => panic!("PROBE-FAIL: limit {bad} must refuse InvalidInput, got {other:?}"),
        }
    }

    db.dispose().await;
}

/// A parent menu from ANOTHER website is "not found on this website" —
/// the closed 404, never a cross-website oracle.
#[tokio::test]
async fn probe_menu_hierarchy_refuses_foreign_parent() {
    let db = TestDb::new("hierforeign").await;
    let site_a = make_website(&db.pool, "hierforeign-a").await;
    let site_b = make_website(&db.pool, "hierforeign-b").await;
    let menus = MenuAdminService::new(db.pool.clone());

    let foreign = mk_menu(&menus, site_b.id, None, "b-root", 10).await;
    match menus.hierarchy_read(site_a.id, Some(foreign), 100).await {
        Err(WebsiteError::NotFound(msg)) => {
            assert!(msg.contains("parent"), "the refusal must name the parent: {msg}")
        }
        other => panic!("PROBE-FAIL: foreign parent must be the typed NotFound, got {other:?}"),
    }

    db.dispose().await;
}
