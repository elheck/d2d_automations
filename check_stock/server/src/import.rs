//! `import-desktop`: copies the desktop app's inventory database into the
//! server's data folder.
//!
//! Used for local development (`dev.sh reset-data`), for staging and, at
//! cutover, for the real move. The source is opened read-only and never
//! changed. The copy is taken with SQLite's backup API (a consistent snapshot
//! even while the desktop app has the file open), integrity-checked, brought
//! up to date by core and the server migrations, and only then moved into
//! place. An existing database is replaced only on request, and is kept as a
//! dated backup first.
//!
//! `--refresh` instead keeps the existing database and swaps only the
//! desktop tables (everything not prefixed `srv_`) for the fresh copy's, in
//! one transaction: users, sessions and the audit log stay.

use crate::audit::{self, action, Actor};
use crate::config::inventory_db_in;
use crate::data_dir::{DataDirLock, LockError};
use crate::db::{open_writer, DbError, BUSY_TIMEOUT};
use crate::migrations::{self, MigrationError, TABLE_PREFIX};
use check_stock_core::inventory_db::InventoryDb;
use rusqlite::{Connection, OpenFlags, OptionalExtension, MAIN_DB};
use std::path::{Path, PathBuf};

/// `srv_meta` key: canonical path of the copied desktop database.
pub const META_COPY_SOURCE: &str = "desktop_copy.source";
/// `srv_meta` key: when the copy was taken (RFC 3339, UTC). Shown as
/// "data as of …" once screens read the copy.
pub const META_COPY_TAKEN_AT: &str = "desktop_copy.taken_at";

/// Most integrity-check messages kept for the error report.
const MAX_INTEGRITY_MESSAGES: usize = 20;

#[derive(Debug)]
pub struct ImportReport {
    /// Where the copy now lives.
    pub target: PathBuf,
    /// Rows in `inventory_cards` (all categories, sold-out rows included).
    pub inventory_rows: i64,
    pub taken_at: String,
    /// Backup of the database that was replaced, if any.
    pub previous_kept_as: Option<PathBuf>,
}

