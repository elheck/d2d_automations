//! Bulk lots (rules in [`crate::bulk`]).
//!
//! Build (`bulk_lot.build`): `GET /api/bulk-lots/options`,
//! `POST /api/bulk-lots/preview`, `POST /api/bulk-lots/types/load`,
//! `GET /api/bulk-lots/types/status`, the user's draft
//! (`GET`/`POST`/`DELETE /api/bulk-lots/draft`, `PUT …/draft/picked`,
//! `POST …/draft/replace`), the history (`GET /api/bulk-lots`,
//! `GET /api/bulk-lots/{id}/cards`) and card images.
//! Finish (`bulk_lot.finish`): `POST /api/bulk-lots/finish`,
//! `GET /api/bulk-lots/{id}/csv`.
//! Manage (`bulk_lot.manage`): `PUT /api/bulk-lots/{id}/price`,
//! `POST /api/bulk-lots/{id}/removed`, `POST /api/bulk-lots/{id}/undo`.

use super::bins::{source, SourceInfo};
use super::{blocking, Access, Api, ApiJson, CurrentUser};
use crate::app::AppState;
use crate::audit::{self, action, Actor};
use crate::bulk::{
    self, backup, candidates, data_changed, delete_draft, draft_picks, load_draft, lookups,
    preview, replace, save_draft, shortfalls, Draft, SpecView,
};
use crate::http::ApiError;
use crate::inventory_import::latest_version;
use crate::jobs::JobStatus;
use crate::permissions::Permission;
use crate::stock::{kept_file_info, Source, CATEGORY};
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::Json;
use check_stock_core::bulk_lot::{
    default_lot_name, export_file_name, powertools_csv, Candidate, LotSpec, LotSummary, Preset,
    CONDITIONS,
};
use check_stock_core::cache::ImageCache;
use check_stock_core::inventory_db::{
    bulk_lot_cards_conn, delete_bulk_lot_in_tx, list_bulk_lots_conn, mark_bulk_lot_removed_conn,
    record_bulk_lot_in_tx, set_bulk_lot_price_conn, BulkLotSummary, MAX_LOT_NAME_CHARS,
    MAX_LOT_PRICE_EUR,
};
use check_stock_core::precon_loader::local_date;
use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use ts_rs::TS;

pub(super) fn routes(api: Api) -> Api {
    let build = Access::Permission(Permission::BulkLotBuild);
    let finish = Access::Permission(Permission::BulkLotFinish);
    let manage = Access::Permission(Permission::BulkLotManage);
    api.route("/api/bulk-lots/options", build, get(options))
        .route("/api/bulk-lots/preview", build, post(preview_lot))
        .route("/api/bulk-lots/types/load", build, post(load_types))
        .route("/api/bulk-lots/types/status", build, get(types_status))
        .route(
            "/api/bulk-lots/draft",
            build,
            get(get_draft).post(make_draft).delete(drop_draft),
        )
        .route("/api/bulk-lots/draft/picked", build, put(set_picked))
        .route("/api/bulk-lots/draft/replace", build, post(replace_pile))
        .route("/api/bulk-lots", build, get(history))
        .route("/api/bulk-lots/{id}/cards", build, get(lot_cards))
        .route(
            "/api/bulk-lots/image/{set}/{number}",
            build,
            get(super::singles::image),
        )
        .route("/api/bulk-lots/finish", finish, post(finish_lot))
        .route("/api/bulk-lots/{id}/csv", finish, get(lot_csv))
        .route("/api/bulk-lots/{id}/price", manage, put(set_price))
        .route("/api/bulk-lots/{id}/removed", manage, post(mark_removed))
        .route("/api/bulk-lots/{id}/undo", manage, post(undo))
}

fn today() -> NaiveDate {
    local_date(Utc::now().timestamp())
}

fn invalid(code: &'static str, message: impl Into<std::borrow::Cow<'static, str>>) -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, code, message)
}

fn no_import() -> ApiError {
    ApiError::conflict(
        "no_import",
        "Bulk lots need an inventory import on the server (Tools → Inventory import)",
    )
}

fn stock_changed() -> ApiError {
    ApiError::conflict(
        "stock_changed",
        "The stock changed meanwhile (an import or a discard); look at the lot again",
    )
}

