//! Card type lookup for the bulk lot builder.
//!
//! The Cardmarket export carries no type line, so a lot restricted to a card
//! type ("100 non-basic lands") needs Scryfall. Printings are looked up 75 at
//! a time by set code and collector number. The few Scryfall does not know
//! under Cardmarket's codes (Cardmarket-only sets such as `XMKM`) are retried
//! one by one by Cardmarket product id. Callers store the results in the
//! permanent [`crate::cache::CardTypeCache`].
//!
//! Blocking: run it on a worker thread. Only set codes, collector numbers and
//! product ids leave the machine.

use crate::api::scryfall::{fetch_card_by_cardmarket_id_from_async, fetch_collection_from_async};
use crate::error::ApiError;
use mtg_common::scryfall::{CollectionIdentifier, COLLECTION_MAX_IDENTIFIERS, SCRYFALL_API};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Pause between batch requests (Scryfall allows about two per second there).
pub const BATCH_PAUSE: Duration = Duration::from_millis(600);
/// Pause between single-card requests (Scryfall allows about ten per second).
pub const SINGLE_PAUSE: Duration = Duration::from_millis(120);
/// Most one-by-one lookups per run, so a set Scryfall lacks entirely cannot
/// keep the screen busy for minutes.
pub const MAX_SINGLE_LOOKUPS: usize = 300;

/// A printing whose type line is wanted.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Printing {
    pub set_code: String,
    pub cn: String,
    /// Cardmarket product id, for the one-by-one fallback.
    pub cardmarket_id: Option<u64>,
}

/// Progress of a running lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupProgress {
    Batches { done: usize, total: usize },
    Singles { done: usize, total: usize },
}

/// What a lookup found. Partial results are kept when it stopped early.
#[derive(Debug, Default, PartialEq)]
pub struct LookupOutcome {
    /// `(set code, collector number, front-face type line)`.
    pub found: Vec<(String, String, String)>,
    /// Printings Scryfall could not resolve.
    pub missing: Vec<(String, String)>,
    /// Why the lookup stopped early, if it did.
    pub error: Option<String>,
    pub cancelled: bool,
}

/// Looks the printings up on Scryfall.
pub fn lookup_types(
    printings: &[Printing],
    cancel: &AtomicBool,
    progress: impl FnMut(LookupProgress),
) -> LookupOutcome {
    lookup_types_from(
        SCRYFALL_API,
        printings,
        cancel,
        progress,
        BATCH_PAUSE,
        SINGLE_PAUSE,
    )
}

fn key(set_code: &str, cn: &str) -> (String, String) {
    (set_code.trim().to_lowercase(), cn.trim().to_string())
}

/// Scryfall answered, but has no such card.
fn is_not_found(e: &ApiError) -> bool {
    matches!(e, ApiError::ApiResponse { code, .. } if code == "not_found")
        || matches!(e, ApiError::HttpStatus(s) if s.as_u16() == 404)
}

/// Scryfall refused the request itself (a bad identifier, not an outage or
/// rate limit): asking again is pointless, but its cards can still be
/// looked up one by one.
fn is_rejected(e: &ApiError) -> bool {
    matches!(e, ApiError::ApiResponse { code, .. }
        if code == "bad_request" || code == "invalid_identifier")
        || matches!(e, ApiError::HttpStatus(s) if s.as_u16() == 400 || s.as_u16() == 422)
}

