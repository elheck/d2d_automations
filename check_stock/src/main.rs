// The binary delegates entirely to the library crate.
// All modules are compiled once as part of the lib; no duplicate mod declarations here.
// Android uses android_main() in lib.rs — no main() is needed for that target.

use log::info;

#[cfg(not(target_os = "android"))]
fn main() {
    // Initialize logger. Set RUST_LOG environment variable to control log level.
    // Examples: RUST_LOG=info, RUST_LOG=warn, RUST_LOG=d2d_automations=trace
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    info!("Starting D2D Automations - MTG Stock Checker");

    if let Err(e) = d2d_automations::ui::launch_gui() {
        log::error!("Application error: {e}");
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
