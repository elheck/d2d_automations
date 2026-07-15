use crate::error::{MtgError, MtgResult};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Cardmarket price guide entry for a single product.
///
/// Matches the JSON schema from Cardmarket's CDN price guide files.
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
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

/// Full price guide file structure from Cardmarket's CDN.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceGuideFile {
    pub version: u32,
    pub created_at: String,
    pub price_guides: Vec<PriceGuideEntry>,
}

/// Price guide lookup by product ID.
#[derive(Debug)]
pub struct PriceGuide {
    entries: HashMap<u64, PriceGuideEntry>,
    created_at: String,
}

impl PriceGuide {
    fn from_file_struct(file: PriceGuideFile) -> Self {
        let created_at = file.created_at;
        let entries = file
            .price_guides
            .into_iter()
            .map(|e| (e.id_product, e))
            .collect();
        Self {
            entries,
            created_at,
        }
    }

    /// Create a PriceGuide directly from entries (for tests and simulations).
    pub fn from_entries(entries: Vec<PriceGuideEntry>, created_at: &str) -> Self {
        let entries = entries.into_iter().map(|e| (e.id_product, e)).collect();
        Self {
            entries,
            created_at: created_at.to_string(),
        }
    }

    /// Load price guide from a JSON file on disk.
    pub fn load(path: &str) -> MtgResult<Self> {
        log::info!("Loading price guide from: {}", path);

        let content = std::fs::read_to_string(path)?;
        let file: PriceGuideFile = serde_json::from_str(&content)?;
        let guide = Self::from_file_struct(file);

        log::info!("Loaded {} price entries", guide.len());
        Ok(guide)
    }

    /// Fetch price guide from Cardmarket's CDN (async).
    pub async fn fetch() -> MtgResult<Self> {
        Self::fetch_from(crate::PRICE_GUIDE_URL).await
    }

    /// Fetches price guide from the given URL (async, for testing with mock servers).
    pub async fn fetch_from(url: &str) -> MtgResult<Self> {
        log::info!("Fetching price guide from: {}", url);

        let response = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()?
            .get(url)
            .header("User-Agent", crate::USER_AGENT)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(MtgError::HttpStatus(response.status()));
        }

        let file: PriceGuideFile = response.json().await?;
        let guide = Self::from_file_struct(file);

        log::info!(
            "Fetched {} price entries (created: {})",
            guide.len(),
            guide.created_at()
        );
        Ok(guide)
    }

    /// Fetch price guide from Cardmarket's CDN (blocking).
    #[cfg(feature = "blocking")]
    pub fn fetch_blocking() -> MtgResult<Self> {
        Self::fetch_from_blocking(crate::PRICE_GUIDE_URL)
    }

    /// Fetches price guide from the given URL (blocking, for testing with mock servers).
    #[cfg(feature = "blocking")]
    pub fn fetch_from_blocking(url: &str) -> MtgResult<Self> {
        log::info!("Fetching price guide from: {}", url);

        let response = reqwest::blocking::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()?
            .get(url)
            .header("User-Agent", crate::USER_AGENT)
            .send()?;

        if !response.status().is_success() {
            return Err(MtgError::HttpStatus(response.status()));
        }

        let file: PriceGuideFile = response.json()?;
        let guide = Self::from_file_struct(file);

        log::info!(
            "Fetched {} price entries (created: {})",
            guide.len(),
            guide.created_at()
        );
        Ok(guide)
    }

    /// Look up price for a cardmarket product ID.
    pub fn get(&self, cardmarket_id: u64) -> Option<&PriceGuideEntry> {
        self.entries.get(&cardmarket_id)
    }

    /// Get the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get the creation timestamp from Cardmarket.
    pub fn created_at(&self) -> &str {
        &self.created_at
    }

    /// Iterate over all price entries.
    pub fn iter(&self) -> impl Iterator<Item = &PriceGuideEntry> {
        self.entries.values()
    }
}

#[cfg(test)]
#[path = "cardmarket_tests.rs"]
mod tests;
