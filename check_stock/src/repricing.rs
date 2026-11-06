//! Rule-based repricing: the pure logic behind the Pricing screen.
//!
//! A [`Ruleset`] is an ordered list of [`Rule`]s. Every in-stock listing is
//! priced by the **first** enabled rule whose conditions all match; listings
//! no rule matches keep their price. A rule's price comes from a base (a
//! market column, the current price, a fixed amount, or "keep"), scaled by a
//! percentage plus an offset, optionally adjusted for condition and language,
//! then bounded by the rule's own limits.
//!
//! Shop-wide [`Guardrails`] apply to every price a rule sets: rarity floors
//! (which always win, even over "raise only"/"lower only"), rounding, a dead
//! band that ignores tiny changes, skipping stale market data, and a review
//! threshold that leaves big moves unticked until the user confirms them.
//!
//! Nothing here touches the UI, the database or the network: the caller
//! passes the listings (CSV rows joined with age/sales facts from the
//! inventory DB), the market rows and today's date.

use crate::api::inventory_sync::{LatestPrice, PriceField, PriceFields};
use crate::mispricing::{condition_factor, momentum_of, Momentum};
use crate::models::{canonical_condition, Card};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Cardmarket refuses listings below this price.
pub const CARDMARKET_MIN_PRICE: f64 = 0.02;
/// Window for the "sold in the last N days" sales signal.
pub const SALES_WINDOW_DAYS: i64 = 30;
/// Canonical condition codes, best first.
pub const CONDITIONS: [&str; 6] = ["NM", "EX", "GD", "LP", "PL", "PO"];
/// Format version written into saved rulesets.
pub const RULESET_VERSION: u32 = 1;
/// Upper bound for any price in a ruleset (sanity cap for loaded files).
pub const MAX_PRICE_EUR: f64 = 100_000.0;
/// Saved rulesets larger than this are refused unread.
pub const MAX_RULESET_FILE_BYTES: u64 = 1024 * 1024;

const MAX_RULES: usize = 100;
const MAX_CONDITIONS: usize = 30;
const MAX_LIST_ITEMS: usize = 50;
const MAX_NAME_CHARS: usize = 80;
const MAX_TERM_CHARS: usize = 300;
const MAX_PCT: f64 = 1000.0;

// ── Conditions ──────────────────────────────────────────────────────────────

/// Card finish a rule can require.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Finish {
    Foil,
    NonFoil,
}

impl Finish {
    pub const ALL: [Finish; 2] = [Finish::NonFoil, Finish::Foil];

    pub fn label(self) -> &'static str {
        match self {
            Finish::Foil => "Foil",
            Finish::NonFoil => "Non-foil",
        }
    }
}

/// Market direction, judged from Cardmarket's 1/7/30-day averages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketMove {
    Rising,
    Falling,
    Flat,
}

impl MarketMove {
    pub const ALL: [MarketMove; 3] = [MarketMove::Rising, MarketMove::Falling, MarketMove::Flat];

    pub fn label(self) -> &'static str {
        match self {
            MarketMove::Rising => "Rising",
            MarketMove::Falling => "Falling",
            MarketMove::Flat => "Flat",
        }
    }

    fn from_momentum(m: Momentum) -> Option<Self> {
        match m {
            Momentum::Rising => Some(MarketMove::Rising),
            Momentum::Falling => Some(MarketMove::Falling),
            Momentum::Flat => Some(MarketMove::Flat),
            Momentum::Unknown => None,
        }
    }
}

/// One test a listing must pass for a rule to apply. A rule matches when all
/// of its conditions do; a rule without conditions matches every listing.
///
/// List conditions (rarity, condition, language) match any listed value; an
/// empty list matches everything. Text conditions take comma-separated terms
/// and match when any term is contained (case-insensitive). Conditions that
/// need data a listing lacks (no market row, unknown age) do not match.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Condition {
    Rarity(Vec<String>),
    /// Canonical condition codes (`NM`, `EX`, …).
    CardCondition(Vec<String>),
    Language(Vec<String>),
    Finish(Finish),
    /// Current listed price, € (bounds inclusive).
    ListedPrice {
        min: Option<f64>,
        max: Option<f64>,
    },
    /// Market reference price ([`Guardrails::reference`]), €.
    MarketPrice {
        min: Option<f64>,
        max: Option<f64>,
    },
    Name(String),
    /// Set name or set code.
    Set(String),
    Location(String),
    /// Days in stock (from the listing date, else the first import).
    AgeDays {
        min: Option<u32>,
        max: Option<u32>,
    },
    /// Days since this exact listing last sold (or was listed, if never).
    NoSaleDays(u32),
    /// Copies of this exact listing sold in the last [`SALES_WINDOW_DAYS`].
    MinSold30d(u32),
    /// Copies of this listing in stock.
    MinQuantity(u32),
    /// Market direction.
    Moving(MarketMove),
}

impl Condition {
    /// One default condition of every kind, in menu order.
    pub fn templates() -> Vec<Condition> {
        vec![
            Condition::Rarity(Vec::new()),
            Condition::CardCondition(Vec::new()),
            Condition::Language(Vec::new()),
            Condition::Finish(Finish::NonFoil),
            Condition::ListedPrice {
                min: None,
                max: None,
            },
            Condition::MarketPrice {
                min: None,
                max: None,
            },
            Condition::Name(String::new()),
            Condition::Set(String::new()),
            Condition::Location(String::new()),
            Condition::AgeDays {
                min: Some(90),
                max: None,
            },
            Condition::NoSaleDays(90),
            Condition::MinSold30d(2),
            Condition::MinQuantity(4),
            Condition::Moving(MarketMove::Rising),
        ]
    }

