//! Visitor service (hand-written; user-owned; see
//! `metaphor.codegen.yaml`).
//!
//! The generated CRUD alias first (keeps lib.rs's wiring compiling),
//! then the GDPR-shaped visitor engine: the keyed digest (HMAC-SHA256
//! over IP+UA+session-id under the rotatable pepper — the RAW IP is
//! never persisted), random 32-byte tokens (both kinds, one column +
//! `kind`), the ONE-statement heartbeat upsert with the track CTE
//! riding it, `FOR NO KEY UPDATE SKIP LOCKED` on concurrent merges,
//! and the login merge (claim-in-place / reparent — never a token
//! upgrade). NO public or officer CREATE verb exists for visitors:
//! the heartbeat is the only creator.

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha256;
use sqlx::PgPool;
use uuid::Uuid;

use backbone_core::GenericCrudService;
use crate::domain::entity::Visitor;
use crate::infrastructure::persistence::VisitorRepository;
use crate::presentation::dto::{CreateVisitorDto, UpdateVisitorDto};

/// Generated CRUD alias (the generator skipped emitting this file
/// because it is user-owned).
pub type VisitorService = GenericCrudService<
    Visitor,
    CreateVisitorDto,
    UpdateVisitorDto,
    VisitorRepository,
>;

use super::website_error::WebsiteError;
use super::website_service::{record_audit, ActorRef};

/// The visit window: a connection after this much silence opens a NEW
/// visit (drives `visit_count`).
pub const VISIT_WINDOW_HOURS: i64 = 8;

/// The connected window: a connection within it counts the visitor as
/// "currently connected" (derived at read).
pub const CONNECTED_WINDOW_MINUTES: i64 = 5;

/// The track dedup window: the same url within it does not write a
/// second track row.
pub const TRACK_DEDUP_MINUTES: i64 = 30;

/// The digest construction label stored beside every digest.
pub const DIGEST_ALGO: &str = "hmac-sha256-v1";

/// The domain-separation label of the digest message.
const DIGEST_CONTEXT: &str = "website-visitor-v1";

type HmacSha256 = Hmac<Sha256>;

/// The keyed digest: hex(HMAC-SHA256(pepper, "website-visitor-v1\n" +
/// ip + "\n" + user_agent + "\n" + session_id)). Domain-separated,
/// newline-delimited arms. A missing pepper is a LOUD typed refusal —
/// the first visitor verb never digests under an empty key.
pub fn visitor_digest(pepper: &str, ip: &str, user_agent: &str, session_id: &str) -> Result<String, WebsiteError> {
    if pepper.is_empty() {
        return Err(WebsiteError::VisitorPepperNotConfigured);
    }
    let mut mac = HmacSha256::new_from_slice(pepper.as_bytes())
        .map_err(|e| WebsiteError::Internal(format!("pepper rejected by HMAC: {e}")))?;
    mac.update(DIGEST_CONTEXT.as_bytes());
    mac.update(b"\n");
    mac.update(ip.as_bytes());
    mac.update(b"\n");
    mac.update(user_agent.as_bytes());
    mac.update(b"\n");
    mac.update(session_id.as_bytes());
    Ok(hex_lower(&mac.finalize().into_bytes()))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// A random 32-byte url-safe token — BOTH kinds ride this generator
/// (the anonymous access token and any future identified partner
/// token); tokens are random and STAY random (no token upgrade ever).
/// Encoded as unpadded base64url: 32 bytes → exactly 43 characters.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(43);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        match chunk.len() {
            3 => {
                out.push(ALPHABET[(n >> 18 & 63) as usize] as char);
                out.push(ALPHABET[(n >> 12 & 63) as usize] as char);
                out.push(ALPHABET[(n >> 6 & 63) as usize] as char);
                out.push(ALPHABET[(n & 63) as usize] as char);
            }
            // The trailing 2-byte chunk's last group is zero-padding,
            // dropped (unpadded form).
            2 => {
                out.push(ALPHABET[(n >> 18 & 63) as usize] as char);
                out.push(ALPHABET[(n >> 12 & 63) as usize] as char);
                out.push(ALPHABET[(n >> 6 & 63) as usize] as char);
            }
            _ => unreachable!("chunks(3) yields 1..=3 byte chunks"),
        }
    }
    out
}

/// The session facts the heartbeat carries.
#[derive(Debug, Clone)]
pub struct SessionFacts<'a> {
    pub ip: &'a str,
    pub user_agent: &'a str,
    pub session_id: &'a str,
    pub country_code: Option<&'a str>,
}

