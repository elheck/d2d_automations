use super::language::Language;
use crate::models::Card;
use crate::stock_analysis::SortOrder;
use eframe::egui;

type CardMatch = (Card, i32, String);
/// (card_name, needed_quantity, matched_cards)
type CardMatchGroup = (String, i32, Vec<CardMatch>);

#[derive(PartialEq)]
pub enum Screen {
    Welcome,
    StockChecker,
    StockAnalysis,
    BinAnalysis,
    StockListing,
    Search,
    Picking,
    Pricing,
    BuyHelper,
    Mispricing,
    Movers,
    Consolidation,
    Restock,
    PreconEv,
    BulkLot,
}

#[derive(PartialEq)]
pub enum OutputFormat {
    PickingList,
    InvoiceList,
    UpdateStock,
}

impl OutputFormat {
    pub fn title(&self) -> &'static str {
        match self {
            OutputFormat::PickingList => "Picking List",
            OutputFormat::InvoiceList => "Invoice List",
            OutputFormat::UpdateStock => "Stock Update",
        }
    }
}

pub struct AppState {
    pub discount_percent: f32, // 0.0 to 100.0
    pub current_screen: Screen,
    pub inventory_path: String,
    pub wantslist_path: String,
    pub output: String,
    pub preferred_language: Language,
    pub preferred_language_only: bool,
    pub all_matches: Vec<CardMatchGroup>,
    pub selected: Vec<bool>,
    pub show_selection: bool,
    pub selection_mode: bool,
    pub show_output_window: bool,
    pub output_window_content: String,
    pub output_window_title: String,
    // ── Inventory Sync connection (shared by all screens) ─────────────────
    pub inventory_sync_url: String,
    pub inventory_sync_status: ConnectionStatus,
    /// Receives the health-check result from a background thread.
    pub inventory_health_rx: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    /// A CSV import the safety check blocked, awaiting user confirmation.
    /// Rendered as a modal by the app shell on every screen.
    pub sync_guard: Option<SyncGuard>,
    /// Outcome of the most recent inventory sync, shown as a banner by the app
    /// shell until dismissed. Syncs are triggered from several screens (and can
    /// be confirmed from the guard modal, which is not on any screen), so the
    /// result is held here rather than in a per-screen state.
    pub sync_report: Option<SyncReport>,
    /// Since-last-visit digest, computed once per app run on the welcome
    /// screen (`None` = not yet computed; `Err` = DB unavailable).
    pub digest: Option<Result<crate::inventory_db::VisitDigest, String>>,
    /// Bumped on every write to the inventory DB. Screens that cache derived
    /// data (stats, restock rows) compare it against the generation they last
    /// loaded and reload when it moves. Without this a sync confirmed from the
    /// guard modal would leave those screens showing pre-import numbers, since
    /// the write happens outside the screen that triggered it.
    pub db_generation: u64,
}

/// A blocked inventory import held for the confirmation dialog.
pub struct SyncGuard {
    /// The loaded CSV rows the user may still choose to sync.
    pub cards: Vec<Card>,
    /// The product category those rows belong to — a forced sync must apply to
    /// the same scope the blocked one was previewed against.
    pub category: String,
    /// What the sync would have changed.
    pub preview: crate::inventory_db::SyncPreview,
}

/// Result of the last inventory sync, in a form the GUI can render directly.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncReport {
    /// Product category the sync was scoped to.
    pub category: String,
    /// Variants inserted or updated from the CSV.
    pub upserted: usize,
    /// In-stock variants missing from the CSV that were set to quantity 0.
    pub zeroed: usize,
    /// Copies whose drop was a bulk lot's removal (not counted as sold).
    pub bulk_removed: i64,
    /// Set when the sync failed; `upserted`/`zeroed` are then meaningless.
    pub error: Option<String>,
    /// True when the user overrode a blocked sync from the guard modal.
    pub forced: bool,
}

