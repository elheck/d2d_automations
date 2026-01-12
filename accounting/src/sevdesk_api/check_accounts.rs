//! Check account (Verrechnungskonto) management functionality.

use anyhow::{Context, Result};
use log::{debug, info, warn};

use crate::models::{CheckAccountResponse, SevDeskResponse};

use super::SevDeskApi;

impl SevDeskApi {
    /// Fetches all check accounts (Verrechnungskonten) from SevDesk.
    ///
    /// Returns only active accounts by default.
    pub async fn fetch_check_accounts(&self) -> Result<Vec<CheckAccountResponse>> {
        info!("Fetching check accounts from SevDesk");
        let url = format!("{}/CheckAccount", self.base_url);
        debug!("Fetching check accounts at: {url}");

        let response = self
            .client
            .get(&url)
            .header("Authorization", &self.api_token)
            .send()
            .await
            .context("Failed to fetch check accounts")?;

        let status = response.status();
        debug!("Check accounts response status: {status}");

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            warn!("Failed to fetch check accounts: {error_text}");
            anyhow::bail!("Failed to fetch check accounts: {}", status);
        }

        let response_text = response
            .text()
            .await
            .context("Failed to read check accounts response")?;
        debug!("Check accounts response: {response_text}");

        let accounts: SevDeskResponse<CheckAccountResponse> = serde_json::from_str(&response_text)
            .context("Failed to parse check accounts response")?;

        let accounts = accounts.objects.unwrap_or_default();

        // Filter to only active accounts
        let active_accounts: Vec<CheckAccountResponse> =
            accounts.into_iter().filter(|a| a.is_active()).collect();

        info!("Found {} active check accounts", active_accounts.len());

        for account in &active_accounts {
            debug!(
                "  - {} (ID: {}, Type: {}, Default: {})",
                account.display_name(),
                account.id,
                account.account_type,
                account.is_default()
            );
        }

        Ok(active_accounts)
    }

    /// Gets the default check account, if one exists.
    #[allow(dead_code)]
    pub async fn get_default_check_account(&self) -> Result<Option<CheckAccountResponse>> {
        let accounts = self.fetch_check_accounts().await?;
        Ok(accounts.into_iter().find(|a| a.is_default()))
    }
}