    /// Short name of the condition kind, for menus and row labels.
    pub fn kind_label(&self) -> &'static str {
        match self {
            Condition::Rarity(_) => "Rarity",
            Condition::CardCondition(_) => "Condition",
            Condition::Language(_) => "Language",
            Condition::Finish(_) => "Finish",
            Condition::ListedPrice { .. } => "Listed price",
            Condition::MarketPrice { .. } => "Market price",
            Condition::Name(_) => "Name",
            Condition::Set(_) => "Set",
            Condition::Location(_) => "Location",
            Condition::AgeDays { .. } => "Time in stock",
            Condition::NoSaleDays(_) => "No sale for",
            Condition::MinSold30d(_) => "Sold recently",
            Condition::MinQuantity(_) => "Copies in stock",
            Condition::Moving(_) => "Market trend",
        }
    }

    /// True when both conditions are of the same kind (values ignored).
    pub fn same_kind(&self, other: &Condition) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    /// One-line human summary, e.g. `Rarity: Common, Uncommon`.
    pub fn describe(&self, reference: PriceField) -> String {
        let list = |label: &str, items: &[String]| {
            if items.is_empty() {
                format!("{label}: any")
            } else {
                format!("{label}: {}", items.join(", "))
            }
        };
        let text = |label: &str, terms: &str| {
            if terms.trim().is_empty() {
                format!("{label}: any")
            } else {
                format!("{label} contains \"{}\"", terms.trim())
            }
        };
        match self {
            Condition::Rarity(r) => list("Rarity", r),
            Condition::CardCondition(c) => list("Condition", c),
            Condition::Language(l) => list("Language", l),
            Condition::Finish(f) => f.label().to_string(),
            Condition::ListedPrice { min, max } => euro_range("Listed", *min, *max),
            Condition::MarketPrice { min, max } => euro_range(reference.as_str(), *min, *max),
            Condition::Name(t) => text("Name", t),
            Condition::Set(t) => text("Set", t),
            Condition::Location(t) => text("Location", t),
            Condition::AgeDays { min, max } => match (min, max) {
                (Some(a), Some(b)) => format!("In stock {a}–{b} days"),
                (Some(a), None) => format!("In stock ≥ {a} days"),
                (None, Some(b)) => format!("In stock ≤ {b} days"),
                (None, None) => "In stock: any".to_string(),
            },
            Condition::NoSaleDays(d) => format!("No sale for ≥ {d} days"),
            Condition::MinSold30d(n) => format!("≥ {n} sold in {SALES_WINDOW_DAYS} days"),
            Condition::MinQuantity(n) => format!("≥ {n} copies in stock"),
            Condition::Moving(m) => format!("Market {}", m.label().to_lowercase()),
        }
    }

    /// Lowercased comma-separated terms of a text condition (empty for other
    /// kinds). Prepared once per evaluation, not once per listing.
    fn terms(&self) -> Vec<String> {
        match self {
            Condition::Name(t) | Condition::Set(t) | Condition::Location(t) => split_terms(t),
            _ => Vec::new(),
        }
    }

    fn matches(&self, l: &Listing, m: &MarketView, terms: &[String]) -> bool {
        match self {
            Condition::Rarity(r) => {
                r.is_empty() || r.iter().any(|x| x.eq_ignore_ascii_case(&l.rarity))
            }
            Condition::CardCondition(c) => {
                c.is_empty() || c.iter().any(|x| canonical_condition(x) == l.condition)
            }
            Condition::Language(langs) => {
                langs.is_empty() || langs.iter().any(|x| x.eq_ignore_ascii_case(&l.language))
            }
            Condition::Finish(Finish::Foil) => l.is_foil,
            Condition::Finish(Finish::NonFoil) => !l.is_foil,
            Condition::ListedPrice { min, max } => in_range(l.price, *min, *max),
            Condition::MarketPrice { min, max } => {
                m.reference.is_some_and(|p| in_range(p, *min, *max))
            }
            Condition::Name(_) => contains_any(&l.name_lc, terms),
            Condition::Set(_) => contains_any(&l.set_lc, terms),
            Condition::Location(_) => contains_any(&l.location_lc, terms),
            Condition::AgeDays { min, max } => l.facts.age_days.is_some_and(|a| {
                min.is_none_or(|lo| a >= i64::from(lo)) && max.is_none_or(|hi| a <= i64::from(hi))
            }),
            Condition::NoSaleDays(d) => l.facts.days_since_sale.is_some_and(|s| s >= i64::from(*d)),
            Condition::MinSold30d(n) => l.facts.sold_30d >= i64::from(*n),
            Condition::MinQuantity(n) => l.quantity >= i64::from(*n),
            Condition::Moving(dir) => m.moving == Some(*dir),
        }
    }

    fn sanitize(&mut self) {
        match self {
            Condition::Rarity(v) | Condition::CardCondition(v) | Condition::Language(v) => {
                v.truncate(MAX_LIST_ITEMS);
                for s in v.iter_mut() {
                    truncate_chars(s, MAX_NAME_CHARS);
                }
            }
            Condition::ListedPrice { min, max } | Condition::MarketPrice { min, max } => {
                *min = clean_price_opt(*min);
                *max = clean_price_opt(*max);
            }
            Condition::Name(t) | Condition::Set(t) | Condition::Location(t) => {
                truncate_chars(t, MAX_TERM_CHARS)
            }
            Condition::Finish(_)
            | Condition::AgeDays { .. }
            | Condition::NoSaleDays(_)
            | Condition::MinSold30d(_)
            | Condition::MinQuantity(_)
            | Condition::Moving(_) => {}
        }
    }
}

// ── Price actions ───────────────────────────────────────────────────────────

/// How a market base combines its two columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Combine {
    /// Use the first column only.
    Only,
    /// The higher of both columns (either one when the other is missing).
    HigherOf,
    /// The lower of both columns (either one when the other is missing).
    LowerOf,
}

impl Combine {
    pub const ALL: [Combine; 3] = [Combine::Only, Combine::HigherOf, Combine::LowerOf];

    pub fn label(self) -> &'static str {
        match self {
            Combine::Only => "only",
            Combine::HigherOf => "or higher of",
            Combine::LowerOf => "or lower of",
        }
    }
}

/// Where a rule's price starts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PriceBase {
    /// A Cardmarket price column (foil column for foils), optionally combined
    /// with a second column.
    Market {
        field: PriceField,
        combine: Combine,
        other: PriceField,
    },
    /// The listing's current price.
    Current,
    /// A fixed amount, €.
    Fixed(f64),
    /// Leave the listing alone (protects it from later rules).
    Keep,
}

impl PriceBase {
    pub fn label(&self) -> &'static str {
        match self {
            PriceBase::Market { .. } => "Market price",
            PriceBase::Current => "Current price",
            PriceBase::Fixed(_) => "Fixed price",
            PriceBase::Keep => "Keep price",
        }
    }

    /// Market base on a single column.
    pub fn market(field: PriceField) -> Self {
        PriceBase::Market {
            field,
            combine: Combine::Only,
            other: PriceField::Avg30,
        }
    }
}

/// Which way a rule may move a price.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Both,
    RaiseOnly,
    LowerOnly,
}

impl Direction {
    pub const ALL: [Direction; 3] = [Direction::Both, Direction::RaiseOnly, Direction::LowerOnly];

    pub fn label(self) -> &'static str {
        match self {
            Direction::Both => "Raise and lower",
            Direction::RaiseOnly => "Only raise",
            Direction::LowerOnly => "Only lower",
        }
    }
}

/// What a rule does to a matching listing's price:
/// `base × percent + offset`, × condition/language factors when `adjust` is
/// set (market base only), then clamped to the rule's limits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PriceAction {
    pub base: PriceBase,
    /// 100 = unchanged.
    pub percent: f64,
    /// € added after the percentage (negative to undercut).
    pub offset: f64,
    /// Apply the ruleset's condition and language factors (market base only:
    /// the price guide does not distinguish condition or language).
    pub adjust: bool,
    pub direction: Direction,
    pub min_price: Option<f64>,
    pub max_price: Option<f64>,
    /// Largest raise per run, % of the current price.
    pub max_raise_pct: Option<f64>,
    /// Largest drop per run, % of the current price.
    pub max_drop_pct: Option<f64>,
}

