//! Card data and images from Scryfall, cached on the server.
//!
//! The browser never talks to Scryfall: it asks this server, which looks the
//! printing up once and keeps it (`<data>/cache/scryfall_cache.json` and
//! `<data>/cache/images/`, the same layout as the desktop app's cache root,
//! so its caches can be copied over).
//!
//! Safety rules:
//! - Only set codes and collector numbers made of letters, digits and `-`
//!   are looked up ([`ImageCache::is_safe_key`]); they end up in a URL path
//!   and a file name.
//! - Images are only downloaded from Scryfall's image host
//!   ([`is_scryfall_image_url`]), without following redirects, at most
//!   [`MAX_IMAGE_BYTES`], and only JPEG or PNG data is kept and served.
//! - Requests to Scryfall's API are spaced [`SCRYFALL_SPACING`] apart for the
//!   whole server, so users cannot get the server banned there.
//! - Printings Scryfall does not know are not cached: made-up keys never
//!   fill the disk.

use check_stock_core::api::scryfall::{fetch_card_from_async, is_scryfall_image_url, ScryfallCard};
use check_stock_core::cache::{CardCache, ImageCache};
use check_stock_core::error::ApiError as CoreError;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::time::Instant;

/// Scryfall's API.
pub const SCRYFALL_API: &str = "https://api.scryfall.com";

/// Pause between two requests to Scryfall's API (Scryfall asks for 50–100 ms).
pub const SCRYFALL_SPACING: Duration = Duration::from_millis(100);

/// Largest image accepted from Scryfall (their `normal` JPEGs are ~150 KB).
pub const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;

/// Timeout for one image download.
const IMAGE_TIMEOUT: Duration = Duration::from_secs(20);

/// Sent to Scryfall, which requires an identifying user agent.
const USER_AGENT: &str = "D2D-Automations/1.0 (check_stock_server)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupError {
    /// Set code or collector number with characters other than `A-Z 0-9 -`.
    InvalidKey,
    /// Scryfall knows no such printing.
    NotFound,
    /// Scryfall has no (acceptable) image for it.
    NoImage,
    /// Scryfall could not be reached or answered with an error.
    Unavailable,
}

/// Kind of image data, by its first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Jpeg,
    Png,
}

impl ImageKind {
    /// JPEG or PNG; anything else (HTML error pages, SVG, …) is `None`.
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(Self::Jpeg)
        } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
            Some(Self::Png)
        } else {
            None
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
        }
    }
}

/// Looks up cards and their images, with the server's caches in front.
pub struct CardLookup {
    scryfall_base: String,
    allow_image_url: fn(&str) -> bool,
    cards: Arc<Mutex<CardCache>>,
    images: Arc<ImageCache>,
    /// Earliest time the next Scryfall API request may start.
    next_request: tokio::sync::Mutex<Instant>,
    spacing: Duration,
    image_client: reqwest::Client,
}

impl CardLookup {
    /// Real Scryfall, caches under `cache_root`.
    pub fn new(cache_root: &Path) -> Self {
        Self::with_endpoints(
            cache_root,
            SCRYFALL_API,
            is_scryfall_image_url,
            SCRYFALL_SPACING,
        )
    }

    /// Another Scryfall base URL, image URL rule and spacing (tests).
    pub fn with_endpoints(
        cache_root: &Path,
        scryfall_base: &str,
        allow_image_url: fn(&str) -> bool,
        spacing: Duration,
    ) -> Self {
        let image_client = reqwest::Client::builder()
            .timeout(IMAGE_TIMEOUT)
            // A redirect could lead anywhere; the URL was checked, not where
            // it points to.
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_else(|e| {
                log::error!("Could not build the image client ({e}); using the default one");
                reqwest::Client::new()
            });
        Self {
            scryfall_base: scryfall_base.trim_end_matches('/').to_string(),
            allow_image_url,
            cards: Arc::new(Mutex::new(CardCache::load_in(cache_root))),
            images: Arc::new(ImageCache::in_root(cache_root)),
            next_request: tokio::sync::Mutex::new(Instant::now()),
            spacing,
            image_client,
        }
    }

