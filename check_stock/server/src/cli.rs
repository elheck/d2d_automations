//! Command line: `serve`, `import-desktop`, `create-admin`, `reset-password`.

use crate::admin::{self, CreateAdminOutcome};
use crate::app;
use crate::config::{ServeConfig, DEFAULT_BIND, DEFAULT_READ_CONNECTIONS};
use crate::import;
use clap::{Args, Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

/// How long `serve` waits for background jobs after the last request.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

#[derive(Parser, Debug)]
#[command(name = "check_stock_server", version)]
#[command(about = "Web backend of the MTG stock checker (LAN only)")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run the web server.
    Serve(ServeArgs),
    /// Copy the desktop app's inventory database into the data folder
    /// (the desktop file is only read, never changed).
    ImportDesktop(ImportArgs),
    /// Create a user with every permission (the server must be stopped).
    /// The password is asked for on the terminal, or read from stdin.
    CreateAdmin(CreateAdminArgs),
    /// Set a new password for a user, unlock it and end its sessions (the
    /// server must be stopped).
    ResetPassword(ResetPasswordArgs),
    /// Put the inventory data of a backup back (e.g. the
    /// `inventory.db.import-<time>.bak` taken before a bad import). Users,
    /// settings and the audit log stay; the current database is kept as a
    /// backup first. The server must be stopped.
    RestoreInventory(RestoreArgs),
}

#[derive(Args, Debug)]
pub struct ServeArgs {
    /// Data folder: database, backups, caches.
    #[arg(long, env = "CHECK_STOCK_DATA_DIR")]
    pub data_dir: PathBuf,

    /// Address to listen on. Anything but a loopback address makes the server
    /// reachable from other machines.
    #[arg(long, env = "CHECK_STOCK_BIND", default_value_t = DEFAULT_BIND)]
    pub bind: SocketAddr,

    /// Extra host name the server answers to, e.g. `stock.example.com`
    /// (repeatable or comma-separated). `localhost`, `127.0.0.1` and `[::1]`
    /// are always allowed.
    #[arg(
        long = "allowed-host",
        env = "CHECK_STOCK_ALLOWED_HOSTS",
        value_delimiter = ','
    )]
    pub allowed_hosts: Vec<String>,

    /// Read-only database connections next to the writer.
    #[arg(long, default_value_t = DEFAULT_READ_CONNECTIONS)]
    pub read_connections: usize,

    /// Start with an empty database if the data folder has none (otherwise
    /// the server refuses to start, so a forgotten import is noticed).
    #[arg(long)]
    pub create_empty: bool,

    /// Proxy (IP address or CIDR range) whose `X-Forwarded-For` header names
    /// the real client, e.g. Caddy. Repeatable or comma-separated. Without it
    /// the header is ignored.
    #[arg(
        long = "trusted-proxy",
        env = "CHECK_STOCK_TRUSTED_PROXIES",
        value_delimiter = ','
    )]
    pub trusted_proxies: Vec<String>,

    /// Base URL of the inventory_sync price tracker, e.g.
    /// `http://cardscanner.local:3000`. Without it pages show no Cardmarket
    /// prices.
    #[arg(long, env = "CHECK_STOCK_INVENTORY_SYNC_URL")]
    pub inventory_sync_url: Option<String>,
}

impl ServeArgs {
    /// Validated server configuration.
    pub fn to_config(&self) -> Result<ServeConfig, String> {
        ServeConfig::new(
            self.bind,
            self.data_dir.clone(),
            &self.allowed_hosts,
            self.read_connections,
        )?
        .with_trusted_proxies(&self.trusted_proxies)?
        .with_inventory_sync_url(self.inventory_sync_url.as_deref())
    }
}

#[derive(Args, Debug)]
pub struct ImportArgs {
    /// Data folder to copy into.
    #[arg(long, env = "CHECK_STOCK_DATA_DIR")]
    pub data_dir: PathBuf,

    /// The desktop app's `inventory.db`.
    #[arg(long = "db")]
    pub source: PathBuf,

