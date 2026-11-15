//! Inventory import on the server (step 4a): upload → preview → confirm.
//!
//! Stricter than the desktop app (owner's decision, 2026-11-15):
//! - Every import is previewed and confirmed; the desktop applies an
//!   unblocked one right away.
//! - The confirmation uploads the same file again. Meanwhile only its
//!   SHA-256, the category, the inventory version it was previewed against
//!   and the preview numbers are kept (`srv_pending_imports`, one per user,
//!   valid [`PENDING_MINUTES`]).
//! - Confirming re-runs the preview on the writer and refuses when another
//!   change of the category came in between (version) or the numbers moved.
//! - An import core's guard blocks (more than half the stock counted as
//!   sold) needs the number of copies typed in, and is logged as
//!   `inventory.import_forced`.
//! - More than 3× the usual sales is a warning only
//!   ([`SyncPreview::unusual_sales`]).
//! - A backup before every import (`inventory.db.import-<time>.bak`, newest
//!   [`KEEP_IMPORT_BACKUPS`] kept); without a backup there is no import.
//! - The sync, the version row, the file kept for bin analysis and the audit
//!   entry commit in one transaction.

use crate::audit::{self, action, Actor};
use crate::import::{prune_backups, unique_backup_path};
use crate::migrations::{format_utc, parse_utc};
use check_stock_core::category::detect_category;
use check_stock_core::inventory_db::{
    preview_sync_conn, sales_pace_conn, sync_inventory_in_tx, SalesPace, SyncPreview, SyncStats,
    UnusualSales,
};
use check_stock_core::io::{parse_csv, CsvReadError};
use check_stock_core::models::Card;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// Largest inventory file (a real Magic export: 4.7 MB for 25k rows).
pub const MAX_IMPORT_BYTES: usize = 16 * 1024 * 1024;
/// Most rows read from a file.
pub const MAX_IMPORT_ROWS: usize = 200_000;
/// Largest quantity of one row.
pub const MAX_QUANTITY: i64 = 1_000_000;
/// Largest price of one row (euros).
pub const MAX_PRICE: f64 = 1_000_000.0;
/// How long a preview can be confirmed.
pub const PENDING_MINUTES: i64 = 30;
/// Backups taken before imports that are kept.
pub const KEEP_IMPORT_BACKUPS: usize = 10;
/// Kind in the backup file names (`inventory.db.import-<time>.bak`).
pub const BACKUP_KIND: &str = "import";
/// Longest file name kept.
const MAX_FILE_NAME_CHARS: usize = 200;
/// Longest category name accepted (`StarWarsUnlimited` has 17).
const MAX_CATEGORY_CHARS: usize = 40;

/// A read and checked inventory file.
#[derive(Debug, Clone)]
pub struct ImportFile {
    /// The file's name without folders (decides the category).
    pub file_name: String,
    pub category: String,
    /// Rows with stock (rows with quantity 0 or without price are skipped).
    pub cards: Vec<Card>,
    /// SHA-256 of the file, hex.
    pub sha256: String,
    pub csv: String,
}

impl ImportFile {
    /// Copies in the file.
    pub fn copies(&self) -> i64 {
        self.cards.iter().map(quantity).sum()
    }
}

fn quantity(card: &Card) -> i64 {
    card.quantity.trim().parse().unwrap_or(0)
}