    /// The printing `set_code` / `collector_number`.
    pub async fn card(
        &self,
        set_code: &str,
        collector_number: &str,
    ) -> Result<ScryfallCard, LookupError> {
        if !ImageCache::is_safe_key(set_code, collector_number) {
            return Err(LookupError::InvalidKey);
        }
        if let Some(card) = self.cached_card(set_code, collector_number) {
            return Ok(card);
        }
        self.wait_for_turn().await;
        // Another request may have fetched it while this one waited.
        if let Some(card) = self.cached_card(set_code, collector_number) {
            return Ok(card);
        }
        let card = fetch_card_from_async(&self.scryfall_base, set_code, collector_number)
            .await
            .map_err(|e| {
                if is_not_found(&e) {
                    LookupError::NotFound
                } else {
                    log::warn!("Scryfall lookup of {set_code}/{collector_number} failed: {e}");
                    LookupError::Unavailable
                }
            })?;
        let cards = self.cards.clone();
        let (set, number, keep) = (
            set_code.to_string(),
            collector_number.to_string(),
            card.clone(),
        );
        // Saving rewrites the cache file: not on an async worker.
        let saved = tokio::task::spawn_blocking(move || {
            let mut cache = lock(&cards);
            cache.insert(&set, &number, keep);
            cache.save().map_err(|e| e.to_string())
        })
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
        if let Err(e) = saved {
            log::warn!("Could not save the card cache: {e}");
        }
        Ok(card)
    }

    /// The card image (JPEG or PNG bytes) and its kind.
    pub async fn image(
        &self,
        set_code: &str,
        collector_number: &str,
    ) -> Result<(Vec<u8>, ImageKind), LookupError> {
        if !ImageCache::is_safe_key(set_code, collector_number) {
            return Err(LookupError::InvalidKey);
        }
        let images = self.images.clone();
        let (set, number) = (set_code.to_string(), collector_number.to_string());
        let cached = tokio::task::spawn_blocking(move || images.get(&set, &number))
            .await
            .ok()
            .flatten();
        if let Some(bytes) = cached {
            if let Some(kind) = ImageKind::sniff(&bytes) {
                return Ok((bytes, kind));
            }
            log::warn!("Cached image of {set_code}/{collector_number} is neither JPEG nor PNG; fetching it again");
        }

        let card = self.card(set_code, collector_number).await?;
        let url = card.image_url().ok_or(LookupError::NoImage)?;
        if !(self.allow_image_url)(url) {
            log::warn!("Refusing image URL outside Scryfall's image host: {url}");
            return Err(LookupError::NoImage);
        }
        let bytes = self.download_image(url).await.map_err(|e| {
            log::warn!("Image download for {set_code}/{collector_number} failed: {e}");
            LookupError::Unavailable
        })?;
        let kind = ImageKind::sniff(&bytes).ok_or_else(|| {
            log::warn!("Image for {set_code}/{collector_number} is neither JPEG nor PNG");
            LookupError::NoImage
        })?;
        let images = self.images.clone();
        let (set, number, keep) = (
            set_code.to_string(),
            collector_number.to_string(),
            bytes.clone(),
        );
        let _ = tokio::task::spawn_blocking(move || images.insert(&set, &number, &keep)).await;
        Ok((bytes, kind))
    }

    fn cached_card(&self, set_code: &str, collector_number: &str) -> Option<ScryfallCard> {
        lock(&self.cards).get(set_code, collector_number).cloned()
    }

    /// Waits until this request may go to Scryfall's API.
    async fn wait_for_turn(&self) {
        let mut next = self.next_request.lock().await;
        tokio::time::sleep_until(*next).await;
        *next = Instant::now() + self.spacing;
    }

    /// Downloads `url` (no redirects, at most [`MAX_IMAGE_BYTES`]).
    async fn download_image(&self, url: &str) -> Result<Vec<u8>, String> {
        let mut response = self
            .image_client
            .get(url)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }
        if response
            .content_length()
            .is_some_and(|len| len > MAX_IMAGE_BYTES as u64)
        {
            return Err("image too large".to_string());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            if bytes.len() + chunk.len() > MAX_IMAGE_BYTES {
                return Err("image too large".to_string());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}

fn is_not_found(e: &CoreError) -> bool {
    match e {
        CoreError::ApiResponse { code, .. } => code == "not_found",
        CoreError::HttpStatus(status) => status.as_u16() == 404,
        _ => false,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
#[path = "cards_tests.rs"]
mod tests;
