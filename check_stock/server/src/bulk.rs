//! Bulk lots on the server (4c): choose old, cheap singles from the stock
//! ([`crate::stock::Source`], minus copies already promised to other lots),
//! pick them, record the lot (revenue + pending removals) and keep its TCG
//! Powertools CSV.
//!
//! Owner-confirmed rules (2026-11-16):
//! - Finishing re-checks, on the writer, that every picked copy is still
//!   free (not promised to another lot, not discarded); otherwise it is
//!   refused naming the piles, and the page offers "Replace".
//! - A backup before finishing, marking a removal as done and undoing
//!   (`inventory.db.bulklot-<time>.bak`, newest [`KEEP_BULK_BACKUPS`]); no
//!   backup, no change.
//! - Undoing a lot whose copies an import already took out needs the number
//!   of those copies typed in (they would come back as stock and count as
//!   sold later).
//! - Price corrections are checked against the price the page showed.
//! - Every change is a data change (banner, open import checks redone).
//! - Card types come from Scryfall for cards of the stock only, as a shared
//!   background job, cached in the data folder.

use crate::import::{prune_backups, unique_backup_path};
use crate::jobs::{Job, JobStatus};
use crate::migrations::format_utc;
use crate::stock::{Source, CATEGORY};
use check_stock_core::bulk_lot::{
    build_candidates, build_lot, funnel, sort_for_picking, summarize, top_up, type_lookups_needed,
    Candidate, CardType, FoilFilter, Funnel, LotOrder, LotPick, LotSpec, LotSummary, Rarity,
    CONDITIONS, MAX_LOT_SIZE,
};
use check_stock_core::cache::CardTypeCache;
use check_stock_core::card_types::{lookup_types_from, LookupOutcome, LookupProgress, Printing};
use check_stock_core::inventory_db::{listing_dates_conn, pending_removals_conn};
use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use ts_rs::TS;

/// Backups taken before bulk-lot changes that are kept.
pub const KEEP_BULK_BACKUPS: usize = 10;
/// Kind in the backup file names.
pub const BACKUP_KIND: &str = "bulklot";
/// Days after which a lot still awaiting its import is shown as overdue.
pub const OVERDUE_DAYS: i64 = 7;
/// Scryfall's API (the type lookup).
pub const SCRYFALL_API: &str = "https://api.scryfall.com";
/// Pauses of the type lookup (Scryfall's rate limits).
const BATCH_PAUSE: Duration = Duration::from_millis(600);
const SINGLE_PAUSE: Duration = Duration::from_millis(150);

