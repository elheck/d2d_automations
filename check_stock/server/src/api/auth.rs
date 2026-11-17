//! Login, logout, the current user and changing one's own password.
//!
//! `POST /api/auth/login` answers every failure the same way (unknown user,
//! wrong password, locked or disabled account) and takes the same time for
//! unknown users, so it reveals nothing about which accounts exist. The audit
//! log records the real reason.

use super::{blocking, in_transaction, Access, Api, ApiJson, CurrentUser};
use crate::app::AppState;
use crate::audit::{self, action, Actor};
use crate::http::ApiError;
use crate::password::{self, MAX_PASSWORD_BYTES};
use crate::permissions::PermissionSet;
use crate::sessions::{self, SessionToken};
use crate::users::{self, StoreError, MAX_USERNAME_CHARS};
use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::{IpAddr, SocketAddr};
use std::time::Instant;
use ts_rs::TS;

pub(super) fn routes(api: Api) -> Api {
    api.route("/api/auth/login", Access::Public, post(login))
        .route("/api/auth/logout", Access::LoggedIn, post(logout))
        .route("/api/auth/me", Access::LoggedIn, get(me))
        .route(
            "/api/auth/password",
            Access::LoggedIn,
            post(change_password),
        )
}

#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// The logged-in user, as the web UI sees it.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, TS)]
#[ts(export)]
pub struct MeResponse {
    pub username: String,
    pub display_name: String,
    pub permissions: Vec<String>,
}

impl MeResponse {
    fn new(username: &str, display_name: &str, permissions: &PermissionSet) -> Self {
        Self {
            username: username.to_string(),
            display_name: display_name.to_string(),
            permissions: permissions.keys().iter().map(|k| k.to_string()).collect(),
        }
    }
}

/// Client address for the rate limit and the audit log.
fn client_ip(state: &AppState, peer: SocketAddr, headers: &HeaderMap) -> IpAddr {
    let forwarded = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());
    state.trusted_proxies.client_ip(peer.ip(), forwarded)
}

fn user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// What a failed attempt typed as user name, safe for the log.
fn attempted_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .take(MAX_USERNAME_CHARS * 2)
        .collect()
}

fn with_session_cookie(token: &SessionToken, body: impl IntoResponse) -> Response {
    let mut response = body.into_response();
    let cookie = HeaderValue::from_str(&sessions::set_cookie(token)).expect("cookie is ASCII");
    response.headers_mut().insert(header::SET_COOKIE, cookie);
    response
}

fn new_token() -> Result<SessionToken, ApiError> {
    SessionToken::generate().map_err(|e| {
        log::error!("no random numbers for a session token: {e}");
        ApiError::internal()
    })
}

async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ApiJson(request): ApiJson<LoginRequest>,
) -> Result<Response, ApiError> {
    let ip = client_ip(&state, peer, &headers);
    state
        .login_limiter
        .attempt(ip, Instant::now())
        .map_err(ApiError::too_many_attempts)?;
    if request.username.len() > MAX_USERNAME_CHARS * 4
        || request.password.len() > MAX_PASSWORD_BYTES
    {
        return Err(ApiError::invalid_credentials());
    }

    let lookup = request.username.trim().to_ascii_lowercase();
    let user = state
        .db
        .read(move |conn| users::find_by_username(conn, &lookup))
        .await?;
    let stored_hash = user
        .as_ref()
        .map(|u| u.password_hash.clone())
        .unwrap_or_else(|| state.dummy_hash.to_string());
    let password = request.password;
    let password_ok = blocking(move || password::verify(&password, &stored_hash)).await?;
    let now = Utc::now();
    let attempted = attempted_name(&request.username);
    let ip_text = ip.to_string();

    let failure = match &user {
        None => Some("unknown_user"),
        Some(u) if u.is_locked(now) => Some("locked"),
        Some(_) if !password_ok => Some("wrong_password"),
        Some(u) if u.disabled => Some("disabled"),
        Some(_) => None,
    };
    if let Some(reason) = failure {
        let user_id = user.as_ref().map(|u| u.id);
        let count_failure = reason == "wrong_password";
        state
            .db
            .write(move |conn| {
                in_transaction(conn, |tx| {
                    let locked = match (user_id, count_failure) {
                        (Some(id), true) => users::record_failed_login(tx, id, now)?,
                        _ => false,
                    };
                    audit::record(
                        tx,
                        now,
                        Actor::Anonymous,
                        action::LOGIN_FAILED,
                        Some(&attempted),
                        json!({ "reason": reason, "ip": ip_text }),
                    )?;
                    if locked {
                        if let Some(id) = user_id {
                            sessions::revoke_all_for_user(tx, id)?;
                        }
                        audit::record(
                            tx,
                            now,
                            Actor::Anonymous,
                            action::ACCOUNT_LOCKED,
                            Some(&attempted),
                            json!({ "ip": ip_text, "minutes": users::LOCKOUT.num_minutes() }),
                        )?;
                    }
                    Ok(())
                })
            })
            .await??;
        return Err(ApiError::invalid_credentials());
    }

    let user = user.expect("a user when nothing failed");
    let token = new_token()?;
    let session_token = token.clone();
    let ua = user_agent(&headers);
    let (id, username) = (user.id, user.username.clone());
    state
        .db
        .write(move |conn| {
            in_transaction(conn, |tx| {
                users::record_successful_login(tx, id, now)?;
                sessions::prune_expired(tx, now)?;
                sessions::create(tx, &session_token, id, ua.as_deref(), now)?;
                audit::record(
                    tx,
                    now,
                    Actor::User {
                        id,
                        username: &username,
                    },
                    action::LOGIN,
                    None,
                    json!({ "ip": ip_text }),
                )?;
                Ok(())
            })
        })
        .await??;
    let body = MeResponse::new(&user.username, &user.display_name, &user.permissions);
    Ok(with_session_cookie(&token, Json(body)))
}

