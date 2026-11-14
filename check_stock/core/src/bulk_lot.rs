//! Bulk lot builder — which inventory copies go into a lot such as
//! "100 commons" or "100 non-basic lands".
//!
//! Pure logic: no egui, no database, no network. The screen feeds it the
//! loaded inventory CSV rows (as [`Candidate`]s), the user's [`LotSpec`], and
//! gets back the picks, a summary and a filter funnel that explains a
//! shortfall.
//!
//! Selection, in order:
//! 1. **Guaranteed hits** (optional): copies worth at least the hit
//!    threshold, cheapest first, so the promise costs as little as possible.
//! 2. **Filler**: copies at or below the price cap, by default the ones listed
//!    the longest first and then the cheapest — dead stock goes first.
//!
//! Both respect the per-name limit ("no repeats" = one copy per card name,
//! across printings) and never take more copies than a row has left after
//! the copies already promised to other lots.

use crate::bin_consolidation::{bin_distance, split_location};
use crate::card_matching::MatchedCard;
use crate::formatters::format_update_stock_csv;
use crate::inventory_db::{card_variant_key, purchase_lot_of, PendingRemoval};
use crate::models::{canonical_condition, Card};
use chrono::NaiveDate;
use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Lot size offered by default.
pub const DEFAULT_LOT_SIZE: usize = 100;
/// Largest lot the builder offers (one picking session).
pub const MAX_LOT_SIZE: usize = 1000;

/// Card rarity as far as lots care: the four booster rarities and the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Mythic,
    /// Tokens, basic lands (Cardmarket rarity "Land"), specials, timeshifted…
    Other,
}

impl Rarity {
    pub const ALL: [Rarity; 5] = [
        Rarity::Common,
        Rarity::Uncommon,
        Rarity::Rare,
        Rarity::Mythic,
        Rarity::Other,
    ];

    /// Parses Cardmarket's rarity column (`"Common"`, `"Mythic"`, …).
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "common" | "c" => Rarity::Common,
            "uncommon" | "u" => Rarity::Uncommon,
            "rare" | "r" => Rarity::Rare,
            "mythic" | "mythic rare" | "m" => Rarity::Mythic,
            _ => Rarity::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Rarity::Common => "Common",
            Rarity::Uncommon => "Uncommon",
            Rarity::Rare => "Rare",
            Rarity::Mythic => "Mythic",
            Rarity::Other => "Other",
        }
    }
}

/// Card type a lot is restricted to. Anything but [`CardType::Any`] needs the
/// card's type line, which the Cardmarket export does not carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardType {
    Any,
    NonBasicLand,
    Creature,
    InstantSorcery,
    Artifact,
    Enchantment,
    Planeswalker,
    NonLand,
}

impl CardType {
    pub const ALL: [CardType; 8] = [
        CardType::Any,
        CardType::NonBasicLand,
        CardType::Creature,
        CardType::InstantSorcery,
        CardType::Artifact,
        CardType::Enchantment,
        CardType::Planeswalker,
        CardType::NonLand,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CardType::Any => "Any type",
            CardType::NonBasicLand => "Non-basic lands",
            CardType::Creature => "Creatures",
            CardType::InstantSorcery => "Instants & sorceries",
            CardType::Artifact => "Artifacts",
            CardType::Enchantment => "Enchantments",
            CardType::Planeswalker => "Planeswalkers",
            CardType::NonLand => "Non-land cards",
        }
    }

    /// Whether matching needs the card's type line.
    pub fn needs_type_line(self) -> bool {
        self != CardType::Any
    }

    /// Whether a front-face type line (`"Artifact Creature — Golem"`) fits.
    /// Only the card types before the dash count, never subtypes.
    pub fn matches(self, front_type: &str) -> bool {
        let has = |t: &str| has_type(front_type, t);
        match self {
            CardType::Any => true,
            CardType::NonBasicLand => has("Land") && !has("Basic"),
            CardType::Creature => has("Creature"),
            CardType::InstantSorcery => has("Instant") || has("Sorcery"),
            CardType::Artifact => has("Artifact"),
            CardType::Enchantment => has("Enchantment"),
            CardType::Planeswalker => has("Planeswalker"),
            CardType::NonLand => !has("Land"),
        }
    }
}

/// Which finishes may go into the lot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoilFilter {
    NonFoil,
    FoilOnly,
    Any,
}

impl FoilFilter {
    pub const ALL: [FoilFilter; 3] = [FoilFilter::NonFoil, FoilFilter::FoilOnly, FoilFilter::Any];