/// Why a file cannot be imported.
#[derive(Debug, Clone, PartialEq)]
pub enum FileError {
    TooLarge,
    NotAnExport {
        line: Option<u64>,
    },
    TooManyRows {
        max: usize,
    },
    BadFileName,
    BadCategory,
    /// Quantity or price out of range (the card's name, shortened).
    BadValue {
        card: String,
    },
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileError::TooLarge => write!(
                f,
                "The file is too large (at most {} MB)",
                MAX_IMPORT_BYTES / (1024 * 1024)
            ),
            FileError::NotAnExport { line } => write!(
                f,
                "This is not a Cardmarket inventory export{}: it needs the export's columns \
                 (cardmarketId, quantity, name, condition, language, price, …) and must be UTF-8 text",
                line.map(|l| format!(" (line {l} cannot be read)"))
                    .unwrap_or_default()
            ),
            FileError::TooManyRows { max } => write!(f, "The file has more than {max} rows"),
            FileError::BadFileName => write!(f, "The file name is missing or too long"),
            FileError::BadCategory => write!(
                f,
                "The product category in the file name is not one this app knows; \
                 keep the name Cardmarket gave the file (inventory-report-…csv)"
            ),
            FileError::BadValue { card } => write!(
                f,
                "\"{card}\" has a quantity that is not a whole number from 1 to 1,000,000, \
                 or a price that is not a number from 0 to 1,000,000"
            ),
        }
    }
}

/// The file's name without any folder part, checked.
fn clean_file_name(name: &str) -> Result<String, FileError> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default().trim();
    if base.is_empty()
        || base.chars().count() > MAX_FILE_NAME_CHARS
        || base.chars().any(char::is_control)
    {
        return Err(FileError::BadFileName);
    }
    Ok(base.to_string())
}

/// At most 60 characters of `text` (for messages).
fn short(text: &str) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(60).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

/// Reads and checks an uploaded export (blocking: up to 16 MB of CSV).
pub fn read_file(file_name: &str, csv: String) -> Result<ImportFile, FileError> {
    if csv.len() > MAX_IMPORT_BYTES {
        return Err(FileError::TooLarge);
    }
    let file_name = clean_file_name(file_name)?;
    let cards = parse_csv(csv.as_bytes(), MAX_IMPORT_ROWS).map_err(|e| match e {
        CsvReadError::TooManyRows { max } => FileError::TooManyRows { max },
        CsvReadError::Csv(_) => FileError::NotAnExport { line: e.line() },
    })?;
    let category = detect_category(&file_name, &cards);
    if category.is_empty()
        || category.len() > MAX_CATEGORY_CHARS
        || !category.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Err(FileError::BadCategory);
    }
    if let Some(card) = cards.iter().find(|card| {
        let qty = card.quantity.trim().parse::<i64>();
        let price = card.price.trim().replace(',', ".").parse::<f64>();
        !matches!(qty, Ok(q) if (1..=MAX_QUANTITY).contains(&q))
            || !matches!(price, Ok(p) if p.is_finite() && (0.0..=MAX_PRICE).contains(&p))
    }) {
        return Err(FileError::BadValue {
            card: short(&card.name),
        });
    }
    let sha256 = hex(&Sha256::digest(csv.as_bytes()));
    Ok(ImportFile {
        file_name,
        category,
        cards,
        sha256,
        csv,
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What an import changes, as shown in the preview and kept with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ImportNumbers {
    /// Listings (variants) not in stock before.
    pub new_variants: i64,
    pub updated_variants: i64,
    /// Listings in stock now that are missing from the file.
    pub zeroed_variants: i64,
    /// Copies that will count as sold.
    pub copies_sold: i64,
    /// Copies gone because they went into a bulk lot (not counted as sold).
    pub bulk_removed: i64,
    pub price_changes: i64,
    pub copies_before: i64,
    pub copies_after: i64,
}

impl From<&SyncPreview> for ImportNumbers {
    fn from(p: &SyncPreview) -> Self {
        let n = |v: usize| i64::try_from(v).unwrap_or(i64::MAX);
        Self {
            new_variants: n(p.new_variants),
            updated_variants: n(p.updated_variants),
            zeroed_variants: n(p.zeroed_variants),
            copies_sold: p.copies_sold,
            bulk_removed: p.bulk_removed,
            price_changes: n(p.price_changes),
            copies_before: p.copies_before,
            copies_after: p.copies_after,
        }
    }
}

/// The checks of a file against the current data.
#[derive(Debug, Clone, PartialEq)]
pub struct Checked {
    pub numbers: ImportNumbers,
    /// Core's guard: more than half the stock would count as sold.
    pub blocked: bool,
    pub pace: Option<SalesPace>,
    pub unusual: Option<UnusualSales>,
    /// Nothing of this category in the data yet.
    pub new_category: bool,
    /// The category's inventory version the check was made against.
    pub base_version: i64,
}

/// Previews `file` against the current data (reads only).
pub fn check(conn: &Connection, file: &ImportFile, today: &str) -> rusqlite::Result<Checked> {
    let preview = preview_sync_conn(conn, &file.cards, &file.category)?;
    let pace = sales_pace_conn(conn, &file.category)?;
    let unusual = pace.as_ref().and_then(|p| preview.unusual_sales(p, today));
    let new_category = conn
        .query_row(
            "SELECT 1 FROM inventory_cards WHERE category = ?1 LIMIT 1",
            [&file.category],
            |_| Ok(()),
        )
        .optional()?
        .is_none();
    Ok(Checked {
        numbers: (&preview).into(),
        blocked: preview.is_suspicious(),
        pace,
        unusual,
        new_category,
        base_version: category_version(conn, &file.category)?,
    })
}

/// Newest version of `category` (0 before the first).
pub fn category_version(conn: &Connection, category: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COALESCE(MAX(id), 0) FROM srv_inventory_versions WHERE category = ?1",
        [category],
        |r| r.get(0),
    )
}

/// Keeps the preview of `user_id` (replacing an older one) and removes
/// expired previews of everyone. Returns until when it can be confirmed.
pub fn save_pending(
    conn: &Connection,
    user_id: i64,
    file: &ImportFile,
    checked: &Checked,
    now: DateTime<Utc>,
) -> rusqlite::Result<DateTime<Utc>> {
    let expires = now + chrono::Duration::minutes(PENDING_MINUTES);
    conn.execute(
        "DELETE FROM srv_pending_imports WHERE expires_at <= ?1",
        [format_utc(now)],
    )?;
    let preview = serde_json::to_string(&checked.numbers).expect("numbers serialize");
    conn.execute(
        "INSERT OR REPLACE INTO srv_pending_imports
             (user_id, category, file_sha256, base_version, preview, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            user_id,
            file.category,
            file.sha256,
            checked.base_version,
            preview,
            format_utc(now),
            format_utc(expires)
        ],
    )?;
    Ok(expires)
}

/// Drops the preview of `user_id`; `false` when there was none.
pub fn discard_pending(conn: &Connection, user_id: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "DELETE FROM srv_pending_imports WHERE user_id = ?1",
        [user_id],
    )? > 0)
}

