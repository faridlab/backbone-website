//! Visitor GDPR probes (WS-10 shape): the keyed digest, the
//! one-statement heartbeat upsert, the never-persisted raw IP, the
//! login merge's two arms, the GC sweep, and concurrent-merge safety.
//!
//! The pepper here is the harness constant `PROBE_PEPPER` — never an
//! environment knob — so every probe runs under a known non-empty key.

use std::sync::Arc;

use uuid::Uuid;

use backbone_website::application::service::visitor_gc::{
    sweep_partnerless_visitors, SweepSummary,
};
use backbone_website::application::service::visitor_service::{
    random_token, visitor_digest, SessionFacts, VisitorEngine,
};
use backbone_website::application::service::website_error::WebsiteError;
use backbone_website::application::service::website_service::ActorRef;

use super::common::{make_website, TestDb, PROBE_PEPPER};

fn officer() -> ActorRef {
    ActorRef::officer(Uuid::new_v4())
}

fn engine(pool: &sqlx::PgPool) -> VisitorEngine {
    VisitorEngine::new(pool.clone(), PROBE_PEPPER.to_string())
}

fn facts<'a>(ip: &'a str, session: &'a str) -> SessionFacts<'a> {
    SessionFacts { ip, user_agent: "probe-agent/1.0", session_id: session, country_code: Some("ID") }
}

