//! Mispricing report (permission `mispricing.view`): every listing in stock
//! against the market, with an action ("raise now", "cut", …) and the
//! internal consistency check (listings that contradict each other). Built
//! on the shared market data ([`crate::market`]), like price movers.
//!
//! - `GET /api/mispricing/status`, `POST /api/mispricing/load` `{refresh}`:
//!   the shared market data (same as `/api/movers/…`, other permission)
//! - `GET /api/mispricing?field=&threshold=&filter=&sort=&desc=`
//!
//! The money totals of the summary (possible extra revenue, capital above
//! market, stock value) also need `financials.view`.

use super::movers::{market_status, start_market_load, MarketLoadRequest, MarketStatusView};
use super::precon_ev::PriceColumn;
use super::{blocking, Access, Api, ApiJson, ApiQuery, CurrentUser};
use crate::app::AppState;
use crate::http::ApiError;
use crate::permissions::Permission;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Json;
use check_stock_core::api::cardmarket::product_url;
use check_stock_core::consistency::{find_issues, IssueKind};
use check_stock_core::mispricing::{
    impact, report_from_tracker, sort_mispriced, with_changes, Action, MispricedCard,
    MispricingSort, Momentum, PriceVerdict, VerdictFilter,
};
use check_stock_core::price_trends::TrendChange;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Rows sent for the table, as on the desktop.
pub const MISPRICING_MAX_ROWS: usize = 300;
/// Consistency conflicts sent, as on the desktop.
pub const MAX_ISSUES: usize = 100;

pub(super) fn routes(api: Api) -> Api {
    let access = Access::Permission(Permission::MispricingView);
    api.route("/api/mispricing/status", access, get(status))
        .route("/api/mispricing/load", access, post(load))
        .route("/api/mispricing", access, get(report))
}

async fn status(State(state): State<AppState>) -> Json<MarketStatusView> {
    Json(market_status(&state))
}

