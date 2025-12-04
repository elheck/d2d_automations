use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Scryfall card response
#[derive(Debug, Deserialize, Serialize, Clone)]
#[allow(dead_code)]
pub struct ScryfallCard {
    pub id: String,
    pub name: String,
    pub set: String,
    pub set_name: String,
    pub collector_number: String,
    pub rarity: String,
    #[serde(default)]
    pub prices: ScryfallPrices,
    #[serde(default)]
    pub image_uris: Option<ImageUris>,
    /// For double-faced cards, images are in card_faces
    #[serde(default)]
    pub card_faces: Option<Vec<CardFace>>,
    /// Cardmarket product ID for price matching
    #[serde(default)]
    pub cardmarket_id: Option<u64>,
    #[serde(default)]
    pub mana_cost: Option<String>,
    #[serde(default)]
    pub type_line: Option<String>,
    #[serde(default)]
    pub oracle_text: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
#[allow(dead_code)]
pub struct ScryfallPrices {
    pub eur: Option<String>,
    pub eur_foil: Option<String>,
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[allow(dead_code)]
pub struct ImageUris {
    pub small: Option<String>,
    pub normal: Option<String>,
    pub large: Option<String>,
    pub png: Option<String>,
    pub art_crop: Option<String>,
    pub border_crop: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[allow(dead_code)]
pub struct CardFace {
    pub name: String,
    #[serde(default)]
    pub image_uris: Option<ImageUris>,
    #[serde(default)]
    pub mana_cost: Option<String>,
    #[serde(default)]
    pub type_line: Option<String>,
    #[serde(default)]
    pub oracle_text: Option<String>,
}

impl ScryfallCard {
    /// Get the primary image URL (normal size)
    pub fn image_url(&self) -> Option<&str> {
        // Try direct image_uris first
        if let Some(ref uris) = self.image_uris {
            return uris.normal.as_deref();
        }
        // For double-faced cards, get front face image
        if let Some(ref faces) = self.card_faces {
            if let Some(face) = faces.first() {
                if let Some(ref uris) = face.image_uris {
                    return uris.normal.as_deref();
                }
            }
        }
        None
    }
}

/// Scryfall API error response
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ScryfallError {
    pub status: u16,
    pub code: String,
    pub details: String,
}

/// Fetch a card from Scryfall by set code and collector number
pub fn fetch_card(set_code: &str, collector_number: &str) -> Result<ScryfallCard, String> {
    let url = format!(
        "https://api.scryfall.com/cards/{}/{}",
        set_code.to_lowercase(),
        collector_number
    );

    log::info!("Fetching card from Scryfall: {}", url);

    let response = reqwest::blocking::Client::new()
        .get(&url)
        .header("User-Agent", "D2D-Automations/1.0")
        .send()
        .map_err(|e| format!("Request failed: {}", e))?;

    if response.status().is_success() {
        response
            .json::<ScryfallCard>()
            .map_err(|e| format!("Failed to parse response: {}", e))
    } else {
        let error: ScryfallError = response
            .json()
            .map_err(|e| format!("Failed to parse error: {}", e))?;
        Err(format!("{}: {}", error.code, error.details))
    }
}

/// Fetch card image bytes
pub fn fetch_image(url: &str) -> Result<Vec<u8>, String> {
    log::debug!("Fetching image: {}", url);

    let response = reqwest::blocking::Client::new()
        .get(url)
        .header("User-Agent", "D2D-Automations/1.0")
        .send()
        .map_err(|e| format!("Image request failed: {}", e))?;

    if response.status().is_success() {
        response
            .bytes()
            .map(|b| b.to_vec())
            .map_err(|e| format!("Failed to read image: {}", e))
    } else {
        Err(format!("Image fetch failed: {}", response.status()))
    }
}

/// Cardmarket price guide entry
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct PriceGuideEntry {
    pub id_product: u64,
    pub id_category: u64,
    pub avg: Option<f64>,
    pub low: Option<f64>,
    pub trend: Option<f64>,
    pub avg1: Option<f64>,
    pub avg7: Option<f64>,
    pub avg30: Option<f64>,
    #[serde(rename = "avg-foil")]
    pub avg_foil: Option<f64>,
    #[serde(rename = "low-foil")]
    pub low_foil: Option<f64>,
    #[serde(rename = "trend-foil")]
    pub trend_foil: Option<f64>,
    #[serde(rename = "avg1-foil")]
    pub avg1_foil: Option<f64>,
    #[serde(rename = "avg7-foil")]
    pub avg7_foil: Option<f64>,
    #[serde(rename = "avg30-foil")]
    pub avg30_foil: Option<f64>,
}

/// Full price guide file structure
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct PriceGuideFile {
    pub version: u32,
    pub created_at: String,
    pub price_guides: Vec<PriceGuideEntry>,
}

/// Price guide lookup by product ID
pub struct PriceGuide {
    entries: HashMap<u64, PriceGuideEntry>,
}

#[allow(dead_code)]
impl PriceGuide {
    /// Load price guide from JSON file
    pub fn load(path: &str) -> Result<Self, String> {
        log::info!("Loading price guide from: {}", path);

        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;

        let file: PriceGuideFile =
            serde_json::from_str(&content).map_err(|e| format!("Failed to parse JSON: {}", e))?;

        let entries: HashMap<u64, PriceGuideEntry> = file
            .price_guides
            .into_iter()
            .map(|e| (e.id_product, e))
            .collect();

        log::info!("Loaded {} price entries", entries.len());

        Ok(Self { entries })
    }