impl SyncReport {
    /// One-line summary for the banner.
    pub fn summary(&self) -> String {
        match &self.error {
            Some(e) => format!("Inventory sync failed ({}): {e}", self.category),
            None => {
                let mut summary = format!(
                    "{} sync ({}): {} variant{} updated, {} zeroed out",
                    if self.forced {
                        "Forced inventory"
                    } else {
                        "Inventory"
                    },
                    self.category,
                    self.upserted,
                    if self.upserted == 1 { "" } else { "s" },
                    self.zeroed
                );
                if self.bulk_removed > 0 {
                    summary.push_str(&format!(
                        ", {} bulk-lot cop{} removed (not sales)",
                        self.bulk_removed,
                        if self.bulk_removed == 1 { "y" } else { "ies" }
                    ));
                }
                summary
            }
        }
    }
}

impl AppState {
    /// Syncs a freshly loaded inventory CSV into the local DB, scoped to the
    /// category the CSV belongs to. When the safety check blocks the import (it
    /// would record most of that category's inventory as sold), the CSV is
    /// parked in [`AppState::sync_guard`] for the confirmation modal instead;
    /// nothing is written until confirmed.
    pub fn sync_inventory_guarded(&mut self, cards: &[Card], category: &str) {
        match crate::inventory_db::sync_inventory(cards, category) {
            Ok(crate::inventory_db::SyncOutcome::Synced(stats)) => {
                self.sync_report = Some(SyncReport {
                    category: category.to_string(),
                    upserted: stats.upserted,
                    zeroed: stats.zeroed,
                    bulk_removed: stats.bulk_removed,
                    error: None,
                    forced: false,
                });
                self.mark_db_changed();
            }
            Ok(crate::inventory_db::SyncOutcome::Blocked(preview)) => {
                // No banner: the guard modal is the feedback, and confirming it
                // produces its own report.
                self.sync_report = None;
                self.sync_guard = Some(SyncGuard {
                    cards: cards.to_vec(),
                    category: category.to_string(),
                    preview,
                });
            }
            Err(e) => {
                log::warn!("Inventory DB sync failed: {e}");
                self.sync_report = Some(SyncReport {
                    category: category.to_string(),
                    upserted: 0,
                    zeroed: 0,
                    bulk_removed: 0,
                    error: Some(e.to_string()),
                    forced: false,
                });
            }
        }
    }

    /// Applies a sync the safety guard had blocked, after the user confirmed it.
    ///
    /// Goes through `AppState` rather than calling `inventory_db` directly so the
    /// write is recorded in [`AppState::db_generation`] — the confirmation modal
    /// lives in the app shell, not in the screen whose cached data just went
    /// stale, so nothing else would invalidate it.
    pub fn sync_inventory_confirmed(&mut self, cards: &[Card], category: &str) {
        match crate::inventory_db::sync_inventory_forced(cards, category) {
            Ok(stats) => {
                log::info!(
                    "Forced inventory sync applied ({}): {} upserted, {} zeroed",
                    category,
                    stats.upserted,
                    stats.zeroed
                );
                self.sync_report = Some(SyncReport {
                    category: category.to_string(),
                    upserted: stats.upserted,
                    zeroed: stats.zeroed,
                    bulk_removed: stats.bulk_removed,
                    error: None,
                    forced: true,
                });
                self.mark_db_changed();
            }
            Err(e) => {
                log::warn!("Forced inventory sync failed: {e}");
                self.sync_report = Some(SyncReport {
                    category: category.to_string(),
                    upserted: 0,
                    zeroed: 0,
                    bulk_removed: 0,
                    error: Some(e.to_string()),
                    forced: true,
                });
            }
        }
    }