    pub fn label(self) -> &'static str {
        match self {
            FoilFilter::NonFoil => "Non-foil only",
            FoilFilter::FoilOnly => "Foil only",
            FoilFilter::Any => "Foil and non-foil",
        }
    }

    fn allows(self, is_foil: bool) -> bool {
        match self {
            FoilFilter::NonFoil => !is_foil,
            FoilFilter::FoilOnly => is_foil,
            FoilFilter::Any => true,
        }
    }
}

/// Which matching cards are taken first. Cards equal by the chosen order
/// are drawn in a random order (fixed per [`LotSpec::seed`]), never by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LotOrder {
    /// From as few neighbouring bins as possible — the quickest lot to pick.
    Nearby,
    /// Longest in stock first, then cheapest — clears dead stock.
    OldestFirst,
    /// Cheapest first, then longest in stock.
    CheapestFirst,
    /// Shuffled.
    Random,
}

impl LotOrder {
    pub const ALL: [LotOrder; 4] = [
        LotOrder::Nearby,
        LotOrder::OldestFirst,
        LotOrder::CheapestFirst,
        LotOrder::Random,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LotOrder::Nearby => "Closest together",
            LotOrder::OldestFirst => "Oldest first",
            LotOrder::CheapestFirst => "Cheapest first",
            LotOrder::Random => "Random",
        }
    }
}

/// Conditions in Cardmarket order, best first (canonical short codes).
pub const CONDITIONS: [&str; 6] = ["NM", "EX", "GD", "LP", "PL", "PO"];

/// What the user asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct LotSpec {
    /// Copies in the lot.
    pub size: usize,
    pub rarities: BTreeSet<Rarity>,
    pub card_type: CardType,
    /// Leave out basic lands (recognised by name, or by type line when known).
    pub exclude_basic_lands: bool,
    pub foil: FoilFilter,
    /// Canonical conditions (`"PL"`) not allowed in the lot.
    pub excluded_conditions: BTreeSet<String>,
    /// Languages (as in the CSV) not allowed in the lot.
    pub excluded_languages: BTreeSet<String>,
    /// Filler copies cost at most this much each, in EUR.
    pub max_price: f64,
    /// Filler and hits must have been listed at least this many days.
    pub min_age_days: i64,
    /// Copies allowed per card name (1 = no repeats).
    pub max_copies_per_name: i64,
    /// Guaranteed copies worth at least `hit_min_price` each.
    pub hits: usize,
    pub hit_min_price: f64,
    pub order: LotOrder,
    /// Seed for the random draw among equally good cards (and for
    /// [`LotOrder::Random`]); a new seed gives a different mix.
    pub seed: u64,
}

impl Default for LotSpec {
    fn default() -> Self {
        Self {
            size: DEFAULT_LOT_SIZE,
            rarities: BTreeSet::from([Rarity::Common]),
            card_type: CardType::Any,
            exclude_basic_lands: true,
            foil: FoilFilter::NonFoil,
            excluded_conditions: BTreeSet::new(),
            excluded_languages: BTreeSet::new(),
            max_price: 0.25,
            min_age_days: 90,
            max_copies_per_name: 1,
            hits: 0,
            hit_min_price: 0.50,
            order: LotOrder::Nearby,
            seed: 1,
        }
    }
}

/// One-click starting points for common lots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Commons,
    Uncommons,
    NonBasicLands,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::Commons, Preset::Uncommons, Preset::NonBasicLands];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Commons => "100 commons",
            Preset::Uncommons => "50 uncommons",
            Preset::NonBasicLands => "100 non-basic lands",
        }
    }

    /// Sets what defines the lot; leaves condition, language, finish, age and
    /// order as the user chose them.
    pub fn apply(self, spec: &mut LotSpec) {
        spec.exclude_basic_lands = true;
        spec.max_copies_per_name = 1;
        match self {
            Preset::Commons => {
                spec.size = 100;
                spec.rarities = BTreeSet::from([Rarity::Common]);
                spec.card_type = CardType::Any;
                spec.max_price = 0.25;
            }
            Preset::Uncommons => {
                spec.size = 50;
                spec.rarities = BTreeSet::from([Rarity::Uncommon]);
                spec.card_type = CardType::Any;
                spec.max_price = 0.30;
            }
            Preset::NonBasicLands => {
                spec.size = 100;
                spec.rarities = BTreeSet::from([
                    Rarity::Common,
                    Rarity::Uncommon,
                    Rarity::Rare,
                    Rarity::Mythic,
                ]);
                spec.card_type = CardType::NonBasicLand;
                spec.max_price = 0.25;
            }
        }
    }
}

