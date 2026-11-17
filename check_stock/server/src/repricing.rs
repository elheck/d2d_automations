//! Repricing (4d): the one published ruleset, each editor's unpublished
//! draft, each user's review of the current import, the kept exports, and
//! the stock prepared for [`check_stock_core::repricing`].
//!
//! - The ruleset has a revision; publishing checks it (no silent overwrite).
//!   Every ruleset is stored after `Ruleset::sanitize`.
//! - A review (tick overrides and hand prices) is per user and keyed by pile
//!   id, valid for one inventory version (import): rows of another version
//!   are ignored, counted as dropped, and deleted by the user's next write.
//! - The stock is the kept file of the last import minus pending discards
//!   ([`crate::stock`]); listings of a card variant with copies reserved for
//!   a bulk lot awaiting its import are **held back**: never ticked, never
//!   exported. Whether the Powertools price import also sets the stock is
//!   unknown (owner, 2026-11-16), so the export must not bring copies back.
//! - Preparing the stock (listing dates, sales, reservations) happens once
//!   per data version, market load and day; evaluations are cached by their
//!   inputs and run on blocking threads, at most [`EVAL_PERMITS`] at a time.

use crate::db::Db;
use crate::inventory_import::latest_version;
use crate::market::{MarketData, MarketService};
use crate::migrations::format_utc;
use crate::stock::{Source, StockError, StockService, CATEGORY};
use check_stock_core::api::inventory_sync::LatestPrice;
use check_stock_core::inventory_db::{
    card_variant_key, listing_dates_conn, pending_removals_conn, sales_by_variant_conn,
};
use check_stock_core::precon_loader::local_date;
use check_stock_core::repricing::{
    distinct_by_copies, evaluate, summarize, Evaluation, Listing, ListingFacts, Outcome, Proposal,
    Ruleset, Summary,
};
use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use ts_rs::TS;

/// Exports kept on the server (newest first).
pub const KEEP_EXPORTS: i64 = 20;
/// Largest ruleset accepted from a browser or a file (as core's file cap).
pub const MAX_RULESET_BYTES: usize = check_stock_core::repricing::MAX_RULESET_FILE_BYTES as usize;
/// Evaluations running at the same time (each checks every rule against
/// every listing).
pub const EVAL_PERMITS: usize = 2;
/// Evaluations kept for reuse (paging, sorting and ticking reuse one).
const EVAL_CACHE: usize = 8;
/// Rarities and languages offered before an import has any.
const DEFAULT_RARITIES: [&str; 4] = ["Common", "Uncommon", "Rare", "Mythic"];
const DEFAULT_LANGUAGES: [&str; 2] = ["English", "German"];

pub fn today() -> NaiveDate {
    local_date(Utc::now().timestamp())
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A stored ruleset that no longer reads (it was sanitized when written).
fn unreadable(e: String) -> rusqlite::Error {
    log::error!("a stored repricing ruleset no longer reads: {e}");
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
}

fn stored(json: &str) -> rusqlite::Result<Ruleset> {
    Ruleset::from_json(json).map_err(unreadable)
}

fn to_json(ruleset: &Ruleset) -> rusqlite::Result<String> {
    serde_json::to_string(ruleset).map_err(|e| unreadable(e.to_string()))
}

// ── The published ruleset ──────────────────────────────────────────────────

/// The ruleset everyone's review and every export uses.
#[derive(Debug, Clone, PartialEq)]
pub struct Published {
    /// 0 before the first publish (then the starter rules).
    pub revision: i64,
    pub ruleset: Ruleset,
    pub updated_at: Option<String>,
    pub updated_by: Option<String>,
}

pub fn published(conn: &Connection) -> rusqlite::Result<Published> {
    let row = conn
        .query_row(
            "SELECT revision, ruleset, updated_at, updated_by FROM srv_repricing_ruleset
             WHERE id = 1",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    Ok(match row {
        None => Published {
            revision: 0,
            ruleset: Ruleset::starter(),
            updated_at: None,
            updated_by: None,
        },
        Some((revision, json, at, by)) => Published {
            revision,
            ruleset: stored(&json)?,
            updated_at: Some(at),
            updated_by: Some(by),
        },
    })
}