    /// Records that the inventory DB changed, so caching screens reload.
    pub fn mark_db_changed(&mut self) {
        self.db_generation = self.db_generation.wrapping_add(1);
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            current_screen: Screen::Welcome,
            inventory_path: String::new(),
            wantslist_path: String::new(),
            output: String::new(),
            preferred_language: Language::English,
            preferred_language_only: false,
            all_matches: Vec::new(),
            selected: Vec::new(),
            show_selection: false,
            selection_mode: false,
            show_output_window: false,
            output_window_content: String::new(),
            output_window_title: String::new(),
            discount_percent: 10.0,
            inventory_sync_url: "http://cardscanner.local:3000".to_string(),
            inventory_sync_status: ConnectionStatus::Unchecked,
            inventory_health_rx: None,
            sync_guard: None,
            sync_report: None,
            digest: None,
            db_generation: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Default)]
pub enum LotSortColumn {
    #[default]
    Lot,
    InStock,
    Copies,
    StockValue,
    Sold,
    Revenue,
    Cost,
    Margin,
}

#[derive(Default)]
pub struct StockAnalysisState {
    pub inventory_path: String,
    pub db_stats: Option<crate::inventory_db::DbStats>,
    pub db_stats_error: Option<String>,
    /// Set to true after the first stats load attempt so we don't query on every frame.
    pub stats_loaded: bool,
    /// [`AppState::db_generation`] the cached stats were loaded at. A mismatch
    /// means the DB was written since (e.g. a sync confirmed from the guard
    /// modal) and the figures on screen — including the lot table — are stale.
    pub stats_generation: u64,
    pub lot_sort_column: LotSortColumn,
    pub lot_sort_ascending: bool,
    /// Lot whose acquisition cost is currently being edited, plus the raw text in
    /// its input field. `None` when no cost cell is in edit mode.
    pub lot_cost_edit: Option<(String, String)>,
    /// Surfaced when saving or clearing a lot cost fails; shown near the table.
    pub lot_cost_error: Option<String>,
}

pub struct BinAnalysisState {
    pub inventory_path: String,
    pub output: String,
    pub free_slots: i32,
    pub sort_order: SortOrder,
    /// Highest bin load (cards) still treated as "sparse" for consolidation.
    pub merge_threshold: i32,
    /// Human-readable consolidation report shown in the UI.
    pub consolidation_output: String,
    /// Moves from the latest sparse-bin plan, used to open the interactive list.
    pub consolidation_moves: Vec<crate::bin_consolidation::Move>,
    /// Latest fragmented-variant report (independent of bin fill).
    pub fragmented: Vec<crate::bin_consolidation::FragmentedVariant>,
    /// Report text for the fragmented-variant section.
    pub fragmented_output: String,
    /// Moves that consolidate each fragmented variant into one bin.
    pub defrag_moves: Vec<crate::bin_consolidation::Move>,
}

impl Default for BinAnalysisState {
    fn default() -> Self {
        Self {
            inventory_path: String::new(),
            output: String::new(),
            free_slots: 5,
            sort_order: SortOrder::ByFreeSlots,
            merge_threshold: 20,
            consolidation_output: String::new(),
            consolidation_moves: Vec::new(),
            fragmented: Vec::new(),
            fragmented_output: String::new(),
            defrag_moves: Vec::new(),
        }
    }
}

use crate::api::cardmarket::PriceGuide;
use crate::api::scryfall::ScryfallCard;
use crate::cache::{CardCache, ImageCache};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

/// Which input field to focus next (consumed after use)
#[derive(Default, PartialEq, Clone, Copy)]
pub enum FocusRequest {
    #[default]
    None,
    Card,
    Quantity,
}

/// Result of a background card + image fetch.
pub struct CardFetchResult {
    pub set_code: String,
    pub collector_number: String,
    pub card: ScryfallCard,
    pub image_bytes: Option<Vec<u8>>,
}

pub enum CardFetchMessage {
    Success(Box<CardFetchResult>),
    Error(String),
}

pub enum PriceGuideMessage {
    Success(PriceGuide),
    Error(String),
}

