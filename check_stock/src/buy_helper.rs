//! Card Buy Helper — pure valuation logic for making a purchase offer from a
//! card export CSV.
//!
//! This module is strictly **read-only** with respect to any inventory
//! database: it only classifies cards and sums their values in memory to help
//! the user decide on an offer. Nothing here touches [`crate::inventory_db`].
//!
//! Cards are split into two buckets:
//! * **Singles** — valued individually and bought at a percentage of market value.
//! * **Bulk** — everything else, bought at a flat bulk rate (euros per N cards).

use crate::models::Card;

/// User-adjustable parameters controlling how cards are split into individually
/// valued "singles" versus "bulk", and how each bucket is priced into an offer.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct BuyParams {
    /// Rarities that qualify a card as a single (matched case-insensitively).
    pub single_common: bool,
    pub single_uncommon: bool,
    pub single_rare: bool,
    pub single_mythic: bool,
    /// Cards whose unit value is `>=` this threshold are singles regardless of
    /// rarity. `None` disables the price rule.
    pub min_single_price: Option<f64>,
    /// Percentage (`0..=100`) of a single card's market value offered.
    pub single_buy_percent: f64,
    /// Bulk price: `bulk_rate` euros per `bulk_batch` cards.
    pub bulk_rate: f64,
    pub bulk_batch: u32,
}

impl Default for BuyParams {
    fn default() -> Self {
        Self {
            single_common: false,
            single_uncommon: false,
            single_rare: true,
            single_mythic: true,
            min_single_price: Some(0.50),
            single_buy_percent: 60.0,
            bulk_rate: 10.0,
            bulk_batch: 1000,
        }
    }
}

/// Which bucket a card falls into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardClass {
    Single,
    Bulk,
}

impl CardClass {
    pub fn as_str(self) -> &'static str {
        match self {
            CardClass::Single => "single",
            CardClass::Bulk => "bulk",
        }
    }
}

/// Parse a card's quantity, defaulting to 1 when unparseable so no card
/// silently drops out of the totals. Negative values are clamped to 0.
pub fn card_qty(card: &Card) -> i64 {
    card.quantity.trim().parse::<i64>().unwrap_or(1).max(0)
}

/// Classify a single card given the rarity toggles and optional price threshold.
///
/// A card is a single if its unit value meets the price threshold **or** its
/// rarity is one of the enabled rarities. Everything else is bulk.
pub fn classify(card: &Card, params: &BuyParams) -> CardClass {
    if let Some(min) = params.min_single_price {
        if card.price_f64() >= min {
            return CardClass::Single;
        }
    }
    let qualifies = match card.rarity.trim().to_lowercase().as_str() {
        "common" => params.single_common,
        "uncommon" => params.single_uncommon,
        "rare" => params.single_rare,
        "mythic" | "mythic rare" => params.single_mythic,
        _ => false,
    };
    if qualifies {
        CardClass::Single
    } else {
        CardClass::Bulk
    }
}

/// Aggregate offer figures for a set of cards.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuySummary {
    /// Number of CSV rows classified as singles.
    pub single_rows: usize,
    /// Total quantity of single cards (sum of per-row quantities).
    pub single_cards: i64,
    /// Sum of `unit_price * quantity` over all singles.
    pub single_market_value: f64,
    /// Amount offered for the singles (`single_market_value * percent / 100`).
    pub single_offer: f64,
    pub bulk_rows: usize,
    pub bulk_cards: i64,
    /// Sum of `unit_price * quantity` over all bulk cards (informational).
    pub bulk_market_value: f64,
    /// Amount offered for the bulk (`bulk_cards / bulk_batch * bulk_rate`).
    pub bulk_offer: f64,
    /// `single_offer + bulk_offer`.
    pub total_offer: f64,
}

