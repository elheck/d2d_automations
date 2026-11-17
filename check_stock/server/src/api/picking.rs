//! Picking lists (`stock_checker.use`), see [`crate::picking`].
//!
//! - `POST /api/picking` `{items: [{pile, quantity}]}`: a new list from the
//!   current stock (piles that are gone or too small are refused).
//! - `GET /api/picking`: the user's list (`null` when none); `outdated` when
//!   the stock changed since (import or discard): locations may differ.
//! - `PUT /api/picking/picked` `{piles, picked}`; `DELETE /api/picking`.
//! - `GET /api/picking/image/{set}/{number}`: card images for the tiles.

use super::bins::source;
use super::stock_checker::{resolve, PileQuantity, MAX_OUTPUT_ITEMS};
use super::{in_transaction, Access, Api, ApiJson, CurrentUser};
use crate::app::AppState;
use crate::audit::{self, action, Actor};
use crate::http::ApiError;
use crate::permissions::Permission;
use crate::picking::{self, PickItem, Session};
use crate::users::StoreError;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::Json;
use check_stock_core::cache::ImageCache;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub(super) fn routes(api: Api) -> Api {
    api.route(
        "/api/picking",
        Access::Permission(Permission::StockCheckerUse),
        get(get_list).post(create).delete(delete),
    )
    .route(
        "/api/picking/picked",
        Access::Permission(Permission::StockCheckerUse),
        put(set_picked),
    )
    .route(
        "/api/picking/image/{set}/{number}",
        Access::Permission(Permission::StockCheckerUse),
        get(super::singles::image),
    )
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct PickItemView {
    pub pile: String,
    pub quantity: i64,
    pub picked: bool,
    pub name: String,
    pub set_name: String,
    pub set_code: String,
    pub collector_number: String,
    pub condition: String,
    pub language: String,
    pub foil: bool,
    pub location: String,
    /// Listed price per copy.
    pub price: f64,
    pub image: Option<String>,
}

impl From<&PickItem> for PickItemView {
    fn from(i: &PickItem) -> Self {
        let set = i.set_code.trim().to_ascii_lowercase();
        let number = i.collector_number.trim();
        Self {
            pile: i.pile.clone(),
            quantity: i.quantity,
            picked: i.picked,
            name: i.name.clone(),
            set_name: i.set_name.clone(),
            set_code: i.set_code.clone(),
            collector_number: i.collector_number.clone(),
            condition: i.condition.clone(),
            language: i.language.clone(),
            foil: i.foil,
            location: i.location.clone(),
            price: i.price,
            image: ImageCache::is_safe_key(&set, number)
                .then(|| format!("/api/picking/image/{set}/{number}")),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PickingView {
    /// RFC 3339.
    pub created_at: String,
    /// The stock changed since the list was made (an import or a discard).
    pub outdated: bool,
    pub items: Vec<PickItemView>,
}

async fn view(state: &AppState, session: Session) -> Result<PickingView, ApiError> {
    let current = source(state).await?;
    let outdated = current
        .is_none_or(|s| s.version != session.version || s.discard_seq != session.discard_seq);
    Ok(PickingView {
        created_at: session.created_at.clone(),
        outdated,
        items: session.items.iter().map(PickItemView::from).collect(),
    })
}

async fn get_list(
    State(state): State<AppState>,
    user: CurrentUser,
) -> Result<Json<Option<PickingView>>, ApiError> {
    let id = user.id;
    let Some(session) = state.db.read(move |conn| picking::load(conn, id)).await? else {
        return Ok(Json(None));
    };
    Ok(Json(Some(view(&state, session).await?)))
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PickingRequest {
    pub items: Vec<PileQuantity>,
}

async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<PickingRequest>,
) -> Result<Json<PickingView>, ApiError> {
    if request.items.is_empty() || request.items.len() > MAX_OUTPUT_ITEMS {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_items",
            "Choose 1 to 5,000 piles",
        ));
    }
    let source = source(&state).await?.ok_or_else(|| {
        ApiError::conflict(
            "no_import",
            "Picking needs an inventory import on the server (Tools → Inventory import)",
        )
    })?;
    let items: Vec<PickItem> = resolve(&source, &request.items)?
        .into_iter()
        .zip(&request.items)
        .map(|((card, quantity), item)| PickItem::of(&item.pile, card, quantity))
        .collect();
    let (id, username) = (user.id, user.username.clone());
    let session = state
        .db
        .write(move |conn| {
            in_transaction(conn, |tx| {
                let now = Utc::now();
                let session = picking::create(tx, id, &source, items, now)?;
                audit::record(
                    tx,
                    now,
                    Actor::User {
                        id,
                        username: &username,
                    },
                    action::PICKING_CREATED,
                    None,
                    serde_json::json!({
                        "piles": session.items.len(),
                        "copies": session.items.iter().map(|i| i.quantity).sum::<i64>(),
                    }),
                )?;
                Ok::<_, StoreError>(session)
            })
        })
        .await??;
    Ok(Json(view(&state, session).await?))
}

#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PickedRequest {
    pub piles: Vec<String>,
    pub picked: bool,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PickedResult {
    pub picked: i64,
}

async fn set_picked(
    State(state): State<AppState>,
    user: CurrentUser,
    ApiJson(request): ApiJson<PickedRequest>,
) -> Result<Json<PickedResult>, ApiError> {
    if request.piles.is_empty()
        || request.piles.len() > MAX_OUTPUT_ITEMS
        || request.piles.iter().any(|p| p.len() > 24)
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_piles",
            "Mark 1 to 5,000 cards",
        ));
    }
    let id = user.id;
    let picked = state
        .db
        .write(move |conn| {
            let tx = conn.transaction()?;
            let picked = picking::set_picked(&tx, id, &request.piles, request.picked)?;
            tx.commit()?;
            Ok(picked)
        })
        .await?
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::NOT_FOUND,
                "not_in_list",
                "That card is not in your picking list",
            )
        })?;
    Ok(Json(PickedResult {
        picked: picked as i64,
    }))
}

async fn delete(State(state): State<AppState>, user: CurrentUser) -> Result<StatusCode, ApiError> {
    let id = user.id;
    if state
        .db
        .write(move |conn| picking::discard(conn, id))
        .await?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "no_list",
            "There is no picking list",
        ))
    }
}

#[cfg(test)]
#[path = "picking_tests.rs"]
mod tests;