pub struct StockListingState {
    pub default_set: String,         // Default set code, e.g. "hou"
    pub default_language: String,    // Default language, e.g. "EN"
    pub card_input: String,          // Collector number or set+number, e.g. "120" or "hou120"
    pub quantity_input: String,      // Quantity of cards (defaults to "1")
    pub focus_request: FocusRequest, // Request focus on next frame (consumed after use)
    pub card: Option<ScryfallCard>,
    pub card_image: Option<egui::TextureHandle>,
    pub image_loading: bool,
    pub error: Option<String>,
    pub price_guide: Option<PriceGuide>,
    pub price_guide_loading: bool,
    pub card_cache: CardCache,
    pub image_cache: ImageCache,
    // Async runtime + channels — private, mirroring the PickingState pattern
    pub(super) runtime: Runtime,
    pub(super) card_tx: UnboundedSender<CardFetchMessage>,
    pub(super) card_rx: UnboundedReceiver<CardFetchMessage>,
    pub(super) price_guide_tx: UnboundedSender<PriceGuideMessage>,
    pub(super) price_guide_rx: UnboundedReceiver<PriceGuideMessage>,
}

impl Default for StockListingState {
    fn default() -> Self {
        let (card_tx, card_rx) = unbounded_channel();
        let (price_guide_tx, price_guide_rx) = unbounded_channel();
        Self {
            default_set: String::new(),
            default_language: String::from("EN"),
            card_input: String::new(),
            quantity_input: String::from("1"),
            focus_request: FocusRequest::Card, // Start with focus on card input
            card: None,
            card_image: None,
            image_loading: false,
            error: None,
            price_guide: None,
            price_guide_loading: false,
            card_cache: CardCache::load(), // Load from disk on startup
            image_cache: ImageCache::new(),
            runtime: Runtime::new().expect("Failed to create Tokio runtime for StockListing"),
            card_tx,
            card_rx,
            price_guide_tx,
            price_guide_rx,
        }
    }
}

pub struct SelectedSearchCard {
    pub card: Card,
    pub quantity: i32,
}

/// What the "Selected Cards" action performs on the current selection.
#[derive(PartialEq, Clone, Copy)]
pub enum SearchAction {
    /// Send the selection to the Stock Checker lists (default).
    AddToLists,
    /// Write the selection off as discarded: reduce inventory without counting
    /// it as sold, and export a negative-delta stock-update CSV.
    Discard,
}

/// Floating per-card price-history window on the Search screen
/// (data fetched from inventory_sync).
#[derive(Default)]
pub struct CardHistoryState {
    pub open: bool,
    /// Card name shown in the window title.
    pub title: String,
    /// Whether the foil price columns should be charted.
    pub is_foil: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub data: Option<crate::api::inventory_sync::PriceData>,
    /// Receiver for the background history fetch, if one is in flight.
    pub rx:
        Option<std::sync::mpsc::Receiver<Result<crate::api::inventory_sync::PriceData, String>>>,
}

pub struct SearchState {
    pub csv_path: String,
    pub search_term: String,
    pub last_search_term: String,
    pub cards: Vec<Card>,
    pub filtered_cards: Vec<Card>,
    pub search_case_sensitive: bool,
    pub search_in_all_languages: bool,
    pub selected_fields: SearchFields,
    pub last_search_time: std::time::Instant,
    pub search_needs_update: bool,
    pub current_page: usize,
    pub results_per_page: usize,
    pub selected_cards: Vec<SelectedSearchCard>,
    pub quantity_inputs: std::collections::HashMap<usize, i32>,
    /// Whether the selection is sent to lists or written off as discarded.
    pub action_mode: SearchAction,
    /// The "import order matters" warning is open; saving the discard CSV
    /// waits for the user to confirm it.
    pub discard_confirm: bool,
    /// Floating per-card price-history window.
    pub history: CardHistoryState,
}

#[derive(Default)]
pub struct SearchFields {
    pub name: bool,
    pub set: bool,
    pub condition: bool,
    pub language: bool,
    pub location: bool,
    pub rarity: bool,
    pub price: bool,
    pub comment: bool,
}

// ── Inventory Sync connection ─────────────────────────────────────────────────

