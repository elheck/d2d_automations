//! Client and wire types for the `inventory_sync` REST API.
//!
//! The `inventory_sync` server collects daily Cardmarket price data into
//! SQLite and exposes it over HTTP. This module holds the shared wire types
//! (serialized by the server, deserialized by client apps) and a thin HTTP
//! client. Endpoints are designed so the server only ever runs indexed
//! `SELECT`s on behalf of clients — any aggregation (price deltas, movers)
//! happens client-side.

use crate::error::{MtgError, MtgResult};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Timeout for the lightweight health check.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(5);
/// Timeout for data requests (bulk lookups can carry thousands of rows).
const HTTP_TIMEOUT: Duration = Duration::from_secs(60);

/// Maximum product IDs per bulk request; the server rejects larger batches.
pub const MAX_BULK_IDS: usize = 10_000;
/// Maximum snapshot dates per bulk price-snapshot request.
pub const MAX_SNAPSHOT_DATES: usize = 8;

// ── Wire types ───────────────────────────────────────────────────────────────

/// Envelope for every JSON response from the inventory_sync API.
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T> ApiResponse<T> {
    /// Convenience constructor for a successful response.
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    /// Convenience constructor for a failed response.
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message.into()),
        }
    }

    /// Unwraps the envelope into the payload, mapping API failures to [`MtgError::Api`].
    pub fn into_result(self) -> MtgResult<T> {
        if !self.success {
            return Err(MtgError::Api {
                code: "inventory_sync".to_string(),
                details: self
                    .error
                    .unwrap_or_else(|| "unknown server error".to_string()),
            });
        }
        self.data.ok_or_else(|| MtgError::Api {
            code: "inventory_sync".to_string(),
            details: "response marked success but carried no data".to_string(),
        })
    }
}

/// Product metadata (for search results and price-detail responses).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductSearchResult {
    pub id_product: u64,
    pub name: String,
    pub category_name: String,
    pub id_expansion: u64,
    pub expansion_name: Option<String>,
}

/// One day of Cardmarket price-guide data for a product.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceHistoryPoint {
    pub price_date: String,
    pub avg: Option<f64>,
    pub low: Option<f64>,
    pub trend: Option<f64>,
    pub avg1: Option<f64>,
    pub avg7: Option<f64>,
    pub avg30: Option<f64>,
    pub avg_foil: Option<f64>,
    pub low_foil: Option<f64>,
    pub trend_foil: Option<f64>,
    pub avg1_foil: Option<f64>,
    pub avg7_foil: Option<f64>,
    pub avg30_foil: Option<f64>,
}

/// Latest price snapshot for a single product (most recent price_date row).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatestPrice {
    pub id_product: u64,
    pub price_date: String,
    pub avg: Option<f64>,
    pub low: Option<f64>,
    pub trend: Option<f64>,
    pub avg1: Option<f64>,
    pub avg7: Option<f64>,
    pub avg30: Option<f64>,
    pub avg_foil: Option<f64>,
    pub low_foil: Option<f64>,
    pub trend_foil: Option<f64>,
    pub avg1_foil: Option<f64>,
    pub avg7_foil: Option<f64>,
    pub avg30_foil: Option<f64>,
}

/// The price row in effect on a requested date: the most recent row with
/// `price_date <= requested_date`. Returned by `POST /api/price-snapshots`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceSnapshot {
    pub id_product: u64,
    /// The date the client asked about.
    pub requested_date: String,
    /// The actual date of the returned row (on or before `requested_date`).
    pub price_date: String,
    pub avg: Option<f64>,
    pub low: Option<f64>,
    pub trend: Option<f64>,
    pub avg1: Option<f64>,
    pub avg7: Option<f64>,
    pub avg30: Option<f64>,
    pub avg_foil: Option<f64>,
    pub low_foil: Option<f64>,
    pub trend_foil: Option<f64>,
    pub avg1_foil: Option<f64>,
    pub avg7_foil: Option<f64>,
    pub avg30_foil: Option<f64>,
}

