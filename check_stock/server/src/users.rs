//! User accounts in `srv_users` and their permissions in
//! `srv_user_permissions`.
//!
//! Plain functions on a connection, so callers combine several of them (and
//! the audit entry) in one transaction. Passwords arrive here already hashed.

use crate::migrations::{format_utc, parse_utc};
use crate::permissions::{Permission, PermissionSet};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};

/// Failed logins in a row before the account is locked.
pub const MAX_FAILED_LOGINS: i64 = 5;
/// How long a locked account stays locked.
pub const LOCKOUT: Duration = Duration::minutes(15);

pub const MIN_USERNAME_CHARS: usize = 3;
pub const MAX_USERNAME_CHARS: usize = 32;
pub const MAX_DISPLAY_NAME_CHARS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub password_hash: String,
    pub disabled: bool,
    pub failed_logins: i64,
    pub locked_until: Option<DateTime<Utc>>,
    pub password_changed_at: String,
    pub created_at: String,
    pub last_login_at: Option<String>,
    pub permissions: PermissionSet,
}

impl User {
    /// Locked by too many failed logins at `now`.
    pub fn is_locked(&self, now: DateTime<Utc>) -> bool {
        self.locked_until.is_some_and(|until| until > now)
    }

    pub fn can(&self, permission: Permission) -> bool {
        self.permissions.contains(permission)
    }
}

#[derive(Debug)]
pub enum StoreError {
    UsernameTaken,
    NotFound,
    /// The change would leave no enabled user who can manage users.
    LastAdmin,
    Sqlite(rusqlite::Error),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::UsernameTaken => write!(f, "this user name is already taken"),
            StoreError::NotFound => write!(f, "no such user"),
            StoreError::LastAdmin => write!(
                f,
                "at least one enabled user must keep the \"{}\" permission",
                Permission::UsersManage.label()
            ),
            StoreError::Sqlite(e) => write!(f, "database error: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sqlite(e)
    }
}

/// Lower-case login name: 3–32 of `a-z 0-9 . _ -`, starting with a letter or
/// digit. Returns a message for the user when invalid.
pub fn normalize_username(raw: &str) -> Result<String, String> {
    let name = raw.trim().to_ascii_lowercase();
    let len = name.chars().count();
    if !(MIN_USERNAME_CHARS..=MAX_USERNAME_CHARS).contains(&len) {
        return Err(format!(
            "The user name needs {MIN_USERNAME_CHARS} to {MAX_USERNAME_CHARS} characters."
        ));
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(
            "The user name may only use letters a-z, digits, '.', '_' and '-'.".to_string(),
        );
    }
    if !name.as_bytes()[0].is_ascii_alphanumeric() {
        return Err("The user name must start with a letter or digit.".to_string());
    }
    Ok(name)
}

/// Trimmed display name: 1–64 characters, no control characters.
pub fn normalize_display_name(raw: &str) -> Result<String, String> {
    let name = raw.trim().to_string();
    let len = name.chars().count();
    if len == 0 || len > MAX_DISPLAY_NAME_CHARS {
        return Err(format!(
            "The display name needs 1 to {MAX_DISPLAY_NAME_CHARS} characters."
        ));
    }
    if name.chars().any(char::is_control) {
        return Err("The display name must not contain control characters.".to_string());
    }
    Ok(name)
}

/// Inserts a user and its permissions; returns the new id. `username` must
/// already be normalized.
pub fn create(
    conn: &Connection,
    username: &str,
    display_name: &str,
    password_hash: &str,
    permissions: &PermissionSet,
    now: DateTime<Utc>,
) -> Result<i64, StoreError> {
    if find_by_username(conn, username)?.is_some() {
        return Err(StoreError::UsernameTaken);
    }
    let now = format_utc(now);
    conn.execute(
        "INSERT INTO srv_users (username, display_name, password_hash, password_changed_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![username, display_name, password_hash, now],
    )?;
    let id = conn.last_insert_rowid();
    set_permissions(conn, id, permissions)?;
    Ok(id)
}

const SELECT_USER: &str =
    "SELECT id, username, display_name, password_hash, disabled, failed_logins,
            locked_until, password_changed_at, created_at, last_login_at
     FROM srv_users";

fn user_from_row(row: &Row<'_>) -> rusqlite::Result<User> {
    let locked_until: Option<String> = row.get(6)?;
    Ok(User {
        id: row.get(0)?,
        username: row.get(1)?,
        display_name: row.get(2)?,
        password_hash: row.get(3)?,
        disabled: row.get::<_, i64>(4)? != 0,
        failed_logins: row.get(5)?,
        locked_until: locked_until.as_deref().and_then(parse_utc),
        password_changed_at: row.get(7)?,
        created_at: row.get(8)?,
        last_login_at: row.get(9)?,
        permissions: PermissionSet::new(),
    })
}

fn with_permissions(conn: &Connection, mut user: User) -> rusqlite::Result<User> {
    user.permissions = permissions_of(conn, user.id)?;
    Ok(user)
}

/// The user with this login name (case-insensitive).
pub fn find_by_username(conn: &Connection, username: &str) -> rusqlite::Result<Option<User>> {
    let user = conn
        .query_row(
            &format!("{SELECT_USER} WHERE username = ?1"),
            params![username.trim()],
            user_from_row,
        )
        .optional()?;
    user.map(|u| with_permissions(conn, u)).transpose()
}

