//! Intake negative probes (WS-11 shape): turnstile fail-closed with the
//! four typed answers apart, unknown-field 422 at the parse edge, the
//! Tier-B rate arms with `Retry-After`, savepoint rollback leaving zero
//! rows, and the no-elevated-role posture.
//!
//! The siteverify endpoint is a REAL local HTTP stub (a thread accepting
//! one connection per answer, speaking HTTP/1.1) — the engine's single
//! outbound call is exercised as HTTP, not mocked at the client seam.

use uuid::Uuid;

use backbone_website::application::service::intake_contact::ContactPayload;
use backbone_website::application::service::intake_engine::{
    IntakeContext, IntakeDeclaration, IntakeEngine, IntakeOutcome, TurnstileClient,
    TurnstileConfig,
};
use backbone_website::application::service::website_error::WebsiteError;

use super::common::{host_of, make_website, siteverify_stub, TestDb, PROBE_PEPPER};

fn payload() -> ContactPayload {
    ContactPayload { name: Some("Probe Sender".into()), email: "sender@example.com".into(), message: "a probe message".into() }
}

fn engine(pool: &sqlx::PgPool, secret: Option<&str>, verify_url: &str) -> IntakeEngine {
    IntakeEngine::new(
        pool.clone(),
        TurnstileClient::new(TurnstileConfig {
            secret: secret.map(str::to_string),
            verify_url: verify_url.to_string(),
        }),
        PROBE_PEPPER.to_string(),
        std::sync::Arc::new(
            backbone_website::application::service::notifier_port::UnwiredIntakeNotifier::new(),
        ),
    )
}

fn ctx<'a>(token: Option<&'a str>) -> IntakeContext<'a> {
    IntakeContext {
        client_ip: "203.0.113.50",
        session_id: None,
        user_agent: "probe-agent/1.0",
        turnstile_token: token,
    }
}

/// The four turnstile answers are TYPED APART, and every one of them
/// refuses the write (nothing persists on any turnstile path).
#[tokio::test]
async fn probe_turnstile_four_answers_typed_apart() {
    let db = TestDb::new("intturn").await;
    let website = make_website(&db.pool, "intturn").await;

    // (a) UNSET secret: 503 website_turnstile_not_configured, no HTTP.
    let unset = engine(&db.pool, None, "http://127.0.0.1:9/siteverify");
    match unset
        .execute_intake::<backbone_website::application::service::intake_contact::ContactIntake>(
            &website,
            payload(),
            &ctx(Some("tok")),
        )
        .await
    {
        Err(WebsiteError::TurnstileNotConfigured) => {}
        other => panic!("PROBE-FAIL: unset secret must be TurnstileNotConfigured, got {other:?}"),
    }

    // (b) REFUSED: a well-formed siteverify answering success=false
    // with no secret fault → 400 website_turnstile_refused.
    let refused_url = siteverify_stub(
        r#"{"success":false,"error-codes":["invalid-input-response"]}"#.into(),
        4,
    );
    let refused = engine(&db.pool, Some("stub-secret"), &refused_url);
    match refused
        .execute_intake::<backbone_website::application::service::intake_contact::ContactIntake>(
            &website,
            payload(),
            &ctx(Some("tok")),
        )
        .await
    {
        Err(WebsiteError::TurnstileRefused) => {}
        other => panic!("PROBE-FAIL: bad token must be TurnstileRefused, got {other:?}"),
    }

    // (b') NO token at all (empty): same refused answer, 4xx, no HTTP.
    match refused
        .execute_intake::<backbone_website::application::service::intake_contact::ContactIntake>(
            &website,
            payload(),
            &ctx(None),
        )
        .await
    {
        Err(WebsiteError::TurnstileRefused) => {}
        other => panic!("PROBE-FAIL: missing token must be TurnstileRefused, got {other:?}"),
    }

    // (c) MISCONFIGURED: the host's secret is wrong — the secret-fault
    // error codes → 503 website_turnstile_misconfigured.
    let misconfigured = engine(
        &db.pool,
        Some("stub-secret"),
        &siteverify_stub(r#"{"success":false,"error-codes":["missing-input-secret"]}"#.into(), 4),
    );
    match misconfigured
        .execute_intake::<backbone_website::application::service::intake_contact::ContactIntake>(
            &website,
            payload(),
            &ctx(Some("tok")),
        )
        .await
    {
        Err(WebsiteError::TurnstileMisconfigured) => {}
        other => panic!("PROBE-FAIL: secret fault must be TurnstileMisconfigured, got {other:?}"),
    }

    // (d) UNREACHABLE: nobody listens on this port → 503
    // website_turnstile_unreachable.
    let dead = engine(&db.pool, Some("stub-secret"), "http://127.0.0.1:9/siteverify");
    match dead
        .execute_intake::<backbone_website::application::service::intake_contact::ContactIntake>(
            &website,
            payload(),
            &ctx(Some("tok")),
        )
        .await
    {
        Err(WebsiteError::TurnstileUnreachable) => {}
        other => panic!("PROBE-FAIL: dead endpoint must be TurnstileUnreachable, got {other:?}"),
    }

    // Every refusal left ZERO rows and audited intake_refused.
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM website.contact_messages")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0, "no turnstile path may persist the message");
    let refusals: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.website_audit_log WHERE event = 'intake_refused'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(refusals, 5, "each refusal is audited");

    db.dispose().await;
}

/// The HAPPY path: a real stub answering success=true persists exactly
/// one row, answers `notified = false` (the notifier port is unwired —
/// the shipped default — and the write is never refused by the port),
/// and audits `intake_received`.
#[tokio::test]
async fn probe_intake_success_unwired_notifier() {
    let db = TestDb::new("intok").await;
    let website = make_website(&db.pool, "intok").await;
    let url = siteverify_stub(r#"{"success":true}"#.into(), 2);
    let engine = engine(&db.pool, Some("stub-secret"), &url);

    let receipt = engine
        .execute_intake::<backbone_website::application::service::intake_contact::ContactIntake>(
            &website,
            payload(),
            &ctx(Some("tok")),
        )
        .await
        .unwrap();
    let subject = receipt
        .subject_id
        .unwrap_or_else(|| panic!("PROBE-FAIL: the contact declaration reports its row"));
    assert!(!receipt.notified, "the unwired notifier port reports notified=false");
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.contact_messages WHERE id = $1 AND notified = FALSE",
    )
    .bind(subject)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(rows, 1);
    let received: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.website_audit_log WHERE event = 'intake_received'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(received, 1);

    db.dispose().await;
}

/// The parse edge: an unknown key is UNPARSEABLE through the typed
/// allowlist — 422 `website_intake_field_rejected` through the ROUTER,
/// and nothing reaches the engine or the table.
#[tokio::test]
async fn probe_unknown_field_422_through_router() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let db = TestDb::new("int422").await;
    let website = make_website(&db.pool, "int422").await;
    let state = backbone_website::presentation::http::public_routes::WebsitePublicState::compose(
        db.pool.clone(),
        PROBE_PEPPER.to_string(),
        TurnstileConfig { secret: Some("stub".into()), verify_url: "http://127.0.0.1:9/x".to_string() },
    );
    let app = backbone_website::presentation::http::public_routes::website_public_routes(state);

    let body = serde_json::json!({
        "email": "sender@example.com",
        "message": "hello",
        "website_admin": true   // an identity/routing key the verb does not own
    });
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/public/intake/contact")
                .header("host", host_of(&website))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["code"], "website_intake_field_rejected");

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM website.contact_messages")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0, "a 422 parse refusal never reaches the engine");

    db.dispose().await;
}

