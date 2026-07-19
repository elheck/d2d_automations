use crate::inventory_db::VisitDigest;
use crate::ui::state::{AppState, Screen};
use eframe::egui;

pub struct WelcomeScreen;

const TILES: [(&str, &str); 10] = [
    ("Stock Checker", "Verify card stock\nagainst order lists"),
    ("Stock Analysis", "Analyse inventory\ntrends and signals"),
    ("Bin Analysis", "Bin capacity and\nfree-slot analysis"),
    ("Magic Singles Listing", "Generate listings\nfor Cardmarket"),
    ("Search Cards", "Find cards by name\nor location"),
    ("Pricing", "Price stock from\nCSV inventory"),
    ("Card Buy Helper", "Value a buy offer\nfrom a card export"),
    ("Mispricing Report", "Compare listings\nto market prices"),
    (
        "Restock Report",
        "Sold-out fast sellers\nworth buying again",
    ),
    ("Price Movers", "7/30-day market moves\nacross your stock"),
];

impl WelcomeScreen {
    pub fn show(ui: &mut egui::Ui, app_state: &mut AppState) {
        // Computed once per app run; local SQLite, so a blocking read is fine.
        if app_state.digest.is_none() {
            app_state.digest = Some(crate::inventory_db::visit_digest().map_err(|e| e.to_string()));
        }
        let current_screen = &mut app_state.current_screen;
        let available = ui.available_size();
        ui.vertical_centered(|ui| {
            ui.add_space(available.y * 0.10);

            ui.label(
                egui::RichText::new("D2D Automations")
                    .size(28.0)
                    .strong()
                    .color(egui::Color32::from_rgb(220, 220, 230)),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("Select a tool to get started")
                    .size(14.0)
                    .color(egui::Color32::from_rgb(150, 150, 165)),
            );

            ui.add_space(12.0);
            if let Some(Ok(digest)) = &app_state.digest {
                Self::show_digest(ui, digest);
            }
            ui.add_space(20.0);

            let tile_w = 200.0_f32;
            let tile_h = 110.0_f32;
            let gap = 16.0_f32;
            let grid_w = tile_w * 3.0 + gap * 2.0;
            let rows = (TILES.len() as f32 / 3.0).ceil();

            let mut clicked: Option<usize> = None;

            ui.allocate_ui_with_layout(
                egui::vec2(grid_w, tile_h * rows + gap * (rows - 1.0)),
                egui::Layout::left_to_right(egui::Align::TOP).with_main_wrap(true),
                |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);

                    for (i, (label, description)) in TILES.iter().enumerate() {
                        if Self::tile_button(ui, label, description, egui::vec2(tile_w, tile_h)) {
                            clicked = Some(i);
                        }
                    }
                },
            );

            if let Some(i) = clicked {
                *current_screen = match i {
                    0 => Screen::StockChecker,
                    1 => Screen::StockAnalysis,
                    2 => Screen::BinAnalysis,
                    3 => Screen::StockListing,
                    4 => Screen::Search,
                    5 => Screen::Pricing,
                    6 => Screen::BuyHelper,
                    7 => Screen::Mispricing,
                    8 => Screen::Restock,
                    _ => Screen::Movers,
                };
            }
        });
    }

    /// Compact "since your last visit" line under the title: sales, new
    /// listings, restock candidates, and how fresh the local inventory is.
    fn show_digest(ui: &mut egui::Ui, d: &VisitDigest) {
        let mut parts: Vec<String> = Vec::new();
        if let Some(since) = &d.since {
            if d.sold_copies > 0 {
                parts.push(format!(
                    "{} copies sold (€{:.2}) since {}",
                    d.sold_copies, d.sold_revenue, since
                ));
            } else {
                parts.push(format!("no sales recorded since {since}"));
            }
            if d.new_listings > 0 {
                parts.push(format!("{} new listings", d.new_listings));
            }
        }
        if d.restock_candidates > 0 {
            parts.push(format!("{} restock candidates", d.restock_candidates));
        }
        if !parts.is_empty() {
            ui.label(
                egui::RichText::new(parts.join(" · "))
                    .size(12.0)
                    .color(egui::Color32::from_rgb(150, 158, 178)),
            );
        }

        // Freshness of the local inventory mirror — the one thing worth a
        // warning color, since every report depends on it.
        let today = chrono::Local::now().date_naive();
        let sync_age = d.last_sync.as_deref().and_then(|s| {
            chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .ok()
                .map(|date| (today - date).num_days())
        });
        let (text, warn) = match sync_age {
            None => (
                "No inventory synced yet — load a Cardmarket CSV".to_string(),
                false,
            ),
            Some(age) if age > 7 => (
                format!("Last inventory sync {age} days ago — import a fresh CSV"),
                true,
            ),
            Some(0) => ("Inventory synced today".to_string(), false),
            Some(age) => (format!("Last inventory sync {age} days ago"), false),
        };
        ui.label(egui::RichText::new(text).size(12.0).color(if warn {
            egui::Color32::from_rgb(230, 170, 80)
        } else {
            egui::Color32::from_rgb(120, 128, 148)
        }));
    }

    fn tile_button(ui: &mut egui::Ui, label: &str, description: &str, size: egui::Vec2) -> bool {
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

        let hovered = response.hovered();
        let pressed = response.is_pointer_button_down_on();

        let bg = if pressed {
            egui::Color32::from_rgb(55, 65, 90)
        } else if hovered {
            egui::Color32::from_rgb(50, 58, 82)
        } else {
            egui::Color32::from_rgb(38, 44, 62)
        };

        let border = if hovered {
            egui::Color32::from_rgb(100, 130, 200)
        } else {
            egui::Color32::from_rgb(60, 68, 92)
        };

        let painter = ui.painter();

        painter.rect(
            rect,
            egui::CornerRadius::same(10),
            bg,
            egui::Stroke::new(1.5_f32, border),
            egui::StrokeKind::Inside,
        );

        // Accent bar on left edge
        let accent = egui::Color32::from_rgb(80, 120, 220);
        let bar = egui::Rect::from_min_size(
            rect.min + egui::vec2(0.0, 12.0),
            egui::vec2(3.0, size.y - 24.0),
        );
        painter.rect_filled(bar, egui::CornerRadius::same(2), accent);

        let text_x = rect.min.x + 18.0;

        let label_color = if hovered {
            egui::Color32::WHITE
        } else {
            egui::Color32::from_rgb(210, 215, 230)
        };

        painter.text(
            egui::pos2(text_x, rect.min.y + 24.0),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::proportional(16.0),
            label_color,
        );

        painter.text(
            egui::pos2(text_x, rect.min.y + 52.0),
            egui::Align2::LEFT_TOP,
            description,
            egui::FontId::proportional(12.0),
            egui::Color32::from_rgb(130, 140, 165),
        );

        response.clicked()
    }
}
