//! Repricing (rules, storage and stock preparation in [`crate::repricing`]).
//!
//! View (`repricing.view`): `GET /api/repricing/setup`,
//! `GET /api/repricing/review`, `GET /api/repricing/listings/{pile}`,
//! `GET /api/repricing/ruleset.json`, the market data
//! (`POST /api/repricing/market/load`, `GET /api/repricing/market/status`).
//! Edit (`repricing.edit_rules`): the user's draft (`PUT`/`DELETE
//! /api/repricing/draft`, `POST /api/repricing/draft/file`) and
//! `POST /api/repricing/publish`.
//! Export (`repricing.export`): the user's review
//! (`PUT /api/repricing/review/ticks`, `PUT /api/repricing/review/manual`,
//! `POST /api/repricing/review/reset`), `POST /api/repricing/export` and the
//! kept exports (`GET /api/repricing/exports`, `…/exports/{id}/csv`,
//! `…/exports/{id}/undo`).
//!
//! The browser never sends card data: rows are built from the prepared
//! stock, ticks and hand prices name pile ids, and an export is computed
//! from the published ruleset, never from a draft.

use super::bins::SourceInfo;
use super::movers::{market_status, start_market_load, MarketLoadRequest, MarketStatusView};
use super::{Access, Api, ApiJson, ApiQuery, CurrentUser};
use crate::app::AppState;
use crate::audit::{self, action};
use crate::http::ApiError;
use crate::inventory_import::latest_version;
use crate::permissions::Permission;
use crate::repricing::{
    self, clear_review, delete_draft, export_file, exports, is_ticked, keep_export,
    parse_ruleset_file, publish_in_tx, save_draft, set_manual, set_ticks, Draft, NewExport,
    Prepared, PriceExportSummary, Published, ReviewFilter, ReviewSort, ReviewView,
    MAX_RULESET_BYTES,
};
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::Json;
use check_stock_core::api::inventory_sync::{PriceField, PriceFields};
use check_stock_core::formatters::format_price_diff_csv;
use check_stock_core::repricing::{
    cents, explain, export_file_name, rule_templates, ticked_changes, undo_file_name, Combine,
    Condition, Direction, Evaluation, Finish, MarketMove, Outcome, Rounding, Rule, Ruleset,
    CARDMARKET_MIN_PRICE, CONDITIONS, MAX_PRICE_EUR, SALES_WINDOW_DAYS,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use ts_rs::TS;

/// Body limit of the routes that take a ruleset (JSON of a file is escaped
/// once more, so twice the ruleset cap; the size is checked after parsing).
const RULESET_BODY_LIMIT: usize = 2 * MAX_RULESET_BYTES;
/// Rows per review page at most.
const MAX_PAGE: u32 = 500;
/// Piles per tick request at most (an import has at most this many rows).
const MAX_TICK_PILES: usize = crate::inventory_import::MAX_IMPORT_ROWS;
/// Sales listed in the detail panel at most.
const MAX_SALES_SHOWN: usize = 30;

pub(super) fn routes(api: Api) -> Api {
    let view = Access::Permission(Permission::RepricingView);
    let edit = Access::Permission(Permission::RepricingEditRules);
    let export = Access::Permission(Permission::RepricingExport);
    api.route("/api/repricing/setup", view, get(setup))
        .route("/api/repricing/review", view, get(review))
        .route("/api/repricing/listings/{pile}", view, get(listing))
        .route("/api/repricing/ruleset.json", view, get(ruleset_file))
        .route("/api/repricing/market/load", view, post(load_market))
        .route("/api/repricing/market/status", view, get(market))
        .route(
            "/api/repricing/draft",
            edit,
            put(put_draft)
                .delete(drop_draft)
                .layer(DefaultBodyLimit::max(RULESET_BODY_LIMIT)),
        )
        .route(
            "/api/repricing/draft/file",
            edit,
            post(open_file).layer(DefaultBodyLimit::max(RULESET_BODY_LIMIT)),
        )
        .route("/api/repricing/publish", edit, post(publish))
        .route("/api/repricing/review/ticks", export, put(put_ticks))
        .route("/api/repricing/review/manual", export, put(put_manual))
        .route("/api/repricing/review/reset", export, post(reset_review))
        .route("/api/repricing/export", export, post(make_export))
        .route("/api/repricing/exports", export, get(list_exports))
        .route("/api/repricing/exports/{id}/csv", export, get(export_csv))
        .route("/api/repricing/exports/{id}/undo", export, get(export_undo))
}

fn invalid(code: &'static str, message: impl Into<std::borrow::Cow<'static, str>>) -> ApiError {
    ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, code, message)
}

fn no_import() -> ApiError {
    ApiError::conflict(
        "no_import",
        "Repricing needs an inventory import on the server (Tools → Inventory import)",
    )
}

