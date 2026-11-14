use super::*;
use http_body_util::BodyExt;

const FILES: &[(&str, &[u8])] = &[
    ("index.html", b"<!doctype html><title>ui</title>"),
    ("assets/index-abc123.js", b"console.log(1)"),
    ("assets/index-abc123.css", b"body{}"),
    ("favicon.svg", b"<svg/>"),
];

async fn get(files: &[(&str, &'static [u8])], path: &str) -> (StatusCode, String, String, String) {
    let response = respond(files, &Method::GET, &path.parse().unwrap());
    let status = response.status();
    let header = |name| {
        response
            .headers()
            .get(name)
            .map(|v: &HeaderValue| v.to_str().unwrap().to_string())
            .unwrap_or_default()
    };
    let (content_type, cache) = (header(header::CONTENT_TYPE), header(header::CACHE_CONTROL));
    let body = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    (status, content_type, cache, body)
}

#[tokio::test]
async fn hashed_assets_are_cached_for_good() {
    let (status, content_type, cache, body) = get(FILES, "/assets/index-abc123.js").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "text/javascript; charset=utf-8");
    assert_eq!(cache, IMMUTABLE);
    assert_eq!(body, "console.log(1)");
}

#[tokio::test]
async fn other_files_are_revalidated() {
    let (_, content_type, cache, _) = get(FILES, "/favicon.svg").await;
    assert_eq!(content_type, "image/svg+xml");
    assert_eq!(cache, REVALIDATE);
}

#[tokio::test]
async fn app_routes_get_index_html() {
    for path in ["/", "/users", "/audit", "/users/7/edit", "/index.html"] {
        let (status, content_type, cache, body) = get(FILES, path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(content_type.starts_with("text/html"), "{path}");
        assert_eq!(cache, REVALIDATE, "{path}: a new version must show at once");
        assert!(body.contains("<title>ui</title>"), "{path}");
    }
}

#[tokio::test]
async fn unknown_api_paths_stay_json_404() {
    for path in ["/api", "/api/", "/api/nothing", "/api/users/1/nope"] {
        let (status, content_type, _, body) = get(FILES, path).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(content_type, "application/json", "{path}");
        assert!(body.contains("not_found"), "{path}");
    }
    // `/apix` is not under /api.
    assert_eq!(get(FILES, "/apix").await.0, StatusCode::OK);
}

#[tokio::test]
async fn paths_never_reach_outside_the_embedded_files() {
    for path in [
        "/../Cargo.toml",
        "/assets/../../build.rs",
        "/%2e%2e/%2e%2e/etc/passwd",
        "//etc/passwd",
        "/assets/index-abc123.js/../../index.html",
    ] {
        let (status, content_type, _, body) = get(FILES, path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(content_type.starts_with("text/html"), "{path}");
        assert!(
            body.contains("<title>ui</title>"),
            "{path} must fall back to the app"
        );
    }
}

#[tokio::test]
async fn only_get_and_head_are_served() {
    let response = respond(FILES, &Method::POST, &"/users".parse().unwrap());
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    let head = respond(FILES, &Method::HEAD, &"/users".parse().unwrap());
    assert_eq!(head.status(), StatusCode::OK);
}

#[tokio::test]
async fn without_a_built_ui_a_hint_page_is_shown() {
    let (status, content_type, _, body) = get(&[], "/users").await;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/html"));
    assert!(body.contains("./dev.sh"), "{body}");
    assert!(
        !body.contains("<script") && !body.contains("style="),
        "CSP: no inline code"
    );
}

#[test]
fn content_types() {
    assert_eq!(content_type("a.HTML"), "text/html; charset=utf-8");
    assert_eq!(content_type("x/y.mjs"), "text/javascript; charset=utf-8");
    assert_eq!(content_type("f.woff2"), "font/woff2");
    assert_eq!(content_type("noext"), "application/octet-stream");
    assert_eq!(content_type("weird.exe"), "application/octet-stream");
}

#[test]
fn embedded_files_have_clean_relative_paths() {
    // Whatever the current build contains: relative, forward slashes, no `..`.
    for (name, _) in embedded_files() {
        assert!(
            !name.starts_with('/') && !name.contains('\\') && !name.contains(".."),
            "{name}"
        );
    }
}