// ── The rules of a lot ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RarityKey {
    Common,
    Uncommon,
    Rare,
    Mythic,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CardTypeKey {
    Any,
    NonBasicLand,
    Creature,
    InstantSorcery,
    Artifact,
    Enchantment,
    Planeswalker,
    NonLand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FoilKey {
    NonFoil,
    FoilOnly,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum OrderKey {
    Nearby,
    OldestFirst,
    CheapestFirst,
    Random,
}

/// What a lot is made of (the desktop's `LotSpec`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SpecView {
    /// Copies in the lot (1–1,000).
    pub size: u32,
    pub rarities: Vec<RarityKey>,
    pub card_type: CardTypeKey,
    pub exclude_basic_lands: bool,
    pub foil: FoilKey,
    /// Short codes (NM, EX, GD, LP, PL, PO) not allowed.
    pub excluded_conditions: Vec<String>,
    /// Languages (as in the export) not allowed.
    pub excluded_languages: Vec<String>,
    /// Filler copies cost at most this much each (euros).
    pub max_price: f64,
    /// Listed at least this many days.
    pub min_age_days: i64,
    /// Copies per card name (1 = no repeats).
    pub max_copies_per_name: i64,
    /// Guaranteed copies worth at least `hit_min_price` each.
    pub hits: u32,
    pub hit_min_price: f64,
    pub order: OrderKey,
    /// Different seeds draw a different mix among equally good cards.
    pub seed: u32,
}

fn rarity(k: RarityKey) -> Rarity {
    match k {
        RarityKey::Common => Rarity::Common,
        RarityKey::Uncommon => Rarity::Uncommon,
        RarityKey::Rare => Rarity::Rare,
        RarityKey::Mythic => Rarity::Mythic,
        RarityKey::Other => Rarity::Other,
    }
}

fn rarity_key(r: Rarity) -> RarityKey {
    match r {
        Rarity::Common => RarityKey::Common,
        Rarity::Uncommon => RarityKey::Uncommon,
        Rarity::Rare => RarityKey::Rare,
        Rarity::Mythic => RarityKey::Mythic,
        Rarity::Other => RarityKey::Other,
    }
}

fn card_type(k: CardTypeKey) -> CardType {
    match k {
        CardTypeKey::Any => CardType::Any,
        CardTypeKey::NonBasicLand => CardType::NonBasicLand,
        CardTypeKey::Creature => CardType::Creature,
        CardTypeKey::InstantSorcery => CardType::InstantSorcery,
        CardTypeKey::Artifact => CardType::Artifact,
        CardTypeKey::Enchantment => CardType::Enchantment,
        CardTypeKey::Planeswalker => CardType::Planeswalker,
        CardTypeKey::NonLand => CardType::NonLand,
    }
}

fn card_type_key(t: CardType) -> CardTypeKey {
    match t {
        CardType::Any => CardTypeKey::Any,
        CardType::NonBasicLand => CardTypeKey::NonBasicLand,
        CardType::Creature => CardTypeKey::Creature,
        CardType::InstantSorcery => CardTypeKey::InstantSorcery,
        CardType::Artifact => CardTypeKey::Artifact,
        CardType::Enchantment => CardTypeKey::Enchantment,
        CardType::Planeswalker => CardTypeKey::Planeswalker,
        CardType::NonLand => CardTypeKey::NonLand,
    }
}

fn foil(k: FoilKey) -> FoilFilter {
    match k {
        FoilKey::NonFoil => FoilFilter::NonFoil,
        FoilKey::FoilOnly => FoilFilter::FoilOnly,
        FoilKey::Any => FoilFilter::Any,
    }
}

fn foil_key(f: FoilFilter) -> FoilKey {
    match f {
        FoilFilter::NonFoil => FoilKey::NonFoil,
        FoilFilter::FoilOnly => FoilKey::FoilOnly,
        FoilFilter::Any => FoilKey::Any,
    }
}

fn order(k: OrderKey) -> LotOrder {
    match k {
        OrderKey::Nearby => LotOrder::Nearby,
        OrderKey::OldestFirst => LotOrder::OldestFirst,
        OrderKey::CheapestFirst => LotOrder::CheapestFirst,
        OrderKey::Random => LotOrder::Random,
    }
}

fn order_key(o: LotOrder) -> OrderKey {
    match o {
        LotOrder::Nearby => OrderKey::Nearby,
        LotOrder::OldestFirst => OrderKey::OldestFirst,
        LotOrder::CheapestFirst => OrderKey::CheapestFirst,
        LotOrder::Random => OrderKey::Random,
    }
}

impl From<&LotSpec> for SpecView {
    fn from(s: &LotSpec) -> Self {
        Self {
            size: u32::try_from(s.size).unwrap_or(u32::MAX),
            rarities: s.rarities.iter().map(|r| rarity_key(*r)).collect(),
            card_type: card_type_key(s.card_type),
            exclude_basic_lands: s.exclude_basic_lands,
            foil: foil_key(s.foil),
            excluded_conditions: s.excluded_conditions.iter().cloned().collect(),
            excluded_languages: s.excluded_languages.iter().cloned().collect(),
            max_price: s.max_price,
            min_age_days: s.min_age_days,
            max_copies_per_name: s.max_copies_per_name,
            hits: u32::try_from(s.hits).unwrap_or(u32::MAX),
            hit_min_price: s.hit_min_price,
            order: order_key(s.order),
            seed: u32::try_from(s.seed).unwrap_or(u32::MAX),
        }
    }
}

impl SpecView {
    /// The rules checked and turned into core's `LotSpec`; the message says
    /// what is wrong.
    pub fn to_spec(&self) -> Result<LotSpec, &'static str> {
        let size = self.size as usize;
        if !(1..=MAX_LOT_SIZE).contains(&size) {
            return Err("The lot size must be 1 to 1,000 copies");
        }
        if self.rarities.is_empty() {
            return Err("Choose at least one rarity");
        }
        if self
            .excluded_conditions
            .iter()
            .any(|c| !CONDITIONS.contains(&c.as_str()))
        {
            return Err("Unknown condition");
        }
        if self.excluded_languages.len() > 20
            || self
                .excluded_languages
                .iter()
                .any(|l| l.is_empty() || l.chars().count() > 40)
        {
            return Err("Too many or too long languages");
        }
        if !self.max_price.is_finite() || !(0.0..=10_000.0).contains(&self.max_price) {
            return Err("The price per card must be 0 to 10,000 €");
        }
        if !(0..=3650).contains(&self.min_age_days) {
            return Err("The time in stock must be 0 to 3,650 days");
        }
        if !(1..=1000).contains(&self.max_copies_per_name) {
            return Err("Copies per card name must be 1 to 1,000");
        }
        if self.hits > self.size {
            return Err("More guaranteed hits than cards in the lot");
        }
        if !self.hit_min_price.is_finite() || !(0.0..=100_000.0).contains(&self.hit_min_price) {
            return Err("The hit price must be 0 to 100,000 €");
        }
        Ok(LotSpec {
            size,
            rarities: self
                .rarities
                .iter()
                .map(|r| rarity(*r))
                .collect::<BTreeSet<_>>(),
            card_type: card_type(self.card_type),
            exclude_basic_lands: self.exclude_basic_lands,
            foil: foil(self.foil),
            excluded_conditions: self.excluded_conditions.iter().cloned().collect(),
            excluded_languages: self.excluded_languages.iter().cloned().collect(),
            max_price: self.max_price,
            min_age_days: self.min_age_days,
            max_copies_per_name: self.max_copies_per_name,
            hits: self.hits as usize,
            hit_min_price: self.hit_min_price,
            order: order(self.order),
            seed: u64::from(self.seed),
        })
    }
}

