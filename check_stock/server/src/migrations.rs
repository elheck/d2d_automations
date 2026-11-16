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
    (
        3,
        // Per-user settings of the screens (e.g. the buy helper's rules), one
        // JSON value per user and setting; gone with the user.
        "CREATE TABLE srv_user_settings (
             user_id    INTEGER NOT NULL REFERENCES srv_users (id) ON DELETE CASCADE,
             key        TEXT NOT NULL,
             value      TEXT NOT NULL CHECK (json_valid(value)),
             updated_at TEXT NOT NULL,
             PRIMARY KEY (user_id, key)
         ) STRICT, WITHOUT ROWID;",
    ),
    (
        4,
        // Inventory imports on the server (4a). Every change of the inventory
        // data gets a version row per category (import, forced import,
        // restore from a backup, desktop copy); the name is kept when the
        // user is deleted. A pending import holds only what is needed to
        // check the confirmation (the file is uploaded again): its SHA-256,
        // category, the version it was previewed against and the preview
        // numbers. The file of the last import per category is kept for bin
        // analysis (every pile with its own location; the inventory tables
        // merge them). Visit days per user for the start page's digest.
        "CREATE TABLE srv_inventory_versions (
             id              INTEGER PRIMARY KEY,
             category        TEXT NOT NULL,
             created_at      TEXT NOT NULL,
             kind            TEXT NOT NULL
                 CHECK (kind IN ('import', 'forced_import', 'restore', 'desktop_copy')),
             user_id         INTEGER REFERENCES srv_users (id) ON DELETE SET NULL,
             user_name       TEXT NOT NULL,
             file_sha256     TEXT,
             file_rows       INTEGER NOT NULL DEFAULT 0,
             copies_sold     INTEGER NOT NULL DEFAULT 0,
             new_variants    INTEGER NOT NULL DEFAULT 0,
             zeroed_variants INTEGER NOT NULL DEFAULT 0
         ) STRICT;
         CREATE INDEX srv_inventory_versions_category ON srv_inventory_versions (category, id);
         CREATE TABLE srv_pending_imports (
             user_id      INTEGER PRIMARY KEY REFERENCES srv_users (id) ON DELETE CASCADE,
             category     TEXT NOT NULL,
             file_sha256  TEXT NOT NULL,
             base_version INTEGER NOT NULL,
             preview      TEXT NOT NULL CHECK (json_valid(preview)),
             created_at   TEXT NOT NULL,
             expires_at   TEXT NOT NULL
         ) STRICT;
         CREATE TABLE srv_inventory_files (
             category   TEXT PRIMARY KEY NOT NULL,
             version_id INTEGER NOT NULL REFERENCES srv_inventory_versions (id),
             file_name  TEXT NOT NULL,
             csv        BLOB NOT NULL
         ) STRICT;
         CREATE TABLE srv_user_visits (
             user_id    INTEGER PRIMARY KEY REFERENCES srv_users (id) ON DELETE CASCADE,
             last_visit TEXT NOT NULL,
             prev_visit TEXT
         ) STRICT;",
    ),
    (
        5,
        // Bin consolidation move lists (3e), one per user. Only the settings
        // are kept: the moves are planned again from the kept import file
        // (the planner is deterministic) for `version_id`; ticks are move ids.
        "CREATE TABLE srv_move_lists (
             user_id    INTEGER PRIMARY KEY REFERENCES srv_users (id) ON DELETE CASCADE,
             kind       TEXT NOT NULL CHECK (kind IN ('consolidation', 'defrag')),
             threshold  INTEGER,
             category   TEXT NOT NULL,
             version_id INTEGER NOT NULL,
             created_at TEXT NOT NULL
         ) STRICT;
         CREATE TABLE srv_move_list_ticks (
             user_id INTEGER NOT NULL REFERENCES srv_move_lists (user_id) ON DELETE CASCADE,
             move_id TEXT NOT NULL,
             PRIMARY KEY (user_id, move_id)
         ) STRICT, WITHOUT ROWID;",
    ),
    (
        6,
        // Discards from Search (4b). Until the next import (or restore or
        // desktop copy) settles them, they are taken off the kept import
        // file for every screen that reads it, and an export that still
        // holds them is refused by the import check. `items` names every
        // pile (id, card, location, copies); the stock-update CSV is kept
        // for the newest 20 so it can be downloaded again. Move lists
        // remember which discards their plan already saw.
        "CREATE TABLE srv_discards (
             id           INTEGER PRIMARY KEY,
             category     TEXT NOT NULL,
             file_version INTEGER NOT NULL,
             created_at   TEXT NOT NULL,
             user_id      INTEGER REFERENCES srv_users (id) ON DELETE SET NULL,
             user_name    TEXT NOT NULL,
             copies       INTEGER NOT NULL,
             variants     INTEGER NOT NULL,
             items        TEXT NOT NULL CHECK (json_valid(items)),
             csv          BLOB,
             settled_at   TEXT
         ) STRICT;
         CREATE INDEX srv_discards_pending ON srv_discards (category, settled_at);
         ALTER TABLE srv_move_lists ADD COLUMN discard_seq INTEGER NOT NULL DEFAULT 0;",
    ),
    (
        7,
        // One picking list per user (4b): which card, from where, how many,
        // picked or not, plus the names and prices to show; the stock it was
        // made on (a later import or discard shows a note).
        "CREATE TABLE srv_picking_sessions (
             user_id     INTEGER PRIMARY KEY REFERENCES srv_users (id) ON DELETE CASCADE,
             created_at  TEXT NOT NULL,
             version_id  INTEGER NOT NULL,
             discard_seq INTEGER NOT NULL,
             items       TEXT NOT NULL CHECK (json_valid(items))
         ) STRICT;",
    ),
    (
        8,
        // Bulk lots (4c): one draft per user (the rules, the chosen piles
        // with their picked ticks, piles swapped out), the TCG Powertools
        // CSV of each recorded lot (to download again), and a counter of
        // other data changes (finished, corrected, removed or undone lots)
        // that is part of the data version open pages watch.
        "CREATE TABLE srv_bulk_lot_drafts (
             user_id    INTEGER PRIMARY KEY REFERENCES srv_users (id) ON DELETE CASCADE,
             spec       TEXT NOT NULL CHECK (json_valid(spec)),
             picks      TEXT NOT NULL CHECK (json_valid(picks)),
             excluded   TEXT NOT NULL CHECK (json_valid(excluded)),
             updated_at TEXT NOT NULL
         ) STRICT;
         CREATE TABLE srv_bulk_lot_csvs (
             lot_id    INTEGER PRIMARY KEY,
             file_name TEXT NOT NULL,
             csv       BLOB NOT NULL
         ) STRICT;
         CREATE TABLE srv_data_changes (
             id   INTEGER PRIMARY KEY,
             at   TEXT NOT NULL,
             kind TEXT NOT NULL
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
