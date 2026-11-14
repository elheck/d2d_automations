//! Serves the web UI that `build.rs` embedded into the binary.
//!
//! Every path that is not under `/api` belongs to the UI: an embedded file is
//! served as is, anything else gets `index.html`, so the browser-side router
//! can show `/users` etc. Lookups are exact matches in the embedded list,
//! never file system paths, so `../` tricks have nothing to reach.
//!
//! Files under `assets/` carry a content hash in their name (Vite) and may be
//! cached for good; `index.html` must be revalidated so a new version is
//! picked up at once.

use crate::http::ApiError;
use axum::http::{header, HeaderValue, Method, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));
}

/// The embedded UI files.
pub fn embedded_files() -> &'static [(&'static str, &'static [u8])] {
    embedded::FILES
}

/// Cache rule for content-hashed files.
pub const IMMUTABLE: &str = "public, max-age=31536000, immutable";
/// Cache rule for everything else: always ask the server.
pub const REVALIDATE: &str = "no-cache";

/// `Content-Type` by file extension.
pub fn content_type(path: &str) -> &'static str {
    let extension = path
        .rsplit_once('.')
        .map(|(_, ext)| ext)
        .unwrap_or_default();
    match extension.to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Answer for a request that matched no API route.
pub fn respond(files: &[(&str, &'static [u8])], method: &Method, uri: &Uri) -> Response {
    let path = uri.path();
    if path == "/api" || path.starts_with("/api/") {
        return ApiError::not_found().into_response();
    }
    if method != Method::GET && method != Method::HEAD {
        return ApiError::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "Method not allowed",
        )
        .into_response();
    }
    let find = |name: &str| files.iter().find(|(n, _)| *n == name).map(|(_, b)| *b);
    let name = path.trim_start_matches('/');
    if let Some(bytes) = (!name.is_empty()).then(|| find(name)).flatten() {
        let cache = if name.starts_with("assets/") {
            IMMUTABLE
        } else {
            REVALIDATE
        };
        return file(bytes, content_type(name), cache);
    }
    match find("index.html") {
        Some(index) => file(index, content_type("index.html"), REVALIDATE),
        None => Html(include_str!("placeholder.html")).into_response(),
    }
}

fn file(bytes: &'static [u8], content_type: &'static str, cache: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
        ],
        bytes,
    )
        .into_response()
}

/// Fallback handler of the router.
pub async fn fallback(method: Method, uri: Uri) -> Response {
    respond(embedded_files(), &method, &uri)
}

#[cfg(test)]
#[path = "web_tests.rs"]
mod tests;
