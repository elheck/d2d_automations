//! Precon EV (permission `precon_ev.use`): Commander precons ranked by the
//! value of their singles against the sealed price.
//!
//! - `GET /api/precon-ev/status`: what is loaded, and the load / sales jobs
//! - `POST /api/precon-ev/load` `{refresh_index}`: starts a load for everyone
//! - `GET /api/precon-ev/ranking?…`: one row per precon for the parameters
//! - `GET /api/precon-ev/products/{id}?…`: KPIs and the card table
//! - `GET /api/precon-ev/products/{id}/history?…`: weekly EV vs. sealed
//! - `GET` / `PUT /api/precon-ev/settings`: the user's own parameters
//!
//! The data is loaded once on the server ([`crate::precon`]); the parameters
//! (price columns, cutoff, foil check, sorting) only change the calculation.

use super::{blocking, in_transaction, Access, Api, ApiJson, ApiQuery, CurrentUser};
use crate::app::AppState;
use crate::audit::{self, action};
use crate::http::ApiError;
use crate::jobs::JobStatus;
use crate::permissions::Permission;
use crate::precon::{HistoryError, StartError};
use crate::user_settings::{self, Setting, SettingValue};
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use check_stock_core::api::cardmarket::product_url;
use check_stock_core::api::inventory_sync::PriceField;
use check_stock_core::api::mtgjson::DeckBoard;
use check_stock_core::precon_ev::{
    build_rows, card_note, ev_history, ranked_by_unit, sort_rows, summarize, top_n_lines,
    trim_leading_gaps, value_cards, visible_rows, DeckEv, EvParams, PreconRow, PreconSort,
    PreconStatus, DEFAULT_CUTOFF_EUR, SEALED_FIELDS, TOP_N,
};
use check_stock_core::precon_loader::local_date;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Highest bulk cutoff (as on the desktop).
pub const MAX_CUTOFF: f64 = 100.0;
/// Longest text filter.
pub const MAX_FILTER_LEN: usize = 100;

pub(super) fn routes(api: Api) -> Api {
    let access = Access::Permission(Permission::PreconEvUse);
    api.route("/api/precon-ev/status", access, get(status))
        .route("/api/precon-ev/load", access, post(load))
        .route("/api/precon-ev/ranking", access, get(ranking))
        .route("/api/precon-ev/products/{id}", access, get(product))
        .route("/api/precon-ev/products/{id}/history", access, get(history))
        .route(
            "/api/precon-ev/settings",
            access,
            get(get_settings).put(put_settings),
        )
}

// ── Parameters ─────────────────────────────────────────────────────────────

/// A Cardmarket price column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PriceColumn {
    Trend,
    Avg,
    Low,
    Avg1,
    Avg7,
    Avg30,
}

impl From<PriceColumn> for PriceField {
    fn from(c: PriceColumn) -> Self {
        match c {
            PriceColumn::Trend => PriceField::Trend,
            PriceColumn::Avg => PriceField::Avg,
            PriceColumn::Low => PriceField::Low,
            PriceColumn::Avg1 => PriceField::Avg1,
            PriceColumn::Avg7 => PriceField::Avg7,
            PriceColumn::Avg30 => PriceField::Avg30,
        }
    }
}

/// Ranking columns that can be sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RankingSort {
    Ratio,
    Ev,
    Liquid,
    Top5,
    Sealed,
    Coverage,
    Release,
    Name,
}

impl From<RankingSort> for PreconSort {
    fn from(s: RankingSort) -> Self {
        match s {
            RankingSort::Ratio => PreconSort::Ratio,
            RankingSort::Ev => PreconSort::Ev,
            RankingSort::Liquid => PreconSort::Liquid,
            RankingSort::Top5 => PreconSort::Top5,
            RankingSort::Sealed => PreconSort::Sealed,
            RankingSort::Coverage => PreconSort::Coverage,
            RankingSort::Release => PreconSort::Release,
            RankingSort::Name => PreconSort::Name,
        }
    }
}

fn invalid_params(message: &'static str) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "invalid_params", message)
}

