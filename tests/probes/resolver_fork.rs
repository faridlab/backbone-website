//! Resolver + first-edit fork probes (the generic-vs-specific spine).
//!
//! WS-1: the ONE resolver folds generic → specific per (key, website);
//! the DB partial unique makes duplicate specifics impossible; the
//! fork verb is idempotent and concurrency-safe (two simultaneous
//! forks of the same generic produce EXACTLY ONE specific row and both
//! callers receive the SAME id).

use std::sync::Arc;

use uuid::Uuid;

use backbone_website::application::service::page_service::{
    CreatePageInput, PageAdminService,
};
use backbone_website::application::service::specificity::{
    resolve_page_by_url, resolve_specific, Resolution,
};
use backbone_website::application::service::versioning_service::{
    fork_to_website, ForkOutcome,
};
use backbone_website::application::service::website_error::WebsiteError;
use backbone_website::application::service::website_service::ActorRef;

use super::common::{make_website, make_website_with_homepage, TestDb};

fn officer() -> ActorRef {
    ActorRef::officer(Uuid::new_v4())
}

async fn make_generic(pages: &PageAdminService, key: &str, url: &str) -> uuid::Uuid {
    pages
        .create_page(
            officer(),
            CreatePageInput {
                key: key.into(),
                website_id: None,
                url: url.into(),
                title: format!("{key} generic"),
                seo_name: None,
                visibility: "public".into(),
                required_member_roles: vec![],
                website_indexed: true,
            },
        )
        .await
        .unwrap_or_else(|e| panic!("PROBE-FAIL: create generic {key}: {e:?}"))
        .id
}

/// The fold: a generic answers for EVERY website until a specific
/// exists; the specific then answers ONLY for its website, and the
/// generic keeps answering for every other website.
#[tokio::test]
async fn probe_fold_generic_then_specific() {
    let db = TestDb::new("fold").await;
    let w1 = make_website(&db.pool, "fold-a").await;
    let w2 = make_website(&db.pool, "fold-b").await;
    let pages = PageAdminService::new(db.pool.clone());

    let generic_id = make_generic(&pages, "about", "/about").await;
    pages.publish(officer(), generic_id).await.unwrap();

    // Generic answers for BOTH websites.
    for w in [&w1, &w2] {
        let got = pages.visible_page(w.id, "/about", None).await.unwrap();
        assert!(got.is_some(), "generic must answer for {}", w.id);
    }

    // Fork to W1: the specific (a copy) answers for W1 with the same
    // url; W2 keeps the generic.
    match fork_to_website(&db.pool, officer(), "about", w1.id).await.unwrap() {
        ForkOutcome::Created(p) => assert_ne!(p.id, generic_id),
        ForkOutcome::Existing(_) => panic!("PROBE-FAIL: first fork must create"),
    }
    match resolve_specific(&db.pool, "about", w1.id).await.unwrap() {
        Resolution::Specific(row) => {
            assert_ne!(row.id, generic_id, "fork must not re-return the generic");
            assert_eq!(row.url, "/about");
        }
        other => panic!("PROBE-FAIL: expected a specific for W1, got {other:?}"),
    }
    // W1 still resolves the url to ITS specific now.
    let via_url_w1 = pages.visible_page(w1.id, "/about", None).await.unwrap().unwrap();
    assert_ne!(via_url_w1.page.id, generic_id);

    // W2 still rides the generic (the fold prefers the specific but
    // W2 has none).
    let via_url_w2 = pages.visible_page(w2.id, "/about", None).await.unwrap().unwrap();
    assert_eq!(via_url_w2.page.id, generic_id);

    // Off-website rows are structurally invisible: a W1-only url
    // never answers for W2.
    let w1_only = pages
        .create_page(
            officer(),
            CreatePageInput {
                key: "w1-only".into(),
                website_id: Some(w1.id),
                url: "/only-w1".into(),
                title: "W1 only".into(),
                seo_name: None,
                visibility: "public".into(),
                required_member_roles: vec![],
                website_indexed: true,
            },
        )
        .await
        .unwrap()
        .id;
    pages.publish(officer(), w1_only).await.unwrap();
    assert!(
        resolve_page_by_url(&db.pool, "/only-w1", w2.id).await.unwrap().row().is_none(),
        "a W1-specific url must never resolve for W2"
    );
    assert!(
        resolve_page_by_url(&db.pool, "/only-w1", w1.id).await.unwrap().row().is_some(),
        "the W1-specific url must resolve for W1"
    );

    db.dispose().await;
}

