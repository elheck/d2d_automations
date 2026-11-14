//! The server's data folder and its lock.
//!
//! Only one process may work on a data folder at a time: two servers on the
//! same database, or an import replacing the database under a running server,
//! would lose data. `serve` and `import-desktop` both hold [`DataDirLock`]
//! while they run. The lock is an OS file lock, so it is released when the
//! process ends, even after a crash.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

/// Name of the lock file inside the data folder.
pub const LOCK_FILE: &str = ".lock";

/// Exclusive lock on a data folder; released on drop.
#[derive(Debug)]
pub struct DataDirLock {
    _file: File,
    path: PathBuf,
}

#[derive(Debug)]
pub enum LockError {
    /// Another process (a running server or import) holds the folder.
    InUse(PathBuf),
    Io(PathBuf, std::io::Error),
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockError::InUse(dir) => write!(
                f,
                "the data folder {} is in use by another check_stock_server process (stop it first)",
                dir.display()
            ),
            LockError::Io(dir, e) => {
                write!(f, "cannot lock the data folder {}: {e}", dir.display())
            }
        }
    }
}

impl std::error::Error for LockError {}

impl DataDirLock {
    /// Creates the folder if needed and takes the lock without waiting.
    pub fn acquire(dir: &Path) -> Result<Self, LockError> {
        std::fs::create_dir_all(dir).map_err(|e| LockError::Io(dir.to_path_buf(), e))?;
        let path = dir.join(LOCK_FILE);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| LockError::Io(dir.to_path_buf(), e))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file, path }),
            Err(TryLockError::WouldBlock) => Err(LockError::InUse(dir.to_path_buf())),
            Err(TryLockError::Error(e)) => Err(LockError::Io(dir.to_path_buf(), e)),
        }
    }

    /// Path of the lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
#[path = "data_dir_tests.rs"]
mod tests;
