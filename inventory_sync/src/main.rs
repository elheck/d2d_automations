//! Inventory Sync - MTG Stock & Pricing Database
//!
//! Syncs card inventory from CSV exports to SQLite and collects pricing data.

use clap::Parser;
use inventory_sync::PriceGuide;
use std::path::PathBuf;

/// MTG inventory sync server - collects pricing data and syncs to SQLite
#[derive(Parser, Debug)]
#[command(name = "inventory_sync")]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the SQLite database file
    #[arg(short, long, default_value_t = default_db_path())]
    database: String,
}

/// Returns the default database path: ~/.local/share/inventory_sync/inventory.db
fn default_db_path() -> String {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("inventory_sync")
        .join("inventory.db")
        .to_string_lossy()
        .to_string()
}

#[tokio::main]
async fn main() {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();
    let db_path = PathBuf::from(&args.database);

    log::info!("Starting inventory_sync...");
    log::info!("Database path: {}", db_path.display());

    // Ensure parent directory exists
    if let Some(parent) = db_path.parent() {
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::error!("Failed to create database directory: {}", e);
                std::process::exit(1);
            }
            log::info!("Created directory: {}", parent.display());
        }
    }

    // Fetch price guide from Cardmarket
    match PriceGuide::fetch().await {
        Ok(guide) => {
            log::info!(
                "Successfully loaded price guide: {} entries (created: {})",
                guide.len(),
                guide.created_at()
            );

            // Example: look up a specific card (Black Lotus has id 3781)
            if let Some(entry) = guide.get(3781) {
                log::info!(
                    "Black Lotus - Trend: {:?}, Avg: {:?}",
                    entry.trend,
                    entry.avg
                );
            }
        }
        Err(e) => {
            log::error!("Failed to fetch price guide: {}", e);
            std::process::exit(1);
        }
    }

    log::info!("inventory_sync completed.");
}
