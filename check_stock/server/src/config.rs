//! Server configuration: where it listens, which host names it answers and
//! where its data lives.

use crate::client_ip::TrustedProxies;
use check_stock_core::inventory_db::InventoryDb;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

/// Port the server listens on unless configured otherwise.
pub const DEFAULT_PORT: u16 = 8080;

/// Default listen address: loopback only. Anything reachable from other
/// machines has to be configured explicitly.
pub const DEFAULT_BIND: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_PORT);

/// Read-only connections kept open next to the single writer.
pub const DEFAULT_READ_CONNECTIONS: usize = 4;

/// Upper bound for [`ServeConfig::read_connections`].
pub const MAX_READ_CONNECTIONS: usize = 32;

/// Everything `serve` needs.
#[derive(Debug, Clone)]
pub struct ServeConfig {
    pub bind: SocketAddr,
    pub data_dir: PathBuf,
    pub allowed_hosts: HostAllowlist,
    pub read_connections: usize,
    /// Proxies (Caddy) whose `X-Forwarded-For` names the real client.
    pub trusted_proxies: TrustedProxies,
    /// Base URL of the inventory_sync price tracker (Cardmarket prices);
    /// `None` = pages show prices as not configured.
    pub inventory_sync_url: Option<String>,
}

impl ServeConfig {
    /// Validates the settings; `extra_hosts` are added to the loopback names.
    pub fn new(
        bind: SocketAddr,
        data_dir: PathBuf,
        extra_hosts: &[String],
        read_connections: usize,
    ) -> Result<Self, String> {
        if !(1..=MAX_READ_CONNECTIONS).contains(&read_connections) {
            return Err(format!(
                "read connections must be between 1 and {MAX_READ_CONNECTIONS}, got {read_connections}"
            ));
        }
        if data_dir.as_os_str().is_empty() {
            return Err("the data folder must not be empty".to_string());
        }
        Ok(Self {
            bind,
            data_dir,
            allowed_hosts: HostAllowlist::new(extra_hosts)?,
            read_connections,
            trusted_proxies: TrustedProxies::default(),
            inventory_sync_url: None,
        })
    }

    /// Sets the inventory_sync base URL (validated by [`parse_service_url`]).
    pub fn with_inventory_sync_url(mut self, url: Option<&str>) -> Result<Self, String> {
        self.inventory_sync_url = match url.map(str::trim).filter(|u| !u.is_empty()) {
            Some(url) => Some(parse_service_url(url)?),
            None => None,
        };
        Ok(self)
    }

    /// Sets the trusted proxies (IP addresses or CIDR ranges).
    pub fn with_trusted_proxies(mut self, proxies: &[String]) -> Result<Self, String> {
        self.trusted_proxies = TrustedProxies::parse(proxies)?;
        Ok(self)
    }

    /// Whether only this machine can connect.
    pub fn is_loopback_only(&self) -> bool {
        self.bind.ip().is_loopback()
    }

    /// The inventory database in the data folder.
    pub fn inventory_db(&self) -> InventoryDb {
        inventory_db_in(&self.data_dir)
    }

    /// Cache root in the data folder (same layout as the desktop cache root).
    pub fn cache_root(&self) -> PathBuf {
        cache_root_in(&self.data_dir)
    }
}

/// The inventory database inside a data folder.
pub fn inventory_db_in(data_dir: &Path) -> InventoryDb {
    InventoryDb::in_dir(data_dir)
}

/// The cache root inside a data folder (Scryfall cards and images, …).
pub fn cache_root_in(data_dir: &Path) -> PathBuf {
    data_dir.join("cache")
}

/// Validates the base URL of another LAN service (inventory_sync): `http`
/// or `https`, a host, no user name or password, no path, query or fragment.
/// Returns it without a trailing slash.
pub fn parse_service_url(raw: &str) -> Result<String, String> {
    let bad = |why: &str| Err(format!("not a valid service URL {raw:?}: {why}"));
    let Ok(url) = reqwest::Url::parse(raw) else {
        return bad("expected e.g. http://cardscanner.local:3000");
    };
    if !matches!(url.scheme(), "http" | "https") {
        return bad("only http and https are allowed");
    }
    if url.host_str().is_none_or(str::is_empty) {
        return bad("no host name");
    }
    if !url.username().is_empty() || url.password().is_some() {
        return bad("no user name or password in the URL");
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return bad("no path, query or fragment");
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

/// Host names the server answers to (shared with inventory_sync in
/// [`mtg_common::net`]).
pub use mtg_common::net::{HostAllowlist, LOOPBACK_HOSTS};

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
