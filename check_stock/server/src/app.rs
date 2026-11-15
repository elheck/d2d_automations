//! Starting and stopping the server.

use crate::cards::CardLookup;
use crate::client_ip::TrustedProxies;
use crate::config::{HostAllowlist, ServeConfig};
use crate::data_dir::{DataDirLock, LockError};
use crate::db::{Db, DbError};
use crate::http;
use crate::password::{self, PasswordError};
use crate::precon::PreconService;
use crate::rate_limit::LoginRateLimiter;
use axum::Router;
use check_stock_core::api::inventory_sync::InventorySyncClient;
use check_stock_core::api::mtgjson::MtgjsonClient;
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
    pub trusted_proxies: Arc<TrustedProxies>,
    pub login_limiter: Arc<LoginRateLimiter>,
    /// Checked instead of a real hash for unknown user names (equal timing).
    pub dummy_hash: Arc<str>,
    /// Scryfall card data and images, cached in the data folder.
    pub cards: Arc<CardLookup>,
    /// The inventory_sync price tracker, if configured.
    pub inventory_sync: Option<Arc<InventorySyncClient>>,
    /// Precon EV data and its background jobs (shared by all users).
    pub precon: Arc<PreconService>,
    /// Where the caches live (`<data>/cache`).
    pub cache_root: Arc<PathBuf>,
}

impl AppState {
    /// Builds the state; computes the dummy hash and reads the card cache
    /// (next to the database, see [`crate::config::cache_root_in`]), so call
    /// it on a blocking thread. No price tracker until
    /// [`with_inventory_sync`](Self::with_inventory_sync).
    pub fn new(
        db: Db,
        allowed_hosts: HostAllowlist,
        trusted_proxies: TrustedProxies,
    ) -> Result<Self, PasswordError> {
        let data_dir = db.path().parent().map(PathBuf::from).unwrap_or_default();
        let cache_root = crate::config::cache_root_in(&data_dir);
        let cards = CardLookup::new(&cache_root);
        let precon = PreconService::new(&cache_root, None, MtgjsonClient::default());
        Ok(Self {
            db,
            allowed_hosts: Arc::new(allowed_hosts),
            trusted_proxies: Arc::new(trusted_proxies),
            login_limiter: Arc::new(LoginRateLimiter::default()),
            dummy_hash: password::dummy_hash()?.into(),
            cards: Arc::new(cards),
            inventory_sync: None,
            precon: Arc::new(precon),
            cache_root: Arc::new(cache_root),
        })
    }

    /// Uses `cards` for card lookups (tests: a fake Scryfall).
    pub fn with_cards(mut self, cards: CardLookup) -> Self {
        self.cards = Arc::new(cards);
        self
    }

    /// Asks the inventory_sync server at `url` for Cardmarket prices (also
    /// for the precon EV, which then uses MTGJSON's real address).
    pub fn with_inventory_sync(mut self, url: Option<&str>) -> Self {
        let client = url.map(InventorySyncClient::new);
        self.inventory_sync = client.clone().map(Arc::new);
        self.precon = Arc::new(PreconService::new(
            &self.cache_root,
            client,
            MtgjsonClient::default(),
        ));
        self
    }

    /// Uses `precon` for the precon EV (tests: fake tracker and MTGJSON).
    pub fn with_precon(mut self, precon: PreconService) -> Self {
        self.precon = Arc::new(precon);
        self
    }
}

#[derive(Debug)]
pub enum RunError {
    Lock(LockError),
    /// The data folder has no database and `--create-empty` was not given.
    NoDatabase(PathBuf),
    Db(DbError),
    Password(PasswordError),
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
            RunError::Password(e) => write!(f, "{e}"),
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
    let allowed_hosts = config.allowed_hosts.clone();
    let trusted_proxies = config.trusted_proxies.clone();
    let inventory_sync_url = config.inventory_sync_url.clone();
    let state = tokio::task::spawn_blocking(move || -> Result<AppState, RunError> {
        let db = Db::open(&inventory_db, read_connections)?;
        log::info!("Database: {}", db.path().display());
        Ok(AppState::new(db, allowed_hosts, trusted_proxies)
            .map_err(RunError::Password)?
            .with_inventory_sync(inventory_sync_url.as_deref()))
    })
    .await
    .map_err(|e| RunError::Db(DbError::Task(e.to_string())))??;
    if !config.trusted_proxies.is_empty() {
        log::info!("Trusting X-Forwarded-For from the configured proxies");
    }
    match &config.inventory_sync_url {
        Some(url) => log::info!("Cardmarket prices from inventory_sync at {url}"),
        None => log::info!("No --inventory-sync-url: pages show no Cardmarket prices"),
    }
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
    let precon = state.precon.clone();
    serve(listener, http::router(state), shutdown).await?;
    // Background jobs stop at their next check; `main` waits only briefly.
    precon.shutdown();
    log::info!("Stopped");
    Ok(())
}

/// Serves `router` on `listener` until `shutdown` completes.
pub async fn serve<S>(listener: TcpListener, router: Router, shutdown: S) -> std::io::Result<()>
where
    S: Future<Output = ()> + Send + 'static,
{
    // Connection info gives handlers the peer address (login rate limit).
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
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