#[derive(Debug)]
pub enum ImportError {
    SourceMissing(PathBuf),
    /// Source and target are the same file.
    SameFile(PathBuf),
    /// The source is not a SQLite file with an inventory table.
    NotAnInventoryDb(PathBuf, String),
    /// A database already exists and `--replace` was not given.
    TargetExists(PathBuf),
    /// `--refresh` needs an existing database.
    NothingToRefresh(PathBuf),
    /// `PRAGMA integrity_check` found problems in the copy.
    Corrupt(Vec<String>),
    Lock(LockError),
    Sqlite(rusqlite::Error),
    Migration(MigrationError),
    Io(std::io::Error),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::SourceMissing(p) => write!(f, "no file at {}", p.display()),
            ImportError::SameFile(p) => write!(
                f,
                "{} is already the server database; pick the desktop app's file",
                p.display()
            ),
            ImportError::NotAnInventoryDb(p, why) => write!(
                f,
                "{} is not a check_stock inventory database ({why})",
                p.display()
            ),
            ImportError::TargetExists(p) => write!(
                f,
                "{} already exists; pass --refresh to update only the desktop data (users stay), or --replace to replace the whole file (it is kept as a backup)",
                p.display()
            ),
            ImportError::NothingToRefresh(p) => write!(
                f,
                "there is no database at {} to refresh; import without --refresh first",
                p.display()
            ),
            ImportError::Corrupt(messages) => write!(
                f,
                "the copy failed the integrity check, nothing was replaced: {}",
                messages.join("; ")
            ),
            ImportError::Lock(e) => write!(f, "{e}"),
            ImportError::Sqlite(e) => write!(f, "database error: {e}"),
            ImportError::Migration(e) => write!(f, "{e}"),
            ImportError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ImportError {}

impl From<rusqlite::Error> for ImportError {
    fn from(e: rusqlite::Error) -> Self {
        ImportError::Sqlite(e)
    }
}

impl From<MigrationError> for ImportError {
    fn from(e: MigrationError) -> Self {
        ImportError::Migration(e)
    }
}

impl From<std::io::Error> for ImportError {
    fn from(e: std::io::Error) -> Self {
        ImportError::Io(e)
    }
}

impl From<LockError> for ImportError {
    fn from(e: LockError) -> Self {
        ImportError::Lock(e)
    }
}

impl From<DbError> for ImportError {
    fn from(e: DbError) -> Self {
        match e {
            DbError::Sqlite(e) => ImportError::Sqlite(e),
            DbError::Migration(e) => ImportError::Migration(e),
            other => ImportError::Io(std::io::Error::other(other.to_string())),
        }
    }
}

/// Copies the desktop database at `source` into `data_dir`.
pub fn import_desktop(
    source: &Path,
    data_dir: &Path,
    replace: bool,
) -> Result<ImportReport, ImportError> {
    let (_lock, source, target) = checked_paths(source, data_dir)?;
    if target.exists() && !replace {
        return Err(ImportError::TargetExists(target));
    }
    let snapshot = prepare_snapshot(&source, data_dir)?;

    let taken_at = migrations::now_utc();
    let inventory_rows = {
        let mut conn = InventoryDb::at(snapshot.path()).open()?;
        migrations::run(&mut conn)?;
        let rows = record_provenance(&conn, &source, &taken_at, action::DESKTOP_IMPORTED)?;
        drop(conn);
        rows
    };

    let previous_kept_as = if target.exists() {
        let backup = backup_current(&target)?;
        remove_database(&target)?;
        Some(backup)
    } else {
        None
    };
    std::fs::rename(snapshot.path(), &target)?;
    snapshot.disarm();
    Ok(ImportReport {
        target,
        inventory_rows,
        taken_at,
        previous_kept_as,
    })
}

/// Replaces only the desktop tables of the existing database in `data_dir`
/// with those of a fresh copy of `source`; the server's tables stay.
pub fn refresh_desktop(source: &Path, data_dir: &Path) -> Result<ImportReport, ImportError> {
    let (_lock, source, target) = checked_paths(source, data_dir)?;
    if !target.exists() {
        return Err(ImportError::NothingToRefresh(target));
    }
    let snapshot = prepare_snapshot(&source, data_dir)?;
    let previous_kept_as = backup_current(&target)?;

    let taken_at = migrations::now_utc();
    let mut conn = open_writer(&inventory_db_in(data_dir))?;
    conn.execute(
        "ATTACH DATABASE ?1 AS fresh",
        [snapshot.path().to_string_lossy().as_ref()],
    )?;
    let result = swap_desktop_tables(&mut conn).and_then(|()| {
        let tx = conn.transaction()?;
        let rows = record_provenance(&tx, &source, &taken_at, action::DESKTOP_REFRESHED)?;
        tx.commit()?;
        Ok(rows)
    });
    conn.execute("DETACH DATABASE fresh", [])?;
    let inventory_rows = result?;
    Ok(ImportReport {
        target,
        inventory_rows,
        taken_at,
        previous_kept_as: Some(previous_kept_as),
    })
}

/// Lock, canonical source, target path; refuses importing the target itself.
fn checked_paths(
    source: &Path,
    data_dir: &Path,
) -> Result<(DataDirLock, PathBuf, PathBuf), ImportError> {
    if !source.is_file() {
        return Err(ImportError::SourceMissing(source.to_path_buf()));
    }
    let lock = DataDirLock::acquire(data_dir)?;
    let target = inventory_db_in(data_dir).path().to_path_buf();
    let source = source.canonicalize()?;
    if target.exists() && target.canonicalize()? == source {
        return Err(ImportError::SameFile(source));
    }
    Ok((lock, source, target))
}

/// A consistent, integrity-checked copy of `source` next to the target, with
/// core's schema migrations and data repairs applied.
fn prepare_snapshot(source: &Path, data_dir: &Path) -> Result<TempFile, ImportError> {
    let tmp = TempFile::new(data_dir.join(format!("{}.importing", InventoryDb::FILE_NAME)))?;
    copy_snapshot(source, tmp.path())?;
    let problems = integrity_problems(tmp.path())?;
    if !problems.is_empty() {
        return Err(ImportError::Corrupt(problems));
    }
    drop(InventoryDb::at(tmp.path()).open()?);
    Ok(tmp)
}

/// Remembers where and when the copy was taken, logs it, returns the number
/// of inventory rows.
fn record_provenance(
    conn: &Connection,
    source: &Path,
    taken_at: &str,
    what: &str,
) -> Result<i64, ImportError> {
    let source_text = source.display().to_string();
    migrations::set_meta(conn, META_COPY_SOURCE, &source_text)?;
    migrations::set_meta(conn, META_COPY_TAKEN_AT, taken_at)?;
    let rows: i64 = conn.query_row("SELECT COUNT(*) FROM inventory_cards", [], |row| row.get(0))?;
    audit::record(
        conn,
        chrono::Utc::now(),
        Actor::Cli,
        what,
        Some(&source_text),
        serde_json::json!({ "inventory_rows": rows }),
    )?;
    Ok(rows)
}

/// In one transaction: drops every desktop table of `main` and recreates
/// them, with their rows, indexes and triggers, from the attached `fresh`
/// copy. Server tables (`srv_…`) and SQLite's own are not touched.
fn swap_desktop_tables(conn: &mut Connection) -> Result<(), ImportError> {
    let tx = conn.transaction()?;
    let not_ours = "name NOT LIKE 'sqlite\\_%' ESCAPE '\\' AND name NOT LIKE ?1 ESCAPE '\\'";
    let server_pattern = format!("{}%", TABLE_PREFIX.replace('_', "\\_"));
    let names = |schema: &str, kind: &str| -> rusqlite::Result<Vec<String>> {
        let mut stmt = tx.prepare(&format!(
            "SELECT name FROM {schema}.sqlite_master WHERE type = '{kind}' AND {not_ours} ORDER BY name"
        ))?;
        let rows = stmt.query_map([&server_pattern], |r| r.get::<_, String>(0))?;
        rows.collect()
    };
    for view in names("main", "view")? {
        tx.execute_batch(&format!("DROP VIEW main.{};", quoted(&view)))?;
    }
    for table in names("main", "table")? {
        tx.execute_batch(&format!("DROP TABLE main.{};", quoted(&table)))?;
    }
    let fresh_tables = names("fresh", "table")?;
    for table in &fresh_tables {
        let sql: String = tx.query_row(
            "SELECT sql FROM fresh.sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |r| r.get(0),
        )?;
        tx.execute_batch(&sql)?;
        tx.execute_batch(&format!(
            "INSERT INTO main.{t} SELECT * FROM fresh.{t};",
            t = quoted(table)
        ))?;
    }
    let has_sequence: bool = tx
        .query_row(
            "SELECT 1 FROM fresh.sqlite_master WHERE name = 'sqlite_sequence'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if has_sequence {
        tx.execute(
            "DELETE FROM main.sqlite_sequence WHERE name NOT LIKE ?1 ESCAPE '\\'",
            [&server_pattern],
        )?;
        tx.execute_batch("INSERT INTO main.sqlite_sequence SELECT * FROM fresh.sqlite_sequence;")?;
    }
    for kind in ["index", "trigger", "view"] {
        let mut stmt = tx.prepare(&format!(
            "SELECT sql FROM fresh.sqlite_master WHERE type = '{kind}' AND sql IS NOT NULL AND {not_ours}"
        ))?;
        let statements = stmt
            .query_map([&server_pattern], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        for sql in statements {
            tx.execute_batch(&sql)?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// SQL identifier in double quotes.
fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Consistent copy of `source` (opened read-only) into `dst`.
fn copy_snapshot(source: &Path, dst: &Path) -> Result<(), ImportError> {
    let not_inventory = |why: String| ImportError::NotAnInventoryDb(source.to_path_buf(), why);
    let src = Connection::open_with_flags(
        source,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| not_inventory(e.to_string()))?;
    src.busy_timeout(BUSY_TIMEOUT)?;
    let has_inventory = src
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'inventory_cards'",
            [],
            |_| Ok(()),
        )
        .optional()
        .map_err(|e| not_inventory(e.to_string()))?
        .is_some();
    if !has_inventory {
        return Err(not_inventory("no inventory_cards table".to_string()));
    }
    src.backup(MAIN_DB, dst, None::<fn(rusqlite::backup::Progress)>)?;
    Ok(())
}

/// Messages of `PRAGMA integrity_check`; empty when the file is fine.
fn integrity_problems(path: &Path) -> Result<Vec<String>, ImportError> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = conn.prepare("PRAGMA integrity_check")?;
    let messages = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .take(MAX_INTEGRITY_MESSAGES)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(if messages == ["ok"] {
        Vec::new()
    } else {
        messages
    })
}

/// Replaced databases kept next to the current one; older ones are deleted.
pub const KEEP_REPLACED: usize = 3;

/// Saves a consistent copy of the database at `target` (including anything
/// still in its WAL) as `inventory.db.replaced-<timestamp>.bak`. Only the
/// newest [`KEEP_REPLACED`] of these backups are kept.
fn backup_current(target: &Path) -> Result<PathBuf, ImportError> {
    let stamp = chrono::Local::now().format("%Y%m%dT%H%M%S").to_string();
    let backup = unique_backup_path(target, &stamp);
    {
        let conn = Connection::open(target)?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])?;
    }
    if let Some(dir) = target.parent() {
        prune_replaced(dir, KEEP_REPLACED)?;
    }
    Ok(backup)
}

/// Removes the database file and its WAL side files.
fn remove_database(target: &Path) -> Result<(), ImportError> {
    std::fs::remove_file(target)?;
    for suffix in ["-wal", "-shm"] {
        let side = PathBuf::from(format!("{}{suffix}", target.display()));
        if side.exists() {
            std::fs::remove_file(side)?;
        }
    }
    Ok(())
}

/// `inventory.db.replaced-<stamp>.bak`, with `-2`, `-3`, … appended when a
/// backup of that second already exists. Names sort chronologically.
fn unique_backup_path(target: &Path, stamp: &str) -> PathBuf {
    let name = |n: u32| {
        let counter = if n == 1 {
            String::new()
        } else {
            format!("-{n}")
        };
        target.with_file_name(format!(
            "{}.replaced-{stamp}{counter}.bak",
            InventoryDb::FILE_NAME
        ))
    };
    (1..).map(name).find(|p| !p.exists()).expect("a free name")
}

/// Deletes all but the `keep` newest replaced-database backups in `dir`.
fn prune_replaced(dir: &Path, keep: usize) -> std::io::Result<()> {
    let prefix = format!("{}.replaced-", InventoryDb::FILE_NAME);
    let mut backups: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".bak"))
        })
        .collect();
    backups.sort_by_key(|p| backup_order_key(p));
    let excess = backups.len().saturating_sub(keep);
    for old in &backups[..excess] {
        std::fs::remove_file(old)?;
        log::info!("Removed old backup {}", old.display());
    }
    Ok(())
}

/// Sort key: the timestamp, then the same-second counter as a number (so
/// `-10` sorts after `-9`).
fn backup_order_key(path: &Path) -> (String, u32) {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let middle = name
        .trim_start_matches(&format!("{}.replaced-", InventoryDb::FILE_NAME))
        .trim_end_matches(".bak");
    match middle.split_once('-') {
        Some((stamp, n)) => (stamp.to_string(), n.parse().unwrap_or(0)),
        None => (middle.to_string(), 1),
    }
}

/// A file removed on drop unless [`TempFile::disarm`] was called. Any stale
/// file of the same name (an interrupted earlier import) is removed first.
struct TempFile {
    path: PathBuf,
    armed: bool,
}

impl TempFile {
    fn new(path: PathBuf) -> std::io::Result<Self> {
        remove_with_journal(&path)?;
        Ok(Self { path, armed: true })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if self.armed {
            if let Err(e) = remove_with_journal(&self.path) {
                log::warn!("Could not remove {}: {e}", self.path.display());
            }
        }
    }
}

fn remove_with_journal(path: &Path) -> std::io::Result<()> {
    for suffix in ["", "-journal", "-wal", "-shm"] {
        let file = PathBuf::from(format!("{}{suffix}", path.display()));
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
