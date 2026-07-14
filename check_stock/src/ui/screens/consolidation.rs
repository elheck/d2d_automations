//! Interactive Bin Consolidation Move List
//!
//! Shows the moves from a [`ConsolidationPlan`](crate::bin_consolidation::ConsolidationPlan)
//! as image tiles grouped by the **source bin** the cards come from, so the
//! warehouse operator can clear one bin at a time and tick off each pile as it is
//! moved. Mirrors the interactive picking list.

use crate::bin_consolidation::{to_update_csv, Move};
use crate::cache::ImageCache;
use crate::card_matching::get_card_name;
use crate::models::Language;
use crate::ui::state::Screen;
use crate::ui::style;
use eframe::egui;
use log::{debug, error, warn};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Semaphore;

/// Message sent from background image-loader tasks.
struct LoadedImage {
    image_key: String,
    set_code: String,
    collector_number: String,
    image_data: Vec<u8>,
}

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

    fn image_key(&self) -> String {
        format!("{}_{}", self.set_code.to_lowercase(), self.collector_number)
    }
}

/// State for the interactive consolidation move list.
pub struct ConsolidationState {
    pub items: Vec<ConsolidationItem>,
    pub images: HashMap<String, egui::TextureHandle>,
    pub loading_images: HashSet<String>,
    pub image_cache: ImageCache,
    pub show_done: bool,
    runtime: Runtime,
    image_sender: UnboundedSender<LoadedImage>,
    image_receiver: UnboundedReceiver<LoadedImage>,
    request_semaphore: Arc<Semaphore>,
}

impl Default for ConsolidationState {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        Self {
            items: Vec::new(),
            images: HashMap::new(),
            loading_images: HashSet::new(),
            image_cache: ImageCache::new(),
            show_done: false,
            runtime: Runtime::new().expect("Failed to create Tokio runtime for consolidation"),
            image_sender: tx,
            image_receiver: rx,
            request_semaphore: Arc::new(Semaphore::new(5)),
        }
    }
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
        Self::poll_loaded_images(ctx, state);

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

                let loading = state.loading_images.len();
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
                    Self::show_groups(ctx, ui, state);
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

    fn show_groups(ctx: &egui::Context, ui: &mut egui::Ui, state: &mut ConsolidationState) {
        // Visible items respecting the show-done toggle.
        let visible: Vec<usize> = (0..state.items.len())
            .filter(|&i| !state.items[i].done || state.show_done)
            .collect();

        // Trigger image loading for all visible items.
        for &i in &visible {
            let item = &state.items[i];
            if !state.images.contains_key(&item.image_key())
                && !state.loading_images.contains(&item.image_key())
                && !item.set_code.is_empty()
                && !item.collector_number.is_empty()
            {
                Self::load_card_image(ctx, state, i);
            }
        }

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
    fn show_tile(ui: &mut egui::Ui, state: &ConsolidationState, i: usize) -> Option<bool> {
        let item = &state.items[i];
        let done = item.done;
        let gray = egui::Color32::GRAY;

        ui.vertical(|ui| {
            // Image.
            if let Some(texture) = state.images.get(&item.image_key()) {
                let aspect = texture.size()[0] as f32 / texture.size()[1] as f32;
                let size = egui::vec2(Self::CARD_IMAGE_HEIGHT * aspect, Self::CARD_IMAGE_HEIGHT);
                if done {
                    ui.add(
                        egui::Image::new((texture.id(), size))
                            .tint(egui::Color32::from_rgba_unmultiplied(128, 128, 128, 180)),
                    );
                } else {
                    ui.image((texture.id(), size));
                }
            } else {
                ui.add_sized(
                    [Self::CARD_TILE_WIDTH, Self::CARD_IMAGE_HEIGHT],
                    egui::Label::new(egui::RichText::new("Loading...").weak()),
                );
            }

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

    // ── Image loading (mirrors the picking list) ──────────────────────────────

    fn poll_loaded_images(ctx: &egui::Context, state: &mut ConsolidationState) {
        while let Ok(loaded) = state.image_receiver.try_recv() {
            if let Ok(image) = image::load_from_memory(&loaded.image_data) {
                let rgba = image.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let pixels = rgba.into_raw();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                let texture = ctx.load_texture(
                    format!("consol_{}_{}", loaded.set_code, loaded.collector_number),
                    color_image,
                    egui::TextureOptions::LINEAR,
                );
                state.images.insert(loaded.image_key.clone(), texture);
            } else {
                error!(
                    "Failed to decode image for {}/{}",
                    loaded.set_code, loaded.collector_number
                );
            }
            state.loading_images.remove(&loaded.image_key);
        }
        if !state.loading_images.is_empty() {
            ctx.request_repaint();
        }
    }

    fn load_card_image(ctx: &egui::Context, state: &mut ConsolidationState, item_index: usize) {
        let item = &state.items[item_index];
        let set_code = item.set_code.clone();
        let collector_number = item.collector_number.clone();
        let image_key = item.image_key();

        state.loading_images.insert(image_key.clone());

        // Disk cache first (fast, synchronous).
        if let Some(bytes) = state.image_cache.get(&set_code, &collector_number) {
            if let Ok(image) = image::load_from_memory(&bytes) {
                let rgba = image.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let pixels = rgba.into_raw();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                let texture = ctx.load_texture(
                    format!("consol_{set_code}_{collector_number}"),
                    color_image,
                    egui::TextureOptions::LINEAR,
                );
                state.images.insert(image_key.clone(), texture);
                state.loading_images.remove(&image_key);
                return;
            }
            warn!("Failed to decode cached image for {set_code}/{collector_number}");
        }

        let sender = state.image_sender.clone();
        let cache_dir = state.image_cache.cache_dir().to_path_buf();
        let ctx_clone = ctx.clone();
        let semaphore = state.request_semaphore.clone();

        state.runtime.spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();
            Self::fetch_image_async(
                sender,
                cache_dir,
                set_code,
                collector_number,
                image_key,
                ctx_clone,
            )
            .await;
        });
    }

    async fn fetch_image_async(
        sender: UnboundedSender<LoadedImage>,
        cache_dir: std::path::PathBuf,
        set_code: String,
        collector_number: String,
        image_key: String,
        ctx: egui::Context,
    ) {
        use crate::api::scryfall::{fetch_card_async, fetch_image_async};

        let card = match fetch_card_async(&set_code, &collector_number).await {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to fetch card data for {set_code}/{collector_number}: {e}");
                ctx.request_repaint();
                return;
            }
        };
        let Some(image_url) = card.image_url().map(str::to_string) else {
            warn!("No image URL for {set_code}/{collector_number}");
            ctx.request_repaint();
            return;
        };
        let bytes = match fetch_image_async(&image_url).await {
            Ok(b) => b,
            Err(e) => {
                error!("Failed to fetch image for {set_code}/{collector_number}: {e}");
                ctx.request_repaint();
                return;
            }
        };

        let cache_path = cache_dir.join(format!(
            "{}_{}.jpg",
            set_code.to_lowercase(),
            collector_number
        ));
        if let Err(e) = std::fs::write(&cache_path, &bytes) {
            warn!("Failed to cache image: {e}");
        }
        debug!("Fetched image for {set_code}/{collector_number}");

        let _ = sender.send(LoadedImage {
            image_key,
            set_code,
            collector_number,
            image_data: bytes,
        });
        ctx.request_repaint();
    }
}

#[cfg(test)]
#[path = "consolidation_tests.rs"]
mod tests;
