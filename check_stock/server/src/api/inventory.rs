//! Inventory import (permission `inventory.import`), see
//! [`crate::inventory_import`] for the rules.
//!
//! - `POST /api/inventory/preview` `{file_name, csv}`: checks the export
//!   against the current data and keeps the preview (30 minutes).
//! - `POST /api/inventory/confirm` `{file_name, csv, confirm_sold?}`: the
//!   same file again; imports it after a backup. A blocked import needs
//!   `confirm_sold` = the copies that will count as sold.
//! - `DELETE /api/inventory/pending`: drops the user's preview.
//! - `GET /api/inventory/imports`: the latest changes of the inventory data.
//!
//! Only these two routes take large bodies (16 MB of CSV); the file is read
//! on a blocking thread. Error messages never quote the file except for a
//! card's name.

use super::{blocking, Access, Api, ApiJson, CurrentUser};
use crate::app::AppState;
use crate::http::ApiError;
use crate::inventory_import::{
    self, check, discard_pending, read_file, recent_versions, save_pending, ConfirmError,
    FileError, ImportFile, ImportNumbers, Importer, InventoryVersion, MAX_IMPORT_BYTES,
    PENDING_MINUTES,
};
use crate::migrations::format_utc;
use crate::permissions::Permission;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::Json;
use check_stock_core::precon_loader::local_date;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Request body limit of the two upload routes: the CSV as a JSON string
/// (escaping can double it) plus the rest.
pub const BODY_LIMIT: usize = 2 * MAX_IMPORT_BYTES + 64 * 1024;
/// Longest typed confirmation accepted.
const MAX_CONFIRMATION_CHARS: usize = 32;
/// Changes listed by `/api/inventory/imports`.
const HISTORY_LIMIT: u32 = 20;

pub(super) fn routes(api: Api) -> Api {
    api.route(
        "/api/inventory/preview",
        Access::Permission(Permission::InventoryImport),
        post(preview).layer(DefaultBodyLimit::max(BODY_LIMIT)),
    )
    .route(
        "/api/inventory/confirm",
        Access::Permission(Permission::InventoryImport),
        post(confirm).layer(DefaultBodyLimit::max(BODY_LIMIT)),
    )
    .route(
        "/api/inventory/pending",
        Access::Permission(Permission::InventoryImport),
        delete(discard),
    )
    .route(
        "/api/inventory/imports",
        Access::Permission(Permission::InventoryImport),
        get(history),
    )
}

/// An inventory export to check.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct InventoryFileRequest {
    /// The file's name (decides the product category).
    pub file_name: String,
    pub csv: String,
}

/// The checked export again, to import it.
#[derive(Debug, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, optional_fields)]
pub struct InventoryConfirmRequest {
    pub file_name: String,
    pub csv: String,
    /// For an import the safety check blocked: the number of copies that
    /// will count as sold, typed in.
    pub confirm_sold: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct SalesPaceView {
    /// Copies usually sold per day.
    pub daily_copies: f64,
    /// Days the average covers (at most 30).
    pub days: i64,
    /// Day of the category's last import.
    pub last_import: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct UnusualSalesView {
    pub daily_copies: f64,
    pub days_since_last_import: i64,
    /// Copies that usually sell in that time.
    pub expected_copies: f64,
}

/// What importing the file would change.
#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InventoryPreview {
    pub file_name: String,
    pub category: String,
    /// Nothing of this category in the data yet.
    pub new_category: bool,
    /// Listings (rows with stock) in the file.
    pub rows: i64,
    pub copies_in_file: i64,
    pub numbers: ImportNumbers,
    /// More than half the stock would count as sold: confirming needs the
    /// typed number of copies.
    pub blocked: bool,
    pub pace: Option<SalesPaceView>,
    /// More than 3× the usual sales (a warning).
    pub unusual: Option<UnusualSalesView>,
    /// Until when the preview can be confirmed (RFC 3339).
    pub expires_at: String,
    pub valid_minutes: i64,
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct InventoryImportResult {
    pub version: i64,
    pub category: String,
    pub numbers: ImportNumbers,
    /// Imported although the safety check blocked it.
    pub forced: bool,
    /// Listings added or updated, and set to 0.
    pub upserted: i64,
    pub zeroed: i64,
    pub bulk_removed: i64,
}

fn body<T>(request: Result<ApiJson<T>, ApiError>) -> Result<T, ApiError> {
    match request {
        Ok(ApiJson(request)) => Ok(request),
        Err(e) if e.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            Err(file_error(FileError::TooLarge))
        }
        Err(e) => Err(e),
    }
}

fn file_error(e: FileError) -> ApiError {
    match e {
        FileError::TooLarge => {
            ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "too_large", e.to_string())
        }
        other => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_file",
            other.to_string(),
        ),
    }
}

