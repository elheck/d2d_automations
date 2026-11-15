//! Per-user settings of the screens, kept on the server (`srv_user_settings`),
//! so they follow the user from device to device.
//!
//! Only the settings listed in [`Setting`] exist; each is stored as one JSON
//! value of at most [`MAX_VALUE_BYTES`]. The screens validate what they read
//! back (a value written by an older version may no longer fit) and fall back
//! to their defaults.

use crate::migrations::format_utc;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};

/// Largest stored value.
pub const MAX_VALUE_BYTES: usize = 16 * 1024;

/// The settings there are. The keys are stored in the database: never rename
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// The buy helper's split rules and rates.
    BuyHelper,
    /// The precon EV's price columns, cutoff, foil check and sorting.
    PreconEv,
}

impl Setting {
    pub fn key(self) -> &'static str {
        match self {
            Setting::BuyHelper => "buy_helper",
            Setting::PreconEv => "precon_ev",
        }
    }
}

/// A value checked for size, ready to store.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingValue {
    value: serde_json::Value,
    text: String,
}

/// The value is larger than [`MAX_VALUE_BYTES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLarge;

impl std::fmt::Display for TooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "setting larger than {MAX_VALUE_BYTES} bytes")
    }
}

impl std::error::Error for TooLarge {}

impl SettingValue {
    pub fn new(value: serde_json::Value) -> Result<Self, TooLarge> {
        let text = value.to_string();
        if text.len() > MAX_VALUE_BYTES {
            return Err(TooLarge);
        }
        Ok(Self { value, text })
    }

    pub fn value(&self) -> &serde_json::Value {
        &self.value
    }
}

/// The stored value of `setting` for `user_id`, if any.
pub fn get(
    conn: &Connection,
    user_id: i64,
    setting: Setting,
) -> rusqlite::Result<Option<serde_json::Value>> {
    let text: Option<String> = conn
        .query_row(
            "SELECT value FROM srv_user_settings WHERE user_id = ?1 AND key = ?2",
            params![user_id, setting.key()],
            |row| row.get(0),
        )
        .optional()?;
    Ok(text.and_then(|t| serde_json::from_str(&t).ok()))
}

/// Stores `value`. Returns `false` (and writes nothing) when it equals the
/// stored value.
pub fn put(
    conn: &Connection,
    user_id: i64,
    setting: Setting,
    value: &SettingValue,
    now: DateTime<Utc>,
) -> rusqlite::Result<bool> {
    if get(conn, user_id, setting)?.as_ref() == Some(&value.value) {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO srv_user_settings (user_id, key, value, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (user_id, key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![user_id, setting.key(), value.text, format_utc(now)],
    )?;
    Ok(true)
}

#[cfg(test)]
#[path = "user_settings_tests.rs"]
mod tests;