/// Someone published another revision since the one the editor started from.
#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    pub revision: i64,
    pub updated_at: Option<String>,
    pub updated_by: Option<String>,
}

/// Publishes `ruleset` (sanitized) when `expected` is still the published
/// revision; returns the new revision.
pub fn publish_in_tx(
    conn: &Connection,
    ruleset: &Ruleset,
    expected: i64,
    now: DateTime<Utc>,
    by: &str,
) -> rusqlite::Result<Result<i64, Conflict>> {
    let current = published(conn)?;
    if current.revision != expected {
        return Ok(Err(Conflict {
            revision: current.revision,
            updated_at: current.updated_at,
            updated_by: current.updated_by,
        }));
    }
    let mut ruleset = ruleset.clone();
    ruleset.sanitize();
    let revision = current.revision + 1;
    conn.execute(
        "INSERT INTO srv_repricing_ruleset (id, revision, ruleset, updated_at, updated_by)
         VALUES (1, ?1, ?2, ?3, ?4)
         ON CONFLICT (id) DO UPDATE SET revision = excluded.revision,
             ruleset = excluded.ruleset, updated_at = excluded.updated_at,
             updated_by = excluded.updated_by",
        params![revision, to_json(&ruleset)?, format_utc(now), by],
    )?;
    Ok(Ok(revision))
}

// ── Drafts ─────────────────────────────────────────────────────────────────

/// An editor's unpublished ruleset.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    /// The published revision it started from.
    pub base_revision: i64,
    pub ruleset: Ruleset,
    pub updated_at: String,
}

pub fn draft(conn: &Connection, user: i64) -> rusqlite::Result<Option<Draft>> {
    let row = conn
        .query_row(
            "SELECT base_revision, ruleset, updated_at FROM srv_repricing_drafts
             WHERE user_id = ?1",
            [user],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    row.map(|(base_revision, json, updated_at)| {
        Ok(Draft {
            base_revision,
            ruleset: stored(&json)?,
            updated_at,
        })
    })
    .transpose()
}

/// Saves the draft (sanitized); returns what was stored.
pub fn save_draft(
    conn: &Connection,
    user: i64,
    base_revision: i64,
    ruleset: &Ruleset,
    now: DateTime<Utc>,
) -> rusqlite::Result<Draft> {
    let mut ruleset = ruleset.clone();
    ruleset.sanitize();
    let updated_at = format_utc(now);
    conn.execute(
        "INSERT INTO srv_repricing_drafts (user_id, base_revision, ruleset, updated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (user_id) DO UPDATE SET base_revision = excluded.base_revision,
             ruleset = excluded.ruleset, updated_at = excluded.updated_at",
        params![user, base_revision.max(0), to_json(&ruleset)?, updated_at],
    )?;
    Ok(Draft {
        base_revision: base_revision.max(0),
        ruleset,
        updated_at,
    })
}

pub fn delete_draft(conn: &Connection, user: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "DELETE FROM srv_repricing_drafts WHERE user_id = ?1",
        [user],
    )? > 0)
}

/// Parses a ruleset file. Errors name the position, never the content.
pub fn parse_ruleset_file(json: &str) -> Result<Ruleset, String> {
    if json.len() > MAX_RULESET_BYTES {
        return Err("The file is too large to be a repricing ruleset".to_string());
    }
    match serde_json::from_str::<Ruleset>(json) {
        Ok(mut ruleset) => {
            ruleset.sanitize();
            Ok(ruleset)
        }
        Err(e) if e.to_string().contains("missing field `rules`") => Err(
            "This file is not a repricing ruleset (old node-graph files cannot be opened)"
                .to_string(),
        ),
        Err(e) => Err(format!(
            "This file is not a valid repricing ruleset (line {}, column {})",
            e.line(),
            e.column()
        )),
    }
}

// ── Reviews ────────────────────────────────────────────────────────────────

