//! Stock analysis (permission `analysis.view`): what is in stock, how fast it
//! sells, how old it is and how each purchase lot is doing.
//!
//! - `GET /api/stock-analysis`
//!
//! Money figures (stock value, revenue, value per aging bucket, lot value,
//! revenue, cost, margin, payback) are only sent with `financials.view`;
//! `money` says whether they are in the answer. Individual card prices (most
//! expensive, longest unsold) are listing prices and always shown.
//!
//! - `PUT /api/lot-costs/{lot}` `{cost, expected}` (`lot_costs.edit`, which
//!   needs `financials.view`): records (`cost` = null clears) a purchase
//!   lot's cost. `expected` is the cost the user saw: when someone changed
//!   it meanwhile the save is refused (409) instead of overwriting. Only
//!   lots the data knows; logged with before and after.

use super::{blocking, in_transaction, Access, Api, ApiJson, CurrentUser};
use crate::app::AppState;
use crate::http::ApiError;
use crate::permissions::Permission;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::Json;
use check_stock_core::category::DEFAULT_CATEGORY;
use check_stock_core::inventory_db::{
    delete_lot_cost_conn, get_db_stats_conn, is_lot_id, lot_cost_conn, lot_known_conn,
    set_lot_cost_conn, DbStats, LotTotals,
};
use check_stock_core::precon_loader::local_date;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub(super) fn routes(api: Api) -> Api {
    api.route(
        "/api/stock-analysis",
        Access::Permission(Permission::AnalysisView),
        get(stock_analysis),
    )
    .route(
        "/api/lot-costs/{lot}",
        Access::Permission(Permission::LotCostsEdit),
        put(put_lot_cost),
    )
}

/// Highest cost accepted for one purchase lot (euros).
pub const MAX_LOT_COST: f64 = 1_000_000.0;

/// A new cost for a purchase lot.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct LotCostRequest {
    /// Euros; `null` removes the recorded cost.
    pub cost: Option<f64>,
    /// The cost the page showed (`null`: none recorded).
    pub expected: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct LotCostView {
    pub lot: String,
    pub cost: Option<f64>,
}

/// What a lot-cost change is refused for.
enum CostRefusal {
    Unknown,
    Changed(Option<f64>),
}

fn same(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => (a - b).abs() < 1e-9,
        _ => false,
    }
}