    /// Replace an existing database (it is kept as a dated backup first).
    #[arg(long, conflicts_with = "refresh")]
    pub replace: bool,

    /// Update only the desktop data of an existing database; users,
    /// sessions and the audit log stay (a backup is kept first).
    #[arg(long)]
    pub refresh: bool,

    /// The desktop app's `repricing_ruleset.json`: published as the next
    /// revision of the repricing rules (unless they are the same). Read
    /// first, so a bad file stops the import.
    #[arg(long)]
    pub ruleset: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct RestoreArgs {
    #[arg(long, env = "CHECK_STOCK_DATA_DIR")]
    pub data_dir: PathBuf,

    /// The backup to restore from.
    #[arg(long)]
    pub backup: PathBuf,
}

#[derive(Args, Debug)]
pub struct CreateAdminArgs {
    #[arg(long, env = "CHECK_STOCK_DATA_DIR")]
    pub data_dir: PathBuf,

    /// Login name (a-z, 0-9, '.', '_', '-').
    #[arg(long)]
    pub username: String,

    /// Name shown in the interface (default: the login name).
    #[arg(long)]
    pub display_name: Option<String>,

    /// Do nothing if the database already has users (for setup scripts).
    #[arg(long)]
    pub if_none: bool,
}

#[derive(Args, Debug)]
pub struct ResetPasswordArgs {
    #[arg(long, env = "CHECK_STOCK_DATA_DIR")]
    pub data_dir: PathBuf,

    #[arg(long)]
    pub username: String,
}

/// Entry point of the binary.
pub fn main_entry() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match run_command(Cli::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Runs one command to completion.
pub fn run_command(command: Command) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        Command::Serve(args) => {
            let config = args.to_config()?;
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            let result = runtime.block_on(app::run(config, args.create_empty));
            // Background jobs (e.g. a precon sales fetch) were asked to stop;
            // do not wait for a slow outside request to finish.
            runtime.shutdown_timeout(SHUTDOWN_GRACE);
            result?;
        }
        Command::ImportDesktop(args) => {
            let ruleset = args
                .ruleset
                .as_deref()
                .map(import::read_ruleset)
                .transpose()?;
            let report = if args.refresh {
                import::refresh_desktop(&args.source, &args.data_dir)?
            } else {
                import::import_desktop(&args.source, &args.data_dir, args.replace)?
            };
            println!(
                "Copied {} inventory rows into {} (copy taken {}).",
                report.inventory_rows,
                report.target.display(),
                report.taken_at
            );
            if let Some(previous) = report.previous_kept_as {
                println!("The previous database is kept as {}.", previous.display());
            }
            if let Some(ruleset) = ruleset {
                match import::publish_ruleset(&ruleset, &args.data_dir)? {
                    import::RulesetImport::Published(r) => {
                        println!("Published the repricing rules as revision {r}.")
                    }
                    import::RulesetImport::Unchanged(r) => {
                        println!("The repricing rules are already published (revision {r}).")
                    }
                }
            }
        }
        Command::RestoreInventory(args) => {
            let report = import::restore_inventory(&args.backup, &args.data_dir)?;
            println!(
                "Restored {} inventory rows ({}) into {}.",
                report.inventory_rows,
                report.categories.join(", "),
                report.target.display()
            );
            println!(
                "The database as it was before is kept as {}.",
                report.previous_kept_as.display()
            );
        }
        Command::CreateAdmin(args) => {
            match admin::create_admin(
                &args.data_dir,
                &args.username,
                args.display_name.as_deref(),
                args.if_none,
                admin::read_new_password,
            )? {
                CreateAdminOutcome::Created { username } => {
                    println!("Created {username} with every permission.")
                }
                // Silent: `--if-none` runs on every start of dev.sh.
                CreateAdminOutcome::SkippedUsersExist => {
                    log::debug!("Users exist already; nothing created.")
                }
            }
        }
        Command::ResetPassword(args) => {
            admin::reset_password(&args.data_dir, &args.username, admin::read_new_password)?;
            println!(
                "New password set for {}; the account is unlocked and logged out everywhere.",
                args.username
            );
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
