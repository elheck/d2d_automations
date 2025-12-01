//! HTTP client functionality and connection testing.

use anyhow::{Context, Result};
use log::{debug, error, info, warn};

use super::SevDeskApi;

impl SevDeskApi {
    /// Tests the connection to the SevDesk API.
    ///
    /// Returns `Ok(true)` if the connection is successful, `Ok(false)` otherwise.
    pub async fn test_connection(&self) -> Result<bool> {
        info!("Testing SevDesk API connection");
        let test_url = format!("{}/Tools/bookkeepingSystemVersion", self.base_url);
        debug!("Testing connection at: {test_url}");

        let response = self
            .client
            .get(&test_url)
            .header("Authorization", &self.api_token)
            .send()
            .await
            .context("Failed to test API connection")?;

        let success = response.status().is_success();
        debug!(
            "API connection test result: {} (status: {})",
            success,
            response.status()
        );

        if !success {
            let error_text = response.text().await.unwrap_or_default();
            warn!("API connection test failed: {error_text}");
            error!("Response body: {error_text}");
        } else {
            let response_text = response.text().await.unwrap_or_default();
            info!("API connection successful. Response: {response_text}");
        }

        Ok(success)
    }
}
