//! Stock Checker (`stock_checker.use`): match a wantslist or deck list
//! against the stock of the last import (minus pending discards), then
//! export what was picked.
//!
//! - `POST /api/stock-checker/check` `{wantslist, language, language_only}`:
//!   the list as text (pasted or read from a file in the browser; at most
//!   256 KB and 2,000 lines, never stored). Deck links are not fetched
//!   (owner's decision, 2026-11-15): the answer says how to paste the list.
//! - `POST /api/stock-checker/output` `{format, discount, groups}`: the
//!   picking list, invoice list or summary as text.
//! - `POST /api/stock-checker/update-csv` `{items}`: the stock-update CSV
//!   that takes the cards off Cardmarket (negative quantities, exact values:
//!   TCG Powertools imports it), always built from the current stock.
//!
//! The browser sends pile ids and quantities only; every value of an output
//! comes from the server's data, and a pile that is gone or has fewer copies
//! than asked for is refused.

use super::bins::{source, SourceInfo};
use super::search::{pile_row, PileRow};
use super::{blocking, Access, Api, ApiJson};
use crate::app::AppState;
use crate::http::ApiError;
use crate::permissions::Permission;
use crate::stock::{quantity, Source};
use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Json;
use check_stock_core::card_matching::{find_matching_cards, MatchedCard};
use check_stock_core::deck_fetch::parse_deck_url;
use check_stock_core::formatters::{
    format_invoice_list, format_picking_list, format_regular_output, format_update_stock_csv,
};
use check_stock_core::models::{Card, Language};
use check_stock_core::wantslist::parse_wantslist;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Longest wantslist text.
pub const MAX_WANTSLIST_BYTES: usize = 256 * 1024;
/// Most lines of a wantslist.
pub const MAX_LINES: usize = 2_000;
/// Most copies of one card asked for.
pub const MAX_WANTED: i32 = 10_000;
/// Most piles in one output.
pub const MAX_OUTPUT_ITEMS: usize = 5_000;
/// Unreadable lines listed back.
const MAX_UNPARSEABLE: usize = 50;

pub(super) fn routes(api: Api) -> Api {
    api.route(
        "/api/stock-checker/check",
        Access::Permission(Permission::StockCheckerUse),
        post(check),
    )
    .route(
        "/api/stock-checker/output",
        Access::Permission(Permission::StockCheckerUse),
        post(output),
    )
    .route(
        "/api/stock-checker/update-csv",
        Access::Permission(Permission::StockCheckerUse),
        post(update_csv),
    )
}

fn invalid(code: &'static str, message: impl Into<std::borrow::Cow<'static, str>>) -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, code, message)
}

fn no_import() -> ApiError {
    ApiError::conflict(
        "no_import",
        "The Stock Checker needs an inventory import on the server (Tools → Inventory import)",
    )
}

fn parse_language(name: &str) -> Result<Language, ApiError> {
    Language::all()
        .iter()
        .copied()
        .find(|l| l.as_str() == name)
        .ok_or_else(|| invalid("invalid_language", "Unknown language"))
}

// ── Check ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct StockCheckRequest {
    pub wantslist: String,
    /// English, German, Spanish, French or Italian.
    pub language: String,
    /// Only cards in that language.
    pub language_only: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct MatchedPile {
    pub pile: PileRow,
    /// Copies taken from this pile.
    pub take: i64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct CheckGroup {
    pub name: String,
    pub wanted: i64,
    pub found: i64,
    pub piles: Vec<MatchedPile>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StockCheckView {
    pub source: SourceInfo,
    pub wanted: i64,
    pub found: i64,
    pub groups: Vec<CheckGroup>,
    /// Lines that are no card row (the first 50).
    pub unparseable: Vec<String>,
    pub unparseable_total: i64,
}

/// Index of `card` in `cards` (it is a reference into that slice).
fn index_of(cards: &[Card], card: &Card) -> Option<usize> {
    cards.iter().position(|c| std::ptr::eq(c, card))
}

fn matched(source: &Source, m: &MatchedCard<'_>) -> Option<MatchedPile> {
    let i = index_of(&source.cards, m.card)?;
    Some(MatchedPile {
        pile: pile_row(&source.ids[i], &source.cards[i]),
        take: i64::from(m.quantity),
    })
}

async fn check(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<StockCheckRequest>,
) -> Result<Json<StockCheckView>, ApiError> {
    if request.wantslist.len() > MAX_WANTSLIST_BYTES {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "too_large",
            "The list is too long (at most 256 KB)",
        ));
    }
    if request.wantslist.lines().count() > MAX_LINES {
        return Err(invalid(
            "too_many_lines",
            "The list has more than 2,000 lines",
        ));
    }
    let language = parse_language(&request.language)?;
    if request
        .wantslist
        .lines()
        .any(|line| parse_deck_url(line).is_some())
    {
        return Err(invalid(
            "deck_link",
            "Deck links are not fetched. Open the deck on Archidekt or Moxfield, use Export → copy as text, and paste the list here",
        ));
    }
    let source = source(&state).await?.ok_or_else(no_import)?;
    let only = request.language_only;
    let view = blocking(move || {
        let parsed = parse_wantslist(&request.wantslist);
        if parsed
            .entries
            .iter()
            .any(|e| !(1..=MAX_WANTED).contains(&e.quantity))
        {
            return Err(invalid(
                "invalid_quantity",
                "Each card can be asked for 1 to 10,000 times",
            ));
        }
        let mut wanted = 0;
        let mut found = 0;
        let groups = parsed
            .entries
            .iter()
            .map(|entry| {
                let piles: Vec<MatchedPile> = find_matching_cards(
                    &entry.name,
                    entry.quantity,
                    &source.cards,
                    Some(language),
                    only,
                )
                .iter()
                .filter_map(|m| matched(&source, m))
                .collect();
                let got: i64 = piles.iter().map(|p| p.take).sum();
                wanted += i64::from(entry.quantity);
                found += got;
                CheckGroup {
                    name: entry.name.clone(),
                    wanted: i64::from(entry.quantity),
                    found: got,
                    piles,
                }
            })
            .collect();
        Ok(StockCheckView {
            source: SourceInfo::from(source.as_ref()),
            wanted,
            found,
            groups,
            unparseable_total: parsed.unparseable.len() as i64,
            unparseable: parsed
                .unparseable
                .into_iter()
                .take(MAX_UNPARSEABLE)
                .map(|l| l.chars().take(200).collect())
                .collect(),
        })
    })
    .await??;
    Ok(Json(view))
}

