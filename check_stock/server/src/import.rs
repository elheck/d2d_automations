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

use crate::config::inventory_db_in;
use crate::data_dir::{DataDirLock, LockError};
use crate::db::BUSY_TIMEOUT;
use crate::migrations::{self, MigrationError};
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
                "{} already exists; pass --replace to replace it (it is kept as a backup)",
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

/// Copies the desktop database at `source` into `data_dir`.
pub fn import_desktop(
    source: &Path,
    data_dir: &Path,
    replace: bool,
) -> Result<ImportReport, ImportError> {
    if !source.is_file() {
        return Err(ImportError::SourceMissing(source.to_path_buf()));
    }
    let _lock = DataDirLock::acquire(data_dir)?;
    let target = inventory_db_in(data_dir).path().to_path_buf();
    let source = source.canonicalize()?;
    if target.exists() {
        if target.canonicalize()? == source {
            return Err(ImportError::SameFile(source));
        }
        if !replace {
            return Err(ImportError::TargetExists(target));
        }
    }

    let tmp = TempFile::new(data_dir.join(format!("{}.importing", InventoryDb::FILE_NAME)))?;
    copy_snapshot(&source, tmp.path())?;
    let problems = integrity_problems(tmp.path())?;
    if !problems.is_empty() {
        return Err(ImportError::Corrupt(problems));
    }

    let taken_at = migrations::now_utc();
    let inventory_rows = {
        // Core's schema migrations and data repairs, then the server tables.
        let mut conn = InventoryDb::at(tmp.path()).open()?;
        migrations::run(&mut conn)?;
        migrations::set_meta(&conn, META_COPY_SOURCE, &source.display().to_string())?;
        migrations::set_meta(&conn, META_COPY_TAKEN_AT, &taken_at)?;
        conn.query_row("SELECT COUNT(*) FROM inventory_cards", [], |row| row.get(0))?
    };

    let previous_kept_as = if target.exists() {
        Some(keep_previous(&target)?)
    } else {
        None
    };
    std::fs::rename(tmp.path(), &target)?;
    tmp.disarm();
    Ok(ImportReport {
        target,
        inventory_rows,
        taken_at,
        previous_kept_as,
    })
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
/// still in its WAL) as `inventory.db.replaced-<timestamp>.bak`, then removes
/// the database and its WAL side files. Only the newest [`KEEP_REPLACED`]
/// of these backups are kept.
fn keep_previous(target: &Path) -> Result<PathBuf, ImportError> {
    let stamp = chrono::Local::now().format("%Y%m%dT%H%M%S").to_string();
    let backup = unique_backup_path(target, &stamp);
    {
        let conn = Connection::open(target)?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])?;
    }
    std::fs::remove_file(target)?;
    for suffix in ["-wal", "-shm"] {
        let side = PathBuf::from(format!("{}{suffix}", target.display()));
        if side.exists() {
            std::fs::remove_file(side)?;
        }
    }
    if let Some(dir) = target.parent() {
        prune_replaced(dir, KEEP_REPLACED)?;
    }
    Ok(backup)
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