/// Technical indicators computed by the server for a single product's history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicalIndicators {
    pub ema_7: Vec<Option<f64>>,
    pub ema_30: Vec<Option<f64>>,
    pub sma_20: Vec<Option<f64>>,
    pub rsi: Vec<Option<f64>>,
    pub macd: Vec<Option<f64>>,
    pub macd_signal: Vec<Option<f64>>,
    pub macd_histogram: Vec<Option<f64>>,
    pub bb_upper: Vec<Option<f64>>,
    pub bb_middle: Vec<Option<f64>>,
    pub bb_lower: Vec<Option<f64>>,
    /// Rate of Change over 7 days: (price - price_7ago) / price_7ago * 100
    pub roc_7: Vec<Option<f64>>,
    /// Rate of Change over 30 days: (price - price_30ago) / price_30ago * 100
    pub roc_30: Vec<Option<f64>>,
    /// Bollinger %B: where price sits within the bands (0.0 = lower band, 1.0 = upper band)
    pub bb_percent_b: Vec<Option<f64>>,
    /// Bollinger Band Width: (upper - lower) / middle — low = stable market, high = volatile
    pub bb_width: Vec<Option<f64>>,
}

/// Cardmarket-native pricing signals derived from Cardmarket's own rolling averages.
///
/// These don't need a long price history — they use avg1/avg7/avg30 already
/// computed by Cardmarket, making them available from day one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardmarketSignals {
    /// avg1 - avg7: positive = recent price spike, negative = recent drop
    pub momentum_1_7: Vec<Option<f64>>,
    /// avg7 - avg30: positive = week trending up vs month, negative = cooling off
    pub momentum_7_30: Vec<Option<f64>>,
    /// low / trend: how far below trend the cheapest listing is (< 0.8 = heavy undercutting)
    pub floor_ratio: Vec<Option<f64>>,
}

/// Full price detail for one product: `GET /api/prices/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceData {
    pub product: ProductSearchResult,
    pub history: Vec<PriceHistoryPoint>,
    pub indicators: TechnicalIndicators,
    pub cardmarket_signals: CardmarketSignals,
}

/// Request body for `POST /api/latest-prices`.
#[derive(Debug, Serialize, Deserialize)]
pub struct BulkPriceRequest {
    pub ids: Vec<u64>,
}

/// Request body for `POST /api/price-snapshots`.
#[derive(Debug, Serialize, Deserialize)]
pub struct PriceSnapshotRequest {
    pub ids: Vec<u64>,
    /// ISO dates (`YYYY-MM-DD`); one snapshot row is returned per (id, date)
    /// pair that has data on or before the date.
    pub dates: Vec<String>,
}

// ── Price field selection ────────────────────────────────────────────────────

/// Which of the standard Cardmarket price-guide columns to read.
///
/// Serialized by variant name — these names are part of the saved node-graph
/// format in check_stock, so renaming a variant breaks old save files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PriceField {
    Trend,
    Avg,
    Low,
    Avg1,
    Avg7,
    Avg30,
}

impl PriceField {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trend => "Trend",
            Self::Avg => "Average",
            Self::Low => "Low",
            Self::Avg1 => "Avg 1-day",
            Self::Avg7 => "Avg 7-day",
            Self::Avg30 => "Avg 30-day",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Trend,
            Self::Avg,
            Self::Low,
            Self::Avg1,
            Self::Avg7,
            Self::Avg30,
        ]
    }
}

/// Rows carrying the standard 12 Cardmarket price columns
/// (six fields × non-foil/foil).
pub trait PriceFields {
    /// Reads the requested field, choosing the foil column when `is_foil`.
    fn price_for(&self, field: PriceField, is_foil: bool) -> Option<f64>;
}