fn not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "not_found",
        "There is no such bulk lot",
    )
}

fn spec_of(view: &SpecView) -> Result<LotSpec, ApiError> {
    view.to_spec().map_err(|m| invalid("invalid_spec", m))
}

async fn required_source(state: &AppState) -> Result<Arc<Source>, ApiError> {
    source(state).await?.ok_or_else(no_import)
}

/// Candidates of the current stock with the cached types.
async fn current(state: &AppState) -> Result<(Arc<Source>, Arc<Vec<Candidate>>), ApiError> {
    let source = required_source(state).await?;
    let types = state.bulk.clone();
    let for_read = source.clone();
    let candidates = state
        .db
        .read(move |conn| {
            types.with_types(|types| candidates(conn, &for_read, Some(types), today()))
        })
        .await?;
    Ok((source, Arc::new(candidates)))
}

// ── Views ──────────────────────────────────────────────────────────────────

/// One line of a lot: copies of one pile.
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct LotLine {
    pub pile: String,
    pub copies: i64,
    pub hit: bool,
    pub picked: bool,
    /// Copies of the pile still free for a lot now; less than `copies`
    /// means another lot or a discard took them ("Replace").
    pub free: i64,
    pub name: String,
    pub set_code: String,
    pub collector_number: String,
    pub condition: String,
    pub language: String,
    pub foil: bool,
    /// Listed price per copy.
    pub price: f64,
    pub location: String,
    pub purchase_lot: String,
    pub age_days: Option<i64>,
    pub image: Option<String>,
}