fn confirm_error(e: ConfirmError) -> ApiError {
    let again = "check the file again";
    match e {
        ConfirmError::NoPreview => ApiError::conflict(
            "no_preview",
            format!("There is no check of this file to confirm; {again}"),
        ),
        ConfirmError::Expired => ApiError::conflict(
            "preview_expired",
            format!("The check is older than {PENDING_MINUTES} minutes; {again}"),
        ),
        ConfirmError::OtherFile => ApiError::conflict(
            "other_file",
            format!("This is not the file that was checked; {again}"),
        ),
        ConfirmError::Outdated => ApiError::conflict(
            "outdated",
            format!("The inventory was changed since the check (another import); {again}"),
        ),
        ConfirmError::Changed => ApiError::conflict(
            "changed",
            format!("The stock changed since the check; {again}"),
        ),
        ConfirmError::NeedsConfirmation { copies_sold } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "confirmation_needed",
            format!(
                "This import is blocked: type the number of copies that will count as sold ({copies_sold}) to import it anyway"
            ),
        ),
        ConfirmError::WrongConfirmation { copies_sold } => ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "confirmation_wrong",
            format!("That is not the number of copies that will count as sold ({copies_sold})"),
        ),
        ConfirmError::BackupFailed => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "backup_failed",
            "The backup before the import failed, so nothing was imported. Try again; if it keeps failing, the server's disk may be full",
        ),
    }
}

fn today() -> String {
    local_date(Utc::now().timestamp())
        .format("%Y-%m-%d")
        .to_string()
}

async fn read(file_name: String, csv: String) -> Result<ImportFile, ApiError> {
    blocking(move || read_file(&file_name, csv))
        .await?
        .map_err(file_error)
}

async fn preview(
    State(state): State<AppState>,
    user: CurrentUser,
    request: Result<ApiJson<InventoryFileRequest>, ApiError>,
) -> Result<Json<InventoryPreview>, ApiError> {
    let request = body(request)?;
    let file = read(request.file_name, request.csv).await?;
    let (today, now, user_id) = (today(), Utc::now(), user.id);
    let (file, checked, expires) = state
        .db
        .write(move |conn| {
            let checked = check(conn, &file, &today)?;
            let expires = save_pending(conn, user_id, &file, &checked, now)?;
            Ok((file, checked, expires))
        })
        .await?;
    Ok(Json(InventoryPreview {
        rows: i64::try_from(file.cards.len()).unwrap_or(i64::MAX),
        copies_in_file: file.copies(),
        file_name: file.file_name,
        category: file.category,
        new_category: checked.new_category,
        numbers: checked.numbers,
        blocked: checked.blocked,
        pace: checked.pace.map(|p| SalesPaceView {
            daily_copies: p.daily_copies,
            days: p.days,
            last_import: p.last_sync,
        }),
        unusual: checked.unusual.map(|u| UnusualSalesView {
            daily_copies: u.daily_copies,
            days_since_last_import: u.days_since_last_import,
            expected_copies: u.expected_copies,
        }),
        expires_at: format_utc(expires),
        valid_minutes: PENDING_MINUTES,
    }))
}

async fn confirm(
    State(state): State<AppState>,
    user: CurrentUser,
    request: Result<ApiJson<InventoryConfirmRequest>, ApiError>,
) -> Result<Json<InventoryImportResult>, ApiError> {
    let request = body(request)?;
    if request
        .confirm_sold
        .as_ref()
        .is_some_and(|t| t.chars().count() > MAX_CONFIRMATION_CHARS)
    {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "confirmation_wrong",
            "That is not a number of copies",
        ));
    }
    let file = read(request.file_name, request.csv).await?;
    let category = file.category.clone();
    let db_path = state.db.path().to_path_buf();
    let typed = request.confirm_sold;
    let (today, now) = (today(), Utc::now());
    let (id, username, display_name) = (user.id, user.username.clone(), user.display_name.clone());
    let result = state
        .db
        .write(move |conn| {
            inventory_import::confirm(
                conn,
                &db_path,
                Importer {
                    id,
                    username: &username,
                    display_name: &display_name,
                },
                &file,
                typed.as_deref(),
                now,
                &today,
            )
        })
        .await?;
    let done = result.map_err(confirm_error)?;
    state.market.stock_changed();
    let n = |v: usize| i64::try_from(v).unwrap_or(i64::MAX);
    Ok(Json(InventoryImportResult {
        version: done.version,
        category,
        numbers: done.numbers,
        forced: done.forced,
        upserted: n(done.upserted),
        zeroed: n(done.zeroed),
        bulk_removed: done.bulk_removed,
    }))
}

async fn discard(State(state): State<AppState>, user: CurrentUser) -> Result<StatusCode, ApiError> {
    let id = user.id;
    state
        .db
        .write(move |conn| discard_pending(conn, id))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn history(State(state): State<AppState>) -> Result<Json<Vec<InventoryVersion>>, ApiError> {
    let versions = state
        .db
        .read(|conn| recent_versions(conn, HISTORY_LIMIT))
        .await?;
    Ok(Json(versions))
}

#[cfg(test)]
#[path = "inventory_tests.rs"]
mod tests;