struct Pending {
    category: String,
    sha256: String,
    base_version: i64,
    numbers: Option<ImportNumbers>,
    expires_at: Option<DateTime<Utc>>,
}

fn load_pending(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<Pending>> {
    conn.query_row(
        "SELECT category, file_sha256, base_version, preview, expires_at
         FROM srv_pending_imports WHERE user_id = ?1",
        [user_id],
        |r| {
            Ok(Pending {
                category: r.get(0)?,
                sha256: r.get(1)?,
                base_version: r.get(2)?,
                numbers: serde_json::from_str(&r.get::<_, String>(3)?).ok(),
                expires_at: parse_utc(&r.get::<_, String>(4)?),
            })
        },
    )
    .optional()
}

/// Why a confirmation was refused (nothing was written, except that an
/// expired preview is removed).
#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmError {
    NoPreview,
    Expired,
    /// Another file (or category) than the one previewed.
    OtherFile,
    /// The category changed since the preview (another import, a restore).
    Outdated,
    /// The preview numbers are different now.
    Changed,
    /// The guard blocked it: the copies counted as sold must be typed in.
    NeedsConfirmation {
        copies_sold: i64,
    },
    WrongConfirmation {
        copies_sold: i64,
    },
    BackupFailed,
}

/// Who imports.
#[derive(Debug, Clone, Copy)]
pub struct Importer<'a> {
    pub id: i64,
    pub username: &'a str,
    pub display_name: &'a str,
}

