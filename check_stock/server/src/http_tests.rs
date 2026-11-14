use super::*;
use crate::db::Db;
use axum::http::Request as HttpRequest;
use check_stock_core::inventory_db::InventoryDb;
use http_body_util::BodyExt;
use tower::ServiceExt;

fn allowlist(extra: &[&str]) -> Arc<HostAllowlist> {
    let extra: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
    Arc::new(HostAllowlist::new(&extra).unwrap())
}

fn state(dir: &std::path::Path) -> AppState {
    AppState::new(
        Db::open(&InventoryDb::in_dir(dir), 1).unwrap(),
        HostAllowlist::new(&["stock.example.com".to_string()]).unwrap(),
        crate::client_ip::TrustedProxies::default(),
    )
    .unwrap()
}

fn req(uri: &str, host: Option<&str>) -> HttpRequest<Body> {
    let mut builder = HttpRequest::builder().uri(uri);
    if let Some(host) = host {
        builder = builder.header(header::HOST, host);
    }
    builder.body(Body::empty()).unwrap()
}

async fn body_text(response: Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

fn assert_security_headers(response: &Response) {
    let headers = response.headers();
    for (name, value) in SECURITY_HEADERS {
        assert_eq!(
            headers.get(name).map(|v| v.to_str().unwrap()),
            Some(value),
            "{name}"
        );
    }
    assert_eq!(headers.get(header::CACHE_CONTROL).unwrap(), "no-store");
}

#[tokio::test]
async fn health_answers_ok_with_security_headers() {
    let dir = tempfile::tempdir().unwrap();
    let response = router(state(dir.path()))
        .oneshot(req("/api/health", Some("localhost:8080")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_security_headers(&response);
    assert_eq!(body_text(response).await, r#"{"status":"ok"}"#);
}

#[tokio::test]
async fn configured_host_names_are_answered() {
    let dir = tempfile::tempdir().unwrap();
    let response = router(state(dir.path()))
        .oneshot(req("/api/health", Some("stock.example.com")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn unknown_or_missing_host_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let app = router(state(dir.path()));
    for host in [
        Some("evil.example"),
        Some("evil.example:8080"),
        Some(""),
        None,
    ] {
        let response = app.clone().oneshot(req("/api/health", host)).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{host:?}");
        assert_security_headers(&response);
        assert_eq!(
            body_text(response).await,
            r#"{"error":{"code":"invalid_host","message":"Unknown host name"}}"#
        );
    }
}

#[tokio::test]
async fn host_in_the_request_line_must_be_allowed_too() {
    let dir = tempfile::tempdir().unwrap();
    let app = router(state(dir.path()));
    let mismatched = req("http://evil.example/api/health", Some("localhost"));
    assert_eq!(
        app.clone().oneshot(mismatched).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
    let only_uri = req("http://localhost/api/health", None);
    assert_eq!(
        app.oneshot(only_uri).await.unwrap().status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn unknown_routes_get_a_json_404() {
    let dir = tempfile::tempdir().unwrap();
    let response = router(state(dir.path()))
        .oneshot(req("/api/nothing-here", Some("localhost")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_security_headers(&response);
    assert_eq!(
        body_text(response).await,
        r#"{"error":{"code":"not_found","message":"Not found"}}"#
    );
}

#[tokio::test]
async fn wrong_method_is_refused_with_headers() {
    let dir = tempfile::tempdir().unwrap();
    // With the CSRF header (without it, any POST is refused before routing).
    let request = HttpRequest::builder()
        .method("POST")
        .uri("/api/health")
        .header(header::HOST, "localhost")
        .header(crate::api::CSRF_HEADER, crate::api::CSRF_VALUE)
        .body(Body::empty())
        .unwrap();
    let response = router(state(dir.path())).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_security_headers(&response);
}

#[tokio::test]
async fn index_is_a_static_placeholder() {
    let dir = tempfile::tempdir().unwrap();
    let response = router(state(dir.path()))
        .oneshot(req("/", Some("localhost")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .starts_with("text/html"));
    assert_security_headers(&response);
    let html = body_text(response).await;
    assert!(html.contains("check_stock server is running"));
    // The CSP forbids inline code; the page must not need any.
    assert!(!html.contains("<script") && !html.contains("<style") && !html.contains("style="));
}

#[tokio::test]
async fn a_panicking_handler_gets_a_plain_500() {
    async fn boom() -> &'static str {
        panic!("secret detail: /home/aron/inventory.db")
    }
    let app = with_middleware(Router::new().route("/boom", get(boom)), allowlist(&[]));
    let response = app.oneshot(req("/boom", Some("localhost"))).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_security_headers(&response);
    let body = body_text(response).await;
    assert_eq!(
        body,
        r#"{"error":{"code":"internal","message":"Internal error, see the server log"}}"#
    );
    assert!(!body.contains("secret"));
}

#[tokio::test]
async fn routes_may_set_their_own_cache_control() {
    async fn cached() -> Response {
        ([(header::CACHE_CONTROL, "public, max-age=60")], "x").into_response()
    }
    let app = with_middleware(Router::new().route("/c", get(cached)), allowlist(&[]));
    let response = app.oneshot(req("/c", Some("localhost"))).await.unwrap();
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "public, max-age=60"
    );
}

#[tokio::test]
async fn failed_health_probe_reveals_nothing() {
    let response = health_response(Err(DbError::Task("disk on fire at /srv".to_string())));
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body_text(response).await, r#"{"status":"unavailable"}"#);
}

#[tokio::test]
async fn database_errors_are_hidden_from_clients() {
    let error: ApiError = DbError::Sqlite(rusqlite::Error::InvalidQuery).into();
    assert_eq!(error, ApiError::internal());
    let body = body_text(error.into_response()).await;
    assert!(!body.to_lowercase().contains("query"), "{body}");
}

#[test]
fn api_error_accessors() {
    let e = ApiError::new(StatusCode::CONFLICT, "conflict", "Changed by someone else");
    assert_eq!(e.status(), StatusCode::CONFLICT);
    assert_eq!(e.code(), "conflict");
    assert_eq!(e.message(), "Changed by someone else");
}
