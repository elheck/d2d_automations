//! Interactive Picking List Screen
//!
//! Displays cards to pick with images, allowing users to mark items as picked.
//! Cards are grouped by location for efficient warehouse picking.

use crate::card_matching::{get_card_name, MatchedCard};
use crate::models::Language;
use crate::ui::components::CardImages;
use crate::ui::state::Screen;
use eframe::egui;

/// A card in the picking list with its picking state
#[derive(Clone)]
pub struct PickingItem {
    pub card_name: String,
    pub set_name: String,
    pub set_code: String,
    pub collector_number: String,
    pub condition: String,
    pub language: String,
    pub quantity: i32,
    pub price: f64,
    pub location: String,
    pub is_foil: bool,
    pub picked: bool,
}

impl PickingItem {
    pub fn from_matched_card(mc: &MatchedCard<'_>) -> Self {
        let lang = Language::parse(&mc.card.language);
        let card_name = get_card_name(mc.card, lang).to_string();
        Self {
            card_name,
            set_name: mc.set_name.clone(),
            set_code: mc.card.set_code.clone(),
            collector_number: mc.card.cn.clone(),
            condition: mc.card.condition.clone(),
            language: mc.card.language.clone(),
            quantity: mc.quantity,
            price: mc.card.price.parse().unwrap_or(0.0),
            location: mc.card.location.clone().unwrap_or_default(),
            is_foil: mc.card.is_foil_card(),
            picked: false,
        }
    }
}

/// State for the picking screen
#[derive(Default)]
pub struct PickingState {
    /// All items to pick, grouped by location
    pub items: Vec<PickingItem>,
    /// Card images (disk cache / Scryfall)
    pub images: CardImages,
    /// Whether to show picked items (collapsed)
    pub show_picked: bool,
    /// Total price of all items
    pub total_price: f64,
    /// Price of picked items
    pub picked_price: f64,
}

