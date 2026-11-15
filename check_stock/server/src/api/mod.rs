//! The JSON API under `/api`.
//!
//! Every route is registered through [`Api::route`] together with who may
//! call it ([`Access`]); there is no other way to add an `/api` route. The
//! access check runs as a layer on the route itself, before the handler, and
//! [`route_table`] lists every route so a test can prove that each one
//! refuses anonymous callers and callers without the permission.
//!
//! Also here: the CSRF check for state-changing requests, the body size limit
//! and [`ApiJson`], which turns malformed request bodies into the usual error
//! format.

mod audit_log;
mod auth;
mod buy_helper;
mod precon_ev;
mod singles;
mod users;

use crate::app::AppState;
use crate::config::HostAllowlist;
use crate::http::{health_response, ApiError};
use crate::permissions::{Permission, PermissionSet};
use crate::sessions::{self, SessionStatus};
use crate::users::StoreError;
use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, FromRequest, FromRequestParts, Query, Request, State};
use axum::http::request::Parts;
use axum::http::{header, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, MethodRouter};
use axum::{Json, Router};
use rusqlite::{Connection, Transaction};
use std::sync::Arc;

/// Largest accepted request body. Upload routes will raise it for themselves.
pub const BODY_LIMIT: usize = 64 * 1024;

/// Header every state-changing request must carry. Browsers only let pages of
/// the same site set custom headers (anything else needs a CORS preflight,
/// which this server never allows), so a foreign page cannot forge requests.
pub const CSRF_HEADER: &str = "x-check-stock-csrf";
pub const CSRF_VALUE: &str = "1";

/// Who may call a route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// Anyone (health check, login).
    Public,
    /// Any logged-in user.
    LoggedIn,
    /// Logged-in users with this permission.
    Permission(Permission),
}

/// One registered route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteInfo {
    pub path: &'static str,
    pub access: Access,
}

/// The logged-in user of a request (set by the access check).
#[derive(Clone, Debug)]
pub struct CurrentUser {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub permissions: PermissionSet,
    /// Hash of this request's session token (logout ends exactly this one).
    pub session_hash: Vec<u8>,
}

impl CurrentUser {
    pub fn actor(&self) -> crate::audit::Actor<'_> {
        crate::audit::Actor::User {
            id: self.id,
            username: &self.username,
        }
    }
}

impl<S: Send + Sync> FromRequestParts<S> for CurrentUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, ApiError> {
        // Only present behind the access check; a public route asking for it
        // is a programming error and must not be served as anonymous.
        parts
            .extensions
            .get::<CurrentUser>()
            .cloned()
            .ok_or_else(ApiError::unauthenticated)
    }
}

/// Collects routes together with their access rule.
pub struct Api {
    state: AppState,
    router: Router<AppState>,
    routes: Vec<RouteInfo>,
}

impl Api {
    fn new(state: AppState) -> Self {
        Self {
            state,
            router: Router::new(),
            routes: Vec::new(),
        }
    }

    /// Adds `handler` at `path`, callable only as `access` allows.
    fn route(
        mut self,
        path: &'static str,
        access: Access,
        handler: MethodRouter<AppState>,
    ) -> Self {
        let handler = match access {
            Access::Public => handler,
            _ => handler.route_layer(middleware::from_fn_with_state(
                (self.state.clone(), access),
                require_access,
            )),
        };
        self.router = self.router.route(path, handler);
        self.routes.push(RouteInfo { path, access });
        self
    }
}

/// Every API route. The single place where routes are added.
fn api(state: AppState) -> Api {
    let api = Api::new(state).route("/api/health", Access::Public, get(health));
    let api = auth::routes(api);
    let api = users::routes(api);
    let api = audit_log::routes(api);
    let api = singles::routes(api);
    let api = buy_helper::routes(api);
    precon_ev::routes(api)
}

/// The API router with CSRF check and body limit.
pub fn router(state: AppState) -> Router {
    let allowed_hosts = state.allowed_hosts.clone();
    let api = api(state.clone());
    api.router
        .layer(middleware::from_fn_with_state(allowed_hosts, check_csrf))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(state)
}

/// Every API route with its access rule.
pub fn route_table(state: AppState) -> Vec<RouteInfo> {
    api(state).routes
}

/// `GET /api/health`: says only whether the database answers.
async fn health(State(state): State<AppState>) -> Response {
    let result = state
        .db
        .read(|conn| conn.query_row("SELECT 1", [], |_| Ok(())))
        .await;
    health_response(result)
}

