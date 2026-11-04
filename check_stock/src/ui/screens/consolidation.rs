//! Interactive Bin Consolidation Move List
//!
//! Shows the moves from a [`ConsolidationPlan`](crate::bin_consolidation::ConsolidationPlan)
//! as image tiles grouped by the **source bin** the cards come from, so the
//! warehouse operator can clear one bin at a time and tick off each pile as it is
//! moved. Mirrors the interactive picking list.

use crate::bin_consolidation::{to_update_csv, Move};
use crate::card_matching::get_card_name;
use crate::models::Language;
use crate::ui::components::CardImages;
use crate::ui::state::Screen;
use crate::ui::style;
use eframe::egui;
use log::error;

/// One suggested move rendered as an interactive tile.
#[derive(Clone)]
pub struct ConsolidationItem {
    pub card_name: String,
    pub set_name: String,
    pub set_code: String,
    pub collector_number: String,
    pub condition: String,
    pub language: String,
    pub quantity: i32,
    pub is_foil: bool,
    pub from_location: String,
    pub to_location: String,
    pub from_bin: String,
    pub to_bin: String,
    /// Ticked once the operator has physically moved this pile.
    pub done: bool,
    /// The underlying move, kept so the "moved" export carries full card data.
    source: Move,
}

impl ConsolidationItem {
    fn from_move(m: &Move) -> Self {
        let lang = Language::parse(&m.card.language);
        Self {
            card_name: get_card_name(&m.card, lang).to_string(),
            set_name: m.card.set.clone(),
            set_code: m.card.set_code.clone(),
            collector_number: m.card.cn.clone(),
            condition: m.card.condition.clone(),
            language: m.card.language.clone(),
            quantity: m.quantity.max(0) as i32,
            is_foil: m.card.is_foil_card(),
            from_location: m.from_location.clone(),
            to_location: m.to_location.clone(),
            from_bin: m.from_bin.clone(),
            to_bin: m.to_bin.clone(),
            done: false,
            source: m.clone(),
        }
    }
}

/// State for the interactive consolidation move list.
#[derive(Default)]
pub struct ConsolidationState {
    pub items: Vec<ConsolidationItem>,
    pub images: CardImages,
    pub show_done: bool,
}

impl ConsolidationState {
    /// Builds the move list from a plan's moves, grouped by source bin.
    pub fn from_moves(moves: &[Move]) -> Self {
        let mut items: Vec<ConsolidationItem> =
            moves.iter().map(ConsolidationItem::from_move).collect();
        // Group by source bin, then by destination for a stable within-group order.
        items.sort_by(|a, b| {
            a.from_bin
                .cmp(&b.from_bin)
                .then_with(|| a.to_bin.cmp(&b.to_bin))
                .then_with(|| a.from_location.cmp(&b.from_location))
        });
        Self {
            items,
            ..Default::default()
        }
    }

    fn done_count(&self) -> usize {
        self.items.iter().filter(|i| i.done).count()
    }

    fn total_count(&self) -> usize {
        self.items.len()
    }

    /// The underlying moves for the piles marked as moved — the export payload.
    fn moved_moves(&self) -> Vec<Move> {
        self.items
            .iter()
            .filter(|i| i.done)
            .map(|i| i.source.clone())
            .collect()
    }
}

pub struct ConsolidationScreen;

impl ConsolidationScreen {
    const CARD_TILE_WIDTH: f32 = 240.0;
    const CARD_IMAGE_HEIGHT: f32 = 320.0;

    pub fn show(ctx: &egui::Context, current_screen: &mut Screen, state: &mut ConsolidationState) {
        state.images.poll(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("← Back to Bin Analysis").clicked() {
                    *current_screen = Screen::BinAnalysis;
                }
                ui.add_space(20.0);

                let done = state.done_count();
                let total = state.total_count();
                let progress = if total > 0 {
                    done as f32 / total as f32
                } else {
                    0.0
                };
                ui.label(format!("Moved: {done}/{total}"));
                ui.add(
                    egui::ProgressBar::new(progress)
                        .desired_width(150.0)
                        .show_percentage(),
                );

                let loading = state.images.loading_count();
                if loading > 0 {
                    ui.add_space(10.0);
                    ui.spinner();
                    ui.label(format!("Loading {loading} images..."));
                }
            });

            ui.add_space(5.0);

            ui.horizontal(|ui| {
                ui.checkbox(&mut state.show_done, "Show completed");
                ui.add_space(20.0);
                if ui.button("Reset All").clicked() {
                    for item in &mut state.items {
                        item.done = false;
                    }
                }
                if ui.button("Mark All Moved").clicked() {
                    for item in &mut state.items {
                        item.done = true;
                    }
                }

                ui.add_space(20.0);
                let moved = state.done_count();
                let export = ui.add_enabled(
                    moved > 0,
                    egui::Button::new(format!("Export Moved CSV ({moved})")),
                );
                if export
                    .on_hover_text("Export only the piles you have marked as moved")
                    .clicked()
                {
                    Self::export_moved(state);
                }
            });

