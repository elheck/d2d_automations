//! Market data of the stock, shared by all users: the latest Cardmarket
//! prices and the prices on six dates (today, 7, 14, 30, 60 and 90 days ago)
//! from inventory_sync, for every card in stock. Price movers (and later the
//! mispricing report) compute from it; nothing is fetched per request.
//!
//! Loading is a background [`Job`] that reads the stock from the database
//! and then asks the tracker (two requests, chunked by the client). A load
//! without `refresh` does nothing when today's data is already there; a new
//! load starts at the earliest [`LOAD_COOLDOWN`] after the last one ended.

use crate::db::Db;
use crate::jobs::{public_message, Job, JobRun, JobStatus};
use check_stock_core::api::inventory_sync::{InventorySyncClient, LatestPrice};
use check_stock_core::category::DEFAULT_CATEGORY;
use check_stock_core::inventory_db::{get_in_stock_cards_conn, InStockCard};
use check_stock_core::precon_loader::local_date;
use check_stock_core::price_trends::SnapshotSet;
use chrono::{DateTime, NaiveDate, Utc};
use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

/// Shortest time between the end of a load and the start of the next.
pub const LOAD_COOLDOWN: Duration = Duration::from_secs(30);
/// Pause before the one retry of a failed tracker request.
const RETRY_DELAY: Duration = Duration::from_millis(500);
/// Products per price-history request. The tracker holds its single database
/// lock for a whole request and the rows are scattered, so one request for
/// the whole stock (11k products × 6 dates) took over 30 s and its answer
/// was sometimes cut off; small portions stay quick and show progress.
pub const HISTORY_CHUNK: usize = 1000;
/// Pause between two history requests, so other tracker clients get through.
const HISTORY_PAUSE: Duration = Duration::from_millis(50);

/// The loaded market data.
pub struct MarketData {
    /// The stock at the time of the load (Magic singles category).
    pub cards: Vec<InStockCard>,
    pub latest: HashMap<u64, LatestPrice>,
    pub snapshots: SnapshotSet,
    /// The day the data is for (server's local date).
    pub today: NaiveDate,
    pub loaded_at: DateTime<Utc>,
    /// Changes with every load.
    pub version: u64,
    /// Distinct Cardmarket products asked for.
    pub products: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    NotConfigured,
    Running,
    TooSoon { wait: Duration },
}

pub struct MarketService {
    tracker: Option<InventorySyncClient>,
    data: RwLock<Option<Arc<MarketData>>>,
    job: Arc<Job>,
    last_end: Mutex<Option<Instant>>,
    cooldown: Duration,
    /// The stock changed (an import) since the last load started.
    stock_changed: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One retry: a dropped answer (thousands of rows) should not cost the load.
fn retry_once<T>(what: &str, mut f: impl FnMut() -> Result<T, String>) -> Result<T, String> {
    f().or_else(|e| {
        log::warn!("{what} failed, retrying once: {e}");
        std::thread::sleep(RETRY_DELAY);
        f()
    })
}

impl MarketService {
    pub fn new(tracker: Option<InventorySyncClient>) -> Self {
        Self {
            tracker,
            data: RwLock::new(None),
            job: Arc::new(Job::default()),
            last_end: Mutex::new(None),
            cooldown: LOAD_COOLDOWN,
            stock_changed: AtomicBool::new(false),
        }
    }

    /// Another cooldown (tests).
    pub fn with_cooldown(mut self, cooldown: Duration) -> Self {
        self.cooldown = cooldown;
        self
    }

    pub fn is_configured(&self) -> bool {
        self.tracker.is_some()
    }

