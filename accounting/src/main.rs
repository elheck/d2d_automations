mod app;
mod csv_processor;
mod models;
mod sevdesk_api;

use eframe::egui;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> Result<(), eframe::Error> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sevdesk_invoicing=debug,info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    log::info!("Starting SevDesk Invoice Creator");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 700.0])
            .with_min_inner_size([600.0, 400.0]),
        ..Default::default()
    };

    log::info!("Launching GUI application");
    
    eframe::run_native(
        "SevDesk Invoice Creator",
        options,
        Box::new(|_cc| {
            log::debug!("Creating application instance");
            Ok(Box::new(app::InvoiceApp::default()))
        }),
    )
}