/// An inventory CSV row that could go into a lot.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Index of the row in the loaded CSV.
    pub source: usize,
    /// Article key of the variant (see [`card_variant_key`]).
    pub variant: String,
    pub name: String,
    /// Name used for the repeat limit (case- and spacing-insensitive).
    pub name_key: String,
    pub set_name: String,
    pub set_code: String,
    pub cn: String,
    pub rarity: Rarity,
    /// Canonical condition (`"NM"`).
    pub condition: String,
    pub language: String,
    pub is_foil: bool,
    /// Listed unit price, in EUR.
    pub price: f64,
    /// Days since listing (or first import); `None` when unknown.
    pub age_days: Option<i64>,
    /// Copies free for a new lot.
    pub available: i64,
    /// Copies already promised to lots no sync has seen leave yet.
    pub reserved: i64,
    pub location: String,
    /// Purchase lot (`"L12"`), `""` when the location names none.
    pub purchase_lot: String,
    /// Front-face type line, once looked up.
    pub front_type: Option<String>,
}

impl Candidate {
    /// Basic land, by name or — when known — by type line.
    pub fn is_basic_land(&self) -> bool {
        is_basic_land_name(&self.name)
            || self
                .front_type
                .as_deref()
                .is_some_and(|t| has_type(t, "Basic") && has_type(t, "Land"))
    }
}

/// Whether a type line names `t` among its super/card types (before the
/// dash), so subtypes never match.
fn has_type(front_type: &str, t: &str) -> bool {
    front_type
        .split('—')
        .next()
        .unwrap_or("")
        .split_whitespace()
        .any(|w| w.eq_ignore_ascii_case(t))
}

/// Basic land names, including Wastes and the snow-covered versions.
pub fn is_basic_land_name(name: &str) -> bool {
    let front = name.split(" // ").next().unwrap_or(name).trim();
    let base = front.strip_prefix("Snow-Covered ").unwrap_or(front).trim();
    ["Plains", "Island", "Swamp", "Mountain", "Forest", "Wastes"]
        .iter()
        .any(|b| b.eq_ignore_ascii_case(base))
}

/// Key for the "no repeats" rule: printings of one card share it.
pub fn name_key(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn parse_price(price: &str) -> Option<f64> {
    price
        .trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|p| p.is_finite() && *p >= 0.0)
}

fn parse_day(s: &str) -> Option<NaiveDate> {
    let day = s.trim().get(..10)?;
    NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()
}

/// Turns inventory CSV rows into lot candidates.
///
/// Rows without stock or a readable price are skipped. Age comes from the
/// row's `listedAt`, else from `listing_dates` (the database's first-seen
/// date per variant, keyed by article key). Copies promised to lots awaiting
/// a sync (`pending`) are taken out, matching the same location first.
pub fn build_candidates(
    cards: &[Card],
    category: &str,
    listing_dates: &HashMap<String, String>,
    pending: &[PendingRemoval],
    today: NaiveDate,
) -> Vec<Candidate> {
    let mut candidates: Vec<Candidate> = cards
        .iter()
        .enumerate()
        .filter_map(|(source, card)| {
            let quantity: i64 = card.quantity.trim().parse().ok().filter(|q| *q > 0)?;
            let price = parse_price(&card.price)?;
            let variant = card_variant_key(card, category);
            let listed = parse_day(&card.listed_at)
                .or_else(|| listing_dates.get(&variant).and_then(|d| parse_day(d)));
            let location = card.location.as_deref().unwrap_or("").trim().to_string();
            Some(Candidate {
                source,
                name: card.name.trim().to_string(),
                name_key: name_key(&card.name),
                set_name: card.set.clone(),
                set_code: card.set_code.clone(),
                cn: card.cn.clone(),
                rarity: Rarity::parse(&card.rarity),
                condition: canonical_condition(&card.condition),
                language: card.language.trim().to_string(),
                is_foil: card.is_foil_card(),
                price,
                age_days: listed.map(|d| (today - d).num_days().max(0)),
                available: quantity,
                reserved: 0,
                purchase_lot: purchase_lot_of(&location).unwrap_or("").to_string(),
                location,
                front_type: None,
                variant,
            })
        })
        .collect();
    reserve_pending(&mut candidates, pending);
    candidates
}

/// Takes copies promised to lots awaiting a sync out of the candidates: from
/// the row at the recorded location first, then from any row of the variant.
fn reserve_pending(candidates: &mut [Candidate], pending: &[PendingRemoval]) {
    for removal in pending {
        let mut left = removal.pending;
        for same_location in [true, false] {
            for c in candidates.iter_mut() {
                if left == 0 {
                    break;
                }
                if c.variant != removal.variant || (same_location && c.location != removal.location)
                {
                    continue;
                }
                let take = left.min(c.available);
                c.available -= take;
                c.reserved += take;
                left -= take;
            }
        }
    }
}