/// [`lookup_types`] against `base_url` with the given pauses (for testing).
pub fn lookup_types_from(
    base_url: &str,
    printings: &[Printing],
    cancel: &AtomicBool,
    mut progress: impl FnMut(LookupProgress),
    batch_pause: Duration,
    single_pause: Duration,
) -> LookupOutcome {
    let mut out = LookupOutcome::default();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            out.error = Some(format!("Could not start the lookup: {e}"));
            return out;
        }
    };

    let mut seen = HashSet::new();
    let unique: Vec<&Printing> = printings
        .iter()
        .filter(|p| !p.set_code.trim().is_empty() && !p.cn.trim().is_empty())
        .filter(|p| seen.insert(key(&p.set_code, &p.cn)))
        .collect();

    // Batches by set code + collector number — only for identifiers Scryfall
    // accepts there; the rest (Cardmarket-only set codes such as `SLDSV23`)
    // go straight to the one-by-one lookup below.
    let (batchable, mut unresolved): (Vec<&Printing>, Vec<&Printing>) = unique
        .into_iter()
        .partition(|p| CollectionIdentifier::new(&p.set_code, &p.cn).is_accepted());
    let batches: Vec<&[&Printing]> = batchable.chunks(COLLECTION_MAX_IDENTIFIERS).collect();
    for (i, batch) in batches.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            return out;
        }
        if i > 0 {
            std::thread::sleep(batch_pause);
        }
        let ids: Vec<CollectionIdentifier> = batch
            .iter()
            .map(|p| CollectionIdentifier::new(&p.set_code, &p.cn))
            .collect();
        // One retry for outages and rate limits (Scryfall answers bursts
        // with 429); none for a request Scryfall refused outright.
        let result = match runtime.block_on(fetch_collection_from_async(base_url, &ids)) {
            Err(e) if !is_rejected(&e) => {
                std::thread::sleep(batch_pause * 3);
                runtime.block_on(fetch_collection_from_async(base_url, &ids))
            }
            result => result,
        };
        let cards = match result {
            Ok(cards) => cards,
            Err(e) if is_rejected(&e) => {
                log::warn!("Scryfall refused a type batch ({e}); looking its cards up one by one");
                unresolved.extend(batch.iter().copied());
                progress(LookupProgress::Batches {
                    done: i + 1,
                    total: batches.len(),
                });
                continue;
            }
            Err(e) => {
                out.error = Some(format!("Scryfall lookup failed: {e}"));
                return out;
            }
        };
        let types: HashMap<(String, String), &str> = cards
            .iter()
            .filter_map(|c| Some((key(&c.set, &c.collector_number), c.front_type_line()?)))
            .collect();
        for p in batch.iter() {
            match types.get(&key(&p.set_code, &p.cn)) {
                Some(t) => out
                    .found
                    .push((p.set_code.clone(), p.cn.clone(), t.to_string())),
                None => unresolved.push(p),
            }
        }
        progress(LookupProgress::Batches {
            done: i + 1,
            total: batches.len(),
        });
    }

    // One by one by Cardmarket product id, for codes Scryfall does not use.
    let (singles, unknown): (Vec<&Printing>, Vec<&Printing>) = unresolved
        .into_iter()
        .partition(|p| p.cardmarket_id.is_some());
    out.missing
        .extend(unknown.iter().map(|p| (p.set_code.clone(), p.cn.clone())));
    let total = singles.len().min(MAX_SINGLE_LOOKUPS);
    for (i, p) in singles.iter().enumerate() {
        if i >= MAX_SINGLE_LOOKUPS {
            // Not looked up this run; they stay unsettled for the next one.
            break;
        }
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            return out;
        }
        if i > 0 {
            std::thread::sleep(single_pause);
        }
        let id = p.cardmarket_id.expect("partitioned on cardmarket_id");
        match runtime.block_on(fetch_card_by_cardmarket_id_from_async(base_url, id)) {
            Ok(card) => match card.front_type_line() {
                Some(t) => out
                    .found
                    .push((p.set_code.clone(), p.cn.clone(), t.to_string())),
                None => out.missing.push((p.set_code.clone(), p.cn.clone())),
            },
            Err(e) if is_not_found(&e) => out.missing.push((p.set_code.clone(), p.cn.clone())),
            Err(e) => {
                out.error = Some(format!("Scryfall lookup failed: {e}"));
                return out;
            }
        }
        progress(LookupProgress::Singles { done: i + 1, total });
    }
    out
}

#[cfg(test)]
#[path = "card_types_tests.rs"]
mod tests;