fn line(source: &Source, c: &Candidate, copies: i64, hit: bool, picked: bool) -> LotLine {
    let set = c.set_code.trim().to_ascii_lowercase();
    let number = c.cn.trim();
    LotLine {
        pile: source.ids[c.source].clone(),
        copies,
        hit,
        picked,
        free: c.available,
        name: c.name.clone(),
        set_code: c.set_code.clone(),
        collector_number: c.cn.clone(),
        condition: c.condition.clone(),
        language: c.language.clone(),
        foil: c.is_foil,
        price: c.price,
        location: c.location.clone(),
        purchase_lot: c.purchase_lot.clone(),
        age_days: c.age_days,
        image: ImageCache::is_safe_key(&set, number)
            .then(|| format!("/api/bulk-lots/image/{set}/{number}")),
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct FunnelStepView {
    pub label: String,
    pub copies: i64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct FunnelView {
    pub steps: Vec<FunnelStepView>,
    /// Copies promised to lots awaiting their import.
    pub reserved: i64,
    /// Copies whose card type is not known yet.
    pub types_unknown: i64,
    pub hit_pool: i64,
    /// The first rule that leaves fewer copies than the lot needs.
    pub bottleneck: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct LotSummaryView {
    pub copies: i64,
    /// Listed value of the copies.
    pub listed_value: f64,
    pub hits: i64,
    pub distinct_names: i64,
    pub oldest_days: Option<i64>,
    pub newest_days: Option<i64>,
    /// `(purchase lot, copies)`.
    pub purchase_lots: Vec<(String, i64)>,
}

impl From<LotSummary> for LotSummaryView {
    fn from(s: LotSummary) -> Self {
        Self {
            copies: s.copies,
            listed_value: s.listed_value,
            hits: s.hits,
            distinct_names: s.distinct_names as i64,
            oldest_days: s.oldest_days,
            newest_days: s.newest_days,
            purchase_lots: s.purchase_lots,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PresetView {
    pub label: String,
    pub spec: SpecView,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LanguageCount {
    pub language: String,
    pub copies: i64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BulkOptions {
    pub source: Option<SourceInfo>,
    pub default_spec: SpecView,
    pub presets: Vec<PresetView>,
    pub conditions: Vec<String>,
    /// Languages of the stock, most copies first.
    pub languages: Vec<LanguageCount>,
    pub max_lot_price: f64,
    pub max_name_chars: i64,
}

async fn options(State(state): State<AppState>) -> Result<Json<BulkOptions>, ApiError> {
    let default = LotSpec::default();
    let presets = Preset::ALL
        .iter()
        .map(|p| {
            let mut spec = default.clone();
            p.apply(&mut spec);
            PresetView {
                label: p.label().to_string(),
                spec: SpecView::from(&spec),
            }
        })
        .collect();
    let (source_info, languages) = match source(&state).await? {
        None => (None, Vec::new()),
        Some(_) => {
            let (source, candidates) = current(&state).await?;
            let mut counts: std::collections::BTreeMap<String, i64> = Default::default();
            for c in candidates.iter() {
                *counts.entry(c.language.clone()).or_default() += c.available + c.reserved;
            }
            let mut languages: Vec<LanguageCount> = counts
                .into_iter()
                .map(|(language, copies)| LanguageCount { language, copies })
                .collect();
            languages.sort_by(|a, b| {
                b.copies
                    .cmp(&a.copies)
                    .then_with(|| a.language.cmp(&b.language))
            });
            (Some(SourceInfo::from(source.as_ref())), languages)
        }
    };
    Ok(Json(BulkOptions {
        source: source_info,
        default_spec: SpecView::from(&default),
        presets,
        conditions: CONDITIONS.iter().map(|c| c.to_string()).collect(),
        languages,
        max_lot_price: MAX_LOT_PRICE_EUR,
        max_name_chars: MAX_LOT_NAME_CHARS as i64,
    }))
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SpecRequest {
    pub spec: SpecView,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BulkPreview {
    pub source: SourceInfo,
    /// The lot size this preview was made for.
    pub size: i64,
    pub funnel: FunnelView,
    pub lines: Vec<LotLine>,
    pub summary: LotSummaryView,
    pub default_name: String,
    /// Printings whose card type still has to be looked up for this spec.
    pub types_needed: i64,
    pub types: JobStatus,
}

async fn preview_lot(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<SpecRequest>,
) -> Result<Json<BulkPreview>, ApiError> {
    let spec = spec_of(&request.spec)?;
    let (source, candidates) = current(&state).await?;
    let bulk = state.bulk.clone();
    let view = blocking(move || {
        let needed = bulk.with_types(|t| lookups(&source, &candidates, &spec, t, today()));
        let (funnel, picks, summary) = preview(&candidates, &spec);
        BulkPreview {
            source: SourceInfo::from(source.as_ref()),
            size: spec.size as i64,
            funnel: FunnelView {
                bottleneck: funnel.bottleneck(spec.size).map(|s| s.label.clone()),
                steps: funnel
                    .steps
                    .iter()
                    .map(|s| FunnelStepView {
                        label: s.label.clone(),
                        copies: s.copies,
                    })
                    .collect(),
                reserved: funnel.reserved,
                types_unknown: funnel.types_unknown,
                hit_pool: funnel.hit_pool,
            },
            lines: picks
                .iter()
                .map(|p| line(&source, &candidates[p.candidate], p.copies, p.hit, false))
                .collect(),
            summary: summary.into(),
            default_name: default_lot_name(&spec),
            types_needed: needed.len() as i64,
            types: bulk.status(),
        }
    })
    .await?;
    Ok(Json(view))
}

async fn load_types(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<SpecRequest>,
) -> Result<(StatusCode, Json<JobStatus>), ApiError> {
    let spec = spec_of(&request.spec)?;
    let (source, candidates) = current(&state).await?;
    let bulk = state.bulk.clone();
    let needed = blocking({
        let bulk = bulk.clone();
        move || bulk.with_types(|t| lookups(&source, &candidates, &spec, t, today()))
    })
    .await?;
    if !bulk.start_lookup(needed, today()) {
        return Err(ApiError::conflict(
            "running",
            "Card types are already being looked up",
        ));
    }
    Ok((StatusCode::ACCEPTED, Json(bulk.status())))
}

async fn types_status(State(state): State<AppState>) -> Json<JobStatus> {
    Json(state.bulk.status())
}

// ── Draft ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DraftView {
    pub spec: SpecView,
    pub default_name: String,
    /// Lines in picking order; a line whose pile is gone is left out and
    /// counted in `gone`.
    pub lines: Vec<LotLine>,
    pub gone: i64,
    pub summary: LotSummaryView,
    /// RFC 3339.
    pub updated_at: String,
}

fn draft_view(
    draft: &Draft,
    source: &Source,
    candidates: &[Candidate],
) -> Result<DraftView, ApiError> {
    let spec = spec_of(&draft.spec)?;
    let index = bulk::candidate_index(source, candidates);
    let mut lines = Vec::new();
    let mut picks = Vec::new();
    let mut gone = 0;
    for p in &draft.picks {
        match index.get(&p.pile) {
            Some(&i) => {
                lines.push(line(source, &candidates[i], p.copies, p.hit, p.picked));
                picks.push(check_stock_core::bulk_lot::LotPick {
                    candidate: i,
                    copies: p.copies,
                    hit: p.hit,
                });
            }
            None => gone += 1,
        }
    }
    let summary = check_stock_core::bulk_lot::summarize(candidates, &picks, spec.hit_min_price);
    Ok(DraftView {
        spec: draft.spec.clone(),
        default_name: default_lot_name(&spec),
        lines,
        gone,
        summary: summary.into(),
        updated_at: draft.updated_at.clone(),
    })
}

async fn get_draft(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<Option<DraftView>>, ApiError> {
    let id = user.id;
    let Some(draft) = state.db.read(move |conn| load_draft(conn, id)).await? else {
        return Ok(Json(None));
    };
    let (source, candidates) = current(&state).await?;
    Ok(Json(Some(
        blocking(move || draft_view(&draft, &source, &candidates)).await??,
    )))
}

async fn make_draft(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<SpecRequest>,
) -> Result<Json<DraftView>, ApiError> {
    let spec = spec_of(&request.spec)?;
    let (source, candidates) = current(&state).await?;
    let (draft, view) = blocking(move || {
        let (_, picks, _) = preview(&candidates, &spec);
        let draft = Draft {
            spec: request.spec,
            picks: draft_picks(&source, &candidates, &picks),
            excluded: Vec::new(),
            updated_at: crate::migrations::now_utc(),
        };
        let view = draft_view(&draft, &source, &candidates);
        (draft, view)
    })
    .await?;
    let view = view?;
    if draft.picks.is_empty() {
        return Err(invalid("no_cards", "No card matches these rules"));
    }
    let id = user.id;
    state
        .db
        .write(move |conn| save_draft(conn, id, &draft))
        .await?;
    Ok(Json(view))
}

async fn drop_draft(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<StatusCode, ApiError> {
    let id = user.id;
    if state.db.write(move |conn| delete_draft(conn, id)).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "no_draft",
            "There is no bulk lot in progress",
        ))
    }
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct DraftPickedRequest {
    pub piles: Vec<String>,
    pub picked: bool,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DraftPickedResult {
    pub picked: i64,
}

async fn set_picked(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<DraftPickedRequest>,
) -> Result<Json<DraftPickedResult>, ApiError> {
    if request.piles.is_empty()
        || request.piles.len() > 10_000
        || request.piles.iter().any(|p| p.len() > 24)
    {
        return Err(invalid("invalid_piles", "Mark 1 to 10,000 cards"));
    }
    let id = user.id;
    let picked = state
        .db
        .write(move |conn| {
            let tx = conn.transaction()?;
            let Some(mut draft) = load_draft(&tx, id)? else {
                return Ok(None);
            };
            if request
                .piles
                .iter()
                .any(|p| !draft.picks.iter().any(|d| &d.pile == p))
            {
                return Ok(None);
            }
            for p in &mut draft.picks {
                if request.piles.contains(&p.pile) {
                    p.picked = request.picked;
                }
            }
            draft.updated_at = crate::migrations::now_utc();
            save_draft(&tx, id, &draft)?;
            tx.commit()?;
            Ok(Some(draft.picks.iter().filter(|p| p.picked).count()))
        })
        .await?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "not_in_draft",
                "That card is not in your lot",
            )
        })?;
    Ok(Json(DraftPickedResult {
        picked: picked as i64,
    }))
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ReplaceRequest {
    pub pile: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReplaceResult {
    pub draft: DraftView,
    /// Copies that came in for the swapped-out pile.
    pub added: i64,
}

async fn replace_pile(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<ReplaceRequest>,
) -> Result<Json<ReplaceResult>, ApiError> {
    if request.pile.len() > 24 {
        return Err(invalid("invalid_pile", "Not a card of the lot"));
    }
    let id = user.id;
    let Some(mut draft) = state.db.read(move |conn| load_draft(conn, id)).await? else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "no_draft",
            "There is no bulk lot in progress",
        ));
    };
    if !draft.picks.iter().any(|p| p.pile == request.pile) {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "not_in_draft",
            "That card is not in your lot",
        ));
    }
    let (source, candidates) = current(&state).await?;
    let (draft, added, view) = blocking(move || {
        let spec = spec_of(&draft.spec)?;
        let added = replace(&mut draft, &request.pile, &source, &candidates, &spec);
        draft.updated_at = crate::migrations::now_utc();
        let view = draft_view(&draft, &source, &candidates)?;
        Ok::<_, ApiError>((draft, added, view))
    })
    .await??;
    state
        .db
        .write(move |conn| save_draft(conn, id, &draft))
        .await?;
    Ok(Json(ReplaceResult { draft: view, added }))
}

// ── Finish ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct FinishRequest {
    pub name: String,
    /// What the lot sells for (euros).
    pub price: f64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FinishResult {
    pub id: i64,
    pub name: String,
    pub copies: i64,
    pub price: f64,
    pub data_version: i64,
}

enum FinishRefusal {
    NoDraft,
    NothingPicked,
    StockChanged,
    Taken(usize),
    BackupFailed,
}

fn check_name(name: &str) -> Result<String, ApiError> {
    let name = name.trim();
    if name.is_empty()
        || name.chars().count() > MAX_LOT_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(invalid(
            "invalid_name",
            "The lot needs a name of 1 to 120 characters",
        ));
    }
    Ok(name.to_string())
}

fn check_price(price: f64) -> Result<(), ApiError> {
    if price.is_finite() && (0.0..=MAX_LOT_PRICE_EUR).contains(&price) {
        Ok(())
    } else {
        Err(invalid(
            "invalid_price",
            "The lot price must be 0 to 100,000 €",
        ))
    }
}

async fn finish_lot(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<FinishRequest>,
) -> Result<Json<FinishResult>, ApiError> {
    let name = check_name(&request.name)?;
    check_price(request.price)?;
    let price = request.price;
    let source = required_source(&state).await?;
    let db_path = state.db.path().to_path_buf();
    let (id, username) = (user.id, user.username.clone());
    let lot_name = name.clone();
    let outcome = state
        .db
        .write(move |conn| {
            let Some(draft) = load_draft(conn, id)? else {
                return Ok(Err(FinishRefusal::NoDraft));
            };
            if !draft.picks.iter().any(|p| p.picked) {
                return Ok(Err(FinishRefusal::NothingPicked));
            }
            // The stock seen must still be the stock (writer: nothing else
            // writes meanwhile).
            let pending_seq = crate::discards::pending(conn, CATEGORY)?
                .iter()
                .map(|(i, _)| *i)
                .max()
                .unwrap_or(0);
            if kept_file_info(conn)?.map(|v| v.0) != Some(source.version)
                || pending_seq != source.discard_seq
            {
                return Ok(Err(FinishRefusal::StockChanged));
            }
            let today = today();
            let cands = candidates(conn, &source, None, today)?;
            let short = shortfalls(&draft, &source, &cands);
            if !short.is_empty() {
                return Ok(Err(FinishRefusal::Taken(short.len())));
            }
            let index = bulk::candidate_index(&source, &cands);
            let lines: Vec<(check_stock_core::models::Card, i64)> = draft
                .picks
                .iter()
                .filter(|p| p.picked)
                .map(|p| (source.cards[cands[index[&p.pile]].source].clone(), p.copies))
                .collect();
            let backup = match backup(conn, &db_path) {
                Ok(path) => path,
                Err(e) => {
                    log::error!("backup before finishing a bulk lot failed: {e}");
                    return Ok(Err(FinishRefusal::BackupFailed));
                }
            };
            let day = today.format("%Y-%m-%d").to_string();
            let now = Utc::now();
            let tx = conn.transaction()?;
            let lot_id = record_bulk_lot_in_tx(&tx, &lot_name, price, &lines, CATEGORY, &day)?;
            bulk::keep_csv(
                &tx,
                lot_id,
                &export_file_name(&lot_name, &day),
                &powertools_csv(&lines),
            )?;
            data_changed(&tx, "bulk_lot.finished", now)?;
            let copies: i64 = lines.iter().map(|(_, c)| c).sum();
            audit::record(
                &tx,
                now,
                Actor::User {
                    id,
                    username: &username,
                },
                action::BULK_LOT_FINISHED,
                Some(&lot_id.to_string()),
                serde_json::json!({
                    "name": lot_name,
                    "price": price,
                    "copies": copies,
                    "backup": backup.file_name().map(|n| n.to_string_lossy().to_string()),
                    "cards": lines.iter().map(|(c, n)| serde_json::json!({
                        "name": c.name,
                        "set": c.set_code,
                        "location": c.location,
                        "copies": n,
                    })).collect::<Vec<_>>(),
                }),
            )?;
            delete_draft(&tx, id)?;
            tx.commit()?;
            Ok(Ok((lot_id, copies, latest_version(conn)?.0)))
        })
        .await?;
    match outcome {
        Ok((lot_id, copies, data_version)) => Ok(Json(FinishResult {
            id: lot_id,
            name,
            copies,
            price,
            data_version,
        })),
        Err(FinishRefusal::NoDraft) => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "no_draft",
            "There is no bulk lot in progress",
        )),
        Err(FinishRefusal::NothingPicked) => Err(invalid(
            "nothing_picked",
            "Mark the cards you picked first: only those go into the lot",
        )),
        Err(FinishRefusal::StockChanged) => Err(stock_changed()),
        Err(FinishRefusal::Taken(n)) => Err(ApiError::conflict(
            "taken",
            format!(
                "{n} picked {} no longer free (another lot or a discard took the copies); replace {}",
                if n == 1 { "card is" } else { "cards are" },
                if n == 1 { "it" } else { "them" }
            ),
        )),
        Err(FinishRefusal::BackupFailed) => Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "backup_failed",
            "The backup before recording the lot failed, so nothing was recorded. Try again; if it keeps failing, the server's disk may be full",
        )),
    }
}

