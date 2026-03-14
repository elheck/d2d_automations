use super::language::Language;
use crate::models::Card;
use crate::stock_analysis::SortOrder;
use eframe::egui;
use serde::{Deserialize, Serialize};

type CardMatch = (Card, i32, String);
/// (card_name, needed_quantity, matched_cards)
type CardMatchGroup = (String, i32, Vec<CardMatch>);

#[derive(PartialEq)]
pub enum Screen {
    Welcome,
    StockChecker,
    StockAnalysis,
    StockListing,
    Search,
    Picking,
    Pricing,
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
        }
    }
}

pub struct StockAnalysisState {
    pub inventory_path: String,
    pub output: String,
    pub free_slots: i32,
    pub sort_order: SortOrder,
    pub db_stats: Option<crate::inventory_db::DbStats>,
    pub db_stats_error: Option<String>,
    /// Set to true after the first stats load attempt so we don't query on every frame.
    pub stats_loaded: bool,
}

impl Default for StockAnalysisState {
    fn default() -> Self {
        Self {
            inventory_path: String::new(),
            output: String::new(),
            free_slots: 5,
            sort_order: SortOrder::ByFreeSlots,
            db_stats: None,
            db_stats_error: None,
            stats_loaded: false,
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
    pub name_de: bool,
    pub name_es: bool,
    pub name_fr: bool,
    pub name_it: bool,
}

// ── Node graph types ─────────────────────────────────────────────────────────

pub type NodeId = usize;

// ── Filter parameter enums ────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ConditionFilter {
    Any,
    Nm,
    Ex,
    Gd,
    Lp,
    Pl,
}

impl ConditionFilter {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::Nm => "NM",
            Self::Ex => "EX",
            Self::Gd => "GD",
            Self::Lp => "LP",
            Self::Pl => "PL",
        }
    }
    pub fn all() -> &'static [Self] {
        &[Self::Any, Self::Nm, Self::Ex, Self::Gd, Self::Lp, Self::Pl]
    }
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LanguageFilter {
    Any,
    English,
    German,
    French,
    Spanish,
    Italian,
}

impl LanguageFilter {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::English => "English",
            Self::German => "German",
            Self::French => "French",
            Self::Spanish => "Spanish",
            Self::Italian => "Italian",
        }
    }
    pub fn all() -> &'static [Self] {
        &[
            Self::Any,
            Self::English,
            Self::German,
            Self::French,
            Self::Spanish,
            Self::Italian,
        ]
    }
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FoilFilter {
    Any,
    FoilOnly,
    NonFoilOnly,
}

impl FoilFilter {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::FoilOnly => "Foil only",
            Self::NonFoilOnly => "Non-foil only",
        }
    }
    pub fn all() -> &'static [Self] {
        &[Self::Any, Self::FoilOnly, Self::NonFoilOnly]
    }
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RarityFilter {
    Any,
    Common,
    Uncommon,
    Rare,
    Mythic,
}

impl RarityFilter {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Any => "Any",
            Self::Common => "Common",
            Self::Uncommon => "Uncommon",
            Self::Rare => "Rare",
            Self::Mythic => "Mythic",
        }
    }
    pub fn all() -> &'static [Self] {
        &[
            Self::Any,
            Self::Common,
            Self::Uncommon,
            Self::Rare,
            Self::Mythic,
        ]
    }
}

// ── Node kinds ────────────────────────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub enum NodeKind {
    // Source / sink
    CsvSource,
    Output,
    // Filters
    FilterCondition { condition: ConditionFilter },
    FilterLanguage { language: LanguageFilter },
    FilterFoil { mode: FoilFilter },
    FilterPrice { min: f64, max: f64 },
    FilterRarity { rarity: RarityFilter },
    FilterName { term: String },
    FilterSet { term: String },
    FilterLocation { term: String },
    // Logic
    LogicalAnd,
    LogicalOr,
    LogicalNot,
}