    /// Fetch price guide from Cardmarket's CDN
    /// URL: https://downloads.s3.cardmarket.com/productCatalog/priceGuide/price_guide_1.json
    pub fn fetch() -> Result<Self, String> {
        const PRICE_GUIDE_URL: &str =
            "https://downloads.s3.cardmarket.com/productCatalog/priceGuide/price_guide_1.json";

        log::info!("Fetching price guide from Cardmarket...");

        let response = reqwest::blocking::Client::new()
            .get(PRICE_GUIDE_URL)
            .header("User-Agent", "D2D-Automations/1.0")
            .send()
            .map_err(|e| format!("Request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!(
                "Failed to fetch price guide: {}",
                response.status()
            ));
        }

        let file: PriceGuideFile = response
            .json()
            .map_err(|e| format!("Failed to parse price guide JSON: {}", e))?;

        let entries: HashMap<u64, PriceGuideEntry> = file
            .price_guides
            .into_iter()
            .map(|e| (e.id_product, e))
            .collect();

        log::info!(
            "Fetched {} price entries (created: {})",
            entries.len(),
            file.created_at
        );

        Ok(Self { entries })
    }

    /// Look up price for a cardmarket product ID
    pub fn get(&self, cardmarket_id: u64) -> Option<&PriceGuideEntry> {
        self.entries.get(&cardmarket_id)
    }

    /// Get the number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Persistent cache for Scryfall card lookups
/// Stores cards in a JSON file to avoid redundant API calls
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CardCache {
    /// Map of "set/collector_number" to card data
    cards: HashMap<String, ScryfallCard>,
}

impl CardCache {
    /// Get the default cache file path
    fn cache_path() -> std::path::PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("d2d_automations")
            .join("scryfall_cache.json")
    }

    /// Load cache from disk, or create empty if doesn't exist
    pub fn load() -> Self {
        let path = Self::cache_path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(cache) => {
                        log::info!("Loaded card cache with {} entries", Self::count(&cache));
                        return cache;
                    }
                    Err(e) => {
                        log::warn!("Failed to parse cache file, starting fresh: {}", e);
                    }
                },
                Err(e) => {
                    log::warn!("Failed to read cache file, starting fresh: {}", e);
                }
            }
        }
        log::info!("Starting with empty card cache");
        Self::default()
    }

    fn count(cache: &Self) -> usize {
        cache.cards.len()
    }

    /// Save cache to disk
    pub fn save(&self) -> Result<(), String> {
        let path = Self::cache_path();

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create cache directory: {}", e))?;
        }

        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize cache: {}", e))?;

        std::fs::write(&path, content).map_err(|e| format!("Failed to write cache: {}", e))?;

        log::debug!("Saved card cache with {} entries", self.cards.len());
        Ok(())
    }

    /// Generate cache key from set code and collector number
    fn key(set_code: &str, collector_number: &str) -> String {
        format!("{}/{}", set_code.to_lowercase(), collector_number)
    }

    /// Get a card from cache
    pub fn get(&self, set_code: &str, collector_number: &str) -> Option<&ScryfallCard> {
        self.cards.get(&Self::key(set_code, collector_number))
    }

    /// Insert a card into cache
    pub fn insert(&mut self, set_code: &str, collector_number: &str, card: ScryfallCard) {
        self.cards
            .insert(Self::key(set_code, collector_number), card);
    }

    /// Get card count
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.cards.len()
    }

    /// Check if empty
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }
}

/// Fetch a card, checking cache first
pub fn fetch_card_cached(
    cache: &mut CardCache,
    set_code: &str,
    collector_number: &str,
) -> Result<ScryfallCard, String> {
    // Check cache first
    if let Some(card) = cache.get(set_code, collector_number) {
        log::info!(
            "Cache hit for {}/{}",
            set_code,
            collector_number
        );
        return Ok(card.clone());
    }

    // Fetch from API
    log::info!(
        "Cache miss for {}/{}, fetching from Scryfall",
        set_code,
        collector_number
    );
    let card = fetch_card(set_code, collector_number)?;

    // Store in cache and save
    cache.insert(set_code, collector_number, card.clone());
    if let Err(e) = cache.save() {
        log::warn!("Failed to save cache: {}", e);
    }

    Ok(card)
}
