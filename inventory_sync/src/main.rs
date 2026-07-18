//! Inventory Sync - MTG Stock & Pricing Database
//!
//! Syncs card inventory from CSV exports to SQLite and collects pricing data.
//! Runs continuously with daily sync scheduling.

use clap::Parser;
use inventory_sync::{
    has_price_data_for_today, init_schema, insert_price_history, upsert_products, PriceGuide,
    ProductCatalog,
};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::interval;

/// MTG inventory sync server - collects pricing data and syncs to SQLite
#[derive(Parser, Debug)]
#[command(name = "inventory_sync")]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the SQLite database file
    #[arg(short, long, default_value_t = default_db_path())]
    database: String,

    /// Run once and exit (default: run continuously with daily schedule)
    #[arg(long, default_value_t = false)]
    once: bool,

    /// Check interval in hours when running continuously
    #[arg(long, default_value_t = 1)]
    interval_hours: u64,

    /// Enable web UI on specified port (default: disabled)
    #[arg(long)]
    web_port: Option<u16>,
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

    // Open database connection
    let conn = match Connection::open(&db_path) {
        Ok(conn) => {
            log::info!("Opened database: {}", db_path.display());
            conn
        }
        Err(e) => {
            log::error!("Failed to open database: {}", e);
            std::process::exit(1);
        }
    };

    // Initialize database schema
    if let Err(e) = init_schema(&conn) {
        log::error!("Failed to initialize database schema: {}", e);
        std::process::exit(1);
    }

    // Wrap connection in Arc<Mutex> for thread-safe sharing
    let db = Arc::new(Mutex::new(conn));

    // Spawn web server if --web-port specified
    if let Some(port) = args.web_port {
        let web_db = Arc::clone(&db);
        let web_db_path = db_path.clone();
        tokio::spawn(async move {
            if let Err(e) = inventory_sync::web::serve(web_db, &web_db_path, port).await {
                log::error!("Web server error: {}", e);
            }
        });
    }

    if args.once {
        // Run once and exit
        run_sync(&db).await;
    } else {
        // Run continuously with interval checks
        log::info!(
            "Running in daemon mode, checking every {} hour(s)",
            args.interval_hours
        );
        run_daemon(&db, args.interval_hours).await;
    }
}

/// Run the sync daemon - checks periodically and syncs when needed
async fn run_daemon(db: &Arc<Mutex<Connection>>, interval_hours: u64) {
    let check_interval = Duration::from_secs(interval_hours * 3600);
    let mut ticker = interval(check_interval);

    // Run immediately on startup
    run_sync(db).await;

    loop {
        ticker.tick().await;
        log::info!("Scheduled check triggered");
        run_sync(db).await;
    }
}

/// Run a single sync operation
async fn run_sync(db: &Arc<Mutex<Connection>>) {
    // Check if we already have price data for today
    let should_sync = {
        let conn = db.lock().unwrap();
        match has_price_data_for_today(&conn) {
            Ok(true) => {
                log::info!("Price data for today already exists in database, skipping download");
                false
            }
            Ok(false) => {
                log::info!("No price data for today, proceeding with download...");
                true
            }
            Err(e) => {
                log::error!("Failed to check existing price data: {}", e);
                false
            }
        }
    };

    if !should_sync {
        return;
    }

    // Fetch product catalog from Cardmarket (singles + non-singles)
    let catalog = match ProductCatalog::fetch().await {
        Ok(catalog) => {
            log::info!(
                "Fetched product catalog: {} products ({} singles, {} non-singles)",
                catalog.len(),
                catalog.singles_count(),
                catalog.non_singles_count()
            );
            catalog
        }
        Err(e) => {
            log::error!("Failed to fetch product catalog: {}", e);
            return;
        }
    };

    // Upsert products into database
    {
        let mut conn = db.lock().unwrap();
        match upsert_products(&mut conn, &catalog) {
            Ok(count) => {
                log::info!("Synced {} products to database", count);
            }
            Err(e) => {
                log::error!("Failed to upsert products: {}", e);
                return;
            }
        }
    }

    // Fetch price guide from Cardmarket
    let guide = match PriceGuide::fetch().await {
        Ok(guide) => {
            log::info!(
                "Fetched price guide: {} entries (created: {})",
                guide.len(),
                guide.created_at()
            );
            guide
        }
        Err(e) => {
            log::error!("Failed to fetch price guide: {}", e);
            return;
        }
    };

    // Insert price history (only if not already present for this date)
    {
        let mut conn = db.lock().unwrap();
        match insert_price_history(&mut conn, &guide, &catalog) {
            Ok(result) => {
                if result.inserted > 0 {
                    log::info!(
                        "Inserted {} price entries for {} ({} products not in catalog)",
                        result.inserted,
                        result.price_date,
                        result.no_product
                    );
                } else {
                    log::info!(
                        "Price data for {} already exists, {} entries skipped",
                        result.price_date,
                        result.skipped
                    );
                }
            }
            Err(e) => {
                log::error!("Failed to insert price history: {}", e);
                return;
            }
        }
    }

    log::info!("Sync completed successfully.");
}