    pub fn loaded(&self) -> Option<Arc<MarketData>> {
        self.data
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn status(&self) -> JobStatus {
        self.job.status()
    }

    pub fn shutdown(&self) {
        self.job.cancel();
    }

    /// The stock changed (an inventory import): the loaded data stays
    /// visible, but the next [`MarketService::start_load`] loads again.
    pub fn stock_changed(&self) {
        self.stock_changed.store(true, Ordering::SeqCst);
    }

    /// Whether the stock changed since the loaded data was read.
    pub fn is_outdated(&self) -> bool {
        self.stock_changed.load(Ordering::SeqCst)
    }

    /// Starts a load unless today's data for the current stock is there
    /// (then `Ok(false)`; `refresh` loads anyway).
    pub fn start_load(self: &Arc<Self>, db: Db, refresh: bool) -> Result<bool, StartError> {
        let tracker = self.tracker.clone().ok_or(StartError::NotConfigured)?;
        let today = local_date(Utc::now().timestamp());
        if !refresh && !self.is_outdated() && self.loaded().is_some_and(|d| d.today == today) {
            return Ok(false);
        }
        if let Some(at) = *lock(&self.last_end) {
            let since = at.elapsed();
            if since < self.cooldown && !self.job.status().is_running() {
                return Err(StartError::TooSoon {
                    wait: self.cooldown - since,
                });
            }
        }
        let Some(run) = self.job.start("Reading the stock…", Utc::now()) else {
            return Err(StartError::Running);
        };
        // Cleared before the stock is read: an import after this point
        // marks the new data outdated again.
        self.stock_changed.store(false, Ordering::SeqCst);
        let service = self.clone();
        tokio::spawn(async move {
            let cards = db
                .read(|conn| get_in_stock_cards_conn(conn, DEFAULT_CATEGORY))
                .await;
            let _ =
                tokio::task::spawn_blocking(move || service.run_load(run, tracker, cards, today))
                    .await;
        });
        Ok(true)
    }

    /// The fetch part of a load (blocking thread).
    fn run_load(
        &self,
        run: JobRun,
        tracker: InventorySyncClient,
        cards: Result<Vec<InStockCard>, crate::db::DbError>,
        today: NaiveDate,
    ) {
        let result = (|| -> Result<Option<MarketData>, String> {
            let cards = cards.map_err(|e| format!("Reading the stock failed: {e}"))?;
            let ids: Vec<u64> = cards
                .iter()
                .filter_map(|c| c.cardmarket_id.trim().parse::<u64>().ok())
                .collect::<BTreeSet<u64>>()
                .into_iter()
                .collect();
            if ids.is_empty() {
                return Err("No cards in stock with a Cardmarket id".to_string());
            }
            run.stage(&format!(
                "Fetching latest prices for {} products…",
                ids.len()
            ));
            let latest = retry_once("Latest prices", || {
                tracker
                    .latest_prices_blocking(&ids)
                    .map_err(|e| format!("Price fetch failed: {e}"))
            })?;
            if run.is_cancelled() {
                return Ok(None);
            }
            let dates = SnapshotSet::request_dates(today);
            let mut snapshots = Vec::new();
            run.progress("Fetching the price history (90 days)", 0, ids.len() as u64);
            for (i, chunk) in ids.chunks(HISTORY_CHUNK).enumerate() {
                if run.is_cancelled() {
                    return Ok(None);
                }
                if i > 0 {
                    std::thread::sleep(HISTORY_PAUSE);
                }
                snapshots.extend(retry_once("Price history", || {
                    tracker
                        .price_snapshots_blocking(chunk, &dates)
                        .map_err(|e| format!("Price history fetch failed: {e}"))
                })?);
                let done = (i * HISTORY_CHUNK + chunk.len()) as u64;
                run.progress(
                    "Fetching the price history (90 days)",
                    done,
                    ids.len() as u64,
                );
            }
            if run.is_cancelled() {
                return Ok(None);
            }
            Ok(Some(MarketData {
                products: ids.len(),
                cards,
                latest: latest.into_iter().map(|p| (p.id_product, p)).collect(),
                snapshots: SnapshotSet::new(&dates, snapshots),
                today,
                loaded_at: Utc::now(),
                version: 0,
            }))
        })();
        *lock(&self.last_end) = Some(Instant::now());
        match result {
            Ok(Some(mut data)) => {
                let mut slot = self.data.write().unwrap_or_else(PoisonError::into_inner);
                data.version = slot.as_ref().map_or(0, |d| d.version) + 1;
                log::info!(
                    "Market data loaded: {} cards, {} products (version {})",
                    data.cards.len(),
                    data.products,
                    data.version
                );
                *slot = Some(Arc::new(data));
                drop(slot);
                run.finish(Utc::now());
            }
            Ok(None) => run.cancelled(Utc::now()),
            Err(e) => {
                log::warn!("Market data load failed: {e}");
                run.fail(&public_message(&e), Utc::now());
            }
        }
    }
}

#[cfg(test)]
#[path = "market_tests.rs"]
pub(crate) mod tests;
