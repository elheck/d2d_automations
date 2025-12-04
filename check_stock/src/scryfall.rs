use serde::Deserialize;
use std::collections::HashMap;

/// Scryfall card response
#[derive(Debug, Deserialize, Clone)]
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

#[derive(Debug, Deserialize, Clone, Default)]
#[allow(dead_code)]
pub struct ScryfallPrices {
    pub eur: Option<String>,
    pub eur_foil: Option<String>,
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct ImageUris {
    pub small: Option<String>,
    pub normal: Option<String>,
    pub large: Option<String>,
    pub png: Option<String>,
    pub art_crop: Option<String>,
    pub border_crop: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
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