impl Default for PriceAction {
    fn default() -> Self {
        Self {
            base: PriceBase::market(PriceField::Trend),
            percent: 100.0,
            offset: 0.0,
            adjust: true,
            direction: Direction::Both,
            min_price: None,
            max_price: None,
            max_raise_pct: None,
            max_drop_pct: None,
        }
    }
}

impl PriceAction {
    /// One-line human summary, e.g. `Trend × 95% − €0.01 · only lower`.
    pub fn describe(&self) -> String {
        let mut s = match &self.base {
            PriceBase::Keep => return "Keep current price".to_string(),
            PriceBase::Current => "Current".to_string(),
            PriceBase::Fixed(p) => euro(*p),
            PriceBase::Market {
                field,
                combine,
                other,
            } => match combine {
                Combine::Only => field.as_str().to_string(),
                Combine::HigherOf => format!("max({}, {})", field.as_str(), other.as_str()),
                Combine::LowerOf => format!("min({}, {})", field.as_str(), other.as_str()),
            },
        };
        if (self.percent - 100.0).abs() > 1e-9 {
            s.push_str(&format!(" × {}%", trim_num(self.percent)));
        }
        if self.offset > 0.0 {
            s.push_str(&format!(" + {}", euro(self.offset)));
        } else if self.offset < 0.0 {
            s.push_str(&format!(" − {}", euro(-self.offset)));
        }
        if self.adjust && matches!(self.base, PriceBase::Market { .. }) {
            s.push_str(" · cond./lang.");
        }
        match self.direction {
            Direction::Both => {}
            Direction::RaiseOnly => s.push_str(" · only raise"),
            Direction::LowerOnly => s.push_str(" · only lower"),
        }
        s
    }

    fn sanitize(&mut self) {
        self.percent = clean(self.percent, 0.0, MAX_PCT, 100.0);
        self.offset = clean(self.offset, -MAX_PRICE_EUR, MAX_PRICE_EUR, 0.0);
        if let PriceBase::Fixed(p) = &mut self.base {
            *p = clean(
                *p,
                CARDMARKET_MIN_PRICE,
                MAX_PRICE_EUR,
                CARDMARKET_MIN_PRICE,
            );
        }
        self.min_price = clean_price_opt(self.min_price);
        self.max_price = clean_price_opt(self.max_price);
        self.max_raise_pct = self.max_raise_pct.map(|p| clean(p, 0.0, MAX_PCT, 0.0));
        self.max_drop_pct = self.max_drop_pct.map(|p| clean(p, 0.0, 100.0, 0.0));
    }
}

/// A named, ordered pricing rule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub conditions: Vec<Condition>,
    #[serde(default)]
    pub action: PriceAction,
}

fn default_true() -> bool {
    true
}

impl Rule {
    /// A catch-all rule following the market trend.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            enabled: true,
            conditions: Vec::new(),
            action: PriceAction::default(),
        }
    }

    /// Summary of the conditions, e.g. `Rarity: Rare · Listed ≥ €2.00`.
    pub fn describe_when(&self, reference: PriceField) -> String {
        if self.conditions.is_empty() {
            "Every listing".to_string()
        } else {
            self.conditions
                .iter()
                .map(|c| c.describe(reference))
                .collect::<Vec<_>>()
                .join(" · ")
        }
    }

    fn sanitize(&mut self) {
        truncate_chars(&mut self.name, MAX_NAME_CHARS);
        self.conditions.truncate(MAX_CONDITIONS);
        for c in &mut self.conditions {
            c.sanitize();
        }
        self.action.sanitize();
    }
}

// ── Guardrails & adjustments ────────────────────────────────────────────────

/// Price rounding applied to every price a rule sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rounding {
    Cent,
    FiveCents,
    TenCents,
    /// Prices end in 9 cents (€0.19, €2.49).
    EndsIn9,
}

impl Rounding {
    pub const ALL: [Rounding; 4] = [
        Rounding::Cent,
        Rounding::FiveCents,
        Rounding::TenCents,
        Rounding::EndsIn9,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Rounding::Cent => "To the cent",
            Rounding::FiveCents => "To 5 cents",
            Rounding::TenCents => "To 10 cents",
            Rounding::EndsIn9 => "Ending in 9 (€x.x9)",
        }
    }
}

/// Lowest price per rarity. Floors always win: a listing below its floor is
/// lifted to it whenever a rule (other than "keep") applies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RarityFloors {
    pub common: f64,
    pub uncommon: f64,
    pub rare: f64,
    pub mythic: f64,
    /// Tokens, lands, specials and anything else.
    pub other: f64,
}

impl Default for RarityFloors {
    fn default() -> Self {
        Self {
            common: 0.05,
            uncommon: 0.05,
            rare: 0.10,
            mythic: 0.20,
            other: 0.05,
        }
    }
}

impl RarityFloors {
    /// Floor for a CSV rarity, never below Cardmarket's minimum.
    pub fn floor_for(&self, rarity: &str) -> f64 {
        let f = match rarity.trim().to_lowercase().as_str() {
            "common" => self.common,
            "uncommon" => self.uncommon,
            "rare" => self.rare,
            "mythic" | "mythic rare" => self.mythic,
            _ => self.other,
        };
        f.max(CARDMARKET_MIN_PRICE)
    }
}

/// Shop-wide limits applied to every price a rule sets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Guardrails {
    pub floors: RarityFloors,
    pub rounding: Rounding,
    /// Changes smaller than this (€) are ignored…
    pub min_change_eur: f64,
    /// …as are changes smaller than this share of the current price (%).
    pub min_change_pct: f64,
    /// Market rows older than this many days are not used (0 = always use).
    pub stale_days: u32,
    /// Changes above this % *and* at least `review_min_eur` need a tick.
    pub review_pct: f64,
    pub review_min_eur: f64,
    /// Market column used by the "market price" condition and the review
    /// table's market column.
    pub reference: PriceField,
}

impl Default for Guardrails {
    fn default() -> Self {
        Self {
            floors: RarityFloors::default(),
            rounding: Rounding::Cent,
            min_change_eur: 0.01,
            min_change_pct: 2.0,
            stale_days: 7,
            review_pct: 30.0,
            review_min_eur: 1.0,
            reference: PriceField::Trend,
        }
    }
}

impl Guardrails {
    fn sanitize(&mut self) {
        let f = &mut self.floors;
        for v in [
            &mut f.common,
            &mut f.uncommon,
            &mut f.rare,
            &mut f.mythic,
            &mut f.other,
        ] {
            *v = clean(*v, 0.0, MAX_PRICE_EUR, CARDMARKET_MIN_PRICE);
        }
        self.min_change_eur = clean(self.min_change_eur, 0.0, 1000.0, 0.01);
        self.min_change_pct = clean(self.min_change_pct, 0.0, 100.0, 0.0);
        self.stale_days = self.stale_days.min(3650);
        self.review_pct = clean(self.review_pct, 0.0, MAX_PCT, 30.0);
        self.review_min_eur = clean(self.review_min_eur, 0.0, MAX_PRICE_EUR, 0.0);
    }
}

