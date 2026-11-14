//! Starting and stopping the server.

use crate::config::{HostAllowlist, ServeConfig};
use crate::data_dir::{DataDirLock, LockError};
use crate::db::{Db, DbError};
use crate::http;
use axum::Router;
use std::future::Future;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;

/// Shared state of every request handler.
#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub allowed_hosts: Arc<HostAllowlist>,
}

#[derive(Debug)]
pub enum RunError {
    Lock(LockError),
    /// The data folder has no database and `--create-empty` was not given.
    NoDatabase(PathBuf),
    Db(DbError),
    Io(std::io::Error),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Lock(e) => write!(f, "{e}"),
            RunError::NoDatabase(path) => write!(
                f,
                "no database at {}: run `import-desktop` first, or pass --create-empty to start with an empty one",
                path.display()
            ),
            RunError::Db(e) => write!(f, "{e}"),
            RunError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RunError {}

impl From<LockError> for RunError {
    fn from(e: LockError) -> Self {
        RunError::Lock(e)
    }
}

impl From<DbError> for RunError {
    fn from(e: DbError) -> Self {
        RunError::Db(e)
    }
}

impl From<std::io::Error> for RunError {
    fn from(e: std::io::Error) -> Self {
        RunError::Io(e)
    }
}

/// Runs the server until Ctrl-C or SIGTERM.
pub async fn run(config: ServeConfig, create_empty: bool) -> Result<(), RunError> {
    run_until(config, create_empty, shutdown_signal(), |_| {}).await
}

/// Runs the server until `shutdown` completes, then finishes the requests in
/// flight and returns. `on_ready` gets the address once it is listening.
pub async fn run_until<S, R>(
    config: ServeConfig,
    create_empty: bool,
    shutdown: S,
    on_ready: R,
) -> Result<(), RunError>
where
    S: Future<Output = ()> + Send + 'static,
    R: FnOnce(SocketAddr),
{
    let _lock = DataDirLock::acquire(&config.data_dir)?;
    let inventory_db = config.inventory_db();
    if !create_empty && !inventory_db.path().exists() {
        return Err(RunError::NoDatabase(inventory_db.path().to_path_buf()));
    }
    let read_connections = config.read_connections;
    let db = tokio::task::spawn_blocking(move || Db::open(&inventory_db, read_connections))
        .await
        .map_err(|e| RunError::Db(DbError::Task(e.to_string())))??;
    log::info!("Database: {}", db.path().display());

    let state = AppState {
        db,
        allowed_hosts: Arc::new(config.allowed_hosts.clone()),
    };
    let listener = TcpListener::bind(config.bind).await?;
    let addr = listener.local_addr()?;
    if !config.is_loopback_only() {
        log::warn!(
            "Listening on {addr}: reachable from other machines. Keep it behind Caddy and the firewall."
        );
    }
    log::info!(
        "Listening on http://{addr} (answering host names: {})",
        config.allowed_hosts.hosts().join(", ")
    );
    on_ready(addr);
    serve(listener, http::router(state), shutdown).await?;
    log::info!("Stopped");
    Ok(())
}

/// Serves `router` on `listener` until `shutdown` completes.
pub async fn serve<S>(listener: TcpListener, router: Router, shutdown: S) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}

/// Completes on Ctrl-C or (Unix) SIGTERM. If a signal cannot be watched it
/// is ignored rather than stopping the server right away.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    log::info!("Shutting down: finishing requests in flight");
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
