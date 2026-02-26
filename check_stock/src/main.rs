fn main() {
    // Initialize logger. Set RUST_LOG environment variable to control log level.
    // Examples: RUST_LOG=info, RUST_LOG=warn, RUST_LOG=d2d_automations=trace
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("Starting D2D Automations - MTG Stock Checker");

    if let Err(e) = d2d_automations::ui::launch_gui() {
        log::error!("Application error: {e}");
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
