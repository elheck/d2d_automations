//! Command line: `serve` and `import-desktop`.

use crate::app;
use crate::config::{ServeConfig, DEFAULT_BIND, DEFAULT_READ_CONNECTIONS};
use crate::import;
use clap::{Args, Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

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
}

impl ServeArgs {
    /// Validated server configuration.
    pub fn to_config(&self) -> Result<ServeConfig, String> {
        ServeConfig::new(
            self.bind,
            self.data_dir.clone(),
            &self.allowed_hosts,
            self.read_connections,
        )
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
    #[arg(long)]
    pub replace: bool,
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
            runtime.block_on(app::run(config, args.create_empty))?;
        }
        Command::ImportDesktop(args) => {
            let report = import::import_desktop(&args.source, &args.data_dir, args.replace)?;
            println!(
                "Copied {} inventory rows into {} (copy taken {}).",
                report.inventory_rows,
                report.target.display(),
                report.taken_at
            );
            if let Some(previous) = report.previous_kept_as {
                println!("The previous database is kept as {}.", previous.display());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