macro_rules! impl_price_fields {
    ($($t:ty),+ $(,)?) => { $(
        impl PriceFields for $t {
            fn price_for(&self, field: PriceField, is_foil: bool) -> Option<f64> {
                use PriceField::*;
                if is_foil {
                    match field {
                        Trend => self.trend_foil,
                        Avg => self.avg_foil,
                        Low => self.low_foil,
                        Avg1 => self.avg1_foil,
                        Avg7 => self.avg7_foil,
                        Avg30 => self.avg30_foil,
                    }
                } else {
                    match field {
                        Trend => self.trend,
                        Avg => self.avg,
                        Low => self.low,
                        Avg1 => self.avg1,
                        Avg7 => self.avg7,
                        Avg30 => self.avg30,
                    }
                }
            }
        }
    )+ };
}

impl_price_fields!(
    LatestPrice,
    PriceSnapshot,
    PriceHistoryPoint,
    crate::cardmarket::PriceGuideEntry,
);

// ── Client ───────────────────────────────────────────────────────────────────

/// Thin HTTP client for an inventory_sync server.
///
/// Async methods are primary; blocking variants (for GUI background threads)
/// are behind the `blocking` feature.
#[derive(Debug, Clone)]
pub struct InventorySyncClient {
    base_url: String,
}