/// Condition and language factors (in %) for market-based prices. The price
/// guide is roughly near-mint, language-blind pricing; these scale it per
/// listing. Missing entries count as 100%.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Adjustments {
    pub condition_pct: BTreeMap<String, f64>,
    pub language_pct: BTreeMap<String, f64>,
}

impl Default for Adjustments {
    fn default() -> Self {
        Self {
            // Same heuristic the mispricing report classifies against.
            condition_pct: CONDITIONS
                .iter()
                .map(|c| (c.to_string(), condition_factor(c) * 100.0))
                .collect(),
            language_pct: BTreeMap::new(),
        }
    }
}

impl Adjustments {
    pub fn condition_factor(&self, condition: &str) -> f64 {
        self.condition_pct
            .get(&canonical_condition(condition))
            .map_or(1.0, |p| p / 100.0)
    }

    pub fn language_factor(&self, language: &str) -> f64 {
        self.language_pct
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(language.trim()))
            .map_or(1.0, |(_, p)| p / 100.0)
    }

    fn sanitize(&mut self) {
        for map in [&mut self.condition_pct, &mut self.language_pct] {
            let cleaned: BTreeMap<String, f64> = std::mem::take(map)
                .into_iter()
                .take(MAX_LIST_ITEMS)
                .map(|(mut k, v)| {
                    truncate_chars(&mut k, MAX_NAME_CHARS);
                    (k, clean(v, 0.0, MAX_PCT, 100.0))
                })
                .collect();
            *map = cleaned;
        }
    }
}

// ── Ruleset ─────────────────────────────────────────────────────────────────

/// A complete, saveable repricing configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ruleset {
    #[serde(default = "ruleset_version")]
    pub version: u32,
    #[serde(default)]
    pub name: String,
    /// Required, so files of other formats (such as the old node-graph
    /// saves) are refused instead of loading as an empty ruleset.
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub guardrails: Guardrails,
    #[serde(default)]
    pub adjustments: Adjustments,
}

fn ruleset_version() -> u32 {
    RULESET_VERSION
}

impl Default for Ruleset {
    fn default() -> Self {
        Self::starter()
    }
}

impl Ruleset {
    /// A sensible first ruleset: protect expensive cards, raise fast sellers,
    /// mark down dead stock, follow the market for the rest.
    pub fn starter() -> Self {
        let templates = rule_templates();
        let pick = |name: &str| {
            templates
                .iter()
                .find(|t| t.rule.name == name)
                .map(|t| t.rule.clone())
                .unwrap_or_else(|| Rule::new(name))
        };
        Self {
            version: RULESET_VERSION,
            name: "My repricing rules".to_string(),
            rules: vec![
                pick("Protect expensive cards"),
                pick("Fast sellers: raise"),
                pick("Dead stock: mark down"),
                pick("Follow the market"),
            ],
            guardrails: Guardrails::default(),
            adjustments: Adjustments::default(),
        }
    }

    /// Clamps every number into a sane range and caps every list/string, so
    /// a hand-edited or hostile file cannot produce absurd prices.
    pub fn sanitize(&mut self) {
        self.version = RULESET_VERSION;
        truncate_chars(&mut self.name, MAX_NAME_CHARS);
        self.rules.truncate(MAX_RULES);
        for r in &mut self.rules {
            r.sanitize();
        }
        self.guardrails.sanitize();
        self.adjustments.sanitize();
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Parses and sanitizes a saved ruleset.
    pub fn from_json(json: &str) -> Result<Self, String> {
        match serde_json::from_str::<Ruleset>(json) {
            Ok(mut rs) => {
                rs.sanitize();
                Ok(rs)
            }
            Err(e) if e.to_string().contains("missing field `rules`") => Err(
                "This file is not a repricing ruleset (old node-graph files cannot be opened)"
                    .to_string(),
            ),
            Err(e) => Err(format!("Not a valid ruleset: {e}")),
        }
    }
}

/// Where the working ruleset is kept between sessions.
pub fn default_ruleset_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("d2d_automations")
        .join("repricing_ruleset.json")
}

/// Writes a ruleset atomically (temp file + rename), creating the folder.
pub fn save_ruleset(path: &Path, ruleset: &Ruleset) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, ruleset.to_json())?;
    std::fs::rename(&tmp, path)
}

/// Reads a ruleset, refusing oversized files before reading them.
pub fn load_ruleset(path: &Path) -> Result<Ruleset, String> {
    let size = std::fs::metadata(path)
        .map_err(|e| format!("Cannot read {}: {e}", path.display()))?
        .len();
    if size > MAX_RULESET_FILE_BYTES {
        return Err(format!(
            "{} is too large to be a ruleset ({size} bytes)",
            path.display()
        ));
    }
    let json = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    Ruleset::from_json(&json)
}

/// A ready-made rule offered in the "add rule" menu.
pub struct RuleTemplate {
    pub hint: &'static str,
    pub rule: Rule,
}

