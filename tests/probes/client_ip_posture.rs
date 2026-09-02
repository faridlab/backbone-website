//! Client-IP posture probes (the trusted-proxy contract): the caller
//! address feeding the visitor digest and the intake Tier-B limiter is
//! resolved by the module's HTTP layer — the RIGHTMOST forwarded hop
//! when a trusted proxy is declared, else the socket address — so a
//! caller CANNOT dodge rate buckets or mint fresh visitor identities
//! by rotating the client-supplied hops of `X-Forwarded-For`.
//!
//! Every request runs through the EXPORTED public router with a fixed
//! `ConnectInfo` extension (the socket the host's
//! `into_make_service_with_connect_info` provides in production), the
//! same hostname binding production traffic uses, and a PASSING local
//! siteverify stub (turnstile runs before the rate arms, so the bucket
//! probe must clear it with a real HTTP answer).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use backbone_website::application::service::intake_engine::TurnstileConfig;
use backbone_website::presentation::http::public_routes::{
    website_public_routes, WebsitePublicState,
};

use super::common::{host_of, make_website, siteverify_stub, TestDb, PROBE_PEPPER};

/// The socket every request in this family arrives from (the address
/// the nearest trusted proxy actually dialed — constant per probe).
const PROBE_SOCKET: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 9)), 41414);

fn app(pool: &sqlx::PgPool, trusted_proxy: bool, verify_url: &str) -> axum::Router {
    let state = WebsitePublicState::compose_with_trusted_proxy(
        pool.clone(),
        PROBE_PEPPER.to_string(),
        TurnstileConfig {
            secret: Some("stub-secret".into()),
            verify_url: verify_url.to_string(),
        },
        trusted_proxy,
    );
    website_public_routes(state)
}

