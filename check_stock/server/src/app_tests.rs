use super::*;
use crate::config::DEFAULT_READ_CONNECTIONS;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;

fn config(dir: &std::path::Path) -> ServeConfig {
    ServeConfig::new(
        "127.0.0.1:0".parse().unwrap(),
        dir.to_path_buf(),
        &[],
        DEFAULT_READ_CONNECTIONS,
    )
    .unwrap()
}

/// Raw HTTP/1.1 request over TCP; returns the whole response.
async fn http_get(addr: SocketAddr, path: &str, host: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

#[tokio::test]
async fn refuses_to_start_without_a_database() {
    let dir = tempfile::tempdir().unwrap();
    let result = run_until(config(dir.path()), false, async {}, |_| {}).await;
    match result {
        Err(RunError::NoDatabase(path)) => assert_eq!(path, dir.path().join("inventory.db")),
        other => panic!("expected NoDatabase, got {other:?}"),
    }
    assert!(
        !dir.path().join("inventory.db").exists(),
        "nothing was created"
    );
}

#[tokio::test]
async fn serves_over_tcp_holds_the_lock_and_stops_gracefully() {
    let dir = tempfile::tempdir().unwrap();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let (ready_tx, ready_rx) = oneshot::channel::<SocketAddr>();
    let server = tokio::spawn(run_until(
        config(dir.path()),
        true,
        async move {
            let _ = stop_rx.await;
        },
        move |addr| {
            let _ = ready_tx.send(addr);
        },
    ));
    let addr = ready_rx.await.expect("server became ready");
    assert!(addr.ip().is_loopback());

    let response = http_get(addr, "/api/health", "localhost").await;
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.ends_with(r#"{"status":"ok"}"#), "{response}");
    let refused = http_get(addr, "/api/health", "evil.example").await;
    assert!(refused.starts_with("HTTP/1.1 400"), "{refused}");

    // While it runs, nobody else gets the data folder.
    assert!(matches!(
        DataDirLock::acquire(dir.path()),
        Err(LockError::InUse(_))
    ));

    stop_tx.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), server)
        .await
        .expect("server stopped")
        .unwrap()
        .unwrap();
    assert!(DataDirLock::acquire(dir.path()).is_ok(), "lock released");
}

#[tokio::test]
async fn a_second_server_on_the_same_folder_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let _held = DataDirLock::acquire(dir.path()).unwrap();
    let result = run_until(config(dir.path()), true, async {}, |_| {}).await;
    assert!(
        matches!(result, Err(RunError::Lock(LockError::InUse(_)))),
        "{result:?}"
    );
}

#[test]
fn errors_explain_what_to_do() {
    let message = RunError::NoDatabase("/srv/data/inventory.db".into()).to_string();
    assert!(message.contains("import-desktop"), "{message}");
    assert!(message.contains("--create-empty"), "{message}");
}

/// Raw HTTP/1.1 request with extra header lines and a body.
async fn http_send(addr: SocketAddr, head: &str, body: &str) -> String {
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let request = format!(
        "{head}\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

#[tokio::test]
async fn login_works_over_a_real_connection() {
    let dir = tempfile::tempdir().unwrap();
    crate::admin::create_admin(dir.path(), "aron", None, false, |_| {
        Ok("a long admin password".into())
    })
    .unwrap();
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let (ready_tx, ready_rx) = oneshot::channel::<SocketAddr>();
    let server = tokio::spawn(run_until(
        config(dir.path()),
        false,
        async move {
            let _ = stop_rx.await;
        },
        move |addr| {
            let _ = ready_tx.send(addr);
        },
    ));
    let addr = ready_rx.await.unwrap();

    let login = http_send(
        addr,
        "POST /api/auth/login HTTP/1.1\r\nContent-Type: application/json\r\nX-Check-Stock-CSRF: 1",
        r#"{"username":"aron","password":"a long admin password"}"#,
    )
    .await;
    assert!(login.starts_with("HTTP/1.1 200"), "{login}");
    let cookie = login
        .lines()
        .find_map(|l| l.strip_prefix("set-cookie: "))
        .and_then(|c| c.split(';').next())
        .expect("session cookie")
        .to_string();

    let me = http_send(
        addr,
        &format!("GET /api/auth/me HTTP/1.1\r\nCookie: {cookie}"),
        "",
    )
    .await;
    assert!(me.starts_with("HTTP/1.1 200"), "{me}");
    assert!(me.contains(r#""username":"aron""#), "{me}");

    stop_tx.send(()).unwrap();
    server.await.unwrap().unwrap();
}
