//! Bin analysis and consolidation move lists (step 3e), on the file of the
//! last inventory import on the server minus pending discards
//! ([`crate::stock::Source`]).
//!
//! The inventory tables cannot be used: they keep one row per card variant
//! with the copies of all bins summed under one location. The export has
//! every pile with its own location.
//!
//! - Plans are cached per (file version, discards, kind, threshold), the last
//!   [`MAX_CACHED_PLANS`] ([`BinsService`]).
//! - A move list keeps only its settings and the ticked move ids: the
//!   planner is deterministic, so the moves are planned again from the same
//!   file. After a newer import or a discard the list is *outdated*: it can be ticked no
//!   more and not exported until it is made again on the new file (ticks of
//!   moves that are still in the new plan carry over). So the "moved" CSV,
//!   which Cardmarket imports, never carries quantities or prices of an older
//!   export.

use crate::migrations::format_utc;
use crate::stock::{Source, StockError, CATEGORY};
use check_stock_core::bin_consolidation::{plan_consolidation, plan_variant_defrag, Move};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use ts_rs::TS;

/// Largest move list.
pub const MAX_LIST_MOVES: usize = 5_000;
/// Plans kept in memory.
pub const MAX_CACHED_PLANS: usize = 8;
/// Bins filled up to this many cards are emptied, unless asked otherwise
/// (the desktop app's default).
pub const DEFAULT_MERGE_THRESHOLD: i64 = 20;

/// What a move list does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MoveListKind {
    /// Empty sparse bins into fuller ones.
    Consolidation,
    /// Gather each variant spread over several bins into one.
    Defrag,
}

impl MoveListKind {
    /// Stored in the database: never rename.
    pub fn key(self) -> &'static str {
        match self {
            MoveListKind::Consolidation => "consolidation",
            MoveListKind::Defrag => "defrag",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        match key {
            "consolidation" => Some(MoveListKind::Consolidation),
            "defrag" => Some(MoveListKind::Defrag),
            _ => None,
        }
    }
}

/// A move with its stable id: the same card variant from the same location
/// to the same location has the same id in every plan, also when its price
/// or comment changed (it is still the same pile; the CSV always takes the
/// current values).
#[derive(Debug, Clone)]
pub struct PlannedMove {
    pub id: String,
    pub planned: Move,
}

#[derive(Debug)]
pub struct Plan {
    pub kind: MoveListKind,
    /// Consolidation only.
    pub threshold: Option<i64>,
    pub version: i64,
    /// The discards the plan saw ([`Source::discard_seq`]).
    pub discard_seq: i64,
    pub moves: Vec<PlannedMove>,
    pub bins_freed: Vec<String>,
    pub cards_moved: i64,
    pub total_distance: i64,
    pub sources_considered: usize,
    pub fragmented_variants: usize,
}

impl Plan {
    pub fn ids(&self) -> HashSet<&str> {
        self.moves.iter().map(|m| m.id.as_str()).collect()
    }
}