/// Core's parameters, if the values are allowed.
fn ev_params(
    field: PriceColumn,
    sealed_field: PriceColumn,
    cutoff: f64,
    foil_needs_sales: bool,
) -> Result<EvParams, ApiError> {
    let sealed: PriceField = sealed_field.into();
    if !SEALED_FIELDS.contains(&sealed) {
        return Err(invalid_params(
            "Sealed products only have the Trend, Low and Average prices",
        ));
    }
    if !cutoff.is_finite() || !(0.0..=MAX_CUTOFF).contains(&cutoff) {
        return Err(invalid_params(
            "The bulk cutoff must be between 0 and 100 €",
        ));
    }
    Ok(EvParams {
        field: field.into(),
        sealed_field: sealed,
        cutoff,
        foil_needs_sales,
    })
}

/// The pricing parameters of a request.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvQuery {
    field: PriceColumn,
    sealed_field: PriceColumn,
    cutoff: f64,
    foil_needs_sales: bool,
}

impl EvQuery {
    fn params(&self) -> Result<EvParams, ApiError> {
        ev_params(
            self.field,
            self.sealed_field,
            self.cutoff,
            self.foil_needs_sales,
        )
    }
}

/// Pricing parameters plus the table's sorting and filter.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RankingQuery {
    field: PriceColumn,
    sealed_field: PriceColumn,
    cutoff: f64,
    foil_needs_sales: bool,
    sort: RankingSort,
    desc: bool,
    #[serde(default)]
    filter: String,
    #[serde(default)]
    hide_no_decklist: bool,
}

// ── Answers ────────────────────────────────────────────────────────────────

/// What is loaded and what the background jobs are doing.
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PreconStatusView {
    /// A price tracker is configured (without one nothing can be loaded).
    pub configured: bool,
    pub load: JobStatus,
    pub sales: JobStatus,
    pub loaded: Option<LoadedInfo>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LoadedInfo {
    /// Changes with every load and when the sales history arrives.
    pub version: u64,
    pub loaded_at: String,
    pub products: usize,
    pub with_decklist: usize,
    pub deck_list_version: Option<String>,
    /// Date (`YYYY-MM-DD`) of MTGJSON's sealed index in use.
    pub index_date: Option<String>,
    /// MTGJSON was unreachable; an older index is in use.
    pub index_stale: bool,
    /// The tracker search hit its limit; precons may be missing.
    pub catalog_truncated: bool,
    /// The sales history is in (foil check and Liquid EV apply).
    pub sales_loaded: bool,
    pub sales_error: Option<String>,
}

/// EV figures of a product (or bundle).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct EvSummary {
    pub ev: f64,
    pub gross: f64,
    pub top5: f64,
    pub liquid: Option<f64>,
    /// Share of copies with a price, 0–1.
    pub coverage: f64,
    pub total_copies: u32,
    pub priced_copies: u32,
    pub counted_copies: u32,
    pub below_cutoff_copies: u32,
    pub unpriced_copies: u32,
    pub foil_fallback_copies: u32,
}

impl From<&DeckEv> for EvSummary {
    fn from(e: &DeckEv) -> Self {
        Self {
            ev: e.ev,
            gross: e.gross,
            top5: e.top5,
            liquid: e.liquid,
            coverage: e.coverage(),
            total_copies: e.total_copies,
            priced_copies: e.priced_copies,
            counted_copies: e.counted_copies,
            below_cutoff_copies: e.below_cutoff_copies,
            unpriced_copies: e.unpriced_copies,
            foil_fallback_copies: e.foil_fallback_copies,
        }
    }
}

/// Why a product has (or lacks) an EV.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ProductState {
    Ready,
    NotOnMtgjson,
    DecklistPending,
    DeckUnavailable,
}

/// One precon of the ranking.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct RankingRow {
    pub id_product: u64,
    pub name: String,
    pub set_label: String,
    pub release_date: Option<String>,
    pub state: ProductState,
    /// Decks in the product (bundles have several); 0 when not ready.
    pub decks: usize,
    pub sealed_price: Option<f64>,
    /// Cheapest current offer (Cardmarket `low`).
    pub sealed_low: Option<f64>,
    /// The cheapest offer, when the sealed price used is stale below it.
    pub stale_sealed: Option<f64>,
    pub ev: Option<EvSummary>,
    /// EV ÷ sealed price.
    pub ratio: Option<f64>,
    /// Short note for the table, and an optional longer explanation.
    pub note: String,
    pub note_detail: Option<String>,
    pub cardmarket_url: String,
}

