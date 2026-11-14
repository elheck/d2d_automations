//! Schema of the server's own tables, versioned separately from core.
//!
//! The server keeps its tables in the same SQLite file as the inventory data
//! (so a change and its audit-log entry commit together), but never touches
//! core's tables here. Every server table name starts with [`TABLE_PREFIX`];
//! core never uses that prefix, which also lets a later refresh from the
//! desktop database replace exactly the non-server tables.
//!
//! Migrations only ever run forward. A database written by a newer server
//! (higher version than [`LATEST`]) is refused rather than misread.

use rusqlite::{params, Connection, OptionalExtension};

/// Prefix of every server table.
pub const TABLE_PREFIX: &str = "srv_";

/// `(version, SQL)`, applied in order, each in its own transaction.
/// Append only: never edit a migration that has shipped.
const MIGRATIONS: &[(u32, &str)] = &[
    (
        1,
        "CREATE TABLE srv_meta (
             key   TEXT PRIMARY KEY NOT NULL,
             value TEXT NOT NULL
         ) STRICT;",
    ),
    (
        2,
        // Users, their permissions, login sessions and the audit log.
        // Times are RFC 3339 UTC strings (`now_utc`), so they also compare
        // correctly as text. Sessions store only the SHA-256 of the token.
        "CREATE TABLE srv_users (
             id                  INTEGER PRIMARY KEY,
             username            TEXT NOT NULL UNIQUE COLLATE NOCASE,
             display_name        TEXT NOT NULL,
             password_hash       TEXT NOT NULL,
             disabled            INTEGER NOT NULL DEFAULT 0 CHECK (disabled IN (0, 1)),
             failed_logins       INTEGER NOT NULL DEFAULT 0,
             locked_until        TEXT,
             password_changed_at TEXT NOT NULL,
             created_at          TEXT NOT NULL,
             last_login_at       TEXT
         ) STRICT;
         CREATE TABLE srv_user_permissions (
             user_id    INTEGER NOT NULL REFERENCES srv_users (id) ON DELETE CASCADE,
             permission TEXT NOT NULL,
             PRIMARY KEY (user_id, permission)
         ) STRICT, WITHOUT ROWID;
         CREATE TABLE srv_sessions (
             token_hash   BLOB PRIMARY KEY NOT NULL,
             user_id      INTEGER NOT NULL REFERENCES srv_users (id) ON DELETE CASCADE,
             created_at   TEXT NOT NULL,
             last_seen_at TEXT NOT NULL,
             expires_at   TEXT NOT NULL,
             user_agent   TEXT
         ) STRICT;
         CREATE INDEX srv_sessions_user ON srv_sessions (user_id);
         CREATE TABLE srv_audit_log (
             id            INTEGER PRIMARY KEY,
             at            TEXT NOT NULL,
             actor_user_id INTEGER,
             actor         TEXT NOT NULL,
             action        TEXT NOT NULL,
             target        TEXT,
             details       TEXT NOT NULL DEFAULT '{}'
         ) STRICT;",
    ),
];

/// Newest schema version this build knows.
pub const LATEST: u32 = MIGRATIONS[MIGRATIONS.len() - 1].0;

#[derive(Debug)]
pub enum MigrationError {
    /// The database was written by a newer server version.
    NewerSchema {
        found: u32,
        supported: u32,
    },
    Sqlite(rusqlite::Error),
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationError::NewerSchema { found, supported } => write!(
                f,
                "the database has server schema version {found}, but this build only knows up to {supported}; use a newer check_stock_server"
            ),
            MigrationError::Sqlite(e) => write!(f, "server schema migration failed: {e}"),
        }
    }
}

impl std::error::Error for MigrationError {}

impl From<rusqlite::Error> for MigrationError {
    fn from(e: rusqlite::Error) -> Self {
        MigrationError::Sqlite(e)
    }
}

/// Brings the server tables up to [`LATEST`]. Returns the versions applied.
pub fn run(conn: &mut Connection) -> Result<Vec<u32>, MigrationError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS srv_migrations (
             version    INTEGER PRIMARY KEY NOT NULL,
             applied_at TEXT NOT NULL
         ) STRICT;",
    )?;
    let current = current_version(conn)?;
    if current > LATEST {
        return Err(MigrationError::NewerSchema {
            found: current,
            supported: LATEST,
        });
    }
    let mut applied = Vec::new();
    for &(version, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute(
            "INSERT INTO srv_migrations (version, applied_at) VALUES (?1, ?2)",
            params![version, now_utc()],
        )?;
        tx.commit()?;
        log::info!("Applied server schema migration {version}");
        applied.push(version);
    }
    Ok(applied)
}

/// Highest applied server schema version (0 for a database without server tables).
pub fn current_version(conn: &Connection) -> rusqlite::Result<u32> {
    let has_table: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'srv_migrations'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !has_table {
        return Ok(0);
    }
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM srv_migrations",
        [],
        |row| row.get(0),
    )
}

/// Reads one server setting.
pub fn get_meta(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM srv_meta WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
}

/// Writes one server setting.
pub fn set_meta(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO srv_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Current time as RFC 3339 in UTC, as stored in the server tables.
pub fn now_utc() -> String {
    format_utc(chrono::Utc::now())
}

/// A time in the stored format (`2026-11-14T18:08:05Z`): fixed width, so the
/// strings also sort and compare correctly in SQL.
pub fn format_utc(time: chrono::DateTime<chrono::Utc>) -> String {
    time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Parses a stored time; `None` if it is not valid RFC 3339.
pub fn parse_utc(text: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|t| t.with_timezone(&chrono::Utc))
}

#[cfg(test)]
#[path = "migrations_tests.rs"]
mod tests;
