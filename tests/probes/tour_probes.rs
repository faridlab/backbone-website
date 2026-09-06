//! Tour persistence probes (the web_tour port): the live-name unique
//! grain (upsert updates, never duplicates; a soft-deleted name
//! recreates as a NEW row), the idempotent per-principal consumption
//! M2M (second consume = AlreadyConsumed; principals independent;
//! reset drops every principal's row), the typed 404 on an unknown
//! tour, and the fail-closed principal tree (unwired verifier or a
//! missing header = the typed 401; a verified bearer = the outcome
//! arm; `/tours/consumed` keeps static-over-param priority over
//! `/tours/:name`).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;
use uuid::Uuid;

use backbone_portal::exports::PortalUserId;
use backbone_website::application::service::principal_port::{
    WebsitePrincipal, WebsitePrincipalVerifier,
};
use backbone_website::application::service::tour_service::{
    ConsumeOutcome, TourService, UpsertTourInput,
};
use backbone_website::application::service::website_error::WebsiteError;
use backbone_website::application::service::website_service::ActorRef;
use backbone_website::presentation::http::tour_routes::{tour_routes, TourPrincipalState};

use super::common::TestDb;

fn officer() -> ActorRef {
    ActorRef::officer(Uuid::new_v4())
}

fn input(name: &str) -> UpsertTourInput {
    UpsertTourInput {
        name: name.into(),
        display_name: name.into(),
        rainbow_man_message: Some("bravo!".into()),
        steps: serde_json::json!([{ "element": "#navbar", "position": "bottom" }]),
    }
}

/// A deterministic probe verifier: `good-<uuid>` verifies as that
/// portal user, anything else refuses.
struct TokenVerifier;

impl WebsitePrincipalVerifier for TokenVerifier {
    fn verify<'a>(
        &'a self,
        token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Option<WebsitePrincipal>> + Send + 'a>> {
        Box::pin(async move {
            let id = token.strip_prefix("good-")?.parse::<Uuid>().ok()?;
            Some(WebsitePrincipal {
                user_id: PortalUserId::from(id),
                email: "probe@example.com".into(),
            })
        })
    }
}

/// The live-name grain: upsert is create-or-update BY NAME (id stable,
/// fields move), a different name is a different row, and a
/// soft-deleted name recreates as a NEW row (the unique index fences
/// only LIVE rows).
#[tokio::test]
async fn probe_tour_name_unique_among_live_rows() {
    let db = TestDb::new("tourname").await;
    let tours = TourService::new(db.pool.clone());

    let a = tours
        .upsert_tour(officer(), input("onboarding"))
        .await
        .unwrap_or_else(|e| panic!("PROBE-FAIL: upsert onboarding: {e:?}"));
    let b = tours
        .upsert_tour(
            officer(),
            UpsertTourInput {
                name: "onboarding".into(),
                display_name: "Onboarding v2".into(),
                rainbow_man_message: None,
                steps: serde_json::json!([]),
            },
        )
        .await
        .unwrap();
    assert_eq!(a.id, b.id, "same name must update the same live row");
    assert_eq!(b.display_name, "Onboarding v2");

    let c = tours
        .upsert_tour(officer(), input("settings"))
        .await
        .unwrap();
    assert_ne!(a.id, c.id);
    assert_eq!(tours.list_tours().await.unwrap().len(), 2);

    // Soft delete frees the name for a fresh row.
    tours.delete_tour(officer(), "onboarding").await.unwrap();
    assert!(tours.tour_by_name("onboarding").await.unwrap().is_none());
    let d = tours
        .upsert_tour(officer(), input("onboarding"))
        .await
        .unwrap();
    assert_ne!(a.id, d.id, "a soft-deleted name recreates as a NEW row");

    db.dispose().await;
}

/// The consumption M2M: idempotent per principal, independent across
/// principals, unknown tour = the typed 404, reset clears every
/// principal's rows.
#[tokio::test]
async fn probe_tour_consumption_idempotent_and_per_principal() {
    let db = TestDb::new("tourconsume").await;
    let tours = TourService::new(db.pool.clone());
    tours
        .upsert_tour(officer(), input("onboarding"))
        .await
        .unwrap();

    let alice = Uuid::new_v4();
    let bob = Uuid::new_v4();

    match tours.consume_tour(alice, "onboarding").await.unwrap() {
        ConsumeOutcome::Consumed { .. } => {}
        other => panic!("PROBE-FAIL: first consume must be Consumed, got {other:?}"),
    }
    let alice_at = match tours.consume_tour(alice, "onboarding").await.unwrap() {
        ConsumeOutcome::AlreadyConsumed { consumed_at } => consumed_at,
        other => panic!("PROBE-FAIL: second consume must be AlreadyConsumed, got {other:?}"),
    };

    match tours.consume_tour(bob, "onboarding").await.unwrap() {
        ConsumeOutcome::Consumed { consumed_at } => assert!(consumed_at >= alice_at),
        other => panic!("PROBE-FAIL: bob's first consume must be Consumed, got {other:?}"),
    }
    assert_eq!(tours.consumed_for(alice).await.unwrap().len(), 1);
    assert_eq!(tours.consumed_for(bob).await.unwrap().len(), 1);

    match tours.consume_tour(alice, "ghost").await {
        Err(WebsiteError::TourNotFound) => {}
        other => panic!("PROBE-FAIL: unknown tour must be TourNotFound, got {other:?}"),
    }

    let dropped = tours.reset_tour(officer(), "onboarding").await.unwrap();
    assert_eq!(dropped, 2, "reset drops BOTH principals' rows");
    assert!(tours.consumed_for(alice).await.unwrap().is_empty());
    assert!(tours.consumed_for(bob).await.unwrap().is_empty());
    match tours.consume_tour(alice, "onboarding").await.unwrap() {
        ConsumeOutcome::Consumed { .. } => {}
        other => panic!("PROBE-FAIL: consume after reset must be Consumed, got {other:?}"),
    }

    db.dispose().await;
}

/// The principal tree: fail-closed on the unwired verifier and on the
/// missing header (the typed 401 with the stable code), the consume
/// verb answering its outcome arm for a verified bearer, and
/// `/tours/consumed` keeping static-over-param priority over
/// `/tours/:name` (no tour named "consumed" is minted, so a 200 with
/// the consumed-set shape is unambiguous).
#[tokio::test]
async fn probe_tour_principal_tree_is_fail_closed() {
    let db = TestDb::new("tourtree").await;
    let tours = TourService::new(db.pool.clone());
    tours
        .upsert_tour(officer(), input("onboarding"))
        .await
        .unwrap();
    let alice = Uuid::new_v4();

    // Unwired (the refusing default): a bearer still reads the typed 401.
    let resp = tour_routes(TourPrincipalState::new(db.pool.clone()))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tours/onboarding/consume")
                .header("authorization", "Bearer anything")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["code"], "website_tour_principal_required");

    // No header at all: the same typed 401.
    let resp = tour_routes(TourPrincipalState::new(db.pool.clone()))
        .oneshot(Request::builder().uri("/tours").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // A verified bearer: consume answers its outcome arm...
    let mut state = TourPrincipalState::new(db.pool.clone());
    state.install_principal_verifier(Arc::new(TokenVerifier));
    let app = tour_routes(state);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/tours/onboarding/consume")
                .header("authorization", format!("Bearer good-{alice}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["outcome"], "consumed");

    // ...and /tours/consumed (static) is NOT captured by /tours/:name.
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/tours/consumed")
                .header("authorization", format!("Bearer good-{alice}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["consumed"][0]["name"], "onboarding");

    db.dispose().await;
}
