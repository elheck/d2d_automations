//! The audit log: who did what and when.
//!
//! Entries are written in the same transaction as the change they record.
//! They keep the actor's name as text, so they survive deleting the user.
//! Details never contain secrets (passwords, tokens).

use crate::migrations::format_utc;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// Largest page `list` returns.
pub const MAX_PAGE: u32 = 200;

/// Who did it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor<'a> {
    User {
        id: i64,
        username: &'a str,
    },
    /// An admin command run on the server (`create-admin`, `import-desktop`, …).
    Cli,
    /// A login attempt before anyone is known.
    Anonymous,
}

impl Actor<'_> {
    fn name(&self) -> &str {
        match self {
            Actor::User { username, .. } => username,
            Actor::Cli => "cli",
            Actor::Anonymous => "anonymous",
        }
    }

    fn user_id(&self) -> Option<i64> {
        match self {
            Actor::User { id, .. } => Some(*id),
            _ => None,
        }
    }
}

/// Action names, so the log stays consistent.
pub mod action {
    pub const LOGIN: &str = "auth.login";
    pub const LOGIN_FAILED: &str = "auth.login_failed";
    pub const ACCOUNT_LOCKED: &str = "auth.account_locked";
    pub const LOGOUT: &str = "auth.logout";
    pub const PASSWORD_CHANGED: &str = "auth.password_changed";
    pub const USER_CREATED: &str = "user.created";
    pub const USER_UPDATED: &str = "user.updated";
    pub const USER_PASSWORD_RESET: &str = "user.password_reset";
    pub const USER_DELETED: &str = "user.deleted";
    pub const DESKTOP_IMPORTED: &str = "import.desktop";
    pub const DESKTOP_REFRESHED: &str = "import.refresh";
}

/// Adds an entry.
pub fn record(
    conn: &Connection,
    now: DateTime<Utc>,
    actor: Actor<'_>,
    action: &str,
    target: Option<&str>,
    details: serde_json::Value,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO srv_audit_log (at, actor_user_id, actor, action, target, details)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            format_utc(now),
            actor.user_id(),
            actor.name(),
            action,
            target,
            details.to_string()
        ],
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: i64,
    pub at: String,
    pub actor: String,
    pub action: String,
    pub target: Option<String>,
    pub details: serde_json::Value,
}

/// Newest entries first, older than `before_id` if given; at most `limit`
/// (capped at [`MAX_PAGE`]).
pub fn list(
    conn: &Connection,
    before_id: Option<i64>,
    limit: u32,
) -> rusqlite::Result<Vec<AuditEntry>> {
    let limit = limit.clamp(1, MAX_PAGE);
    let mut stmt = conn.prepare(
        "SELECT id, at, actor, action, target, details FROM srv_audit_log
         WHERE ?1 IS NULL OR id < ?1
         ORDER BY id DESC LIMIT ?2",
    )?;
    let entries = stmt
        .query_map(params![before_id, limit], |r| {
            let details: String = r.get(5)?;
            Ok(AuditEntry {
                id: r.get(0)?,
                at: r.get(1)?,
                actor: r.get(2)?,
                action: r.get(3)?,
                target: r.get(4)?,
                details: serde_json::from_str(&details).unwrap_or(serde_json::Value::Null),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(entries)
}

#[cfg(test)]
#[path = "audit_tests.rs"]
mod tests;
