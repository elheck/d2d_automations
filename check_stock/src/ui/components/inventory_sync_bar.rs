//! Shared inventory_sync connection bar.
//!
//! One row: server URL entry, a health-check button, and the connection
//! status. Screens that pull data from the server render this first and add
//! their own controls via the `extra` closure, which receives whether the
//! server is currently connected.

use crate::api::inventory_sync::InventorySyncClient;
use crate::ui::{
    state::{AppState, ConnectionStatus},
    style,
};
use eframe::egui;
use log::{error, info};

pub struct InventorySyncBar;

impl InventorySyncBar {
    /// Polls any in-flight health check and draws the bar.
    pub fn show(
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        app_state: &mut AppState,
        extra: impl FnOnce(&mut egui::Ui, bool),
    ) {
        // Poll health-check channel
        if let Some(rx) = &app_state.inventory_health_rx {
            if let Ok(result) = rx.try_recv() {
                match &result {
                    Ok(()) => info!(
                        "Inventory sync health check succeeded ({})",
                        app_state.inventory_sync_url
                    ),
                    Err(e) => error!(
                        "Inventory sync health check failed ({}): {e}",
                        app_state.inventory_sync_url
                    ),
                }
                app_state.inventory_sync_status = match result {
                    Ok(()) => ConnectionStatus::Connected,
                    Err(e) => ConnectionStatus::Failed(e),
                };
                app_state.inventory_health_rx = None;
            }
        }
        if app_state.inventory_health_rx.is_some() {
            ctx.request_repaint();
        }

        style::section_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Inventory Sync:")
                        .color(style::TEXT_MUTED)
                        .size(12.0),
                );

                let te = egui::TextEdit::singleline(&mut app_state.inventory_sync_url)
                    .hint_text("http://cardscanner.local:3000")
                    .desired_width(260.0);
                ui.add(te);

                let checking =
                    matches!(app_state.inventory_sync_status, ConnectionStatus::Checking);
                let check_label = if checking { "Checking…" } else { "Check" };
                if style::secondary_button(ui, check_label).clicked() && !checking {
                    Self::start_health_check(app_state);
                }

                let mut connected = false;
                match &app_state.inventory_sync_status {
                    ConnectionStatus::Unchecked => {
                        ui.label(
                            egui::RichText::new("not checked")
                                .color(style::TEXT_MUTED)
                                .size(11.0),
                        );
                    }
                    ConnectionStatus::Checking => {
                        ui.spinner();
                    }
                    ConnectionStatus::Connected => {
                        connected = true;
                        ui.label(
                            egui::RichText::new("connected")
                                .color(egui::Color32::from_rgb(60, 190, 90))
                                .size(11.0),
                        );
                    }
                    ConnectionStatus::Failed(msg) => {
                        ui.label(
                            egui::RichText::new(format!("failed: {msg}"))
                                .color(egui::Color32::from_rgb(220, 60, 60))
                                .size(11.0),
                        );
                    }
                }

                extra(ui, connected);
            });
        });
    }

    /// Kicks off a background health check against the configured server.
    pub fn start_health_check(app_state: &mut AppState) {
        info!(
            "Inventory sync: checking connection to {}",
            app_state.inventory_sync_url
        );
        app_state.inventory_sync_status = ConnectionStatus::Checking;
        let (tx, rx) = std::sync::mpsc::channel();
        app_state.inventory_health_rx = Some(rx);
        let client = InventorySyncClient::new(&app_state.inventory_sync_url);
        std::thread::spawn(move || {
            let _ = tx.send(client.health_blocking().map_err(|e| e.to_string()));
        });
    }
}
