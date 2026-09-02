//! Publish-fence probes (WS-2/WS-6 shape): `is_published` moves ONLY
//! through the dedicated verbs, `date_publish` is the lazy
//! first-publish instant, and per-website visibility never leaks
//! across the fold.

use uuid::Uuid;

use backbone_website::application::service::page_service::{
    CreatePageInput, PageAdminService, PUBLISH_FENCED_FIELDS,
};
use backbone_website::application::service::website_service::ActorRef;

use super::common::{make_website, TestDb};

fn officer() -> ActorRef {
    ActorRef::officer(Uuid::new_v4())
}

async fn make_specific(pages: &PageAdminService, website: Option<Uuid>, key: &str, url: &str) -> uuid::Uuid {
    pages
        .create_page(
            officer(),
            CreatePageInput {
                key: key.into(),
                website_id: website,
                url: url.into(),
                title: format!("{key} title"),
                seo_name: None,
                visibility: "public".into(),
                required_member_roles: vec![],
                website_indexed: true,
            },
        )
        .await
        .unwrap_or_else(|e| panic!("PROBE-FAIL: create page {key}: {e:?}"))
        .id
}

/// Unpublished → invisible (the 404 hides existence); publish →
/// visible; unpublish → invisible again, with `date_publish` pinned to
/// the FIRST publish instant across the republish.
#[tokio::test]
async fn probe_publish_unpublish_visibility_and_lazy_date() {
    let db = TestDb::new("pubfence").await;
    let website = make_website(&db.pool, "pubfence").await;
    let pages = PageAdminService::new(db.pool.clone());
    let page = make_specific(&pages, Some(website.id), "guide", "/guide").await;

    // Unpublished rows are invisible to the public read.
    assert!(
        pages.visible_page(website.id, "/guide", None).await.unwrap().is_none(),
        "an unpublished page must not be served"
    );

    // Publish: lazy date_publish lands NOW.
    pages.publish(officer(), page).await.unwrap();
    let served = pages.visible_page(website.id, "/guide", None).await.unwrap();
    assert!(served.is_some(), "a published page must be served");
    let first_date: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT date_publish FROM website.pages WHERE id = $1")
            .bind(page)
            .fetch_one(&db.pool)
            .await
            .unwrap();

    // Unpublish: invisible again, date_publish RETAINED.
    pages.unpublish(officer(), page).await.unwrap();
    assert!(pages.visible_page(website.id, "/guide", None).await.unwrap().is_none());
    let kept: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT date_publish FROM website.pages WHERE id = $1")
            .bind(page)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(kept.map(|d| d.timestamp()), Some(first_date.timestamp()));

    // Republish: the lazy date keeps the FIRST publish instant.
    pages.publish(officer(), page).await.unwrap();
    let again: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT date_publish FROM website.pages WHERE id = $1")
            .bind(page)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(
        again.map(|d| d.timestamp()),
        Some(first_date.timestamp()),
        "republish must not move the first-publish instant"
    );

    // The fence is declared, and complete.
    assert_eq!(PUBLISH_FENCED_FIELDS, &["is_published", "date_publish"]);

    db.dispose().await;
}

/// Per-website visibility: publishing a website's specific says
/// NOTHING about the generic or any other website's copy — each row
/// carries its own is_published.
#[tokio::test]
async fn probe_per_website_publish_independence() {
    let db = TestDb::new("pubper").await;
    let w1 = make_website(&db.pool, "pub-a").await;
    let w2 = make_website(&db.pool, "pub-b").await;
    let pages = PageAdminService::new(db.pool.clone());

    // The generic, unpublished.
    let generic = make_specific(&pages, None, "shared", "/shared").await;

    // W1's specific: publish it (bootstrap gives W1 a homepage fork;
    // create the /shared fork through the fork verb).
    let w1_specific = backbone_website::application::service::versioning_service::fork_to_website(
        &db.pool, officer(), "shared", w1.id,
    )
    .await
    .unwrap();
    let w1_id = match w1_specific {
        backbone_website::application::service::versioning_service::ForkOutcome::Created(p) => p.id,
        backbone_website::application::service::versioning_service::ForkOutcome::Existing(_) => {
            panic!("PROBE-FAIL: fresh fork must create")
        }
    };
    pages.publish(officer(), w1_id).await.unwrap();

    // W1 sees the page (its own published specific)...
    assert!(pages.visible_page(w1.id, "/shared", None).await.unwrap().is_some());
    // ...W2 does NOT (the generic is still unpublished).
    assert!(
        pages.visible_page(w2.id, "/shared", None).await.unwrap().is_none(),
        "publishing W1's specific must not publish the generic"
    );

    // Publish the generic: W2 now sees it — still independently.
    pages.publish(officer(), generic).await.unwrap();
    assert!(pages.visible_page(w2.id, "/shared", None).await.unwrap().is_some());

    // Unpublish the generic: W1 keeps its own copy; W2 loses the page.
    pages.unpublish(officer(), generic).await.unwrap();
    assert!(pages.visible_page(w1.id, "/shared", None).await.unwrap().is_some());
    assert!(pages.visible_page(w2.id, "/shared", None).await.unwrap().is_none());

    db.dispose().await;
}

/// The patch route fence: a generic PATCH body carrying a fenced field
/// is refused with the typed 422 and audited `publish_refused` —
/// through the ADMIN ROUTER (the layer that enforces it).
#[tokio::test]
async fn probe_patch_publish_fence_via_router() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let db = TestDb::new("patchfence").await;
    let website = make_website(&db.pool, "patchfence").await;
    let pages = PageAdminService::new(db.pool.clone());
    let page = make_specific(&pages, Some(website.id), "fenced", "/fenced").await;

    let state = backbone_website::presentation::http::admin_routes::WebsiteAdminState::new(
        db.pool.clone(),
        super::common::PROBE_PEPPER.to_string(),
    );
    let app = backbone_website::presentation::http::admin_routes::website_admin_routes(state);

    for fenced_field in ["is_published", "date_publish"] {
        let body = serde_json::json!({ fenced_field: true });
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri(format!("/admin/pages/{page}"))
                    .header("content-type", "application/json")
                    .extension(backbone_website::presentation::http::admin_routes::WebsiteActor(
                        Uuid::new_v4(),
                    ))
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "fenced field {fenced_field} must be a 422"
        );
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            parsed["code"], "website_field_not_patchable",
            "fenced field {fenced_field} refusal code"
        );
    }

    // The refusal is AUDITED (publish_refused) — one row per attempt.
    let refusals: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.website_audit_log WHERE event = 'publish_refused'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(refusals, 2, "each fenced attempt is audited");

    // And the row really is untouched.
    let (published, date): (bool, Option<chrono::DateTime<chrono::Utc>>) =
        sqlx::query_as("SELECT is_published, date_publish FROM website.pages WHERE id = $1")
            .bind(page)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(!published);
    assert!(date.is_none());

    db.dispose().await;
}
