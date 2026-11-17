use super::*;
use std::net::Ipv6Addr;

fn config(bind: SocketAddr) -> ServeConfig {
    ServeConfig::new(bind, PathBuf::from("data"), &[], DEFAULT_READ_CONNECTIONS).unwrap()
}

#[test]
fn default_bind_is_loopback_only() {
    assert_eq!(DEFAULT_BIND.to_string(), "127.0.0.1:8080");
    assert!(config(DEFAULT_BIND).is_loopback_only());
}

#[test]
fn loopback_detection() {
    let v6 = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 8080);
    assert!(config(v6).is_loopback_only());
    assert!(!config("0.0.0.0:8080".parse().unwrap()).is_loopback_only());
    assert!(!config("192.168.178.20:8080".parse().unwrap()).is_loopback_only());
    assert!(!config("[::]:8080".parse().unwrap()).is_loopback_only());
}

#[test]
fn read_connections_are_bounded() {
    let new = |n| ServeConfig::new(DEFAULT_BIND, PathBuf::from("data"), &[], n);
    assert!(new(0).is_err());
    assert!(new(1).is_ok());
    assert!(new(MAX_READ_CONNECTIONS).is_ok());
    assert!(new(MAX_READ_CONNECTIONS + 1).is_err());
}

#[test]
fn data_dir_must_not_be_empty() {
    assert!(ServeConfig::new(DEFAULT_BIND, PathBuf::new(), &[], 4).is_err());
}

#[test]
fn files_live_in_the_data_dir() {
    let c = config(DEFAULT_BIND);
    assert_eq!(c.inventory_db().path(), Path::new("data/inventory.db"));
    assert_eq!(c.cache_root(), PathBuf::from("data/cache"));
}

#[test]
fn service_urls_are_validated() {
    assert_eq!(
        parse_service_url("http://cardscanner.local:3000").unwrap(),
        "http://cardscanner.local:3000"
    );
    assert_eq!(
        parse_service_url("https://inventory.example.com/").unwrap(),
        "https://inventory.example.com"
    );
    assert_eq!(
        parse_service_url("http://192.168.178.20:3000").unwrap(),
        "http://192.168.178.20:3000"
    );
    for bad in [
        "",
        "cardscanner.local:3000",
        "ftp://cardscanner.local",
        "file:///etc/passwd",
        "http://user:secret@cardscanner.local:3000",
        "http://cardscanner.local:3000/api",
        "http://cardscanner.local:3000/?x=1",
        "http://cardscanner.local:3000/#x",
        "http://",
    ] {
        assert!(parse_service_url(bad).is_err(), "{bad:?} should be refused");
    }
}

#[test]
fn inventory_sync_url_is_optional() {
    let c = config(DEFAULT_BIND);
    assert_eq!(c.inventory_sync_url, None);
    let c = c.with_inventory_sync_url(Some("  ")).unwrap();
    assert_eq!(c.inventory_sync_url, None);
    let c = c
        .with_inventory_sync_url(Some("http://cardscanner.local:3000/"))
        .unwrap();
    assert_eq!(
        c.inventory_sync_url.as_deref(),
        Some("http://cardscanner.local:3000")
    );
    assert!(config(DEFAULT_BIND)
        .with_inventory_sync_url(Some("javascript:alert(1)"))
        .is_err());
}