/// The concurrent first-edit fork: many simultaneous forks of the same
/// generic into the same website produce EXACTLY ONE specific row,
/// every caller receives the SAME id, exactly one outcome is Created.
#[tokio::test]
async fn probe_concurrent_fork_single_row() {
    let db = TestDb::new("concfork").await;
    let website = make_website(&db.pool, "concfork").await;
    let pages = PageAdminService::new(db.pool.clone());
    let generic_id = make_generic(&pages, "landing", "/landing").await;
    pages.publish(officer(), generic_id).await.unwrap();

    const CALLERS: usize = 8;
    let pool = Arc::new(db.pool.clone());
    let target = website.id;
    let mut handles = Vec::new();
    for _ in 0..CALLERS {
        let pool = pool.clone();
        handles.push(tokio::spawn(async move {
            fork_to_website(&pool, officer(), "landing", target).await
        }));
    }
    let mut created = 0usize;
    let mut ids: Vec<uuid::Uuid> = Vec::new();
    for h in handles {
        let outcome = h
            .await
            .unwrap_or_else(|e| panic!("PROBE-FAIL: probe join: {e}"))
            .unwrap_or_else(|e| panic!("PROBE-FAIL: concurrent fork: {e:?}"));
        match outcome {
            ForkOutcome::Created(p) => {
                created += 1;
                ids.push(p.id);
            }
            ForkOutcome::Existing(p) => ids.push(p.id),
        }
    }
    assert_eq!(created, 1, "exactly one caller may create (got {created})");
    assert!(
        ids.iter().all(|id| *id == ids[0]),
        "every caller must receive the SAME winner id, got {ids:?}"
    );

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM website.pages WHERE key = 'landing' AND website_id = $1")
        .bind(target)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(rows, 1, "exactly ONE specific row may exist (got {rows})");

    db.dispose().await;
}

/// Duplicate specificity: a second live row on the same grain is
/// REFUSED by the partial unique and surfaces as the typed 409
/// (`website_specificity_conflict`).
#[tokio::test]
async fn probe_duplicate_specificity_typed_409() {
    let db = TestDb::new("dupgrain").await;
    let website = make_website(&db.pool, "dupgrain").await;
    let pages = PageAdminService::new(db.pool.clone());

    let input = |key: &'static str, url: &'static str| CreatePageInput {
        key: key.into(),
        website_id: Some(website.id),
        url: url.into(),
        title: "dup".into(),
        seo_name: None,
        visibility: "public".into(),
        required_member_roles: vec![],
        website_indexed: true,
    };

    pages.create_page(officer(), input("dup", "/dup-1")).await.unwrap();

    // Same (key, website) grain.
    let second = pages.create_page(officer(), input("dup", "/dup-2")).await;
    match second {
        Err(WebsiteError::SpecificityConflict { constraint }) => {
            assert!(constraint.contains("specificity"), "constraint name {constraint}");
            let e = WebsiteError::SpecificityConflict { constraint };
            assert_eq!(e.http_status(), 409);
            assert_eq!(e.code(), "website_specificity_conflict");
        }
        other => panic!("PROBE-FAIL: expected typed 409, got {other:?}"),
    }

    // Same (url, website) grain under a DIFFERENT key — the url-scope
    // fence.
    let url_dup = pages.create_page(officer(), input("dup2", "/dup-1")).await;
    match url_dup {
        Err(WebsiteError::SpecificityConflict { constraint }) => {
            assert!(constraint.contains("url_scope"), "constraint name {constraint}");
        }
        other => panic!("PROBE-FAIL: expected url-scope 409, got {other:?}"),
    }

    // And the DB fence itself: a raw second row cannot land.
    let raw = sqlx::query(
        r#"INSERT INTO website.pages (id, key, website_id, url, title) VALUES ($1, 'dup', $2, '/dup-raw', 'raw')"#,
    )
    .bind(Uuid::new_v4())
    .bind(website.id)
    .execute(&db.pool)
    .await;
    assert!(raw.is_err(), "the partial unique must refuse a raw duplicate too");

    db.dispose().await;
}

