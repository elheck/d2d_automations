//! Server configuration: where it listens, which host names it answers and
//! where its data lives.

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
        })
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
        self.data_dir.join("cache")
    }
}

/// The inventory database inside a data folder.
pub fn inventory_db_in(data_dir: &Path) -> InventoryDb {
    InventoryDb::in_dir(data_dir)
}

/// Host names the server answers to. A request whose `Host` header names any
/// other host is refused, so a web page that points its own domain at
/// `127.0.0.1` (DNS rebinding) cannot talk to the server through a browser.
/// Loopback names are always allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostAllowlist {
    hosts: Vec<String>,
}

/// Always allowed: the machine itself.
pub const LOOPBACK_HOSTS: [&str; 3] = ["localhost", "127.0.0.1", "[::1]"];

impl HostAllowlist {
    /// Loopback names plus `extra` (e.g. `stock.example.com`).
    pub fn new(extra: &[String]) -> Result<Self, String> {
        let mut hosts: Vec<String> = LOOPBACK_HOSTS.iter().map(|h| h.to_string()).collect();
        for raw in extra {
            let host = raw.trim().to_ascii_lowercase();
            if !is_valid_host_name(&host) {
                return Err(format!(
                    "not a valid host name: {raw:?} (letters, digits, '-' and '.', no port, no wildcards)"
                ));
            }
            if !hosts.contains(&host) {
                hosts.push(host);
            }
        }
        Ok(Self { hosts })
    }

    /// Whether a `Host` header value (with or without port) is allowed.
    pub fn allows(&self, host_header: &str) -> bool {
        match host_without_port(host_header) {
            Some(host) => self.hosts.iter().any(|h| h.eq_ignore_ascii_case(host)),
            None => false,
        }
    }

    /// The allowed names, loopback first.
    pub fn hosts(&self) -> &[String] {
        &self.hosts
    }
}

/// `example.com:8080` → `example.com`, `[::1]:8080` → `[::1]`. `None` for
/// anything malformed.
fn host_without_port(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(rest) = value.strip_prefix('[') {
        let end = rest.find(']')?;
        let after = &rest[end + 1..];
        if !(after.is_empty() || is_port_suffix(after)) {
            return None;
        }
        return Some(&value[..end + 2]);
    }
    match value.split_once(':') {
        Some((host, port)) if is_port_suffix(&format!(":{port}")) => Some(host),
        Some(_) => None,
        None => Some(value),
    }
}

fn is_port_suffix(s: &str) -> bool {
    s.strip_prefix(':')
        .is_some_and(|p| !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()))
}

/// A DNS name (labels of letters, digits and `-`) or an IPv4 address.
fn is_valid_host_name(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