fn stock_changed() -> ApiError {
    ApiError::conflict(
        "stock_changed",
        "The stock changed meanwhile (an import, a discard or a bulk lot); check the prices again",
    )
}

async fn prepared(state: &AppState) -> Result<Option<Arc<Prepared>>, ApiError> {
    Ok(state
        .repricing
        .prepared(&state.db, &state.stock, &state.market)
        .await?)
}

async fn required(state: &AppState) -> Result<Arc<Prepared>, ApiError> {
    prepared(state).await?.ok_or_else(no_import)
}

/// The ruleset a user looks at: their draft (editors, when asked and one
/// exists) or the published one. `true` = the draft.
async fn rules_for(
    state: &AppState,
    user: &CurrentUser,
    want_draft: bool,
) -> Result<(Published, Option<Draft>), ApiError> {
    let id = user.id;
    let editor = user.permissions.contains(Permission::RepricingEditRules);
    Ok(state
        .db
        .read(move |conn| {
            let published = repricing::published(conn)?;
            let draft = if want_draft && editor {
                repricing::draft(conn, id)?
            } else {
                None
            };
            Ok((published, draft))
        })
        .await?)
}

// ── Setup ──────────────────────────────────────────────────────────────────

/// The published ruleset.
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PublishedView {
    /// 0 = never published (the starter rules).
    pub revision: i64,
    pub ruleset: Ruleset,
    pub updated_at: Option<String>,
    pub updated_by: Option<String>,
}

impl From<Published> for PublishedView {
    fn from(p: Published) -> Self {
        Self {
            revision: p.revision,
            ruleset: p.ruleset,
            updated_at: p.updated_at,
            updated_by: p.updated_by,
        }
    }
}

/// An editor's unpublished ruleset.
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RulesetDraftView {
    pub base_revision: i64,
    pub ruleset: Ruleset,
    pub updated_at: String,
}

