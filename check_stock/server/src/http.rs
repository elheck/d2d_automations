//! HTTP layer: routes, middleware and the error format.
//!
//! Middleware, outermost first:
//! 1. security headers on **every** response (also rejections and panics)
//! 2. panics become a plain 500 without details
//! 3. the `Host` check (DNS-rebinding protection)
//! 4. a timeout for every request
//!
//! HSTS is added by Caddy, which terminates TLS; this server speaks plain
//! HTTP behind it (or on `localhost` during development).

use crate::app::AppState;
use crate::config::HostAllowlist;
use crate::db::DbError;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderName, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use std::any::Any;
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;

/// Longest a request may take before it is answered with 408.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Content Security Policy: only this origin, no inline code, no framing.
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; \
    style-src 'self'; img-src 'self'; font-src 'self'; connect-src 'self'; \
    object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

/// Headers set on every response.
pub const SECURITY_HEADERS: [(&str, &str); 7] = [
    ("content-security-policy", CONTENT_SECURITY_POLICY),
    ("x-content-type-options", "nosniff"),
    ("x-frame-options", "DENY"),
    ("referrer-policy", "no-referrer"),
    ("cross-origin-opener-policy", "same-origin"),
    ("cross-origin-resource-policy", "same-origin"),
    (
        "permissions-policy",
        "camera=(), microphone=(), geolocation=(), payment=(), usb=()",
    ),
];

/// Business data must not end up in browser or proxy caches. Routes that
/// serve static files may set their own `cache-control` (only added when
/// missing).
pub const DEFAULT_CACHE_CONTROL: &str = "no-store";

/// The router with all routes and middleware.
pub fn router(state: AppState) -> Router {
    let allowed_hosts = state.allowed_hosts.clone();
    let routes = Router::new()
        .route("/", get(index))
        .route("/api/health", get(health))
        .with_state(state);
    with_middleware(routes, allowed_hosts)
}

/// Adds the 404 fallback and the middleware stack to `routes`. Separate from
/// [`router`] so tests can wrap routes of their own.
pub fn with_middleware(routes: Router, allowed_hosts: Arc<HostAllowlist>) -> Router {
    let mut router = routes
        .fallback(not_found)
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            REQUEST_TIMEOUT,
        ))
        .layer(middleware::from_fn_with_state(allowed_hosts, check_host))
        .layer(CatchPanicLayer::custom(panic_response));
    for (name, value) in SECURITY_HEADERS {
        router = router.layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        ));
    }
    router.layer(SetResponseHeaderLayer::if_not_present(
        header::CACHE_CONTROL,
        HeaderValue::from_static(DEFAULT_CACHE_CONTROL),
    ))
}

/// Refuses requests for any host name that is not on the allowlist. Both the
/// `Host` header and, for absolute-form requests, the authority in the
/// request line must be allowed.
async fn check_host(
    State(allowed): State<Arc<HostAllowlist>>,
    request: Request,
    next: Next,
) -> Response {
    let header_host = request.headers().get(header::HOST).map(|v| v.to_str());
    let uri_host = request.uri().authority().map(|a| a.as_str());
    let allowed_request = match (header_host, uri_host) {
        (None, None) => false,
        (Some(Err(_)), _) => false,
        (Some(Ok(h)), None) => allowed.allows(h),
        (None, Some(u)) => allowed.allows(u),
        (Some(Ok(h)), Some(u)) => allowed.allows(h) && allowed.allows(u),
    };
    if allowed_request {
        next.run(request).await
    } else {
        ApiError::invalid_host().into_response()
    }
}

/// Placeholder page until the web UI arrives (step 1c).
async fn index() -> Html<&'static str> {
    Html(include_str!("placeholder.html"))
}

/// `GET /api/health`: the only route without a login. Says only whether the
/// database answers; no version, no counts.
async fn health(State(state): State<AppState>) -> Response {
    let result = state
        .db
        .read(|conn| conn.query_row("SELECT 1", [], |_| Ok(())))
        .await;
    health_response(result)
}

/// Body of the health check for a database probe result.
pub fn health_response(result: Result<(), DbError>) -> Response {
    match result {
        Ok(()) => Json(HealthBody { status: "ok" }).into_response(),
        Err(e) => {
            log::error!("Health check failed: {e}");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthBody {
                    status: "unavailable",
                }),
            )
                .into_response()
        }
    }
}

#[derive(Serialize)]
struct HealthBody {
    status: &'static str,
}

async fn not_found() -> ApiError {
    ApiError::not_found()
}

/// A panicking handler is logged with its message; the client only gets a 500.
fn panic_response(panic: Box<dyn Any + Send + 'static>) -> Response<Body> {
    let message = panic
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string());
    log::error!("Request handler panicked: {message}");
    ApiError::internal().into_response()
}

/// Error returned to clients: a status, a stable machine-readable `code` and
/// a short message. Never contains internal details (SQL, paths, panics);
/// those go to the server log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: Cow<'static, str>,
}

impl ApiError {
    pub fn new(
        status: StatusCode,
        code: &'static str,
        message: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "Not found")
    }

    pub fn invalid_host() -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_host", "Unknown host name")
    }

    pub fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "Internal error, see the server log",
        )
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: ErrorDetail<'a>,
}

#[derive(Serialize)]
struct ErrorDetail<'a> {
    code: &'a str,
    message: &'a str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: ErrorDetail {
                code: self.code,
                message: &self.message,
            },
        };
        (self.status, Json(body)).into_response()
    }
}

impl From<DbError> for ApiError {
    /// Logs the database error and hides it from the client.
    fn from(e: DbError) -> Self {
        log::error!("{e}");
        ApiError::internal()
    }
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
