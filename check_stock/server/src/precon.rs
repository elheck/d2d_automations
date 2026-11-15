//! Precon EV data on the server, shared by all users.
//!
//! "Load" runs core's `precon_loader` as a background [`Job`]: catalog and
//! prices from inventory_sync, decklists from MTGJSON (cached on disk in
//! `<data>/cache`, the desktop app's layout). When the ranking data is in,
//! the sales history follows as a second job, like on the desktop. Pages
//! compute rankings and details from the loaded data with the parameters
//! they send; nothing is fetched for that.
//!
//! Protection of the outside services:
//! - one load at a time, and the next one only [`LOAD_COOLDOWN`] after the
//!   last one ended;
//! - MTGJSON only at its fixed address; deck file names pass core's
//!   `is_safe_deck_file_name`;
//! - history charts are fetched one at a time and the last
//!   [`HISTORY_CACHE_PRODUCTS`] are kept (until the next load).

use crate::jobs::{public_message, Job, JobRun, JobStatus};
use check_stock_core::api::inventory_sync::InventorySyncClient;
use check_stock_core::api::mtgjson::MtgjsonClient;
use check_stock_core::cache::{DeckCache, SalesCache};
use check_stock_core::precon_ev::{sales_candidates, PreconData, SalesMap};
use check_stock_core::precon_loader::{
    load_history, load_precons, load_sales, local_date, HistoryData, LoadEvent, LoadOptions,
};
use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

/// Shortest time between the end of a load and the start of the next.
pub const LOAD_COOLDOWN: Duration = Duration::from_secs(30);
/// Products whose price history is kept in memory.
pub const HISTORY_CACHE_PRODUCTS: usize = 20;