/// Compute the full offer summary for `cards` under `params`.
pub fn compute_summary(cards: &[Card], params: &BuyParams) -> BuySummary {
    let mut s = BuySummary::default();
    for card in cards {
        let qty = card_qty(card);
        let value = card.price_f64() * qty as f64;
        match classify(card, params) {
            CardClass::Single => {
                s.single_rows += 1;
                s.single_cards += qty;
                s.single_market_value += value;
            }
            CardClass::Bulk => {
                s.bulk_rows += 1;
                s.bulk_cards += qty;
                s.bulk_market_value += value;
            }
        }
    }
    s.single_offer = s.single_market_value * params.single_buy_percent / 100.0;
    s.bulk_offer = if params.bulk_batch > 0 {
        s.bulk_cards as f64 / params.bulk_batch as f64 * params.bulk_rate
    } else {
        0.0
    };
    s.total_offer = s.single_offer + s.bulk_offer;
    s
}

/// Build an exportable CSV describing the offer.
///
/// Produces one row per card (with a `class` column and a per-row `offer` so the
/// `offer` column sums to `total_offer`), followed by labelled summary rows.
/// The bulk offer is distributed across bulk rows in proportion to quantity.
pub fn export_csv(cards: &[Card], params: &BuyParams) -> Result<String, String> {
    let summary = compute_summary(cards, params);
    let per_bulk_card_offer = if summary.bulk_cards > 0 {
        summary.bulk_offer / summary.bulk_cards as f64
    } else {
        0.0
    };

    let mut wtr = csv::Writer::from_writer(Vec::new());
    wtr.write_record([
        "name",
        "set",
        "setCode",
        "cn",
        "condition",
        "language",
        "isFoil",
        "rarity",
        "quantity",
        "unitPrice",
        "marketValue",
        "class",
        "offer",
    ])
    .map_err(|e| e.to_string())?;

    for card in cards {
        let qty = card_qty(card);
        let unit = card.price_f64();
        let value = unit * qty as f64;
        let (class, offer) = match classify(card, params) {
            CardClass::Single => (
                CardClass::Single.as_str(),
                value * params.single_buy_percent / 100.0,
            ),
            CardClass::Bulk => (CardClass::Bulk.as_str(), per_bulk_card_offer * qty as f64),
        };
        wtr.write_record([
            card.name.as_str(),
            card.set.as_str(),
            card.set_code.as_str(),
            card.cn.as_str(),
            card.condition.as_str(),
            card.language.as_str(),
            if card.is_foil_card() { "1" } else { "0" },
            card.rarity.as_str(),
            &qty.to_string(),
            &format!("{unit:.2}"),
            &format!("{value:.2}"),
            class,
            &format!("{offer:.2}"),
        ])
        .map_err(|e| e.to_string())?;
    }

    // Blank spacer, then labelled summary rows. These are NOT card rows — the
    // leading marker in the `name` column keeps them from being confused with
    // per-card data if the file is loaded into a spreadsheet.
    let empty = || [""; 13];
    wtr.write_record(empty()).map_err(|e| e.to_string())?;
    write_summary_row(
        &mut wtr,
        "=== SINGLES ===",
        summary.single_cards,
        summary.single_market_value,
        summary.single_offer,
    )?;
    write_summary_row(
        &mut wtr,
        "=== BULK ===",
        summary.bulk_cards,
        summary.bulk_market_value,
        summary.bulk_offer,
    )?;
    write_summary_row(
        &mut wtr,
        "=== TOTAL ===",
        summary.single_cards + summary.bulk_cards,
        summary.single_market_value + summary.bulk_market_value,
        summary.total_offer,
    )?;

    let bytes = wtr.into_inner().map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

fn write_summary_row(
    wtr: &mut csv::Writer<Vec<u8>>,
    label: &str,
    cards: i64,
    market_value: f64,
    offer: f64,
) -> Result<(), String> {
    let cards = cards.to_string();
    let market_value = format!("{market_value:.2}");
    let offer = format!("{offer:.2}");
    // Columns: name, ..., quantity(8), marketValue(10), ..., offer(12)
    let record = [
        label,
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        cards.as_str(),
        "",
        market_value.as_str(),
        "",
        offer.as_str(),
    ];
    wtr.write_record(record).map_err(|e| e.to_string())
}

#[path = "buy_helper_tests.rs"]
#[cfg(test)]
mod tests;