/// A user's review of one import.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Review {
    /// Ticks set by hand (pile → ticked); others follow the proposal.
    pub ticks: HashMap<String, bool>,
    /// Prices typed by hand (pile → €).
    pub manual: HashMap<String, f64>,
    /// Ticks and hand prices made for an older import (ignored).
    pub dropped_ticks: usize,
    pub dropped_manual: usize,
}

pub fn review(conn: &Connection, user: i64, version: i64) -> rusqlite::Result<Review> {
    let mut stmt = conn.prepare(
        "SELECT version, pile, ticked, manual_cents FROM srv_repricing_review
         WHERE user_id = ?1",
    )?;
    let rows = stmt.query_map([user], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<bool>>(2)?,
            r.get::<_, Option<i64>>(3)?,
        ))
    })?;
    let mut out = Review::default();
    for row in rows {
        let (v, pile, ticked, cents) = row?;
        if v != version {
            out.dropped_ticks += usize::from(ticked.is_some());
            out.dropped_manual += usize::from(cents.is_some());
            continue;
        }
        if let Some(t) = ticked {
            out.ticks.insert(pile.clone(), t);
        }
        if let Some(c) = cents {
            out.manual.insert(pile, c as f64 / 100.0);
        }
    }
    Ok(out)
}

fn drop_outdated(conn: &Connection, user: i64, version: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM srv_repricing_review WHERE user_id = ?1 AND version != ?2",
        params![user, version],
    )?;
    Ok(())
}

fn drop_empty(conn: &Connection, user: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM srv_repricing_review
         WHERE user_id = ?1 AND ticked IS NULL AND manual_cents IS NULL",
        [user],
    )?;
    Ok(())
}

/// Sets (or with `None` removes) the tick override of `piles`.
pub fn set_ticks(
    conn: &Connection,
    user: i64,
    version: i64,
    piles: &[String],
    ticked: Option<bool>,
) -> rusqlite::Result<()> {
    drop_outdated(conn, user, version)?;
    let mut stmt = conn.prepare(
        "INSERT INTO srv_repricing_review (user_id, version, pile, ticked)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (user_id, pile) DO UPDATE SET ticked = excluded.ticked",
    )?;
    for pile in piles {
        stmt.execute(params![user, version, pile, ticked])?;
    }
    drop_empty(conn, user)
}

/// Sets (or with `None` removes) the hand price of `pile`, in cents.
pub fn set_manual(
    conn: &Connection,
    user: i64,
    version: i64,
    pile: &str,
    cents: Option<i64>,
) -> rusqlite::Result<()> {
    drop_outdated(conn, user, version)?;
    conn.execute(
        "INSERT INTO srv_repricing_review (user_id, version, pile, manual_cents)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (user_id, pile) DO UPDATE SET manual_cents = excluded.manual_cents",
        params![user, version, pile, cents],
    )?;
    drop_empty(conn, user)
}

/// Removes all ticks and hand prices of the user.
pub fn clear_review(conn: &Connection, user: i64) -> rusqlite::Result<usize> {
    conn.execute(
        "DELETE FROM srv_repricing_review WHERE user_id = ?1",
        [user],
    )
}

// ── Exports ────────────────────────────────────────────────────────────────

/// An export to keep.
pub struct NewExport<'a> {
    pub user_id: i64,
    pub username: &'a str,
    pub revision: i64,
    pub version: i64,
    pub changes: usize,
    pub raises: usize,
    pub lowers: usize,
    pub file_name: &'a str,
    pub csv: &'a str,
    pub undo_csv: &'a str,
}

/// Keeps an export and drops the oldest beyond [`KEEP_EXPORTS`].
pub fn keep_export(
    conn: &Connection,
    now: DateTime<Utc>,
    e: &NewExport<'_>,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO srv_repricing_exports (created_at, user_id, username, revision, version,
             changes, raises, lowers, file_name, csv, undo_csv)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            format_utc(now),
            e.user_id,
            e.username,
            e.revision,
            e.version,
            e.changes as i64,
            e.raises as i64,
            e.lowers as i64,
            e.file_name,
            e.csv.as_bytes(),
            e.undo_csv.as_bytes()
        ],
    )?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "DELETE FROM srv_repricing_exports WHERE id NOT IN
             (SELECT id FROM srv_repricing_exports ORDER BY id DESC LIMIT ?1)",
        [KEEP_EXPORTS],
    )?;
    Ok(id)
}

