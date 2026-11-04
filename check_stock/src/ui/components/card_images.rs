//! Card images for tile views (picking list, bulk lots, bin consolidation).
//!
//! Images come from the on-disk cache when present, otherwise from Scryfall in
//! the background (at most [`MAX_CONCURRENT`] requests at once); textures are
//! created on the UI thread as they arrive. A failed load shows a placeholder
//! instead of a spinner that never ends. Printings whose set code or collector
//! number is unsafe for a URL or file name (see [`ImageCache::is_safe_key`])
//! are never requested.

use crate::api::scryfall::{fetch_card_async, fetch_image_async};
use crate::cache::ImageCache;
use eframe::egui;
use log::{debug, warn};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Semaphore;

/// Most Scryfall image requests in flight (Scryfall asks for ≤ 10/s).
pub const MAX_CONCURRENT: usize = 5;

/// Result of one background load; `bytes` is `None` when it failed.
struct Loaded {
    key: String,
    bytes: Option<Vec<u8>>,
}

pub struct CardImages {
    textures: HashMap<String, egui::TextureHandle>,
    loading: HashSet<String>,
    failed: HashSet<String>,
    cache: ImageCache,
    /// Created on the first network fetch, so screens never opened cost no
    /// worker threads.
    runtime: Option<Runtime>,
    tx: UnboundedSender<Loaded>,
    rx: UnboundedReceiver<Loaded>,
    semaphore: Arc<Semaphore>,
}

impl Default for CardImages {
    fn default() -> Self {
        Self::new()
    }
}

impl CardImages {
    pub fn new() -> Self {
        let (tx, rx) = unbounded_channel();
        Self {
            textures: HashMap::new(),
            loading: HashSet::new(),
            failed: HashSet::new(),
            cache: ImageCache::new(),
            runtime: None,
            tx,
            rx,
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT)),
        }
    }

    /// Key of a printing's image: lowercase set code, `_`, collector number.
    pub fn key(set_code: &str, collector_number: &str) -> String {
        format!("{}_{}", set_code.to_lowercase(), collector_number)
    }

    /// Images still being fetched.
    pub fn loading_count(&self) -> usize {
        self.loading.len()
    }

    pub fn texture(&self, set_code: &str, collector_number: &str) -> Option<&egui::TextureHandle> {
        self.textures.get(&Self::key(set_code, collector_number))
    }

    /// The image could not be loaded (or is never requested); draw a placeholder.
    pub fn is_unavailable(&self, set_code: &str, collector_number: &str) -> bool {
        !ImageCache::is_safe_key(set_code, collector_number)
            || self.failed.contains(&Self::key(set_code, collector_number))
    }

    /// Turns finished background loads into textures. Call once per frame.
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(loaded) = self.rx.try_recv() {
            self.loading.remove(&loaded.key);
            match loaded.bytes.as_deref().and_then(decode) {
                Some(image) => {
                    let texture = ctx.load_texture(
                        format!("card_{}", loaded.key),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.textures.insert(loaded.key, texture);
                }
                None => {
                    self.failed.insert(loaded.key);
                }
            }
        }
        if !self.loading.is_empty() {
            ctx.request_repaint();
        }
    }

    /// Starts loading a printing's image unless it is loaded, loading,
    /// failed or unsafe.
    pub fn request(&mut self, ctx: &egui::Context, set_code: &str, collector_number: &str) {
        if !ImageCache::is_safe_key(set_code, collector_number) {
            return;
        }
        let key = Self::key(set_code, collector_number);
        if self.textures.contains_key(&key)
            || self.loading.contains(&key)
            || self.failed.contains(&key)
        {
            return;
        }

        // Disk cache first: fast enough to do synchronously.
        if let Some(bytes) = self.cache.get(set_code, collector_number) {
            if let Some(image) = decode(&bytes) {
                let texture =
                    ctx.load_texture(format!("card_{key}"), image, egui::TextureOptions::LINEAR);
                self.textures.insert(key, texture);
                return;
            }
            warn!("Failed to decode cached image for {set_code}/{collector_number}");
        }

        if self.runtime.is_none() {
            match Runtime::new() {
                Ok(runtime) => self.runtime = Some(runtime),
                Err(e) => {
                    warn!("Cannot load card images: no async runtime ({e})");
                    self.failed.insert(key);
                    return;
                }
            }
        }
        let Some(runtime) = &self.runtime else {
            return;
        };
        self.loading.insert(key.clone());
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let semaphore = self.semaphore.clone();
        let cache_dir = self.cache.cache_dir().to_path_buf();
        let set_code = set_code.to_string();
        let collector_number = collector_number.to_string();
        runtime.spawn(async move {
            let bytes = match semaphore.acquire().await {
                Ok(_permit) => fetch(&set_code, &collector_number).await,
                Err(_) => None,
            };
            if let Some(bytes) = &bytes {
                ImageCache::with_dir(cache_dir).insert(&set_code, &collector_number, bytes);
            }
            let _ = tx.send(Loaded { key, bytes });
            ctx.request_repaint();
        });
    }

    /// Draws a printing's image centred in a `size` box, or a framed
    /// placeholder while it loads or when it is unavailable. Images are only
    /// requested once their box scrolls into view. The response senses
    /// clicks, so callers can make the image a toggle.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        set_code: &str,
        collector_number: &str,
        size: egui::Vec2,
        dimmed: bool,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        if !ui.is_rect_visible(rect) {
            return response;
        }
        self.request(ui.ctx(), set_code, collector_number);
        if let Some(texture) = self.texture(set_code, collector_number) {
            let aspect = texture.size()[0] as f32 / texture.size()[1].max(1) as f32;
            let fitted = egui::vec2((size.y * aspect).min(size.x), size.y);
            let tint = if dimmed {
                egui::Color32::from_rgba_unmultiplied(128, 128, 128, 160)
            } else {
                egui::Color32::WHITE
            };
            egui::Image::new((texture.id(), fitted))
                .tint(tint)
                .corner_radius(egui::CornerRadius::same(8))
                .paint_at(ui, egui::Rect::from_center_size(rect.center(), fitted));
            return response;
        }

        let painter = ui.painter();
        painter.rect(
            rect,
            egui::CornerRadius::same(8),
            egui::Color32::from_rgb(34, 40, 58),
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(52, 62, 88)),
            egui::StrokeKind::Inside,
        );
        let text = if self.is_unavailable(set_code, collector_number) {
            "No image"
        } else {
            "Loading…"
        };
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(13.0),
            egui::Color32::from_rgb(120, 128, 148),
        );
        response
    }
}

fn decode(bytes: &[u8]) -> Option<egui::ColorImage> {
    let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(
        size,
        &rgba.into_raw(),
    ))
}

/// Card metadata, then the image itself, from Scryfall.
async fn fetch(set_code: &str, collector_number: &str) -> Option<Vec<u8>> {
    let card = match fetch_card_async(set_code, collector_number).await {
        Ok(card) => card,
        Err(e) => {
            warn!("Failed to fetch card data for {set_code}/{collector_number}: {e}");
            return None;
        }
    };
    let Some(url) = card.image_url() else {
        warn!("No image URL for {set_code}/{collector_number}");
        return None;
    };
    match fetch_image_async(url).await {
        Ok(bytes) => {
            debug!("Fetched image for {set_code}/{collector_number}");
            Some(bytes)
        }
        Err(e) => {
            warn!("Failed to fetch image for {set_code}/{collector_number}: {e}");
            None
        }
    }
}

#[cfg(test)]
#[path = "card_images_tests.rs"]
mod tests;
