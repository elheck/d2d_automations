//! Interactive Picking List Screen
//!
//! Displays cards to pick with images, allowing users to mark items as picked.
//! Cards are grouped by location for efficient warehouse picking.

use crate::api::scryfall::{fetch_card_async, fetch_image_async};
use crate::cache::ImageCache;
use crate::card_matching::{get_card_name, MatchedCard};
use crate::models::Language;
use crate::ui::state::Screen;
use eframe::egui;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Semaphore;

/// Message sent from background image loader tasks
pub struct LoadedImage {
    pub image_key: String,
    pub set_code: String,
    pub collector_number: String,
    pub image_data: Vec<u8>,
}

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

    /// Generate a cache key for this card's image
    pub fn image_key(&self) -> String {
        format!("{}_{}", self.set_code.to_lowercase(), self.collector_number)
    }
}

/// State for the picking screen
pub struct PickingState {
    /// All items to pick, grouped by location
    pub items: Vec<PickingItem>,
    /// Cached card images (texture handles keyed by set_code_cn)
    pub images: HashMap<String, egui::TextureHandle>,
    /// Images currently being loaded
    pub loading_images: std::collections::HashSet<String>,
    /// Image cache for fetching from disk/network
    pub image_cache: ImageCache,
    /// Whether to show picked items (collapsed)
    pub show_picked: bool,
    /// Total price of all items
    pub total_price: f64,
    /// Price of picked items
    pub picked_price: f64,
    /// Tokio runtime for async operations
    runtime: Runtime,
    /// Channel sender for background image loading
    image_sender: UnboundedSender<LoadedImage>,
    /// Channel receiver for background image loading
    image_receiver: UnboundedReceiver<LoadedImage>,
    /// Semaphore to limit concurrent requests (Scryfall rate limit: 10/sec)
    request_semaphore: Arc<Semaphore>,
}

impl Default for PickingState {
    fn default() -> Self {
        let (tx, rx) = unbounded_channel();
        let runtime = Runtime::new().expect("Failed to create Tokio runtime");
        Self {
            items: Vec::new(),
            images: HashMap::new(),
            loading_images: std::collections::HashSet::new(),
            image_cache: ImageCache::new(),
            show_picked: false,
            total_price: 0.0,
            picked_price: 0.0,
            runtime,
            image_sender: tx,
            image_receiver: rx,
            request_semaphore: Arc::new(Semaphore::new(5)), // Max 5 concurrent requests
        }
    }
}