/// Connection status for the inventory_sync server.
pub enum ConnectionStatus {
    /// No check has been performed yet.
    Unchecked,
    /// A health-check request is in flight.
    Checking,
    /// Server responded successfully.
    Connected,
    /// Check failed with an error message.
    Failed(String),
}

// The price-field selector and latest-price row are shared wire types from
// mtg_common; re-exported under the names the UI code already uses.
pub use crate::api::inventory_sync::{LatestPrice, PriceField as InventoryPriceSource};

/// State for the Card Buy Helper screen.
///
/// Holds the loaded card export plus the adjustable offer parameters. This
/// screen is read-only: cards are only held in memory to compute an offer and
/// are **never** written to the inventory database.
pub struct BuyHelperState {
    pub csv_path: String,
    pub cards: Vec<crate::models::Card>,
    pub load_error: Option<String>,
    // ── Rarity toggles: which rarities count as individually-valued singles ──
    pub single_common: bool,
    pub single_uncommon: bool,
    pub single_rare: bool,
    pub single_mythic: bool,
    // ── Price threshold rule ────────────────────────────────────────────────
    pub use_min_price: bool,
    pub min_price: f64,
    // ── Offer rates ─────────────────────────────────────────────────────────
    pub single_buy_percent: f64,
    pub bulk_rate: f64,
    pub bulk_batch: u32,
    // ── Detailed CSV output window ──────────────────────────────────────────
    pub show_output_window: bool,
    pub output_content: String,
}

impl Default for BuyHelperState {
    fn default() -> Self {
        let defaults = crate::buy_helper::BuyParams::default();
        Self {
            csv_path: String::new(),
            cards: Vec::new(),
            load_error: None,
            single_common: defaults.single_common,
            single_uncommon: defaults.single_uncommon,
            single_rare: defaults.single_rare,
            single_mythic: defaults.single_mythic,
            use_min_price: defaults.min_single_price.is_some(),
            min_price: defaults.min_single_price.unwrap_or(0.50),
            single_buy_percent: defaults.single_buy_percent,
            bulk_rate: defaults.bulk_rate,
            bulk_batch: defaults.bulk_batch,
            show_output_window: false,
            output_content: String::new(),
        }
    }
}

impl BuyHelperState {
    /// Build the pure [`BuyParams`](crate::buy_helper::BuyParams) from the
    /// current UI field values.
    pub fn params(&self) -> crate::buy_helper::BuyParams {
        crate::buy_helper::BuyParams {
            single_common: self.single_common,
            single_uncommon: self.single_uncommon,
            single_rare: self.single_rare,
            single_mythic: self.single_mythic,
            min_single_price: self.use_min_price.then_some(self.min_price),
            single_buy_percent: self.single_buy_percent,
            bulk_rate: self.bulk_rate,
            bulk_batch: self.bulk_batch,
        }
    }
}

// ── Mispricing screen ─────────────────────────────────────────────────────────

/// Which column the mispricing table is sorted by.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum MispricingSort {
    /// Combined "what to fix first" score (impact × urgency × confidence) —
    /// see [`crate::mispricing::priority_score`].
    #[default]
    Priority,
    /// Absolute money impact of the mispricing: |delta_abs| × quantity.
    Impact,
    DeltaPct,
    Listed,
    Market,
    /// Market movement over the last 7 days (from inventory_sync snapshots).
    Change7,
    /// Market movement over the last 30 days (from inventory_sync snapshots).
    Change30,
    /// Days since the card was listed.
    Age,
    Name,
    Set,
    Quantity,
    /// Verdict rank (underpriced → overpriced → fair → no data).
    Verdict,
}

/// Where the mispricing market reference prices come from.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum MarketSource {
    /// Latest prices collected daily by the inventory_sync server.
    #[default]
    InventorySync,
    /// Full Cardmarket price-guide download (~50 MB, point-in-time).
    PriceGuide,
}

