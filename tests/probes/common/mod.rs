//! Shared harness: one DISPOSABLE scratch database per probe,
//! FAIL-HARD.
//!
//! The suite never runs against a shared database (and NEVER against
//! the live dev database on 5432): each probe mints
//! `website_seat_<marker>_<hex>` on the local scratch Postgres
//! (127.0.0.1:5433 — the pinned scratch container), applies this
//! module's migrations with a raw SQL file runner, installs the
//! minimal portal stub the ghost-principal mint writes to, runs, and
//! drops the database.
//!
//! FAIL-HARD CONTRACT: a probe that cannot reach its scratch database
//! PANICS — [`TestDb::new`] refuses to return `None`, and [`skipped`]
//! panics on principle. A green suite means the behaviors were
//! exercised, not that they were unreachable.
//!
//! The portal stub: this module's DDL has NO cross-schema foreign key
//! into portal (principal references are logical, keyed on the
//! portal-user row id), and the one cross-schema WRITE is the
//! bootstrap verb's ghost-principal mint
//! (`INSERT INTO portal.portal_users (...)`). The real portal DDL is
//! upstream's contract; the probes materialize only the five columns
//! that INSERT touches, so the probes test THIS module without
//! compiling portal's migrations into the scratch database.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use uuid::Uuid;

/// The scratch Postgres every probe database is born on and dropped
/// from. 127.0.0.1:5433 — the pinned scratch container, NEVER a live
/// service database.
pub const SCRATCH_ADMIN_URL: &str = "postgres://postgres:postgres@127.0.0.1:5433/postgres";

/// The probe pepper (explicit, never from the environment — probes
/// must not depend on host configuration).
pub const PROBE_PEPPER: &str = "website-probe-pepper";

fn admin_url() -> String {
    std::env::var("WEBSITE_TEST_ADMIN_URL").unwrap_or_else(|_| SCRATCH_ADMIN_URL.into())
}

/// The fail-hard skip: reaching this is a FAILURE, never a green tick.
pub fn skipped(reason: &str) -> ! {
    panic!("VACUOUS SKIP IS A FAILURE: {reason}");
}

/// One disposable scratch database, migrations + portal stub applied.
/// Panics (never returns `None`) when the scratch Postgres is
/// unreachable.
pub struct TestDb {
    pub pool: PgPool,
    name: String,
    admin: PgPool,
}

impl TestDb {
    pub async fn new(marker: &str) -> Self {
        let url = admin_url();
        let admin = match PgPoolOptions::new()
            .max_connections(4)
            .acquire_timeout(Duration::from_secs(5))
            .connect(&url)
            .await
        {
            Ok(a) => a,
            Err(e) => {
                eprintln!("PROBE-FAIL: {marker}: admin connect to {url} failed: {e}");
                skipped(&format!("scratch Postgres unreachable: {e}"));
            }
        };
        let suffix: String = Uuid::new_v4().simple().to_string().chars().take(8).collect();
        let name = format!("website_seat_{marker}_{suffix}");
        // Disposable by construction: a stale DB of the same name goes first.
        if let Err(e) = sqlx::query(&format!(r#"DROP DATABASE IF EXISTS "{name}" WITH (FORCE)"#))
            .execute(&admin)
            .await
        {
            eprintln!("PROBE-FAIL: {marker}: pre-drop of {name} failed: {e}");
            skipped(&format!("scratch pre-drop failed: {e}"));
        }
        if let Err(e) = sqlx::query(&format!(r#"CREATE DATABASE "{name}""#))
            .execute(&admin)
            .await
        {
            eprintln!("PROBE-FAIL: {marker}: create database {name} failed: {e}");
            skipped(&format!("scratch create failed: {e}"));
        }
        // Splice ONLY the trailing path segment.
        let db_url = match url.rfind('/') {
            Some(i) => format!("{}{}", &url[..=i], name),
            None => url.clone(),
        };
        let pool = match PgPoolOptions::new()
            .max_connections(8)
            .acquire_timeout(Duration::from_secs(10))
            .connect(&db_url)
            .await
        {
            Ok(p) => p,
            Err(e) => {
                eprintln!("PROBE-FAIL: {marker}: connect to {db_url} failed: {e}");
                skipped(&format!("scratch connect failed: {e}"));
            }
        };
        if let Err(what) = apply_module_migrations(&pool, marker).await {
            skipped(&what);
        }
        if let Err(what) = install_portal_stub(&pool, marker).await {
            skipped(&what);
        }
        Self { pool, name, admin }
    }

    /// Explicit teardown: drop the scratch database entirely.
    pub async fn dispose(self) {
        self.drop_db().await;
    }

    async fn drop_db(&self) {
        // FORCE: the connected probe pool may still hold an idle session.
        let _ = sqlx::query(&format!(r#"DROP DATABASE IF EXISTS "{}" WITH (FORCE)"#, self.name))
            .execute(&self.admin)
            .await;
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        let name = self.name.clone();
        let url = admin_url();
        // Leak-guard teardown for panicking probes; dispose() is the
        // happy path.
        std::thread::spawn(move || {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                rt.block_on(async move {
                    if let Ok(admin) = sqlx::PgPool::connect(&url).await {
                        let _ = sqlx::query(&format!(
                            r#"DROP DATABASE IF EXISTS "{name}" WITH (FORCE)"#
                        ))
                        .execute(&admin)
                        .await;
                    }
                });
            }
        });
    }
}

/// Apply this module's migrations with a raw SQL file runner (sorted
/// `.up.sql` order — the module's files are self-contained).
async fn apply_module_migrations(pool: &PgPool, marker: &str) -> Result<(), String> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let dir = format!("{manifest}/migrations");
    let mut files: Vec<std::path::PathBuf> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.ends_with(".up.sql"))
                    .unwrap_or(false)
            })
            .collect(),
        Err(e) => return Err(format!("PROBE-FAIL: {marker}: cannot read {dir}: {e}")),
    };
    files.sort();
    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| format!("PROBE-FAIL: {marker}: cannot acquire pool conn: {e}"))?;
    for file in files {
        let sql = std::fs::read_to_string(&file)
            .map_err(|e| format!("PROBE-FAIL: {marker}: cannot read {}: {e}", file.display()))?;
        if let Err(e) = sqlx::raw_sql(&sql).execute(&mut *conn).await {
            return Err(format!(
                "PROBE-FAIL: {marker}: migration {} failed: {e}",
                file.display()
            ));
        }
    }
    Ok(())
}

