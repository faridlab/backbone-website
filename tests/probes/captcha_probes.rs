//! Captcha provider-arm probes (the recaptcha sibling + selection,
//! docs/spec.md §6.3): the four typed answers through the reCAPTCHA
//! arm, the selector's pure parse + its fail-closed unknown arm (never
//! a silent provider fallback), the router compose seam over the
//! selected verifier, and the secret/token never surfacing in any
//! audit row or error text.
//!
//! The siteverify endpoint is a REAL local HTTP stub (the same harness
//! the turnstile intake probes use) — the verify call is exercised as
//! HTTP, not mocked at the client seam. Google's answer body differs
//! from Cloudflare's only in its error-code dialect; the verdict typing
//! is shared with the turnstile arm (the frozen `website_turnstile_*`
//! codes — the prefix is historical and now means "the captcha
//! verifier").

use backbone_website::application::service::captcha_recaptcha::{
    CaptchaProvider, CaptchaVerifier, RecaptchaClient, RecaptchaConfig,
};
use backbone_website::application::service::intake_contact::ContactPayload;
use backbone_website::application::service::intake_engine::IntakeEngine;
use backbone_website::application::service::website_error::WebsiteError;

use super::common::{host_of, make_website, siteverify_stub, TestDb, PROBE_PEPPER};

fn payload() -> ContactPayload {
    ContactPayload {
        name: Some("Probe Sender".into()),
        email: "sender@example.com".into(),
        message: "a probe message".into(),
    }
}

fn notifier() -> std::sync::Arc<dyn backbone_website::application::service::notifier_port::IntakeNotifier>
{
    std::sync::Arc::new(
        backbone_website::application::service::notifier_port::UnwiredIntakeNotifier::new(),
    )
}

/// One engine over the RECAPTCHA arm with explicit knobs (never the
/// process environment — probes must not depend on host configuration).
fn recaptcha_engine(pool: &sqlx::PgPool, secret: Option<&str>, verify_url: &str) -> IntakeEngine {
    IntakeEngine::with_verifier(
        pool.clone(),
        CaptchaVerifier::Recaptcha(RecaptchaClient::new(RecaptchaConfig {
            secret: secret.map(str::to_string),
            verify_url: verify_url.to_string(),
        })),
        PROBE_PEPPER.to_string(),
        notifier(),
    )
}

fn unknown_engine(pool: &sqlx::PgPool) -> IntakeEngine {
    IntakeEngine::with_verifier(
        pool.clone(),
        CaptchaVerifier::UnknownProvider,
        PROBE_PEPPER.to_string(),
        notifier(),
    )
}

fn ctx<'a>(token: Option<&'a str>) -> backbone_website::application::service::intake_engine::IntakeContext<'a> {
    backbone_website::application::service::intake_engine::IntakeContext {
        client_ip: "203.0.113.60",
        session_id: None,
        user_agent: "probe-agent/1.0",
        turnstile_token: token,
    }
}

