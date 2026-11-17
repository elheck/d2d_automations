use super::*;

fn parse(args: &[&str]) -> Args {
    Args::try_parse_from(std::iter::once("inventory_sync").chain(args.iter().copied())).unwrap()
}

#[test]
fn the_web_ui_listens_on_this_machine_unless_told_otherwise() {
    // The variables are what the Docker image and deploy set; tests must not
    // see them from the environment.
    for var in [
        "INVENTORY_SYNC_WEB_BIND",
        "INVENTORY_SYNC_TRUSTED_PROXIES",
        "INVENTORY_SYNC_ALLOWED_HOSTS",
    ] {
        assert!(
            std::env::var_os(var).is_none(),
            "{var} is set in the test environment"
        );
    }
    let args = parse(&["--web-port", "3000"]);
    let config = args.web_config(3000).unwrap();
    assert_eq!(config.bind.to_string(), "127.0.0.1");
    assert!(config.options.allowed_hosts.is_none());

    let args = parse(&["--web-port", "3000", "--web-bind", "0.0.0.0"]);
    assert_eq!(args.web_config(3000).unwrap().bind.to_string(), "0.0.0.0");
}

#[test]
fn proxies_and_hosts_are_parsed_and_checked() {
    let args = parse(&[
        "--trusted-proxy",
        "172.18.0.0/16",
        "--trusted-proxy",
        "10.0.0.2",
        "--allowed-host",
        "cardscanner.local",
    ]);
    let config = args.web_config(3000).unwrap();
    let proxies = &config.options.trusted_proxies;
    assert!(proxies.contains("172.18.4.5".parse().unwrap()));
    assert!(proxies.contains("10.0.0.2".parse().unwrap()));
    let hosts = config.options.allowed_hosts.unwrap();
    assert!(hosts.allows("cardscanner.local:3000"));
    assert!(hosts.allows("localhost:3000"));
    assert!(!hosts.allows("evil.example"));

    assert!(parse(&["--trusted-proxy", "not-an-ip"])
        .web_config(3000)
        .is_err());
    assert!(parse(&["--allowed-host", "*.example.com"])
        .web_config(3000)
        .is_err());
    assert!(Args::try_parse_from(["inventory_sync", "--web-bind", "everywhere"]).is_err());
}
