//! Admin commands that run on the server machine: `create-admin` and
//! `reset-password`. They hold the data-folder lock (so the server must be
//! stopped) and log to the audit log as `cli`.
//!
//! Passwords are never command-line arguments (those end up in the shell
//! history and the process list): they are read without echo from the
//! terminal, or as one line from standard input for scripts.

use crate::api::in_transaction;
use crate::audit::{self, action, Actor};
use crate::config::inventory_db_in;
use crate::data_dir::{DataDirLock, LockError};
use crate::db::{open_writer, DbError};
use crate::password::{self, PasswordError};
use crate::permissions::PermissionSet;
use crate::sessions;
use crate::users::{self, StoreError};
use std::io::{BufRead, IsTerminal};
use std::path::Path;

#[derive(Debug)]
pub enum AdminError {
    Lock(LockError),
    Db(DbError),
    Store(StoreError),
    Password(PasswordError),
    /// Invalid input; the message is for the user.
    Invalid(String),
    Io(std::io::Error),
}

impl std::fmt::Display for AdminError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdminError::Lock(e) => write!(f, "{e}"),
            AdminError::Db(e) => write!(f, "{e}"),
            AdminError::Store(e) => write!(f, "{e}"),
            AdminError::Password(e) => write!(f, "{e}"),
            AdminError::Invalid(message) => write!(f, "{message}"),
            AdminError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for AdminError {}

impl From<LockError> for AdminError {
    fn from(e: LockError) -> Self {
        AdminError::Lock(e)
    }
}

impl From<DbError> for AdminError {
    fn from(e: DbError) -> Self {
        AdminError::Db(e)
    }
}

impl From<StoreError> for AdminError {
    fn from(e: StoreError) -> Self {
        AdminError::Store(e)
    }
}

impl From<rusqlite::Error> for AdminError {
    fn from(e: rusqlite::Error) -> Self {
        AdminError::Db(DbError::Sqlite(e))
    }
}

impl From<std::io::Error> for AdminError {
    fn from(e: std::io::Error) -> Self {
        AdminError::Io(e)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CreateAdminOutcome {
    Created {
        username: String,
    },
    /// `--if-none` and the database already has users.
    SkippedUsersExist,
}

/// Creates a user with every permission. `read_password` is called only
/// when the account will really be created (not when `if_none` skips).
pub fn create_admin(
    data_dir: &Path,
    username: &str,
    display_name: Option<&str>,
    if_none: bool,
    read_password: impl FnOnce(&str) -> std::io::Result<String>,
) -> Result<CreateAdminOutcome, AdminError> {
    let username = users::normalize_username(username).map_err(AdminError::Invalid)?;
    let display_name = users::normalize_display_name(display_name.unwrap_or(&username))
        .map_err(AdminError::Invalid)?;
    let _lock = DataDirLock::acquire(data_dir)?;
    let mut conn = open_writer(&inventory_db_in(data_dir))?;
    if if_none && users::count(&conn)? > 0 {
        return Ok(CreateAdminOutcome::SkippedUsersExist);
    }
    if users::find_by_username(&conn, &username)?.is_some() {
        return Err(StoreError::UsernameTaken.into());
    }
    let pw = read_password(&username)?;
    password::check_policy(&pw, &username).map_err(AdminError::Invalid)?;
    let hash = password::hash(&pw).map_err(AdminError::Password)?;
    let now = chrono::Utc::now();
    let all = PermissionSet::all();
    in_transaction(&mut conn, |tx| {
        users::create(tx, &username, &display_name, &hash, &all, now)?;
        audit::record(
            tx,
            now,
            Actor::Cli,
            action::USER_CREATED,
            Some(&username),
            serde_json::json!({ "via": "create-admin", "permissions": all.keys() }),
        )?;
        Ok(())
    })??;
    Ok(CreateAdminOutcome::Created { username })
}

/// Sets a new password for `username`, unlocks the account and ends its
/// sessions. For when nobody can log in to change it.
pub fn reset_password(
    data_dir: &Path,
    username: &str,
    read_password: impl FnOnce(&str) -> std::io::Result<String>,
) -> Result<(), AdminError> {
    let username = users::normalize_username(username).map_err(AdminError::Invalid)?;
    let _lock = DataDirLock::acquire(data_dir)?;
    let mut conn = open_writer(&inventory_db_in(data_dir))?;
    let user = users::find_by_username(&conn, &username)?.ok_or(StoreError::NotFound)?;
    let pw = read_password(&username)?;
    password::check_policy(&pw, &username).map_err(AdminError::Invalid)?;
    let hash = password::hash(&pw).map_err(AdminError::Password)?;
    let now = chrono::Utc::now();
    in_transaction(&mut conn, |tx| {
        users::set_password(tx, user.id, &hash, now)?;
        sessions::revoke_all_for_user(tx, user.id)?;
        audit::record(
            tx,
            now,
            Actor::Cli,
            action::USER_PASSWORD_RESET,
            Some(&username),
            serde_json::json!({ "via": "reset-password" }),
        )?;
        Ok(())
    })??;
    Ok(())
}

/// Reads a new password: on a terminal twice without echo (they must match),
/// otherwise one line from standard input.
pub fn read_new_password(username: &str) -> std::io::Result<String> {
    if std::io::stdin().is_terminal() {
        let first = rpassword::prompt_password(format!("New password for {username}: "))?;
        let second = rpassword::prompt_password("Repeat it: ")?;
        if first != second {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the two passwords differ",
            ));
        }
        Ok(first)
    } else {
        read_password_line(&mut std::io::stdin().lock())
    }
}

/// One line (without the line break) from `reader`.
pub fn read_password_line(reader: &mut impl BufRead) -> std::io::Result<String> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let line = line.strip_suffix('\n').unwrap_or(&line);
    let line = line.strip_suffix('\r').unwrap_or(line);
    Ok(line.to_string())
}

#[cfg(test)]
#[path = "admin_tests.rs"]
mod tests;