async fn lot_csv(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Response, ApiError> {
    let (file_name, csv) = state
        .db
        .read(move |conn| bulk::csv_of(conn, id))
        .await?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "This lot's CSV is not kept (lots from the desktop app, or after a restore)",
            )
        })?;
    // The kept name comes from `export_file_name` (letters, digits, `_`, `-`).
    let safe: String = file_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        .collect();
    let disposition = format!("attachment; filename=\"{safe}\"");
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/csv; charset=utf-8"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&disposition).map_err(|_| ApiError::internal())?,
            ),
        ],
        csv,
    )
        .into_response())
}

// ── History and management ─────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LotStatus {
    /// Its copies wait for an import that sees them gone.
    AwaitingImport,
    /// … for more than 7 days.
    Overdue,
    Done,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BulkLotView {
    pub id: i64,
    pub name: String,
    /// `YYYY-MM-DD`.
    pub created_at: String,
    pub copies: i64,
    pub pending_copies: i64,
    pub status: LotStatus,
    /// What it sold for and the copies' listed value (only with the finish
    /// or manage permission).
    pub price: Option<f64>,
    pub listed_value: Option<f64>,
    pub csv_available: bool,
}

fn status_of(lot: &BulkLotSummary, today: NaiveDate) -> LotStatus {
    if lot.pending_copies <= 0 {
        return LotStatus::Done;
    }
    let age = NaiveDate::parse_from_str(&lot.created_at, "%Y-%m-%d")
        .map(|d| (today - d).num_days())
        .unwrap_or(0);
    if age > bulk::OVERDUE_DAYS {
        LotStatus::Overdue
    } else {
        LotStatus::AwaitingImport
    }
}

