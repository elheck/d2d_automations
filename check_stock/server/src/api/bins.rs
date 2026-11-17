//! Bin analysis (`analysis.view`) and move lists (`bins.use`) on the file of
//! the last inventory import, see [`crate::bins`].
//!
//! - `GET /api/bins?min_free=&sort=`: every bin with its fill; `/export` the
//!   same as CSV (spreadsheet-safe).
//! - `GET /api/bins/fragmented`: variants spread over several bins, and what
//!   gathering them would take.
//! - `GET /api/bins/consolidation?threshold=`: what emptying the bins filled
//!   up to `threshold` would take.
//! - `GET` / `POST` / `DELETE /api/bins/list`: the user's move list;
//!   `PUT /api/bins/list/ticks` ticks moves; `GET /api/bins/list/export` the
//!   ticked piles as a Cardmarket stock-update CSV (exact values: it is
//!   imported again).
//! - `GET /api/bins/image/{set}/{number}`: card images for the move list.

use super::{blocking, in_transaction, Access, Api, ApiJson, ApiQuery, CurrentUser};
use crate::app::AppState;
use crate::audit::{self, action};
use crate::bins::{
    discard_list, load_list, save_list, set_ticks, ticks, MoveListKind, Plan, StoredList,
    MAX_LIST_MOVES,
};
use crate::http::ApiError;
use crate::permissions::Permission;
use crate::stock::{Source, StockError};
use crate::users::StoreError;
use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Json;
use check_stock_core::bin_consolidation::{
    bin_loads, fragmented_variants, to_update_csv, BIN_CAPACITY,
};
use check_stock_core::cache::ImageCache;
use check_stock_core::card_matching::get_card_name;
use check_stock_core::models::Language;
use check_stock_core::spreadsheet::spreadsheet_safe;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use ts_rs::TS;

/// Fragmented variants listed (all count in `total`).
const FRAGMENTED_LIMIT: usize = 200;
/// Longest move id accepted (16 hex characters plus a counter).
const MAX_ID_CHARS: usize = 24;

pub(super) fn routes(api: Api) -> Api {
    api.route(
        "/api/bins",
        Access::Permission(Permission::AnalysisView),
        get(bins),
    )
    .route(
        "/api/bins/export",
        Access::Permission(Permission::AnalysisView),
        get(bins_export),
    )
    .route(
        "/api/bins/fragmented",
        Access::Permission(Permission::AnalysisView),
        get(fragmented),
    )
    .route(
        "/api/bins/consolidation",
        Access::Permission(Permission::AnalysisView),
        get(consolidation),
    )
    .route(
        "/api/bins/list",
        Access::Permission(Permission::BinsUse),
        get(get_list).post(create_list).delete(delete_list),
    )
    .route(
        "/api/bins/list/ticks",
        Access::Permission(Permission::BinsUse),
        axum::routing::put(put_ticks),
    )
    .route(
        "/api/bins/list/export",
        Access::Permission(Permission::BinsUse),
        get(list_export),
    )
    .route(
        "/api/bins/image/{set}/{number}",
        Access::Permission(Permission::BinsUse),
        get(super::singles::image),
    )
}

// ── Errors ─────────────────────────────────────────────────────────────────

impl From<StockError> for ApiError {
    fn from(e: StockError) -> Self {
        match e {
            StockError::Db(e) => e.into(),
            StockError::Unreadable | StockError::Internal => ApiError::internal(),
        }
    }
}

fn no_import() -> ApiError {
    ApiError::conflict(
        "no_import",
        "Bin analysis needs an inventory import on the server (Tools → Inventory import): only the export lists every pile with its own location",
    )
}

fn outdated() -> ApiError {
    ApiError::conflict(
        "outdated",
        "The inventory was imported again since this list was made; make the list again (ticked piles that are still in it stay ticked)",
    )
}

fn no_list() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "no_list", "There is no move list")
}

fn invalid(code: &'static str, message: &'static str) -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, code, message)
}

pub(super) async fn source(state: &AppState) -> Result<Option<Arc<Source>>, ApiError> {
    Ok(state.stock.source(&state.db).await?)
}

