//! Network helpers shared by the servers (check_stock_server, inventory_sync):
//! the client's address behind a trusted proxy, and the `Host` allowlist
//! against DNS rebinding.
//!
//! Behind Caddy every connection comes from Caddy, so the real address is in
//! `X-Forwarded-For`. That header is only believed when the connection comes
//! from a configured trusted proxy; anyone else could put anything in it.
//! Caddy appends the address it saw, so the client is the rightmost entry
//! that is not itself a trusted proxy.

use std::net::IpAddr;

/// An IP address or a CIDR range (`172.18.0.0/16`, `::1/128`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IpRange {
    network: IpAddr,
    prefix: u8,
}

impl IpRange {
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let invalid = || format!("not an IP address or CIDR range: {text:?}");
        let (addr, prefix) = match text.split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (text, None),
        };
        let network: IpAddr = addr.parse().map_err(|_| invalid())?;
        let network = network.to_canonical();
        let max = if network.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            Some(p) => p.parse::<u8>().map_err(|_| invalid())?,
            None => max,
        };
        if prefix > max {
            return Err(invalid());
        }
        Ok(Self { network, prefix })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.network, ip.to_canonical()) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = u32::MAX
                    .checked_shl(32 - u32::from(self.prefix))
                    .unwrap_or(0);
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = u128::MAX
                    .checked_shl(128 - u32::from(self.prefix))
                    .unwrap_or(0);
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

/// Proxies whose `X-Forwarded-For` is believed. Empty by default.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrustedProxies(Vec<IpRange>);

impl TrustedProxies {
    pub fn parse(entries: &[String]) -> Result<Self, String> {
        entries
            .iter()
            .map(|e| IpRange::parse(e))
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        self.0.iter().any(|range| range.contains(ip))
    }

    /// The client address for a connection from `peer` with the given
    /// `X-Forwarded-For` value. Unparsable entries end the walk, so a forged
    /// prefix can never be reached through garbage.
    pub fn client_ip(&self, peer: IpAddr, forwarded_for: Option<&str>) -> IpAddr {
        let peer = peer.to_canonical();
        if !self.contains(peer) {
            return peer;
        }
        let Some(header) = forwarded_for else {
            return peer;
        };
        let mut client = peer;
        for entry in header.rsplit(',') {
            match entry.trim().parse::<IpAddr>() {
                Ok(ip) => {
                    client = ip.to_canonical();
                    if !self.contains(client) {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        client
    }
}

/// Host names a server answers to. A request whose `Host` header names any
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
#[path = "net_tests.rs"]
mod tests;