async fn history(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<Vec<BulkLotView>>, ApiError> {
    let prices = user.permissions.contains(Permission::BulkLotFinish)
        || user.permissions.contains(Permission::BulkLotManage);
    let lots = state
        .db
        .read(move |conn| {
            list_bulk_lots_conn(conn)?
                .into_iter()
                .map(|l| {
                    Ok(BulkLotView {
                        id: l.id,
                        status: status_of(&l, today()),
                        csv_available: bulk::has_csv(conn, l.id)?,
                        price: prices.then_some(l.price),
                        listed_value: prices.then_some(l.listed_value),
                        name: l.name,
                        created_at: l.created_at,
                        copies: l.copies,
                        pending_copies: l.pending_copies,
                    })
                })
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .await?;
    Ok(Json(lots))
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BulkLotCardView {
    pub name: String,
    pub set_name: String,
    pub rarity: String,
    pub condition: String,
    pub language: String,
    pub foil: bool,
    pub location: String,
    pub purchase_lot: String,
    pub copies: i64,
    pub pending_copies: i64,
    pub listed_price: f64,
}

async fn lot_cards(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<BulkLotCardView>>, ApiError> {
    let cards = state
        .db
        .read(move |conn| {
            if !list_bulk_lots_conn(conn)?.iter().any(|l| l.id == id) {
                return Ok(None);
            }
            Ok(Some(bulk_lot_cards_conn(conn, id)?))
        })
        .await?
        .ok_or_else(not_found)?;
    Ok(Json(
        cards
            .into_iter()
            .map(|c| BulkLotCardView {
                name: c.name,
                set_name: c.set_name,
                rarity: c.rarity,
                condition: c.condition,
                language: c.language,
                foil: c.foil,
                location: c.location,
                purchase_lot: c.purchase_lot,
                copies: c.copies,
                pending_copies: c.pending_copies,
                listed_price: c.listed_price,
            })
            .collect(),
    ))
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct LotPriceRequest {
    pub price: f64,
    /// The price the page showed.
    pub expected: f64,
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, optional_fields)]
pub struct UndoRequest {
    /// When an import already took copies out: their number, typed in.
    pub confirm_removed: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ManageResult {
    pub data_version: i64,
}

enum ManageRefusal {
    NotFound,
    Changed(f64),
    NothingPending,
    NeedsConfirmation(i64),
    WrongConfirmation(i64),
    BackupFailed,
}

fn manage_error(e: ManageRefusal) -> ApiError {
    match e {
        ManageRefusal::NotFound => not_found(),
        ManageRefusal::Changed(now) => ApiError::conflict(
            "changed",
            format!("Someone changed this lot's price meanwhile (now {now:.2} €); check and save again"),
        ),
        ManageRefusal::NothingPending => ApiError::conflict(
            "nothing_pending",
            "This lot has no copies waiting for an import",
        ),
        ManageRefusal::NeedsConfirmation(n) => invalid(
            "confirmation_needed",
            format!(
                "An import already took {n} copies of this lot out. Undoing brings them back as stock, and when they disappear again they count as sold. Type {n} to undo anyway"
            ),
        ),
        ManageRefusal::WrongConfirmation(n) => invalid(
            "confirmation_wrong",
            format!("That is not the number of copies an import already took out ({n})"),
        ),
        ManageRefusal::BackupFailed => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "backup_failed",
            "The backup failed, so nothing was changed. Try again; if it keeps failing, the server's disk may be full",
        ),
    }
}

fn find_lot(conn: &rusqlite::Connection, id: i64) -> rusqlite::Result<Option<BulkLotSummary>> {
    Ok(list_bulk_lots_conn(conn)?.into_iter().find(|l| l.id == id))
}

async fn manage(
    state: &AppState,
    user: &CurrentUser,
    lot_id: i64,
    op: impl FnOnce(
            &mut rusqlite::Connection,
            &std::path::Path,
            Actor<'_>,
            chrono::DateTime<Utc>,
        ) -> rusqlite::Result<Result<(), ManageRefusal>>
        + Send
        + 'static,
) -> Result<Json<ManageResult>, ApiError> {
    let db_path = state.db.path().to_path_buf();
    let (id, username) = (user.id, user.username.clone());
    let _ = lot_id;
    let outcome = state
        .db
        .write(move |conn| {
            let actor = Actor::User {
                id,
                username: &username,
            };
            match op(conn, &db_path, actor, Utc::now())? {
                Ok(()) => Ok(Ok(latest_version(conn)?.0)),
                Err(e) => Ok(Err(e)),
            }
        })
        .await?;
    let data_version = outcome.map_err(manage_error)?;
    Ok(Json(ManageResult { data_version }))
}

async fn set_price(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(lot_id): Path<i64>,
    ApiJson(request): ApiJson<LotPriceRequest>,
) -> Result<Json<ManageResult>, ApiError> {
    check_price(request.price)?;
    manage(&state, &user, lot_id, move |conn, _, actor, now| {
        let Some(lot) = find_lot(conn, lot_id)? else {
            return Ok(Err(ManageRefusal::NotFound));
        };
        if (lot.price - request.expected).abs() > 1e-9 {
            return Ok(Err(ManageRefusal::Changed(lot.price)));
        }
        let tx = conn.transaction()?;
        set_bulk_lot_price_conn(&tx, lot_id, request.price)?;
        data_changed(&tx, "bulk_lot.price", now)?;
        audit::record(
            &tx,
            now,
            actor,
            action::BULK_LOT_PRICE_CHANGED,
            Some(&lot_id.to_string()),
            serde_json::json!({ "name": lot.name, "before": lot.price, "after": request.price }),
        )?;
        tx.commit()?;
        Ok(Ok(()))
    })
    .await
}

fn backup_or_refuse(
    conn: &rusqlite::Connection,
    db_path: &std::path::Path,
) -> Result<String, ManageRefusal> {
    backup(conn, db_path)
        .map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        })
        .map_err(|e| {
            log::error!("backup before a bulk-lot change failed: {e}");
            ManageRefusal::BackupFailed
        })
}

async fn mark_removed(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(lot_id): Path<i64>,
) -> Result<Json<ManageResult>, ApiError> {
    manage(&state, &user, lot_id, move |conn, db_path, actor, now| {
        let Some(lot) = find_lot(conn, lot_id)? else {
            return Ok(Err(ManageRefusal::NotFound));
        };
        if lot.pending_copies <= 0 {
            return Ok(Err(ManageRefusal::NothingPending));
        }
        let backup = match backup_or_refuse(conn, db_path) {
            Ok(b) => b,
            Err(e) => return Ok(Err(e)),
        };
        let tx = conn.transaction()?;
        mark_bulk_lot_removed_conn(&tx, lot_id)?;
        data_changed(&tx, "bulk_lot.removed", now)?;
        audit::record(
            &tx,
            now,
            actor,
            action::BULK_LOT_MARKED_REMOVED,
            Some(&lot_id.to_string()),
            serde_json::json!({ "name": lot.name, "pending_copies": lot.pending_copies, "backup": backup }),
        )?;
        tx.commit()?;
        Ok(Ok(()))
    })
    .await
}

async fn undo(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(lot_id): Path<i64>,
    ApiJson(request): ApiJson<UndoRequest>,
) -> Result<Json<ManageResult>, ApiError> {
    if request
        .confirm_removed
        .as_ref()
        .is_some_and(|t| t.chars().count() > 32)
    {
        return Err(invalid(
            "confirmation_wrong",
            "That is not a number of copies",
        ));
    }
    manage(&state, &user, lot_id, move |conn, db_path, actor, now| {
        let Some(lot) = find_lot(conn, lot_id)? else {
            return Ok(Err(ManageRefusal::NotFound));
        };
        let removed = lot.copies - lot.pending_copies;
        if removed > 0 {
            match request.confirm_removed.as_deref() {
                None => return Ok(Err(ManageRefusal::NeedsConfirmation(removed))),
                Some(t) if crate::inventory_import::parse_typed_count(t) != Some(removed) => {
                    return Ok(Err(ManageRefusal::WrongConfirmation(removed)))
                }
                Some(_) => {}
            }
        }
        let backup = match backup_or_refuse(conn, db_path) {
            Ok(b) => b,
            Err(e) => return Ok(Err(e)),
        };
        let tx = conn.transaction()?;
        delete_bulk_lot_in_tx(&tx, lot_id)?;
        bulk::forget_csv(&tx, lot_id)?;
        data_changed(&tx, "bulk_lot.undone", now)?;
        audit::record(
            &tx,
            now,
            actor,
            action::BULK_LOT_UNDONE,
            Some(&lot_id.to_string()),
            serde_json::json!({
                "name": lot.name,
                "price": lot.price,
                "copies": lot.copies,
                "pending_copies": lot.pending_copies,
                "removed_copies": removed,
                "backup": backup,
            }),
        )?;
        tx.commit()?;
        Ok(Ok(()))
    })
    .await
}

#[cfg(test)]
#[path = "bulk_lots_tests.rs"]
mod tests;
