//! Public-surface probes: the §7.3 negative-probe contract (the public
//! read allowlist's DoD gate) and the §9 route-table mount assertions
//! (probe class 11).
//!
//! Every request runs UNAUTHENTICATED through the exported public
//! router — hostname-bound exactly like production traffic.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use backbone_website::application::service::intake_engine::TurnstileConfig;
use backbone_website::application::service::menu_service::{
    CreateMenuInput, MenuAdminService,
};
use backbone_website::application::service::page_service::{
    CreatePageInput, PageAdminService, PagePatch,
};
use backbone_website::application::service::versioning_service::fork_to_website;
use backbone_website::application::service::website_service::ActorRef;
use backbone_website::presentation::http::public_routes::{
    website_public_routes, WebsitePublicState,
};

use super::common::{host_of, make_website, TestDb, PROBE_PEPPER};

fn officer() -> ActorRef {
    ActorRef::officer(Uuid::new_v4())
}

fn router(pool: &sqlx::PgPool) -> axum::Router {
    let state = WebsitePublicState::compose(
        pool.clone(),
        PROBE_PEPPER.to_string(),
        TurnstileConfig { secret: Some("stub".into()), verify_url: "http://127.0.0.1:9/x".to_string() },
    );
    website_public_routes(state)
}

async fn get(app: &axum::Router, host: &str, path: &str) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(path)
                .header("host", host)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap_or_else(|e| panic!("PROBE-FAIL: probe request {path}: {e}"));
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, body)
}