/// Ready-made rules for common repricing strategies.
pub fn rule_templates() -> Vec<RuleTemplate> {
    let rule = |name: &str, conditions: Vec<Condition>, action: PriceAction| Rule {
        name: name.to_string(),
        enabled: true,
        conditions,
        action,
    };
    vec![
        RuleTemplate {
            hint: "Leave cards listed at €50 or more alone; price those by hand",
            rule: rule(
                "Protect expensive cards",
                vec![Condition::ListedPrice {
                    min: Some(50.0),
                    max: None,
                }],
                PriceAction {
                    base: PriceBase::Keep,
                    ..PriceAction::default()
                },
            ),
        },
        RuleTemplate {
            hint: "Copies that sold 2+ times in 30 days: price at the higher of trend and 7-day average +10%, never lower, at most +25% per run",
            rule: rule(
                "Fast sellers: raise",
                vec![Condition::MinSold30d(2)],
                PriceAction {
                    base: PriceBase::Market {
                        field: PriceField::Trend,
                        combine: Combine::HigherOf,
                        other: PriceField::Avg7,
                    },
                    percent: 110.0,
                    direction: Direction::RaiseOnly,
                    max_raise_pct: Some(25.0),
                    ..PriceAction::default()
                },
            ),
        },
        RuleTemplate {
            hint: "No sale for 180+ days: 10% off the current price. Every run takes another 10% off (floors still apply), so run it about monthly",
            rule: rule(
                "Dead stock: mark down",
                vec![Condition::NoSaleDays(180)],
                PriceAction {
                    base: PriceBase::Current,
                    percent: 90.0,
                    adjust: false,
                    direction: Direction::LowerOnly,
                    ..PriceAction::default()
                },
            ),
        },
        RuleTemplate {
            hint: "Price at the market trend, adjusted for condition and language",
            rule: rule("Follow the market", Vec::new(), PriceAction::default()),
        },
        RuleTemplate {
            hint: "Never sell into a dip: the higher of trend and the 30-day average",
            rule: rule(
                "Don't sell into a dip",
                Vec::new(),
                PriceAction {
                    base: PriceBase::Market {
                        field: PriceField::Trend,
                        combine: Combine::HigherOf,
                        other: PriceField::Avg30,
                    },
                    ..PriceAction::default()
                },
            ),
        },
        RuleTemplate {
            hint: "Cards with a €2+ trend: one cent below the cheapest offer, at most 15% lower per run",
            rule: rule(
                "Undercut the cheapest offer",
                vec![Condition::MarketPrice {
                    min: Some(2.0),
                    max: None,
                }],
                PriceAction {
                    base: PriceBase::market(PriceField::Low),
                    offset: -0.01,
                    adjust: false,
                    max_drop_pct: Some(15.0),
                    ..PriceAction::default()
                },
            ),
        },
        RuleTemplate {
            hint: "The market is climbing: follow it up, never down",
            rule: rule(
                "Rising market: follow up",
                vec![Condition::Moving(MarketMove::Rising)],
                PriceAction {
                    base: PriceBase::Market {
                        field: PriceField::Trend,
                        combine: Combine::HigherOf,
                        other: PriceField::Avg1,
                    },
                    direction: Direction::RaiseOnly,
                    ..PriceAction::default()
                },
            ),
        },
        RuleTemplate {
            hint: "Commons and uncommons worth under €0.10 at one flat price",
            rule: rule(
                "Bulk at a flat price",
                vec![
                    Condition::Rarity(vec!["Common".to_string(), "Uncommon".to_string()]),
                    Condition::MarketPrice {
                        min: None,
                        max: Some(0.10),
                    },
                ],
                PriceAction {
                    base: PriceBase::Fixed(0.10),
                    adjust: false,
                    ..PriceAction::default()
                },
            ),
        },
        RuleTemplate {
            hint: "Leave listings from the last 7 days alone",
            rule: rule(
                "Protect new listings",
                vec![Condition::AgeDays {
                    min: None,
                    max: Some(7),
                }],
                PriceAction {
                    base: PriceBase::Keep,
                    ..PriceAction::default()
                },
            ),
        },
    ]
}

// ── Listings ────────────────────────────────────────────────────────────────

/// What the inventory DB knows about a listing's history.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ListingFacts {
    /// Days since the listing date (else the first import that saw it).
    pub age_days: Option<i64>,
    pub last_sale: Option<NaiveDate>,
    /// Days since the later of the last sale and the listing date.
    pub days_since_sale: Option<i64>,
    /// Copies sold in the last [`SALES_WINDOW_DAYS`].
    pub sold_30d: i64,
}

impl ListingFacts {
    /// Facts from the listing date and the `(date, copies)` sale history.
    /// Unparseable dates are ignored; future dates count as today.
    pub fn from_history(listed: Option<&str>, sales: &[(String, i64)], today: NaiveDate) -> Self {
        let listed = listed.and_then(parse_date);
        let mut last_sale: Option<NaiveDate> = None;
        let mut sold_30d = 0;
        for (date, copies) in sales {
            let Some(d) = parse_date(date) else { continue };
            if last_sale.is_none_or(|l| d > l) {
                last_sale = Some(d);
            }
            if (today - d).num_days() < SALES_WINDOW_DAYS {
                sold_30d += (*copies).max(0);
            }
        }
        let since = |d: NaiveDate| (today - d).num_days().max(0);
        let latest = match (listed, last_sale) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        Self {
            age_days: listed.map(since),
            last_sale,
            days_since_sale: latest.map(since),
            sold_30d,
        }
    }
}

/// One in-stock CSV row, prepared for rule matching.
#[derive(Clone, Debug, PartialEq)]
pub struct Listing {
    /// Index of the row in the loaded CSV.
    pub row: usize,
    pub product_id: Option<u64>,
    pub name: String,
    pub set: String,
    pub set_code: String,
    pub rarity: String,
    /// Canonical condition code.
    pub condition: String,
    pub language: String,
    pub is_foil: bool,
    pub quantity: i64,
    /// Current listed price, € (0 when the CSV value is unusable).
    pub price: f64,
    pub location: String,
    pub facts: ListingFacts,
    name_lc: String,
    set_lc: String,
    location_lc: String,
}

impl Listing {
    /// `None` for rows without stock (the inventory report keeps sold-out rows).
    pub fn from_card(row: usize, card: &Card, facts: ListingFacts) -> Option<Self> {
        let quantity = card
            .quantity
            .trim()
            .parse::<i64>()
            .ok()
            .filter(|q| *q > 0)?;
        let location = card.location.clone().unwrap_or_default();
        Some(Self {
            row,
            product_id: card.cardmarket_id.trim().parse().ok(),
            name_lc: card.name.to_lowercase(),
            set_lc: format!("{} {}", card.set, card.set_code).to_lowercase(),
            location_lc: location.to_lowercase(),
            name: card.name.clone(),
            set: card.set.clone(),
            set_code: card.set_code.clone(),
            rarity: card.rarity.trim().to_string(),
            condition: canonical_condition(&card.condition),
            language: card.language.trim().to_string(),
            is_foil: card.is_foil_card(),
            quantity,
            price: parse_price(&card.price).unwrap_or(0.0),
            location,
            facts,
        })
    }
}

/// Builds the listings for every in-stock row of `cards`.
pub fn listings_from_cards(
    cards: &[Card],
    mut facts_of: impl FnMut(&Card) -> ListingFacts,
) -> Vec<Listing> {
    cards
        .iter()
        .enumerate()
        .filter_map(|(i, c)| Listing::from_card(i, c, facts_of(c)))
        .collect()
}

/// Distinct values of a listing field, most copies first.
pub fn distinct_by_copies(listings: &[Listing], field: impl Fn(&Listing) -> &str) -> Vec<String> {
    let mut counts: HashMap<&str, i64> = HashMap::new();
    for l in listings {
        let v = field(l);
        if !v.is_empty() {
            *counts.entry(v).or_insert(0) += l.quantity;
        }
    }
    let mut out: Vec<(&str, i64)> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    out.into_iter().map(|(v, _)| v.to_string()).collect()
}

// ── Market view ─────────────────────────────────────────────────────────────

/// The market facts of one listing, resolved once per evaluation.
#[derive(Clone, Copy, Debug, Default)]
pub struct MarketView<'a> {
    pub row: Option<&'a LatestPrice>,
    /// Reference column (foil-aware), when positive.
    pub reference: Option<f64>,
    pub moving: Option<MarketMove>,
    /// Age of the market row in days.
    pub age_days: Option<i64>,
}