/// The result of a load.
pub struct Loaded {
    pub data: PreconData,
    pub loaded_at: DateTime<Utc>,
    /// Increases with every load and when the sales history arrives, so
    /// pages know when to ask again.
    pub version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// The server was started without `--inventory-sync-url`.
    NotConfigured,
    /// A load is running already (nothing new started).
    Running,
    /// The last load ended less than [`LOAD_COOLDOWN`] ago.
    TooSoon { wait: Duration },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryError {
    NotConfigured,
    NotLoaded,
    /// The product has no (loaded) decklist.
    NoDecklist,
    /// The tracker failed; the text is for the log only.
    Unavailable(String),
}

struct HistoryCache {
    version: u64,
    entries: VecDeque<(u64, Arc<HistoryData>)>,
}

/// Precon EV state of the server.
pub struct PreconService {
    tracker: Option<InventorySyncClient>,
    mtgjson: MtgjsonClient,
    deck_cache: Arc<DeckCache>,
    sales_cache: Arc<SalesCache>,
    data: RwLock<Option<Arc<Loaded>>>,
    load_job: Arc<Job>,
    sales_job: Arc<Job>,
    /// When the last load (its ranking phase) ended, however it ended.
    last_end: Mutex<Option<Instant>>,
    cooldown: Duration,
    /// Held while a history is fetched (one at a time), then the cache.
    history: tokio::sync::Mutex<HistoryCache>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl PreconService {
    /// Caches under `cache_root`; `tracker` = the inventory_sync server.
    pub fn new(
        cache_root: &Path,
        tracker: Option<InventorySyncClient>,
        mtgjson: MtgjsonClient,
    ) -> Self {
        Self {
            tracker,
            mtgjson,
            deck_cache: Arc::new(DeckCache::in_root(cache_root)),
            sales_cache: Arc::new(SalesCache::in_root(cache_root)),
            data: RwLock::new(None),
            load_job: Arc::new(Job::default()),
            sales_job: Arc::new(Job::default()),
            last_end: Mutex::new(None),
            cooldown: LOAD_COOLDOWN,
            history: tokio::sync::Mutex::new(HistoryCache {
                version: 0,
                entries: VecDeque::new(),
            }),
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

    pub fn loaded(&self) -> Option<Arc<Loaded>> {
        self.data
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn load_status(&self) -> JobStatus {
        self.load_job.status()
    }

    pub fn sales_status(&self) -> JobStatus {
        self.sales_job.status()
    }

    /// Stops running jobs at their next check (server shutdown).
    pub fn shutdown(&self) {
        self.load_job.cancel();
        self.sales_job.cancel();
    }

    /// Starts a load (and then the sales history) in the background.
    /// `refresh_index` re-downloads MTGJSON's sealed index (~12 MB).
    pub fn start_load(self: &Arc<Self>, refresh_index: bool) -> Result<(), StartError> {
        let tracker = self.tracker.clone().ok_or(StartError::NotConfigured)?;
        {
            let last = lock(&self.last_end);
            if let Some(at) = *last {
                let since = at.elapsed();
                if since < self.cooldown && !self.load_job.status().is_running() {
                    return Err(StartError::TooSoon {
                        wait: self.cooldown - since,
                    });
                }
            }
            let Some(run) = self.load_job.start("Starting…", Utc::now()) else {
                return Err(StartError::Running);
            };
            // A sales fetch of the previous load stops at its next batch.
            self.sales_job.cancel();
            let service = self.clone();
            let opts = LoadOptions {
                tracker,
                mtgjson: self.mtgjson.clone(),
                force_index_refresh: refresh_index,
                now_unix: Utc::now().timestamp(),
            };
            tokio::task::spawn_blocking(move || service.run_load(run, opts));
        }
        Ok(())
    }

    /// The load job (blocking thread).
    fn run_load(self: Arc<Self>, run: JobRun, opts: LoadOptions) {
        let progress = |event: LoadEvent| match event {
            LoadEvent::Stage(stage) => run.stage(&stage),
            LoadEvent::Decks { done, total } => {
                run.progress("Loading decklists…", done as u64, total as u64)
            }
            LoadEvent::Sales { done, total } => {
                run.progress("Sales history", done as u64, total as u64)
            }
        };
        let loaded = load_precons(&opts, &self.deck_cache, &progress);
        // The cooldown counts from here (a slow load must not allow an
        // immediate reload).
        *lock(&self.last_end) = Some(Instant::now());
        let data = match loaded {
            Ok(data) => data,
            Err(e) => {
                log::warn!("Precon load failed: {e}");
                run.fail(&public_message(&e), Utc::now());
                return;
            }
        };
        if run.is_cancelled() {
            run.cancelled(Utc::now());
            return;
        }
        let candidates = sales_candidates(&data.decks, &data.prices);
        let version = self.store(data);
        run.finish(Utc::now());
        log::info!("Precon EV: loaded (version {version}); fetching sales history");

        let sales_run = self
            .sales_job
            .replace("Fetching sales history…", Utc::now());
        let today = local_date(opts.now_unix);
        let progress = |event: LoadEvent| {
            if let LoadEvent::Sales { done, total } = event {
                sales_run.progress("Sales history", done as u64, total as u64);
            }
        };
        let result = load_sales(
            &opts.tracker,
            &self.sales_cache,
            &candidates,
            today,
            &progress,
            sales_run.cancel_flag(),
        );
        match result {
            Ok(map) => {
                self.attach_sales(version, Ok(map));
                sales_run.finish(Utc::now());
            }
            Err(_) if sales_run.is_cancelled() => sales_run.cancelled(Utc::now()),
            Err(e) => {
                log::warn!("Precon sales history failed: {e}");
                let message = public_message(&e);
                self.attach_sales(version, Err(message.clone()));
                sales_run.fail(&message, Utc::now());
            }
        }
    }

    /// Makes `data` the current data; returns its version.
    fn store(&self, data: PreconData) -> u64 {
        let mut slot = self.data.write().unwrap_or_else(PoisonError::into_inner);
        let version = slot.as_ref().map_or(0, |l| l.version) + 1;
        *slot = Some(Arc::new(Loaded {
            data,
            loaded_at: Utc::now(),
            version,
        }));
        version
    }

    /// Adds the sales history to the data of `version` (unless a newer load
    /// replaced it meanwhile).
    fn attach_sales(&self, version: u64, sales: Result<SalesMap, String>) {
        let current = self.loaded();
        let Some(current) = current.filter(|l| l.version == version) else {
            return;
        };
        let mut data = current.data.clone();
        match sales {
            Ok(map) => {
                data.sales = Some(map);
                data.sales_error = None;
            }
            Err(e) => {
                data.sales = None;
                data.sales_error = Some(e);
            }
        }
        let mut slot = self.data.write().unwrap_or_else(PoisonError::into_inner);
        if slot.as_ref().is_some_and(|l| l.version == version) {
            *slot = Some(Arc::new(Loaded {
                data,
                loaded_at: current.loaded_at,
                version: version + 1,
            }));
        }
    }

    /// Price snapshots for a product's EV history, for the loaded data.
    pub async fn history(
        &self,
        id_product: u64,
        today: chrono::NaiveDate,
    ) -> Result<(Arc<Loaded>, Arc<HistoryData>), HistoryError> {
        let tracker = self.tracker.clone().ok_or(HistoryError::NotConfigured)?;
        let loaded = self.loaded().ok_or(HistoryError::NotLoaded)?;
        let (card_ids, dates) = loaded
            .data
            .history_request(id_product, today)
            .ok_or(HistoryError::NoDecklist)?;
        let mut cache = self.history.lock().await;
        if cache.version != loaded.version {
            cache.version = loaded.version;
            cache.entries.clear();
        }
        if let Some((_, history)) = cache
            .entries
            .iter()
            .find(|(id, h)| *id == id_product && h.dates == dates)
        {
            return Ok((loaded.clone(), history.clone()));
        }
        // The lock stays held while fetching: one history fetch at a time.
        let fetched = tokio::task::spawn_blocking(move || {
            load_history(&tracker, id_product, card_ids, dates)
        })
        .await
        .map_err(|e| HistoryError::Unavailable(e.to_string()))?
        .map_err(HistoryError::Unavailable)?;
        let history = Arc::new(fetched);
        if cache.entries.len() >= HISTORY_CACHE_PRODUCTS {
            cache.entries.pop_front();
        }
        cache.entries.retain(|(id, _)| *id != id_product);
        cache.entries.push_back((id_product, history.clone()));
        Ok((loaded, history))
    }
}

#[cfg(test)]
#[path = "precon_tests.rs"]
pub(crate) mod tests;