// ── Matching ────────────────────────────────────────────────────────────────

fn passes_rarity(c: &Candidate, spec: &LotSpec) -> bool {
    spec.rarities.contains(&c.rarity)
}

/// Type filter plus the basic-land rule. A card whose type line is needed but
/// unknown does not pass.
fn passes_type(c: &Candidate, spec: &LotSpec) -> bool {
    if spec.exclude_basic_lands && c.is_basic_land() {
        return false;
    }
    if !spec.card_type.needs_type_line() {
        return true;
    }
    c.front_type
        .as_deref()
        .is_some_and(|t| spec.card_type.matches(t))
}

fn passes_finish(c: &Candidate, spec: &LotSpec) -> bool {
    spec.foil.allows(c.is_foil)
        && !spec.excluded_conditions.contains(&c.condition)
        && !spec.excluded_languages.contains(&c.language)
}

fn passes_age(c: &Candidate, spec: &LotSpec) -> bool {
    spec.min_age_days <= 0 || c.age_days.is_some_and(|a| a >= spec.min_age_days)
}

/// Every filter except price.
fn base_match(c: &Candidate, spec: &LotSpec) -> bool {
    c.available > 0
        && passes_rarity(c, spec)
        && passes_type(c, spec)
        && passes_finish(c, spec)
        && passes_age(c, spec)
}

fn is_filler(c: &Candidate, spec: &LotSpec) -> bool {
    base_match(c, spec) && c.price <= spec.max_price + 1e-9
}

fn is_hit(c: &Candidate, spec: &LotSpec) -> bool {
    base_match(c, spec) && c.price + 1e-9 >= spec.hit_min_price
}

/// Candidates whose type line the current filters need but nobody has
/// looked up yet — one index per printing (set code + collector number).
/// Only candidates that pass every other filter are listed, which keeps the
/// lookup small.
pub fn type_lookups_needed(candidates: &[Candidate], spec: &LotSpec) -> Vec<usize> {
    if !spec.card_type.needs_type_line() {
        return Vec::new();
    }
    let mut seen = HashSet::new();
    candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            c.front_type.is_none()
                && c.available > 0
                && !c.set_code.trim().is_empty()
                && !c.cn.trim().is_empty()
                && passes_rarity(c, spec)
                && !(spec.exclude_basic_lands && is_basic_land_name(&c.name))
                && passes_finish(c, spec)
                && passes_age(c, spec)
                && (c.price <= spec.max_price + 1e-9
                    || (spec.hits > 0 && c.price + 1e-9 >= spec.hit_min_price))
        })
        .filter_map(|(i, c)| {
            let key = (c.set_code.trim().to_lowercase(), c.cn.trim().to_string());
            seen.insert(key).then_some(i)
        })
        .collect()
}

// ── Funnel ──────────────────────────────────────────────────────────────────

/// Copies left after one filter.
#[derive(Debug, Clone, PartialEq)]
pub struct FunnelStep {
    pub label: String,
    pub copies: i64,
}

/// How many copies survive each filter in turn — tells the user which one to
/// loosen when the lot comes up short.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Funnel {
    pub steps: Vec<FunnelStep>,
    /// Copies set aside for lots awaiting a sync.
    pub reserved: i64,
    /// Copies of the requested rarity whose type line is still unknown.
    pub types_unknown: i64,
    /// Copies eligible as guaranteed hits.
    pub hit_pool: i64,
}

impl Funnel {
    /// The first filter that leaves fewer than `size` copies.
    pub fn bottleneck(&self, size: usize) -> Option<&FunnelStep> {
        self.steps.iter().find(|s| s.copies < size as i64)
    }
}