async fn send(
    app: &axum::Router,
    host: &str,
    method: &str,
    path: &str,
    xff: Option<&str>,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", host)
        .header("user-agent", "probe-agent/1.0")
        // The host's connect-info wiring, faithfully replayed.
        .extension(ConnectInfo(PROBE_SOCKET));
    if let Some(forwarded) = xff {
        builder = builder.header("x-forwarded-for", forwarded);
    }
    if path.starts_with("/public/intake/") {
        // The turnstile gate runs before the rate arms — carry a token
        // the passing stub accepts.
        builder = builder.header("x-turnstile-token", "probe-turnstile-token");
    }
    let request = match body {
        Some(json) => builder
            .header("content-type", "application/json")
            .body(Body::from(json.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = app
        .clone()
        .oneshot(request)
        .await
        .unwrap_or_else(|e| panic!("PROBE-FAIL: probe request {path}: {e}"));
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let parsed = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, parsed)
}

fn contact_body() -> serde_json::Value {
    serde_json::json!({
        "name": "Probe Sender",
        "email": "sender@example.com",
        "message": "client-ip posture probe",
    })
}

/// 30 contact intakes, one constant socket, ONLY the client-supplied
/// FIRST forwarded hop rotating (the rightmost hop — the entry the
/// trusted proxy recorded — stays fixed). A first-hop reader would see
/// 30 distinct addresses and never trip a bucket; the resolved
/// constant address carries every request into the one anonymous IP
/// window (limit 20) and trips it on the 21st request.
#[tokio::test]
async fn probe_intake_bucket_ignores_spoofed_first_hop() {
    let db = TestDb::new("clipip1").await;
    let website = make_website(&db.pool, "clipip1").await;
    let host = host_of(&website);
    let passing = siteverify_stub(r#"{"success":true}"#.into(), 32);
    let router = app(&db.pool, true, &passing);

    let mut first_limited = None;
    let mut accepted = 0usize;
    for i in 0..30u32 {
        // First hop rotates (spoofed); the proxy-recorded rightmost
        // hop is constant.
        let xff = format!("203.0.113.{}, 198.51.100.7", 10 + i);
        let (status, body) = send(
            &router,
            &host,
            "POST",
            "/public/intake/contact",
            Some(&xff),
            Some(contact_body()),
        )
        .await;
        match status {
            StatusCode::CREATED => accepted += 1,
            StatusCode::TOO_MANY_REQUESTS => {
                if first_limited.is_none() {
                    first_limited = Some((i + 1, body));
                }
            }
            other => panic!(
                "PROBE-FAIL: request {} answered {other}, body {body:?} — spoofed first hop must not change the intake answer",
                i + 1
            ),
        }
    }
    let (at, body) = first_limited
        .unwrap_or_else(|| panic!("PROBE-FAIL: 30 rotating-first-hop intakes never tripped a rate bucket — the limiter is keyed on a spoofable hop"));
    assert_eq!(
        at, 21,
        "the constant resolved address must fill the anonymous IP window (limit 20) and refuse request 21"
    );
    assert_eq!(accepted, 20, "exactly the first twenty submissions persist");
    assert!(
        body.get("retry_after_seconds").is_some(),
        "the 429 carries the machine hint: {body:?}"
    );
    db.dispose().await;
}

/// Without a declared trusted proxy the forwarded header is ignored
/// ENTIRELY: a caller rotating every hop of it still lands on the one
/// socket address and trips the same bucket — the fail-closed default.
#[tokio::test]
async fn probe_intake_bucket_ignores_forwarded_header_without_proxy() {
    let db = TestDb::new("clipip2").await;
    let website = make_website(&db.pool, "clipip2").await;
    let host = host_of(&website);
    let passing = siteverify_stub(r#"{"success":true}"#.into(), 32);
    let router = app(&db.pool, false, &passing);

    let mut first_limited = None;
    for i in 0..30u32 {
        // The whole header rotates — nothing in it is trustworthy.
        let xff = format!("203.0.113.{}", 10 + i);
        let (status, _) = send(
            &router,
            &host,
            "POST",
            "/public/intake/contact",
            Some(&xff),
            Some(contact_body()),
        )
        .await;
        if status == StatusCode::TOO_MANY_REQUESTS && first_limited.is_none() {
            first_limited = Some(i + 1);
        } else if i < 20 {
            assert_eq!(
                status,
                StatusCode::CREATED,
                "request {} must persist while the bucket holds",
                i + 1
            );
        }
    }
    assert_eq!(
        first_limited,
        Some(21),
        "the socket address — not the rotating header — keys the anonymous IP window (limit 20)"
    );
    db.dispose().await;
}

/// The visitor digest consumes the SAME resolved address: rotating the
/// spoofed first hop never mints a new visitor, while a genuinely
/// different proxy-recorded address does.
#[tokio::test]
async fn probe_heartbeat_visitor_stable_under_spoofed_first_hop() {
    let db = TestDb::new("clipip3").await;
    let website = make_website(&db.pool, "clipip3").await;
    let host = host_of(&website);
    let router = app(&db.pool, true, "http://127.0.0.1:9/siteverify");

    let mut visitor_ids = Vec::new();
    for i in 0..3u32 {
        let xff = format!("203.0.113.{}, 198.51.100.7", 10 + i);
        let (status, body) = send(
            &router,
            &host,
            "POST",
            "/public/visitors/heartbeat",
            Some(&xff),
            Some(serde_json::json!({
                "session_id": "probe-client-ip-session",
                "url": "/anywhere",
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "heartbeat body {body:?}");
        let id = body["visitor_id"]
            .as_str()
            .unwrap_or_else(|| panic!("PROBE-FAIL: heartbeat lacks visitor_id: {body:?}"))
            .to_string();
        visitor_ids.push(id);
    }
    assert!(
        visitor_ids[0] == visitor_ids[1] && visitor_ids[1] == visitor_ids[2],
        "rotating the spoofed first hop must not mint visitors: {visitor_ids:?}"
    );

    // Control: a DIFFERENT proxy-recorded address is a different
    // client — the digest must actually distinguish.
    let (status, body) = send(
        &router,
        &host,
        "POST",
        "/public/visitors/heartbeat",
        Some("203.0.113.99, 198.51.100.8"),
        Some(serde_json::json!({
            "session_id": "probe-client-ip-session",
            "url": "/anywhere",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "control heartbeat body {body:?}");
    let other = body["visitor_id"]
        .as_str()
        .unwrap_or_else(|| panic!("PROBE-FAIL: control heartbeat lacks visitor_id: {body:?}"));
    assert_ne!(
        other,
        visitor_ids[0],
        "a different proxy-recorded address is a different client"
    );
    db.dispose().await;
}
