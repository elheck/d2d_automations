//! The start page's "since your last visit" digest.
//!
//! - `POST /api/digest` (logged in): records today's visit of the user (a
//!   write, hence POST with the CSRF check) and returns what changed since
//!   their previous visit day. Sales figures only with `analysis.view`,
//!   revenue only with `financials.view` as well; the day of the last
//!   import for everyone.

use super::{Access, Api, CurrentUser};
use crate::app::AppState;
use crate::http::ApiError;
use crate::permissions::Permission;
use crate::visits::record_visit;
use axum::extract::State;
use axum::routing::post;
use axum::Json;
use check_stock_core::inventory_db::digest_since_conn;
use check_stock_core::precon_loader::local_date;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub(super) fn routes(api: Api) -> Api {
    api.route("/api/digest", Access::LoggedIn, post(digest))
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct DigestView {
    /// The previous visit day the figures start after; `None` on the first
    /// visit.
    pub since: Option<String>,
    /// Day of the last inventory import.
    pub last_import: Option<String>,
    /// Sales figures (only with `analysis.view`).
    pub sold_copies: Option<i64>,
    pub new_listings: Option<i64>,
    pub bulk_lots: Option<i64>,
    pub restock_candidates: Option<i64>,
    /// Money (`analysis.view` and `financials.view`).
    pub sold_revenue: Option<f64>,
    pub bulk_revenue: Option<f64>,
}

async fn digest(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<DigestView>, ApiError> {
    let today = local_date(Utc::now().timestamp())
        .format("%Y-%m-%d")
        .to_string();
    let id = user.id;
    let since = state
        .db
        .write(move |conn| record_visit(conn, id, &today))
        .await?;
    let d = state
        .db
        .read(move |conn| digest_since_conn(conn, since.as_deref()))
        .await?;
    let sales = user.permissions.contains(Permission::AnalysisView);
    let money = sales && user.permissions.contains(Permission::FinancialsView);
    let s = |v: i64| sales.then_some(v);
    let m = |v: f64| money.then_some(v);
    Ok(Json(DigestView {
        since: d.since,
        last_import: d.last_sync,
        sold_copies: s(d.sold_copies),
        new_listings: s(d.new_listings),
        bulk_lots: s(d.bulk_lots.lots),
        restock_candidates: s(d.restock_candidates),
        sold_revenue: m(d.sold_revenue),
        bulk_revenue: m(d.bulk_lots.revenue),
    }))
}

#[cfg(test)]
#[path = "home_tests.rs"]
mod tests;