/// A kept export, without its files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct PriceExportSummary {
    pub id: i64,
    /// RFC 3339.
    pub created_at: String,
    pub username: String,
    pub revision: i64,
    /// Inventory version (import) the prices are for.
    pub version: i64,
    pub changes: i64,
    pub raises: i64,
    pub lowers: i64,
    pub file_name: String,
    pub undo_file_name: String,
}

pub fn exports(conn: &Connection) -> rusqlite::Result<Vec<PriceExportSummary>> {
    conn.prepare(
        "SELECT id, created_at, username, revision, version, changes, raises, lowers, file_name
         FROM srv_repricing_exports ORDER BY id DESC",
    )?
    .query_map([], |r| {
        let file_name: String = r.get(8)?;
        Ok(PriceExportSummary {
            id: r.get(0)?,
            created_at: r.get(1)?,
            username: r.get(2)?,
            revision: r.get(3)?,
            version: r.get(4)?,
            changes: r.get(5)?,
            raises: r.get(6)?,
            lowers: r.get(7)?,
            undo_file_name: check_stock_core::repricing::undo_file_name(&file_name),
            file_name,
        })
    })?
    .collect()
}

/// The new-prices file (or the undo file) of a kept export: name and bytes.
pub fn export_file(
    conn: &Connection,
    id: i64,
    undo: bool,
) -> rusqlite::Result<Option<(String, Vec<u8>)>> {
    conn.query_row(
        "SELECT file_name, csv, undo_csv FROM srv_repricing_exports WHERE id = ?1",
        [id],
        |r| {
            let name: String = r.get(0)?;
            Ok(if undo {
                (
                    check_stock_core::repricing::undo_file_name(&name),
                    r.get(2)?,
                )
            } else {
                (name, r.get(1)?)
            })
        },
    )
    .optional()
}

// ── The stock, prepared ────────────────────────────────────────────────────

static NO_MARKET: LazyLock<HashMap<u64, LatestPrice>> = LazyLock::new(HashMap::new);

/// The listings of the current stock with their facts, shared by all users.
pub struct Prepared {
    /// [`latest_version`] when it was prepared.
    pub data_version: i64,
    pub today: NaiveDate,
    pub source: Arc<Source>,
    pub market: Option<Arc<MarketData>>,
    /// One per pile of the source with copies (`Listing::row` = its index).
    pub listings: Vec<Listing>,
    /// Pile id of each listing.
    pub piles: Vec<String>,
    index: HashMap<String, usize>,
    /// Listings of a variant with copies reserved for a bulk lot awaiting
    /// its import: never ticked, never exported.
    pub held_back: Vec<bool>,
    keys: Vec<String>,
    sales: HashMap<String, Vec<(String, i64)>>,
    pub rarities: Vec<String>,
    pub languages: Vec<String>,
}