async fn put_lot_cost(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(lot): Path<String>,
    ApiJson(request): ApiJson<LotCostRequest>,
) -> Result<Json<LotCostView>, ApiError> {
    if !is_lot_id(&lot) {
        return Err(ApiError::bad_request("Not a purchase lot (e.g. L12)"));
    }
    if request
        .cost
        .is_some_and(|c| !c.is_finite() || !(0.0..=MAX_LOT_COST).contains(&c))
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_cost",
            "A lot cost must be between 0 and 1,000,000 €",
        ));
    }
    let (id, username) = (user.id, user.username.clone());
    let today = local_date(chrono::Utc::now().timestamp())
        .format("%Y-%m-%d")
        .to_string();
    let target = lot.clone();
    let outcome = state
        .db
        .write(move |conn| {
            in_transaction(conn, |tx| {
                if !lot_known_conn(tx, &target)? {
                    return Ok(Err(CostRefusal::Unknown));
                }
                let before = lot_cost_conn(tx, &target)?;
                if !same(before, request.expected) {
                    return Ok(Err(CostRefusal::Changed(before)));
                }
                match request.cost {
                    Some(cost) => set_lot_cost_conn(tx, &target, cost, &today)?,
                    None => delete_lot_cost_conn(tx, &target)?,
                }
                crate::audit::record(
                    tx,
                    chrono::Utc::now(),
                    crate::audit::Actor::User {
                        id,
                        username: &username,
                    },
                    crate::audit::action::LOT_COST_CHANGED,
                    Some(&target),
                    serde_json::json!({ "before": before, "after": request.cost }),
                )?;
                Ok::<_, crate::users::StoreError>(Ok(request.cost))
            })
        })
        .await??;
    match outcome {
        Ok(cost) => Ok(Json(LotCostView { lot, cost })),
        Err(CostRefusal::Unknown) => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "unknown_lot",
            "The data has no such purchase lot",
        )),
        Err(CostRefusal::Changed(now)) => Err(ApiError::conflict(
            "changed",
            match now {
                Some(cost) => format!("Someone changed this lot's cost meanwhile (now {cost:.2} €); check and save again"),
                None => "Someone removed this lot's cost meanwhile; check and save again".to_string(),
            },
        )),
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct NamedCount {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct NamedPrice {
    pub name: String,
    pub price: f64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct NamedDate {
    pub name: String,
    pub date: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct UnsoldCard {
    pub name: String,
    /// Listed since (`YYYY-MM-DD`).
    pub date: String,
    pub price: f64,
    pub quantity: i64,
    pub location: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct Velocity {
    /// Days of sales history the figures cover.
    pub period_days: i64,
    pub sold_copies: i64,
    pub copies_per_week: f64,
    pub last7_copies: Option<i64>,
    pub last30_copies: Option<i64>,
    pub bulk_lots: i64,
    pub bulk_copies: i64,
    /// Money (only with `financials.view`).
    pub sold_revenue: Option<f64>,
    pub revenue_per_week: Option<f64>,
    pub bulk_revenue: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct AgingRow {
    pub label: String,
    pub min_days: i64,
    pub max_days: Option<i64>,
    pub listings: i64,
    pub copies: i64,
    /// Capital in this age bucket (only with `financials.view`).
    pub value: Option<f64>,
}

/// One purchase lot (the `L…` part of the location).
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct LotRow {
    pub lot: String,
    pub in_stock_listings: i64,
    pub in_stock_copies: i64,
    pub sold_copies: i64,
    /// Of `sold_copies`: copies sold in bulk lots.
    pub bulk_copies: i64,
    /// Money (only with `financials.view`).
    pub in_stock_value: Option<f64>,
    pub sold_revenue: Option<f64>,
    pub bulk_revenue: Option<f64>,
    /// Recorded purchase cost; `None` also when none is recorded.
    pub cost: Option<f64>,
    /// (revenue − cost) / cost.
    pub margin: Option<f64>,
    pub recouped: Option<bool>,
    pub cost_to_recoup: Option<f64>,
}

/// Totals of the lot table's money columns.
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct LotMoneyTotals {
    pub in_stock_value: f64,
    pub sold_revenue: f64,
    pub cost: f64,
    /// Across the lots with a recorded cost.
    pub margin: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StockAnalysisView {
    /// Whether the money figures are in this answer (`financials.view`).
    pub money: bool,
    pub total_articles: i64,
    pub in_stock_articles: i64,
    pub total_copies: i64,
    pub foil_count: i64,
    pub signed_count: i64,
    /// Listed value of the stock (only with `financials.view`).
    pub total_value: Option<f64>,
    pub first_synced_date: Option<String>,
    pub top_by_quantity: Vec<NamedCount>,
    pub top_by_price: Vec<NamedPrice>,
    pub longest_unsold: Vec<UnsoldCard>,
    pub oldest_listed: Option<NamedDate>,
    pub newest_listed: Option<NamedDate>,
    pub languages: Vec<NamedCount>,
    pub conditions: Vec<NamedCount>,
    pub rarities: Vec<NamedCount>,
    /// `None` until there are two days of history.
    pub velocity: Option<Velocity>,
    pub aging: Vec<AgingRow>,
    pub lots: Vec<LotRow>,
    pub lots_sold_copies: i64,
    pub lot_totals: Option<LotMoneyTotals>,
    /// All bulk lots ever built.
    pub bulk_lots: i64,
    pub bulk_copies: i64,
    pub bulk_revenue: Option<f64>,
}

fn counts(list: Vec<(String, i64)>) -> Vec<NamedCount> {
    list.into_iter()
        .map(|(name, count)| NamedCount { name, count })
        .collect()
}

/// The answer for `stats`, with money figures only when `money`.
pub fn view(stats: DbStats, money: bool) -> StockAnalysisView {
    let m = |v: f64| money.then_some(v);
    let lot_totals = LotTotals::of(&stats.lot_breakdown);
    StockAnalysisView {
        money,
        total_articles: stats.total_articles,
        in_stock_articles: stats.in_stock_articles,
        total_copies: stats.total_copies,
        foil_count: stats.foil_count,
        signed_count: stats.signed_count,
        total_value: m(stats.total_value),
        first_synced_date: stats.first_synced_date,
        top_by_quantity: counts(stats.top_by_quantity),
        top_by_price: stats
            .top_by_price
            .into_iter()
            .map(|(name, price)| NamedPrice { name, price })
            .collect(),
        longest_unsold: stats
            .top_oldest_in_stock
            .into_iter()
            .map(|e| UnsoldCard {
                name: e.name,
                date: e.date,
                price: e.price,
                quantity: e.quantity,
                location: e.location,
            })
            .collect(),
        oldest_listed: stats
            .oldest_listed
            .map(|(name, date)| NamedDate { name, date }),
        newest_listed: stats
            .newest_listed
            .map(|(name, date)| NamedDate { name, date }),
        languages: counts(stats.language_breakdown),
        conditions: counts(stats.condition_breakdown),
        rarities: counts(stats.rarity_breakdown),
        velocity: stats.velocity.map(|v| Velocity {
            period_days: v.period_days,
            sold_copies: v.sold_copies,
            copies_per_week: v.copies_per_week,
            last7_copies: v.last7_copies,
            last30_copies: v.last30_copies,
            bulk_lots: v.bulk_lots.lots,
            bulk_copies: v.bulk_lots.copies,
            sold_revenue: m(v.sold_revenue),
            revenue_per_week: m(v.revenue_per_week),
            bulk_revenue: m(v.bulk_lots.revenue),
        }),
        aging: stats
            .aging_buckets
            .into_iter()
            .map(|b| AgingRow {
                label: b.label.to_string(),
                min_days: b.min_days,
                max_days: b.max_days,
                listings: b.listings,
                copies: b.copies,
                value: m(b.value),
            })
            .collect(),
        lots_sold_copies: lot_totals.sold_copies,
        lot_totals: money.then(|| LotMoneyTotals {
            in_stock_value: lot_totals.in_stock_value,
            sold_revenue: lot_totals.sold_revenue,
            cost: lot_totals.cost,
            margin: lot_totals.realized_margin_fraction(),
        }),
        lots: stats
            .lot_breakdown
            .iter()
            .map(|l| LotRow {
                lot: l.lot.clone(),
                in_stock_listings: l.in_stock_listings,
                in_stock_copies: l.in_stock_copies,
                sold_copies: l.sold_copies,
                bulk_copies: l.bulk_copies,
                in_stock_value: m(l.in_stock_value),
                sold_revenue: m(l.sold_revenue),
                bulk_revenue: m(l.bulk_revenue),
                cost: if money { l.cost } else { None },
                margin: if money {
                    l.realized_margin_fraction()
                } else {
                    None
                },
                recouped: if money { l.is_recouped() } else { None },
                cost_to_recoup: if money { l.cost_to_recoup() } else { None },
            })
            .collect(),
        bulk_lots: stats.bulk_totals.lots,
        bulk_copies: stats.bulk_totals.copies,
        bulk_revenue: m(stats.bulk_totals.revenue),
    }
}

async fn stock_analysis(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<StockAnalysisView>, ApiError> {
    let money = user.permissions.contains(Permission::FinancialsView);
    let today = local_date(chrono::Utc::now().timestamp())
        .format("%Y-%m-%d")
        .to_string();
    let stats = state
        .db
        .read(move |conn| get_db_stats_conn(conn, DEFAULT_CATEGORY, &today))
        .await?;
    Ok(Json(blocking(move || view(stats, money)).await?))
}

#[cfg(test)]
#[path = "stock_analysis_tests.rs"]
mod tests;
