//! End-to-end tests of the API through the real router and middleware.

use super::*;
use crate::cards::CardLookup;
use crate::client_ip::TrustedProxies;
use crate::db::Db;
use crate::password;
use crate::users;
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{HeaderValue, Request as HttpRequest};
use check_stock_core::inventory_db::InventoryDb;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::net::SocketAddr;
use tower::ServiceExt;

pub(super) const PASSWORD: &str = "a long enough password";

pub(super) struct TestApp {
    pub(super) dir: tempfile::TempDir,
    pub(super) state: AppState,
}

/// Card lookups that never leave the machine: nothing listens on port 9, and
/// no image URL is allowed.
pub(super) fn offline_cards(cache_root: &std::path::Path) -> CardLookup {
    CardLookup::with_endpoints(
        cache_root,
        "http://127.0.0.1:9",
        |_| false,
        std::time::Duration::ZERO,
    )
}

impl TestApp {
    pub(super) fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&InventoryDb::in_dir(dir.path()), 2).unwrap();
        let state = AppState::new(
            db,
            HostAllowlist::new(&[]).unwrap(),
            TrustedProxies::default(),
        )
        .unwrap()
        .with_cards(offline_cards(&dir.path().join("cache")));
        Self { dir, state }
    }

    /// Changes the state (fake Scryfall, price tracker, …).
    pub(super) fn with(mut self, f: impl FnOnce(AppState, &std::path::Path) -> AppState) -> Self {
        self.state = f(self.state, self.dir.path());
        self
    }

    /// The full router, as if the request came from `peer`.
    pub(super) fn router_from(&self, peer: [u8; 4]) -> Router {
        crate::http::router(self.state.clone())
            .layer(MockConnectInfo(SocketAddr::from((peer, 40000))))
    }

    pub(super) fn router(&self) -> Router {
        self.router_from([192, 168, 178, 30])
    }

    pub(super) async fn add_user(&self, name: &str, keys: &[&str]) -> i64 {
        let granted = PermissionSet::from_keys(keys).unwrap();
        let hash = password::hash(PASSWORD).unwrap();
        let name = name.to_string();
        self.state
            .db
            .write(move |conn| {
                Ok(users::create(conn, &name, &name, &hash, &granted, chrono::Utc::now()).unwrap())
            })
            .await
            .unwrap()
    }

    pub(super) async fn admin(&self) -> (i64, String) {
        let id = self.add_user("admin", &all_keys()).await;
        let cookie = login_ok(&self.router(), "admin", PASSWORD).await;
        (id, cookie)
    }

    pub(super) async fn audit_actions(&self) -> Vec<(String, String, Value)> {
        let entries = self
            .state
            .db
            .read(|conn| crate::audit::list(conn, None, 200))
            .await
            .unwrap();
        entries
            .into_iter()
            .map(|e| (e.action, e.target.unwrap_or_default(), e.details))
            .collect()
    }
}

fn all_keys() -> Vec<&'static str> {
    Permission::ALL.iter().map(|p| p.key()).collect()
}

/// A request the way the web UI sends it: same host, CSRF header, JSON.
pub(super) fn request(
    method: Method,
    path: &str,
    body: Option<Value>,
    cookie: Option<&str>,
) -> HttpRequest<Body> {
    let mut builder = HttpRequest::builder()
        .method(method.clone())
        .uri(path)
        .header(header::HOST, "localhost:8080");
    if !is_safe_method(&method) {
        builder = builder.header(CSRF_HEADER, CSRF_VALUE);
    }
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    match body {
        Some(body) => builder
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

pub(super) async fn send(
    router: &Router,
    request: HttpRequest<Body>,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()))
    };
    (status, headers, body)
}

/// The `name=value` part of the session cookie in a response.
fn session_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with(sessions::COOKIE_NAME) && !v.contains("Max-Age=0"))
        .map(|v| v.split(';').next().unwrap().to_string())
}