async fn logout(State(state): State<AppState>, user: CurrentUser) -> Result<Response, ApiError> {
    let now = Utc::now();
    state
        .db
        .write(move |conn| {
            in_transaction(conn, |tx| {
                sessions::revoke(tx, &user.session_hash)?;
                audit::record(tx, now, user.actor(), action::LOGOUT, None, json!({}))?;
                Ok(())
            })
        })
        .await??;
    let clear = HeaderValue::from_str(&sessions::clear_cookie()).expect("cookie is ASCII");
    Ok((StatusCode::NO_CONTENT, [(header::SET_COOKIE, clear)]).into_response())
}

async fn me(user: CurrentUser) -> Json<MeResponse> {
    Json(MeResponse::new(
        &user.username,
        &user.display_name,
        &user.permissions,
    ))
}

#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

/// Changes one's own password. Needs the current one (a stolen session alone
/// is not enough), ends every other session and starts a new one. Wrong
/// current passwords count towards the account lockout like failed logins.
async fn change_password(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    user: CurrentUser,
    ApiJson(request): ApiJson<ChangePasswordRequest>,
) -> Result<Response, ApiError> {
    let ip = client_ip(&state, peer, &headers);
    state
        .login_limiter
        .attempt(ip, Instant::now())
        .map_err(ApiError::too_many_attempts)?;
    password::check_policy(&request.new_password, &user.username).map_err(ApiError::bad_request)?;

    let id = user.id;
    let stored = state
        .db
        .read(move |conn| users::find_by_id(conn, id))
        .await?
        .ok_or_else(ApiError::unauthenticated)?;
    let current = request.current_password;
    let stored_hash = stored.password_hash.clone();
    if !blocking(move || password::verify(&current, &stored_hash)).await? {
        let now = Utc::now();
        let username = user.username.clone();
        state
            .db
            .write(move |conn| {
                in_transaction(conn, |tx| {
                    if users::record_failed_login(tx, id, now)? {
                        sessions::revoke_all_for_user(tx, id)?;
                        audit::record(
                            tx,
                            now,
                            Actor::User {
                                id,
                                username: &username,
                            },
                            action::ACCOUNT_LOCKED,
                            Some(&username),
                            json!({ "minutes": users::LOCKOUT.num_minutes() }),
                        )?;
                    }
                    Ok(())
                })
            })
            .await??;
        return Err(ApiError::bad_request("The current password is wrong"));
    }

    let new_password = request.new_password;
    let new_hash = blocking(move || password::hash(&new_password))
        .await?
        .map_err(|e| {
            log::error!("{e}");
            ApiError::internal()
        })?;
    let token = new_token()?;
    let session_token = token.clone();
    let ua = user_agent(&headers);
    let now = Utc::now();
    let actor_name = user.username.clone();
    state
        .db
        .write(move |conn| {
            in_transaction(conn, |tx| {
                users::set_password(tx, id, &new_hash, now)?;
                sessions::revoke_all_for_user(tx, id)?;
                sessions::create(tx, &session_token, id, ua.as_deref(), now)?;
                audit::record(
                    tx,
                    now,
                    Actor::User {
                        id,
                        username: &actor_name,
                    },
                    action::PASSWORD_CHANGED,
                    Some(&actor_name),
                    json!({}),
                )?;
                Ok::<_, StoreError>(())
            })
        })
        .await??;
    Ok(with_session_cookie(&token, StatusCode::NO_CONTENT))
}
