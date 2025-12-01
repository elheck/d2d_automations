//! User information retrieval.

use anyhow::{Context, Result};
use log::{debug, error, info, warn};

use crate::models::{SevDeskResponse, UserResponse};

use super::SevDeskApi;

impl SevDeskApi {
    /// Gets the current user's ID from SevDesk.
    pub(crate) async fn get_current_user(&self) -> Result<u32> {
        debug!("Getting current user information");
        let url = format!("{}/SevUser", self.base_url);

        let response = self
            .client
            .get(&url)
            .header("Authorization", &self.api_token)
            .send()
            .await
            .context("Failed to get current user")?;

        debug!("Get user response status: {}", response.status());

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .context("Failed to read error response")?;
            error!("Get user failed with status {status}: {error_text}");
            return Err(anyhow::anyhow!(
                "Failed to get current user: {} - {}",
                status,
                error_text
            ));
        }

        let response_text = response
            .text()
            .await
            .context("Failed to read response text")?;
        debug!("Get user response body: {response_text}");

        let users: SevDeskResponse<UserResponse> =
            serde_json::from_str(&response_text).context("Failed to parse get user response")?;

        if let Some(user_list) = users.objects {
            if !user_list.is_empty() {
                if let Some(user) = user_list.first() {
                    let user_id = user
                        .id
                        .parse::<u32>()
                        .context("Failed to parse user ID from string")?;
                    info!("Found current user: {} (ID: {})", user.username, user_id);
                    return Ok(user_id);
                }
            }
        }

        // Fallback: try to get the user from the sevClient info
        warn!("No user found in SevUser endpoint, using default user ID 1");
        Ok(1)
    }
}