            ui.separator();

            if state.items.is_empty() {
                ui.add_space(20.0);
                ui.label("No consolidation moves. Run 'Suggest Consolidation' first.");
                return;
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    Self::show_groups(ui, state);
                });
        });
    }

    /// Writes a stock-update CSV of the moved piles to a user-chosen file.
    fn export_moved(state: &ConsolidationState) {
        let csv = to_update_csv(&state.moved_moves());
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("bin_consolidation_moved.csv")
            .add_filter("CSV", &["csv"])
            .save_file()
        {
            if let Err(e) = std::fs::write(&path, csv) {
                error!("Failed to save moved-piles CSV: {e}");
            }
        }
    }

    fn show_groups(ui: &mut egui::Ui, state: &mut ConsolidationState) {
        // Visible items respecting the show-done toggle.
        let visible: Vec<usize> = (0..state.items.len())
            .filter(|&i| !state.items[i].done || state.show_done)
            .collect();

        // Partition visible indices into consecutive groups by source bin.
        let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
        for &i in &visible {
            let bin = state.items[i].from_bin.clone();
            match groups.last_mut() {
                Some((b, idxs)) if *b == bin => idxs.push(i),
                _ => groups.push((bin, vec![i])),
            }
        }

        let cols = ((ui.available_width() / Self::CARD_TILE_WIDTH).floor() as usize).max(1);

        for (bin, idxs) in groups {
            let cards: i32 = idxs.iter().map(|&i| state.items[i].quantity).sum();
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(format!(
                    "From bin {bin}  —  {} piles, {cards} cards",
                    idxs.len()
                ))
                .size(16.0)
                .strong()
                .color(style::TEXT_PRIMARY),
            );
            ui.separator();

            egui::Grid::new(format!("consolidation_grid_{bin}"))
                .num_columns(cols)
                .min_col_width(Self::CARD_TILE_WIDTH)
                .max_col_width(Self::CARD_TILE_WIDTH)
                .spacing([10.0, 15.0])
                .show(ui, |ui| {
                    for (col_idx, &i) in idxs.iter().enumerate() {
                        if let Some(new_done) = Self::show_tile(ui, state, i) {
                            state.items[i].done = new_done;
                        }
                        if (col_idx + 1) % cols == 0 {
                            ui.end_row();
                        }
                    }
                    let rem = idxs.len() % cols;
                    if rem != 0 {
                        ui.end_row();
                    }
                });
        }
    }

    /// Renders one move tile; returns `Some(new_done)` if its button was clicked.
    fn show_tile(ui: &mut egui::Ui, state: &mut ConsolidationState, i: usize) -> Option<bool> {
        let item = &state.items[i];
        let done = item.done;
        let gray = egui::Color32::GRAY;
        let images = &mut state.images;

        ui.vertical(|ui| {
            // Image.
            images.show(
                ui,
                &item.set_code,
                &item.collector_number,
                egui::vec2(Self::CARD_TILE_WIDTH, Self::CARD_IMAGE_HEIGHT),
                done,
            );

            // Destination (the key action).
            let dest = egui::RichText::new(format!("→ {}", item.to_location))
                .size(15.0)
                .strong()
                .color(if done { gray } else { style::COLOR_SUCCESS });
            ui.label(dest);

            // Origin (which pile to grab), muted.
            ui.label(
                egui::RichText::new(format!("from {}", item.from_location))
                    .size(12.0)
                    .color(style::TEXT_MUTED),
            );

            // Card name.
            let name = if done {
                egui::RichText::new(&item.card_name)
                    .size(15.0)
                    .strikethrough()
                    .color(gray)
            } else {
                egui::RichText::new(&item.card_name).size(15.0).strong()
            };
            ui.label(name);

            // Details.
            let info = format!(
                "{} • {}\nQty: {} • {}{}",
                item.set_name,
                item.condition,
                item.quantity,
                item.language,
                if item.is_foil { " • Foil" } else { "" }
            );
            ui.label(egui::RichText::new(info).size(13.0).color(if done {
                gray
            } else {
                style::TEXT_PRIMARY
            }));

            // Toggle.
            if done {
                if ui.button("Undo").clicked() {
                    return Some(false);
                }
            } else if ui.button("Mark Moved").clicked() {
                return Some(true);
            }
            None
        })
        .inner
    }
}

#[cfg(test)]
#[path = "consolidation_tests.rs"]
mod tests;