impl PickingState {
    /// Initialize picking list from matched cards
    pub fn from_matched_cards(matches: &[(String, i32, Vec<MatchedCard<'_>>)]) -> Self {
        let (tx, rx) = unbounded_channel();
        let runtime = Runtime::new().expect("Failed to create Tokio runtime");
        let mut items: Vec<PickingItem> = matches
            .iter()
            .flat_map(|(_, _, cards)| cards.iter().map(PickingItem::from_matched_card))
            .collect();

        // Sort by location for efficient picking
        items.sort_by(|a, b| a.location.cmp(&b.location));

        let total_price: f64 = items.iter().map(|i| i.price * i.quantity as f64).sum();

        Self {
            items,
            images: HashMap::new(),
            loading_images: std::collections::HashSet::new(),
            image_cache: ImageCache::new(),
            show_picked: false,
            total_price,
            picked_price: 0.0,
            runtime,
            image_sender: tx,
            image_receiver: rx,
            request_semaphore: Arc::new(Semaphore::new(5)), // Max 5 concurrent requests
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
        // Poll for loaded images from background tasks (non-blocking)
        Self::poll_loaded_images(ctx, state);

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
                let loading_count = state.loading_images.len();
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
                    Self::show_picking_list(ctx, ui, state);
                });
        });
    }

    const CARD_TILE_WIDTH: f32 = 260.0;
    const CARD_IMAGE_HEIGHT: f32 = 360.0;

    fn show_picking_list(ctx: &egui::Context, ui: &mut egui::Ui, state: &mut PickingState) {
        let mut price_changed = false;

        // Collect all visible item indices
        let visible: Vec<usize> = (0..state.items.len())
            .filter(|&i| !state.items[i].picked || state.show_picked)
            .collect();

        // Trigger image loading for all visible items
        for &i in &visible {
            let item = &state.items[i];
            let image_key = item.image_key();
            if !state.images.contains_key(&image_key)
                && !state.loading_images.contains(&image_key)
                && !item.set_code.is_empty()
                && !item.collector_number.is_empty()
            {
                Self::load_card_image(ctx, state, i);
            }
        }

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
                    let image_key = item.image_key();
                    let gray = egui::Color32::GRAY;

                    // Each grid cell: vertical group with image on top, info below
                    let response = ui
                        .vertical(|ui| {
                            // Card image
                            if let Some(texture) = state.images.get(&image_key) {
                                let aspect = texture.size()[0] as f32 / texture.size()[1] as f32;
                                let width = Self::CARD_IMAGE_HEIGHT * aspect;
                                let size = egui::vec2(width, Self::CARD_IMAGE_HEIGHT);

                                if picked {
                                    ui.add(egui::Image::new((texture.id(), size)).tint(
                                        egui::Color32::from_rgba_unmultiplied(128, 128, 128, 180),
                                    ));
                                } else {
                                    ui.image((texture.id(), size));
                                }
                            } else {
                                ui.add_sized(
                                    [Self::CARD_TILE_WIDTH, Self::CARD_IMAGE_HEIGHT],
                                    egui::Label::new(egui::RichText::new("Loading...").weak()),
                                );
                            }

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

    /// Poll the channel for loaded images and create textures (non-blocking)
    fn poll_loaded_images(ctx: &egui::Context, state: &mut PickingState) {
        // Process all available loaded images (non-blocking)
        while let Ok(loaded) = state.image_receiver.try_recv() {
            debug!(
                "Received loaded image for {}/{}",
                loaded.set_code, loaded.collector_number
            );

            // Create texture from image data
            if let Ok(image) = image::load_from_memory(&loaded.image_data) {
                let rgba = image.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let pixels = rgba.into_raw();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);

                let texture = ctx.load_texture(
                    format!("pick_{}_{}", loaded.set_code, loaded.collector_number),
                    color_image,
                    egui::TextureOptions::LINEAR,
                );

                state.images.insert(loaded.image_key.clone(), texture);
                info!(
                    "Created texture for {}/{}",
                    loaded.set_code, loaded.collector_number
                );
            } else {
                error!(
                    "Failed to decode image for {}/{}",
                    loaded.set_code, loaded.collector_number
                );
            }

            state.loading_images.remove(&loaded.image_key);
        }

        // Request repaint if still loading images
        if !state.loading_images.is_empty() {
            ctx.request_repaint();
        }
    }

    /// Spawn a tokio task to load a card image
    fn load_card_image(ctx: &egui::Context, state: &mut PickingState, item_index: usize) {
        let item = &state.items[item_index];
        let set_code = item.set_code.clone();
        let collector_number = item.collector_number.clone();
        let image_key = item.image_key();

        debug!(
            "Starting async load for {}/{} (key: {})",
            set_code, collector_number, image_key
        );

        // Mark as loading
        state.loading_images.insert(image_key.clone());

        // Try to load from disk cache first (this is fast, keep synchronous)
        if let Some(bytes) = state.image_cache.get(&set_code, &collector_number) {
            info!(
                "Image cache HIT for {}/{} - loading from disk",
                set_code, collector_number
            );
            if let Ok(image) = image::load_from_memory(&bytes) {
                let rgba = image.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let pixels = rgba.into_raw();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);

                let texture = ctx.load_texture(
                    format!("pick_{}_{}", set_code, collector_number),
                    color_image,
                    egui::TextureOptions::LINEAR,
                );

                state.images.insert(image_key.clone(), texture);
                state.loading_images.remove(&image_key);
                return;
            } else {
                warn!(
                    "Failed to decode cached image for {}/{}",
                    set_code, collector_number
                );
            }
        }

        info!(
            "Image cache MISS for {}/{} - spawning tokio task",
            set_code, collector_number
        );

        // Spawn tokio task to fetch from network with rate limiting
        let sender = state.image_sender.clone();
        let cache_dir = state.image_cache.cache_dir().to_path_buf();
        let ctx_clone = ctx.clone();
        let semaphore = state.request_semaphore.clone();

        state.runtime.spawn(async move {
            // Acquire semaphore permit for rate limiting (max 5 concurrent requests)
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

    /// Async function to fetch an image from Scryfall
    async fn fetch_image_async(
        sender: UnboundedSender<LoadedImage>,
        cache_dir: std::path::PathBuf,
        set_code: String,
        collector_number: String,
        image_key: String,
        ctx: egui::Context,
    ) {
        debug!(
            "Async: Fetching card data for {}/{}",
            set_code, collector_number
        );

        // Fetch card metadata to get the image URL (handles DFCs via ScryfallCard::image_url())
        let card = match fetch_card_async(&set_code, &collector_number).await {
            Ok(c) => c,
            Err(e) => {
                error!(
                    "Async: Failed to fetch card data for {}/{}: {}",
                    set_code, collector_number, e
                );
                ctx.request_repaint();
                return;
            }
        };

        let image_url = match card.image_url() {
            Some(url) => url.to_string(),
            None => {
                warn!(
                    "Async: No image URL found for {}/{}",
                    set_code, collector_number
                );
                ctx.request_repaint();
                return;
            }
        };

        debug!("Async: Fetching image from: {}", image_url);

        // Fetch the image bytes
        let bytes = match fetch_image_async(&image_url).await {
            Ok(b) => b,
            Err(e) => {
                error!(
                    "Async: Failed to fetch image for {}/{}: {}",
                    set_code, collector_number, e
                );
                ctx.request_repaint();
                return;
            }
        };

        // Save to disk cache
        let cache_path = cache_dir.join(format!(
            "{}_{}.jpg",
            set_code.to_lowercase(),
            collector_number
        ));
        if let Err(e) = std::fs::write(&cache_path, &bytes) {
            warn!("Failed to cache image: {}", e);
        }

        info!("Async: Fetched image for {}/{}", set_code, collector_number);

        // Send to main thread and request repaint
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
#[path = "picking_tests.rs"]
mod tests;