impl MarketSource {
    pub fn as_str(self) -> &'static str {
        match self {
            MarketSource::InventorySync => "Inventory Sync",
            MarketSource::PriceGuide => "Price guide download",
        }
    }
    pub fn all() -> &'static [Self] {
        &[Self::InventorySync, Self::PriceGuide]
    }
}

/// Message from a background inventory_sync fetch: streamed progress updates
/// while the fetch runs, then exactly one final result.
pub enum FetchMsg<T> {
    /// Human-readable description of the stage the fetch is currently in.
    Progress(String),
    Done(Result<T, String>),
}

/// Payload of a successful background fetch from inventory_sync for the
/// mispricing screen: latest prices, raw snapshot rows, and the snapshot
/// dates they were requested for.
pub type MispricingSyncData = (
    Vec<LatestPrice>,
    Vec<crate::api::inventory_sync::PriceSnapshot>,
    crate::price_trends::SnapshotDates,
);

/// Verdict filter applied to the mispricing table.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum VerdictFilter {
    #[default]
    All,
    Underpriced,
    Overpriced,
    NoData,
}

impl VerdictFilter {
    pub fn as_str(self) -> &'static str {
        match self {
            VerdictFilter::All => "All",
            VerdictFilter::Underpriced => "Underpriced",
            VerdictFilter::Overpriced => "Overpriced",
            VerdictFilter::NoData => "No data",
        }
    }
}

/// State for the read-only Mispricing / Margin report screen.
pub struct MispricingState {
    /// Half-width of the "fair" price band, in percent.
    pub threshold_pct: f64,
    /// Which price-guide field to treat as the market reference.
    pub ref_source: InventoryPriceSource,
    /// Which source supplies the market reference prices.
    pub source: MarketSource,
    /// Loaded Cardmarket price guide (from CDN fetch or a local file).
    pub price_guide: Option<PriceGuide>,
    /// Human-readable status of the price-guide acquisition.
    pub guide_status: String,
    pub guide_loading: bool,
    /// Path used by the "load from file" picker.
    pub guide_path: String,
    /// The computed report, if analysis has been run.
    pub report: Option<crate::mispricing::MispricingReport>,
    /// Internal contradictions between our own listings (no market data
    /// needed); recomputed together with the report.
    pub consistency: Vec<crate::consistency::ConsistencyIssue>,
    pub error: Option<String>,
    pub sort: MispricingSort,
    pub sort_desc: bool,
    pub filter: VerdictFilter,
    /// Receiver for the background price-guide fetch, if one is in flight.
    pub guide_rx: Option<std::sync::mpsc::Receiver<Result<PriceGuide, String>>>,
    // ── Inventory Sync data ─────────────────────────────────────────────────
    /// Latest prices from inventory_sync, keyed by cardmarket product ID.
    pub inventory_prices: std::collections::HashMap<u64, LatestPrice>,
    /// Snapshot rows backing the Δ7d / Δ30d columns.
    pub snapshots: crate::price_trends::SnapshotSet,
    /// Human-readable status of the last inventory_sync fetch.
    pub sync_status: String,
    pub sync_loading: bool,
    /// Receiver for the background inventory_sync fetch, if one is in flight.
    /// Streams progress messages, then the final result.
    pub sync_rx: Option<std::sync::mpsc::Receiver<FetchMsg<MispricingSyncData>>>,
}

impl Default for MispricingState {
    fn default() -> Self {
        Self {
            threshold_pct: 15.0,
            ref_source: InventoryPriceSource::Trend,
            source: MarketSource::default(),
            price_guide: None,
            guide_status: String::new(),
            guide_loading: false,
            guide_path: String::new(),
            report: None,
            consistency: Vec::new(),
            error: None,
            sort: MispricingSort::default(),
            sort_desc: true,
            filter: VerdictFilter::default(),
            guide_rx: None,
            inventory_prices: std::collections::HashMap::new(),
            snapshots: crate::price_trends::SnapshotSet::default(),
            sync_status: String::new(),
            sync_loading: false,
            sync_rx: None,
        }
    }
}