async fn required_source(state: &AppState) -> Result<Arc<Source>, ApiError> {
    source(state).await?.ok_or_else(no_import)
}

// ── Bin table ──────────────────────────────────────────────────────────────

/// The kept import file the figures come from.
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct SourceInfo {
    pub version: i64,
    pub file_name: String,
    /// RFC 3339.
    pub imported_at: String,
}

impl From<&Source> for SourceInfo {
    fn from(s: &Source) -> Self {
        Self {
            version: s.version,
            file_name: s.file_name.clone(),
            imported_at: s.imported_at.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum BinSort {
    /// Most free slots first.
    Free,
    /// Shelf order.
    Location,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BinsQuery {
    /// Only bins with at least this many free slots (0–60).
    pub min_free: i64,
    pub sort: BinSort,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct BinRow {
    pub bin: String,
    pub cards: i64,
    pub piles: i64,
    pub free: i64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BinsView {
    /// `None` before the first import on the server.
    pub source: Option<SourceInfo>,
    pub capacity: i64,
    pub bins_total: i64,
    pub cards_total: i64,
    /// In-stock rows without a recognisable bin location.
    pub unplaced_rows: i64,
    /// The bins with at least `min_free` free slots, sorted.
    pub rows: Vec<BinRow>,
}

fn n(v: usize) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

async fn bin_view(state: &AppState, query: BinsQuery) -> Result<BinsView, ApiError> {
    if !(0..=BIN_CAPACITY).contains(&query.min_free) {
        return Err(ApiError::bad_request("min_free must be between 0 and 60"));
    }
    let Some(source) = source(state).await? else {
        return Ok(BinsView {
            source: None,
            capacity: BIN_CAPACITY,
            bins_total: 0,
            cards_total: 0,
            unplaced_rows: 0,
            rows: Vec::new(),
        });
    };
    blocking(move || {
        let (loads, unplaced) = bin_loads(&source.cards);
        let mut rows: Vec<BinRow> = loads
            .iter()
            .filter(|b| b.free() >= query.min_free)
            .map(|b| BinRow {
                bin: b.bin.clone(),
                cards: b.cards,
                piles: n(b.piles),
                free: b.free(),
            })
            .collect();
        if query.sort == BinSort::Free {
            // Stable: shelf order within the same free slots.
            rows.sort_by_key(|r| std::cmp::Reverse(r.free));
        }
        BinsView {
            source: Some(SourceInfo::from(source.as_ref())),
            capacity: BIN_CAPACITY,
            bins_total: n(loads.len()),
            cards_total: loads.iter().map(|b| b.cards).sum(),
            unplaced_rows: n(unplaced),
            rows,
        }
    })
    .await
}

async fn bins(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<BinsQuery>,
) -> Result<Json<BinsView>, ApiError> {
    Ok(Json(bin_view(&state, query).await?))
}

fn csv_response(file_name: &'static str, csv: String) -> Response {
    let disposition = format!("attachment; filename=\"{file_name}\"");
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/csv; charset=utf-8"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&disposition).expect("static file name"),
            ),
        ],
        csv,
    )
        .into_response()
}

async fn bins_export(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<BinsQuery>,
) -> Result<Response, ApiError> {
    let view = bin_view(&state, query).await?;
    let mut csv = String::from("bin,cards,piles,free\n");
    for row in &view.rows {
        csv.push_str(&format!(
            "{},{},{},{}\n",
            csv_cell(&spreadsheet_safe(&row.bin)),
            row.cards,
            row.piles,
            row.free
        ));
    }
    Ok(csv_response("bin_analysis.csv", csv))
}

/// A CSV cell, quoted when needed.
fn csv_cell(text: &str) -> String {
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

// ── Fragmented variants and plan summaries ─────────────────────────────────

/// What a plan would do.
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct PlanSummary {
    pub kind: MoveListKind,
    pub threshold: Option<i64>,
    pub moves: i64,
    pub cards_moved: i64,
    /// Bins that end up empty.
    pub bins_freed: Vec<String>,
    /// Sum of the move distances (lower = less walking).
    pub total_distance: i64,
    /// Sparse bins looked at (consolidation).
    pub sources_considered: i64,
    pub fragmented_variants: i64,
    /// More moves than a move list can hold.
    pub too_large: bool,
}

impl From<&Plan> for PlanSummary {
    fn from(p: &Plan) -> Self {
        Self {
            kind: p.kind,
            threshold: p.threshold,
            moves: n(p.moves.len()),
            cards_moved: p.cards_moved,
            bins_freed: p.bins_freed.clone(),
            total_distance: p.total_distance,
            sources_considered: n(p.sources_considered),
            fragmented_variants: n(p.fragmented_variants),
            too_large: p.moves.len() > MAX_LIST_MOVES,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct PileView {
    pub location: String,
    pub quantity: i64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct FragmentedRow {
    pub name: String,
    pub condition: String,
    pub language: String,
    pub foil: bool,
    pub copies: i64,
    pub bins: i64,
    pub piles: Vec<PileView>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FragmentedView {
    pub source: Option<SourceInfo>,
    /// All fragmented variants (the first 200 are listed).
    pub total: i64,
    pub variants: Vec<FragmentedRow>,
    /// Gathering every variant into one bin.
    pub defrag: Option<PlanSummary>,
}

async fn fragmented(State(state): State<AppState>) -> Result<Json<FragmentedView>, ApiError> {
    let Some(source) = source(&state).await? else {
        return Ok(Json(FragmentedView {
            source: None,
            total: 0,
            variants: Vec::new(),
            defrag: None,
        }));
    };
    let plan = state
        .bins
        .plan(source.clone(), MoveListKind::Defrag, None)
        .await?;
    let info = SourceInfo::from(source.as_ref());
    let (total, variants) = blocking(move || {
        let all = fragmented_variants(&source.cards);
        let rows = all
            .iter()
            .take(FRAGMENTED_LIMIT)
            .map(|f| FragmentedRow {
                name: f.name.clone(),
                condition: f.condition.clone(),
                language: f.language.clone(),
                foil: f.is_foil,
                copies: f.total_copies,
                bins: n(f.bin_count()),
                piles: f
                    .placements
                    .iter()
                    .map(|(_, location, quantity)| PileView {
                        location: location.clone(),
                        quantity: *quantity,
                    })
                    .collect(),
            })
            .collect();
        (n(all.len()), rows)
    })
    .await?;
    Ok(Json(FragmentedView {
        source: Some(info),
        total,
        variants,
        defrag: Some(PlanSummary::from(plan.as_ref())),
    }))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsolidationQuery {
    /// Bins filled up to this many cards are emptied (1–60).
    pub threshold: i64,
}

fn check_threshold(threshold: i64) -> Result<(), ApiError> {
    if (1..=BIN_CAPACITY).contains(&threshold) {
        Ok(())
    } else {
        Err(invalid(
            "invalid_threshold",
            "The fill to empty must be between 1 and 60 cards",
        ))
    }
}

async fn consolidation(
    State(state): State<AppState>,
    ApiQuery(query): ApiQuery<ConsolidationQuery>,
) -> Result<Json<PlanSummary>, ApiError> {
    check_threshold(query.threshold)?;
    let source = required_source(&state).await?;
    let plan = state
        .bins
        .plan(source, MoveListKind::Consolidation, Some(query.threshold))
        .await?;
    Ok(Json(PlanSummary::from(plan.as_ref())))
}

// ── Move lists ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct MoveView {
    pub id: String,
    pub name: String,
    /// The name in the card's language, when it differs.
    pub local_name: Option<String>,
    pub set_name: String,
    pub set_code: String,
    pub collector_number: String,
    pub condition: String,
    pub language: String,
    pub foil: bool,
    pub quantity: i64,
    pub from_location: String,
    pub to_location: String,
    pub from_bin: String,
    pub to_bin: String,
    pub done: bool,
    /// The card image route, when set code and number are usable.
    pub image: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MoveListView {
    pub kind: MoveListKind,
    pub threshold: Option<i64>,
    /// RFC 3339.
    pub created_at: String,
    /// The import the list was made on.
    pub version: i64,
    /// A newer import exists (or none is kept): the list has to be made
    /// again before it can be used.
    pub outdated: bool,
    /// The current import file.
    pub source: Option<SourceInfo>,
    /// `None` and no moves when outdated.
    pub summary: Option<PlanSummary>,
    pub moves: Vec<MoveView>,
    pub ticked: i64,
}

fn move_view(m: &crate::bins::PlannedMove, done: bool) -> MoveView {
    let c = &m.planned.card;
    let local = get_card_name(c, Language::parse(&c.language));
    let set = c.set_code.trim().to_ascii_lowercase();
    let number = c.cn.trim();
    MoveView {
        id: m.id.clone(),
        name: c.name.clone(),
        local_name: (local != c.name && !local.is_empty()).then(|| local.to_string()),
        set_name: c.set.clone(),
        set_code: c.set_code.clone(),
        collector_number: c.cn.clone(),
        condition: c.condition.clone(),
        language: c.language.clone(),
        foil: c.is_foil_card(),
        quantity: m.planned.quantity,
        from_location: m.planned.from_location.clone(),
        to_location: m.planned.to_location.clone(),
        from_bin: m.planned.from_bin.clone(),
        to_bin: m.planned.to_bin.clone(),
        done,
        image: ImageCache::is_safe_key(&set, number)
            .then(|| format!("/api/bins/image/{set}/{number}")),
    }
}

/// The stored list with its plan on the current file, or `None` for the
/// plan when it is outdated.
struct Current {
    stored: StoredList,
    source: Option<Arc<Source>>,
    plan: Option<Arc<Plan>>,
    ticks: HashSet<String>,
}

async fn current(state: &AppState, user_id: i64) -> Result<Option<Current>, ApiError> {
    let (stored, ticks) = state
        .db
        .read(move |conn| Ok((load_list(conn, user_id)?, ticks(conn, user_id)?)))
        .await?;
    let Some(stored) = stored else {
        return Ok(None);
    };
    let source = source(state).await?;
    let plan = match &source {
        Some(s) if s.version == stored.version && s.discard_seq == stored.discard_seq => Some(
            state
                .bins
                .plan(s.clone(), stored.kind, stored.threshold)
                .await?,
        ),
        _ => None,
    };
    Ok(Some(Current {
        stored,
        source,
        plan,
        ticks,
    }))
}

fn list_view(current: &Current) -> MoveListView {
    let moves: Vec<MoveView> = current
        .plan
        .iter()
        .flat_map(|p| p.moves.iter())
        .map(|m| move_view(m, current.ticks.contains(&m.id)))
        .collect();
    MoveListView {
        kind: current.stored.kind,
        threshold: current.stored.threshold,
        created_at: current.stored.created_at.clone(),
        version: current.stored.version,
        outdated: current.plan.is_none(),
        source: current.source.as_deref().map(SourceInfo::from),
        summary: current.plan.as_deref().map(PlanSummary::from),
        ticked: n(current.ticks.len()),
        moves,
    }
}

async fn get_list(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<Option<MoveListView>>, ApiError> {
    let Some(current) = current(&state, user.id).await? else {
        return Ok(Json(None));
    };
    let view = blocking(move || list_view(&current)).await?;
    Ok(Json(Some(view)))
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, optional_fields)]
pub struct MoveListRequest {
    pub kind: MoveListKind,
    /// Consolidation: bins filled up to this many cards are emptied (1–60).
    pub threshold: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MoveListCreated {
    pub list: MoveListView,
    /// Ticks of the previous list whose move is in this one too.
    pub carried_ticks: i64,
    /// Ticks of the previous list whose move is not in this one.
    pub dropped_ticks: i64,
}

async fn create_list(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<MoveListRequest>,
) -> Result<Json<MoveListCreated>, ApiError> {
    let threshold = match request.kind {
        MoveListKind::Consolidation => {
            let threshold = request
                .threshold
                .unwrap_or(crate::bins::DEFAULT_MERGE_THRESHOLD);
            check_threshold(threshold)?;
            Some(threshold)
        }
        MoveListKind::Defrag => None,
    };
    let source = required_source(&state).await?;
    let plan = state
        .bins
        .plan(source.clone(), request.kind, threshold)
        .await?;
    if plan.moves.is_empty() {
        return Err(invalid(
            "no_moves",
            "There is nothing to move with these settings",
        ));
    }
    if plan.moves.len() > MAX_LIST_MOVES {
        return Err(invalid(
            "too_many_moves",
            "That would be more than 5,000 moves; choose a lower fill to empty",
        ));
    }
    let (id, username) = (user.id, user.username.clone());
    let saved = plan.clone();
    let (carried, dropped) = state
        .db
        .write(move |conn| {
            in_transaction(conn, |tx| {
                let now = Utc::now();
                let (carried, dropped) = save_list(tx, id, &saved, now)?;
                audit::record(
                    tx,
                    now,
                    crate::audit::Actor::User {
                        id,
                        username: &username,
                    },
                    action::BINS_LIST_CREATED,
                    Some(saved.kind.key()),
                    serde_json::json!({
                        "threshold": saved.threshold,
                        "version": saved.version,
                        "moves": saved.moves.len(),
                        "carried_ticks": carried,
                        "dropped_ticks": dropped,
                    }),
                )?;
                Ok::<_, StoreError>((carried, dropped))
            })
        })
        .await??;
    let current = current(&state, user.id).await?.ok_or_else(no_list)?;
    let list = blocking(move || list_view(&current)).await?;
    Ok(Json(MoveListCreated {
        list,
        carried_ticks: n(carried),
        dropped_ticks: n(dropped),
    }))
}

async fn delete_list(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<StatusCode, ApiError> {
    let (id, username) = (user.id, user.username.clone());
    let found = state
        .db
        .write(move |conn| {
            in_transaction(conn, |tx| {
                let found = discard_list(tx, id)?;
                if found {
                    audit::record(
                        tx,
                        Utc::now(),
                        crate::audit::Actor::User {
                            id,
                            username: &username,
                        },
                        action::BINS_LIST_DISCARDED,
                        None,
                        serde_json::json!({}),
                    )?;
                }
                Ok::<_, StoreError>(found)
            })
        })
        .await??;
    if found {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(no_list())
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct TickRequest {
    pub ids: Vec<String>,
    pub done: bool,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TickResult {
    pub ticked: i64,
}

async fn put_ticks(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<TickRequest>,
) -> Result<Json<TickResult>, ApiError> {
    if request.ids.is_empty()
        || request.ids.len() > MAX_LIST_MOVES
        || request.ids.iter().any(|id| id.len() > MAX_ID_CHARS)
    {
        return Err(invalid("invalid_ids", "Tick between 1 and 5,000 moves"));
    }
    let current = current(&state, user.id).await?.ok_or_else(no_list)?;
    let Some(plan) = current.plan else {
        return Err(outdated());
    };
    let known = plan.ids();
    if request.ids.iter().any(|id| !known.contains(id.as_str())) {
        return Err(invalid("unknown_move", "That move is not in the list"));
    }
    let (id, ids, done) = (user.id, request.ids, request.done);
    let ticked = state
        .db
        .write(move |conn| {
            let tx = conn.transaction()?;
            let ticked = set_ticks(&tx, id, &ids, done)?;
            tx.commit()?;
            Ok(ticked)
        })
        .await?;
    Ok(Json(TickResult { ticked: n(ticked) }))
}

async fn list_export(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Response, ApiError> {
    let current = current(&state, user.id).await?.ok_or_else(no_list)?;
    let Some(plan) = current.plan else {
        return Err(outdated());
    };
    let ticks = current.ticks;
    let kind = current.stored.kind;
    let csv = blocking(move || {
        let moved: Vec<_> = plan
            .moves
            .iter()
            .filter(|m| ticks.contains(&m.id))
            .map(|m| m.planned.clone())
            .collect();
        (!moved.is_empty()).then(|| to_update_csv(&moved))
    })
    .await?
    .ok_or_else(|| invalid("nothing_ticked", "Tick the piles you moved first"))?;
    let name = match kind {
        MoveListKind::Consolidation => "bin_consolidation_moved.csv",
        MoveListKind::Defrag => "bin_defrag_moved.csv",
    };
    Ok(csv_response(name, csv))
}

#[cfg(test)]
#[path = "bins_tests.rs"]
mod tests;
