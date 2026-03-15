use crate::error::{ApiError, ApiResult};
use std::collections::HashMap;
use std::time::Duration;

const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

pub use mtg_common::cardmarket::{PriceGuideEntry, PriceGuideFile};

/// Price guide lookup by product ID
#[derive(Debug)]
pub struct PriceGuide {
    entries: HashMap<u64, PriceGuideEntry>,
}

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
        Self::fetch_from(mtg_common::PRICE_GUIDE_URL)
    }

    /// Fetches price guide from the given URL (for testing with mock servers).
    pub(crate) fn fetch_from(url: &str) -> ApiResult<Self> {
        log::info!("Fetching price guide from: {}", url);

        let response = reqwest::blocking::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()?
            .get(url)
            .header("User-Agent", mtg_common::USER_AGENT)
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

    /// Fetch price guide from Cardmarket's CDN (async)
    pub async fn fetch_async() -> ApiResult<Self> {
        Self::fetch_from_async(mtg_common::PRICE_GUIDE_URL).await
    }

    /// Fetches price guide from the given URL (async, for testing with mock servers).
    pub(crate) async fn fetch_from_async(url: &str) -> ApiResult<Self> {
        log::info!("Fetching price guide from: {}", url);

        let response = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()?
            .get(url)
            .header("User-Agent", mtg_common::USER_AGENT)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(ApiError::HttpStatus(response.status()));
        }

        let file: PriceGuideFile = response.json().await?;

        let entries: HashMap<u64, PriceGuideEntry> = file
            .price_guides
            .into_iter()
            .map(|e| (e.id_product, e))
            .collect();

        log::info!("Fetched {} price entries", entries.len());

        Ok(Self { entries })
    }
}
