use super::*;

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

fn proxies(list: &[&str]) -> TrustedProxies {
    TrustedProxies::parse(&list.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
}

#[test]
fn ranges() {
    let docker = IpRange::parse("172.18.0.0/16").unwrap();
    assert!(docker.contains(ip("172.18.0.5")));
    assert!(docker.contains(ip("172.18.255.255")));
    assert!(!docker.contains(ip("172.19.0.1")));
    assert!(!docker.contains(ip("::1")));

    let single = IpRange::parse("192.168.178.20").unwrap();
    assert!(single.contains(ip("192.168.178.20")));
    assert!(!single.contains(ip("192.168.178.21")));
    assert!(
        single.contains(ip("::ffff:192.168.178.20")),
        "IPv4-mapped IPv6"
    );

    assert!(IpRange::parse("0.0.0.0/0").unwrap().contains(ip("8.8.8.8")));
    assert!(IpRange::parse("fd00::/8").unwrap().contains(ip("fd12::1")));
}

#[test]
fn invalid_ranges_are_rejected() {
    for bad in [
        "",
        "localhost",
        "192.168.1.0/33",
        "::/129",
        "1.2.3.4/",
        "1.2.3/24",
        "1.2.3.4/x",
    ] {
        assert!(IpRange::parse(bad).is_err(), "{bad:?}");
    }
    assert!(TrustedProxies::parse(&["172.18.0.0/16".into(), "nope".into()]).is_err());
}

#[test]
fn without_trusted_proxies_the_header_is_ignored() {
    let none = TrustedProxies::default();
    assert!(none.is_empty());
    assert_eq!(
        none.client_ip(ip("192.168.178.30"), Some("6.6.6.6")),
        ip("192.168.178.30")
    );
}

#[test]
fn an_untrusted_peer_cannot_claim_another_address() {
    let caddy = proxies(&["172.18.0.2"]);
    assert_eq!(
        caddy.client_ip(ip("192.168.178.30"), Some("127.0.0.1")),
        ip("192.168.178.30")
    );
}

#[test]
fn behind_caddy_the_client_is_the_rightmost_untrusted_entry() {
    let caddy = proxies(&["172.18.0.0/16"]);
    assert_eq!(
        caddy.client_ip(ip("172.18.0.2"), Some("192.168.178.30")),
        ip("192.168.178.30")
    );
    // A client that sends its own forged header: Caddy appends the real one.
    assert_eq!(
        caddy.client_ip(ip("172.18.0.2"), Some("1.2.3.4, 192.168.178.30")),
        ip("192.168.178.30")
    );
    // A chain of trusted proxies is walked through.
    assert_eq!(
        caddy.client_ip(ip("172.18.0.2"), Some("192.168.178.30, 172.18.0.9")),
        ip("192.168.178.30")
    );
}

#[test]
fn garbage_stops_the_walk() {
    let caddy = proxies(&["172.18.0.0/16"]);
    assert_eq!(caddy.client_ip(ip("172.18.0.2"), None), ip("172.18.0.2"));
    assert_eq!(
        caddy.client_ip(ip("172.18.0.2"), Some("not-an-ip")),
        ip("172.18.0.2")
    );
    assert_eq!(
        caddy.client_ip(ip("172.18.0.2"), Some("6.6.6.6, garbage, 172.18.0.9")),
        ip("172.18.0.9"),
        "never reaches the forged entry behind the garbage"
    );
}
