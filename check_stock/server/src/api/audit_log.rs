//! `GET /api/audit` (permission `audit.view`): the audit log, newest first,
//! paged with `before_id`.

use super::{Access, Api};
use crate::app::AppState;
use crate::audit::{self, AuditEntry};
use crate::http::ApiError;
use crate::permissions::Permission;
use axum::extract::{Query, State};
use axum::routing::get;
use axum::Json;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Page size when the request names none.
pub const DEFAULT_PAGE: u32 = 50;

pub(super) fn routes(api: Api) -> Api {
    api.route(
        "/api/audit",
        Access::Permission(Permission::AuditView),
        get(list_entries),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditQuery {
    before_id: Option<i64>,
    limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct AuditPage {
    pub entries: Vec<AuditEntry>,
    /// Pass as `before_id` for the next (older) page; `None` at the end.
    pub next_before_id: Option<i64>,
}

async fn list_entries(
    State(state): State<AppState>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<AuditPage>, ApiError> {
    let limit = query
        .limit
        .unwrap_or(DEFAULT_PAGE)
        .clamp(1, audit::MAX_PAGE);
    let before = query.before_id;
    let entries = state
        .db
        .read(move |conn| audit::list(conn, before, limit))
        .await?;
    let next_before_id = (entries.len() == limit as usize)
        .then(|| entries.last().map(|e| e.id))
        .flatten();
    Ok(Json(AuditPage {
        entries,
        next_before_id,
    }))
}