impl From<&PreconRow> for RankingRow {
    fn from(row: &PreconRow) -> Self {
        let (state, decks) = match &row.status {
            PreconStatus::Ready { decks } => (ProductState::Ready, *decks),
            PreconStatus::NotOnMtgjson => (ProductState::NotOnMtgjson, 0),
            PreconStatus::DecklistPending(_) => (ProductState::DecklistPending, 0),
            PreconStatus::DeckUnavailable(_) => (ProductState::DeckUnavailable, 0),
        };
        let (note, detail) = row.notes();
        // A failed download's text can name addresses: not for the page.
        let note_detail = match state {
            ProductState::DeckUnavailable => None,
            _ => detail,
        };
        Self {
            id_product: row.id_product,
            name: row.name.clone(),
            set_label: row.set_label.clone(),
            release_date: row.release_date.clone(),
            state,
            decks,
            sealed_price: row.sealed_price,
            sealed_low: row.sealed_low,
            stale_sealed: row.stale_sealed(),
            ev: row.ev.as_ref().map(EvSummary::from),
            ratio: row.ratio,
            note,
            note_detail,
            cardmarket_url: product_url(row.id_product),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RankingView {
    pub version: u64,
    pub rows: Vec<RankingRow>,
    /// Precons in the catalog / with a decklist (before the filter).
    pub total: usize,
    pub ready: usize,
}

/// Where a card sits in the decklist.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Board {
    Commander,
    Main,
    Side,
    Display,
}

impl From<DeckBoard> for Board {
    fn from(b: DeckBoard) -> Self {
        match b {
            DeckBoard::Commander => Board::Commander,
            DeckBoard::Main => Board::Main,
            DeckBoard::Side => Board::Side,
            DeckBoard::Display => Board::Display,
        }
    }
}

/// One decklist line with its value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct CardLine {
    /// Position by unit price within its deck (1 = most expensive).
    pub rank: usize,
    /// Contributes to the Top 5.
    pub top5: bool,
    pub name: String,
    pub is_foil: bool,
    pub set_code: String,
    pub number: String,
    pub board: Board,
    pub count: u32,
    pub unit: Option<f64>,
    /// A non-foil price stands in for the foil.
    pub foil_fallback: bool,
    pub line: Option<f64>,
    /// Priced at or above the cutoff (counts toward EV).
    pub counted: bool,
    /// Days with a sale / observed days (of the last 14); `observed_days`
    /// 0 = not checked (under 0.50 €).
    pub sale_days: Option<u8>,
    pub observed_days: Option<u8>,
    /// Weight in Liquid EV, 0–1.
    pub liquidity: Option<f64>,
    pub note: String,
    pub cardmarket_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DeckDetail {
    pub name: String,
    pub ev: f64,
    pub cards: Vec<CardLine>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProductDetail {
    pub version: u64,
    pub row: RankingRow,
    /// Empty when the product has no decklist yet.
    pub decks: Vec<DeckDetail>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct HistoryPoint {
    pub date: String,
    /// `null` when fewer than 90% of the copies had a price that week.
    pub ev: Option<f64>,
    pub top5: Option<f64>,
    pub sealed: Option<f64>,
    pub coverage: f64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EvHistory {
    pub version: u64,
    pub points: Vec<HistoryPoint>,
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct LoadRequest {
    /// Re-download MTGJSON's sealed product index (~12 MB).
    pub refresh_index: bool,
}

// ── Handlers ───────────────────────────────────────────────────────────────

fn not_loaded() -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "not_loaded",
        "The precons are not loaded yet",
    )
}

fn not_configured() -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        "not_configured",
        "No price tracker is configured on the server",
    )
}

fn status_view(state: &AppState) -> PreconStatusView {
    let precon = &state.precon;
    let loaded = precon.loaded().map(|l| {
        let data = &l.data;
        let with_decklist = data
            .catalog
            .iter()
            .filter(|p| data.decks_for(p.id_product).is_some())
            .count();
        LoadedInfo {
            version: l.version,
            loaded_at: crate::migrations::format_utc(l.loaded_at),
            products: data.catalog.len(),
            with_decklist,
            deck_list_version: data.deck_list_version.clone(),
            index_date: chrono::DateTime::from_timestamp(data.index_fetched_at_unix, 0)
                .map(|d| d.format("%Y-%m-%d").to_string()),
            index_stale: data.index_stale,
            catalog_truncated: data.catalog_truncated,
            sales_loaded: data.sales.is_some(),
            sales_error: data.sales_error.clone(),
        }
    });
    PreconStatusView {
        configured: precon.is_configured(),
        load: precon.load_status(),
        sales: precon.sales_status(),
        loaded,
    }
}

async fn status(State(state): State<AppState>) -> Json<PreconStatusView> {
    Json(status_view(&state))
}

/// `POST /api/precon-ev/load`: starts a load unless one runs (then it just
/// answers with the status).
async fn load(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<LoadRequest>,
) -> Result<Response, ApiError> {
    match state.precon.start_load(request.refresh_index) {
        Ok(()) => {
            log::info!(
                "Precon load started by {} (refresh index: {})",
                user.username,
                request.refresh_index
            );
        }
        Err(StartError::Running) => {}
        Err(StartError::NotConfigured) => return Err(not_configured()),
        Err(StartError::TooSoon { wait }) => {
            let seconds = wait.as_secs().max(1);
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "too_soon",
                format!("The precons were just loaded; try again in {seconds} s"),
            )
            .with_header(
                header::RETRY_AFTER,
                HeaderValue::from_str(&seconds.to_string()).map_err(|_| ApiError::internal())?,
            ));
        }
    }
    Ok((StatusCode::ACCEPTED, Json(status_view(&state))).into_response())
}