async fn login(
    router: &Router,
    user: &str,
    pw: &str,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    send(
        router,
        request(
            Method::POST,
            "/api/auth/login",
            Some(json!({ "username": user, "password": pw })),
            None,
        ),
    )
    .await
}

pub(super) async fn login_ok(router: &Router, user: &str, pw: &str) -> String {
    let (status, headers, body) = login(router, user, pw).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    session_cookie(&headers).expect("session cookie")
}

/// Substitutes path parameters for the route test.
fn concrete(path: &str) -> String {
    path.replace("{id}", "1")
        .replace("{set}", "hou")
        .replace("{number}", "120")
        .replace("{product}", "101")
        .replace("{lot}", "L1")
        .replace("{pile}", "0123456789abcdef")
}

const METHODS: [Method; 5] = [
    Method::GET,
    Method::POST,
    Method::PUT,
    Method::PATCH,
    Method::DELETE,
];

// ── Every route declares and enforces its access ───────────────────────────

#[tokio::test]
async fn only_health_and_login_are_public() {
    let app = TestApp::new();
    let public: Vec<&str> = route_table(app.state.clone())
        .iter()
        .filter(|r| r.access == Access::Public)
        .map(|r| r.path)
        .collect();
    assert_eq!(public, ["/api/health", "/api/auth/login"]);
}

#[tokio::test]
async fn every_route_refuses_anonymous_callers() {
    let app = TestApp::new();
    let router = app.router();
    for route in route_table(app.state.clone())
        .iter()
        .filter(|r| r.access != Access::Public)
    {
        let mut served = false;
        for method in METHODS {
            let (status, _, body) = send(
                &router,
                request(method.clone(), &concrete(route.path), Some(json!({})), None),
            )
            .await;
            match status {
                StatusCode::METHOD_NOT_ALLOWED => {}
                StatusCode::UNAUTHORIZED => {
                    served = true;
                    assert_eq!(body["error"]["code"], "unauthenticated");
                }
                other => panic!("{method} {} answered {other} without a session", route.path),
            }
        }
        assert!(served, "{} serves no method", route.path);
    }
}

#[tokio::test]
async fn every_permission_route_refuses_users_without_it() {
    let app = TestApp::new();
    app.add_user("nobody", &[]).await;
    let router = app.router();
    let cookie = login_ok(&router, "nobody", PASSWORD).await;
    for route in route_table(app.state.clone()) {
        let Access::Permission(permission) = route.access else {
            continue;
        };
        let mut served = false;
        for method in METHODS {
            let (status, _, body) = send(
                &router,
                request(
                    method.clone(),
                    &concrete(route.path),
                    Some(json!({})),
                    Some(&cookie),
                ),
            )
            .await;
            match status {
                StatusCode::METHOD_NOT_ALLOWED => {}
                StatusCode::FORBIDDEN => {
                    served = true;
                    assert_eq!(body["error"]["code"], "forbidden");
                    assert!(body["error"]["message"]
                        .as_str()
                        .unwrap()
                        .contains(permission.label()));
                }
                other => panic!(
                    "{method} {} answered {other} without {permission:?}",
                    route.path
                ),
            }
        }
        assert!(served, "{} serves no method", route.path);
    }
}

#[tokio::test]
async fn users_with_the_permission_get_through() {
    let app = TestApp::new();
    let (_, cookie) = app.admin().await;
    let router = app.router();
    for route in route_table(app.state.clone())
        .iter()
        .filter(|r| r.access != Access::Public)
    {
        let (status, _, body) = send(
            &router,
            request(Method::GET, &concrete(route.path), None, Some(&cookie)),
        )
        .await;
        assert!(
            !matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN),
            "GET {} refused an admin: {status} {body}",
            route.path
        );
    }
}

// ── CSRF ───────────────────────────────────────────────────────────────────

