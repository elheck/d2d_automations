//! Cardmarket product catalog fetching and parsing

use crate::error::{Error, Result};
use serde::Deserialize;
use std::collections::HashMap;

/// Cardmarket product catalog URLs (MTG = category 1)
const SINGLES_URL: &str =
    "https://downloads.s3.cardmarket.com/productCatalog/productList/products_singles_1.json";
const NON_SINGLES_URL: &str =
    "https://downloads.s3.cardmarket.com/productCatalog/productList/products_nonsingles_1.json";

/// Cardmarket product entry
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProductEntry {
    pub id_product: u64,
    pub name: String,
    pub id_category: u64,
    pub category_name: String,
    pub id_expansion: u64,
    pub id_metacard: u64,
    pub date_added: String,
}

/// Full product catalog file structure from Cardmarket
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct ProductCatalogFile {
    pub version: u32,
    pub created_at: String,
    pub products: Vec<ProductEntry>,
}

/// Product catalog lookup by product ID
pub struct ProductCatalog {
    entries: HashMap<u64, ProductEntry>,
    singles_count: usize,
    non_singles_count: usize,
}

impl ProductCatalog {
    /// Fetch both singles and non-singles product catalogs from Cardmarket's CDN
    pub async fn fetch() -> Result<Self> {
        let client = reqwest::Client::new();

        // Fetch singles
        log::info!("Fetching singles product catalog from Cardmarket...");
        let singles = Self::fetch_catalog(&client, SINGLES_URL).await?;
        let singles_count = singles.len();
        log::info!("Fetched {} singles products", singles_count);

        // Fetch non-singles
        log::info!("Fetching non-singles product catalog from Cardmarket...");
        let non_singles = Self::fetch_catalog(&client, NON_SINGLES_URL).await?;
        let non_singles_count = non_singles.len();
        log::info!("Fetched {} non-singles products", non_singles_count);

        // Merge both catalogs
        let mut entries = singles;
        entries.extend(non_singles);

        log::info!(
            "Total products loaded: {} ({} singles + {} non-singles)",
            entries.len(),
            singles_count,
            non_singles_count
        );

        Ok(Self {
            entries,
            singles_count,
            non_singles_count,
        })
    }

    /// Fetch a single catalog file
    async fn fetch_catalog(
        client: &reqwest::Client,
        url: &str,
    ) -> Result<HashMap<u64, ProductEntry>> {
        let response = client
            .get(url)
            .header("User-Agent", "inventory_sync/1.0")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(Error::HttpStatus(response.status()));
        }

        let file: ProductCatalogFile = response.json().await?;

        let entries: HashMap<u64, ProductEntry> = file
            .products
            .into_iter()
            .map(|p| (p.id_product, p))
            .collect();

        Ok(entries)
    }

    /// Look up a product by its Cardmarket product ID
    pub fn get(&self, product_id: u64) -> Option<&ProductEntry> {
        self.entries.get(&product_id)
    }

    /// Get the total number of products
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get the number of singles
    pub fn singles_count(&self) -> usize {
        self.singles_count
    }

    /// Get the number of non-singles
    pub fn non_singles_count(&self) -> usize {
        self.non_singles_count
    }

    /// Iterate over all products
    pub fn iter(&self) -> impl Iterator<Item = &ProductEntry> {
        self.entries.values()
    }

    /// Create a ProductCatalog from entries (for testing)
    #[cfg(test)]
    pub fn from_entries(entries: Vec<ProductEntry>) -> Self {
        let count = entries.len();
        let entries = entries.into_iter().map(|p| (p.id_product, p)).collect();
        Self {
            entries,
            singles_count: count,
            non_singles_count: 0,
        }
    }
}

#[cfg(test)]
pub use tests::make_test_product;

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a test product entry with default values
    pub fn make_test_product(id_product: u64, name: &str) -> ProductEntry {
        ProductEntry {
            id_product,
            name: name.to_string(),
            id_category: 1,
            category_name: "Magic Single".to_string(),
            id_expansion: 1,
            id_metacard: id_product,
            date_added: "2007-01-01 00:00:00".to_string(),
        }
    }

    #[test]
    fn product_catalog_from_entries() {
        let entries = vec![
            make_test_product(1, "Black Lotus"),
            make_test_product(2, "Mox Pearl"),
        ];
        let catalog = ProductCatalog::from_entries(entries);

        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog.singles_count(), 2);
        assert_eq!(catalog.non_singles_count(), 0);
        assert_eq!(catalog.get(1).unwrap().name, "Black Lotus");
        assert_eq!(catalog.get(2).unwrap().name, "Mox Pearl");
        assert!(catalog.get(999).is_none());
    }

    #[test]
    fn product_entry_deserializes() {
        let json = r#"{
            "idProduct": 12345,
            "name": "Black Lotus",
            "idCategory": 1,
            "categoryName": "Magic Single",
            "idExpansion": 1,
            "idMetacard": 567,
            "dateAdded": "2007-01-01 00:00:00"
        }"#;

        let entry: ProductEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.id_product, 12345);
        assert_eq!(entry.name, "Black Lotus");
        assert_eq!(entry.category_name, "Magic Single");
    }
}