impl InventorySyncClient {
    /// Creates a client for the given base URL (trailing slashes are trimmed).
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// The normalized base URL this client talks to.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn history_query(days: Option<u32>) -> String {
        match days {
            Some(d) => format!("?days={d}"),
            None => String::new(),
        }
    }

    // ── Async API ────────────────────────────────────────────────────────────

    /// `GET /api/health` — checks the server is reachable and healthy.
    pub async fn health(&self) -> MtgResult<()> {
        let response = reqwest::Client::builder()
            .timeout(HEALTH_TIMEOUT)
            .build()?
            .get(self.url("/api/health"))
            .header("User-Agent", crate::USER_AGENT)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(MtgError::HttpStatus(response.status()));
        }
        Ok(())
    }

    /// `POST /api/latest-prices` — latest price row per product.
    ///
    /// Requests are chunked to [`MAX_BULK_IDS`] internally; callers may pass
    /// any number of IDs. Products without price history are omitted.
    pub async fn latest_prices(&self, ids: &[u64]) -> MtgResult<Vec<LatestPrice>> {
        let client = reqwest::Client::builder().timeout(HTTP_TIMEOUT).build()?;
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(MAX_BULK_IDS) {
            let response = client
                .post(self.url("/api/latest-prices"))
                .header("User-Agent", crate::USER_AGENT)
                .json(&BulkPriceRequest {
                    ids: chunk.to_vec(),
                })
                .send()
                .await?;
            if !response.status().is_success() {
                return Err(MtgError::HttpStatus(response.status()));
            }
            let body: ApiResponse<Vec<LatestPrice>> = response.json().await?;
            out.extend(body.into_result()?);
        }
        Ok(out)
    }

    /// `GET /api/prices/{id}` — full history + indicators for one product.
    ///
    /// `days` limits the history window; `None` returns everything.
    pub async fn price_history(&self, id_product: u64, days: Option<u32>) -> MtgResult<PriceData> {
        let response = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()?
            .get(self.url(&format!(
                "/api/prices/{id_product}{}",
                Self::history_query(days)
            )))
            .header("User-Agent", crate::USER_AGENT)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(MtgError::HttpStatus(response.status()));
        }
        let body: ApiResponse<PriceData> = response.json().await?;
        body.into_result()
    }

    /// `POST /api/price-snapshots` — the price row in effect on each requested
    /// date, per product. Chunked to [`MAX_BULK_IDS`] internally.
    ///
    /// Missing (id, date) combinations (no data on or before the date) are
    /// simply absent from the result.
    pub async fn price_snapshots(
        &self,
        ids: &[u64],
        dates: &[String],
    ) -> MtgResult<Vec<PriceSnapshot>> {
        let client = reqwest::Client::builder().timeout(HTTP_TIMEOUT).build()?;
        let mut out = Vec::new();
        for chunk in ids.chunks(MAX_BULK_IDS) {
            let response = client
                .post(self.url("/api/price-snapshots"))
                .header("User-Agent", crate::USER_AGENT)
                .json(&PriceSnapshotRequest {
                    ids: chunk.to_vec(),
                    dates: dates.to_vec(),
                })
                .send()
                .await?;
            if !response.status().is_success() {
                return Err(MtgError::HttpStatus(response.status()));
            }
            let body: ApiResponse<Vec<PriceSnapshot>> = response.json().await?;
            out.extend(body.into_result()?);
        }
        Ok(out)
    }

    // ── Blocking API (GUI background threads) ────────────────────────────────

    /// Blocking variant of [`Self::health`].
    #[cfg(feature = "blocking")]
    pub fn health_blocking(&self) -> MtgResult<()> {
        let response = reqwest::blocking::Client::builder()
            .timeout(HEALTH_TIMEOUT)
            .build()?
            .get(self.url("/api/health"))
            .header("User-Agent", crate::USER_AGENT)
            .send()?;
        if !response.status().is_success() {
            return Err(MtgError::HttpStatus(response.status()));
        }
        Ok(())
    }

    /// Blocking variant of [`Self::latest_prices`].
    #[cfg(feature = "blocking")]
    pub fn latest_prices_blocking(&self, ids: &[u64]) -> MtgResult<Vec<LatestPrice>> {
        let client = reqwest::blocking::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()?;
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(MAX_BULK_IDS) {
            let response = client
                .post(self.url("/api/latest-prices"))
                .header("User-Agent", crate::USER_AGENT)
                .json(&BulkPriceRequest {
                    ids: chunk.to_vec(),
                })
                .send()?;
            if !response.status().is_success() {
                return Err(MtgError::HttpStatus(response.status()));
            }
            let body: ApiResponse<Vec<LatestPrice>> = response.json()?;
            out.extend(body.into_result()?);
        }
        Ok(out)
    }

    /// Blocking variant of [`Self::price_history`].
    #[cfg(feature = "blocking")]
    pub fn price_history_blocking(
        &self,
        id_product: u64,
        days: Option<u32>,
    ) -> MtgResult<PriceData> {
        let response = reqwest::blocking::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()?
            .get(self.url(&format!(
                "/api/prices/{id_product}{}",
                Self::history_query(days)
            )))
            .header("User-Agent", crate::USER_AGENT)
            .send()?;
        if !response.status().is_success() {
            return Err(MtgError::HttpStatus(response.status()));
        }
        let body: ApiResponse<PriceData> = response.json()?;
        body.into_result()
    }

    /// Blocking variant of [`Self::price_snapshots`].
    #[cfg(feature = "blocking")]
    pub fn price_snapshots_blocking(
        &self,
        ids: &[u64],
        dates: &[String],
    ) -> MtgResult<Vec<PriceSnapshot>> {
        let client = reqwest::blocking::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()?;
        let mut out = Vec::new();
        for chunk in ids.chunks(MAX_BULK_IDS) {
            let response = client
                .post(self.url("/api/price-snapshots"))
                .header("User-Agent", crate::USER_AGENT)
                .json(&PriceSnapshotRequest {
                    ids: chunk.to_vec(),
                    dates: dates.to_vec(),
                })
                .send()?;
            if !response.status().is_success() {
                return Err(MtgError::HttpStatus(response.status()));
            }
            let body: ApiResponse<Vec<PriceSnapshot>> = response.json()?;
            out.extend(body.into_result()?);
        }
        Ok(out)
    }
}

#[cfg(test)]
#[path = "inventory_sync_tests.rs"]
mod tests;