impl Prepared {
    /// Joins the source with the DB facts (both keyed by `card_variant_key`).
    pub fn build(
        data_version: i64,
        today: NaiveDate,
        source: Arc<Source>,
        market: Option<Arc<MarketData>>,
        dates: &HashMap<String, String>,
        sales: HashMap<String, Vec<(String, i64)>>,
        reserved: &HashSet<String>,
    ) -> Self {
        let mut listings = Vec::with_capacity(source.cards.len());
        let mut piles = Vec::with_capacity(source.cards.len());
        let mut keys = Vec::with_capacity(source.cards.len());
        let mut held_back = Vec::with_capacity(source.cards.len());
        for (row, card) in source.cards.iter().enumerate() {
            let key = card_variant_key(card, CATEGORY);
            let facts = ListingFacts::from_history(
                dates.get(&key).map(String::as_str),
                sales.get(&key).map_or(&[][..], Vec::as_slice),
                today,
            );
            if let Some(listing) = Listing::from_card(row, card, facts) {
                listings.push(listing);
                piles.push(source.ids[row].clone());
                held_back.push(reserved.contains(&key));
                keys.push(key);
            }
        }
        let index = piles
            .iter()
            .enumerate()
            .map(|(i, p)| (p.clone(), i))
            .collect();
        let rarities = with_defaults(
            distinct_by_copies(&listings, |l| &l.rarity),
            &DEFAULT_RARITIES,
        );
        let languages = with_defaults(
            distinct_by_copies(&listings, |l| &l.language),
            &DEFAULT_LANGUAGES,
        );
        Self {
            data_version,
            today,
            source,
            market,
            listings,
            piles,
            index,
            held_back,
            keys,
            sales,
            rarities,
            languages,
        }
    }

    pub fn listing_of(&self, pile: &str) -> Option<usize> {
        self.index.get(pile).copied()
    }

    pub fn market_rows(&self) -> &HashMap<u64, LatestPrice> {
        self.market.as_ref().map_or(&NO_MARKET, |m| &m.latest)
    }

    pub fn market_version(&self) -> Option<u64> {
        self.market.as_ref().map(|m| m.version)
    }

    /// Sale history of a listing's variant: `(date, copies)`, newest first.
    pub fn sales_of(&self, i: usize) -> Vec<(String, i64)> {
        let mut out = self.sales.get(&self.keys[i]).cloned().unwrap_or_default();
        out.sort_by(|a, b| b.0.cmp(&a.0));
        out
    }

    /// The hand prices of a review by listing index (unknown piles ignored).
    pub fn manual_by_listing(&self, review: &Review) -> HashMap<usize, f64> {
        review
            .manual
            .iter()
            .filter_map(|(pile, price)| self.listing_of(pile).map(|i| (i, *price)))
            .collect()
    }
}

/// Values found in the stock first, then the defaults not seen there.
fn with_defaults(mut found: Vec<String>, defaults: &[&str]) -> Vec<String> {
    for d in defaults {
        if !found.iter().any(|f| f.eq_ignore_ascii_case(d)) {
            found.push(d.to_string());
        }
    }
    found
}

// ── Review rows ────────────────────────────────────────────────────────────

/// Whether a listing goes into the export.
pub fn is_ticked(p: &Proposal, overridden: Option<bool>, held_back: bool) -> bool {
    !held_back && p.outcome.is_change() && overridden.unwrap_or_else(|| p.default_ticked())
}

/// Which listings the review lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ReviewFilter {
    Changes,
    Raises,
    Lowers,
    /// Big changes not ticked yet.
    NeedsReview,
    Unchanged,
    /// Changes held back for a bulk lot.
    HeldBack,
    All,
}

/// Sort column of the review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ReviewSort {
    /// |change| × copies.
    Impact,
    /// Change in %.
    Change,
    Name,
    Set,
    Quantity,
    Market,
    Current,
    New,
    Rule,
}

/// The review's view of one evaluation for one user.
pub struct ReviewView<'a> {
    pub prepared: &'a Prepared,
    pub eval: &'a Evaluation,
    pub ticks: &'a HashMap<String, bool>,
}