async fn ranking(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<RankingQuery>,
) -> Result<Json<RankingView>, ApiError> {
    let params = ev_params(
        query.field,
        query.sealed_field,
        query.cutoff,
        query.foil_needs_sales,
    )?;
    if query.filter.chars().count() > MAX_FILTER_LEN {
        return Err(invalid_params("The filter is too long"));
    }
    let loaded = state.precon.loaded().ok_or_else(not_loaded)?;
    let view = blocking(move || {
        let mut rows = build_rows(&loaded.data, params);
        sort_rows(&mut rows, query.sort.into(), query.desc);
        let ready = rows.iter().filter(|r| r.is_ready()).count();
        let total = rows.len();
        let rows = visible_rows(&rows, &query.filter, query.hide_no_decklist)
            .into_iter()
            .map(RankingRow::from)
            .collect();
        RankingView {
            version: loaded.version,
            rows,
            total,
            ready,
        }
    })
    .await?;
    Ok(Json(view))
}

fn unknown_product() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "unknown_product",
        "No precon with this id is loaded",
    )
}

async fn product(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    ApiQuery(query): ApiQuery<EvQuery>,
) -> Result<Json<ProductDetail>, ApiError> {
    let params = query.params()?;
    let loaded = state.precon.loaded().ok_or_else(not_loaded)?;
    let detail = blocking(move || {
        let data = &loaded.data;
        let row = build_rows(data, params)
            .into_iter()
            .find(|r| r.id_product == id)?;
        let mut decks_out = Vec::new();
        if let Some(decks) = data.decks_for(id) {
            let values = value_cards(&decks, |i| data.prices.get(&i), data.sales.as_ref(), params);
            let top = top_n_lines(&values, TOP_N);
            let order = ranked_by_unit(&values);
            for (deck_idx, deck) in decks.iter().enumerate() {
                let deck_order: Vec<usize> = order
                    .iter()
                    .copied()
                    .filter(|&i| values[i].deck_idx == deck_idx)
                    .collect();
                let deck_values: Vec<_> = deck_order.iter().map(|&i| values[i].clone()).collect();
                let cards = deck_order
                    .iter()
                    .enumerate()
                    .map(|(rank, &i)| {
                        let v = &values[i];
                        CardLine {
                            rank: rank + 1,
                            top5: top.contains(&i),
                            name: v.card.name.clone(),
                            is_foil: v.card.is_foil,
                            set_code: v.card.set_code.clone(),
                            number: v.card.number.clone(),
                            board: v.card.board.into(),
                            count: v.card.count,
                            unit: v.unit.map(|u| u.eur),
                            foil_fallback: v.unit.is_some_and(|u| u.foil_fallback),
                            line: v.line_total(),
                            counted: v.unit.is_some_and(|u| u.eur >= params.cutoff),
                            sale_days: v.sale_days,
                            observed_days: v.observed_days,
                            liquidity: v.liquidity,
                            note: card_note(v, &data.prices, params.field, params.cutoff)
                                .to_string(),
                            cardmarket_url: v.card.mcm_id.map(product_url),
                        }
                    })
                    .collect();
                decks_out.push(DeckDetail {
                    name: deck.name.clone(),
                    ev: summarize(&deck_values, params.cutoff).ev,
                    cards,
                });
            }
        }
        Some(ProductDetail {
            version: loaded.version,
            row: RankingRow::from(&row),
            decks: decks_out,
        })
    })
    .await?
    .ok_or_else(unknown_product)?;
    Ok(Json(detail))
}