/// Heartbeat upsert: two beats → ONE row; the second beat within the
/// visit window does NOT open a new visit; the beat carries a track
/// row once and the dedup window refuses an immediate re-track.
#[tokio::test]
async fn probe_heartbeat_upsert_one_row_and_windows() {
    let db = TestDb::new("visbeat").await;
    let website = make_website(&db.pool, "visbeat").await;
    let visitors = engine(&db.pool);

    let first = visitors
        .heartbeat(website.id, &facts("203.0.113.9", "sess-1"), Some("/"), None)
        .await
        .unwrap();
    assert!(first.inserted, "the first beat mints the visitor row");
    assert!(first.tracked);
    assert_eq!(first.kind, "anonymous");
    assert_eq!(first.access_token.len(), 43, "32-byte unpadded base64url token");

    // Same session facts immediately again: SAME row, no new visit, and
    // the track dedup window refuses a second track of the same url.
    let second = visitors
        .heartbeat(website.id, &facts("203.0.113.9", "sess-1"), Some("/"), None)
        .await
        .unwrap();
    assert!(!second.inserted, "the second beat must NOT mint a second row");
    assert_eq!(second.visitor_id, first.visitor_id);
    assert!(!second.tracked, "the 30-minute dedup window refuses the re-track");
    // A different url tracks.
    let third = visitors
        .heartbeat(website.id, &facts("203.0.113.9", "sess-1"), Some("/pricing"), None)
        .await
        .unwrap();
    assert!(third.tracked);

    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.visitors WHERE website_id = $1",
    )
    .bind(website.id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(rows, 1, "two beats → exactly one visitor row");

    let (visit_count, tracks): (i32, i64) = sqlx::query_as(
        "SELECT visit_count, (SELECT count(*) FROM website.visitor_tracks t \
         WHERE t.visitor_id = website.visitors.id) FROM website.visitors WHERE id = $1",
    )
    .bind(first.visitor_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    // Spec §1: visit_count DEFAULT 0 — a visit is COUNTED when a later
    // beat finds the previous connection older than the 8h window, not
    // when the row first opens.
    assert_eq!(visit_count, 0, "beats inside the 8h window bump nothing");
    assert_eq!(tracks, 2, "one track per distinct url inside the dedup window");

    // The visit window DOES count when the last connection aged out:
    // age the row past 8h, beat again.
    sqlx::query("UPDATE website.visitors SET last_connection_at = now() - interval '9 hours' WHERE id = $1")
        .bind(first.visitor_id)
        .execute(&db.pool)
        .await
        .unwrap();
    let aged = visitors
        .heartbeat(website.id, &facts("203.0.113.9", "sess-1"), None, None)
        .await
        .unwrap();
    assert!(!aged.inserted);
    assert!(!aged.tracked, "a beat without a url writes no track row");
    let visit_count: i32 = sqlx::query_scalar(
        "SELECT visit_count FROM website.visitors WHERE id = $1",
    )
    .bind(first.visitor_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(visit_count, 1, "a beat past the 8h window bumps the count");

    db.dispose().await;
}

/// The raw IP is NOWHERE: every text column of every row in the
/// visitors + tracks tables is fetched WHOLE and asserted not to
/// contain the raw IP string — the digest stands in for identity.
#[tokio::test]
async fn probe_raw_ip_never_persisted_full_row() {
    let db = TestDb::new("visip").await;
    let website = make_website(&db.pool, "visip").await;
    let visitors = engine(&db.pool);

    const RAW_IP: &str = "198.51.100.77";
    let out = visitors
        .heartbeat(website.id, &facts(RAW_IP, "sess-ip"), Some("/contact"), None)
        .await
        .unwrap();
    let _ = out;

    // Full-row assertion over BOTH tables: stringify every value of
    // every column (casting to text server-side) and grep for the IP.
    for table in ["website.visitors", "website.visitor_tracks"] {
        let row = sqlx::query_scalar::<_, String>(&format!(
            "SELECT string_agg(v::text, ' | ') FROM (SELECT ({t}.*)::text AS v FROM {t}) s",
            t = table
        ))
        .fetch_one(&db.pool)
        .await
        .unwrap();
        assert!(
            !row.contains(RAW_IP),
            "the raw IP must never appear in any column of {table}: {row}"
        );
    }

    // The digest of these facts is present (identity IS stored — keyed).
    let digest = visitor_digest(PROBE_PEPPER, RAW_IP, "probe-agent/1.0", "sess-ip").unwrap();
    let stored: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.visitors WHERE digest = $1",
    )
    .bind(&digest)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(stored, 1, "the keyed digest stands in for the IP");

    // Pepper rotation deterministically changes the digest (unit-held;
    // re-asserted here as the probe arm).
    let rotated = visitor_digest("rotated-pepper", RAW_IP, "probe-agent/1.0", "sess-ip").unwrap();
    assert_ne!(rotated, digest);

    // Tokens: random per mint, url-safe alphabet, no relation to any
    // identity input.
    let a = random_token();
    let b = random_token();
    assert_ne!(a, b);
    assert_eq!(a.len(), 43);
    assert!(a.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'));

    db.dispose().await;
}

/// An UNSET pepper is a typed loud refusal — the first visitor verb
/// never digests under an empty key, and nothing lands.
#[tokio::test]
async fn probe_unset_pepper_typed_refusal() {
    let db = TestDb::new("vispepper").await;
    let website = make_website(&db.pool, "vispepper").await;
    let unpeppered = VisitorEngine::new(db.pool.clone(), String::new());

    match unpeppered
        .heartbeat(website.id, &facts("192.0.2.5", "sess-nope"), Some("/"), None)
        .await
    {
        Err(WebsiteError::VisitorPepperNotConfigured) => {}
        other => panic!("PROBE-FAIL: expected VisitorPepperNotConfigured, got {other:?}"),
    }
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM website.visitors")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(rows, 0, "a refused heartbeat writes nothing");

    // The digest function itself gates the same way, with the typed code.
    match visitor_digest("", "192.0.2.5", "UA", "s") {
        Err(WebsiteError::VisitorPepperNotConfigured) => {}
        other => panic!("PROBE-FAIL: digest must gate on the empty pepper, got {other:?}"),
    }

    db.dispose().await;
}

/// The login merge — BOTH arms:
/// claim-in-place (no identified row yet: kind flips, principal lands,
/// the RANDOM token is kept) and reparent (identified row exists: the
/// anonymous tracks move, the anonymous row dies).
#[tokio::test]
async fn probe_login_merge_both_arms() {
    let db = TestDb::new("vismerge").await;
    let website = make_website(&db.pool, "vismerge").await;
    let visitors = engine(&db.pool);
    let principal = Uuid::new_v4();

    // Arm 1: claim-in-place.
    let anon1 = visitors
        .heartbeat(website.id, &facts("192.0.2.10", "sess-a1"), Some("/a"), None)
        .await
        .unwrap();
    let token_before: String = sqlx::query_scalar(
        "SELECT access_token FROM website.visitors WHERE id = $1",
    )
    .bind(anon1.visitor_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    let claimed = visitors
        .merge_visitor(officer(), website.id, anon1.visitor_id, principal)
        .await
        .unwrap();
    assert_eq!(claimed.id, anon1.visitor_id, "claim keeps the row in place");
    assert_eq!(claimed.kind, "identified");
    assert_eq!(claimed.portal_user_id, Some(principal));
    let token_after: String = sqlx::query_scalar(
        "SELECT access_token FROM website.visitors WHERE id = $1",
    )
    .bind(anon1.visitor_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(token_before, token_after, "claim never upgrades the token");

    // Arm 2: reparent — a NEW anonymous row on the same website merges
    // INTO the claimed one: tracks move, the anonymous row dies.
    let anon2 = visitors
        .heartbeat(website.id, &facts("192.0.2.11", "sess-a2"), Some("/b"), None)
        .await
        .unwrap();
    assert_ne!(anon2.visitor_id, claimed.id);
    let reparented = visitors
        .merge_visitor(officer(), website.id, anon2.visitor_id, principal)
        .await
        .unwrap();
    assert_eq!(reparented.id, claimed.id, "reparent survives into the identified row");
    let dead: i64 = sqlx::query_scalar("SELECT count(*) FROM website.visitors WHERE id = $1")
        .bind(anon2.visitor_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(dead, 0, "the absorbed anonymous row is deleted");
    let moved: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.visitor_tracks WHERE visitor_id = $1 AND url = '/b'",
    )
    .bind(claimed.id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(moved, 1, "the absorbed row's tracks re-parented");
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.website_audit_log WHERE event = 'visitor_merged'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(audit, 2, "each merge arm is audited once");

    db.dispose().await;
}

/// Concurrent merges of the SAME anonymous row: SKIP LOCKED keeps the
/// contenders moving instead of deadlocking — a contender that cannot
/// take the row observes the typed NotFound, never a serialization
/// failure or a panic. The final row count is stable at one.
#[tokio::test]
async fn probe_concurrent_merge_skip_locked() {
    let db = TestDb::new("visconc").await;
    let website = make_website(&db.pool, "visconc").await;
    let visitors = Arc::new(engine(&db.pool));
    let anon = visitors
        .heartbeat(website.id, &facts("192.0.2.20", "sess-c"), Some("/c"), None)
        .await
        .unwrap();

    let target = website.id;
    let id = anon.visitor_id;
    let mut handles = Vec::new();
    for _ in 0..6 {
        let visitors = visitors.clone();
        handles.push(tokio::spawn(async move {
            visitors.merge_visitor(officer(), target, id, Uuid::new_v4()).await
        }));
    }
    let mut claimers = 0usize;
    let mut not_founds = 0usize;
    for h in handles {
        match h.await.unwrap_or_else(|e| panic!("PROBE-FAIL: probe join: {e}")) {
            Ok(view) => {
                assert_eq!(view.id, id, "a successful claim keeps the row in place");
                claimers += 1;
            }
            // A contender that could not take the row under SKIP LOCKED
            // observes the typed 404 — the accepted miss, never a crash.
            Err(WebsiteError::NotFound(_)) => not_founds += 1,
            Err(e) => panic!(
                "PROBE-FAIL: a concurrent merge failed with something other than the typed miss: {e:?}"
            ),
        }
    }
    assert!(claimers >= 1, "at least one contender must claim (got {claimers})");
    assert_eq!(
        claimers + not_founds,
        6,
        "every contender either claims or takes the typed miss"
    );
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.visitors WHERE website_id = $1",
    )
    .bind(website.id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(rows, 1, "concurrent merges never duplicate the visitor row");
    let tracks: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.visitor_tracks WHERE visitor_id = $1",
    )
    .bind(id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(tracks, 1, "the track row neither duplicates nor vanishes");

    db.dispose().await;
}

/// The GC sweep: partnerless rows older than the retention horizon go
/// (batched); identified rows and fresh rows stay; the summary reports
/// batches.
#[tokio::test]
async fn probe_gc_sweep_batches_and_horizon() {
    let db = TestDb::new("visgc").await;
    let website = make_website(&db.pool, "visgc").await;
    let visitors = engine(&db.pool);

    // One fresh anonymous row (stays), one aged anonymous row (goes),
    // one aged IDENTIFIED row (stays).
    let fresh = visitors
        .heartbeat(website.id, &facts("192.0.2.30", "sess-f"), Some("/f"), None)
        .await
        .unwrap();
    let old = visitors
        .heartbeat(website.id, &facts("192.0.2.31", "sess-o"), Some("/o"), None)
        .await
        .unwrap();
    let old_identified = visitors
        .heartbeat(website.id, &facts("192.0.2.32", "sess-i"), Some("/i"), None)
        .await
        .unwrap();
    visitors
        .merge_visitor(officer(), website.id, old_identified.visitor_id, Uuid::new_v4())
        .await
        .unwrap();
    sqlx::query("UPDATE website.visitors SET last_connection_at = now() - interval '90 days' WHERE id = ANY($1)")
        .bind(vec![old.visitor_id, old_identified.visitor_id])
        .execute(&db.pool)
        .await
        .unwrap();

    // A small batch forces multiple batches for the same work.
    let summary: SweepSummary =
        sweep_partnerless_visitors(&db.pool, 60, 1).await.unwrap();
    assert_eq!(summary.swept, 1, "exactly the aged partnerless row is swept");
    assert!(summary.batches >= 1);

    let remaining: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM website.visitors WHERE website_id = $1",
    )
    .bind(website.id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(remaining, 2, "fresh + identified rows survive the sweep");
    let fresh_alive: i64 =
        sqlx::query_scalar("SELECT count(*) FROM website.visitors WHERE id = $1")
            .bind(fresh.visitor_id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(fresh_alive, 1);

    db.dispose().await;
}