impl From<Draft> for RulesetDraftView {
    fn from(d: Draft) -> Self {
        Self {
            base_revision: d.base_revision,
            ruleset: d.ruleset,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FieldChoice {
    #[ts(as = "check_stock_core::repricing::PriceFieldTs")]
    pub key: PriceField,
    pub label: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CombineChoice {
    pub key: Combine,
    pub label: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DirectionChoice {
    pub key: Direction,
    pub label: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RoundingChoice {
    pub key: Rounding,
    pub label: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MoveChoice {
    pub key: MarketMove,
    pub label: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FinishChoice {
    pub key: Finish,
    pub label: String,
}

/// A condition offered in the "add condition" menu.
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ConditionTemplate {
    pub label: String,
    pub help: String,
    pub condition: Condition,
}

/// A ready-made rule.
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RuleTemplate {
    pub hint: String,
    pub rule: Rule,
}

/// What the rule editor offers.
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RepricingOptions {
    /// Values in the stock first (most copies first), then common ones.
    pub rarities: Vec<String>,
    pub languages: Vec<String>,
    pub conditions: Vec<String>,
    pub fields: Vec<FieldChoice>,
    pub combines: Vec<CombineChoice>,
    pub directions: Vec<DirectionChoice>,
    pub roundings: Vec<RoundingChoice>,
    pub moves: Vec<MoveChoice>,
    pub finishes: Vec<FinishChoice>,
    pub condition_templates: Vec<ConditionTemplate>,
    pub rule_templates: Vec<RuleTemplate>,
    pub sales_window_days: u32,
    pub min_price: f64,
    pub max_price: f64,
    pub max_ruleset_bytes: u32,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RepricingSetup {
    /// `None` before the first inventory import on the server.
    pub source: Option<SourceInfo>,
    pub published: PublishedView,
    /// The user's unpublished changes (editors only).
    pub draft: Option<RulesetDraftView>,
    pub options: RepricingOptions,
}

fn options(prepared: Option<&Prepared>) -> RepricingOptions {
    let (rarities, languages) = match prepared {
        Some(p) => (p.rarities.clone(), p.languages.clone()),
        None => (
            ["Common", "Uncommon", "Rare", "Mythic"]
                .map(String::from)
                .to_vec(),
            ["English", "German"].map(String::from).to_vec(),
        ),
    };
    RepricingOptions {
        rarities,
        languages,
        conditions: CONDITIONS.map(String::from).to_vec(),
        fields: PriceField::all()
            .iter()
            .map(|&key| FieldChoice {
                key,
                label: key.as_str().to_string(),
            })
            .collect(),
        combines: Combine::ALL
            .iter()
            .map(|&key| CombineChoice {
                key,
                label: key.label().to_string(),
            })
            .collect(),
        directions: Direction::ALL
            .iter()
            .map(|&key| DirectionChoice {
                key,
                label: key.label().to_string(),
            })
            .collect(),
        roundings: Rounding::ALL
            .iter()
            .map(|&key| RoundingChoice {
                key,
                label: key.label().to_string(),
            })
            .collect(),
        moves: MarketMove::ALL
            .iter()
            .map(|&key| MoveChoice {
                key,
                label: key.label().to_string(),
            })
            .collect(),
        finishes: Finish::ALL
            .iter()
            .map(|&key| FinishChoice {
                key,
                label: key.label().to_string(),
            })
            .collect(),
        condition_templates: Condition::templates()
            .into_iter()
            .map(|condition| ConditionTemplate {
                label: condition.kind_label().to_string(),
                help: condition.help().to_string(),
                condition,
            })
            .collect(),
        rule_templates: rule_templates()
            .into_iter()
            .map(|t| RuleTemplate {
                hint: t.hint.to_string(),
                rule: t.rule,
            })
            .collect(),
        sales_window_days: SALES_WINDOW_DAYS as u32,
        min_price: CARDMARKET_MIN_PRICE,
        max_price: MAX_PRICE_EUR,
        max_ruleset_bytes: MAX_RULESET_BYTES as u32,
    }
}

async fn setup(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<RepricingSetup>, ApiError> {
    let prepared = prepared(&state).await?;
    let (published, draft) = rules_for(&state, &user, true).await?;
    Ok(Json(RepricingSetup {
        source: prepared
            .as_ref()
            .map(|p| SourceInfo::from(p.source.as_ref())),
        published: published.into(),
        draft: draft.map(RulesetDraftView::from),
        options: options(prepared.as_deref()),
    }))
}

// ── Review ─────────────────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, optional_fields)]
pub struct ReviewQuery {
    /// Show the user's draft instead of the published rules (editors).
    pub draft: Option<bool>,
    pub filter: Option<ReviewFilter>,
    /// Only listings priced by this rule (index).
    pub rule: Option<u32>,
    pub search: Option<String>,
    pub sort: Option<ReviewSort>,
    pub desc: Option<bool>,
    pub offset: Option<u32>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum OutcomeKey {
    Raise,
    Lower,
    NoRule,
    Kept,
    NoMarketData,
    StaleMarket,
    NoPrice,
    Held,
    SmallChange,
    Same,
}

impl From<Outcome> for OutcomeKey {
    fn from(o: Outcome) -> Self {
        match o {
            Outcome::Raise => OutcomeKey::Raise,
            Outcome::Lower => OutcomeKey::Lower,
            Outcome::NoRule => OutcomeKey::NoRule,
            Outcome::Kept => OutcomeKey::Kept,
            Outcome::NoMarketData => OutcomeKey::NoMarketData,
            Outcome::StaleMarket => OutcomeKey::StaleMarket,
            Outcome::NoPrice => OutcomeKey::NoPrice,
            Outcome::Held => OutcomeKey::Held,
            Outcome::SmallChange => OutcomeKey::SmallChange,
            Outcome::Same => OutcomeKey::Same,
        }
    }
}

/// One listing of the review.
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct ReviewRow {
    pub pile: String,
    pub name: String,
    pub set_code: String,
    pub set_name: String,
    pub collector_number: String,
    pub rarity: String,
    pub condition: String,
    pub language: String,
    pub foil: bool,
    pub quantity: i64,
    pub location: String,
    pub old_price: f64,
    /// Equals `old_price` unless the outcome is a raise or a drop.
    pub new_price: f64,
    /// Market reference price.
    pub market: Option<f64>,
    pub outcome: OutcomeKey,
    pub outcome_label: String,
    /// Index of the rule that priced it.
    pub rule: Option<u32>,
    /// Priced by hand.
    pub manual: bool,
    /// A big change that starts unticked.
    pub review: bool,
    /// Goes into the export.
    pub ticked: bool,
    /// The user's tick, when set by hand.
    pub overridden: Option<bool>,
    /// Copies of this card are reserved for a bulk lot awaiting its import:
    /// never exported.
    pub held_back: bool,
    pub delta_pct: f64,
}

fn row_of(view: &ReviewView<'_>, i: usize) -> ReviewRow {
    let prep = view.prepared;
    let l = &prep.listings[i];
    let p = &view.eval.proposals[i];
    let card = &prep.source.cards[l.row];
    ReviewRow {
        pile: prep.piles[i].clone(),
        name: l.name.clone(),
        set_code: l.set_code.clone(),
        set_name: l.set.clone(),
        collector_number: card.cn.clone(),
        rarity: l.rarity.clone(),
        condition: l.condition.clone(),
        language: l.language.clone(),
        foil: l.is_foil,
        quantity: l.quantity,
        location: l.location.clone(),
        old_price: p.old_price,
        new_price: p.new_price,
        market: p.market,
        outcome: p.outcome.into(),
        outcome_label: p.outcome.label().to_string(),
        rule: p.rule.map(|r| r as u32),
        manual: p.manual,
        review: p.review,
        ticked: view.ticked(i),
        overridden: view.overridden(i),
        held_back: prep.held_back[i],
        delta_pct: if p.outcome.is_change() {
            p.delta_pct()
        } else {
            0.0
        },
    }
}

/// A rule with what it did.
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct RuleView {
    pub name: String,
    pub enabled: bool,
    pub when: String,
    pub then: String,
    /// Listings it priced (first match).
    pub matched: u32,
    pub raised: u32,
    pub lowered: u32,
    /// Σ (new − old) × copies over its changes (listed prices).
    pub value_delta: f64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct SkippedCount {
    pub label: String,
    pub count: u32,
}

/// Totals of the review (listed prices, not revenue).
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct ReviewSummary {
    pub listings: u32,
    pub copies: i64,
    pub raises: u32,
    pub lowers: u32,
    /// Big changes waiting for a tick.
    pub review: u32,
    /// Changes going into the export, and their copies.
    pub selected: u32,
    pub selected_copies: i64,
    /// Σ current price × copies.
    pub value_before: f64,
    /// The same with the ticked changes applied.
    pub value_after: f64,
    /// Changes held back for bulk lots.
    pub held_back: u32,
    /// Listings without a change, by reason.
    pub skipped: Vec<SkippedCount>,
}

/// The market data the review used.
#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct ReviewMarket {
    pub loaded: bool,
    pub version: Option<u64>,
    /// The day it was loaded for.
    pub day: Option<String>,
    /// Newest price date among the rows (the tracker's last update).
    pub newest_price_date: Option<String>,
    /// Listings with a market row.
    pub covered: u32,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReviewPage {
    pub source: SourceInfo,
    /// Inventory data version the review is for (export checks it).
    pub data_version: i64,
    pub market: ReviewMarket,
    /// Published revision (also when the draft is shown).
    pub revision: i64,
    /// The rows come from the user's draft.
    pub draft: bool,
    pub rules: Vec<RuleView>,
    pub summary: ReviewSummary,
    /// Rows passing the filters.
    pub total: u32,
    pub offset: u32,
    pub rows: Vec<ReviewRow>,
    /// Ticks and hand prices made for an older import (ignored).
    pub dropped_ticks: u32,
    pub dropped_manual: u32,
}

fn rule_views(ruleset: &Ruleset, eval: &Evaluation) -> Vec<RuleView> {
    let reference = ruleset.guardrails.reference;
    ruleset
        .rules
        .iter()
        .zip(&eval.rule_stats)
        .map(|(r, s)| RuleView {
            name: r.name.clone(),
            enabled: r.enabled,
            when: r.describe_when(reference),
            then: r.action.describe(),
            matched: s.matched as u32,
            raised: s.raised as u32,
            lowered: s.lowered as u32,
            value_delta: s.value_delta,
        })
        .collect()
}

fn market_of(prep: &Prepared) -> ReviewMarket {
    let rows = prep.market_rows();
    ReviewMarket {
        loaded: prep.market.is_some(),
        version: prep.market_version(),
        day: prep
            .market
            .as_ref()
            .map(|m| m.today.format("%Y-%m-%d").to_string()),
        newest_price_date: rows
            .values()
            .map(|r| r.price_date.chars().take(10).collect::<String>())
            .max(),
        covered: prep
            .listings
            .iter()
            .filter(|l| l.product_id.is_some_and(|id| rows.contains_key(&id)))
            .count() as u32,
    }
}

/// Everything a review request computes.
struct Evaluated {
    prepared: Arc<Prepared>,
    published: Published,
    draft: Option<Draft>,
    review: repricing::Review,
    eval: Arc<Evaluation>,
}

impl Evaluated {
    fn ruleset(&self) -> &Ruleset {
        self.draft
            .as_ref()
            .map_or(&self.published.ruleset, |d| &d.ruleset)
    }

    fn view(&self) -> ReviewView<'_> {
        ReviewView {
            prepared: &self.prepared,
            eval: &self.eval,
            ticks: &self.review.ticks,
        }
    }
}

async fn evaluated(
    state: &AppState,
    user: &CurrentUser,
    want_draft: bool,
) -> Result<Evaluated, ApiError> {
    let prepared = required(state).await?;
    let (published, draft) = rules_for(state, user, want_draft).await?;
    let (id, version) = (user.id, prepared.source.version);
    let review = state
        .db
        .read(move |conn| repricing::review(conn, id, version))
        .await?;
    let manual = prepared.manual_by_listing(&review);
    let ruleset = draft.as_ref().map_or(&published.ruleset, |d| &d.ruleset);
    let eval = state.repricing.evaluate(&prepared, ruleset, manual).await?;
    Ok(Evaluated {
        prepared,
        published,
        draft,
        review,
        eval,
    })
}

fn rule_index(rule: Option<u32>, ruleset: &Ruleset) -> Result<Option<usize>, ApiError> {
    match rule {
        Some(r) if r as usize >= ruleset.rules.len() => {
            Err(ApiError::bad_request("There is no such rule"))
        }
        r => Ok(r.map(|r| r as usize)),
    }
}

fn search_of(search: Option<&str>) -> Result<&str, ApiError> {
    let search = search.unwrap_or("");
    if search.chars().count() > 200 {
        return Err(ApiError::bad_request("The search is too long"));
    }
    Ok(search)
}

async fn review(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiQuery(query): ApiQuery<ReviewQuery>,
) -> Result<Json<ReviewPage>, ApiError> {
    let limit = query.limit.unwrap_or(100).clamp(1, MAX_PAGE);
    let offset = query.offset.unwrap_or(0);
    let search = search_of(query.search.as_deref())?.to_string();
    let e = evaluated(&state, &user, query.draft.unwrap_or(false)).await?;
    let rule = rule_index(query.rule, e.ruleset())?;
    let filter = query.filter.unwrap_or(ReviewFilter::Changes);
    let sort = query.sort.unwrap_or(ReviewSort::Impact);
    let desc = query.desc.unwrap_or(true);
    let (prepared, eval, ticks) = (e.prepared.clone(), e.eval.clone(), e.review.ticks.clone());
    let (total, rows, s, held) = super::blocking(move || {
        let view = ReviewView {
            prepared: &prepared,
            eval: &eval,
            ticks: &ticks,
        };
        let rows = view.rows(filter, rule, &search, sort, desc);
        let shown: Vec<ReviewRow> = rows
            .iter()
            .skip(offset as usize)
            .take(limit as usize)
            .map(|&i| row_of(&view, i))
            .collect();
        let (s, held) = view.summary();
        (rows.len(), shown, s, held)
    })
    .await?;
    Ok(Json(ReviewPage {
        source: SourceInfo::from(e.prepared.source.as_ref()),
        data_version: e.prepared.data_version,
        market: market_of(&e.prepared),
        revision: e.published.revision,
        draft: e.draft.is_some(),
        rules: rule_views(e.ruleset(), &e.eval),
        summary: ReviewSummary {
            listings: s.listings as u32,
            copies: s.copies,
            raises: s.raises as u32,
            lowers: s.lowers as u32,
            review: s.review as u32,
            selected: s.selected as u32,
            selected_copies: s.selected_copies,
            value_before: s.value_before,
            value_after: s.value_after,
            held_back: held as u32,
            skipped: s
                .skipped
                .iter()
                .map(|(label, count)| SkippedCount {
                    label: label.to_string(),
                    count: *count as u32,
                })
                .collect(),
        },
        total: total as u32,
        offset,
        rows,
        dropped_ticks: e.review.dropped_ticks as u32,
        dropped_manual: e.review.dropped_manual as u32,
    }))
}

// ── Detail of one listing ──────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, optional_fields)]
pub struct RulesQuery {
    pub draft: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct MarketColumn {
    pub label: String,
    /// Foil column for foils.
    pub price: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct SaleView {
    pub date: String,
    pub copies: i64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ListingExplained {
    pub row: ReviewRow,
    /// How the price was found, step by step.
    pub steps: Vec<String>,
    pub market_date: Option<String>,
    pub market: Vec<MarketColumn>,
    pub age_days: Option<i64>,
    pub days_since_sale: Option<i64>,
    pub last_sale: Option<String>,
    pub sold_30d: i64,
    /// Newest first, at most 30.
    pub sales: Vec<SaleView>,
    /// The hand price, if one is set.
    pub manual_price: Option<f64>,
}

/// Pile ids are 16 hex digits, `-2`, `-3`, … for identical rows.
fn valid_pile(pile: &str) -> bool {
    !pile.is_empty() && pile.len() <= 24 && pile.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

async fn listing(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(pile): Path<String>,
    ApiQuery(query): ApiQuery<RulesQuery>,
) -> Result<Json<ListingExplained>, ApiError> {
    if !valid_pile(&pile) {
        return Err(ApiError::not_found());
    }
    let e = evaluated(&state, &user, query.draft.unwrap_or(false)).await?;
    let i = e.prepared.listing_of(&pile).ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "This card is no longer in the stock",
        )
    })?;
    let l = &e.prepared.listings[i];
    let manual = e.review.manual.get(&pile).copied();
    let steps = explain(
        e.ruleset(),
        l,
        e.prepared.market_rows(),
        manual,
        e.prepared.today,
    );
    let market_row = l
        .product_id
        .and_then(|id| e.prepared.market_rows().get(&id));
    let market = market_row
        .map(|m| {
            PriceField::all()
                .iter()
                .map(|&f| MarketColumn {
                    label: f.as_str().to_string(),
                    price: m.price_for(f, l.is_foil).filter(|p| *p > 0.0),
                })
                .collect()
        })
        .unwrap_or_default();
    let sales = e
        .prepared
        .sales_of(i)
        .into_iter()
        .take(MAX_SALES_SHOWN)
        .map(|(date, copies)| SaleView { date, copies })
        .collect();
    Ok(Json(ListingExplained {
        row: row_of(&e.view(), i),
        steps,
        market_date: market_row.map(|m| m.price_date.chars().take(10).collect()),
        market,
        age_days: l.facts.age_days,
        days_since_sale: l.facts.days_since_sale,
        last_sale: l.facts.last_sale.map(|d| d.format("%Y-%m-%d").to_string()),
        sold_30d: l.facts.sold_30d,
        sales,
        manual_price: manual,
    }))
}

// ── Ruleset file download ──────────────────────────────────────────────────

async fn ruleset_file(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiQuery(query): ApiQuery<RulesQuery>,
) -> Result<Response, ApiError> {
    let (published, draft) = rules_for(&state, &user, query.draft.unwrap_or(false)).await?;
    let ruleset = draft.map_or(published.ruleset, |d| d.ruleset);
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json; charset=utf-8"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=\"repricing_ruleset.json\""),
            ),
        ],
        ruleset.to_json(),
    )
        .into_response())
}

// ── Market data ────────────────────────────────────────────────────────────

async fn market(State(state): State<AppState>) -> Json<MarketStatusView> {
    Json(market_status(&state))
}

async fn load_market(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<MarketLoadRequest>,
) -> Result<Response, ApiError> {
    start_market_load(&state, &user, request.refresh).await
}

// ── Draft and publish ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct DraftRequest {
    /// The published revision the draft started from.
    pub base_revision: i64,
    pub ruleset: Ruleset,
}

/// The stored draft; `None` when it equals the published rules (then no
/// draft is kept).
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DraftSaved {
    pub draft: Option<RulesetDraftView>,
}

fn check_size(ruleset: &Ruleset) -> Result<(), ApiError> {
    let size = serde_json::to_vec(ruleset)
        .map(|v| v.len())
        .unwrap_or(usize::MAX);
    if size > MAX_RULESET_BYTES {
        return Err(invalid(
            "too_large",
            "The ruleset is too large (at most 1 MB)",
        ));
    }
    Ok(())
}

/// Stores `ruleset` as the user's draft, or drops the draft when it is the
/// published ruleset.
async fn store_draft(
    state: &AppState,
    user: &CurrentUser,
    base_revision: Option<i64>,
    mut ruleset: Ruleset,
) -> Result<Option<RulesetDraftView>, ApiError> {
    ruleset.sanitize();
    check_size(&ruleset)?;
    let id = user.id;
    let result = state
        .db
        .write(move |conn| {
            let published = repricing::published(conn)?;
            let base = base_revision.unwrap_or(published.revision);
            if base < 0 || base > published.revision {
                return Ok(Err(()));
            }
            if ruleset == published.ruleset {
                delete_draft(conn, id)?;
                return Ok(Ok(None));
            }
            Ok(Ok(Some(save_draft(conn, id, base, &ruleset, Utc::now())?)))
        })
        .await?;
    match result {
        Ok(draft) => Ok(draft.map(RulesetDraftView::from)),
        Err(()) => Err(invalid(
            "invalid_revision",
            "The draft names a revision that was never published",
        )),
    }
}

async fn put_draft(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<DraftRequest>,
) -> Result<Json<DraftSaved>, ApiError> {
    let draft = store_draft(&state, &user, Some(request.base_revision), request.ruleset).await?;
    Ok(Json(DraftSaved { draft }))
}

async fn drop_draft(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<StatusCode, ApiError> {
    let id = user.id;
    state.db.write(move |conn| delete_draft(conn, id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RulesetFileRequest {
    /// The file's text.
    pub json: String,
}

/// A ruleset file becomes the user's draft (never the published rules).
async fn open_file(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<RulesetFileRequest>,
) -> Result<Json<DraftSaved>, ApiError> {
    if request.json.len() > MAX_RULESET_BYTES {
        return Err(invalid(
            "too_large",
            "The file is too large to be a repricing ruleset (at most 1 MB)",
        ));
    }
    let ruleset = super::blocking(move || parse_ruleset_file(&request.json))
        .await?
        .map_err(|m| invalid("invalid_ruleset", m))?;
    let draft = store_draft(&state, &user, None, ruleset).await?;
    Ok(Json(DraftSaved { draft }))
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PublishRequest {
    /// The published revision the editor saw; another one → 409.
    pub expected_revision: i64,
}

enum PublishRefusal {
    NoDraft,
    Conflict(repricing::Conflict),
}

async fn publish(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<PublishRequest>,
) -> Result<Json<PublishedView>, ApiError> {
    let (id, username, display) = (user.id, user.username.clone(), user.display_name.clone());
    let expected = request.expected_revision;
    let outcome = state
        .db
        .write(move |conn| {
            let tx = conn.transaction()?;
            let Some(draft) = repricing::draft(&tx, id)? else {
                return Ok(Err(PublishRefusal::NoDraft));
            };
            let now = Utc::now();
            let revision = match publish_in_tx(&tx, &draft.ruleset, expected, now, &display)? {
                Ok(r) => r,
                Err(c) => return Ok(Err(PublishRefusal::Conflict(c))),
            };
            let rules = &draft.ruleset.rules;
            audit::record(
                &tx,
                now,
                audit::Actor::User {
                    id,
                    username: &username,
                },
                action::REPRICING_PUBLISHED,
                Some(&revision.to_string()),
                serde_json::json!({
                    "revision": revision,
                    "previous": expected,
                    "based_on": draft.base_revision,
                    "name": draft.ruleset.name,
                    "rules": rules.len(),
                    "enabled": rules.iter().filter(|r| r.enabled).count(),
                    "rule_names": rules.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
                }),
            )?;
            delete_draft(&tx, id)?;
            tx.commit()?;
            Ok(Ok(repricing::published(conn)?))
        })
        .await?;
    match outcome {
        Ok(published) => Ok(Json(published.into())),
        Err(PublishRefusal::NoDraft) => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "no_draft",
            "There are no unpublished changes",
        )),
        Err(PublishRefusal::Conflict(c)) => Err(ApiError::conflict(
            "ruleset_conflict",
            format!(
                "{} published other rules meanwhile (revision {}); look at them before you publish yours",
                c.updated_by.as_deref().unwrap_or("Someone"),
                c.revision
            ),
        )),
    }
}

// ── The user's review ──────────────────────────────────────────────────────

/// Which listings a tick request changes.
#[derive(Debug, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum TickTarget {
    Piles(Vec<String>),
    /// Every change the review lists with these filters.
    Shown(ShownRows),
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ShownRows {
    pub draft: bool,
    pub filter: ReviewFilter,
    pub rule: Option<u32>,
    pub search: String,
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct TicksRequest {
    /// Inventory version (import) the review is for.
    pub version: i64,
    pub target: TickTarget,
    /// `null` = back to the proposal's default.
    pub ticked: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TicksResult {
    pub changed: u32,
}

fn check_version(prepared: &Prepared, version: i64) -> Result<(), ApiError> {
    if prepared.source.version != version {
        return Err(stock_changed());
    }
    Ok(())
}

async fn put_ticks(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<TicksRequest>,
) -> Result<Json<TicksResult>, ApiError> {
    let piles = match request.target {
        TickTarget::Piles(piles) => {
            let prepared = required(&state).await?;
            check_version(&prepared, request.version)?;
            if piles.len() > MAX_TICK_PILES {
                return Err(invalid("too_many", "Too many cards in one request"));
            }
            if piles.iter().any(|p| prepared.listing_of(p).is_none()) {
                return Err(invalid(
                    "unknown_pile",
                    "A card is no longer in the stock; reload the page",
                ));
            }
            piles
        }
        TickTarget::Shown(ShownRows {
            draft,
            filter,
            rule,
            search,
        }) => {
            let search = search_of(Some(&search))?.to_string();
            let e = evaluated(&state, &user, draft).await?;
            check_version(&e.prepared, request.version)?;
            let rule = rule_index(rule, e.ruleset())?;
            let view = e.view();
            let rows = view.rows(filter, rule, &search, ReviewSort::Name, false);
            view.piles_of(&rows)
        }
    };
    let (id, version, ticked) = (user.id, request.version, request.ticked);
    let changed = piles.len() as u32;
    state
        .db
        .write(move |conn| {
            let tx = conn.transaction()?;
            set_ticks(&tx, id, version, &piles, ticked)?;
            tx.commit()
        })
        .await?;
    Ok(Json(TicksResult { changed }))
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ManualRequest {
    pub version: i64,
    pub pile: String,
    /// € (0.02–100,000); `null` removes the hand price.
    pub price: Option<f64>,
}

async fn put_manual(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<ManualRequest>,
) -> Result<StatusCode, ApiError> {
    let cents_value = match request.price {
        None => None,
        Some(p) if p.is_finite() && (CARDMARKET_MIN_PRICE..=MAX_PRICE_EUR).contains(&p) => {
            Some(cents(p))
        }
        Some(_) => {
            return Err(invalid(
                "invalid_price",
                "A price must be between 0.02 € and 100,000 €",
            ))
        }
    };
    let prepared = required(&state).await?;
    check_version(&prepared, request.version)?;
    if prepared.listing_of(&request.pile).is_none() {
        return Err(invalid(
            "unknown_pile",
            "This card is no longer in the stock; reload the page",
        ));
    }
    let (id, version, pile) = (user.id, request.version, request.pile);
    state
        .db
        .write(move |conn| set_manual(conn, id, version, &pile, cents_value))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn reset_review(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<StatusCode, ApiError> {
    let id = user.id;
    state.db.write(move |conn| clear_review(conn, id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Export ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PriceExportRequest {
    /// The published revision, data version and market version the user
    /// checked; any other → 409.
    pub revision: i64,
    pub data_version: i64,
    pub market_version: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PriceExportResult {
    pub id: i64,
    pub file_name: String,
    pub undo_file_name: String,
    pub changes: u32,
    pub raises: u32,
    pub lowers: u32,
}

enum ExportRefusal {
    Changed,
    RulesChanged,
}

async fn make_export(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<PriceExportRequest>,
) -> Result<Json<PriceExportResult>, ApiError> {
    // Always the published rules, never a draft.
    let e = evaluated(&state, &user, false).await?;
    if e.prepared.data_version != request.data_version
        || e.prepared.market_version() != request.market_version
    {
        return Err(stock_changed());
    }
    if e.published.revision != request.revision {
        return Err(rules_changed());
    }
    let (changes, csv, undo_csv) = super::blocking({
        let prepared = e.prepared.clone();
        let eval = e.eval.clone();
        let ticks = e.review.ticks.clone();
        move || {
            let changes = ticked_changes(&eval, &prepared.listings, |i, p| {
                is_ticked(
                    p,
                    ticks.get(&prepared.piles[i]).copied(),
                    prepared.held_back[i],
                )
            });
            let rows: Vec<usize> = changes.iter().map(|c| c.row).collect();
            let new: HashMap<usize, f64> = changes.iter().map(|c| (c.row, c.new_price)).collect();
            let old: HashMap<usize, f64> = changes.iter().map(|c| (c.row, c.old_price)).collect();
            let cards = &prepared.source.cards;
            (
                changes,
                format_price_diff_csv(cards, &rows, &new),
                format_price_diff_csv(cards, &rows, &old),
            )
        }
    })
    .await?;
    if changes.is_empty() {
        return Err(invalid("nothing_ticked", "No price change is ticked"));
    }
    let raises = changes.iter().filter(|c| c.new_price > c.old_price).count();
    let lowers = changes.len() - raises;
    let file_name = export_file_name(e.prepared.today);
    let (id, username) = (user.id, user.username.clone());
    let (revision, data_version, version) = (
        request.revision,
        request.data_version,
        e.prepared.source.version,
    );
    let count = changes.len();
    let name = file_name.clone();
    let outcome = state
        .db
        .write(move |conn| {
            let tx = conn.transaction()?;
            // Nothing may have changed between the check and this write.
            if latest_version(&tx)?.0 != data_version {
                return Ok(Err(ExportRefusal::Changed));
            }
            if repricing::published(&tx)?.revision != revision {
                return Ok(Err(ExportRefusal::RulesChanged));
            }
            let now = Utc::now();
            let export_id = keep_export(
                &tx,
                now,
                &NewExport {
                    user_id: id,
                    username: &username,
                    revision,
                    version,
                    changes: count,
                    raises,
                    lowers,
                    file_name: &name,
                    csv: &csv,
                    undo_csv: &undo_csv,
                },
            )?;
            audit::record(
                &tx,
                now,
                audit::Actor::User {
                    id,
                    username: &username,
                },
                action::REPRICING_EXPORTED,
                Some(&export_id.to_string()),
                serde_json::json!({
                    "changes": count,
                    "raises": raises,
                    "lowers": lowers,
                    "revision": revision,
                    "version": version,
                    "file_name": name,
                }),
            )?;
            tx.commit()?;
            Ok(Ok(export_id))
        })
        .await?;
    match outcome {
        Ok(export_id) => Ok(Json(PriceExportResult {
            id: export_id,
            undo_file_name: undo_file_name(&file_name),
            file_name,
            changes: count as u32,
            raises: raises as u32,
            lowers: lowers as u32,
        })),
        Err(ExportRefusal::Changed) => Err(stock_changed()),
        Err(ExportRefusal::RulesChanged) => Err(rules_changed()),
    }
}

fn rules_changed() -> ApiError {
    ApiError::conflict(
        "rules_changed",
        "Someone published other rules meanwhile; check the prices again",
    )
}

async fn list_exports(
    State(state): State<AppState>,
) -> Result<Json<Vec<PriceExportSummary>>, ApiError> {
    Ok(Json(state.db.read(exports).await?))
}

async fn export_csv(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    export_download(&state, id, false).await
}

async fn export_undo(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    export_download(&state, id, true).await
}

async fn export_download(state: &AppState, id: i64, undo: bool) -> Result<Response, ApiError> {
    let (file_name, csv) = state
        .db
        .read(move |conn| export_file(conn, id, undo))
        .await?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "not_found",
                "This export is no longer kept (only the last 20 are)",
            )
        })?;
    // Names come from `export_file_name` (letters, digits, `_`, `-`, `.`).
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

#[cfg(test)]
#[path = "repricing_tests.rs"]
mod tests;