/// The §7.3 seed: website A (P1 published generic + its specific fork on
/// A, P2 unpublished, P3 indexed=false, P4 restricted) + website B (P5
/// published, its own fork of P1's key).
async fn seed_negative_contract(
    pool: &sqlx::PgPool,
) -> (backbone_website::application::service::website_service::WebsiteView, String, String, String, String) {
    let a = make_website(pool, "neg-a").await;
    let b = make_website(pool, "neg-b").await;
    let pages = PageAdminService::new(pool.clone());

    let mk = |website: Option<Uuid>, key: &str, url: &str, visibility: &str| CreatePageInput {
        key: key.into(),
        website_id: website,
        url: url.into(),
        title: format!("{key} on {:?}", website.is_some()),
        seo_name: None,
        visibility: visibility.into(),
        required_member_roles: vec![],
        website_indexed: true,
    };

    // P1: the published GENERIC (answers for A until A's fork shadows).
    let p1 = pages.create_page(officer(), mk(None, "shared", "/shared", "public")).await.unwrap();
    pages.publish(officer(), p1.id).await.unwrap();

    // P2: unpublished specific on A.
    let p2 = pages
        .create_page(officer(), mk(Some(a.id), "draft", "/draft", "public"))
        .await
        .unwrap();

    // P3: published but website_indexed = false on A.
    let mut input = mk(Some(a.id), "unindexed", "/unindexed", "public");
    input.website_indexed = false;
    let p3 = pages.create_page(officer(), input).await.unwrap();
    pages.publish(officer(), p3.id).await.unwrap();

    // P4: published + restricted on A.
    let p4 = pages
        .create_page(officer(), mk(Some(a.id), "members", "/members", "restricted"))
        .await
        .unwrap();
    pages.publish(officer(), p4.id).await.unwrap();

    // P5: published specific on B only.
    let p5 = pages
        .create_page(officer(), mk(Some(b.id), "b-only", "/b-only", "public"))
        .await
        .unwrap();
    pages.publish(officer(), p5.id).await.unwrap();

    // A's specific fork of P1's key — content DISTINCT from the generic
    // (the fork is a copy; patch its title to prove which row serves).
    let a_fork = match fork_to_website(pool, officer(), "shared", a.id).await.unwrap() {
        backbone_website::application::service::versioning_service::ForkOutcome::Created(p) => p,
        _ => panic!("PROBE-FAIL: A's fresh fork must create"),
    };
    pages
        .patch_page(
            officer(),
            a_fork.id,
            PagePatch {
                title: Some("A-fork content".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    pages.publish(officer(), a_fork.id).await.unwrap();

    // B's own fork of the same key (its copy, its content).
    match fork_to_website(pool, officer(), "shared", b.id).await.unwrap() {
        backbone_website::application::service::versioning_service::ForkOutcome::Created(p) => {
            pages.publish(officer(), p.id).await.unwrap();
        }
        _ => panic!("PROBE-FAIL: B's fresh fork must create"),
    }

    // Menus on A: a public child + a connected-tier child + a restricted
    // child under A's bootstrap root menu.
    let menus = MenuAdminService::new(pool.clone());
    let root: Uuid = sqlx::query_scalar(
        "SELECT id FROM website.menus WHERE website_id = $1 AND parent_id IS NULL \
         AND (metadata->>'deleted_at') IS NULL ORDER BY sequence, id LIMIT 1",
    )
    .bind(a.id)
    .fetch_one(pool)
    .await
    .unwrap();
    for (name, visibility) in
        [("A-public-entry", "public"), ("A-connected-entry", "connected"), ("A-restricted-entry", "restricted")]
    {
        menus
            .create_menu(
                officer(),
                CreateMenuInput {
                    website_id: a.id,
                    parent_id: Some(root),
                    name: name.into(),
                    page_id: None,
                    url: Some(format!("https://{name}.example")),
                    new_window: false,
                    sequence: 5,
                    visibility: visibility.into(),
                    required_member_roles: vec![],
                    is_mega_menu: false,
                },
            )
            .await
            .unwrap();
    }

    (a, p2.url.clone(), p3.url.clone(), p4.url.clone(), p5.url.clone())
}

/// The full §7.3 negative contract, from one unauthenticated caller
/// bound to A's host.
#[tokio::test]
async fn probe_public_negative_contract() {
    let db = TestDb::new("negsurface").await;
    let (a, p2_url, p3_url, p4_url, p5_url) = seed_negative_contract(&db.pool).await;
    let app = router(&db.pool);
    let host = host_of(&a);

    // Unpublished → 404 (existence hidden).
    let (s, _) = get(&app, &host, &format!("/public/pages/{p2_url}")).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "unpublished page must 404: {p2_url}");

    // Off-website → 404 (B's P5 is structurally invisible to A).
    let (s, _) = get(&app, &host, &format!("/public/pages/{p5_url}")).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "off-website page must 404: {p5_url}");

    // Published + restricted → the typed 403.
    let (s, body) = get(&app, &host, &format!("/public/pages/{p4_url}")).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "restricted page must 403");
    assert_eq!(body["code"], "website_page_visibility_refused");

    // P1's url → 200 with the FORK's content (the specific shadows the
    // generic on A).
    let (s, body) = get(&app, &host, "/public/pages/shared").await;
    assert_eq!(s, StatusCode::OK, "the forked shared page must serve on A");
    assert_eq!(
        body["page"]["title"], "A-fork content",
        "A must see ITS fork, not the generic"
    );

    // Sitemap: P1's url present; P2/P3/P5's urls absent.
    let (s, body) = get(&app, &host, "/public/sitemap").await;
    assert_eq!(s, StatusCode::OK);
    let urls = body["urls"]
        .as_array()
        .unwrap_or_else(|| panic!("PROBE-FAIL: sitemap urls shape: {body}"))
        .clone();
    let contains = |u: &str| urls.iter().any(|v| v.as_str() == Some(u));
    assert!(contains("/shared"), "the published fork serves the sitemap: {urls:?}");
    assert!(!contains(&p2_url), "unpublished pages never sitemap");
    assert!(!contains(&p3_url), "website_indexed = false pages never sitemap");
    assert!(!contains(&p5_url), "off-website pages never sitemap on A");

    // Menus: only public-tier entries reach an anonymous caller.
    let (s, body) = get(&app, &host, "/public/menus").await;
    assert_eq!(s, StatusCode::OK);
    let rendered = body.to_string();
    assert!(rendered.contains("A-public-entry"), "public entries reach anonymous: {rendered}");
    assert!(
        !rendered.contains("A-connected-entry"),
        "connected entries must not reach anonymous (port unwired → fail-closed)"
    );
    assert!(!rendered.contains("A-restricted-entry"), "restricted entries never reach anonymous");

    // The website NOT bound by this host never bleeds in.
    let (s, body) = get(&app, "no-such-host.example", "/public/menus").await;
    assert_eq!(s, StatusCode::NOT_FOUND, "an unbound host is a loud 404");
    assert_eq!(body["code"], "website_not_resolved");

    db.dispose().await;
}