/// A confirmed import.
#[derive(Debug, Clone, PartialEq)]
pub struct Confirmed {
    pub version: i64,
    pub numbers: ImportNumbers,
    pub forced: bool,
    pub upserted: usize,
    pub zeroed: usize,
    pub bulk_removed: i64,
    /// File name of the backup taken before (no folder).
    pub backup: String,
}

/// `"1,234"`, `"1.234"`, `" 1234 "` → 1234; anything else `None`.
pub fn parse_typed_count(text: &str) -> Option<i64> {
    let digits: String = text
        .chars()
        .filter(|c| !matches!(c, ',' | '.' | '\'' | ' ' | '\u{a0}' | '\u{202f}'))
        .collect();
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Confirms the previewed import of `file` for `user`: re-checks it, takes
/// the backup and imports in one transaction (writer connection; nothing
/// else writes meanwhile). Domain refusals are the inner `Err`.
pub fn confirm(
    conn: &mut Connection,
    db_path: &Path,
    user: Importer<'_>,
    file: &ImportFile,
    typed_count: Option<&str>,
    now: DateTime<Utc>,
    today: &str,
) -> rusqlite::Result<Result<Confirmed, ConfirmError>> {
    let Some(pending) = load_pending(conn, user.id)? else {
        return Ok(Err(ConfirmError::NoPreview));
    };
    if pending.expires_at.is_none_or(|at| at <= now) {
        discard_pending(conn, user.id)?;
        return Ok(Err(ConfirmError::Expired));
    }
    if pending.sha256 != file.sha256 || pending.category != file.category {
        return Ok(Err(ConfirmError::OtherFile));
    }
    if category_version(conn, &file.category)? != pending.base_version {
        return Ok(Err(ConfirmError::Outdated));
    }
    let checked = check(conn, file, today)?;
    if pending.numbers.as_ref() != Some(&checked.numbers) {
        return Ok(Err(ConfirmError::Changed));
    }
    let copies_sold = checked.numbers.copies_sold;
    if checked.blocked {
        match typed_count {
            None => return Ok(Err(ConfirmError::NeedsConfirmation { copies_sold })),
            Some(text) if parse_typed_count(text) != Some(copies_sold) => {
                return Ok(Err(ConfirmError::WrongConfirmation { copies_sold }))
            }
            Some(_) => {}
        }
    }
    let backup = match backup_before_import(conn, db_path) {
        Ok(path) => path,
        Err(e) => {
            log::error!("backup before the inventory import failed, nothing imported: {e}");
            return Ok(Err(ConfirmError::BackupFailed));
        }
    };
    let backup_name = backup
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();

    let tx = conn.transaction()?;
    let SyncStats {
        upserted,
        zeroed,
        bulk_removed,
    } = sync_inventory_in_tx(&tx, &file.cards, &file.category, today)?;
    let kind = if checked.blocked {
        "forced_import"
    } else {
        "import"
    };
    tx.execute(
        "INSERT INTO srv_inventory_versions
             (category, created_at, kind, user_id, user_name, file_sha256, file_rows,
              copies_sold, new_variants, zeroed_variants)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            file.category,
            format_utc(now),
            kind,
            user.id,
            user.display_name,
            file.sha256,
            i64::try_from(file.cards.len()).unwrap_or(i64::MAX),
            copies_sold,
            checked.numbers.new_variants,
            checked.numbers.zeroed_variants
        ],
    )?;
    let version = tx.last_insert_rowid();
    tx.execute(
        "INSERT OR REPLACE INTO srv_inventory_files (category, version_id, file_name, csv)
         VALUES (?1, ?2, ?3, ?4)",
        params![file.category, version, file.file_name, file.csv.as_bytes()],
    )?;
    discard_pending(&tx, user.id)?;
    audit::record(
        &tx,
        now,
        Actor::User {
            id: user.id,
            username: user.username,
        },
        if checked.blocked {
            action::INVENTORY_IMPORT_FORCED
        } else {
            action::INVENTORY_IMPORTED
        },
        Some(&file.category),
        serde_json::json!({
            "version": version,
            "file": file.file_name,
            "sha256": file.sha256,
            "rows": file.cards.len(),
            "numbers": checked.numbers,
            "unusual_sales": checked.unusual.is_some(),
            "backup": backup_name,
        }),
    )?;
    tx.commit()?;
    Ok(Ok(Confirmed {
        version,
        numbers: checked.numbers,
        forced: checked.blocked,
        upserted,
        zeroed,
        bulk_removed,
        backup: backup_name,
    }))
}