pub fn find_by_id(conn: &Connection, id: i64) -> rusqlite::Result<Option<User>> {
    let user = conn
        .query_row(
            &format!("{SELECT_USER} WHERE id = ?1"),
            params![id],
            user_from_row,
        )
        .optional()?;
    user.map(|u| with_permissions(conn, u)).transpose()
}

/// Every user, by login name.
pub fn list(conn: &Connection) -> rusqlite::Result<Vec<User>> {
    let mut stmt = conn.prepare(&format!("{SELECT_USER} ORDER BY username"))?;
    let users = stmt
        .query_map([], user_from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    users
        .into_iter()
        .map(|u| with_permissions(conn, u))
        .collect()
}

pub fn count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM srv_users", [], |r| r.get(0))
}

/// Granted permissions; keys this build does not know (e.g. written by a
/// newer version) are skipped.
pub fn permissions_of(conn: &Connection, user_id: i64) -> rusqlite::Result<PermissionSet> {
    let mut stmt =
        conn.prepare("SELECT permission FROM srv_user_permissions WHERE user_id = ?1")?;
    let keys = stmt
        .query_map(params![user_id], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut set = PermissionSet::new();
    for key in keys {
        match Permission::from_key(&key) {
            Some(p) => set.insert(p),
            None => log::warn!("Ignoring unknown permission {key:?} of user {user_id}"),
        }
    }
    Ok(set)
}

/// Replaces the user's permissions.
pub fn set_permissions(
    conn: &Connection,
    user_id: i64,
    permissions: &PermissionSet,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM srv_user_permissions WHERE user_id = ?1",
        params![user_id],
    )?;
    let mut insert =
        conn.prepare("INSERT INTO srv_user_permissions (user_id, permission) VALUES (?1, ?2)")?;
    for key in permissions.keys() {
        insert.execute(params![user_id, key])?;
    }
    Ok(())
}

fn changed_one(rows: usize) -> Result<(), StoreError> {
    if rows == 1 {
        Ok(())
    } else {
        Err(StoreError::NotFound)
    }
}

pub fn set_display_name(conn: &Connection, user_id: i64, name: &str) -> Result<(), StoreError> {
    changed_one(conn.execute(
        "UPDATE srv_users SET display_name = ?2 WHERE id = ?1",
        params![user_id, name],
    )?)
}

pub fn set_disabled(conn: &Connection, user_id: i64, disabled: bool) -> Result<(), StoreError> {
    changed_one(conn.execute(
        "UPDATE srv_users SET disabled = ?2 WHERE id = ?1",
        params![user_id, i64::from(disabled)],
    )?)
}

/// New password hash; also unlocks the account.
pub fn set_password(
    conn: &Connection,
    user_id: i64,
    password_hash: &str,
    now: DateTime<Utc>,
) -> Result<(), StoreError> {
    changed_one(conn.execute(
        "UPDATE srv_users
         SET password_hash = ?2, password_changed_at = ?3, failed_logins = 0, locked_until = NULL
         WHERE id = ?1",
        params![user_id, password_hash, format_utc(now)],
    )?)
}

pub fn unlock(conn: &Connection, user_id: i64) -> Result<(), StoreError> {
    changed_one(conn.execute(
        "UPDATE srv_users SET failed_logins = 0, locked_until = NULL WHERE id = ?1",
        params![user_id],
    )?)
}

/// Deletes the user; sessions and permissions go with it (foreign keys).
pub fn delete(conn: &Connection, user_id: i64) -> Result<(), StoreError> {
    changed_one(conn.execute("DELETE FROM srv_users WHERE id = ?1", params![user_id])?)
}

/// Counts a failed login. Returns `true` when this failure locked the account
/// (after [`MAX_FAILED_LOGINS`] in a row); the counter then starts over.
pub fn record_failed_login(
    conn: &Connection,
    user_id: i64,
    now: DateTime<Utc>,
) -> Result<bool, StoreError> {
    let failed: i64 = conn
        .query_row(
            "UPDATE srv_users SET failed_logins = failed_logins + 1 WHERE id = ?1
             RETURNING failed_logins",
            params![user_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(StoreError::NotFound)?;
    if failed >= MAX_FAILED_LOGINS {
        conn.execute(
            "UPDATE srv_users SET failed_logins = 0, locked_until = ?2 WHERE id = ?1",
            params![user_id, format_utc(now + LOCKOUT)],
        )?;
        return Ok(true);
    }
    Ok(false)
}

/// Resets the failure counter and remembers the login time.
pub fn record_successful_login(
    conn: &Connection,
    user_id: i64,
    now: DateTime<Utc>,
) -> Result<(), StoreError> {
    changed_one(conn.execute(
        "UPDATE srv_users SET failed_logins = 0, locked_until = NULL, last_login_at = ?2
         WHERE id = ?1",
        params![user_id, format_utc(now)],
    )?)
}

/// Enabled users who can manage users.
pub fn enabled_admin_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM srv_users u
         JOIN srv_user_permissions p ON p.user_id = u.id AND p.permission = ?1
         WHERE u.disabled = 0",
        params![Permission::UsersManage.key()],
        |r| r.get(0),
    )
}

/// Fails with [`StoreError::LastAdmin`] if no enabled user can manage users.
/// Call it after a change, inside the same transaction, and roll back on
/// error: then no sequence of changes can lock everyone out.
pub fn ensure_an_admin_remains(conn: &Connection) -> Result<(), StoreError> {
    if enabled_admin_count(conn)? == 0 {
        Err(StoreError::LastAdmin)
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "users_tests.rs"]
mod tests;