impl ReviewView<'_> {
    pub fn overridden(&self, i: usize) -> Option<bool> {
        self.ticks.get(&self.prepared.piles[i]).copied()
    }

    pub fn ticked(&self, i: usize) -> bool {
        is_ticked(
            &self.eval.proposals[i],
            self.overridden(i),
            self.prepared.held_back[i],
        )
    }

    fn accepts(&self, i: usize, filter: ReviewFilter) -> bool {
        let p = &self.eval.proposals[i];
        let held = self.prepared.held_back[i];
        match filter {
            ReviewFilter::Changes => p.outcome.is_change(),
            ReviewFilter::Raises => p.outcome == Outcome::Raise,
            ReviewFilter::Lowers => p.outcome == Outcome::Lower,
            ReviewFilter::NeedsReview => {
                p.outcome.is_change() && p.review && !held && !self.ticked(i)
            }
            ReviewFilter::Unchanged => !p.outcome.is_change(),
            ReviewFilter::HeldBack => p.outcome.is_change() && held,
            ReviewFilter::All => true,
        }
    }

    /// Listing indices passing the filters, in table order (desktop rules:
    /// comma-separated terms match the name, the set name or the exact set
    /// code; ties by name, then stock order).
    pub fn rows(
        &self,
        filter: ReviewFilter,
        rule: Option<usize>,
        search: &str,
        sort: ReviewSort,
        desc: bool,
    ) -> Vec<usize> {
        let terms: Vec<String> = search
            .split(',')
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        let l = &self.prepared.listings;
        let p = &self.eval.proposals;
        let mut rows: Vec<usize> = (0..l.len())
            .filter(|&i| {
                self.accepts(i, filter)
                    && rule.is_none_or(|r| p[i].rule == Some(r))
                    && (terms.is_empty()
                        || terms.iter().any(|t| {
                            l[i].name.to_lowercase().contains(t)
                                || l[i].set.to_lowercase().contains(t)
                                || l[i].set_code.to_lowercase() == *t
                        }))
            })
            .collect();
        let f = |a: f64, b: f64| a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal);
        let impact = |i: usize| {
            if p[i].outcome.is_change() {
                p[i].delta().abs() * l[i].quantity as f64
            } else {
                0.0
            }
        };
        let change = |i: usize| {
            if p[i].outcome.is_change() {
                p[i].delta_pct()
            } else {
                0.0
            }
        };
        rows.sort_by(|&a, &b| {
            let ord = match sort {
                ReviewSort::Impact => f(impact(a), impact(b)),
                ReviewSort::Change => f(change(a), change(b)),
                ReviewSort::Name => l[a].name.cmp(&l[b].name),
                ReviewSort::Set => l[a]
                    .set_code
                    .cmp(&l[b].set_code)
                    .then_with(|| l[a].name.cmp(&l[b].name)),
                ReviewSort::Quantity => l[a].quantity.cmp(&l[b].quantity),
                ReviewSort::Market => f(p[a].market.unwrap_or(-1.0), p[b].market.unwrap_or(-1.0)),
                ReviewSort::Current => f(p[a].old_price, p[b].old_price),
                ReviewSort::New => f(p[a].new_price, p[b].new_price),
                ReviewSort::Rule => p[a]
                    .rule
                    .unwrap_or(usize::MAX)
                    .cmp(&p[b].rule.unwrap_or(usize::MAX)),
            };
            let ord = if desc { ord.reverse() } else { ord };
            ord.then_with(|| l[a].name.cmp(&l[b].name)).then(a.cmp(&b))
        });
        rows
    }

    /// Totals (ticked = what the export would contain) and the number of
    /// changes held back for bulk lots.
    pub fn summary(&self) -> (Summary, usize) {
        let summary = summarize(self.eval, &self.prepared.listings, |i, _| self.ticked(i));
        let held = (0..self.prepared.listings.len())
            .filter(|&i| self.prepared.held_back[i] && self.eval.proposals[i].outcome.is_change())
            .count();
        (summary, held)
    }

    /// Piles of all rows passing the filters (for "tick all shown").
    pub fn piles_of(&self, rows: &[usize]) -> Vec<String> {
        rows.iter()
            .filter(|&&i| self.eval.proposals[i].outcome.is_change() && !self.prepared.held_back[i])
            .map(|&i| self.prepared.piles[i].clone())
            .collect()
    }
}

// ── The service ────────────────────────────────────────────────────────────

type EvalKey = [u8; 32];

/// Prepared stock and recent evaluations, shared by all users.
pub struct RepricingService {
    prepared: Mutex<Option<Arc<Prepared>>>,
    evals: Mutex<VecDeque<(EvalKey, Arc<Evaluation>)>>,
    permits: tokio::sync::Semaphore,
}