// ── Candidates, types, previews ────────────────────────────────────────────

/// The candidates of `source`: copies promised to lots awaiting their import
/// are reserved, listing dates from the database, type lines from `types`.
pub fn candidates(
    conn: &Connection,
    source: &Source,
    types: Option<&CardTypeCache>,
    today: NaiveDate,
) -> rusqlite::Result<Vec<Candidate>> {
    let pending = pending_removals_conn(conn, CATEGORY)?;
    let dates = listing_dates_conn(conn, CATEGORY)?;
    let mut candidates = build_candidates(&source.cards, CATEGORY, &dates, &pending, today);
    if let Some(types) = types {
        for c in &mut candidates {
            c.front_type = types.type_line(&c.set_code, &c.cn).map(str::to_string);
        }
    }
    Ok(candidates)
}

/// Printings whose type line the spec needs and nobody has looked up (or
/// that failed long enough ago to try again).
pub fn lookups(
    source: &Source,
    candidates: &[Candidate],
    spec: &LotSpec,
    types: &CardTypeCache,
    today: NaiveDate,
) -> Vec<Printing> {
    type_lookups_needed(candidates, spec)
        .into_iter()
        .filter(|&i| !types.is_settled(&candidates[i].set_code, &candidates[i].cn, today))
        .map(|i| {
            let c = &candidates[i];
            Printing {
                set_code: c.set_code.clone(),
                cn: c.cn.clone(),
                cardmarket_id: source.cards[c.source].cardmarket_id.trim().parse().ok(),
            }
        })
        .collect()
}