/// The heartbeat honors the declared no-op headers (bot suppression,
/// tracking opt-out): a 200 `tracked:false` and NO visitor row.
#[tokio::test]
async fn probe_heartbeat_noop_headers() {
    let db = TestDb::new("noophdr").await;
    let website = make_website(&db.pool, "noophdr").await;
    let app = router(&db.pool);

    for header in ["x-bot", "x-disable-tracking"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/public/visitors/heartbeat")
                    .header("host", host_of(&website))
                    .header(header, "true")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({ "session_id": "s-noop", "url": "/" }).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "header {header}");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["tracked"], false, "header {header} is a declared no-op");
    }

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM website.visitors")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0, "no-op headers never mint visitor rows");

    db.dispose().await;
}

/// The public route table registers EXACTLY §7.2's seven route groups —
/// a grep-asserted route table over the router's source, plus
/// behavioral 404s for every enumeration shape the table must not have.
#[test]
fn probe_public_route_table_is_exactly_the_allowlist() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let path = std::path::Path::new(manifest)
        .join("src")
        .join("presentation")
        .join("http")
        .join("public_routes.rs");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("PROBE-FAIL: cannot read {}: {e}", path.display()));

    let mut registered: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(".route(\"") {
            let Some(end) = rest.find('"') else { continue };
            registered.push(rest[..end].to_string());
        }
    }
    let mut expected = vec![
        "/public/pages/*url",
        "/public/menus",
        "/public/sitemap",
        "/public/robots.txt",
        "/public/resolve",
        "/public/visitors/heartbeat",
        "/public/intake/:verb",
    ];
    expected.sort();
    registered.sort();
    assert_eq!(
        registered, expected,
        "the public router must register EXACTLY the §7.2 allowlist"
    );
}

/// No enumeration verb exists anywhere on the public tree: the classic
/// list shapes all answer 404 through the real router.
#[tokio::test]
async fn probe_no_public_enumeration_verbs() {
    let db = TestDb::new("noenum").await;
    let website = make_website(&db.pool, "noenum").await;
    let app = router(&db.pool);

    let host = host_of(&website);
    for path in [
        "/public/pages",
        "/public/pages-list",
        "/public/websites",
        "/public/menus/all",
        "/admin/pages",
        "/admin/websites",
        "/api/v1/website/admin/visitors",
        "/public/visitors",
        "/public/intake",
    ] {
        let (s, _) = get(&app, &host, path).await;
        assert_eq!(
            s,
            StatusCode::NOT_FOUND,
            "no public answer for {path} — the allowlist is exhaustive"
        );
    }

    db.dispose().await;
}

/// The admin router's registered table is exactly §9.2's (19 paths,
/// every one under `/admin/**`), and its doc contract names the module
/// write gate `website` — the host composes the gate itself; the module
/// exports the pure router.
#[test]
fn probe_admin_route_table_and_gate_name() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let path = std::path::Path::new(manifest)
        .join("src")
        .join("presentation")
        .join("http")
        .join("admin_routes.rs");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("PROBE-FAIL: cannot read {}: {e}", path.display()));

    let mut registered: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(".route(\"") {
            let Some(end) = rest.find('"') else { continue };
            registered.push(rest[..end].to_string());
        }
    }
    assert!(!registered.is_empty(), "the admin router must register its table");
    assert!(
        registered.iter().all(|r| r.starts_with("/admin/")),
        "every admin route sits under /admin/**: {registered:?}"
    );
    registered.sort();
    let mut expected = vec![
        "/admin/websites",
        "/admin/websites/:id",
        "/admin/pages",
        "/admin/pages/fork",
        "/admin/pages/fanout-delete",
        "/admin/pages/:id",
        "/admin/pages/:id/publish",
        "/admin/pages/:id/unpublish",
        "/admin/pages/:id/rename",
        "/admin/blocks/page/:page_id",
        "/admin/menus",
        "/admin/menus/:id",
        "/admin/menus/:id/fanout",
        "/admin/redirects",
        "/admin/redirects/:id",
        "/admin/visitors",
        "/admin/visitors/merge",
        "/admin/visitors/sweep",
        "/admin/intake/contact-messages",
    ];
    expected.sort();
    assert_eq!(registered, expected, "the admin table is exactly §9.2's");

    // The mount contract is documented ON the router (the host reads
    // this to compose the gate innermost) — assert it names the gate.
    assert!(
        text.contains("ModuleWriteGate::new(pool, \"website\")"),
        "the admin router's doc must name the module write gate `website`"
    );
}