/// The minimal portal.principal stub (see the module doc): the five
/// columns the ghost-principal mint writes.
async fn install_portal_stub(pool: &PgPool, marker: &str) -> Result<(), String> {
    let mut conn = pool
        .acquire()
        .await
        .map_err(|e| format!("PROBE-FAIL: {marker}: cannot acquire pool conn: {e}"))?;
    let stub = r#"
        CREATE SCHEMA IF NOT EXISTS portal;
        CREATE TABLE IF NOT EXISTS portal.portal_users (
            id UUID PRIMARY KEY,
            email TEXT,
            display_name TEXT,
            status TEXT NOT NULL DEFAULT 'invited',
            metadata JSONB NOT NULL DEFAULT '{}'::jsonb
        );
    "#;
    if let Err(e) = sqlx::raw_sql(stub).execute(&mut *conn).await {
        return Err(format!("PROBE-FAIL: {marker}: portal stub failed: {e}"));
    }
    Ok(())
}

// ── shared fixtures ─────────────────────────────────────────────────────────

use backbone_website::application::service::website_service::{
    ActorRef, CreateWebsiteInput, WebsiteRootService, WebsiteView,
};

/// A unique company id per probe (companies are a host-side concern;
/// the website module only stores the id).
pub fn fresh_company() -> Uuid {
    Uuid::new_v4()
}

/// The website's bound host string (make_website always binds one; the
/// domain column is optional at the schema level).
pub fn host_of(website: &WebsiteView) -> String {
    website.domain.clone().unwrap_or_default()
}

/// Create one website (bootstrap verb) with a unique company, a unique
/// domain bound to it, and an EXPLICIT homepage url (the per-site
/// `websites.homepage_url` the bootstrap fork must stamp onto this
/// website's own homepage page — the default helper pins `/`).
pub async fn make_website_with_homepage(
    pool: &sqlx::PgPool,
    name: &str,
    homepage_url: &str,
) -> WebsiteView {
    let service = WebsiteRootService::new(pool.clone());
    let suffix: String = Uuid::new_v4().simple().to_string().chars().take(6).collect();
    let domain = format!("{}-{}.probe", name, suffix);
    service
        .create_website(
            ActorRef::officer(Uuid::new_v4()),
            CreateWebsiteInput {
                name: name.to_string(),
                domain: Some(domain),
                company_id: fresh_company(),
                public_user_id: None,
                default_lang_code: "en".into(),
                homepage_url: homepage_url.into(),
                robots_txt: Some("User-agent: *\nAllow: /\n".into()),
                social_links: None,
                contact_recipients: vec!["officer@example.com".into()],
                sequence: 10,
            },
        )
        .await
        .unwrap_or_else(|e| panic!("PROBE-FAIL: bootstrap website {name}: {e:?}"))
}

/// Create one website (bootstrap verb) with a unique company and a
/// unique domain bound to it.
pub async fn make_website(pool: &sqlx::PgPool, name: &str) -> WebsiteView {
    make_website_with_homepage(pool, name, "/").await
}

/// Serve `body` (a JSON siteverify answer) for `count` requests, then
/// stop accepting. Returns the base URL the stub listens on — a
/// PASSING stub is `{"success":true}`.
pub fn siteverify_stub(body: String, count: usize) -> String {
    let listener =
        TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| panic!("PROBE-FAIL: stub bind: {e}"));
    let addr = listener
        .local_addr()
        .unwrap_or_else(|e| panic!("PROBE-FAIL: stub addr: {e}"));
    std::thread::spawn(move || {
        for _ in 0..count {
            let Ok((mut sock, _)) = listener.accept() else { return };
            let mut buf = [0u8; 4096];
            let _ = sock.read(&mut buf); // drain the request (headers + form body)
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = sock.write_all(response.as_bytes());
            let _ = sock.flush();
        }
    });
    format!("http://{addr}/siteverify")
}