/// A lot built from `candidates`, in picking order.
pub fn preview(candidates: &[Candidate], spec: &LotSpec) -> (Funnel, Vec<LotPick>, LotSummary) {
    let mut picks = build_lot(candidates, spec);
    sort_for_picking(candidates, &mut picks);
    let summary = summarize(candidates, &picks, spec.hit_min_price);
    (funnel(candidates, spec), picks, summary)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The card-type cache and its lookup job, shared by all users.
pub struct BulkService {
    types: Arc<Mutex<CardTypeCache>>,
    job: Arc<Job>,
    scryfall: String,
    pauses: (Duration, Duration),
}

impl BulkService {
    /// Types cached in `<cache_root>/…` (the desktop app's layout).
    pub fn new(cache_root: &Path) -> Self {
        Self {
            types: Arc::new(Mutex::new(CardTypeCache::in_root(cache_root))),
            job: Arc::new(Job::default()),
            scryfall: SCRYFALL_API.to_string(),
            pauses: (BATCH_PAUSE, SINGLE_PAUSE),
        }
    }

    /// Another Scryfall address, without pauses (tests).
    pub fn with_scryfall(mut self, base: &str) -> Self {
        self.scryfall = base.to_string();
        self.pauses = (Duration::ZERO, Duration::ZERO);
        self
    }

    /// Runs `f` with the type cache.
    pub fn with_types<T>(&self, f: impl FnOnce(&CardTypeCache) -> T) -> T {
        f(&lock(&self.types))
    }

    pub fn status(&self) -> JobStatus {
        self.job.status()
    }

    pub fn shutdown(&self) {
        self.job.cancel();
    }

    /// Looks `printings` up on Scryfall as a background job; `false` when a
    /// lookup is already running.
    pub fn start_lookup(&self, printings: Vec<Printing>, today: NaiveDate) -> bool {
        if printings.is_empty() {
            return true;
        }
        let Some(run) = self.job.start("Looking up card types…", Utc::now()) else {
            return false;
        };
        let types = self.types.clone();
        let base = self.scryfall.clone();
        let (batch_pause, single_pause) = self.pauses;
        tokio::task::spawn_blocking(move || {
            let outcome = lookup_types_from(
                &base,
                &printings,
                run.cancel_flag(),
                |p| match p {
                    LookupProgress::Batches { done, total } => {
                        run.progress("Card types from Scryfall", done as u64, total as u64)
                    }
                    LookupProgress::Singles { done, total } => {
                        run.progress("Card types one by one", done as u64, total as u64)
                    }
                },
                batch_pause,
                single_pause,
            );
            let LookupOutcome {
                found,
                missing,
                error,
                cancelled,
            } = outcome;
            {
                let mut cache = lock(&types);
                for (set, cn, type_line) in &found {
                    cache.insert(set, cn, type_line);
                }
                for (set, cn) in &missing {
                    cache.insert_missing(set, cn, today);
                }
                cache.save();
            }
            match (error, cancelled) {
                (Some(e), _) => run.fail(&crate::jobs::public_message(&e), Utc::now()),
                (None, true) => run.cancelled(Utc::now()),
                (None, false) => run.finish(Utc::now()),
            }
        });
        true
    }
}

// ── Drafts ─────────────────────────────────────────────────────────────────

/// One chosen pile of a draft.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DraftPick {
    pub pile: String,
    pub copies: i64,
    pub hit: bool,
    pub picked: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub spec: SpecView,
    pub picks: Vec<DraftPick>,
    /// Piles swapped out (never chosen again for this draft).
    pub excluded: Vec<String>,
    pub updated_at: String,
}

pub fn load_draft(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<Draft>> {
    let row: Option<(String, String, String, String)> = conn
        .query_row(
            "SELECT spec, picks, excluded, updated_at FROM srv_bulk_lot_drafts WHERE user_id = ?1",
            [user_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    Ok(row.and_then(|(spec, picks, excluded, updated_at)| {
        Some(Draft {
            spec: serde_json::from_str(&spec).ok()?,
            picks: serde_json::from_str(&picks).ok()?,
            excluded: serde_json::from_str(&excluded).ok()?,
            updated_at,
        })
    }))
}

pub fn save_draft(conn: &Connection, user_id: i64, draft: &Draft) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO srv_bulk_lot_drafts (user_id, spec, picks, excluded, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (user_id) DO UPDATE SET spec = excluded.spec, picks = excluded.picks,
             excluded = excluded.excluded, updated_at = excluded.updated_at",
        params![
            user_id,
            serde_json::to_string(&draft.spec).expect("spec serializes"),
            serde_json::to_string(&draft.picks).expect("picks serialize"),
            serde_json::to_string(&draft.excluded).expect("ids serialize"),
            draft.updated_at,
        ],
    )?;
    Ok(())
}

pub fn delete_draft(conn: &Connection, user_id: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "DELETE FROM srv_bulk_lot_drafts WHERE user_id = ?1",
        [user_id],
    )? > 0)
}

/// Picks as draft lines (pile ids of `source`).
pub fn draft_picks(source: &Source, candidates: &[Candidate], picks: &[LotPick]) -> Vec<DraftPick> {
    picks
        .iter()
        .map(|p| DraftPick {
            pile: source.ids[candidates[p.candidate].source].clone(),
            copies: p.copies,
            hit: p.hit,
            picked: false,
        })
        .collect()
}

/// Index of every pile's candidate.
pub fn candidate_index(source: &Source, candidates: &[Candidate]) -> HashMap<String, usize> {
    candidates
        .iter()
        .enumerate()
        .map(|(i, c)| (source.ids[c.source].clone(), i))
        .collect()
}