async fn history(
    State(state): State<AppState>,
    Path(id): Path<u64>,
    ApiQuery(query): ApiQuery<EvQuery>,
) -> Result<Json<EvHistory>, ApiError> {
    let params = query.params()?;
    let today = local_date(chrono::Utc::now().timestamp());
    let (loaded, history) = state.precon.history(id, today).await.map_err(|e| match e {
        HistoryError::NotConfigured => not_configured(),
        HistoryError::NotLoaded => not_loaded(),
        HistoryError::NoDecklist => ApiError::new(
            StatusCode::NOT_FOUND,
            "no_decklist",
            "This precon has no decklist, so it has no EV history",
        ),
        HistoryError::Unavailable(detail) => {
            log::warn!("Precon history for {id} failed: {detail}");
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                "tracker_unavailable",
                "The price tracker did not answer; try again in a moment",
            )
        }
    })?;
    let version = loaded.version;
    let points = blocking(move || {
        let data = &loaded.data;
        let Some(decks) = data.decks_for(id) else {
            return Vec::new();
        };
        trim_leading_gaps(ev_history(
            &decks,
            id,
            &history.snapshots,
            &history.dates,
            data.sales.as_ref(),
            params,
        ))
        .into_iter()
        .map(|p| HistoryPoint {
            date: p.date,
            ev: p.ev,
            top5: p.top5,
            sealed: p.sealed,
            coverage: p.coverage,
        })
        .collect()
    })
    .await?;
    Ok(Json(EvHistory { version, points }))
}

// ── Settings ───────────────────────────────────────────────────────────────

/// The user's precon EV parameters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PreconSettings {
    pub field: PriceColumn,
    pub sealed_field: PriceColumn,
    pub cutoff: f64,
    pub foil_needs_sales: bool,
    pub sort: RankingSort,
    pub desc: bool,
    pub hide_no_decklist: bool,
}

impl PreconSettings {
    fn validate(&self) -> Result<(), ApiError> {
        ev_params(
            self.field,
            self.sealed_field,
            self.cutoff,
            self.foil_needs_sales,
        )
        .map(|_| ())
    }
}

/// The desktop app's defaults.
fn default_settings() -> PreconSettings {
    PreconSettings {
        field: PriceColumn::Trend,
        sealed_field: PriceColumn::Trend,
        cutoff: DEFAULT_CUTOFF_EUR,
        foil_needs_sales: true,
        sort: RankingSort::Ratio,
        desc: true,
        hide_no_decklist: false,
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct PreconSettingsView {
    pub settings: PreconSettings,
    pub defaults: PreconSettings,
}

async fn get_settings(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<PreconSettingsView>, ApiError> {
    let user_id = user.id;
    let stored = state
        .db
        .read(move |conn| user_settings::get(conn, user_id, Setting::PreconEv))
        .await?;
    let settings = stored
        .and_then(|v| serde_json::from_value::<PreconSettings>(v).ok())
        .filter(|s| s.validate().is_ok())
        .unwrap_or_else(default_settings);
    Ok(Json(PreconSettingsView {
        settings,
        defaults: default_settings(),
    }))
}

async fn put_settings(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(settings): ApiJson<PreconSettings>,
) -> Result<Json<PreconSettingsView>, ApiError> {
    settings.validate()?;
    let value = serde_json::to_value(&settings)
        .ok()
        .and_then(|v| SettingValue::new(v).ok())
        .ok_or_else(|| invalid_params("The settings cannot be stored"))?;
    let now = chrono::Utc::now();
    state
        .db
        .write(move |conn| {
            in_transaction(conn, |tx| {
                if user_settings::put(tx, user.id, Setting::PreconEv, &value, now)? {
                    audit::record(
                        tx,
                        now,
                        user.actor(),
                        action::SETTINGS_CHANGED,
                        Some(Setting::PreconEv.key()),
                        value.value().clone(),
                    )?;
                }
                Ok(())
            })
        })
        .await??;
    Ok(Json(PreconSettingsView {
        settings,
        defaults: default_settings(),
    }))
}

#[cfg(test)]
#[path = "precon_ev_tests.rs"]
mod tests;