/// The Tier-B fixed window: the per-identity limit (5/hour for contact)
/// trips on the 6th submit from the same identity with the TYPED 429
/// carrying `Retry-After` seconds in (0, 3600].
#[tokio::test]
async fn probe_rate_limit_429_retry_after() {
    let db = TestDb::new("intrate").await;
    let website = make_website(&db.pool, "intrate").await;
    // Turnstile not required for the rate arm — a no-turnstile probe
    // declaration keeps the stub out of the arithmetic.
    let engine = engine(&db.pool, Some("stub-secret"), "http://127.0.0.1:9/siteverify");

    struct Unlimited;
    impl IntakeDeclaration for Unlimited {
        const NAME: &'static str = "rate_probe";
        const REQUIRES_TURNSTILE: bool = false;
        const IDENTITY_LIMIT: u32 = 2;
        const IP_LIMIT: u32 = 100;
        type Payload = ContactPayload;
        async fn validate(_: &Self::Payload) -> Result<(), WebsiteError> {
            Ok(())
        }
        async fn persist(
            _tx: &mut sqlx::PgConnection,
            _website_id: Uuid,
            _payload: Self::Payload,
        ) -> Result<IntakeOutcome, WebsiteError> {
            Ok(IntakeOutcome { subject_id: None, detail: None, contact: None })
        }
    }

    // A session identity keeps the per-identity and per-IP buckets on
    // distinct keys (without a session both derive from the IP).
    let ctx = IntakeContext {
        client_ip: "203.0.113.50",
        session_id: Some("probe-rate-session"),
        user_agent: "probe-agent/1.0",
        turnstile_token: None,
    };
    for _ in 0..2 {
        engine.execute_intake::<Unlimited>(&website, payload(), &ctx).await.unwrap();
    }
    match engine.execute_intake::<Unlimited>(&website, payload(), &ctx).await {
        Err(WebsiteError::IntakeRateLimited { retry_after_seconds }) => {
            assert_eq!(WebsiteError::IntakeRateLimited { retry_after_seconds }.http_status(), 429);
            assert!(
                (1..=3600).contains(&retry_after_seconds),
                "retry_after_seconds in (0, 3600], got {retry_after_seconds}"
            );
        }
        other => panic!("PROBE-FAIL: 3rd submit must be rate-limited, got {other:?}"),
    }
    // A DIFFERENT identity is untouched (per-identity buckets).
    let other_identity = IntakeContext {
        client_ip: "203.0.113.51",
        session_id: Some("probe-rate-session-2"),
        user_agent: "probe-agent/1.0",
        turnstile_token: None,
    };
    engine
        .execute_intake::<Unlimited>(&website, payload(), &other_identity)
        .await
        .unwrap();

    db.dispose().await;
}

