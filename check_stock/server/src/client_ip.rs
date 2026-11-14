//! The client's IP address, for the login rate limit and the audit log.
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

#[cfg(test)]
#[path = "client_ip_tests.rs"]
mod tests;
