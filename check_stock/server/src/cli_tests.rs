use super::*;

fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(std::iter::once("check_stock_server").chain(args.iter().copied()))
}

#[test]
fn serve_defaults() {
    let Command::Serve(args) = parse(&["serve", "--data-dir", "dev-data"]).unwrap().command else {
        panic!("expected serve");
    };
    assert_eq!(args.data_dir, PathBuf::from("dev-data"));
    assert_eq!(args.bind, DEFAULT_BIND);
    assert!(args.allowed_hosts.is_empty());
    assert_eq!(args.read_connections, DEFAULT_READ_CONNECTIONS);
    assert!(!args.create_empty);
    let config = args.to_config().unwrap();
    assert!(config.is_loopback_only());
}

#[test]
fn serve_options() {
    let cli = parse(&[
        "serve",
        "--data-dir",
        "/data",
        "--bind",
        "0.0.0.0:9000",
        "--allowed-host",
        "stock.example.com,other.example.com",
        "--allowed-host",
        "third.example.com",
        "--read-connections",
        "8",
        "--create-empty",
    ])
    .unwrap();
    let Command::Serve(args) = cli.command else {
        panic!("expected serve");
    };
    assert_eq!(args.bind.to_string(), "0.0.0.0:9000");
    assert_eq!(
        args.allowed_hosts,
        [
            "stock.example.com",
            "other.example.com",
            "third.example.com"
        ]
    );
    assert!(args.create_empty);
    let config = args.to_config().unwrap();
    assert!(!config.is_loopback_only());
    assert!(config.allowed_hosts.allows("third.example.com"));
}

#[test]
fn invalid_serve_settings_are_rejected() {
    let Command::Serve(bad_host) = parse(&[
        "serve",
        "--data-dir",
        "d",
        "--allowed-host",
        "*.example.com",
    ])
    .unwrap()
    .command
    else {
        panic!("expected serve");
    };
    assert!(bad_host.to_config().is_err());

    let Command::Serve(bad_readers) =
        parse(&["serve", "--data-dir", "d", "--read-connections", "0"])
            .unwrap()
            .command
    else {
        panic!("expected serve");
    };
    assert!(bad_readers.to_config().is_err());

    assert!(parse(&["serve", "--data-dir", "d", "--bind", "not-an-address"]).is_err());
}

#[test]
fn import_desktop_arguments() {
    let cli = parse(&[
        "import-desktop",
        "--data-dir",
        "dev-data",
        "--db",
        "/home/u/.local/share/d2d_automations/inventory.db",
        "--replace",
    ])
    .unwrap();
    let Command::ImportDesktop(args) = cli.command else {
        panic!("expected import-desktop");
    };
    assert_eq!(args.data_dir, PathBuf::from("dev-data"));
    assert_eq!(
        args.source,
        PathBuf::from("/home/u/.local/share/d2d_automations/inventory.db")
    );
    assert!(args.replace);
}

#[test]
fn import_desktop_needs_a_source() {
    assert!(parse(&["import-desktop", "--data-dir", "d"]).is_err());
}

#[test]
fn unknown_commands_are_rejected() {
    assert!(parse(&["deploy"]).is_err());
    assert!(parse(&[]).is_err());
}

#[test]
fn trusted_proxies_are_parsed() {
    let Command::Serve(args) = parse(&[
        "serve",
        "--data-dir",
        "d",
        "--trusted-proxy",
        "172.18.0.0/16,127.0.0.1",
    ])
    .unwrap()
    .command
    else {
        panic!("expected serve");
    };
    let config = args.to_config().unwrap();
    assert!(config
        .trusted_proxies
        .contains("172.18.0.7".parse().unwrap()));
    let Command::Serve(bad) = parse(&["serve", "--data-dir", "d", "--trusted-proxy", "everyone"])
        .unwrap()
        .command
    else {
        panic!("expected serve");
    };
    assert!(bad.to_config().is_err());
}

#[test]
fn refresh_and_replace_exclude_each_other() {
    assert!(parse(&[
        "import-desktop",
        "--data-dir",
        "d",
        "--db",
        "x",
        "--refresh"
    ])
    .is_ok());
    assert!(parse(&[
        "import-desktop",
        "--data-dir",
        "d",
        "--db",
        "x",
        "--refresh",
        "--replace"
    ])
    .is_err());
}

#[test]
fn admin_commands() {
    let Command::CreateAdmin(args) = parse(&[
        "create-admin",
        "--data-dir",
        "d",
        "--username",
        "aron",
        "--display-name",
        "Aron",
        "--if-none",
    ])
    .unwrap()
    .command
    else {
        panic!("expected create-admin");
    };
    assert_eq!(args.username, "aron");
    assert_eq!(args.display_name.as_deref(), Some("Aron"));
    assert!(args.if_none);

    let Command::ResetPassword(args) =
        parse(&["reset-password", "--data-dir", "d", "--username", "aron"])
            .unwrap()
            .command
    else {
        panic!("expected reset-password");
    };
    assert_eq!(args.username, "aron");
    // Passwords are never accepted as arguments.
    assert!(parse(&[
        "create-admin",
        "--data-dir",
        "d",
        "--username",
        "a",
        "--password",
        "x"
    ])
    .is_err());
}

#[test]
fn inventory_sync_url_option() {
    let Command::Serve(args) = parse(&[
        "serve",
        "--data-dir",
        "d",
        "--inventory-sync-url",
        "http://cardscanner.local:3000",
    ])
    .unwrap()
    .command
    else {
        panic!("expected serve");
    };
    let config = args.to_config().unwrap();
    assert_eq!(
        config.inventory_sync_url.as_deref(),
        Some("http://cardscanner.local:3000")
    );

    let Command::Serve(bad) = parse(&[
        "serve",
        "--data-dir",
        "d",
        "--inventory-sync-url",
        "http://user:pw@cardscanner.local:3000",
    ])
    .unwrap()
    .command
    else {
        panic!("expected serve");
    };
    assert!(bad.to_config().is_err());
}
