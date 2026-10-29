//! Country ID resolution with caching.
//!
//! Country names are matched **exactly** (case-insensitive, trimmed) against
//! SevDesk's German and English names plus a few fixed aliases. There is no
//! partial matching and no default country: "Niger" must never become
//! "Nigeria", and an unknown country must never silently become Germany.
//! An unknown name is an error; the invoice run asks the user to pick the
//! right SevDesk country instead (see [`SevDeskApi::remember_country`]).

use std::collections::HashMap;

use anyhow::{Context, Result};
use log::{debug, error, info};

use crate::models::{SevDeskResponse, StaticCountryResponse};

use super::SevDeskApi;

/// A SevDesk country the user can choose when an order's country is unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountryOption {
    pub id: u32,
    /// English name with the German name in brackets when they differ,
    /// e.g. `Germany (Deutschland)`.
    pub display_name: String,
}

/// Cached country data with every accepted name mapped to its SevDesk ID.
#[derive(Debug, Clone, Default)]
pub(crate) struct CountryCache {
    /// Maps normalized country names (German, English, aliases) to SevDesk ID.
    pub(crate) name_to_id: HashMap<String, u32>,
    /// All countries, sorted by display name, for the country picker.
    pub(crate) options: Vec<CountryOption>,
    /// Whether the cache has been populated.
    pub(crate) loaded: bool,
}

/// Normalizes a country name for lookup: trimmed and lowercase.
pub(crate) fn normalize_country_name(name: &str) -> String {
    name.trim().to_lowercase()
}

impl CountryCache {
    /// Builds a loaded cache from SevDesk's `/StaticCountry` list.
    pub(crate) fn from_countries(countries: &[StaticCountryResponse]) -> Result<Self> {
        let mut cache = CountryCache {
            loaded: true,
            ..Default::default()
        };

        for country in countries {
            let id: u32 = country
                .id
                .parse()
                .with_context(|| format!("Failed to parse country ID '{}'", country.id))?;

            cache.remember(&country.name, id);
            if let Some(name_en) = &country.name_en {
                cache.remember(name_en, id);
            }

            // Fixed aliases for frequently used countries.
            match id {
                1 => cache.remember("de", id),
                9 => {
                    for alias in ["uk", "great britain", "großbritannien"] {
                        cache.remember(alias, id);
                    }
                }
                _ => {}
            }

            let display_name = match &country.name_en {
                Some(en) if !en.trim().is_empty() && en.trim() != country.name.trim() => {
                    format!("{} ({})", en.trim(), country.name.trim())
                }
                _ => country.name.trim().to_string(),
            };
            cache.options.push(CountryOption { id, display_name });
        }

        cache
            .options
            .sort_by_key(|option| option.display_name.to_lowercase());
        Ok(cache)
    }

    /// Exact, case-insensitive lookup. Returns `None` for unknown names.
    pub(crate) fn lookup(&self, name: &str) -> Option<u32> {
        self.name_to_id.get(&normalize_country_name(name)).copied()
    }

    /// Accepts `name` as a name for country `id` from now on.
    pub(crate) fn remember(&mut self, name: &str, id: u32) {
        let key = normalize_country_name(name);
        if !key.is_empty() {
            self.name_to_id.insert(key, id);
        }
    }
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
        let country_list = countries.objects.unwrap_or_default();
        info!("Loaded {} countries from SevDesk API", country_list.len());

        // Build the whole cache first so a parse error never leaves it half-filled.
        let loaded = CountryCache::from_countries(&country_list)?;
        info!(
            "Country cache populated with {} name mappings",
            loaded.name_to_id.len()
        );

        let mut cache = self.country_cache.write().await;
        // Keep names the user already mapped in this run.
        let remembered = std::mem::take(&mut cache.name_to_id);
        *cache = loaded;
        for (name, id) in remembered {
            cache.name_to_id.entry(name).or_insert(id);
        }

        Ok(())
    }

    /// Looks up a country by exact name, fetching the list if not cached.
    ///
    /// Returns `Ok(None)` when SevDesk has no country with this name.
    pub(crate) async fn lookup_country_id(&self, country_name: &str) -> Result<Option<u32>> {
        self.fetch_countries().await?;
        let id = self.country_cache.read().await.lookup(country_name);
        match id {
            Some(id) => debug!("Country '{country_name}' resolved to ID {id}"),
            None => debug!("Country '{country_name}' is not a known SevDesk country"),
        }
        Ok(id)
    }

    /// Gets the SevDesk ID of a country by exact name.
    ///
    /// Fails for unknown names instead of guessing.
    pub(crate) async fn get_country_id(&self, country_name: &str) -> Result<u32> {
        self.lookup_country_id(country_name)
            .await?
            .with_context(|| {
                format!("Unknown country '{country_name}': SevDesk has no country with this name")
            })
    }

    /// All SevDesk countries, sorted by display name, for the country picker.
    pub(crate) async fn country_options(&self) -> Result<Vec<CountryOption>> {
        self.fetch_countries().await?;
        Ok(self.country_cache.read().await.options.clone())
    }

    /// Accepts `country_name` as SevDesk country `id` for the rest of this run.
    ///
    /// Used when the user resolves an unknown country by picking it from the list.
    pub(crate) async fn remember_country(&self, country_name: &str, id: u32) {
        info!("Using SevDesk country {id} for '{country_name}' in this run");
        self.country_cache.write().await.remember(country_name, id);
    }
}

#[cfg(test)]
#[path = "countries_tests.rs"]
mod tests;