/// A consistent copy of the database next to it, before an import; keeps
/// the newest [`KEEP_IMPORT_BACKUPS`]. Runs outside any transaction.
pub fn backup_before_import(conn: &Connection, db_path: &Path) -> Result<PathBuf, String> {
    let stamp = chrono::Local::now().format("%Y%m%dT%H%M%S").to_string();
    let target = unique_backup_path(db_path, BACKUP_KIND, &stamp);
    conn.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])
        .map_err(|e| e.to_string())?;
    if let Some(dir) = db_path.parent() {
        if let Err(e) = prune_backups(dir, BACKUP_KIND, KEEP_IMPORT_BACKUPS) {
            log::warn!("could not remove old import backups: {e}");
        }
    }
    Ok(target)
}

/// One change of the inventory data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InventoryVersion {
    pub id: i64,
    pub category: String,
    /// RFC 3339, UTC.
    pub created_at: String,
    /// `import`, `forced_import`, `restore` or `desktop_copy`.
    pub kind: String,
    pub user_name: String,
    pub file_rows: i64,
    pub copies_sold: i64,
    pub new_variants: i64,
    pub zeroed_variants: i64,
}

/// The newest `limit` versions, newest first.
pub fn recent_versions(conn: &Connection, limit: u32) -> rusqlite::Result<Vec<InventoryVersion>> {
    let mut stmt = conn.prepare(
        "SELECT id, category, created_at, kind, user_name, file_rows, copies_sold,
                new_variants, zeroed_variants
         FROM srv_inventory_versions ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |r| {
        Ok(InventoryVersion {
            id: r.get(0)?,
            category: r.get(1)?,
            created_at: r.get(2)?,
            kind: r.get(3)?,
            user_name: r.get(4)?,
            file_rows: r.get(5)?,
            copies_sold: r.get(6)?,
            new_variants: r.get(7)?,
            zeroed_variants: r.get(8)?,
        })
    })?;
    rows.collect()
}

/// After the inventory tables were replaced from a file (`restore-inventory`,
/// `import-desktop --refresh`): a version row per category, no pending
/// previews, and no kept import files unless `keep_files` (a restore brings
/// its own back).
pub fn record_replaced_data(
    conn: &Connection,
    kind: &str,
    now: DateTime<Utc>,
    keep_files: bool,
) -> rusqlite::Result<Vec<String>> {
    let categories: Vec<String> = conn
        .prepare("SELECT DISTINCT category FROM inventory_cards ORDER BY category")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for category in &categories {
        conn.execute(
            "INSERT INTO srv_inventory_versions (category, created_at, kind, user_name)
             VALUES (?1, ?2, ?3, 'command line')",
            params![category, format_utc(now), kind],
        )?;
    }
    conn.execute("DELETE FROM srv_pending_imports", [])?;
    if !keep_files {
        conn.execute("DELETE FROM srv_inventory_files", [])?;
    }
    Ok(categories)
}

/// Newest version of any category (0 before the first) and when it was made.
pub fn latest_version(conn: &Connection) -> rusqlite::Result<(i64, Option<String>)> {
    conn.query_row(
        "SELECT id, created_at FROM srv_inventory_versions ORDER BY id DESC LIMIT 1",
        [],
        |r| Ok((r.get(0)?, Some(r.get(1)?))),
    )
    .optional()
    .map(|v| v.unwrap_or((0, None)))
}

#[cfg(test)]
#[path = "inventory_import_tests.rs"]
mod tests;