// ── Price Movers screen ───────────────────────────────────────────────────────

/// Direction filter for the movers table.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum MoverDirection {
    #[default]
    All,
    Risers,
    Fallers,
}

impl MoverDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            MoverDirection::All => "All",
            MoverDirection::Risers => "Risers",
            MoverDirection::Fallers => "Fallers",
        }
    }
}

/// Which column the movers table is sorted by.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum MoverSort {
    /// Market movement over the last 7 days.
    #[default]
    Change7,
    /// Market movement over the last 30 days.
    Change30,
    /// Days since the card was listed.
    Age,
    /// Current market value of the reference field.
    Current,
    Listed,
    Name,
    Set,
    Quantity,
    Location,
}

/// Payload of a successful background snapshot fetch for the movers screen:
/// raw snapshot rows plus the three dates they were requested for.
pub type MoversSyncData = (
    Vec<crate::api::inventory_sync::PriceSnapshot>,
    crate::price_trends::SnapshotDates,
);

/// State for the read-only Price Movers screen (stock × market movement).
pub struct MoversState {
    /// In-stock cards loaded from the inventory DB at fetch time.
    pub cards: Vec<crate::inventory_db::InStockCard>,
    /// Raw snapshot rows from inventory_sync, indexed for delta lookup.
    pub snapshots: crate::price_trends::SnapshotSet,
    /// Joined + filtered rows, rebuilt after every fetch or parameter change.
    pub movers: Vec<crate::price_trends::StockMover>,
    /// Which price field drives the movement columns.
    pub field: InventoryPriceSource,
    /// Cards whose current field value is below this are hidden (bulk noise).
    pub min_price: f64,
    /// Only show cards at least this old (0 = everyone); pairs movement with
    /// the aging report to find dead stock that is also losing value.
    pub min_age_days: i64,
    pub direction: MoverDirection,
    pub sort: MoverSort,
    pub sort_desc: bool,
    /// Human-readable status of the last fetch.
    pub status: String,
    pub error: Option<String>,
    pub loading: bool,
    /// Receiver for the background snapshot fetch, if one is in flight.
    pub rx: Option<std::sync::mpsc::Receiver<Result<MoversSyncData, String>>>,
}

impl Default for MoversState {
    fn default() -> Self {
        Self {
            cards: Vec::new(),
            snapshots: crate::price_trends::SnapshotSet::default(),
            movers: Vec::new(),
            field: InventoryPriceSource::Trend,
            min_price: 0.30,
            min_age_days: 0,
            direction: MoverDirection::default(),
            sort: MoverSort::default(),
            sort_desc: true,
            status: String::new(),
            error: None,
            loading: false,
            rx: None,
        }
    }
}

// ── Restock screen ────────────────────────────────────────────────────────────

/// Which column the restock table is sorted by.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum RestockSort {
    /// Copies sold per week over the sell-through window.
    #[default]
    Velocity,
    SoldCopies,
    Revenue,
    Days,
    Price,
    Name,
    /// Source lot extracted from the last known location.
    Lot,
}

/// State for the read-only Restock Recommendations screen.
pub struct RestockState {
    /// Minimum lifetime sold copies for a variant to count (filters one-offs).
    pub min_copies: i64,
    /// The ranked report, if it has been built.
    pub rows: Option<Vec<crate::restock::RankedRestock>>,
    pub error: Option<String>,
    /// Guards the one-shot auto-load when the screen is first shown.
    pub loaded: bool,
    /// [`AppState::db_generation`] the cached rows were built at; a mismatch
    /// means a sync has landed since and the report is stale.
    pub generation: u64,
    pub sort: RestockSort,
    pub sort_desc: bool,
}

impl Default for RestockState {
    fn default() -> Self {
        Self {
            min_copies: 2,
            rows: None,
            error: None,
            loaded: false,
            generation: 0,
            sort: RestockSort::default(),
            sort_desc: true,
        }
    }
}

// ── Precon EV screen ──────────────────────────────────────────────────────────