/// The heartbeat's answer.
#[derive(Debug, Clone)]
pub struct HeartbeatOutcome {
    pub visitor_id: Uuid,
    /// True when this call minted the visitor row (false = existing
    /// row touched).
    pub inserted: bool,
    /// True when a track row rode this call (dedup window refused it).
    pub tracked: bool,
    pub access_token: String,
    /// The token kind as stored ('anonymous' at this activation).
    pub kind: String,
}

/// A visitor row as the officer read serves it (NO digest exposure —
/// the digest is an identity artifact, not display data).
#[derive(Debug, Clone, sqlx::FromRow, serde::Serialize)]
pub struct VisitorView {
    pub id: Uuid,
    pub website_id: Uuid,
    pub kind: String,
    pub digest_algo: String,
    pub portal_user_id: Option<Uuid>,
    pub country_code: Option<String>,
    pub visit_count: i32,
    pub last_connection_at: DateTime<Utc>,
}

/// The visitor engine.
pub struct VisitorEngine {
    pool: PgPool,
    pepper: String,
}

impl VisitorEngine {
    pub fn new(pool: PgPool, pepper: String) -> Self {
        Self { pool, pepper }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// The heartbeat — ONE statement: upsert the visitor on
    /// (digest, website) with the visit-window counter, the track CTE
    /// riding the same statement (a visitor row and its first track
    /// land or neither does), SKIP LOCKED semantics on concurrent
    /// upserts of the same digest via the unique conflict target.
    pub async fn heartbeat(
        &self,
        website_id: Uuid,
        session: &SessionFacts<'_>,
        url: Option<&str>,
        page_id: Option<Uuid>,
    ) -> Result<HeartbeatOutcome, WebsiteError> {
        let digest = visitor_digest(&self.pepper, session.ip, session.user_agent, session.session_id)?;
        let token = random_token();
        let with_track = url.is_some();
        let url = url.unwrap_or("");
        let page_id = page_id.filter(|_| with_track);

        let (visitor_id, inserted, token, kind, tracks): (Uuid, bool, String, String, i64) =
            sqlx::query_as(
                r#"
                WITH v AS (
                    INSERT INTO website.visitors
                        (id, website_id, access_token, kind, digest, digest_algo, country_code)
                    VALUES (gen_random_uuid(), $1, $2, 'anonymous', $3, $4, $5)
                    ON CONFLICT (digest, website_id) DO UPDATE SET
                        last_connection_at = now(),
                        visit_count = website.visitors.visit_count
                            + CASE WHEN website.visitors.last_connection_at
                                       < now() - make_interval(hours => $6) THEN 1 ELSE 0 END
                    RETURNING id, (xmax = 0) AS inserted, access_token, kind::text
                ), t AS (
                    INSERT INTO website.visitor_tracks (id, visitor_id, page_id, url, visited_at)
                    SELECT gen_random_uuid(), v.id, $7, $8, now() FROM v
                    WHERE $9::bool AND NOT EXISTS (
                        SELECT 1 FROM website.visitor_tracks x
                        WHERE x.visitor_id = v.id AND x.url = $8
                          AND x.visited_at > now() - make_interval(mins => $10))
                    RETURNING 1
                )
                SELECT v.id, v.inserted, v.access_token, v.kind, (SELECT count(*) FROM t) FROM v
                "#,
            )
            .bind(website_id)
            .bind(&token)
            .bind(&digest)
            .bind(DIGEST_ALGO)
            .bind(session.country_code)
            .bind(VISIT_WINDOW_HOURS as i32)
            .bind(page_id)
            .bind(url)
            .bind(with_track)
            .bind(TRACK_DEDUP_MINUTES as i32)
            .fetch_one(&self.pool)
            .await
            .map_err(super::website_error::map_unique_violation)?;

        Ok(HeartbeatOutcome {
            visitor_id,
            inserted,
            tracked: tracks > 0,
            access_token: token,
            kind,
        })
    }

    /// The login merge (officer verb, also the declared seam livechat /
    /// WB-8 consume). One transaction, idempotent, audited
    /// `visitor_merged`:
    ///
    /// 1. find the principal's identified visitor on this website;
    /// 2. exists → REPARENT the anonymous row's tracks onto it, delete
    ///    the anonymous row;
    /// 3. not exists → CLAIM the anonymous row in place (`kind =
    ///    'identified'`, `portal_user_id = $p`) — the RANDOM token is
    ///    kept, no token upgrade ever.
    pub async fn merge_visitor(
        &self,
        actor: ActorRef,
        website_id: Uuid,
        anonymous_visitor_id: Uuid,
        portal_user_id: Uuid,
    ) -> Result<VisitorView, WebsiteError> {
        let mut tx = self.pool.begin().await?;

        // The identified row, if one exists (lock-skip: a concurrent
        // merge for the same principal may already hold it).
        let identified: Option<Uuid> = sqlx::query_scalar(
            r#"
            SELECT id FROM website.visitors
            WHERE id IN (SELECT id FROM website.visitors
                         WHERE website_id = $1 AND portal_user_id = $2
                         FOR NO KEY UPDATE SKIP LOCKED)
            "#,
        )
        .bind(website_id)
        .bind(portal_user_id)
        .fetch_optional(&mut *tx)
        .await?;

        let survivor: Uuid = match identified {
            Some(existing) => {
                sqlx::query(
                    "UPDATE website.visitor_tracks SET visitor_id = $1 \
                     WHERE visitor_id = $2",
                )
                .bind(existing)
                .bind(anonymous_visitor_id)
                .execute(&mut *tx)
                .await?;
                sqlx::query("DELETE FROM website.visitors WHERE id = $1")
                    .bind(anonymous_visitor_id)
                    .execute(&mut *tx)
                    .await?;
                existing
            }
            None => {
                sqlx::query_scalar::<_, Uuid>(
                    r#"
                    UPDATE website.visitors
                    SET kind = 'identified',
                        portal_user_id = $2,
                        last_connection_at = now()
                    WHERE id IN (SELECT id FROM website.visitors
                                 WHERE id = $1
                                 FOR NO KEY UPDATE SKIP LOCKED)
                    RETURNING id
                    "#,
                )
                .bind(anonymous_visitor_id)
                .bind(portal_user_id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| {
                    WebsiteError::NotFound(format!("visitor {anonymous_visitor_id}"))
                })?
            }
        };

        record_audit(
            &mut *tx,
            "visitor_merged",
            actor,
            Some("visitor"),
            Some(survivor),
            Some(serde_json::json!({
                "website_id": website_id,
                "portal_user_id": portal_user_id,
                "anonymous_absorbed": anonymous_visitor_id,
                "mode": if identified.is_some() { "reparent" } else { "claim" },
            })),
        )
        .await?;

        let view = sqlx::query_as::<_, VisitorView>(
            r#"
            SELECT id, website_id, kind::text, digest_algo, portal_user_id,
                   country_code, visit_count, last_connection_at
            FROM website.visitors WHERE id = $1
            "#,
        )
        .bind(survivor)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(view)
    }

    /// Officer list for one website (digest never leaves the store).
    pub async fn list(&self, website_id: Uuid) -> Result<Vec<VisitorView>, WebsiteError> {
        let rows = sqlx::query_as::<_, VisitorView>(
            r#"
            SELECT id, website_id, kind::text, digest_algo, portal_user_id,
                   country_code, visit_count, last_connection_at
            FROM website.visitors
            WHERE website_id = $1
            ORDER BY last_connection_at DESC
            LIMIT 500
            "#,
        )
        .bind(website_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Officer read: the "currently connected" count, derived at read
    /// (last connection within the connected window).
    pub async fn connected_count(&self, website_id: Uuid) -> Result<i64, WebsiteError> {
        let (n,): (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*) FROM website.visitors
            WHERE website_id = $1
              AND last_connection_at > now() - make_interval(mins => $2)
            "#,
        )
        .bind(website_id)
        .bind(CONNECTED_WINDOW_MINUTES as i32)
        .fetch_one(&self.pool)
        .await?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_deterministic_and_pepper_gated() {
        let a = visitor_digest("pepper", "1.2.3.4", "UA", "s1").unwrap();
        let b = visitor_digest("pepper", "1.2.3.4", "UA", "s1").unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 64); // hex sha256
        // Pepper rotation deterministically changes the digest.
        let c = visitor_digest("pepper2", "1.2.3.4", "UA", "s1").unwrap();
        assert_ne!(a, c);
        // Unset pepper refuses loudly.
        assert!(matches!(
            visitor_digest("", "1.2.3.4", "UA", "s1"),
            Err(WebsiteError::VisitorPepperNotConfigured)
        ));
    }

    #[test]
    fn digest_arms_are_domain_separated() {
        // Newline-delimited arms: no concatenation ambiguity.
        let a = visitor_digest("p", "1.2", "3.4", "5").unwrap();
        let b = visitor_digest("p", "1", "2.3.4", "5").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn tokens_are_random_43_char_base64url() {
        for _ in 0..64 {
            let a = random_token();
            let b = random_token();
            assert_eq!(a.len(), 43, "32 bytes unpadded base64url = 43 chars");
            assert_ne!(a, b);
            assert!(
                a.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'),
                "url-safe alphabet only: {a}"
            );
        }
    }
}