/// The id of a move: 16 hex characters of a SHA-256 over what identifies
/// the pile and where it goes.
fn move_id(m: &Move) -> String {
    let c = &m.card;
    let identity = [
        c.cardmarket_id.as_str(),
        c.condition.trim(),
        c.language.trim(),
        if c.is_foil_card() { "foil" } else { "" },
        if c.is_signed_card() { "signed" } else { "" },
        &m.from_location,
        &m.to_location,
    ]
    .join("\u{1f}");
    Sha256::digest(identity.as_bytes())[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Plans `kind` on `source` and gives every move its id (an identical pile
/// listed twice gets `-2`, `-3`, …).
pub fn make_plan(source: &Source, kind: MoveListKind, threshold: Option<i64>) -> Plan {
    let threshold = match kind {
        MoveListKind::Consolidation => Some(threshold.unwrap_or(DEFAULT_MERGE_THRESHOLD)),
        MoveListKind::Defrag => None,
    };
    let plan = match kind {
        MoveListKind::Consolidation => {
            plan_consolidation(&source.cards, threshold.unwrap_or_default())
        }
        MoveListKind::Defrag => plan_variant_defrag(&source.cards),
    };
    let mut seen: HashSet<String> = HashSet::new();
    let moves = plan
        .moves
        .into_iter()
        .map(|planned| {
            let base = move_id(&planned);
            let id = (1..)
                .map(|n| {
                    if n == 1 {
                        base.clone()
                    } else {
                        format!("{base}-{n}")
                    }
                })
                .find(|id| !seen.contains(id))
                .expect("a free id");
            seen.insert(id.clone());
            PlannedMove { id, planned }
        })
        .collect();
    Plan {
        kind,
        threshold,
        version: source.version,
        discard_seq: source.discard_seq,
        moves,
        bins_freed: plan.bins_freed,
        cards_moved: plan.cards_moved,
        total_distance: plan.total_move_distance,
        sources_considered: plan.source_bins_considered,
        fragmented_variants: plan.fragmented_variants,
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The plans, shared by all users.
#[derive(Default)]
pub struct BinsService {
    plans: Mutex<VecDeque<Arc<Plan>>>,
}

impl BinsService {
    /// The plan of `kind` on `source` (cached).
    pub async fn plan(
        &self,
        source: Arc<Source>,
        kind: MoveListKind,
        threshold: Option<i64>,
    ) -> Result<Arc<Plan>, StockError> {
        let threshold = match kind {
            MoveListKind::Consolidation => Some(threshold.unwrap_or(DEFAULT_MERGE_THRESHOLD)),
            MoveListKind::Defrag => None,
        };
        let cached = lock(&self.plans)
            .iter()
            .find(|p| {
                p.version == source.version
                    && p.discard_seq == source.discard_seq
                    && p.kind == kind
                    && p.threshold == threshold
            })
            .cloned();
        if let Some(plan) = cached {
            return Ok(plan);
        }
        let plan =
            tokio::task::spawn_blocking(move || Arc::new(make_plan(&source, kind, threshold)))
                .await
                .map_err(|_| StockError::Internal)?;
        let mut plans = lock(&self.plans);
        plans.push_back(plan.clone());
        while plans.len() > MAX_CACHED_PLANS {
            plans.pop_front();
        }
        Ok(plan)
    }
}

/// A user's move list as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredList {
    pub kind: MoveListKind,
    pub threshold: Option<i64>,
    pub version: i64,
    pub discard_seq: i64,
    pub created_at: String,
}

pub fn load_list(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<StoredList>> {
    let row: Option<(String, Option<i64>, i64, i64, String)> = conn
        .query_row(
            "SELECT kind, threshold, version_id, discard_seq, created_at
             FROM srv_move_lists WHERE user_id = ?1",
            [user_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    Ok(
        row.and_then(|(kind, threshold, version, discard_seq, created_at)| {
            Some(StoredList {
                kind: MoveListKind::from_key(&kind)?,
                threshold,
                version,
                discard_seq,
                created_at,
            })
        }),
    )
}

/// The ticked move ids of `user_id`.
pub fn ticks(conn: &Connection, user_id: i64) -> rusqlite::Result<HashSet<String>> {
    conn.prepare("SELECT move_id FROM srv_move_list_ticks WHERE user_id = ?1")?
        .query_map([user_id], |r| r.get(0))?
        .collect()
}

/// Saves `plan` as the user's list (replacing the old one); ticks of moves
/// that are in the new plan too stay. Returns (carried, dropped) ticks.
pub fn save_list(
    conn: &Connection,
    user_id: i64,
    plan: &Plan,
    now: DateTime<Utc>,
) -> rusqlite::Result<(usize, usize)> {
    let old = ticks(conn, user_id)?;
    conn.execute(
        "INSERT INTO srv_move_lists
             (user_id, kind, threshold, category, version_id, discard_seq, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (user_id) DO UPDATE SET kind = excluded.kind,
             threshold = excluded.threshold, category = excluded.category,
             version_id = excluded.version_id, discard_seq = excluded.discard_seq,
             created_at = excluded.created_at",
        params![
            user_id,
            plan.kind.key(),
            plan.threshold,
            CATEGORY,
            plan.version,
            plan.discard_seq,
            format_utc(now)
        ],
    )?;
    let ids = plan.ids();
    let mut dropped = 0;
    for id in &old {
        if !ids.contains(id.as_str()) {
            conn.execute(
                "DELETE FROM srv_move_list_ticks WHERE user_id = ?1 AND move_id = ?2",
                params![user_id, id],
            )?;
            dropped += 1;
        }
    }
    Ok((old.len() - dropped, dropped))
}

/// Ticks or unticks `ids` (all must be moves of the list; checked by the
/// caller). Returns how many are ticked now.
pub fn set_ticks(
    conn: &Connection,
    user_id: i64,
    ids: &[String],
    done: bool,
) -> rusqlite::Result<usize> {
    for id in ids {
        if done {
            conn.execute(
                "INSERT OR IGNORE INTO srv_move_list_ticks (user_id, move_id) VALUES (?1, ?2)",
                params![user_id, id],
            )?;
        } else {
            conn.execute(
                "DELETE FROM srv_move_list_ticks WHERE user_id = ?1 AND move_id = ?2",
                params![user_id, id],
            )?;
        }
    }
    Ok(ticks(conn, user_id)?.len())
}

/// Removes the user's list and its ticks; `false` when there was none.
pub fn discard_list(conn: &Connection, user_id: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute("DELETE FROM srv_move_lists WHERE user_id = ?1", [user_id])? > 0)
}

#[cfg(test)]
#[path = "bins_tests.rs"]
pub(crate) mod tests;
