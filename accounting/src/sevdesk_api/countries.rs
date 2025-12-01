//! Country ID resolution with caching.

use std::collections::HashMap;

use anyhow::{Context, Result};
use log::{debug, error, info, warn};

use crate::models::{SevDeskResponse, StaticCountryResponse};

use super::SevDeskApi;

/// Cached country data with both name variants mapped to ID.
#[derive(Debug, Clone, Default)]
pub(crate) struct CountryCache {
    /// Maps lowercase country name (both local and English) to SevDesk ID.
    pub(crate) name_to_id: HashMap<String, u32>,
    /// Whether the cache has been populated.
    pub(crate) loaded: bool,
}

impl SevDeskApi {
    /// Fetches all countries from SevDesk API and populates the cache.
    pub(crate) async fn fetch_countries(&self) -> Result<()> {
        // Check if already loaded
        {
            let cache = self.country_cache.read().await;
            if cache.loaded {
                debug!(
                    "Country cache already loaded with {} entries",
                    cache.name_to_id.len()
                );
                return Ok(());
            }
        }

        info!("Fetching countries from SevDesk API");
        let url = format!("{}/StaticCountry", self.base_url);

        let response = self
            .client
            .get(&url)
            .header("Authorization", &self.api_token)
            .query(&[("limit", "500")]) // Fetch all countries
            .send()
            .await
            .context("Failed to fetch countries")?;

        debug!("Fetch countries response status: {}", response.status());

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .context("Failed to read error response")?;
            error!("Failed to fetch countries with status {status}: {error_text}");
            return Err(anyhow::anyhow!(
                "Failed to fetch countries: {} - {}",
                status,
                error_text
            ));
        }

        let response_text = response
            .text()
            .await
            .context("Failed to read response text")?;
        debug!(
            "Fetch countries response length: {} bytes",
            response_text.len()
        );

        let countries: SevDeskResponse<StaticCountryResponse> =
            serde_json::from_str(&response_text).context("Failed to parse countries response")?;

        let mut cache = self.country_cache.write().await;

        if let Some(country_list) = countries.objects {
            info!("Loaded {} countries from SevDesk API", country_list.len());

            for country in country_list {
                let country_id: u32 = country.id.parse().context("Failed to parse country ID")?;

                // Add the local name (lowercase for case-insensitive matching)
                let name_lower = country.name.to_lowercase();
                cache.name_to_id.insert(name_lower.clone(), country_id);
                debug!("Cached country: {} -> {}", country.name, country_id);

                // Add the English name if available and different
                if let Some(name_en) = &country.name_en {
                    let name_en_lower = name_en.to_lowercase();
                    if name_en_lower != name_lower {
                        cache.name_to_id.insert(name_en_lower, country_id);
                        debug!("Cached country (EN): {} -> {}", name_en, country_id);
                    }
                }

                // Add common aliases for frequently used countries
                match country_id {
                    1 => {
                        cache.name_to_id.insert("de".to_string(), country_id);
                    }
                    9 => {
                        cache.name_to_id.insert("uk".to_string(), country_id);
                        cache
                            .name_to_id
                            .insert("great britain".to_string(), country_id);
                        cache
                            .name_to_id
                            .insert("großbritannien".to_string(), country_id);
                    }
                    _ => {}
                }
            }

            cache.loaded = true;
            info!(
                "Country cache populated with {} name mappings",
                cache.name_to_id.len()
            );
        } else {
            warn!("No countries returned from SevDesk API");
        }

        Ok(())
    }

    /// Gets country ID by name, fetching from API if not cached.
    pub(crate) async fn get_country_id(&self, country_name: &str) -> Result<u32> {
        debug!("Getting country ID for: {country_name}");

        // Ensure countries are loaded
        self.fetch_countries().await?;

        // Look up in cache (case-insensitive)
        let country_name_lower = country_name.to_lowercase().trim().to_string();

        let cache = self.country_cache.read().await;
        if let Some(&country_id) = cache.name_to_id.get(&country_name_lower) {
            debug!("Country '{country_name}' found in cache with ID: {country_id}");
            return Ok(country_id);
        }

        // Try partial matching for common variations
        for (cached_name, &cached_id) in &cache.name_to_id {
            if cached_name.contains(&country_name_lower) || country_name_lower.contains(cached_name)
            {
                info!("Country '{country_name}' matched to '{cached_name}' with ID: {cached_id}");
                return Ok(cached_id);
            }
        }

        // Default to Germany if not found
        warn!("Unknown country '{country_name}', defaulting to Germany (ID: 1)");
        Ok(1)
    }
}
