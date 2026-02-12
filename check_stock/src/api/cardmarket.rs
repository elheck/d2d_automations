use crate::error::{ApiError, ApiResult};
use serde::Deserialize;
use std::collections::HashMap;

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
#[derive(Debug)]
pub struct PriceGuide {
    entries: HashMap<u64, PriceGuideEntry>,
}

#[allow(dead_code)]
impl PriceGuide {
    /// Load price guide from JSON file
    pub fn load(path: &str) -> ApiResult<Self> {
        log::info!("Loading price guide from: {}", path);

        let content = std::fs::read_to_string(path)?;
        let file: PriceGuideFile = serde_json::from_str(&content)?;

        let entries: HashMap<u64, PriceGuideEntry> = file
            .price_guides
            .into_iter()
            .map(|e| (e.id_product, e))
            .collect();

        log::info!("Loaded {} price entries", entries.len());

        Ok(Self { entries })
    }

    /// Fetch price guide from Cardmarket's CDN
    pub fn fetch() -> ApiResult<Self> {
        Self::fetch_from(
            "https://downloads.s3.cardmarket.com/productCatalog/priceGuide/price_guide_1.json",
        )
    }

    /// Fetches price guide from the given URL (for testing with mock servers).
    pub(crate) fn fetch_from(url: &str) -> ApiResult<Self> {
        log::info!("Fetching price guide from: {}", url);

        let response = reqwest::blocking::Client::new()
            .get(url)
            .header("User-Agent", "D2D-Automations/1.0")
            .send()?;

        if !response.status().is_success() {
            return Err(ApiError::HttpStatus(response.status()));
        }

        let file: PriceGuideFile = response.json()?;

        let entries: HashMap<u64, PriceGuideEntry> = file
            .price_guides
            .into_iter()
            .map(|e| (e.id_product, e))
            .collect();

        log::info!("Fetched {} price entries", entries.len());

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