// ── Outputs ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PileQuantity {
    pub pile: String,
    pub quantity: i64,
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct OutputGroup {
    pub name: String,
    pub wanted: i64,
    pub items: Vec<PileQuantity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum OutputFormat {
    /// Sorted by location, for walking the shelves.
    Picking,
    /// Names, conditions and prices for the customer.
    Invoice,
    /// Every wanted card with what was found and the total (with discount).
    Summary,
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct OutputRequest {
    pub format: OutputFormat,
    /// Percent off the total (summary), 0–100.
    pub discount: f64,
    pub groups: Vec<OutputGroup>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct OutputText {
    pub text: String,
}

/// The piles of `items` from the current stock, with their quantities;
/// refused when a pile is gone or has fewer copies than asked for.
pub(super) fn resolve<'a>(
    source: &'a Source,
    items: &[PileQuantity],
) -> Result<Vec<(&'a Card, i64)>, ApiError> {
    items
        .iter()
        .map(|item| {
            let card = source.pile(&item.pile).ok_or_else(|| {
                ApiError::conflict(
                    "stock_changed",
                    "A card is no longer in stock (a discard or a new import); check again",
                )
            })?;
            let available = if card.is_playset_card() {
                quantity(card) * 4
            } else {
                quantity(card)
            };
            if !(1..=available).contains(&item.quantity) {
                return Err(ApiError::conflict(
                    "not_enough",
                    "A card has fewer copies in stock than asked for; check again",
                ));
            }
            Ok((card, item.quantity))
        })
        .collect()
}

fn check_size(count: usize) -> Result<(), ApiError> {
    if count == 0 || count > MAX_OUTPUT_ITEMS {
        Err(invalid("invalid_items", "Choose 1 to 5,000 piles"))
    } else {
        Ok(())
    }
}

fn matched_cards<'a>(cards: &[(&'a Card, i64)]) -> Vec<MatchedCard<'a>> {
    cards
        .iter()
        .map(|(card, q)| MatchedCard {
            card,
            quantity: i32::try_from(*q).unwrap_or(i32::MAX),
            set_name: format!("{} ({})", card.set, card.set_code),
        })
        .collect()
}

async fn output(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<OutputRequest>,
) -> Result<Json<OutputText>, ApiError> {
    check_size(request.groups.iter().map(|g| g.items.len()).sum())?;
    if request.groups.len() > MAX_LINES || request.groups.iter().any(|g| g.name.len() > 200) {
        return Err(invalid("invalid_items", "Too many or too long card names"));
    }
    if !(0.0..=100.0).contains(&request.discount) {
        return Err(invalid(
            "invalid_discount",
            "The discount must be 0 to 100 %",
        ));
    }
    let source = source(&state).await?.ok_or_else(no_import)?;
    let text = blocking(move || {
        let groups = request
            .groups
            .iter()
            .map(|g| {
                Ok((
                    g.name.clone(),
                    i32::try_from(g.wanted.clamp(0, i64::from(MAX_WANTED))).unwrap_or(0),
                    resolve(&source, &g.items)?,
                ))
            })
            .collect::<Result<Vec<_>, ApiError>>()?;
        let all: Vec<(&Card, i64)> = groups.iter().flat_map(|g| g.2.iter().copied()).collect();
        Ok::<_, ApiError>(match request.format {
            OutputFormat::Picking => format_picking_list(&matched_cards(&all)),
            OutputFormat::Invoice => format_invoice_list(&matched_cards(&all)),
            OutputFormat::Summary => {
                let matches: Vec<(String, i32, Vec<MatchedCard>)> = groups
                    .iter()
                    .map(|(name, wanted, cards)| (name.clone(), *wanted, matched_cards(cards)))
                    .collect();
                format_regular_output(&matches, request.discount as f32)
            }
        })
    })
    .await??;
    Ok(Json(OutputText { text }))
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct UpdateCsvRequest {
    pub items: Vec<PileQuantity>,
}

async fn update_csv(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<UpdateCsvRequest>,
) -> Result<Response, ApiError> {
    check_size(request.items.len())?;
    let source = source(&state).await?.ok_or_else(no_import)?;
    let csv = blocking(move || {
        let cards = resolve(&source, &request.items)?;
        Ok::<_, ApiError>(format_update_stock_csv(&matched_cards(&cards)))
    })
    .await??;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/csv; charset=utf-8"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=\"stock_update.csv\""),
            ),
        ],
        csv,
    )
        .into_response())
}

#[cfg(test)]
#[path = "stock_checker_tests.rs"]
mod tests;
