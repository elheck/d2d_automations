//! Contact management functionality.

use anyhow::{Context, Result};
use log::{debug, error, info};

use crate::models::{
    AddressCategory, ContactCategory, ContactResponse, OrderRecord, SevDeskAddress, SevDeskContact,
    SevDeskCountry, SevDeskResponse, SevDeskSingleObjectResponse,
};

use super::SevDeskApi;

impl SevDeskApi {
    /// Gets an existing contact by name or creates a new one.
    pub(crate) async fn get_or_create_contact(&self, order: &OrderRecord) -> Result<u32> {
        debug!("Getting or creating contact for: {}", order.name);
        // First, try to find existing contact by name
        let search_url = format!("{}/Contact", self.base_url);
        debug!("Searching for existing contact at: {search_url}");

        let response = self
            .client
            .get(&search_url)
            .header("Authorization", &self.api_token)
            .query(&[("name", &order.name)])
            .send()
            .await
            .context("Failed to search for contact")?;

        debug!("Contact search response status: {}", response.status());

        // Get the response text first to debug the structure
        let response_text = response
            .text()
            .await
            .context("Failed to read response text")?;
        debug!("Contact search response body: {response_text}");

        // Try to parse the response
        let contacts: SevDeskResponse<ContactResponse> = serde_json::from_str(&response_text)
            .context("Failed to parse contact search response")?;

        if let Some(existing_contacts) = contacts.objects {
            if !existing_contacts.is_empty() {
                if let Some(contact) = existing_contacts.first() {
                    let contact_id = contact
                        .id
                        .parse::<u32>()
                        .context("Failed to parse contact ID from string")?;
                    info!(
                        "Found existing contact: {} (ID: {})",
                        order.name, contact_id
                    );
                    return Ok(contact_id);
                }
            }
        }

        debug!("No existing contact found, creating new contact");
        // Create new contact if not found
        let country_id = self.get_country_id(&order.country).await?;

        let new_contact = SevDeskContact {
            name: order.name.clone(),
            category: ContactCategory {
                id: 3,
                object_name: "Category".to_string(),
            }, // Customer category
            addresses: vec![SevDeskAddress {
                street: order.street.clone(),
                zip: order.zip.clone(),
                city: order.city.clone(),
                country: SevDeskCountry {
                    id: country_id,
                    object_name: "StaticCountry".to_string(),
                },
                category: AddressCategory {
                    id: 47,
                    object_name: "Category".to_string(),
                }, // Billing address
            }],
        };

        let create_url = format!("{}/Contact", self.base_url);
        debug!("Creating new contact at: {create_url}");
        debug!(
            "Contact payload: {}",
            serde_json::to_string_pretty(&new_contact)
                .unwrap_or_else(|_| "Failed to serialize".to_string())
        );

        let response = self
            .client
            .post(&create_url)
            .header("Authorization", &self.api_token)
            .header("Content-Type", "application/json")
            .json(&new_contact)
            .send()
            .await
            .context("Failed to create contact")?;

        debug!("Create contact response status: {}", response.status());

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .context("Failed to read error response")?;
            error!("Contact creation failed with status {status}: {error_text}");
            return Err(anyhow::anyhow!(
                "Failed to create contact: {} - {}",
                status,
                error_text
            ));
        }

        let response_text = response
            .text()
            .await
            .context("Failed to read response text")?;
        debug!("Create contact response body: {response_text}");

        let created_contact: SevDeskSingleObjectResponse<ContactResponse> =
            serde_json::from_str(&response_text)
                .context("Failed to parse create contact response")?;

        let contact_id = created_contact
            .objects
            .id
            .parse::<u32>()
            .context("Failed to parse created contact ID from string")?;

        info!("Created new contact: {} (ID: {})", order.name, contact_id);
        Ok(contact_id)
    }
}