/// The access check in front of every non-public route: a valid session,
/// an enabled account and (if required) the permission.
async fn require_access(
    State((state, access)): State<(AppState, Access)>,
    mut request: Request,
    next: Next,
) -> Response {
    // Only the token is taken from the request: holding the request itself
    // across the database calls would make this future non-`Send`.
    let token = sessions::token_from_headers(request.headers());
    match authorize(&state, access, token).await {
        Ok(user) => {
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Err(e) => e.into_response(),
    }
}

async fn authorize(
    state: &AppState,
    access: Access,
    token: Option<sessions::SessionToken>,
) -> Result<CurrentUser, ApiError> {
    let token = token.ok_or_else(ApiError::unauthenticated)?;
    let hash = token.hash();
    let now = chrono::Utc::now();
    let lookup_hash = hash.clone();
    let found = state
        .db
        .read(move |conn| {
            let Some(session) = sessions::find(conn, &lookup_hash)? else {
                return Ok(None);
            };
            let user = crate::users::find_by_id(conn, session.user_id)?;
            Ok(Some((session, user)))
        })
        .await?;
    let Some((session, user)) = found else {
        return Err(ApiError::unauthenticated());
    };
    let user = match (session.status(now), user) {
        (SessionStatus::Valid { needs_touch }, Some(user)) if !user.disabled => {
            if needs_touch {
                let touch_hash = hash.clone();
                if let Err(e) = state
                    .db
                    .write(move |conn| sessions::touch(conn, &touch_hash, now))
                    .await
                {
                    log::warn!("Could not record session activity: {e}");
                }
            }
            user
        }
        _ => {
            // Expired, user gone or disabled: the session is over for good.
            let revoke_hash = hash.clone();
            state
                .db
                .write(move |conn| sessions::revoke(conn, &revoke_hash))
                .await?;
            return Err(ApiError::unauthenticated());
        }
    };
    if let Access::Permission(permission) = access {
        if !user.can(permission) {
            return Err(ApiError::forbidden(permission));
        }
    }
    Ok(CurrentUser {
        id: user.id,
        username: user.username,
        display_name: user.display_name,
        permissions: user.permissions,
        session_hash: hash,
    })
}

/// Refuses state-changing requests that did not come from this site's own
/// pages: they need [`CSRF_HEADER`], and the browser's `Origin` and
/// `Sec-Fetch-Site` (when sent) must point to this site. Together with the
/// `SameSite=Strict` session cookie this stops cross-site request forgery,
/// including forged logins.
async fn check_csrf(
    State(allowed): State<Arc<HostAllowlist>>,
    request: Request,
    next: Next,
) -> Response {
    if is_safe_method(request.method()) || csrf_ok(request.headers(), &allowed) {
        next.run(request).await
    } else {
        ApiError::csrf().into_response()
    }
}

fn is_safe_method(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn csrf_ok(headers: &axum::http::HeaderMap, allowed: &HostAllowlist) -> bool {
    let has_token = headers
        .get(CSRF_HEADER)
        .is_some_and(|v| v.as_bytes() == CSRF_VALUE.as_bytes());
    let origin_ok = match headers.get(header::ORIGIN).map(|v| v.to_str()) {
        None => true,
        Some(Ok(origin)) => origin_host(origin).is_some_and(|host| allowed.allows(host)),
        Some(Err(_)) => false,
    };
    let fetch_site_ok = match headers.get("sec-fetch-site").map(|v| v.to_str()) {
        None => true,
        Some(Ok(site)) => site == "same-origin",
        Some(Err(_)) => false,
    };
    has_token && origin_ok && fetch_site_ok
}

/// `http://host:port` → `host:port`. `None` for `null` or anything else.
fn origin_host(origin: &str) -> Option<&str> {
    let rest = origin
        .strip_prefix("https://")
        .or_else(|| origin.strip_prefix("http://"))?;
    (!rest.is_empty() && !rest.contains('/')).then_some(rest)
}

/// JSON body extractor whose errors use the API's error format (and never
/// echo the parser's message, which can quote the input).
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, ApiError> {
        match Json::<T>::from_request(request, state).await {
            Ok(Json(value)) => Ok(ApiJson(value)),
            Err(rejection) => Err(if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
                ApiError::new(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "too_large",
                    "The request is too large",
                )
            } else {
                ApiError::new(
                    rejection.status(),
                    "invalid_request",
                    "The request body is not valid for this endpoint",
                )
            }),
        }
    }
}

/// Query string extractor whose errors use the API's error format (and never
/// echo the parser's message, which can quote the input).
pub struct ApiQuery<T>(pub T);

impl<S, T> FromRequestParts<S> for ApiQuery<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        match Query::<T>::from_request_parts(parts, state).await {
            Ok(Query(value)) => Ok(ApiQuery(value)),
            Err(_) => Err(ApiError::bad_request(
                "The query parameters are not valid for this endpoint",
            )),
        }
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::UsernameTaken => ApiError::conflict("username_taken", e.to_string()),
            StoreError::NotFound => {
                ApiError::new(StatusCode::NOT_FOUND, "not_found", "No such user")
            }
            StoreError::LastAdmin => ApiError::conflict("last_admin", e.to_string()),
            StoreError::Sqlite(e) => {
                log::error!("database error: {e}");
                ApiError::internal()
            }
        }
    }
}

/// Runs `f` in a transaction on the writer: commits on `Ok`, rolls back on
/// any error. Domain errors come back as the inner `Err`, database errors as
/// the outer one.
pub fn in_transaction<T>(
    conn: &mut Connection,
    f: impl FnOnce(&Transaction<'_>) -> Result<T, StoreError>,
) -> rusqlite::Result<Result<T, StoreError>> {
    let tx = conn.transaction()?;
    match f(&tx) {
        Ok(value) => {
            tx.commit()?;
            Ok(Ok(value))
        }
        Err(StoreError::Sqlite(e)) => Err(e),
        Err(other) => Ok(Err(other)),
    }
}

/// `blocking` on a blocking thread; a panic there becomes a 500.
pub async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f).await.map_err(|e| {
        log::error!("blocking task failed: {e}");
        ApiError::internal()
    })
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