/// A plain delete on a GENERIC row is refused — the COU fanout verb is
/// the only generic deletion path.
#[tokio::test]
async fn probe_generic_delete_requires_fanout_verb() {
    let db = TestDb::new("gendel").await;
    let website = make_website(&db.pool, "gendel").await;
    let pages = PageAdminService::new(db.pool.clone());
    let generic_id = make_generic(&pages, "terms", "/terms").await;

    match pages.delete_specific(officer(), generic_id).await {
        Err(WebsiteError::GenericRequiresFanoutVerb) => {}
        other => panic!("PROBE-FAIL: expected GenericRequiresFanoutVerb, got {other:?}"),
    }

    // A SPECIFIC deletes locally, no questions asked.
    let spec = pages
        .create_page(
            officer(),
            CreatePageInput {
                key: "terms".into(),
                website_id: Some(website.id),
                url: "/terms".into(),
                title: "terms".into(),
                seo_name: None,
                visibility: "public".into(),
                required_member_roles: vec![],
                website_indexed: true,
            },
        )
        .await
        .unwrap();
    pages.delete_specific(officer(), spec.id).await.unwrap();

    db.dispose().await;
}

/// The bootstrap fork stamps THIS website's `homepage_url` onto its own
/// homepage page: `websites.homepage_url` is a per-site column, so a
/// second website bootstrapped after the first must not inherit the
/// first site's homepage url through the shared generic row (its
/// resolved `/` would 404 against its own routes).
#[tokio::test]
async fn probe_bootstrap_fork_stamps_own_homepage() {
    let db = TestDb::new("forkhp").await;
    let a = make_website_with_homepage(&db.pool, "forkhp-a", "/home-a").await;
    let b = make_website_with_homepage(&db.pool, "forkhp-b", "/home-b").await;

    let homepage_url_of = |website_id: uuid::Uuid| {
        let pool = db.pool.clone();
        async move {
            sqlx::query_scalar::<_, String>(
                r#"
                SELECT url FROM website.pages
                WHERE key = 'homepage' AND website_id = $1
                  AND (metadata->>'deleted_at') IS NULL
                "#,
            )
            .bind(website_id)
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|e| panic!("PROBE-FAIL: read homepage page: {e:?}"))
        }
    };

    // BOTH websites resolve their own homepage through the ONE
    // resolver with the url each bootstrap declared.
    let url_a = homepage_url_of(a.id).await;
    let url_b = homepage_url_of(b.id).await;
    assert_eq!(url_a, "/home-a", "the first website's fork carries its own url");
    assert_eq!(
        url_b, "/home-b",
        "the second website's fork must stamp ITS homepage_url, not the generic row's url"
    );

    // And the declared per-site column agrees with the forked page.
    for (site, expected) in [(a.id, "/home-a"), (b.id, "/home-b")] {
        let declared: String = sqlx::query_scalar(
            r#"SELECT homepage_url FROM website.websites WHERE id = $1"#,
        )
        .bind(site)
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert_eq!(declared, expected);
    }

    db.dispose().await;
}