async fn load(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<MarketLoadRequest>,
) -> Result<Response, ApiError> {
    start_market_load(&state, &user, request.refresh).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum VerdictFilterKey {
    All,
    Underpriced,
    Overpriced,
    NoData,
}

impl From<VerdictFilterKey> for VerdictFilter {
    fn from(f: VerdictFilterKey) -> Self {
        match f {
            VerdictFilterKey::All => VerdictFilter::All,
            VerdictFilterKey::Underpriced => VerdictFilter::Underpriced,
            VerdictFilterKey::Overpriced => VerdictFilter::Overpriced,
            VerdictFilterKey::NoData => VerdictFilter::NoData,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MispricingSortKey {
    Priority,
    Impact,
    DeltaPct,
    Listed,
    Market,
    Change7,
    Change30,
    Age,
    Name,
    Set,
    Quantity,
    Verdict,
}

impl From<MispricingSortKey> for MispricingSort {
    fn from(s: MispricingSortKey) -> Self {
        match s {
            MispricingSortKey::Priority => MispricingSort::Priority,
            MispricingSortKey::Impact => MispricingSort::Impact,
            MispricingSortKey::DeltaPct => MispricingSort::DeltaPct,
            MispricingSortKey::Listed => MispricingSort::Listed,
            MispricingSortKey::Market => MispricingSort::Market,
            MispricingSortKey::Change7 => MispricingSort::Change7,
            MispricingSortKey::Change30 => MispricingSort::Change30,
            MispricingSortKey::Age => MispricingSort::Age,
            MispricingSortKey::Name => MispricingSort::Name,
            MispricingSortKey::Set => MispricingSort::Set,
            MispricingSortKey::Quantity => MispricingSort::Quantity,
            MispricingSortKey::Verdict => MispricingSort::Verdict,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MispricingQuery {
    field: PriceColumn,
    /// Half-width of the fair band in percent.
    threshold: f64,
    filter: VerdictFilterKey,
    sort: MispricingSortKey,
    desc: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Verdict {
    Underpriced,
    Overpriced,
    Fair,
    NoMarketData,
}

impl From<PriceVerdict> for Verdict {
    fn from(v: PriceVerdict) -> Self {
        match v {
            PriceVerdict::Underpriced => Verdict::Underpriced,
            PriceVerdict::Overpriced => Verdict::Overpriced,
            PriceVerdict::Fair => Verdict::Fair,
            PriceVerdict::NoMarketData => Verdict::NoMarketData,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MarketMomentum {
    Rising,
    Falling,
    Flat,
    Unknown,
}

impl From<Momentum> for MarketMomentum {
    fn from(m: Momentum) -> Self {
        match m {
            Momentum::Rising => MarketMomentum::Rising,
            Momentum::Falling => MarketMomentum::Falling,
            Momentum::Flat => MarketMomentum::Flat,
            Momentum::Unknown => MarketMomentum::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PriceAction {
    RaiseNow,
    Raise,
    Watch,
    CutNow,
    Cut,
    Hold,
    None,
}

impl From<Action> for PriceAction {
    fn from(a: Action) -> Self {
        match a {
            Action::RaiseNow => PriceAction::RaiseNow,
            Action::Raise => PriceAction::Raise,
            Action::Watch => PriceAction::Watch,
            Action::CutNow => PriceAction::CutNow,
            Action::Cut => PriceAction::Cut,
            Action::Hold => PriceAction::Hold,
            Action::None => PriceAction::None,
        }
    }
}

/// One listing against the market.
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct MispricingRow {
    pub name: String,
    pub set_code: String,
    pub condition: String,
    pub language: String,
    pub is_foil: bool,
    pub location: String,
    pub quantity: i64,
    pub age_days: i64,
    /// Our listing price (euros).
    pub listed: f64,
    /// Market reference adjusted to the condition (`condition_factor`).
    pub market: Option<f64>,
    pub condition_factor: f64,
    /// Cheapest listing on the market.
    pub market_low: Option<f64>,
    pub delta_pct: f64,
    /// |listed − market| × quantity (euros).
    pub impact: f64,
    pub verdict: Verdict,
    /// Listed below the market's cheapest listing.
    pub below_low: bool,
    /// Underpriced and listed within the last day (a typo?).
    pub recently_listed: bool,
    /// Non-English/German copy: the language-blind reference likely
    /// overstates its value.
    pub language_flagged: bool,
    /// Market data older than a week.
    pub stale: bool,
    /// The fair band applied (widened to the card's volatility).
    pub effective_threshold_pct: f64,
    pub momentum: MarketMomentum,
    pub action: PriceAction,
    pub priority: f64,
    pub pct_7d: Option<f64>,
    pub pct_30d: Option<f64>,
    pub cardmarket_url: Option<String>,
}

impl MispricingRow {
    fn new(c: &MispricedCard, change: &TrendChange) -> Self {
        Self {
            name: c.name.clone(),
            set_code: c.set_code.clone(),
            condition: c.condition.clone(),
            language: c.language.clone(),
            is_foil: c.is_foil,
            location: c.location.clone(),
            quantity: c.quantity,
            age_days: c.age_days,
            listed: c.listed_price,
            market: c.market_price,
            condition_factor: c.condition_factor,
            market_low: c.market_low,
            delta_pct: c.delta_pct,
            impact: impact(c),
            verdict: c.verdict.into(),
            below_low: c.below_low,
            recently_listed: c.verdict == PriceVerdict::Underpriced && c.recently_listed(),
            language_flagged: c.language_flagged,
            stale: c.stale,
            effective_threshold_pct: c.effective_threshold_pct,
            momentum: c.momentum.into(),
            action: c.action.into(),
            priority: c.priority,
            pct_7d: change.pct_7d,
            pct_30d: change.pct_30d,
            cardmarket_url: c.cardmarket_id.trim().parse::<u64>().ok().map(product_url),
        }
    }
}

/// Counts over all listings; the money totals only with `financials.view`.
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct MispricingSummary {
    pub underpriced_rows: usize,
    pub underpriced_copies: i64,
    pub overpriced_rows: usize,
    pub overpriced_copies: i64,
    pub fair_rows: usize,
    pub no_data_rows: usize,
    pub raise_now_rows: usize,
    pub cut_now_rows: usize,
    pub stale_rows: usize,
    /// Extra revenue if every underpriced copy were raised to market.
    pub upside: Option<f64>,
    /// Capital above market in overpriced listings.
    pub excess: Option<f64>,
    /// Listed and market value of the listings with market data.
    pub listed_value: Option<f64>,
    pub market_value: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ConflictKind {
    ConditionInversion,
    FoilInversion,
    DuplicatePrice,
}

impl From<IssueKind> for ConflictKind {
    fn from(k: IssueKind) -> Self {
        match k {
            IssueKind::ConditionInversion => ConflictKind::ConditionInversion,
            IssueKind::FoilInversion => ConflictKind::FoilInversion,
            IssueKind::DuplicatePrice => ConflictKind::DuplicatePrice,
        }
    }
}

/// Two of our own listings that contradict each other.
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct Conflict {
    pub kind: ConflictKind,
    pub name: String,
    pub set_code: String,
    /// The two listings (condition/foil, price, location each).
    pub details: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MispricingView {
    pub version: u64,
    pub summary: MispricingSummary,
    /// The first [`MISPRICING_MAX_ROWS`] of the filtered, sorted rows.
    pub rows: Vec<MispricingRow>,
    /// Rows after the filter.
    pub total: usize,
    /// The first [`MAX_ISSUES`] conflicts, and how many there are.
    pub conflicts: Vec<Conflict>,
    pub conflicts_total: usize,
}

async fn report(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiQuery(query): ApiQuery<MispricingQuery>,
) -> Result<Json<MispricingView>, ApiError> {
    if !query.threshold.is_finite() || !(0.0..=100.0).contains(&query.threshold) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_params",
            "The fair band must be between 0 and 100 %",
        ));
    }
    let financials = user.permissions.contains(Permission::FinancialsView);
    let data = state.market.loaded().ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "not_loaded",
            "The market data is not loaded yet",
        )
    })?;
    let view = blocking(move || {
        let field = query.field.into();
        let report = report_from_tracker(
            &data.cards,
            &data.latest,
            &data.snapshots,
            field,
            query.threshold,
            data.today,
        );
        let money = |v: f64| financials.then_some(v);
        let summary = MispricingSummary {
            underpriced_rows: report.underpriced_rows,
            underpriced_copies: report.underpriced_copies,
            overpriced_rows: report.overpriced_rows,
            overpriced_copies: report.overpriced_copies,
            fair_rows: report.fair_rows,
            no_data_rows: report.no_data_rows,
            raise_now_rows: report.raise_now_rows,
            cut_now_rows: report.cut_now_rows,
            stale_rows: report.stale_rows,
            upside: money(report.underpriced_upside),
            excess: money(report.overpriced_excess),
            listed_value: money(report.total_listed_value),
            market_value: money(report.total_market_value),
        };
        let filter: VerdictFilter = query.filter.into();
        let filtered: Vec<MispricedCard> = report
            .rows
            .into_iter()
            .filter(|c| filter.matches(c.verdict))
            .collect();
        let mut rows = with_changes(filtered, &data.snapshots, field);
        sort_mispriced(&mut rows, query.sort.into(), query.desc);
        let issues = find_issues(&data.cards);
        MispricingView {
            version: data.version,
            summary,
            total: rows.len(),
            rows: rows
                .iter()
                .take(MISPRICING_MAX_ROWS)
                .map(|(c, change)| MispricingRow::new(c, change))
                .collect(),
            conflicts_total: issues.len(),
            conflicts: issues
                .into_iter()
                .take(MAX_ISSUES)
                .map(|i| Conflict {
                    kind: i.kind.into(),
                    name: i.name,
                    set_code: i.set_code,
                    details: i.details,
                })
                .collect(),
        }
    })
    .await?;
    Ok(Json(view))
}

#[cfg(test)]
#[path = "mispricing_tests.rs"]
mod tests;