impl PickingState {
    /// Initialize picking list from matched cards
    pub fn from_matched_cards(matches: &[(String, i32, Vec<MatchedCard<'_>>)]) -> Self {
        let mut items: Vec<PickingItem> = matches
            .iter()
            .flat_map(|(_, _, cards)| cards.iter().map(PickingItem::from_matched_card))
            .collect();

        // Sort by location for efficient picking
        items.sort_by(|a, b| a.location.cmp(&b.location));

        let total_price: f64 = items.iter().map(|i| i.price * i.quantity as f64).sum();

        Self {
            items,
            total_price,
            ..Self::default()
        }
    }

    /// Count of picked items
    pub fn picked_count(&self) -> usize {
        self.items.iter().filter(|i| i.picked).count()
    }

    /// Count of total items
    pub fn total_count(&self) -> usize {
        self.items.len()
    }

    /// Recalculate picked price
    pub fn update_picked_price(&mut self) {
        self.picked_price = self
            .items
            .iter()
            .filter(|i| i.picked)
            .map(|i| i.price * i.quantity as f64)
            .sum();
    }
}

pub struct PickingScreen;

impl PickingScreen {
    pub fn show(ctx: &egui::Context, current_screen: &mut Screen, state: &mut PickingState) {
        // Turn finished background image loads into textures (non-blocking)
        state.images.poll(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            // Header with back button and progress
            ui.horizontal(|ui| {
                if ui.button("← Back to Stock Checker").clicked() {
                    *current_screen = Screen::StockChecker;
                }

                ui.add_space(20.0);

                // Progress indicator
                let picked = state.picked_count();
                let total = state.total_count();
                let progress = if total > 0 {
                    picked as f32 / total as f32
                } else {
                    0.0
                };

                ui.label(format!("Progress: {}/{}", picked, total));
                ui.add(
                    egui::ProgressBar::new(progress)
                        .desired_width(150.0)
                        .show_percentage(),
                );

                ui.add_space(20.0);

                // Price info
                ui.label(format!(
                    "Picked: {:.2} € / {:.2} €",
                    state.picked_price, state.total_price
                ));

                // Loading indicator
                let loading_count = state.images.loading_count();
                if loading_count > 0 {
                    ui.add_space(10.0);
                    ui.spinner();
                    ui.label(format!("Loading {} images...", loading_count));
                }
            });

            ui.add_space(5.0);

            // Controls
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.show_picked, "Show picked items");

                ui.add_space(20.0);

                if ui.button("Reset All").clicked() {
                    for item in &mut state.items {
                        item.picked = false;
                    }
                    state.update_picked_price();
                }

                if ui.button("Mark All Picked").clicked() {
                    for item in &mut state.items {
                        item.picked = true;
                    }
                    state.update_picked_price();
                }
            });

            ui.separator();

            // Picking list
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    Self::show_picking_list(ui, state);
                });
        });
    }

    const CARD_TILE_WIDTH: f32 = 260.0;
    const CARD_IMAGE_HEIGHT: f32 = 360.0;

    fn show_picking_list(ui: &mut egui::Ui, state: &mut PickingState) {
        let mut price_changed = false;

        // Collect all visible item indices
        let visible: Vec<usize> = (0..state.items.len())
            .filter(|&i| !state.items[i].picked || state.show_picked)
            .collect();

        let available_width = ui.available_width();
        let cols = ((available_width / Self::CARD_TILE_WIDTH).floor() as usize).max(1);

        // Single grid for all cards
        egui::Grid::new("picking_grid")
            .num_columns(cols)
            .min_col_width(Self::CARD_TILE_WIDTH)
            .max_col_width(Self::CARD_TILE_WIDTH)
            .spacing([10.0, 15.0])
            .show(ui, |ui| {
                for (col_idx, &i) in visible.iter().enumerate() {
                    let item = &state.items[i];
                    let picked = item.picked;
                    let gray = egui::Color32::GRAY;
                    let images = &mut state.images;

                    // Each grid cell: vertical group with image on top, info below
                    let response = ui
                        .vertical(|ui| {
                            // Card image
                            images.show(
                                ui,
                                &item.set_code,
                                &item.collector_number,
                                egui::vec2(Self::CARD_TILE_WIDTH, Self::CARD_IMAGE_HEIGHT),
                                picked,
                            );

                            // Location (prominent)
                            let loc_text = if item.location.is_empty() {
                                "No location".to_string()
                            } else {
                                item.location.clone()
                            };
                            let loc_style = if picked {
                                egui::RichText::new(&loc_text)
                                    .size(15.0)
                                    .strong()
                                    .color(gray)
                            } else {
                                egui::RichText::new(&loc_text)
                                    .size(15.0)
                                    .strong()
                                    .color(egui::Color32::LIGHT_BLUE)
                            };
                            ui.label(loc_style);

                            // Card name
                            let name_text = if picked {
                                egui::RichText::new(&item.card_name)
                                    .size(16.0)
                                    .strikethrough()
                                    .color(gray)
                            } else {
                                egui::RichText::new(&item.card_name).size(16.0).strong()
                            };
                            ui.label(name_text);

                            // Card details
                            let info = format!(
                                "{} • {}\nQty: {} • {:.2} €\n{}{}",
                                item.set_name,
                                item.condition,
                                item.quantity,
                                item.price,
                                item.language,
                                if item.is_foil { " • Foil" } else { "" }
                            );
                            let info_text = if picked {
                                egui::RichText::new(info).size(14.0).color(gray)
                            } else {
                                egui::RichText::new(info).size(14.0)
                            };
                            ui.label(info_text);

                            // Pick/Undo button
                            if picked {
                                if ui.button("Undo").clicked() {
                                    return Some(false);
                                }
                            } else if ui.button("Pick").clicked() {
                                return Some(true);
                            }
                            None
                        })
                        .inner;

                    if let Some(new_picked) = response {
                        state.items[i].picked = new_picked;
                        price_changed = true;
                    }

                    // End row after filling all columns
                    if (col_idx + 1) % cols == 0 {
                        ui.end_row();
                    }
                }
            });

        if price_changed {
            state.update_picked_price();
        }
    }
}

#[cfg(test)]
#[path = "picking_tests.rs"]
mod tests;
