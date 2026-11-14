//! Login sessions.
//!
//! A session is a random 32-byte token in an `HttpOnly; Secure;
//! SameSite=Strict` cookie. The database (and with it every backup) holds
//! only the token's SHA-256, so a leaked copy of the database contains no
//! usable sessions. A session ends after [`IDLE_TIMEOUT`] without requests
//! and at the latest [`ABSOLUTE_LIFETIME`] after login.

use crate::migrations::{format_utc, parse_utc};
use axum::http::{header, HeaderMap};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

/// Cookie name. The `__Host-` prefix makes browsers insist on `Secure`,
/// `Path=/` and no `Domain`, so no other site or subdomain can set it.
pub const COOKIE_NAME: &str = "__Host-check_stock_session";

/// Logged out after this long without a request.
pub const IDLE_TIMEOUT: Duration = Duration::hours(8);
/// Logged out at the latest this long after logging in.
pub const ABSOLUTE_LIFETIME: Duration = Duration::days(7);
/// `last_seen_at` is written at most this often per session.
pub const TOUCH_INTERVAL: Duration = Duration::minutes(5);

const TOKEN_BYTES: usize = 32;
const MAX_USER_AGENT_CHARS: usize = 200;

/// The secret in the cookie (64 hex characters). Never stored, never logged.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionToken(String);

impl std::fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionToken(…)")
    }
}

impl SessionToken {
    /// A new token from the operating system's random generator.
    pub fn generate() -> Result<Self, getrandom::Error> {
        let mut bytes = [0u8; TOKEN_BYTES];
        getrandom::fill(&mut bytes)?;
        Ok(Self(bytes.iter().map(|b| format!("{b:02x}")).collect()))
    }

    /// Accepts only well-formed tokens (64 lower-case hex characters).
    pub fn parse(value: &str) -> Option<Self> {
        (value.len() == TOKEN_BYTES * 2
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .then(|| Self(value.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// What the database stores.
    pub fn hash(&self) -> Vec<u8> {
        Sha256::digest(self.0.as_bytes()).to_vec()
    }
}

/// The session token from the request's `Cookie` headers, if well-formed.
pub fn token_from_headers(headers: &HeaderMap) -> Option<SessionToken> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE_NAME)
        .and_then(|(_, value)| SessionToken::parse(value.trim()))
}

/// `Set-Cookie` value that stores the token in the browser.
pub fn set_cookie(token: &SessionToken) -> String {
    format!(
        "{COOKIE_NAME}={}; Path=/; Max-Age={}; HttpOnly; Secure; SameSite=Strict",
        token.as_str(),
        ABSOLUTE_LIFETIME.num_seconds()
    )
}

/// `Set-Cookie` value that removes the cookie.
pub fn clear_cookie() -> String {
    format!("{COOKIE_NAME}=; Path=/; Max-Age=0; HttpOnly; Secure; SameSite=Strict")
}

/// Stores a new session for `user_id`.
pub fn create(
    conn: &Connection,
    token: &SessionToken,
    user_id: i64,
    user_agent: Option<&str>,
    now: DateTime<Utc>,
) -> rusqlite::Result<()> {
    let user_agent: Option<String> = user_agent.map(|ua| {
        ua.chars()
            .filter(|c| !c.is_control())
            .take(MAX_USER_AGENT_CHARS)
            .collect()
    });
    conn.execute(
        "INSERT INTO srv_sessions (token_hash, user_id, created_at, last_seen_at, expires_at, user_agent)
         VALUES (?1, ?2, ?3, ?3, ?4, ?5)",
        params![
            token.hash(),
            user_id,
            format_utc(now),
            format_utc(now + ABSOLUTE_LIFETIME),
            user_agent
        ],
    )?;
    Ok(())
}

/// A stored session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub user_id: i64,
    pub last_seen_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// What a stored session is worth at a given moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Valid { needs_touch: bool },
    Expired,
}

impl Session {
    pub fn status(&self, now: DateTime<Utc>) -> SessionStatus {
        if now >= self.expires_at || now - self.last_seen_at >= IDLE_TIMEOUT {
            SessionStatus::Expired
        } else {
            SessionStatus::Valid {
                needs_touch: now - self.last_seen_at >= TOUCH_INTERVAL,
            }
        }
    }
}

/// The stored session for a token hash. A row with unreadable times counts as
/// expired.
pub fn find(conn: &Connection, token_hash: &[u8]) -> rusqlite::Result<Option<Session>> {
    let row: Option<(i64, String, String)> = conn
        .query_row(
            "SELECT user_id, last_seen_at, expires_at FROM srv_sessions WHERE token_hash = ?1",
            params![token_hash],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(row.map(|(user_id, last_seen, expires)| Session {
        user_id,
        last_seen_at: parse_utc(&last_seen).unwrap_or(DateTime::<Utc>::MIN_UTC),
        expires_at: parse_utc(&expires).unwrap_or(DateTime::<Utc>::MIN_UTC),
    }))
}

/// Records activity (keeps the session from idling out).
pub fn touch(conn: &Connection, token_hash: &[u8], now: DateTime<Utc>) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE srv_sessions SET last_seen_at = ?2 WHERE token_hash = ?1",
        params![token_hash, format_utc(now)],
    )?;
    Ok(())
}

/// Ends one session.
pub fn revoke(conn: &Connection, token_hash: &[u8]) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM srv_sessions WHERE token_hash = ?1",
        params![token_hash],
    )?;
    Ok(())
}

/// Ends every session of a user (password change, disabled account).
pub fn revoke_all_for_user(conn: &Connection, user_id: i64) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM srv_sessions WHERE user_id = ?1",
        params![user_id],
    )
}

/// Deletes sessions that have expired by either rule.
pub fn prune_expired(conn: &Connection, now: DateTime<Utc>) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM srv_sessions WHERE expires_at <= ?1 OR last_seen_at <= ?2",
        params![format_utc(now), format_utc(now - IDLE_TIMEOUT)],
    )
}

#[cfg(test)]
#[path = "sessions_tests.rs"]
mod tests;
