//! Cardmarket price guide fetching and parsing

use crate::error::{Error, Result};
use serde::Deserialize;
use std::collections::HashMap;

/// Cardmarket price guide URL (MTG = category 1)
const PRICE_GUIDE_URL: &str =
    "https://downloads.s3.cardmarket.com/productCatalog/priceGuide/price_guide_1.json";

/// Cardmarket price guide entry for a single product
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

/// Full price guide file structure from Cardmarket
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct PriceGuideFile {
    pub version: u32,
    pub created_at: String,
    pub price_guides: Vec<PriceGuideEntry>,
}

/// Price guide lookup by product ID
pub struct PriceGuide {
    entries: HashMap<u64, PriceGuideEntry>,
    created_at: String,
}

impl PriceGuide {
    /// Fetch price guide from Cardmarket's CDN (async)
    pub async fn fetch() -> Result<Self> {
        log::info!("Fetching price guide from Cardmarket...");

        let client = reqwest::Client::new();
        let response = client
            .get(PRICE_GUIDE_URL)
            .header("User-Agent", "inventory_sync/1.0")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(Error::HttpStatus(response.status()));
        }

        let file: PriceGuideFile = response.json().await?;
        let created_at = file.created_at.clone();

        let entries: HashMap<u64, PriceGuideEntry> = file
            .price_guides
            .into_iter()
            .map(|e| (e.id_product, e))
            .collect();

        log::info!(
            "Fetched {} price entries (created: {})",
            entries.len(),
            created_at
        );

        Ok(Self {
            entries,
            created_at,
        })
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

    /// Get the creation timestamp from Cardmarket
    pub fn created_at(&self) -> &str {
        &self.created_at
    }

    /// Iterate over all price entries
    pub fn iter(&self) -> impl Iterator<Item = &PriceGuideEntry> {
        self.entries.values()
    }

    /// Create a PriceGuide from entries (for testing)
    #[cfg(test)]
    pub fn from_entries(entries: Vec<PriceGuideEntry>, created_at: &str) -> Self {
        let entries = entries.into_iter().map(|e| (e.id_product, e)).collect();
        Self {
            entries,
            created_at: created_at.to_string(),
        }
    }
}

#[cfg(test)]
pub use tests::make_test_price_entry;

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a test price entry with default values
    pub fn make_test_price_entry(id_product: u64, trend: Option<f64>) -> PriceGuideEntry {
        PriceGuideEntry {
            id_product,
            id_category: 1,
            avg: trend,
            low: trend.map(|t| t * 0.8),
            trend,
            avg1: None,
            avg7: None,
            avg30: None,
            avg_foil: None,
            low_foil: None,
            trend_foil: None,
            avg1_foil: None,
            avg7_foil: None,
            avg30_foil: None,
        }
    }

    #[test]
    fn price_guide_from_entries() {
        let entries = vec![
            make_test_price_entry(1, Some(10.0)),
            make_test_price_entry(2, Some(20.0)),
        ];
        let guide = PriceGuide::from_entries(entries, "2026-02-01T10:00:00+0100");

        assert_eq!(guide.len(), 2);
        assert_eq!(guide.created_at(), "2026-02-01T10:00:00+0100");
        assert_eq!(guide.get(1).unwrap().trend, Some(10.0));
        assert_eq!(guide.get(2).unwrap().trend, Some(20.0));
        assert!(guide.get(999).is_none());
    }

    #[test]
    fn price_guide_entry_deserializes_with_nulls() {
        let json = r#"{
            "idProduct": 12345,
            "idCategory": 1,
            "avg": 1.5,
            "low": 0.5,
            "trend": 1.2,
            "avg1": null,
            "avg7": null,
            "avg30": null,
            "avg-foil": null,
            "low-foil": null,
            "trend-foil": null,
            "avg1-foil": null,
            "avg7-foil": null,
            "avg30-foil": null
        }"#;

        let entry: PriceGuideEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.id_product, 12345);
        assert_eq!(entry.avg, Some(1.5));
        assert_eq!(entry.avg1, None);
    }
}