pub fn market_view<'a>(
    listing: &Listing,
    market: &'a HashMap<u64, LatestPrice>,
    reference: PriceField,
    today: NaiveDate,
) -> MarketView<'a> {
    let Some(row) = listing.product_id.and_then(|id| market.get(&id)) else {
        return MarketView::default();
    };
    let foil = listing.is_foil;
    let (momentum, _, _) = momentum_of(
        row.price_for(PriceField::Avg1, foil),
        row.price_for(PriceField::Avg7, foil),
        row.price_for(PriceField::Avg30, foil),
    );
    MarketView {
        row: Some(row),
        reference: positive(row.price_for(reference, foil)),
        moving: MarketMove::from_momentum(momentum),
        age_days: parse_date(&row.price_date).map(|d| (today - d).num_days().max(0)),
    }
}

// ── Evaluation ──────────────────────────────────────────────────────────────

/// What happens to a listing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Outcome {
    Raise,
    Lower,
    /// No enabled rule matches.
    NoRule,
    /// The matching rule keeps the price.
    Kept,
    /// The rule needs a market price the listing has none of.
    NoMarketData,
    /// The market row is older than the guardrail allows.
    StaleMarket,
    /// The listing's own price is missing or zero.
    NoPrice,
    /// The rule only raises (or only lowers) and the price would go the other way.
    Held,
    /// The change is inside the dead band.
    SmallChange,
    /// Already at the target price.
    Same,
}

impl Outcome {
    pub fn is_change(self) -> bool {
        matches!(self, Outcome::Raise | Outcome::Lower)
    }

    pub fn label(self) -> &'static str {
        match self {
            Outcome::Raise => "Raise",
            Outcome::Lower => "Lower",
            Outcome::NoRule => "No rule matches",
            Outcome::Kept => "Kept by rule",
            Outcome::NoMarketData => "No market price",
            Outcome::StaleMarket => "Market data too old",
            Outcome::NoPrice => "Listing has no price",
            Outcome::Held => "Wrong direction for rule",
            Outcome::SmallChange => "Change too small",
            Outcome::Same => "Already at target",
        }
    }
}

/// The repricing result for one listing.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    /// Index of the rule that matched (also set for manual prices).
    pub rule: Option<usize>,
    /// Price typed in by hand; wins over every rule.
    pub manual: bool,
    pub old_price: f64,
    /// The computed price (equals `old_price` when nothing was computed).
    /// Only applied when `outcome.is_change()`.
    pub new_price: f64,
    /// Market reference price, for display.
    pub market: Option<f64>,
    pub outcome: Outcome,
    /// Big change that needs an explicit tick before export.
    pub review: bool,
}

impl Proposal {
    pub fn delta(&self) -> f64 {
        self.new_price - self.old_price
    }

    /// Change in % of the current price (0 when there is no current price).
    pub fn delta_pct(&self) -> f64 {
        if self.old_price > 0.0 {
            self.delta() / self.old_price * 100.0
        } else {
            0.0
        }
    }

    /// Ticked for export unless the user said otherwise.
    pub fn default_ticked(&self) -> bool {
        self.outcome.is_change() && (self.manual || !self.review)
    }
}

/// Per-rule counters for the rule list.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RuleStats {
    /// Listings this rule priced (first match).
    pub matched: usize,
    pub raised: usize,
    pub lowered: usize,
    /// Σ (new − old) × copies over its changes.
    pub value_delta: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Evaluation {
    /// One per listing, same order.
    pub proposals: Vec<Proposal>,
    /// One per rule, same order.
    pub rule_stats: Vec<RuleStats>,
}

/// Prices every listing. `manual` maps listing index → price typed by hand.
pub fn evaluate(
    ruleset: &Ruleset,
    listings: &[Listing],
    market: &HashMap<u64, LatestPrice>,
    manual: &HashMap<usize, f64>,
    today: NaiveDate,
) -> Evaluation {
    let terms = prepare_terms(ruleset);
    let mut rule_stats = vec![RuleStats::default(); ruleset.rules.len()];
    let proposals = listings
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let mv = market_view(l, market, ruleset.guardrails.reference, today);
            let rule = first_match(ruleset, &terms, l, &mv);
            if let Some(&price) = manual.get(&i) {
                return manual_proposal(l, rule, price, mv.reference);
            }
            let p = propose(ruleset, rule, l, &mv, &mut Trace::off());
            if let Some(stats) = rule.map(|r| &mut rule_stats[r]) {
                stats.matched += 1;
                match p.outcome {
                    Outcome::Raise => stats.raised += 1,
                    Outcome::Lower => stats.lowered += 1,
                    _ => {}
                }
                if p.outcome.is_change() {
                    stats.value_delta += p.delta() * l.quantity as f64;
                }
            }
            p
        })
        .collect();
    Evaluation {
        proposals,
        rule_stats,
    }
}

/// Step-by-step explanation of how one listing got its price.
pub fn explain(
    ruleset: &Ruleset,
    listing: &Listing,
    market: &HashMap<u64, LatestPrice>,
    manual: Option<f64>,
    today: NaiveDate,
) -> Vec<String> {
    let terms = prepare_terms(ruleset);
    let mv = market_view(listing, market, ruleset.guardrails.reference, today);
    let rule = first_match(ruleset, &terms, listing, &mv);
    if let Some(price) = manual {
        return vec![format!("Set by hand to {}", euro(price))];
    }
    let mut steps = Vec::new();
    match rule {
        Some(r) => steps.push(format!("Rule {}: {}", r + 1, ruleset.rules[r].name)),
        None => steps.push("No enabled rule matches; the price stays".to_string()),
    }
    propose(ruleset, rule, listing, &mv, &mut Trace::on(&mut steps));
    steps
}

fn prepare_terms(ruleset: &Ruleset) -> Vec<Vec<Vec<String>>> {
    ruleset
        .rules
        .iter()
        .map(|r| r.conditions.iter().map(Condition::terms).collect())
        .collect()
}

fn first_match(
    ruleset: &Ruleset,
    terms: &[Vec<Vec<String>>],
    l: &Listing,
    mv: &MarketView,
) -> Option<usize> {
    ruleset.rules.iter().enumerate().position(|(ri, r)| {
        r.enabled
            && r.conditions
                .iter()
                .zip(&terms[ri])
                .all(|(c, t)| c.matches(l, mv, t))
    })
}

fn manual_proposal(l: &Listing, rule: Option<usize>, price: f64, market: Option<f64>) -> Proposal {
    let (old_c, new_c) = (cents(l.price), cents(price));
    Proposal {
        rule,
        manual: true,
        old_price: l.price,
        new_price: new_c as f64 / 100.0,
        market,
        outcome: if new_c > old_c {
            Outcome::Raise
        } else if new_c < old_c {
            Outcome::Lower
        } else {
            Outcome::Same
        },
        review: false,
    }
}

/// Collects explanation steps only when asked to (the detail pane), so a
/// full evaluation allocates no strings.
struct Trace<'a>(Option<&'a mut Vec<String>>);

impl<'a> Trace<'a> {
    fn off() -> Self {
        Trace(None)
    }