impl NodeKind {
    pub fn title(&self) -> &'static str {
        match self {
            Self::CsvSource => "CSV Source",
            Self::Output => "Output",
            Self::FilterCondition { .. } => "Filter Condition",
            Self::FilterLanguage { .. } => "Filter Language",
            Self::FilterFoil { .. } => "Filter Foil",
            Self::FilterPrice { .. } => "Filter Price",
            Self::FilterRarity { .. } => "Filter Rarity",
            Self::FilterName { .. } => "Filter Name",
            Self::FilterSet { .. } => "Filter Set",
            Self::FilterLocation { .. } => "Filter Location",
            Self::LogicalAnd => "AND",
            Self::LogicalOr => "OR",
            Self::LogicalNot => "NOT",
        }
    }

    pub fn accent_color(&self) -> egui::Color32 {
        match self {
            Self::CsvSource => egui::Color32::from_rgb(50, 100, 170),
            Self::Output => egui::Color32::from_rgb(150, 115, 40),
            Self::FilterCondition { .. } => egui::Color32::from_rgb(30, 125, 140),
            Self::FilterLanguage { .. } => egui::Color32::from_rgb(40, 105, 160),
            Self::FilterFoil { .. } => egui::Color32::from_rgb(110, 70, 155),
            Self::FilterPrice { .. } => egui::Color32::from_rgb(30, 140, 110),
            Self::FilterRarity { .. } => egui::Color32::from_rgb(155, 90, 40),
            Self::FilterName { .. } => egui::Color32::from_rgb(35, 148, 125),
            Self::FilterSet { .. } => egui::Color32::from_rgb(45, 115, 155),
            Self::FilterLocation { .. } => egui::Color32::from_rgb(140, 80, 45),
            Self::LogicalAnd => egui::Color32::from_rgb(100, 60, 160),
            Self::LogicalOr => egui::Color32::from_rgb(60, 130, 80),
            Self::LogicalNot => egui::Color32::from_rgb(170, 55, 55),
        }
    }

    pub fn input_count(&self) -> usize {
        match self {
            Self::CsvSource => 0,
            Self::LogicalAnd | Self::LogicalOr => 2,
            _ => 1,
        }
    }

    pub fn output_count(&self) -> usize {
        match self {
            Self::Output => 0,
            _ => 1,
        }
    }

    pub fn param_count(&self) -> usize {
        match self {
            Self::CsvSource | Self::Output => 0,
            Self::LogicalAnd | Self::LogicalOr | Self::LogicalNot => 0,
            Self::FilterPrice { .. } => 2,
            _ => 1,
        }
    }
}

pub struct GraphNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub pos: egui::Pos2,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Wire {
    pub from_node: NodeId,
    pub from_port: usize,
    pub to_node: NodeId,
    pub to_port: usize,
}

pub struct NodeGraph {
    pub nodes: Vec<GraphNode>,
    pub wires: Vec<Wire>,
    next_id: NodeId,
    pub canvas_offset: egui::Vec2,
    pub canvas_zoom: f32,
    /// (node_id, unused) — drag delta applied each frame via response.drag_delta()
    pub drag: Option<(NodeId, egui::Vec2)>,
    /// Started wiring from this output port; wire follows cursor until released
    pub pending_wire: Option<(NodeId, usize)>,
}

impl Default for NodeGraph {
    fn default() -> Self {
        let mut g = Self {
            nodes: Vec::new(),
            wires: Vec::new(),
            next_id: 0,
            canvas_offset: egui::vec2(0.0, 0.0),
            canvas_zoom: 1.0,
            drag: None,
            pending_wire: None,
        };
        g.add_node(NodeKind::CsvSource, egui::pos2(40.0, 100.0));
        g.add_node(NodeKind::Output, egui::pos2(460.0, 100.0));
        g
    }
}

impl NodeGraph {
    pub fn add_node(&mut self, kind: NodeKind, pos: egui::Pos2) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push(GraphNode { id, kind, pos });
        id
    }

    pub fn remove_node(&mut self, id: NodeId) {
        self.nodes.retain(|n| n.id != id);
        self.wires.retain(|w| w.from_node != id && w.to_node != id);
    }

    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut GraphNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn save(&self) -> SavedGraph {
        SavedGraph {
            nodes: self
                .nodes
                .iter()
                .map(|n| SavedNode {
                    id: n.id,
                    kind: n.kind.clone(),
                    x: n.pos.x,
                    y: n.pos.y,
                })
                .collect(),
            wires: self.wires.clone(),
            canvas_offset_x: self.canvas_offset.x,
            canvas_offset_y: self.canvas_offset.y,
            canvas_zoom: self.canvas_zoom,
        }
    }

    pub fn load(saved: SavedGraph) -> Self {
        let max_id = saved.nodes.iter().map(|n| n.id).max().unwrap_or(0);
        Self {
            nodes: saved
                .nodes
                .into_iter()
                .map(|n| GraphNode {
                    id: n.id,
                    kind: n.kind,
                    pos: egui::pos2(n.x, n.y),
                })
                .collect(),
            wires: saved.wires,
            next_id: max_id + 1,
            canvas_offset: egui::vec2(saved.canvas_offset_x, saved.canvas_offset_y),
            canvas_zoom: saved.canvas_zoom,
            drag: None,
            pending_wire: None,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct SavedNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub x: f32,
    pub y: f32,
}

#[derive(Serialize, Deserialize)]
pub struct SavedGraph {
    pub nodes: Vec<SavedNode>,
    pub wires: Vec<Wire>,
    pub canvas_offset_x: f32,
    pub canvas_offset_y: f32,
    pub canvas_zoom: f32,
}

#[derive(Default)]
pub struct PricingState {
    pub csv_path: String,
    pub cards: Vec<crate::models::Card>,
    pub load_error: Option<String>,
    pub graph: NodeGraph,
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
                name_de: true,
                name_es: true,
                name_fr: true,
                name_it: true,
            },
            selected_cards: Vec::new(),
            quantity_inputs: std::collections::HashMap::new(),
        }
    }
}