/// Message from the background precon load: progress events while it runs,
/// then exactly one final result.
pub enum PreconLoadMsg {
    Event(crate::precon_loader::LoadEvent),
    /// The ranking data. Boxed: the payload is large compared with events.
    Done(Result<Box<crate::precon_ev::PreconData>, String>),
    /// The sales stats, fetched after `Done` (the slow, background phase).
    Sales(Result<crate::precon_ev::SalesMap, String>),
}

/// State for the Precon EV screen (sealed precon value vs. singles).
pub struct PreconEvState {
    /// Raw data of the last successful load; rows are rebuilt from it.
    pub data: Option<crate::precon_ev::PreconData>,
    /// Ranking rows for the current parameters, sorted.
    pub rows: Vec<crate::precon_ev::PreconRow>,
    /// Price column for the cards.
    pub field: InventoryPriceSource,
    /// Price column for the sealed product (one of `precon_ev::SEALED_FIELDS`).
    pub sealed_field: InventoryPriceSource,
    /// Value foils at the non-foil price unless the foil sold recently.
    pub foil_needs_sales: bool,
    /// Card copies below this unit price (€) are excluded from EV.
    pub cutoff: f64,
    pub filter: String,
    pub hide_no_decklist: bool,
    pub sort: crate::precon_ev::PreconSort,
    pub sort_desc: bool,
    pub loading: bool,
    /// The background sales-history phase (after the ranking is shown).
    pub sales_loading: bool,
    /// Cancels a running sales-history fetch (set when a new load starts).
    pub sales_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Current load stage (shown next to the spinner).
    pub stage: String,
    /// Counted progress of the current stage: (label, done, total).
    pub progress: Option<(&'static str, usize, usize)>,
    pub error: Option<String>,
    pub rx: Option<std::sync::mpsc::Receiver<PreconLoadMsg>>,
    /// Product shown in the detail view.
    pub selected: Option<u64>,
    /// Snapshot history per product id (fetched live per load).
    pub histories: std::collections::HashMap<u64, crate::precon_loader::HistoryData>,
    /// EV chart points for the selected product and current parameters.
    pub history_points: Vec<crate::precon_ev::EvPoint>,
    pub history_rx:
        Option<std::sync::mpsc::Receiver<Result<crate::precon_loader::HistoryData, String>>>,
    pub history_error: Option<String>,
}

impl Default for PreconEvState {
    fn default() -> Self {
        Self {
            data: None,
            rows: Vec::new(),
            field: InventoryPriceSource::Trend,
            sealed_field: InventoryPriceSource::Trend,
            foil_needs_sales: true,
            cutoff: crate::precon_ev::DEFAULT_CUTOFF_EUR,
            filter: String::new(),
            hide_no_decklist: false,
            sort: crate::precon_ev::PreconSort::default(),
            sort_desc: true,
            loading: false,
            sales_loading: false,
            sales_cancel: None,
            stage: String::new(),
            progress: None,
            error: None,
            rx: None,
            selected: None,
            histories: std::collections::HashMap::new(),
            history_points: Vec::new(),
            history_rx: None,
            history_error: None,
        }
    }
}

impl Default for SearchState {
    fn default() -> Self {
        Self {
            csv_path: String::new(),
            search_term: String::new(),
            last_search_term: String::new(),
            cards: Vec::new(),
            filtered_cards: Vec::new(),
            search_case_sensitive: false,
            search_in_all_languages: true,
            last_search_time: std::time::Instant::now(),
            search_needs_update: false,
            current_page: 0,
            results_per_page: 100,
            selected_fields: SearchFields {
                name: true,
                set: true,
                condition: false,
                language: false,
                location: false,
                rarity: false,
                price: false,
                comment: false,
            },
            selected_cards: Vec::new(),
            quantity_inputs: std::collections::HashMap::new(),
            action_mode: SearchAction::AddToLists,
            discard_confirm: false,
            history: CardHistoryState::default(),
        }
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