fn login_request(extra: &[(&str, &str)], with_csrf: bool) -> HttpRequest<Body> {
    let mut builder = HttpRequest::builder()
        .method(Method::POST)
        .uri("/api/auth/login")
        .header(header::HOST, "localhost:8080")
        .header(header::CONTENT_TYPE, "application/json");
    if with_csrf {
        builder = builder.header(CSRF_HEADER, CSRF_VALUE);
    }
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    builder
        .body(Body::from(
            json!({ "username": "x", "password": "y" }).to_string(),
        ))
        .unwrap()
}

#[tokio::test]
async fn state_changing_requests_need_the_csrf_header() {
    let app = TestApp::new();
    let router = app.router();
    let (status, _, body) = send(&router, login_request(&[], false)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "csrf");
    let (status, _, _) = send(&router, login_request(&[], true)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "passes the CSRF check");
}

#[tokio::test]
async fn foreign_origins_are_refused() {
    let app = TestApp::new();
    let router = app.router();
    for headers in [
        vec![("origin", "https://evil.example")],
        vec![("origin", "null")],
        vec![("origin", "http://localhost.evil.example")],
        vec![("sec-fetch-site", "cross-site")],
        vec![("sec-fetch-site", "same-site")],
        vec![
            ("origin", "http://localhost:8080"),
            ("sec-fetch-site", "cross-site"),
        ],
    ] {
        let (status, _, body) = send(&router, login_request(&headers, true)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{headers:?}");
        assert_eq!(body["error"]["code"], "csrf");
    }
    let same_site = [
        ("origin", "http://localhost:8080"),
        ("sec-fetch-site", "same-origin"),
    ];
    let (status, _, _) = send(&router, login_request(&same_site, true)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "same origin passes");
}

#[test]
fn origin_hosts() {
    assert_eq!(origin_host("http://localhost:5173"), Some("localhost:5173"));
    assert_eq!(
        origin_host("https://stock.example.com"),
        Some("stock.example.com")
    );
    assert_eq!(origin_host("null"), None);
    assert_eq!(origin_host("http://"), None);
    assert_eq!(origin_host("http://a.example/path"), None);
    assert_eq!(origin_host("ftp://a.example"), None);
}

// ── Login and sessions ─────────────────────────────────────────────────────

#[tokio::test]
async fn login_sets_a_protected_cookie_and_me_works() {
    let app = TestApp::new();
    app.add_user("anna", &["search.use"]).await;
    let router = app.router();
    let (status, headers, body) = login(&router, "  ANNA ", PASSWORD).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({ "username": "anna", "display_name": "anna", "permissions": ["search.use"] })
    );
    let raw = headers.get(header::SET_COOKIE).unwrap().to_str().unwrap();
    for attribute in [
        "__Host-check_stock_session=",
        "HttpOnly",
        "Secure",
        "SameSite=Strict",
        "Path=/",
    ] {
        assert!(raw.contains(attribute), "{raw}");
    }

    let cookie = session_cookie(&headers).unwrap();
    let (status, _, me) = send(
        &router,
        request(Method::GET, "/api/auth/me", None, Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me, body);

    let audit = app.audit_actions().await;
    assert_eq!(audit[0].0, "auth.login");
    assert_eq!(audit[0].2["ip"], "192.168.178.30");
}

#[tokio::test]
async fn failures_all_look_the_same_but_are_logged_with_the_reason() {
    let app = TestApp::new();
    app.add_user("anna", &[]).await;
    let router = app.router();
    let (s1, _, wrong) = login(&router, "anna", "not the password").await;
    let (s2, _, unknown) = login(&router, "nobody", PASSWORD).await;
    assert_eq!(
        (s1, s2),
        (StatusCode::UNAUTHORIZED, StatusCode::UNAUTHORIZED)
    );
    assert_eq!(wrong, unknown);
    assert_eq!(wrong["error"]["code"], "invalid_credentials");

    let reasons: Vec<Value> = app
        .audit_actions()
        .await
        .into_iter()
        .filter(|(a, _, _)| a == "auth.login_failed")
        .map(|(_, _, d)| d["reason"].clone())
        .collect();
    assert_eq!(reasons, [json!("unknown_user"), json!("wrong_password")]);
}

#[tokio::test]
async fn five_wrong_passwords_lock_the_account() {
    let app = TestApp::new();
    app.add_user("anna", &[]).await;
    let router = app.router();
    for _ in 0..users::MAX_FAILED_LOGINS {
        login(&router, "anna", "not the password").await;
    }
    let (status, _, body) = login(&router, "anna", PASSWORD).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "locked even with the right password"
    );
    assert_eq!(body["error"]["code"], "invalid_credentials");
    let actions: Vec<String> = app
        .audit_actions()
        .await
        .into_iter()
        .map(|(a, _, _)| a)
        .collect();
    assert!(actions.contains(&"auth.account_locked".to_string()));
    assert_eq!(
        actions[0], "auth.login_failed",
        "the last try was refused as locked"
    );
}