impl Default for RepricingService {
    fn default() -> Self {
        Self {
            prepared: Mutex::new(None),
            evals: Mutex::new(VecDeque::new()),
            permits: tokio::sync::Semaphore::new(EVAL_PERMITS),
        }
    }
}

impl RepricingService {
    /// The prepared stock; `None` before the first import on the server.
    pub async fn prepared(
        &self,
        db: &Db,
        stock: &StockService,
        market: &MarketService,
    ) -> Result<Option<Arc<Prepared>>, StockError> {
        let today = today();
        let market = market.loaded();
        let market_version = market.as_ref().map(|m| m.version);
        let data_version = db.read(|c| latest_version(c).map(|v| v.0)).await?;
        if let Some(cached) = lock(&self.prepared).as_ref() {
            if cached.data_version == data_version
                && cached.market_version() == market_version
                && cached.today == today
            {
                return Ok(Some(cached.clone()));
            }
        }
        let Some(source) = stock.source(db).await? else {
            return Ok(None);
        };
        let (dates, sales, reserved) = db
            .read(|c| {
                let reserved: HashSet<String> = pending_removals_conn(c, CATEGORY)?
                    .into_iter()
                    .filter(|r| r.pending > 0)
                    .map(|r| r.variant)
                    .collect();
                Ok((
                    listing_dates_conn(c, CATEGORY)?,
                    sales_by_variant_conn(c, CATEGORY)?,
                    reserved,
                ))
            })
            .await?;
        let prepared = tokio::task::spawn_blocking(move || {
            Arc::new(Prepared::build(
                data_version,
                today,
                source,
                market,
                &dates,
                sales,
                &reserved,
            ))
        })
        .await
        .map_err(|_| StockError::Internal)?;
        *lock(&self.prepared) = Some(prepared.clone());
        Ok(Some(prepared))
    }

    /// Prices every listing by `ruleset` (cached by all inputs).
    pub async fn evaluate(
        &self,
        prepared: &Arc<Prepared>,
        ruleset: &Ruleset,
        manual: HashMap<usize, f64>,
    ) -> Result<Arc<Evaluation>, StockError> {
        let key = eval_key(prepared, ruleset, &manual);
        if let Some(found) = lock(&self.evals).iter().find(|(k, _)| *k == key) {
            return Ok(found.1.clone());
        }
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| StockError::Internal)?;
        // Another request may have done the same work while this one waited.
        if let Some(found) = lock(&self.evals).iter().find(|(k, _)| *k == key) {
            return Ok(found.1.clone());
        }
        let (prep, rules) = (prepared.clone(), ruleset.clone());
        let eval = tokio::task::spawn_blocking(move || {
            Arc::new(evaluate(
                &rules,
                &prep.listings,
                prep.market_rows(),
                &manual,
                prep.today,
            ))
        })
        .await
        .map_err(|_| StockError::Internal)?;
        let mut evals = lock(&self.evals);
        evals.push_front((key, eval.clone()));
        evals.truncate(EVAL_CACHE);
        Ok(eval)
    }
}

/// SHA-256 over everything an evaluation depends on.
fn eval_key(prepared: &Prepared, ruleset: &Ruleset, manual: &HashMap<usize, f64>) -> EvalKey {
    let mut h = Sha256::new();
    h.update(serde_json::to_vec(ruleset).unwrap_or_default());
    h.update(prepared.data_version.to_le_bytes());
    h.update(prepared.market_version().unwrap_or(0).to_le_bytes());
    h.update([u8::from(prepared.market.is_some())]);
    h.update(prepared.today.to_string().as_bytes());
    let mut manual: Vec<(usize, f64)> = manual.iter().map(|(i, p)| (*i, *p)).collect();
    manual.sort_by_key(|(i, _)| *i);
    for (i, p) in manual {
        h.update((i as u64).to_le_bytes());
        h.update(p.to_le_bytes());
    }
    h.finalize().into()
}

#[cfg(test)]
#[path = "repricing_tests.rs"]
pub(crate) mod tests;