pub fn funnel(candidates: &[Candidate], spec: &LotSpec) -> Funnel {
    let in_stock: Vec<&Candidate> = candidates.iter().filter(|c| c.available > 0).collect();
    let sum = |cs: &[&Candidate]| cs.iter().map(|c| c.available).sum::<i64>();
    let mut steps = vec![FunnelStep {
        label: "In stock".to_string(),
        copies: sum(&in_stock),
    }];
    let mut push = |label: String, cs: &[&Candidate]| {
        steps.push(FunnelStep {
            label,
            copies: sum(cs),
        })
    };

    let rarity: Vec<&Candidate> = in_stock
        .into_iter()
        .filter(|c| passes_rarity(c, spec))
        .collect();
    let rarity_labels: Vec<&str> = Rarity::ALL
        .iter()
        .filter(|r| spec.rarities.contains(r))
        .map(|r| r.label())
        .collect();
    push(
        if rarity_labels.is_empty() {
            "Rarity (none selected)".to_string()
        } else {
            rarity_labels.join(", ")
        },
        &rarity,
    );

    let types_unknown = if spec.card_type.needs_type_line() {
        rarity
            .iter()
            .filter(|c| c.front_type.is_none())
            .map(|c| c.available)
            .sum()
    } else {
        0
    };
    let typed: Vec<&Candidate> = rarity
        .into_iter()
        .filter(|c| passes_type(c, spec))
        .collect();
    if spec.card_type.needs_type_line() || spec.exclude_basic_lands {
        let label = match (spec.card_type, spec.exclude_basic_lands) {
            (CardType::Any, _) => "Basic lands excluded".to_string(),
            (t, _) => t.label().to_string(),
        };
        push(label, &typed);
    }

    let finish: Vec<&Candidate> = typed
        .into_iter()
        .filter(|c| passes_finish(c, spec))
        .collect();
    push(
        format!("{}, condition & language", spec.foil.label()),
        &finish,
    );

    let aged: Vec<&Candidate> = finish.into_iter().filter(|c| passes_age(c, spec)).collect();
    if spec.min_age_days > 0 {
        push(format!("Listed ≥ {} days", spec.min_age_days), &aged);
    }
    let hit_pool = if spec.hits > 0 {
        aged.iter()
            .filter(|c| c.price + 1e-9 >= spec.hit_min_price)
            .map(|c| c.available)
            .sum()
    } else {
        0
    };

    let cheap: Vec<&Candidate> = aged
        .into_iter()
        .filter(|c| c.price <= spec.max_price + 1e-9)
        .collect();
    push(format!("≤ €{:.2} per card", spec.max_price), &cheap);

    let mut per_name: HashMap<&str, i64> = HashMap::new();
    for c in &cheap {
        *per_name.entry(c.name_key.as_str()).or_insert(0) += c.available;
    }
    let limit = spec.max_copies_per_name.max(1);
    steps.push(FunnelStep {
        label: if limit == 1 {
            "No repeated card names".to_string()
        } else {
            format!("≤ {limit} copies per card name")
        },
        copies: per_name.values().map(|n| (*n).min(limit)).sum(),
    });

    Funnel {
        steps,
        reserved: candidates.iter().map(|c| c.reserved).sum(),
        types_unknown,
        hit_pool,
    }
}

// ── Selection ───────────────────────────────────────────────────────────────

/// Copies of one candidate in the lot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LotPick {
    /// Index into the candidate list.
    pub candidate: usize,
    pub copies: i64,
    /// Chosen as a guaranteed hit (replaced by a hit when swapped out).
    pub hit: bool,
}

/// Picks a lot from scratch.
pub fn build_lot(candidates: &[Candidate], spec: &LotSpec) -> Vec<LotPick> {
    top_up(candidates, spec, &[], &HashSet::new())
}

/// New picks that bring `existing` back up to the spec (missing hits first,
/// then filler), never using a candidate in `excluded` — the rows the user
/// could not find or did not want. Returns only the additions.
pub fn top_up(
    candidates: &[Candidate],
    spec: &LotSpec,
    existing: &[LotPick],
    excluded: &HashSet<usize>,
) -> Vec<LotPick> {
    let mut used: HashMap<usize, i64> = HashMap::new();
    let mut per_name: HashMap<&str, i64> = HashMap::new();
    let mut total = 0;
    let mut hits = 0;
    for p in existing {
        *used.entry(p.candidate).or_insert(0) += p.copies;
        if let Some(c) = candidates.get(p.candidate) {
            *per_name.entry(c.name_key.as_str()).or_insert(0) += p.copies;
        }
        total += p.copies;
        if p.hit {
            hits += p.copies;
        }
    }

    let size = spec.size as i64;
    let limit = spec.max_copies_per_name.max(1);
    let mut added = Vec::new();
    let mut take = |pool: Vec<usize>, wanted: i64, hit: bool, added: &mut Vec<LotPick>| {
        let mut left = wanted;
        for i in pool {
            if left <= 0 {
                break;
            }
            let c = &candidates[i];
            let free = c.available - used.get(&i).copied().unwrap_or(0);
            let name_left = limit - per_name.get(c.name_key.as_str()).copied().unwrap_or(0);
            let n = free.min(name_left).min(left);
            if n <= 0 {
                continue;
            }
            *used.entry(i).or_insert(0) += n;
            *per_name.entry(c.name_key.as_str()).or_insert(0) += n;
            added.push(LotPick {
                candidate: i,
                copies: n,
                hit,
            });
            left -= n;
        }
        wanted - left
    };

    // "Closest together": every candidate's walking distance to where the lot
    // is gathered — the existing picks' bins when topping up, else the most
    // compact stretch of bins that can fill the lot on its own.
    let distance = (spec.order == LotOrder::Nearby).then(|| {
        let anchors: Vec<String> = if existing.is_empty() {
            let filler: Vec<usize> = (0..candidates.len())
                .filter(|i| !excluded.contains(i) && is_filler(&candidates[*i], spec))
                .collect();
            let need = (size - (spec.hits as i64).min(size)).max(1);
            best_window(candidates, &filler, need, limit)
        } else {
            let bins: BTreeSet<String> = existing
                .iter()
                .filter_map(|p| candidates.get(p.candidate).and_then(bin_of))
                .collect();
            bins.into_iter().collect()
        };
        distances_to(candidates, &anchors)
    });

    let wanted_hits = (spec.hits as i64).min(size) - hits;
    if wanted_hits > 0 {
        let pool = hit_pool(candidates, spec, excluded, distance.as_deref());
        total += take(pool, wanted_hits.min(size - total), true, &mut added);
    }
    if total < size {
        let pool = filler_pool(candidates, spec, excluded, distance.as_deref());
        take(pool, size - total, false, &mut added);
    }
    added
}

