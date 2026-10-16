//! Background loading for the Precon EV screen (blocking; no egui).
//!
//! Freshness rules:
//! - **Live on every load:** the precon catalog (tracker `/api/search`), the
//!   MTGJSON `DeckList.json` (which decklists are published) and all prices.
//! - **Cached permanently:** complete decklists ([`DeckCache`]).
//! - **Cached, rebuilt on demand:** the sealed-product index from the 12 MB
//!   `SetList.json` — see [`crate::cache::needs_rebuild`].

use crate::api::inventory_sync::{InventorySyncClient, PriceSnapshot};
use crate::api::mtgjson::{resolve_decks, DeckResolution, MtgjsonClient, PreconDeck};
use crate::cache::{needs_rebuild, CachedIndex, DeckCache};
use crate::precon_ev::{
    catalog_truncated, filter_precon_catalog, price_ids, PreconData, CATALOG_LIMIT, CATALOG_QUERY,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Parallel decklist downloads on a cold cache.
const DECK_FETCH_THREADS: usize = 4;

/// Progress reported while [`load_precons`] runs.
#[derive(Debug, Clone, PartialEq)]
pub enum LoadEvent {
    /// Human-readable description of the current stage.
    Stage(String),
    /// Decklists available so far (cache hits count as done).
    Decks { done: usize, total: usize },
}

/// Inputs for [`load_precons`].
pub struct LoadOptions {
    pub tracker: InventorySyncClient,
    pub mtgjson: MtgjsonClient,
    /// Rebuild the sealed index even if the cached one is fresh.
    pub force_index_refresh: bool,
    pub now_unix: i64,
}

/// Fetches catalog, index, decklists and prices. Fails only when the
/// catalog, DeckList or prices cannot be fetched (or there is no index at
/// all); individual decklist failures are recorded in `deck_errors`.
pub fn load_precons(
    opts: &LoadOptions,
    cache: &DeckCache,
    progress: &(dyn Fn(LoadEvent) + Sync),
) -> Result<PreconData, String> {
    progress(LoadEvent::Stage(
        "Fetching precon catalog from the price tracker…".to_string(),
    ));
    let rows = opts
        .tracker
        .search_blocking(CATALOG_QUERY, CATALOG_LIMIT)
        .map_err(|e| format!("Catalog fetch failed: {e}"))?;
    let truncated = catalog_truncated(rows.len(), CATALOG_LIMIT);
    let catalog = filter_precon_catalog(rows);
    let catalog_ids: Vec<u64> = catalog.iter().map(|p| p.id_product).collect();
    log::info!("Precon EV: {} precon products in catalog", catalog.len());

    let cached = cache.load_index();
    let rebuild =
        opts.force_index_refresh || needs_rebuild(cached.as_ref(), &catalog_ids, opts.now_unix);
    let (index, index_fetched_at_unix, index_stale) = match (rebuild, cached) {
        (false, Some(c)) => (c.index, c.fetched_at_unix, false),
        (_, cached) => {
            progress(LoadEvent::Stage(
                "Downloading MTGJSON SetList (~12 MB)…".to_string(),
            ));
            match opts.mtgjson.sealed_index_blocking() {
                Ok(index) => {
                    log::info!(
                        "Precon EV: rebuilt sealed index ({} products)",
                        index.products.len()
                    );
                    cache.store_index(&CachedIndex {
                        schema: crate::cache::deck_cache::INDEX_SCHEMA,
                        fetched_at_unix: opts.now_unix,
                        seen_catalog_ids: catalog_ids.clone(),
                        index: index.clone(),
                    });
                    (index, opts.now_unix, false)
                }
                Err(e) => match cached {
                    Some(c) => {
                        log::warn!("MTGJSON SetList failed, using cached index: {e}");
                        (c.index, c.fetched_at_unix, true)
                    }
                    None => return Err(format!("MTGJSON SetList download failed: {e}")),
                },
            }
        }
    };

    progress(LoadEvent::Stage("Fetching MTGJSON deck list…".to_string()));
    let (deck_list_version, deck_list) = opts
        .mtgjson
        .deck_list_blocking()
        .map_err(|e| format!("MTGJSON DeckList download failed: {e}"))?;
    let mut all = resolve_decks(&index, &deck_list);
    let resolved: HashMap<u64, DeckResolution> = catalog_ids
        .iter()
        .filter_map(|id| all.remove(id).map(|r| (*id, r)))
        .collect();

    let mut needed: Vec<String> = resolved
        .values()
        .filter_map(|r| match r {
            DeckResolution::Ready(refs) => Some(refs.iter().map(|d| d.file_name.clone())),
            DeckResolution::Pending(_) => None,
        })
        .flatten()
        .collect();
    needed.sort_unstable();
    needed.dedup();
    let (decks, deck_errors) = load_decks(&opts.mtgjson, cache, &needed, progress);

    progress(LoadEvent::Stage("Fetching latest prices…".to_string()));
    let ids = price_ids(&catalog, &decks);
    let prices = opts
        .tracker
        .latest_prices_blocking(&ids)
        .map_err(|e| format!("Price fetch failed: {e}"))?
        .into_iter()
        .map(|p| (p.id_product, p))
        .collect();

    Ok(PreconData {
        catalog,
        catalog_truncated: truncated,
        index,
        index_fetched_at_unix,
        index_stale,
        deck_list_version,
        resolved,
        decks,
        deck_errors,
        prices,
    })
}

/// Loads decklists from the cache, downloading misses in parallel. Complete
/// downloads are written back to the cache.
fn load_decks(
    client: &MtgjsonClient,
    cache: &DeckCache,
    needed: &[String],
    progress: &(dyn Fn(LoadEvent) + Sync),
) -> (HashMap<String, PreconDeck>, HashMap<String, String>) {
    let total = needed.len();
    let mut decks = HashMap::new();
    let mut misses = Vec::new();
    for name in needed {
        match cache.get(name) {
            Some(deck) => {
                decks.insert(name.clone(), deck);
            }
            None => misses.push(name.clone()),
        }
    }
    log::info!(
        "Precon EV: {} decklists cached, {} to download",
        decks.len(),
        misses.len()
    );
    progress(LoadEvent::Decks {
        done: decks.len(),
        total,
    });

    let done = AtomicUsize::new(decks.len());
    let cursor = AtomicUsize::new(0);
    let results: Mutex<Vec<(String, Result<PreconDeck, String>)>> = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..DECK_FETCH_THREADS.min(misses.len()) {
            s.spawn(|| {
                while let Some(name) = misses.get(cursor.fetch_add(1, Ordering::Relaxed)) {
                    let result = client.deck_blocking(name).map_err(|e| e.to_string());
                    match &result {
                        Ok(deck) => {
                            if !cache.insert(name, deck) {
                                log::info!("Decklist {name} incomplete; not cached");
                            }
                        }
                        Err(e) => log::warn!("Decklist {name} failed: {e}"),
                    }
                    results
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push((name.clone(), result));
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    progress(LoadEvent::Decks { done: n, total });
                }
            });
        }
    });

    let mut errors = HashMap::new();
    for (name, result) in results.into_inner().unwrap_or_else(|e| e.into_inner()) {
        match result {
            Ok(deck) => {
                decks.insert(name, deck);
            }
            Err(e) => {
                errors.insert(name, e);
            }
        }
    }
    (decks, errors)
}

/// Snapshot rows for one product's EV history chart.
#[derive(Debug, Clone)]
pub struct HistoryData {
    pub id_product: u64,
    pub dates: Vec<String>,
    pub snapshots: Vec<PriceSnapshot>,
}

/// Fetches price snapshots for the deck cards and the sealed product on
/// every date (the client chunks dates and ids per request).
pub fn load_history(
    tracker: &InventorySyncClient,
    id_product: u64,
    card_ids: Vec<u64>,
    dates: Vec<String>,
) -> Result<HistoryData, String> {
    let mut ids = card_ids;
    ids.push(id_product);
    ids.sort_unstable();
    ids.dedup();
    let snapshots = tracker
        .price_snapshots_blocking(&ids, &dates)
        .map_err(|e| format!("History fetch failed: {e}"))?;
    Ok(HistoryData {
        id_product,
        dates,
        snapshots,
    })
}

#[cfg(test)]
#[path = "precon_loader_tests.rs"]
mod tests;
