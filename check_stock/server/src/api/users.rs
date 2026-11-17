//! User administration (permission `users.manage`) and the permission
//! catalog with its descriptions (any logged-in user).
//!
//! Rules enforced here, inside the same transaction as the change:
//! - at least one enabled user keeps "Manage users" (no lock-out),
//! - nobody disables or deletes their own account,
//! - permissions are granted together with the ones they require,
//! - disabling an account or setting its password ends its sessions.

use super::{blocking, in_transaction, Access, Api, ApiJson, CurrentUser};
use crate::app::AppState;
use crate::audit::{self, action};
use crate::http::ApiError;
use crate::password;
use crate::permissions::{self, Permission, PermissionInfo, PermissionSet};
use crate::sessions;
use crate::users::{self, StoreError, User};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;

const MANAGE: Access = Access::Permission(Permission::UsersManage);

pub(super) fn routes(api: Api) -> Api {
    api.route("/api/permissions", Access::LoggedIn, get(list_permissions))
        .route("/api/users", MANAGE, get(list_users).post(create_user))
        .route(
            "/api/users/{id}",
            MANAGE,
            patch(update_user).delete(delete_user),
        )
        .route("/api/users/{id}/password", MANAGE, post(set_password))
}

/// A user as the admin interface sees it (never the password hash).
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct UserView {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub disabled: bool,
    /// Set while the account is locked after failed logins.
    pub locked_until: Option<String>,
    pub permissions: Vec<String>,
    pub created_at: String,
    pub last_login_at: Option<String>,
}

impl UserView {
    fn new(user: &User, now: DateTime<Utc>) -> Self {
        Self {
            id: user.id,
            username: user.username.clone(),
            display_name: user.display_name.clone(),
            disabled: user.disabled,
            locked_until: user
                .is_locked(now)
                .then(|| user.locked_until.map(crate::migrations::format_utc))
                .flatten(),
            permissions: user
                .permissions
                .keys()
                .iter()
                .map(|k| k.to_string())
                .collect(),
            created_at: user.created_at.clone(),
            last_login_at: user.last_login_at.clone(),
        }
    }
}

/// `GET /api/permissions`: every permission with label, description, group
/// and requirements, for the user administration.
async fn list_permissions() -> Json<Vec<PermissionInfo>> {
    Json(permissions::catalog())
}

async fn list_users(State(state): State<AppState>) -> Result<Json<Vec<UserView>>, ApiError> {
    let now = Utc::now();
    let list = state.db.read(users::list).await?;
    Ok(Json(list.iter().map(|u| UserView::new(u, now)).collect()))
}

fn hash_error(e: password::PasswordError) -> ApiError {
    log::error!("{e}");
    ApiError::internal()
}

async fn load_view(state: &AppState, id: i64) -> Result<UserView, ApiError> {
    let now = Utc::now();
    state
        .db
        .read(move |conn| users::find_by_id(conn, id))
        .await?
        .map(|u| UserView::new(&u, now))
        .ok_or_else(|| StoreError::NotFound.into())
}

#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct CreateUserRequest {
    pub username: String,
    pub display_name: String,
    pub password: String,
    /// Left out: no permissions.
    #[serde(default)]
    pub permissions: Vec<String>,
}

async fn create_user(
    State(state): State<AppState>,
    admin: CurrentUser,
    ApiJson(request): ApiJson<CreateUserRequest>,
) -> Result<Response, ApiError> {
    let username = users::normalize_username(&request.username).map_err(ApiError::bad_request)?;
    let display_name =
        users::normalize_display_name(&request.display_name).map_err(ApiError::bad_request)?;
    let granted = PermissionSet::from_keys(&request.permissions).map_err(ApiError::bad_request)?;
    password::check_policy(&request.password, &username).map_err(ApiError::bad_request)?;
    let pw = request.password;
    let hash = blocking(move || password::hash(&pw))
        .await?
        .map_err(hash_error)?;

    let now = Utc::now();
    let id = state
        .db
        .write(move |conn| {
            in_transaction(conn, |tx| {
                let id = users::create(tx, &username, &display_name, &hash, &granted, now)?;
                audit::record(
                    tx,
                    now,
                    admin.actor(),
                    action::USER_CREATED,
                    Some(&username),
                    json!({ "display_name": display_name, "permissions": granted.keys() }),
                )?;
                Ok(id)
            })
        })
        .await??;
    let view = load_view(&state, id).await?;
    Ok((StatusCode::CREATED, Json(view)).into_response())
}

/// Fields left out stay as they are.
#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, optional_fields)]
pub struct UpdateUserRequest {
    pub display_name: Option<String>,
    pub disabled: Option<bool>,
    /// Replaces the whole set.
    pub permissions: Option<Vec<String>>,
    /// `true` lifts a lockout after failed logins.
    pub unlock: Option<bool>,
}