/// Hit candidates: cheapest first (keeps the promise as cheap as possible),
/// then longest in stock — or, for a lot gathered by location, nearest first.
fn hit_pool(
    candidates: &[Candidate],
    spec: &LotSpec,
    excluded: &HashSet<usize>,
    distance: Option<&[i64]>,
) -> Vec<usize> {
    let mut pool: Vec<usize> = (0..candidates.len())
        .filter(|i| !excluded.contains(i) && is_hit(&candidates[*i], spec))
        .collect();
    pool.sort_by(|&i, &j| {
        let (a, b) = (&candidates[i], &candidates[j]);
        let near = distance.map_or(Ordering::Equal, |d| d[i].cmp(&d[j]));
        near.then_with(|| a.price.total_cmp(&b.price))
            .then_with(|| older_first(a, b))
            .then_with(|| random_tie(spec.seed, a, b))
    });
    pool
}

fn filler_pool(
    candidates: &[Candidate],
    spec: &LotSpec,
    excluded: &HashSet<usize>,
    distance: Option<&[i64]>,
) -> Vec<usize> {
    let mut pool: Vec<usize> = (0..candidates.len())
        .filter(|i| !excluded.contains(i) && is_filler(&candidates[*i], spec))
        .collect();
    match (spec.order, distance) {
        (LotOrder::Nearby, Some(d)) => {
            // Nearest bins first, each walked in shelf order.
            let mut keyed: Vec<(i64, LocationKey, u64, usize)> = pool
                .iter()
                .map(|&i| {
                    let c = &candidates[i];
                    (
                        d[i],
                        location_sort_key(&c.location),
                        tie_key(spec.seed, c),
                        i,
                    )
                })
                .collect();
            keyed.sort();
            pool = keyed.into_iter().map(|(.., i)| i).collect();
        }
        (LotOrder::OldestFirst | LotOrder::Nearby, _) => pool.sort_by(|&a, &b| {
            let (a, b) = (&candidates[a], &candidates[b]);
            older_first(a, b)
                .then_with(|| a.price.total_cmp(&b.price))
                .then_with(|| random_tie(spec.seed, a, b))
        }),
        (LotOrder::CheapestFirst, _) => pool.sort_by(|&a, &b| {
            let (a, b) = (&candidates[a], &candidates[b]);
            a.price
                .total_cmp(&b.price)
                .then_with(|| older_first(a, b))
                .then_with(|| random_tie(spec.seed, a, b))
        }),
        (LotOrder::Random, _) => shuffle(&mut pool, spec.seed),
    }
    pool
}

/// Longest in stock first; unknown ages last.
fn older_first(a: &Candidate, b: &Candidate) -> Ordering {
    Reverse(a.age_days.unwrap_or(-1)).cmp(&Reverse(b.age_days.unwrap_or(-1)))
}

/// Order among otherwise equal cards: random but fixed per seed. Ordering by
/// name here would fill a lot of identical commons with cards starting "A".
fn random_tie(seed: u64, a: &Candidate, b: &Candidate) -> Ordering {
    tie_key(seed, a)
        .cmp(&tie_key(seed, b))
        .then_with(|| a.source.cmp(&b.source))
}