/// The four typed answers through the reCAPTCHA arm — unset secret,
/// bad token, Google's two secret-fault codes, transport failure —
/// plus the pass. Every refusal leaves zero rows and is audited; the
/// codes are the frozen turnstile family's (the shared verdict typing).
#[tokio::test]
async fn probe_recaptcha_four_answers_typed_apart() {
    use backbone_website::application::service::intake_contact::ContactIntake;
    let db = TestDb::new("rcptr").await;
    let website = make_website(&db.pool, "rcptr").await;

    // (a) UNSET secret: 503 website_turnstile_not_configured, no HTTP.
    let unset = recaptcha_engine(&db.pool, None, "http://127.0.0.1:9/siteverify");
    match unset
        .execute_intake::<ContactIntake>(&website, payload(), &ctx(Some("tok")))
        .await
    {
        Err(WebsiteError::TurnstileNotConfigured) => {}
        other => panic!("PROBE-FAIL: unset recaptcha secret must be TurnstileNotConfigured, got {other:?}"),
    }

    // (b) REFUSED: Google answers success=false with a token-fault code
    // (or no code at all) → 400 website_turnstile_refused.
    let refused_url =
        siteverify_stub(r#"{"success":false,"error-codes":["invalid-input-response"]}"#.into(), 2);
    let refused = recaptcha_engine(&db.pool, Some("stub-secret"), &refused_url);
    match refused
        .execute_intake::<ContactIntake>(&website, payload(), &ctx(Some("tok")))
        .await
    {
        Err(WebsiteError::TurnstileRefused) => {}
        other => panic!("PROBE-FAIL: bad recaptcha token must be TurnstileRefused, got {other:?}"),
    }
    // (b') NO token at all (empty): same refused answer, no HTTP.
    match refused
        .execute_intake::<ContactIntake>(&website, payload(), &ctx(None))
        .await
    {
        Err(WebsiteError::TurnstileRefused) => {}
        other => panic!("PROBE-FAIL: missing recaptcha token must be TurnstileRefused, got {other:?}"),
    }
    // (b'') no error codes at all: still refused, never a passthrough.
    let bare = siteverify_stub(r#"{"success":false}"#.into(), 1);
    match recaptcha_engine(&db.pool, Some("stub-secret"), &bare)
        .execute_intake::<ContactIntake>(&website, payload(), &ctx(Some("tok")))
        .await
    {
        Err(WebsiteError::TurnstileRefused) => {}
        other => panic!("PROBE-FAIL: bare failure must be TurnstileRefused, got {other:?}"),
    }

    // (c) MISCONFIGURED: Google's secret-fault pair — BOTH codes → 503
    // website_turnstile_misconfigured (distinguishable from a bad token).
    for code in ["missing-input-secret", "invalid-input-secret"] {
        let body = format!(r#"{{"success":false,"error-codes":["{code}"]}}"#);
        let url = siteverify_stub(body, 1);
        match recaptcha_engine(&db.pool, Some("stub-secret"), &url)
            .execute_intake::<ContactIntake>(&website, payload(), &ctx(Some("tok")))
            .await
        {
            Err(WebsiteError::TurnstileMisconfigured) => {}
            other => panic!("PROBE-FAIL: secret fault {code} must be TurnstileMisconfigured, got {other:?}"),
        }
    }

    // (d) UNREACHABLE: nobody listens → 503 website_turnstile_unreachable.
    match recaptcha_engine(&db.pool, Some("stub-secret"), "http://127.0.0.1:9/siteverify")
        .execute_intake::<ContactIntake>(&website, payload(), &ctx(Some("tok")))
        .await
    {
        Err(WebsiteError::TurnstileUnreachable) => {}
        other => panic!("PROBE-FAIL: dead recaptcha endpoint must be TurnstileUnreachable, got {other:?}"),
    }

    // Every refusal left ZERO rows; each was audited (a, b, b', b'',
    // c x2, d = 7 refusals).
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM website.contact_messages")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0, "no recaptcha refusal path may persist the message");
    let refusals: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.website_audit_log WHERE event = 'intake_refused'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(refusals, 7, "each recaptcha refusal is audited");

    // (e) PASS: a stub answering success=true → exactly one row.
    let pass_url = siteverify_stub(r#"{"success":true}"#.into(), 1);
    let receipt = recaptcha_engine(&db.pool, Some("stub-secret"), &pass_url)
        .execute_intake::<ContactIntake>(&website, payload(), &ctx(Some("tok")))
        .await
        .unwrap();
    assert!(receipt.subject_id.is_some(), "the contact declaration reports its row");
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM website.contact_messages")
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

/// The selector: pure parse rows, the selected-verifier construction,
/// and the fail-closed unknown arm — a selector naming no known
/// provider REFUSES every gated verb with its own typed 503, never a
/// silent fallback to the default provider.
#[tokio::test]
async fn probe_captcha_provider_selection() {
    use backbone_website::application::service::intake_contact::ContactIntake;

    // The pure parse: unset/empty and `turnstile` (any case, trimmed)
    // keep the shipped default; `recaptcha` selects the sibling;
    // anything else names no known provider.
    let rows = [
        ("", Some(CaptchaProvider::Turnstile)),
        ("turnstile", Some(CaptchaProvider::Turnstile)),
        ("  TURNSTILE  ", Some(CaptchaProvider::Turnstile)),
        ("recaptcha", Some(CaptchaProvider::Recaptcha)),
        ("ReCaptcha", Some(CaptchaProvider::Recaptcha)),
        ("recaptche", None),
        ("1", None),
        ("cloudflare", None),
        ("true", None),
    ];
    for (raw, want) in rows {
        assert_eq!(
            CaptchaProvider::parse(raw),
            want,
            "parse({raw:?}) must be {want:?}"
        );
    }

    // The selected-verifier construction over explicit configs.
    let turnstile_cfg = backbone_website::application::service::intake_engine::TurnstileConfig {
        secret: Some("t".into()),
        verify_url: "http://127.0.0.1:9/turnstile".into(),
    };
    let recaptcha_cfg = RecaptchaConfig {
        secret: Some("r".into()),
        verify_url: "http://127.0.0.1:9/recaptcha".into(),
    };
    assert_eq!(
        CaptchaVerifier::selected("", turnstile_cfg.clone(), recaptcha_cfg.clone()).provider(),
        Some(CaptchaProvider::Turnstile),
        "unset selector keeps the turnstile default"
    );
    assert_eq!(
        CaptchaVerifier::selected("recaptcha", turnstile_cfg.clone(), recaptcha_cfg.clone())
            .provider(),
        Some(CaptchaProvider::Recaptcha)
    );
    assert_eq!(
        CaptchaVerifier::selected("typo", turnstile_cfg, recaptcha_cfg).provider(),
        None,
        "an unknown selector yields the unknown arm"
    );

    // The unknown arm through the engine: the gated verb REFUSES with
    // the typed 503 website_captcha_provider_unknown — the selector
    // never silently falls back to a default provider.
    let db = TestDb::new("rcsel").await;
    let website = make_website(&db.pool, "rcsel").await;
    let engine = unknown_engine(&db.pool);
    match engine
        .execute_intake::<ContactIntake>(&website, payload(), &ctx(Some("tok")))
        .await
    {
        Err(e @ WebsiteError::CaptchaProviderUnknown) => {
            assert_eq!(e.http_status(), 503);
            assert_eq!(e.code(), "website_captcha_provider_unknown");
        }
        other => panic!("PROBE-FAIL: unknown selector must refuse with CaptchaProviderUnknown, got {other:?}"),
    }
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM website.contact_messages")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0, "the unknown arm persists nothing");
    let refusals: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.website_audit_log WHERE event = 'intake_refused' \
         AND detail->>'code' = 'website_captcha_provider_unknown'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(refusals, 1, "the unknown-arm refusal is audited with its code");

    db.dispose().await;
}

/// The router compose seam: `compose_with_verifier` wires the selected
/// verifier end to end — the recaptcha arm passes a good token (201),
/// refuses a bad one with the typed 400 code, and the unknown arm
/// answers its typed 503.
#[tokio::test]
async fn probe_recaptcha_router_compose_seam() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use backbone_website::presentation::http::public_routes::{website_public_routes, WebsitePublicState};

    let db = TestDb::new("rcrt").await;
    let website = make_website(&db.pool, "rcrt").await;

    let state_for = |verifier: CaptchaVerifier| {
        website_public_routes(WebsitePublicState::compose_with_verifier(
            db.pool.clone(),
            PROBE_PEPPER.to_string(),
            verifier,
            false,
        ))
    };
    let recaptcha_with = |secret: Option<&str>, url: &str| {
        CaptchaVerifier::Recaptcha(RecaptchaClient::new(RecaptchaConfig {
            secret: secret.map(str::to_string),
            verify_url: url.to_string(),
        }))
    };
    let body = serde_json::json!({
        "email": "sender@example.com",
        "message": "hello",
    });
    let post = |app: axum::Router, token: &str| {
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri("/public/intake/contact")
                .header("host", host_of(&website))
                .header("x-turnstile-token", token)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
    };

    // PASS: the recaptcha arm answers 201 through the compose seam.
    let pass_url = siteverify_stub(r#"{"success":true}"#.into(), 1);
    let response = post(
        state_for(recaptcha_with(Some("stub-secret"), &pass_url)),
        "tok",
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);

    // REFUSED: the typed 400 + the frozen code on the wire.
    let refused_url =
        siteverify_stub(r#"{"success":false,"error-codes":["timeout-or-duplicate"]}"#.into(), 1);
    let response = post(
        state_for(recaptcha_with(Some("stub-secret"), &refused_url)),
        "tok",
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["code"], "website_turnstile_refused");

    // UNKNOWN PROVIDER: the typed 503 + its code on the wire.
    let response = post(state_for(CaptchaVerifier::UnknownProvider), "tok")
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["code"], "website_captcha_provider_unknown");

    // Exactly the one passing row landed.
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM website.contact_messages")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);

    db.dispose().await;
}

/// Secret/token hygiene: with a distinctive secret configured and a
/// distinctive token presented, every refusal's error text and EVERY
/// audit row (whole-row JSON, case-insensitive) stays free of both
/// values.
#[tokio::test]
async fn probe_recaptcha_secret_and_token_never_surface() {
    use backbone_website::application::service::intake_contact::ContactIntake;

    const SECRET_NEEDLE: &str = "leakcheck-recaptcha-secret-a1b2c3";
    const TOKEN_NEEDLE: &str = "leakcheck-recaptcha-token-d4e5f6";

    let db = TestDb::new("rclkg").await;
    let website = make_website(&db.pool, "rclkg").await;

    // Every refusal path with both needles armed: bad token, both
    // secret-fault codes, unreachable, unset secret, unknown provider.
    let cases: [(&'static str, Option<&str>, String); 4] = [
        (
            "refused",
            Some(SECRET_NEEDLE),
            siteverify_stub(r#"{"success":false,"error-codes":["invalid-input-response"]}"#.into(), 1),
        ),
        (
            "misconfigured",
            Some(SECRET_NEEDLE),
            siteverify_stub(r#"{"success":false,"error-codes":["invalid-input-secret"]}"#.into(), 1),
        ),
        ("unreachable", Some(SECRET_NEEDLE), "http://127.0.0.1:9/siteverify".to_string()),
        ("not-configured", None, "http://127.0.0.1:9/siteverify".to_string()),
    ];
    let mut verdict_texts: Vec<String> = Vec::new();
    for (label, secret, url) in cases {
        let engine = recaptcha_engine(&db.pool, secret, &url);
        match engine
            .execute_intake::<ContactIntake>(&website, payload(), &ctx(Some(TOKEN_NEEDLE)))
            .await
        {
            Err(e) => verdict_texts.push(format!("{label}: {e}")),
            Ok(_) => panic!("PROBE-FAIL: {label} must refuse"),
        }
    }
    let engine = unknown_engine(&db.pool);
    match engine
        .execute_intake::<ContactIntake>(&website, payload(), &ctx(Some(TOKEN_NEEDLE)))
        .await
    {
        Err(e) => verdict_texts.push(format!("unknown-provider: {e}")),
        Ok(_) => panic!("PROBE-FAIL: unknown provider must refuse"),
    }

    // No error Display text carries either needle.
    for text in &verdict_texts {
        assert!(
            !text.to_ascii_lowercase().contains(&SECRET_NEEDLE.to_ascii_lowercase()),
            "error text leaks the secret: {text}"
        );
        assert!(
            !text.to_ascii_lowercase().contains(&TOKEN_NEEDLE.to_ascii_lowercase()),
            "error text leaks the token: {text}"
        );
    }

    // No audit row (whole row as JSON, case-insensitive) carries
    // either needle — the trail records the verb + code only.
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT lower(to_jsonb(t)::text) FROM website.website_audit_log t",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert!(rows.len() >= 5, "the refusal paths audited ({} rows)", rows.len());
    let secret_lc = SECRET_NEEDLE.to_ascii_lowercase();
    let token_lc = TOKEN_NEEDLE.to_ascii_lowercase();
    for (i, row) in rows.iter().enumerate() {
        assert!(!row.contains(&secret_lc), "audit row {i} leaks the secret");
        assert!(!row.contains(&token_lc), "audit row {i} leaks the token");
    }

    // And nothing persisted.
    let persisted: i64 = sqlx::query_scalar("SELECT count(*) FROM website.contact_messages")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(persisted, 0);

    db.dispose().await;
}