    fn on(steps: &'a mut Vec<String>) -> Self {
        Trace(Some(steps))
    }

    fn note(&mut self, f: impl FnOnce() -> String) {
        if let Some(steps) = self.0.as_mut() {
            steps.push(f());
        }
    }
}

fn propose(
    rs: &Ruleset,
    rule: Option<usize>,
    l: &Listing,
    mv: &MarketView,
    trace: &mut Trace,
) -> Proposal {
    let old = l.price;
    let mut p = Proposal {
        rule,
        manual: false,
        old_price: old,
        new_price: old,
        market: mv.reference,
        outcome: Outcome::NoRule,
        review: false,
    };
    let Some(ri) = rule else { return p };
    let a = &rs.rules[ri].action;
    let g = &rs.guardrails;
    if old <= 0.0 {
        return stop(p, Outcome::NoPrice, trace);
    }

    // 1. Base price.
    let mut target = match &a.base {
        PriceBase::Keep => return stop(p, Outcome::Kept, trace),
        PriceBase::Current => {
            trace.note(|| format!("Current price {}", euro(old)));
            old
        }
        PriceBase::Fixed(v) => {
            trace.note(|| format!("Fixed price {}", euro(*v)));
            *v
        }
        PriceBase::Market {
            field,
            combine,
            other,
        } => {
            let Some(row) = mv.row else {
                return stop(p, Outcome::NoMarketData, trace);
            };
            if let Some(age) = mv
                .age_days
                .filter(|d| g.stale_days > 0 && *d > i64::from(g.stale_days))
            {
                trace.note(|| format!("Market data is {age} days old"));
                return stop(p, Outcome::StaleMarket, trace);
            }
            let first = positive(row.price_for(*field, l.is_foil));
            let second = match combine {
                Combine::Only => None,
                _ => positive(row.price_for(*other, l.is_foil)),
            };
            let value = match (combine, first, second) {
                (Combine::Only, f, _) => f,
                (Combine::HigherOf, Some(x), Some(y)) => Some(x.max(y)),
                (Combine::LowerOf, Some(x), Some(y)) => Some(x.min(y)),
                (_, x, y) => x.or(y),
            };
            let Some(v) = value else {
                return stop(p, Outcome::NoMarketData, trace);
            };
            let finish = if l.is_foil { "foil " } else { "" };
            trace.note(|| match combine {
                Combine::Only => format!("Market {finish}{} {}", field.as_str(), euro(v)),
                _ => format!(
                    "Market {finish}{} {} / {} {}, {} = {}",
                    field.as_str(),
                    opt_euro(first),
                    other.as_str(),
                    opt_euro(second),
                    if *combine == Combine::HigherOf {
                        "higher"
                    } else {
                        "lower"
                    },
                    euro(v)
                ),
            });
            v
        }
    };

    // 2. Percentage and offset.
    if (a.percent - 100.0).abs() > 1e-9 {
        target *= a.percent / 100.0;
        trace.note(|| format!("× {}% = {}", trim_num(a.percent), euro(target)));
    }
    if a.offset != 0.0 {
        target += a.offset;
        let sign = if a.offset > 0.0 { '+' } else { '−' };
        trace.note(|| format!("{sign} {} = {}", euro(a.offset.abs()), euro(target)));
    }

    // 3. Condition / language (the guide is ≈ NM, language-blind).
    if a.adjust && matches!(a.base, PriceBase::Market { .. }) {
        let cf = rs.adjustments.condition_factor(&l.condition);
        if (cf - 1.0).abs() > 1e-9 {
            target *= cf;
            trace.note(|| {
                format!(
                    "{} × {}% = {}",
                    l.condition,
                    trim_num(cf * 100.0),
                    euro(target)
                )
            });
        }
        let lf = rs.adjustments.language_factor(&l.language);
        if (lf - 1.0).abs() > 1e-9 {
            target *= lf;
            trace.note(|| {
                format!(
                    "{} × {}% = {}",
                    l.language,
                    trim_num(lf * 100.0),
                    euro(target)
                )
            });
        }
    }

    // 4. The rule's own limits.
    if let Some(min) = a.min_price.filter(|m| target < *m) {
        target = min;
        trace.note(|| format!("Rule minimum {}", euro(min)));
    }
    if let Some(max) = a.max_price.filter(|m| target > *m) {
        target = max;
        trace.note(|| format!("Rule maximum {}", euro(max)));
    }
    if let Some(cap) = a.max_raise_pct.map(|r| old * (1.0 + r / 100.0)) {
        if target > cap {
            target = cap;
            trace.note(|| {
                format!(
                    "Capped at +{}% = {}",
                    trim_num(a.max_raise_pct.unwrap_or(0.0)),
                    euro(cap)
                )
            });
        }
    }
    if let Some(cap) = a.max_drop_pct.map(|d| old * (1.0 - d / 100.0)) {
        if target < cap {
            target = cap;
            trace.note(|| {
                format!(
                    "Capped at −{}% = {}",
                    trim_num(a.max_drop_pct.unwrap_or(0.0)),
                    euro(cap)
                )
            });
        }
    }

    // 5. Direction. Floors still win over it.
    let floor = g.floors.floor_for(&l.rarity);
    let below_floor = cents(old) < cents(floor);
    let held = match a.direction {
        Direction::Both => false,
        Direction::RaiseOnly => cents(target) < cents(old),
        Direction::LowerOnly => cents(target) > cents(old),
    };
    if held {
        trace.note(|| format!("{}: keeps {}", a.direction.label(), euro(old)));
        if !below_floor {
            p.new_price = old;
            p.outcome = Outcome::Held;
            return p;
        }
        target = old;
    }

    // 6. Rarity floor, then rounding (which never undercuts the floor).
    if target < floor {
        target = floor;
        trace.note(|| format!("Floor for {} {}", rarity_label(&l.rarity), euro(floor)));
    }
    let floor_c = cents(floor);
    let raw_c = cents(target);
    let mut new_c = round_cents(raw_c, g.rounding, RoundMode::Nearest);
    if new_c < floor_c {
        new_c = round_cents(floor_c, g.rounding, RoundMode::Up);
    }
    if let Some(max_c) = a.max_price.map(cents) {
        let down = round_cents(max_c, g.rounding, RoundMode::Down);
        if new_c > max_c && down >= floor_c {
            new_c = down;
        }
    }
    new_c = new_c.min(cents(MAX_PRICE_EUR));
    if new_c != raw_c {
        trace.note(|| {
            format!(
                "Rounded ({}) = {}",
                g.rounding.label().to_lowercase(),
                euro(new_c as f64 / 100.0)
            )
        });
    }
    p.new_price = new_c as f64 / 100.0;

    // 7. Compare with the current price.
    let old_c = cents(old);
    if new_c == old_c {
        return stop(p, Outcome::Same, trace);
    }
    let delta_eur = (new_c - old_c).abs() as f64 / 100.0;
    let pct = delta_eur / old * 100.0;
    let tiny = delta_eur + 1e-9 < g.min_change_eur || pct + 1e-9 < g.min_change_pct;
    if tiny && !below_floor {
        return stop(p, Outcome::SmallChange, trace);
    }
    p.review = pct > g.review_pct && delta_eur + 1e-9 >= g.review_min_eur;
    p.outcome = if new_c > old_c {
        Outcome::Raise
    } else {
        Outcome::Lower
    };
    trace.note(|| {
        format!(
            "{} {} to {} ({:+.1}%){}",
            p.outcome.label(),
            euro(old),
            euro(p.new_price),
            p.delta_pct(),
            if p.review { " — needs review" } else { "" }
        )
    });
    p
}