fn tie_key(seed: u64, c: &Candidate) -> u64 {
    splitmix(seed ^ (c.source as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The bin a candidate sits in (`A-0-0-12` of `A-0-0-12-L15-R`), if its
/// location has one.
fn bin_of(c: &Candidate) -> Option<String> {
    split_location(&c.location).map(|(bin, _)| bin)
}

/// Walking distance from every candidate to the nearest anchor bin
/// (weighted aisle/shelf/row/column steps, as in bin consolidation).
/// Candidates without a bin are farthest; with no anchors, all are equal.
fn distances_to(candidates: &[Candidate], anchors: &[String]) -> Vec<i64> {
    let mut per_bin: HashMap<String, i64> = HashMap::new();
    candidates
        .iter()
        .map(|c| match bin_of(c) {
            None => i64::MAX,
            Some(_) if anchors.is_empty() => 0,
            Some(bin) => *per_bin.entry(bin).or_insert_with_key(|bin| {
                anchors
                    .iter()
                    .map(|a| bin_distance(bin, a))
                    .min()
                    .unwrap_or(0)
            }),
        })
        .collect()
}

/// The most compact run of neighbouring bins (in shelf order) whose cards in
/// `pool` can fill `need` copies on their own, honouring the per-name limit:
/// the shortest walk from its first to its last bin, then the fewest bins.
/// Returns its bins, or every bin when even all of them fall short.
fn best_window(candidates: &[Candidate], pool: &[usize], need: i64, limit: i64) -> Vec<String> {
    let mut by_bin: HashMap<String, Vec<usize>> = HashMap::new();
    for &i in pool {
        if let Some(bin) = bin_of(&candidates[i]) {
            by_bin.entry(bin).or_default().push(i);
        }
    }
    let mut bins: Vec<(String, Vec<usize>)> = by_bin.into_iter().collect();
    bins.sort_by_cached_key(|(bin, _)| location_sort_key(bin));

    // Usable copies in the window = Σ per name of min(copies, limit).
    let mut per_name: HashMap<&str, i64> = HashMap::new();
    let mut usable = 0;
    let mut best: Option<(i64, usize, usize, usize)> = None;
    let mut lo = 0;
    for hi in 0..bins.len() {
        for &i in &bins[hi].1 {
            let c = &candidates[i];
            let n = per_name.entry(c.name_key.as_str()).or_insert(0);
            for _ in 0..c.available.max(0) {
                if *n < limit {
                    usable += 1;
                }
                *n += 1;
            }
        }
        while usable >= need && lo <= hi {
            let window = (bin_distance(&bins[lo].0, &bins[hi].0), hi - lo + 1, lo, hi);
            if best.is_none_or(|b| (window.0, window.1) < (b.0, b.1)) {
                best = Some(window);
            }
            for &i in &bins[lo].1 {
                let c = &candidates[i];
                let n = per_name.entry(c.name_key.as_str()).or_insert(0);
                for _ in 0..c.available.max(0) {
                    *n -= 1;
                    if *n < limit {
                        usable -= 1;
                    }
                }
            }
            lo += 1;
        }
    }
    match best {
        Some((_, _, lo, hi)) => bins[lo..=hi].iter().map(|(b, _)| b.clone()).collect(),
        None => bins.into_iter().map(|(b, _)| b).collect(),
    }
}

/// Fisher–Yates with SplitMix64: the same seed always gives the same lot, so
/// replacing a card keeps the rest of a random lot stable.
fn shuffle(items: &mut [usize], seed: u64) {
    let mut state = seed;
    for i in (1..items.len()).rev() {
        state = splitmix(state);
        let j = (state % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

// ── Summary ─────────────────────────────────────────────────────────────────

/// Key figures of a lot, for the preview and the finish dialog.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LotSummary {
    pub copies: i64,
    /// What the copies are listed at as singles — the value given away.
    pub listed_value: f64,
    /// Copies worth at least the hit threshold.
    pub hits: i64,
    pub distinct_names: usize,
    pub oldest_days: Option<i64>,
    pub newest_days: Option<i64>,
    /// Copies per purchase lot (`""` = no lot), in lot-number order.
    pub purchase_lots: Vec<(String, i64)>,
}

pub fn summarize<'a>(
    candidates: &[Candidate],
    picks: impl IntoIterator<Item = &'a LotPick>,
    hit_min_price: f64,
) -> LotSummary {
    let mut s = LotSummary::default();
    let mut names = HashSet::new();
    let mut lots: BTreeMap<(u64, String), i64> = BTreeMap::new();
    for p in picks {
        let Some(c) = candidates.get(p.candidate) else {
            continue;
        };
        s.copies += p.copies;
        s.listed_value += c.price * p.copies as f64;
        if c.price + 1e-9 >= hit_min_price {
            s.hits += p.copies;
        }
        names.insert(c.name_key.as_str());
        if let Some(age) = c.age_days {
            s.oldest_days = Some(s.oldest_days.map_or(age, |o| o.max(age)));
            s.newest_days = Some(s.newest_days.map_or(age, |n| n.min(age)));
        }
        let number = c
            .purchase_lot
            .get(1..)
            .and_then(|n| n.parse().ok())
            .unwrap_or(u64::MAX);
        *lots.entry((number, c.purchase_lot.clone())).or_insert(0) += p.copies;
    }
    s.distinct_names = names.len();
    s.purchase_lots = lots.into_iter().map(|((_, lot), n)| (lot, n)).collect();
    s
}

/// Suggested lot name, e.g. "100 Commons" or "100 Non-Basic Lands".
pub fn default_lot_name(spec: &LotSpec) -> String {
    let single = |r: Rarity| spec.rarities.len() == 1 && spec.rarities.contains(&r);
    let rarity = Rarity::ALL[..4]
        .iter()
        .copied()
        .find(|r| single(*r))
        .map(|r| r.label());
    let mut name = match (spec.card_type, rarity) {
        (CardType::Any, Some(r)) => format!("{} {r}s", spec.size),
        (CardType::Any, None) => format!("{} Cards", spec.size),
        (t, Some(r)) => format!("{} {r} {}", spec.size, title_case(t.label())),
        (t, None) => format!("{} {}", spec.size, title_case(t.label())),
    };
    if spec.foil == FoilFilter::FoilOnly {
        name.push_str(" (Foil)");
    }
    name
}

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
        .replace("Non-basic", "Non-Basic")
        .replace("Non-land", "Non-Land")
}

// ── Picking and export ──────────────────────────────────────────────────────

/// Shelf-order sort key of a location (see [`location_sort_key`]).
pub type LocationKey = (bool, Vec<(u64, String)>);

/// Sort key that orders locations the way they sit on the shelf:
/// `A-0-0-9` before `A-0-0-10`. Rows without a location sort last.
pub fn location_sort_key(location: &str) -> LocationKey {
    let parts = location
        .trim()
        .split(['-', '_'])
        .map(|part| match part.parse::<u64>() {
            Ok(n) => (n, String::new()),
            Err(_) => (u64::MAX, part.to_lowercase()),
        })
        .collect();
    (location.trim().is_empty(), parts)
}

/// Orders picks for walking the shelves: by location, then card name.
pub fn sort_for_picking(candidates: &[Candidate], picks: &mut [LotPick]) {
    picks.sort_by(|a, b| {
        let (a, b) = (&candidates[a.candidate], &candidates[b.candidate]);
        location_sort_key(&a.location)
            .cmp(&location_sort_key(&b.location))
            .then_with(|| a.name_key.cmp(&b.name_key))
            .then_with(|| a.source.cmp(&b.source))
    });
}

/// Stock-update CSV for TCG Powertools that removes the lot's copies: the
/// same negative-quantity format as the Stock Checker's export. Foils that
/// the inventory report marks only via `finishType` get `isFoil` set, so the
/// right article is reduced.
pub fn powertools_csv(lines: &[(Card, i64)]) -> String {
    let rows: Vec<Card> = lines
        .iter()
        .map(|(card, _)| {
            let mut card = card.clone();
            if card.is_foil_card() && card.is_foil.trim().is_empty() {
                card.is_foil = "true".to_string();
            }
            card
        })
        .collect();
    let matched: Vec<MatchedCard> = rows
        .iter()
        .zip(lines)
        .map(|(card, (_, copies))| MatchedCard {
            card,
            quantity: i32::try_from(*copies).unwrap_or(i32::MAX),
            set_name: card.set.clone(),
        })
        .collect();
    format_update_stock_csv(&matched)
}

/// File name offered when saving a lot's CSV: `bulk-lot_<date>_<name>.csv`,
/// limited to letters, digits and dashes.
pub fn export_file_name(lot_name: &str, date: &str) -> String {
    let mut slug = String::new();
    for ch in lot_name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_matches('-').chars().take(60).collect();
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        format!("bulk-lot_{date}.csv")
    } else {
        format!("bulk-lot_{date}_{slug}.csv")
    }
}

#[cfg(test)]
#[path = "bulk_lot_tests.rs"]
mod tests;
