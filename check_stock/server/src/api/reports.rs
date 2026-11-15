//! Read-only reports on the inventory data (Phase 3).
//!
//! - `GET /api/data-info` (logged in): how current the data is — when the
//!   desktop copy was taken and the day of the last inventory import
//! - `GET /api/restock?min_copies=&sort=&desc=` (`analysis.view`): sold-out
//!   variants ranked by sell-through speed
//! - `GET /api/restock/export?…` (`analysis.view`): the same list as a
//!   buy-list CSV download
//!
//! Money figures (realized revenue) are only sent to users who also hold
//! `financials.view`; without it they are left out, and sorting by them is
//! refused (the order would give them away).

use super::{blocking, Access, Api, ApiQuery, CurrentUser};
use crate::app::AppState;
use crate::http::ApiError;
use crate::import::META_COPY_TAKEN_AT;
use crate::migrations::get_meta;
use crate::permissions::Permission;
use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use check_stock_core::api::cardmarket::product_url;
use check_stock_core::inventory_db::{get_restock_candidates_conn, last_sync_date_conn};
use check_stock_core::restock::{
    format_buy_list_csv, rank_candidates, sort_ranked, RankedRestock, RestockSort,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Rows sent for the table (the export has all of them), as on the desktop.
pub const RESTOCK_MAX_ROWS: usize = 300;
/// Highest "minimum copies sold".
pub const MAX_MIN_COPIES: i64 = 1000;

pub(super) fn routes(api: Api) -> Api {
    api.route("/api/data-info", Access::LoggedIn, get(data_info))
        .route(
            "/api/restock",
            Access::Permission(Permission::AnalysisView),
            get(restock),
        )
        .route(
            "/api/restock/export",
            Access::Permission(Permission::AnalysisView),
            get(restock_export),
        )
}

// ── Data info ──────────────────────────────────────────────────────────────

/// How current the inventory data is.
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct DataInfo {
    /// When the copy of the desktop app's database was taken (RFC 3339);
    /// `None` for a server that never imported one.
    pub copy_taken_at: Option<String>,
    /// Day of the last inventory import in that data (`YYYY-MM-DD`).
    pub last_import: Option<String>,
}

async fn data_info(State(state): State<AppState>) -> Result<Json<DataInfo>, ApiError> {
    let info = state
        .db
        .read(|conn| {
            let copy_taken_at = get_meta(conn, META_COPY_TAKEN_AT)?;
            let last_import = last_sync_date_conn(conn)?;
            Ok(DataInfo {
                copy_taken_at,
                last_import,
            })
        })
        .await?;
    Ok(Json(info))
}

// ── Restock ────────────────────────────────────────────────────────────────

/// Restock table columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RestockSortKey {
    Velocity,
    SoldCopies,
    Revenue,
    Days,
    Price,
    Name,
    Lot,
}

impl From<RestockSortKey> for RestockSort {
    fn from(k: RestockSortKey) -> Self {
        match k {
            RestockSortKey::Velocity => RestockSort::Velocity,
            RestockSortKey::SoldCopies => RestockSort::SoldCopies,
            RestockSortKey::Revenue => RestockSort::Revenue,
            RestockSortKey::Days => RestockSort::Days,
            RestockSortKey::Price => RestockSort::Price,
            RestockSortKey::Name => RestockSort::Name,
            RestockSortKey::Lot => RestockSort::Lot,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestockQuery {
    min_copies: i64,
    sort: RestockSortKey,
    desc: bool,
}

/// One sold-out variant worth buying again.
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct RestockRow {
    pub name: String,
    pub set_code: String,
    pub cn: String,
    pub condition: String,
    pub language: String,
    pub is_foil: bool,
    pub rarity: String,
    /// Purchase lot of the last copies (`L2`), if the location had one.
    pub lot: Option<String>,
    pub sold_copies: i64,
    pub copies_per_week: f64,
    pub days_to_sell_out: i64,
    /// Price it was last listed at (euros).
    pub last_price: f64,
    /// Only with `financials.view`.
    pub realized_revenue: Option<f64>,
    pub sold_out_date: String,
    pub cardmarket_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct RestockView {
    /// The first [`RESTOCK_MAX_ROWS`] in the requested order.
    pub rows: Vec<RestockRow>,
    /// Candidates in total (after the minimum).
    pub total: usize,
    pub copies: i64,
    /// Σ realized revenue; only with `financials.view`.
    pub revenue: Option<f64>,
}

fn restock_row(r: &RankedRestock, financials: bool) -> RestockRow {
    let c = &r.candidate;
    RestockRow {
        name: c.name.clone(),
        set_code: c.set_code.clone(),
        cn: c.cn.clone(),
        condition: c.condition.clone(),
        language: c.language.clone(),
        is_foil: c.is_foil,
        rarity: c.rarity.clone(),
        lot: c.lot.clone(),
        sold_copies: c.sold_copies,
        copies_per_week: r.copies_per_week,
        days_to_sell_out: r.days_to_sell_out,
        last_price: c.last_price,
        realized_revenue: financials.then_some(c.realized_revenue),
        sold_out_date: c.sold_out_date.clone(),
        cardmarket_url: c.cardmarket_id.trim().parse::<u64>().ok().map(product_url),
    }
}

/// The ranked, sorted restock list for `query` (checked against the user's
/// permissions).
async fn ranked(
    state: &AppState,
    user: &CurrentUser,
    query: RestockQuery,
) -> Result<Vec<RankedRestock>, ApiError> {
    if !(1..=MAX_MIN_COPIES).contains(&query.min_copies) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_params",
            "The minimum must be between 1 and 1000 copies",
        ));
    }
    if query.sort == RestockSortKey::Revenue
        && !user.permissions.contains(Permission::FinancialsView)
    {
        return Err(ApiError::forbidden(Permission::FinancialsView));
    }
    // All categories: sleeves and other accessories are worth restocking too.
    let candidates = state
        .db
        .read(|conn| get_restock_candidates_conn(conn, None))
        .await?;
    blocking(move || {
        let mut rows = rank_candidates(candidates, query.min_copies);
        sort_ranked(&mut rows, query.sort.into(), query.desc);
        rows
    })
    .await
}

async fn restock(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiQuery(query): ApiQuery<RestockQuery>,
) -> Result<Json<RestockView>, ApiError> {
    let financials = user.permissions.contains(Permission::FinancialsView);
    let rows = ranked(&state, &user, query).await?;
    let copies = rows.iter().map(|r| r.candidate.sold_copies).sum();
    let revenue = financials.then(|| rows.iter().map(|r| r.candidate.realized_revenue).sum());
    Ok(Json(RestockView {
        total: rows.len(),
        copies,
        revenue,
        rows: rows
            .iter()
            .take(RESTOCK_MAX_ROWS)
            .map(|r| restock_row(r, financials))
            .collect(),
    }))
}

async fn restock_export(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiQuery(query): ApiQuery<RestockQuery>,
) -> Result<Response, ApiError> {
    let financials = user.permissions.contains(Permission::FinancialsView);
    let rows = ranked(&state, &user, query).await?;
    let csv = blocking(move || format_buy_list_csv(&rows, financials)).await?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/csv; charset=utf-8"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=\"restock_buy_list.csv\""),
            ),
        ],
        csv,
    )
        .into_response())
}

#[cfg(test)]
#[path = "reports_tests.rs"]
mod tests;