/// Swaps `pile` out of `draft`: the copies that leave are replaced by the
/// next matching cards (core's `top_up`), never from swapped-out piles.
/// Returns how many copies came in.
pub fn replace(
    draft: &mut Draft,
    pile: &str,
    source: &Source,
    candidates: &[Candidate],
    spec: &LotSpec,
) -> i64 {
    let index = candidate_index(source, candidates);
    draft.picks.retain(|p| p.pile != pile);
    if !draft.excluded.iter().any(|e| e == pile) {
        draft.excluded.push(pile.to_string());
    }
    let existing: Vec<LotPick> = draft
        .picks
        .iter()
        .filter_map(|p| {
            Some(LotPick {
                candidate: *index.get(&p.pile)?,
                copies: p.copies,
                hit: p.hit,
            })
        })
        .collect();
    let excluded: HashSet<usize> = draft
        .excluded
        .iter()
        .filter_map(|e| index.get(e).copied())
        .collect();
    let mut added = top_up(candidates, spec, &existing, &excluded);
    sort_for_picking(candidates, &mut added);
    let copies = added.iter().map(|p| p.copies).sum();
    draft.picks.extend(draft_picks(source, candidates, &added));
    // Keep picking order over the whole list.
    let mut all: Vec<(DraftPick, Option<usize>)> = draft
        .picks
        .drain(..)
        .map(|p| {
            let c = index.get(&p.pile).copied();
            (p, c)
        })
        .collect();
    all.sort_by(|(_, a), (_, b)| match (a, b) {
        (Some(a), Some(b)) => {
            let (a, b) = (&candidates[*a], &candidates[*b]);
            check_stock_core::bulk_lot::location_sort_key(&a.location)
                .cmp(&check_stock_core::bulk_lot::location_sort_key(&b.location))
                .then_with(|| a.name_key.cmp(&b.name_key))
        }
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    draft.picks = all.into_iter().map(|(p, _)| p).collect();
    copies
}

/// Picked piles that are no longer free in `candidates` (taken by another
/// lot, discarded or gone), with the copies still free.
pub fn shortfalls(draft: &Draft, source: &Source, candidates: &[Candidate]) -> Vec<(String, i64)> {
    let index = candidate_index(source, candidates);
    draft
        .picks
        .iter()
        .filter(|p| p.picked)
        .filter_map(|p| {
            let free = index.get(&p.pile).map_or(0, |&i| candidates[i].available);
            (p.copies > free).then(|| (p.pile.clone(), free))
        })
        .collect()
}

// ── Backups, CSVs, data changes ────────────────────────────────────────────

/// A consistent copy of the database next to it, before a bulk-lot change;
/// keeps the newest [`KEEP_BULK_BACKUPS`]. Runs outside any transaction.
pub fn backup(conn: &Connection, db_path: &Path) -> Result<PathBuf, String> {
    let stamp = chrono::Local::now().format("%Y%m%dT%H%M%S").to_string();
    let target = unique_backup_path(db_path, BACKUP_KIND, &stamp);
    conn.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])
        .map_err(|e| e.to_string())?;
    if let Some(dir) = db_path.parent() {
        if let Err(e) = prune_backups(dir, BACKUP_KIND, KEEP_BULK_BACKUPS) {
            log::warn!("could not remove old bulk-lot backups: {e}");
        }
    }
    Ok(target)
}

/// Records a data change (part of the data version).
pub fn data_changed(conn: &Connection, kind: &str, now: DateTime<Utc>) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO srv_data_changes (at, kind) VALUES (?1, ?2)",
        params![format_utc(now), kind],
    )?;
    Ok(())
}

pub fn keep_csv(
    conn: &Connection,
    lot_id: i64,
    file_name: &str,
    csv: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO srv_bulk_lot_csvs (lot_id, file_name, csv) VALUES (?1, ?2, ?3)",
        params![lot_id, file_name, csv.as_bytes()],
    )?;
    Ok(())
}

/// File name and CSV of lot `lot_id`, if kept.
pub fn csv_of(conn: &Connection, lot_id: i64) -> rusqlite::Result<Option<(String, Vec<u8>)>> {
    conn.query_row(
        "SELECT file_name, csv FROM srv_bulk_lot_csvs WHERE lot_id = ?1",
        [lot_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

pub fn has_csv(conn: &Connection, lot_id: i64) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM srv_bulk_lot_csvs WHERE lot_id = ?1)",
        [lot_id],
        |r| r.get(0),
    )
}

pub fn forget_csv(conn: &Connection, lot_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM srv_bulk_lot_csvs WHERE lot_id = ?1", [lot_id])?;
    Ok(())
}

#[cfg(test)]
#[path = "bulk_tests.rs"]
mod tests;