#[tokio::test]
async fn too_many_attempts_from_one_address_are_refused() {
    let app = TestApp::new();
    let router = app.router();
    for _ in 0..crate::rate_limit::MAX_LOGIN_ATTEMPTS {
        let (status, _, _) = login(&router, "nobody", "whatever it is").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, headers, body) = login(&router, "nobody", "whatever it is").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"]["code"], "too_many_attempts");
    assert!(headers.get(header::RETRY_AFTER).is_some());

    // Another address is not affected.
    let other = app.router_from([192, 168, 178, 31]);
    let (status, _, _) = login(&other, "nobody", "whatever it is").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_ends_the_session() {
    let app = TestApp::new();
    app.add_user("anna", &[]).await;
    let router = app.router();
    let cookie = login_ok(&router, "anna", PASSWORD).await;
    let (status, headers, _) = send(
        &router,
        request(Method::POST, "/api/auth/logout", None, Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .contains("Max-Age=0"));
    let (status, _, _) = send(
        &router,
        request(Method::GET, "/api/auth/me", None, Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_idle_session_expires_and_is_deleted() {
    let app = TestApp::new();
    app.add_user("anna", &[]).await;
    let router = app.router();
    let cookie = login_ok(&router, "anna", PASSWORD).await;
    app.state
        .db
        .write(|conn| {
            conn.execute(
                "UPDATE srv_sessions SET last_seen_at = '2020-01-01T00:00:00Z'",
                [],
            )
        })
        .await
        .unwrap();
    let (status, headers, _) = send(
        &router,
        request(Method::GET, "/api/auth/me", None, Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(headers[header::SET_COOKIE]
        .to_str()
        .unwrap()
        .contains("Max-Age=0"));
    let left: i64 = app
        .state
        .db
        .read(|conn| conn.query_row("SELECT COUNT(*) FROM srv_sessions", [], |r| r.get(0)))
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn forged_or_unknown_cookies_are_refused() {
    let app = TestApp::new();
    let router = app.router();
    let fake = format!("{}={}", sessions::COOKIE_NAME, "a".repeat(64));
    for cookie in [
        fake.as_str(),
        "__Host-check_stock_session=not-a-token",
        "other=1",
    ] {
        let (status, _, _) = send(
            &router,
            request(Method::GET, "/api/auth/me", None, Some(cookie)),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{cookie}");
    }
}

#[tokio::test]
async fn disabled_users_cannot_log_in_and_lose_their_sessions() {
    let app = TestApp::new();
    let (_, admin) = app.admin().await;
    let anna = app.add_user("anna", &[]).await;
    let router = app.router();
    let anna_cookie = login_ok(&router, "anna", PASSWORD).await;

    let (status, _, body) = send(
        &router,
        request(
            Method::PATCH,
            &format!("/api/users/{anna}"),
            Some(json!({ "disabled": true })),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["disabled"], true);
    let (status, _, _) = send(
        &router,
        request(Method::GET, "/api/auth/me", None, Some(&anna_cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = login(&router, "anna", PASSWORD).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn permission_changes_apply_at_once() {
    let app = TestApp::new();
    let (_, admin) = app.admin().await;
    let anna = app.add_user("anna", &[]).await;
    let router = app.router();
    let anna_cookie = login_ok(&router, "anna", PASSWORD).await;
    let audit = request(Method::GET, "/api/audit", None, Some(&anna_cookie));
    assert_eq!(send(&router, audit).await.0, StatusCode::FORBIDDEN);

    send(
        &router,
        request(
            Method::PATCH,
            &format!("/api/users/{anna}"),
            Some(json!({ "permissions": ["audit.view"] })),
            Some(&admin),
        ),
    )
    .await;
    let audit = request(Method::GET, "/api/audit", None, Some(&anna_cookie));
    assert_eq!(
        send(&router, audit).await.0,
        StatusCode::OK,
        "no new login needed"
    );
}

// ── Own password ───────────────────────────────────────────────────────────

#[tokio::test]
async fn changing_the_password_needs_the_old_one_and_ends_other_sessions() {
    let app = TestApp::new();
    app.add_user("anna", &[]).await;
    let router = app.router();
    let first = login_ok(&router, "anna", PASSWORD).await;
    let second = login_ok(&router, "anna", PASSWORD).await;
    let new_password = "an even longer new password";

    let wrong = json!({ "current_password": "not it at all", "new_password": new_password });
    let (status, _, body) = send(
        &router,
        request(
            Method::POST,
            "/api/auth/password",
            Some(wrong),
            Some(&first),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let weak = json!({ "current_password": PASSWORD, "new_password": "short" });
    let (status, _, _) = send(
        &router,
        request(Method::POST, "/api/auth/password", Some(weak), Some(&first)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let ok = json!({ "current_password": PASSWORD, "new_password": new_password });
    let (status, headers, _) = send(
        &router,
        request(Method::POST, "/api/auth/password", Some(ok), Some(&first)),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let renewed = session_cookie(&headers).expect("a new session for this browser");

    for old in [&first, &second] {
        let (status, _, _) = send(
            &router,
            request(Method::GET, "/api/auth/me", None, Some(old)),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "old sessions end");
    }
    let (status, _, _) = send(
        &router,
        request(Method::GET, "/api/auth/me", None, Some(&renewed)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        login(&router, "anna", PASSWORD).await.0,
        StatusCode::UNAUTHORIZED
    );
    login_ok(&router, "anna", new_password).await;
}

// ── User administration ────────────────────────────────────────────────────

#[tokio::test]
async fn admins_create_list_update_and_delete_users() {
    let app = TestApp::new();
    let (_, admin) = app.admin().await;
    let router = app.router();
    let new_user = json!({
        "username": "Ben",
        "display_name": " Ben Becker ",
        "password": "bens long password",
        "permissions": ["search.use", "inventory.discard"]
    });
    let (status, _, ben) = send(
        &router,
        request(
            Method::POST,
            "/api/users",
            Some(new_user.clone()),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{ben}");
    assert_eq!(ben["username"], "ben");
    assert_eq!(ben["display_name"], "Ben Becker");
    assert_eq!(
        ben["permissions"],
        json!(["inventory.discard", "search.use"])
    );
    assert!(ben.get("password_hash").is_none());
    let ben_id = ben["id"].as_i64().unwrap();

    let (status, _, body) = send(
        &router,
        request(Method::POST, "/api/users", Some(new_user), Some(&admin)),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "username_taken");

    let (_, _, list) = send(
        &router,
        request(Method::GET, "/api/users", None, Some(&admin)),
    )
    .await;
    let names: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["username"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["admin", "ben"]);

    let change = json!({ "display_name": "Benjamin", "permissions": ["listing.use"] });
    let (status, _, ben) = send(
        &router,
        request(
            Method::PATCH,
            &format!("/api/users/{ben_id}"),
            Some(change),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ben["display_name"], "Benjamin");
    assert_eq!(ben["permissions"], json!(["listing.use"]));

    let (status, _, _) = send(
        &router,
        request(
            Method::DELETE,
            &format!("/api/users/{ben_id}"),
            None,
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        login(&router, "ben", "bens long password").await.0,
        StatusCode::UNAUTHORIZED
    );

    let audit = app.audit_actions().await;
    let update = audit.iter().find(|(a, _, _)| a == "user.updated").unwrap();
    assert_eq!(update.1, "ben");
    assert_eq!(update.2["permissions"]["added"], json!(["listing.use"]));
    assert_eq!(
        update.2["permissions"]["removed"],
        json!(["inventory.discard", "search.use"])
    );
    assert_eq!(update.2["display_name"]["to"], "Benjamin");
    assert!(audit
        .iter()
        .any(|(a, t, _)| a == "user.deleted" && t == "ben"));
    assert!(audit
        .iter()
        .any(|(a, t, _)| a == "user.created" && t == "ben"));
}

#[tokio::test]
async fn invalid_user_data_is_rejected() {
    let app = TestApp::new();
    let (_, admin) = app.admin().await;
    let router = app.router();
    for body in [
        json!({ "username": "b", "display_name": "B", "password": PASSWORD }),
        json!({ "username": "ben", "display_name": "", "password": PASSWORD }),
        json!({ "username": "ben", "display_name": "Ben", "password": "short" }),
        json!({ "username": "ben", "display_name": "Ben", "password": PASSWORD, "permissions": ["repricing.export"] }),
        json!({ "username": "ben", "display_name": "Ben", "password": PASSWORD, "permissions": ["root"] }),
    ] {
        let (status, _, response) = send(
            &router,
            request(Method::POST, "/api/users", Some(body.clone()), Some(&admin)),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body} -> {response}");
        assert_eq!(response["error"]["code"], "invalid_request");
    }
}

#[tokio::test]
async fn the_last_admin_and_your_own_account_are_protected() {
    let app = TestApp::new();
    let (admin_id, admin) = app.admin().await;
    let router = app.router();
    let me = format!("/api/users/{admin_id}");

    let drop_own = json!({ "permissions": ["search.use"] });
    let (status, _, body) = send(
        &router,
        request(Method::PATCH, &me, Some(drop_own.clone()), Some(&admin)),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "last_admin");

    let (status, _, body) = send(
        &router,
        request(
            Method::PATCH,
            &me,
            Some(json!({ "disabled": true })),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("self"))
    );
    let (status, _, body) = send(&router, request(Method::DELETE, &me, None, Some(&admin))).await;
    assert_eq!(
        (status, body["error"]["code"].clone()),
        (StatusCode::CONFLICT, json!("self"))
    );

    // With a second admin, giving up one's own admin rights is fine.
    app.add_user("second", &["users.manage"]).await;
    let (status, _, _) = send(
        &router,
        request(Method::PATCH, &me, Some(drop_own), Some(&admin)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn setting_a_password_unlocks_and_ends_sessions() {
    let app = TestApp::new();
    let (_, admin) = app.admin().await;
    let anna = app.add_user("anna", &[]).await;
    let router = app.router();
    let anna_cookie = login_ok(&router, "anna", PASSWORD).await;
    for _ in 0..users::MAX_FAILED_LOGINS {
        login(&app.router_from([10, 0, 0, 1]), "anna", "not the password").await;
    }
    let (_, _, list) = send(
        &router,
        request(Method::GET, "/api/users", None, Some(&admin)),
    )
    .await;
    assert!(list[1]["locked_until"].is_string(), "{list}");

    let reset = json!({ "password": "a brand new password" });
    let (status, _, _) = send(
        &router,
        request(
            Method::POST,
            &format!("/api/users/{anna}/password"),
            Some(reset),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        send(
            &router,
            request(Method::GET, "/api/auth/me", None, Some(&anna_cookie))
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    login_ok(&router, "anna", "a brand new password").await;
}

#[tokio::test]
async fn unknown_users_are_404_and_empty_changes_400() {
    let app = TestApp::new();
    let (_, admin) = app.admin().await;
    let router = app.router();
    let (status, _, _) = send(
        &router,
        request(
            Method::PATCH,
            "/api/users/999",
            Some(json!({ "display_name": "X" })),
            Some(&admin),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = send(
        &router,
        request(Method::DELETE, "/api/users/999", None, Some(&admin)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = send(
        &router,
        request(Method::PATCH, "/api/users/1", Some(json!({})), Some(&admin)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_permission_catalog_with_descriptions_is_served() {
    let app = TestApp::new();
    app.add_user("anna", &[]).await;
    let router = app.router();
    let cookie = login_ok(&router, "anna", PASSWORD).await;
    let (status, _, catalog) = send(
        &router,
        request(Method::GET, "/api/permissions", None, Some(&cookie)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let entries = catalog.as_array().unwrap();
    assert_eq!(entries.len(), Permission::ALL.len());
    for entry in entries {
        for field in ["key", "label", "description", "group", "group_label"] {
            assert!(
                entry[field].as_str().is_some_and(|s| !s.is_empty()),
                "{entry}"
            );
        }
    }
    let manage = entries.iter().find(|e| e["key"] == "users.manage").unwrap();
    assert!(manage["description"]
        .as_str()
        .unwrap()
        .contains("fully trust"));
}

// ── Audit log ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn the_audit_log_pages_backwards() {
    let app = TestApp::new();
    let (_, admin) = app.admin().await;
    let router = app.router();
    for _ in 0..3 {
        login(&router, "nobody", "whatever it is").await;
    }
    let (status, _, page) = send(
        &router,
        request(Method::GET, "/api/audit?limit=2", None, Some(&admin)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["entries"].as_array().unwrap().len(), 2);
    let next = page["next_before_id"].as_i64().unwrap();
    let (_, _, older) = send(
        &router,
        request(
            Method::GET,
            &format!("/api/audit?limit=2&before_id={next}"),
            None,
            Some(&admin),
        ),
    )
    .await;
    assert!(older["entries"][0]["id"].as_i64().unwrap() < next);
    let (status, _, body) = send(
        &router,
        request(Method::GET, "/api/audit?nonsense=1", None, Some(&admin)),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // The usual error format, not axum's plain-text rejection.
    assert_eq!(body["error"]["code"], "invalid_request");
    assert!(!body["error"]["message"]
        .as_str()
        .unwrap()
        .contains("nonsense"));
}

// ── Request bodies ─────────────────────────────────────────────────────────

#[tokio::test]
async fn bad_bodies_get_the_usual_error_format() {
    let app = TestApp::new();
    let router = app.router();
    let mut malformed = request(Method::POST, "/api/auth/login", None, None);
    *malformed.body_mut() = Body::from("{not json");
    malformed.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    let (status, _, body) = send(&router, malformed).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_request");

    let extra = json!({ "username": "a", "password": "b", "admin": true });
    let (status, _, body) = send(
        &router,
        request(Method::POST, "/api/auth/login", Some(extra), None),
    )
    .await;
    assert!(status.is_client_error(), "{status}");
    assert_eq!(
        body["error"]["code"], "invalid_request",
        "unknown fields are refused"
    );

    let huge = json!({ "username": "a", "password": "x".repeat(BODY_LIMIT) });
    let (status, _, body) = send(
        &router,
        request(Method::POST, "/api/auth/login", Some(huge), None),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["error"]["code"], "too_large");
}