/// Ends a proposal early with a non-change outcome.
fn stop(mut p: Proposal, outcome: Outcome, trace: &mut Trace) -> Proposal {
    trace.note(|| outcome.label().to_string());
    p.outcome = outcome;
    p
}

// ── Summary & export ────────────────────────────────────────────────────────

/// Totals for the review header.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub listings: usize,
    pub copies: i64,
    pub raises: usize,
    pub lowers: usize,
    /// Changes waiting for an explicit tick.
    pub review: usize,
    /// Changes ticked for export, and their copies.
    pub selected: usize,
    pub selected_copies: i64,
    /// Σ current price × copies over all listings.
    pub value_before: f64,
    /// Same, with the ticked changes applied.
    pub value_after: f64,
    /// Listings per non-change outcome.
    pub skipped: BTreeMap<&'static str, usize>,
}

impl Summary {
    pub fn changes(&self) -> usize {
        self.raises + self.lowers
    }
}

pub fn summarize(
    eval: &Evaluation,
    listings: &[Listing],
    is_ticked: impl Fn(usize, &Proposal) -> bool,
) -> Summary {
    let mut s = Summary {
        listings: listings.len(),
        ..Summary::default()
    };
    for (i, (p, l)) in eval.proposals.iter().zip(listings).enumerate() {
        let qty = l.quantity as f64;
        s.copies += l.quantity;
        s.value_before += p.old_price * qty;
        match p.outcome {
            Outcome::Raise => s.raises += 1,
            Outcome::Lower => s.lowers += 1,
            other => *s.skipped.entry(other.label()).or_insert(0) += 1,
        }
        let ticked = p.outcome.is_change() && is_ticked(i, p);
        if p.outcome.is_change() && p.review && !ticked {
            s.review += 1;
        }
        if ticked {
            s.selected += 1;
            s.selected_copies += l.quantity;
            s.value_after += p.new_price * qty;
        } else {
            s.value_after += p.old_price * qty;
        }
    }
    s
}

/// One price change to export.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PriceChange {
    /// Index of the row in the loaded CSV.
    pub row: usize,
    pub old_price: f64,
    pub new_price: f64,
}

/// The ticked changes, in CSV order.
pub fn ticked_changes(
    eval: &Evaluation,
    listings: &[Listing],
    is_ticked: impl Fn(usize, &Proposal) -> bool,
) -> Vec<PriceChange> {
    let mut out: Vec<PriceChange> = eval
        .proposals
        .iter()
        .zip(listings)
        .enumerate()
        .filter(|(i, (p, _))| p.outcome.is_change() && is_ticked(*i, p))
        .map(|(_, (p, l))| PriceChange {
            row: l.row,
            old_price: p.old_price,
            new_price: p.new_price,
        })
        .collect();
    out.sort_by_key(|c| c.row);
    out
}

// ── Rounding ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundMode {
    Nearest,
    Up,
    Down,
}

/// Rounds a price in cents to the rounding grid. Never returns less than the
/// smallest grid value (5, 10 or 9 cents; 1 for `Cent`).
pub fn round_cents(c: i64, rounding: Rounding, mode: RoundMode) -> i64 {
    let (step, offset) = match rounding {
        Rounding::Cent => return c.max(1),
        Rounding::FiveCents => (5, 0),
        Rounding::TenCents => (10, 0),
        Rounding::EndsIn9 => (10, 9),
    };
    // Grid values are `offset + k·step` for k ≥ 0 (k ≥ 1 when offset is 0).
    let k_min = if offset == 0 { 1 } else { 0 };
    let x = (c - offset) as f64 / step as f64;
    let k = match mode {
        RoundMode::Nearest => x.round(),
        RoundMode::Up => x.ceil(),
        RoundMode::Down => x.floor(),
    } as i64;
    offset + k.max(k_min) * step
}

// ── Small helpers ───────────────────────────────────────────────────────────

/// Price in whole cents.
pub fn cents(p: f64) -> i64 {
    (p * 100.0).round() as i64
}

pub fn euro(p: f64) -> String {
    format!("€{p:.2}")
}

fn opt_euro(p: Option<f64>) -> String {
    p.map_or_else(|| "—".to_string(), euro)
}

fn euro_range(label: &str, min: Option<f64>, max: Option<f64>) -> String {
    match (min, max) {
        (Some(a), Some(b)) => format!("{label} {}–{}", euro(a), euro(b)),
        (Some(a), None) => format!("{label} ≥ {}", euro(a)),
        (None, Some(b)) => format!("{label} ≤ {}", euro(b)),
        (None, None) => format!("{label}: any"),
    }
}

/// `95` instead of `95.0`, `12.5` stays.
fn trim_num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

fn rarity_label(rarity: &str) -> &str {
    if rarity.is_empty() {
        "unknown rarity"
    } else {
        rarity
    }
}

/// Parses a CSV price, accepting `.` or `,` as decimal separator.
pub fn parse_price(s: &str) -> Option<f64> {
    s.trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|p| p.is_finite() && *p > 0.0)
}

fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s.trim().get(..10)?, "%Y-%m-%d").ok()
}

fn positive(p: Option<f64>) -> Option<f64> {
    p.filter(|v| v.is_finite() && *v > 0.0)
}

fn in_range(v: f64, min: Option<f64>, max: Option<f64>) -> bool {
    min.is_none_or(|m| v >= m) && max.is_none_or(|m| v <= m)
}

fn split_terms(s: &str) -> Vec<String> {
    s.split(',')
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect()
}

/// True when no terms are given or any term is contained in `hay`.
fn contains_any(hay: &str, terms: &[String]) -> bool {
    terms.is_empty() || terms.iter().any(|t| hay.contains(t.as_str()))
}

fn clean(v: f64, lo: f64, hi: f64, fallback: f64) -> f64 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        fallback
    }
}

fn clean_price_opt(v: Option<f64>) -> Option<f64> {
    v.filter(|p| p.is_finite())
        .map(|p| p.clamp(0.0, MAX_PRICE_EUR))
}

fn truncate_chars(s: &mut String, max: usize) {
    if let Some((idx, _)) = s.char_indices().nth(max) {
        s.truncate(idx);
    }
}

#[cfg(test)]
#[path = "repricing_tests.rs"]
mod tests;