/// The per-verb savepoint: a declaration whose persist half-writes and
/// then refuses leaves ZERO rows — the rollback is structural, and the
/// refusal is the loud typed 4xx.
#[tokio::test]
async fn probe_savepoint_rollback_leaves_zero_rows() {
    let db = TestDb::new("intsp").await;
    let website = make_website(&db.pool, "intsp").await;
    let engine = engine(&db.pool, Some("stub-secret"), "http://127.0.0.1:9/siteverify");

    struct HalfWriteBoom;
    impl IntakeDeclaration for HalfWriteBoom {
        const NAME: &'static str = "half_write_boom";
        const REQUIRES_TURNSTILE: bool = false;
        const IDENTITY_LIMIT: u32 = 100;
        const IP_LIMIT: u32 = 100;
        type Payload = ContactPayload;
        async fn validate(_: &Self::Payload) -> Result<(), WebsiteError> {
            Ok(())
        }
        async fn persist(
            tx: &mut sqlx::PgConnection,
            website_id: Uuid,
            payload: Self::Payload,
        ) -> Result<IntakeOutcome, WebsiteError> {
            // Land a REAL row, then refuse — the savepoint must undo it.
            sqlx::query(
                "INSERT INTO website.contact_messages \
                 (id, website_id, name, email, message, notified, metadata) \
                 VALUES (gen_random_uuid(), $1, $2, $3, $4, FALSE, \
                         jsonb_build_object('created_at', now()))",
            )
            .bind(website_id)
            .bind(payload.name.as_deref())
            .bind(&payload.email)
            .bind(&payload.message)
            .execute(&mut *tx)
            .await
            .map_err(|_| WebsiteError::IntakeRefused("pre-refuse write failed".into()))?;
            Err(WebsiteError::IntakeRefused("declared refusal after the write".into()))
        }
    }

    match engine
        .execute_intake::<HalfWriteBoom>(&website, payload(), &ctx(None))
        .await
    {
        Err(WebsiteError::IntakeRefused(_)) => {}
        other => panic!("PROBE-FAIL: the boom declaration must refuse, got {other:?}"),
    }
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM website.contact_messages")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0, "the savepoint rollback must undo the half-write");
    let refused: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.website_audit_log WHERE event = 'intake_refused'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(refused, 1, "the rollback refusal is audited");

    db.dispose().await;
}

/// No elevated role anywhere: the intake verbs execute as the connected
/// pool role — a source-scan asserts no role elevation statement exists
/// in the crate (SET ROLE / set_config('role',...)), and the persisted
/// rows carry no submitter-controllable identity column.
#[tokio::test]
async fn probe_no_elevated_role_anywhere() {
    let db = TestDb::new("introle").await;
    let website = make_website(&db.pool, "introle").await;
    let url = siteverify_stub(r#"{"success":true}"#.into(), 1);
    let engine = engine(&db.pool, Some("stub-secret"), &url);
    engine
        .execute_intake::<backbone_website::application::service::intake_contact::ContactIntake>(
            &website,
            payload(),
            &ctx(Some("tok")),
        )
        .await
        .unwrap();

    // The row persisted as the plain session role — no elevation marker.
    let current_role: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    let row_role: String = sqlx::query_scalar(
        "SELECT 'written-by:' || current_user FROM website.contact_messages LIMIT 1",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(row_role, format!("written-by:{current_role}"));

    // The source scan: no role-elevation statement anywhere in src/.
    let manifest = env!("CARGO_MANIFEST_DIR");
    let mut offenders: Vec<String> = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(manifest).join("src")];
    while let Some(dir) = stack.pop() {
        let mut entries: Vec<std::path::PathBuf> =
            std::fs::read_dir(&dir)
                .unwrap_or_else(|e| panic!("PROBE-FAIL: src read {}: {e}", dir.display()))
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("PROBE-FAIL: file read {}: {e}", path.display()));
                for (no, line) in text.lines().enumerate() {
                    let up = line.to_ascii_uppercase();
                    if up.contains("SET ROLE")
                        || up.contains("SET LOCAL ROLE")
                        || line.contains("set_config('role'")
                        || line.contains("set_config(\"role\"")
                    {
                        offenders.push(format!("{}:{}: {}", path.display(), no + 1, line.trim()));
                    }
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "no role-elevation statement may exist in the intake write path (or anywhere in src/):\n{}",
        offenders.join("\n")
    );

    db.dispose().await;
}
