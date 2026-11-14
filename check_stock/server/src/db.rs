//! Database access for the server: one writer, several readers.
//!
//! SQLite allows one writer at a time; in WAL mode readers never wait for it.
//! So the server keeps a single writer connection behind a mutex and a small
//! pool of read-only connections. All database work runs on Tokio's blocking
//! threads, never on the async workers.
//!
//! The writer is opened through [`InventoryDb::open`], so core's schema
//! migrations and data repairs run exactly as in the desktop app; the
//! server's own tables are migrated right after ([`crate::migrations`]).

use crate::migrations::{self, MigrationError};
use check_stock_core::inventory_db::InventoryDb;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::Semaphore;

/// How long a connection waits for a lock before giving up.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Shared handle to the server's database; cheap to clone.
#[derive(Clone)]
pub struct Db {
    inner: Arc<Inner>,
}

struct Inner {
    path: PathBuf,
    writer: Mutex<Connection>,
    readers: Mutex<Vec<Connection>>,
    read_slots: Arc<Semaphore>,
}

#[derive(Debug)]
pub enum DbError {
    Sqlite(rusqlite::Error),
    Migration(MigrationError),
    /// WAL could not be enabled (e.g. a file system without shared memory).
    NoWal(String),
    /// The blocking task panicked or was cancelled.
    Task(String),
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::Sqlite(e) => write!(f, "database error: {e}"),
            DbError::Migration(e) => write!(f, "{e}"),
            DbError::NoWal(mode) => write!(
                f,
                "could not switch the database to WAL mode (journal mode is {mode:?})"
            ),
            DbError::Task(e) => write!(f, "database task failed: {e}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::Sqlite(e)
    }
}

impl From<MigrationError> for DbError {
    fn from(e: MigrationError) -> Self {
        DbError::Migration(e)
    }
}

impl Db {
    /// Opens the writer (core schema + repair, WAL, server migrations) and
    /// `read_connections` read-only connections.
    pub fn open(db: &InventoryDb, read_connections: usize) -> Result<Self, DbError> {
        let mut writer = db.open()?;
        writer.busy_timeout(BUSY_TIMEOUT)?;
        let mode: String = writer.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(DbError::NoWal(mode));
        }
        migrations::run(&mut writer)?;

        let path = db.path().to_path_buf();
        let readers = (0..read_connections)
            .map(|_| open_reader(&path))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            inner: Arc::new(Inner {
                path,
                writer: Mutex::new(writer),
                readers: Mutex::new(readers),
                read_slots: Arc::new(Semaphore::new(read_connections.max(1))),
            }),
        })
    }

    /// Path of the database file.
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Runs `f` on a read-only connection. At most `read_connections` reads
    /// run at once; further ones wait for a free connection.
    pub async fn read<T, F>(&self, f: F) -> Result<T, DbError>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let permit = self
            .inner
            .read_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| DbError::Task(e.to_string()))?;
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let pooled = lock(&inner.readers).pop();
            // A connection lost to a panic is replaced here.
            let conn = match pooled {
                Some(conn) => conn,
                None => open_reader(&inner.path)?,
            };
            let result = f(&conn);
            lock(&inner.readers).push(conn);
            result.map_err(DbError::Sqlite)
        })
        .await
        .map_err(|e| DbError::Task(e.to_string()))?
    }

    /// Runs `f` on the single writer connection. `f` should do its writes in
    /// one transaction (`conn.transaction()`), so a failure or a shutdown
    /// never leaves half a change behind.
    pub async fn write<T, F>(&self, f: F) -> Result<T, DbError>
    where
        F: FnOnce(&mut Connection) -> rusqlite::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            // A panic inside `f` drops its transaction (= rollback), so the
            // connection is still consistent and the poison can be ignored.
            let mut conn = lock(&inner.writer);
            f(&mut conn).map_err(DbError::Sqlite)
        })
        .await
        .map_err(|e| DbError::Task(e.to_string()))?
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A connection that cannot write, enforced twice: SQLite's read-only open
/// flag and `query_only`.
fn open_reader(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.pragma_update(None, "query_only", true)?;
    Ok(conn)
}

#[cfg(test)]
#[path = "db_tests.rs"]
mod tests;