async fn update_user(
    State(state): State<AppState>,
    admin: CurrentUser,
    Path(id): Path<i64>,
    ApiJson(request): ApiJson<UpdateUserRequest>,
) -> Result<Json<UserView>, ApiError> {
    if request.display_name.is_none()
        && request.disabled.is_none()
        && request.permissions.is_none()
        && request.unlock != Some(true)
    {
        return Err(ApiError::bad_request("Nothing to change"));
    }
    if request.disabled == Some(true) && id == admin.id {
        return Err(ApiError::conflict(
            "self",
            "You cannot disable your own account",
        ));
    }
    let display_name = request
        .display_name
        .as_deref()
        .map(users::normalize_display_name)
        .transpose()
        .map_err(ApiError::bad_request)?;
    let permissions = request
        .permissions
        .as_deref()
        .map(PermissionSet::from_keys)
        .transpose()
        .map_err(ApiError::bad_request)?;
    let disabled = request.disabled;
    let unlock = request.unlock == Some(true);

    let now = Utc::now();
    state
        .db
        .write(move |conn| {
            in_transaction(conn, |tx| {
                let before = users::find_by_id(tx, id)?.ok_or(StoreError::NotFound)?;
                let mut changes = serde_json::Map::new();
                if let Some(name) = &display_name {
                    if *name != before.display_name {
                        users::set_display_name(tx, id, name)?;
                        changes.insert(
                            "display_name".into(),
                            json!({ "from": before.display_name, "to": name }),
                        );
                    }
                }
                if let Some(disabled) = disabled {
                    if disabled != before.disabled {
                        users::set_disabled(tx, id, disabled)?;
                        if disabled {
                            sessions::revoke_all_for_user(tx, id)?;
                        }
                        changes.insert(
                            "disabled".into(),
                            json!({ "from": before.disabled, "to": disabled }),
                        );
                    }
                }
                if let Some(granted) = &permissions {
                    if *granted != before.permissions {
                        users::set_permissions(tx, id, granted)?;
                        let added: Vec<_> = granted
                            .iter()
                            .filter(|p| !before.permissions.contains(*p))
                            .map(Permission::key)
                            .collect();
                        let removed: Vec<_> = before
                            .permissions
                            .iter()
                            .filter(|p| !granted.contains(*p))
                            .map(Permission::key)
                            .collect();
                        changes.insert(
                            "permissions".into(),
                            json!({ "added": added, "removed": removed }),
                        );
                    }
                }
                if unlock && before.is_locked(now) {
                    users::unlock(tx, id)?;
                    changes.insert("unlocked".into(), json!(true));
                }
                users::ensure_an_admin_remains(tx)?;
                if !changes.is_empty() {
                    audit::record(
                        tx,
                        now,
                        admin.actor(),
                        action::USER_UPDATED,
                        Some(&before.username),
                        serde_json::Value::Object(changes),
                    )?;
                }
                Ok(())
            })
        })
        .await??;
    Ok(Json(load_view(&state, id).await?))
}

#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SetPasswordRequest {
    pub password: String,
}

/// Sets another user's password (or one's own without the old one). Unlocks
/// the account and ends all its sessions.
async fn set_password(
    State(state): State<AppState>,
    admin: CurrentUser,
    Path(id): Path<i64>,
    ApiJson(request): ApiJson<SetPasswordRequest>,
) -> Result<StatusCode, ApiError> {
    let target = state
        .db
        .read(move |conn| users::find_by_id(conn, id))
        .await?
        .ok_or(StoreError::NotFound)?;
    password::check_policy(&request.password, &target.username).map_err(ApiError::bad_request)?;
    let pw = request.password;
    let hash = blocking(move || password::hash(&pw))
        .await?
        .map_err(hash_error)?;
    let now = Utc::now();
    state
        .db
        .write(move |conn| {
            in_transaction(conn, |tx| {
                users::set_password(tx, id, &hash, now)?;
                sessions::revoke_all_for_user(tx, id)?;
                audit::record(
                    tx,
                    now,
                    admin.actor(),
                    action::USER_PASSWORD_RESET,
                    Some(&target.username),
                    json!({}),
                )?;
                Ok(())
            })
        })
        .await??;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_user(
    State(state): State<AppState>,
    admin: CurrentUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    if id == admin.id {
        return Err(ApiError::conflict(
            "self",
            "You cannot delete your own account",
        ));
    }
    let now = Utc::now();
    state
        .db
        .write(move |conn| {
            in_transaction(conn, |tx| {
                let user = users::find_by_id(tx, id)?.ok_or(StoreError::NotFound)?;
                users::delete(tx, id)?;
                users::ensure_an_admin_remains(tx)?;
                audit::record(
                    tx,
                    now,
                    admin.actor(),
                    action::USER_DELETED,
                    Some(&user.username),
                    json!({ "display_name": user.display_name }),
                )?;
                Ok(())
            })
        })
        .await??;
    Ok(StatusCode::NO_CONTENT)
}
