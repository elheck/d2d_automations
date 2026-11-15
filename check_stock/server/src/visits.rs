//! Visit days per user (`srv_user_visits`), the baseline of the start page's
//! "since your last visit" digest. Same rule as the desktop app, per user:
//! the first visit on a new day moves the previous visit day to the
//! baseline; more visits that day keep it.

use rusqlite::{params, Connection, OptionalExtension};

/// Records a visit of `user_id` on `today` (`YYYY-MM-DD`) and returns the
/// day the digest starts from (`None` on the first visit ever).
pub fn record_visit(
    conn: &Connection,
    user_id: i64,
    today: &str,
) -> rusqlite::Result<Option<String>> {
    let stored: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT last_visit, prev_visit FROM srv_user_visits WHERE user_id = ?1",
            [user_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match stored {
        None => {
            conn.execute(
                "INSERT INTO srv_user_visits (user_id, last_visit) VALUES (?1, ?2)",
                params![user_id, today],
            )?;
            Ok(None)
        }
        Some((last, _)) if last != today => {
            conn.execute(
                "UPDATE srv_user_visits SET prev_visit = ?2, last_visit = ?3 WHERE user_id = ?1",
                params![user_id, last, today],
            )?;
            Ok(Some(last))
        }
        Some((_, prev)) => Ok(prev),
    }
}

#[cfg(test)]
#[path = "visits_tests.rs"]
mod tests;
