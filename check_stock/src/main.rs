// The binary delegates entirely to the library crate.
// All modules are compiled once as part of the lib; no duplicate mod declarations here.
//
// cargo-apk compiles all targets including the binary when building for Android,
// but the binary is never executed there — the cdylib's android_main() is the entry point.
// The stub below satisfies the compiler for that target.

#[cfg(not(target_os = "android"))]
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

#[cfg(target_os = "android")]
fn main() {}
