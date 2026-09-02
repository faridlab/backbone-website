//! Versioning verb probes: idempotent fork, and the all-or-nothing
//! copy-on-unlink fanout delete.

use uuid::Uuid;

use backbone_website::application::service::menu_service::{
    CreateMenuInput, MenuAdminService,
};
use backbone_website::application::service::page_service::{
    CreatePageInput, PageAdminService,
};
use backbone_website::application::service::versioning_service::{
    delete_generic_with_fanout, fork_to_website, ForkOutcome,
};
use backbone_website::application::service::website_error::WebsiteError;
use backbone_website::application::service::website_service::ActorRef;

use super::common::{make_website, TestDb};

fn officer() -> ActorRef {
    ActorRef::officer(Uuid::new_v4())
}

/// Fork is IDEMPOTENT: the second call returns Existing with the SAME
/// row, creates nothing, and does not touch the generic.
#[tokio::test]
async fn probe_fork_idempotent() {
    let db = TestDb::new("forkidem").await;
    let website = make_website(&db.pool, "forkidem").await;
    let pages = PageAdminService::new(db.pool.clone());

    let generic = pages
        .create_page(
            officer(),
            CreatePageInput {
                key: "pricing".into(),
                website_id: None,
                url: "/pricing".into(),
                title: "Pricing".into(),
                seo_name: None,
                visibility: "public".into(),
                required_member_roles: vec![],
                website_indexed: true,
            },
        )
        .await
        .unwrap();

    let first = fork_to_website(&db.pool, officer(), "pricing", website.id)
        .await
        .unwrap();
    let first_id = match &first {
        ForkOutcome::Created(p) => p.id,
        ForkOutcome::Existing(_) => panic!("PROBE-FAIL: first fork must create"),
    };

    let second = fork_to_website(&db.pool, officer(), "pricing", website.id)
        .await
        .unwrap();
    match second {
        ForkOutcome::Existing(p) => assert_eq!(p.id, first_id, "idempotent fork returns same row"),
        ForkOutcome::Created(_) => panic!("PROBE-FAIL: second fork must be Existing"),
    }

    // Rows: the generic + exactly one specific.
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM website.pages WHERE key = 'pricing'")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(count, 2, "generic + exactly one specific (got {count})");

    // Provenance rode along.
    let (forked_from, forked_by_set): (Option<Uuid>, bool) = sqlx::query_as(
        "SELECT forked_from, (forked_by IS NOT NULL) FROM website.pages WHERE id = $1",
    )
    .bind(first_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(forked_from, Some(generic.id), "fork records its source");
    assert!(forked_by_set);

    // A fork of a MISSING generic is the typed refusal.
    match fork_to_website(&db.pool, officer(), "no-such-key", website.id).await {
        Err(WebsiteError::ForkSourceMissing) => {}
        other => panic!("PROBE-FAIL: expected ForkSourceMissing, got {other:?}"),
    }

    db.dispose().await;
}

/// The fanout delete: every website lacking a specific receives one
/// (content preserved), menus bound to the generic re-point to their
/// website's own row, and the generic soft-deletes. All-or-nothing in
/// ONE transaction.
#[tokio::test]
async fn probe_fanout_delete_all_or_nothing() {
    let db = TestDb::new("fanout").await;
    let w1 = make_website(&db.pool, "fan-a").await;
    let w2 = make_website(&db.pool, "fan-b").await;
    let w3 = make_website(&db.pool, "fan-c").await;
    let pages = PageAdminService::new(db.pool.clone());
    let menus = MenuAdminService::new(db.pool.clone());

    // The generic, published, with one content block.
    let generic = pages
        .create_page(
            officer(),
            CreatePageInput {
                key: "docs".into(),
                website_id: None,
                url: "/docs".into(),
                title: "Docs".into(),
                seo_name: None,
                visibility: "public".into(),
                required_member_roles: vec![],
                website_indexed: true,
            },
        )
        .await
        .unwrap();
    pages.publish(officer(), generic.id).await.unwrap();
    sqlx::query(
        "INSERT INTO website.page_blocks (id, page_id, kind, position, payload) \
         VALUES ($1, $2, 'rich_text', 0, '{\"body\":\"doc body\"}')",
    )
    .bind(Uuid::new_v4())
    .bind(generic.id)
    .execute(&db.pool)
    .await
    .unwrap();

    // W1 already forked (it must NOT be re-created; its menu keeps its row).
    let w1_specific = match fork_to_website(&db.pool, officer(), "docs", w1.id).await.unwrap() {
        ForkOutcome::Created(p) => p.id,
        ForkOutcome::Existing(_) => panic!("PROBE-FAIL: W1 pre-fork must create"),
    };

    // Menus on every website bound to the page they currently see.
    for (i, w) in [&w1, &w2, &w3].iter().enumerate() {
        let bound_page = if w.id == w1.id { w1_specific } else { generic.id };
        menus
            .create_menu(
                officer(),
                CreateMenuInput {
                    website_id: w.id,
                    parent_id: None,
                    name: format!("Docs {i}"),
                    page_id: Some(bound_page),
                    url: None,
                    new_window: false,
                    sequence: i as i32 + 1,
                    visibility: "public".into(),
                    required_member_roles: vec![],
                    is_mega_menu: false,
                },
            )
            .await
            .unwrap();
    }

    let deletion = delete_generic_with_fanout(&db.pool, officer(), "docs")
        .await
        .unwrap();

    // W2 and W3 received specifics; W1 did not need one.
    assert_eq!(deletion.created.len(), 2, "exactly W2+W3 forked");
    assert!(deletion.websites_touched.contains(&w2.id));
    assert!(deletion.websites_touched.contains(&w3.id));
    assert!(!deletion.websites_touched.contains(&w1.id));

    // The generic is soft-deleted (invisible to the resolver).
    let live_generic: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.pages WHERE key = 'docs' AND website_id IS NULL \
         AND (metadata->>'deleted_at') IS NULL",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(live_generic, 0, "the generic must be soft-deleted");

    // Every website still sees the page through its OWN specific...
    for w in [&w1, &w2, &w3] {
        let got = pages.visible_page(w.id, "/docs", None).await.unwrap();
        assert!(got.is_some(), "website {} must keep its copy", w.id);
    }

    // ...with the content block preserved (copy-on-unlink is lossless).
    for w in [&w2, &w3] {
        let blocks = pages
            .page_blocks(
                sqlx::query_scalar::<_, Uuid>(
                    "SELECT id FROM website.pages WHERE key = 'docs' AND website_id = $1",
                )
                .bind(w.id)
                .fetch_one(&db.pool)
                .await
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(blocks.len(), 1, "fanout copies carry the content blocks");
    }

    // Every menu now points at ITS website's row — no menu left on the
    // soft-deleted generic.
    let orphaned: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.menus m JOIN website.pages p ON p.id = m.page_id \
         WHERE p.key = 'docs' AND (p.metadata->>'deleted_at') IS NOT NULL",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(orphaned, 0, "no menu may remain bound to the deleted generic");

    let w1_menu_still_own: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.menus m WHERE m.website_id = $1 AND m.page_id = $2",
    )
    .bind(w1.id)
    .bind(w1_specific)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(w1_menu_still_own, 1, "W1's menu keeps its pre-existing specific");

    // Two specifics per fanout + W1's = one live specific per website.
    for w in [&w1, &w2, &w3] {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM website.pages WHERE key = 'docs' AND website_id = $1 \
             AND (metadata->>'deleted_at') IS NULL",
        )
        .bind(w.id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(n, 1);
    }

    db.dispose().await;
}
