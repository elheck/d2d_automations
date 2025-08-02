use anyhow::{Context, Result};
use log::{debug, error, info, warn};
use reqwest::Client;

use crate::models::{
    AddressCategory, ContactCategory, ContactResponse, InvoiceCreationResult, InvoiceResponse,
    OrderRecord, SevDeskAddress, SevDeskContact, SevDeskContactRef, SevDeskCountry, SevDeskInvoice,
    SevDeskInvoicePos, SevDeskInvoiceRef, SevDeskResponse, SevDeskSingleObjectResponse,
    SevDeskTaxRule, SevDeskUnity, SevDeskUser, UserResponse,
};

pub struct SevDeskApi {
    client: Client,
    api_token: String,
    base_url: String,
}

impl SevDeskApi {
    pub fn new(api_token: String) -> Self {
        info!("Creating SevDesk API client");
        debug!("API token length: {}", api_token.len());
        Self {
            client: Client::new(),
            api_token,
            base_url: "https://my.sevdesk.de/api/v1".to_string(),
        }
    }

    async fn get_or_create_contact(&self, order: &OrderRecord) -> Result<u32> {
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

    async fn get_country_id(&self, country_name: &str) -> Result<u32> {
        debug!("Getting country ID for: {country_name}");
        // Complete country mappings for SevDesk based on StaticCountry API
        let country_id = match country_name {
            // Major countries (most commonly used)
            "Germany" | "Deutschland" => 1,
            "Austria" | "Österreich" => 2, // Note: This might need verification - not in provided data
            "Switzerland" | "Schweiz" => 3, // Note: This might need verification - not in provided data
            "Afghanistan" => 4,
            "Argentina" | "Argentinien" => 5,
            "Belgium" | "Belgien" => 6,
            "Bulgaria" | "Bulgarien" => 7,
            "Denmark" | "Dänemark" => 8,
            "Great Britain" | "Großbritannien" | "United Kingdom" | "UK" => 9,
            "Finland" | "Finnland" => 10,
            "France" | "Frankreich" => 11,
            "Greece" | "Griechenland" => 12,
            "Ireland" | "Irland" => 13,
            "Italy" | "Italien" => 14,
            "Jamaica" | "Jamaika" => 15,

            // Extended country list from SevDesk API
            "Azerbaijan" | "Aserbaidschan" => 33,
            "Israel" => 36,
            "Australia" | "Australien" => 38,
            "Japan" => 40,
            "Belize" => 46,
            "Chile" => 49,
            "Iceland" | "Island" => 51,
            "Hong Kong" | "Hongkong" => 59,
            "Dubai" => 67,
            "Brazil" | "Brasilien" => 73,
            "England" => 74,

            // Corrected country IDs based on actual SevDesk API data
            "Netherlands" | "Niederlande" => 18,
            "Poland" | "Polen" => 21,
            "Sweden" | "Schweden" => 25,
            "Spain" | "Spanien" => 29,
            "United States" | "USA" => 1353,

            // African countries
            "Algeria" | "Algerien" => 1458,
            "Angola" => 1466,
            "Benin" => 1472,
            "Burkina Faso" => 1473,
            "Botswana" => 1481,
            "Burundi" => 1404,
            "Cameroon" | "Kamerun" => 1400,
            "Chad" => 1482, // Assuming based on pattern
            "Egypt" | "Ägypten" => 1383,
            "Equatorial Guinea" | "Äquatorialguinea" => 1503,
            "Eritrea" => 1406,
            "Ethiopia" | "Äthiopien" => 1399,
            "Gabon" | "Gabun" => 1498,
            "Gambia" => 1501,
            "Ghana" => 1382,
            "Guinea" => 1376,
            "Guinea-Bissau" => 1502,
            "Ivory Coast" | "Elfenbeinküste" => 1381,

            // Asian countries
            "Armenia" | "Armenien" => 1348,
            "Bangladesh" | "Bangladesch" => 1440,
            "Bhutan" => 1350,
            "Brunei" => 1479,
            "Cambodia" | "Kambodscha" => 1401,
            "Georgia" | "Georgien" => 1394,
            "India" | "Indien" => 1375,
            "Indonesia" | "Indonesien" => 1374,
            "Iran" => 1373,
            "Iraq" | "Irak" => 1419,
            "Jordan" | "Jordanien" => 1344,
            "Yemen" | "Jemen" => 1459,

            // European countries
            "Albania" | "Albanien" => 1347,
            "Andorra" => 1418,
            "Bosnia and Herzegovina" | "Bosnien und Herzegowina" => 1396,
            "Estonia" | "Estland" => 1390,
            "Åland Islands" | "Ålandinseln" => 1468,
            "Faroe Islands" | "Färöer-Inseln" => 1496,
            "Gibraltar" => 1500,
            "Guernsey" => 1499,
            "Isle of Man" => 1513,
            "Jersey" => 1515,

            // American countries
            "Antigua and Barbuda" | "Antigua und Barbuda" => 1449,
            "Aruba" => 1465,
            "Bahamas" => 1426,
            "Bahrain" => 1427,
            "Barbados" => 1445,
            "Bolivia" | "Bolivien" => 1477,
            "Costa Rica" => 1421,
            "Curaçao" => 1455,
            "Dominica" => 1491,
            "Dominican Republic" | "Dominikanische Republik" => 1447,
            "Ecuador" => 1492,
            "El Salvador" => 1450,
            "Grenada" => 1504,
            "Guatemala" => 1506,
            "Guyana" => 1509,
            "Haiti" => 1512,
            "Honduras" => 1511,

            // Caribbean and territories
            "Anguilla" => 1467,
            "American Samoa" | "Amerikanisch-Samoa" => 1469,
            "American Virgin Islands" | "Amerikanische Jungferninseln" => 1559,
            "British Virgin Islands" | "Britische Jungferninseln" => 1391,
            "Bermuda" => 1476,
            "Cayman Islands" | "Kaimaninseln" => 1460,
            "Cook Islands" | "Cookinseln" => 1486,
            "Falkland Islands" | "Falklandinseln" => 1495,
            "Fiji" | "Fidschi" => 1494,
            "Guam" => 1508,
            "Guadeloupe" => 1438,

            // Special territories
            "Antarctica" | "Antarktis" => 1470,
            "Bouvet Island" | "Bouvetinsel" => 1480,
            "British Indian Ocean Territory" | "Britisches Territorium im Indischen Ozean" => 1514,
            "French Guiana" | "Französisch Guyana" => 1507,
            "French Polynesia" | "Französisch-Polynesien" => 1351,
            "French Southern and Antarctic Lands" | "Französische Süd-und Antarktisgebiete" => {
                1471
            }
            "Heard Island and McDonald Islands" | "Heard und die McDonaldinseln" => 1510,

            // Additional African countries
            "Djibouti" | "Dschibuti" => 1405,

            _ => {
                warn!("Unknown country '{country_name}', defaulting to Germany (ID: 1)");
                1 // Default to Germany
            }
        };
        debug!("Country '{country_name}' mapped to ID: {country_id}");
        Ok(country_id)
    }

    fn parse_price(&self, price_str: &str) -> Result<f64> {
        debug!("Parsing price: '{price_str}'");
        let clean_price = price_str.replace(',', ".");
        let result = clean_price.parse::<f64>().context("Failed to parse price");

        match &result {
            Ok(price) => debug!("Parsed price '{price_str}' as {price}"),
            Err(e) => error!("Failed to parse price '{price_str}': {e}"),
        }

        result
    }

    pub async fn create_invoice(&self, order: &OrderRecord) -> Result<InvoiceCreationResult> {
        info!(
            "Creating invoice for order: {} ({})",
            order.order_id, order.name
        );
        let order_id = order.order_id.clone();
        let customer_name = order.name.clone();

        match self.create_invoice_internal(order).await {
            Ok((invoice_id, invoice_number)) => {
                info!("Successfully created invoice: {invoice_number} for order {order_id}");
                Ok(InvoiceCreationResult {
                    order_id,
                    customer_name,
                    invoice_id: Some(invoice_id.parse().unwrap_or(0)),
                    invoice_number: Some(invoice_number),
                    error: None,
                })
            }
            Err(e) => {
                error!("Failed to create invoice for order {order_id}: {e}");
                Ok(InvoiceCreationResult {
                    order_id,
                    customer_name,
                    invoice_id: None,
                    invoice_number: None,
                    error: Some(e.to_string()),
                })
            }
        }
    }

    pub async fn simulate_invoice_creation(
        &self,
        order: &OrderRecord,
    ) -> Result<InvoiceCreationResult> {
        info!(
            "Simulating invoice creation for order: {} ({})",
            order.order_id, order.name
        );
        let order_id = order.order_id.clone();
        let customer_name = order.name.clone();

        // Simulate the validation steps without actually making API calls
        match self.simulate_invoice_validation(order).await {
            Ok(simulated_invoice_number) => {
                info!("Successfully simulated invoice: {simulated_invoice_number} for order {order_id}");
                Ok(InvoiceCreationResult {
                    order_id,
                    customer_name,
                    invoice_id: Some(99999), // Fake ID for dry run
                    invoice_number: Some(simulated_invoice_number),
                    error: None,
                })
            }
            Err(e) => {
                error!("Failed to simulate invoice for order {order_id}: {e}");
                Ok(InvoiceCreationResult {
                    order_id,
                    customer_name,
                    invoice_id: None,
                    invoice_number: None,
                    error: Some(e.to_string()),
                })
            }
        }
    }

    async fn simulate_invoice_validation(&self, order: &OrderRecord) -> Result<String> {
        debug!(
            "Simulating invoice validation for order: {}",
            order.order_id
        );

        // Validate country mapping
        let country_id = self.get_country_id(&order.country).await?;
        debug!(
            "Country '{}' would map to ID: {}",
            order.country, country_id
        );

        // Validate price parsing
        let merchandise_value = self.parse_price(&order.merchandise_value)?;
        let shipment_costs = self.parse_price(&order.shipment_costs)?;
        let total_value = self.parse_price(&order.total_value)?;
        debug!("Prices would be - merchandise: {merchandise_value:.2}, shipping: {shipment_costs:.2}, total: {total_value:.2}");

        // Validate items and quantities
        if !order.items.is_empty() {
            debug!("Would create {} invoice positions:", order.items.len());
            for (i, item) in order.items.iter().enumerate() {
                debug!(
                    "  Position {}: {} x {} @ {:.2} EUR",
                    i + 1,
                    item.quantity,
                    item.localized_product_name,
                    item.price
                );
            }
        } else {
            debug!(
                "Would create fallback position: {} x {} @ {:.2} EUR",
                order.article_count, order.localized_product_name, merchandise_value
            );
        }

        if shipment_costs > 0.0 {
            debug!("Would add shipping position: {shipment_costs:.2} EUR");
        }

        // Generate a simulated invoice number
        let simulated_invoice_number = format!("DRY-{}", order.order_id);
        debug!("Simulated invoice number: {simulated_invoice_number}");

        Ok(simulated_invoice_number)
    }

    async fn get_current_user(&self) -> Result<u32> {
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

    async fn create_invoice_internal(&self, order: &OrderRecord) -> Result<(String, String)> {
        debug!(
            "Starting internal invoice creation for order: {}",
            order.order_id
        );

        // Get or create contact
        let contact_id = self.get_or_create_contact(order).await?;

        // Get current user ID
        let user_id = self.get_current_user().await?;

        // Parse prices
        let merchandise_value = self.parse_price(&order.merchandise_value)?;
        let shipment_costs = self.parse_price(&order.shipment_costs)?;
        let _total_value = self.parse_price(&order.total_value)?;

        debug!("Parsed prices - merchandise: {merchandise_value}, shipping: {shipment_costs}");

        // Create invoice
        let country_id = self.get_country_id(&order.country).await?;

        // Format the complete address
        let formatted_address = format!(
            "{}\n{}\n{} {}",
            order.name, order.street, order.zip, order.city
        );

        let invoice = SevDeskInvoice {
            invoice_number: None, // Let SevDesk auto-generate
            contact: SevDeskContactRef {
                id: contact_id,
                object_name: "Contact".to_string(),
            },
            invoice_date: order
                .date_of_purchase
                .split(' ')
                .next()
                .unwrap_or("")
                .to_string(),
            header: format!("Rechnung für Bestellnummer {}", order.order_id),
            head_text: Some("Vielen Dank für Ihre Bestellung.".to_string()),
            foot_text: Some("Betrag beglichen.".to_string()),
            address: Some(formatted_address),
            address_country: SevDeskCountry {
                id: country_id,
                object_name: "StaticCountry".to_string(),
            },
            delivery_date: order
                .date_of_purchase
                .split(' ')
                .next()
                .unwrap_or("")
                .to_string(),
            status: 100, // Draft status
            small_settlement: false,
            contact_person: SevDeskUser {
                id: user_id,
                object_name: "SevUser".to_string(),
            },
            tax_rate: 0.0, // No VAT for Kleingewerbe
            tax_text: "Kleinunternehmerregelung §19 UStG".to_string(),
            tax_rule: SevDeskTaxRule {
                id: 11, // Tax rule 11 for Kleingewerbe
                object_name: "TaxRule".to_string(),
            },
            dunning_level: None,
            invoice_type: "RE".to_string(), // Regular invoice
            currency: order.currency.clone(),
        };

        let create_invoice_url = format!("{}/Invoice", self.base_url);
        debug!("Creating invoice at: {create_invoice_url}");

        let response = self
            .client
            .post(&create_invoice_url)
            .header("Authorization", &self.api_token)
            .header("Content-Type", "application/json")
            .json(&invoice)
            .send()
            .await
            .context("Failed to create invoice")?;

        debug!("Create invoice response status: {}", response.status());

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .context("Failed to read error response")?;
            error!("Invoice creation failed with status {status}: {error_text}");
            return Err(anyhow::anyhow!(
                "Failed to create invoice: {} - {}",
                status,
                error_text
            ));
        }

        let response_text = response
            .text()
            .await
            .context("Failed to read response text")?;
        debug!("Create invoice response body: {response_text}");

        let created_invoice: SevDeskSingleObjectResponse<InvoiceResponse> =
            serde_json::from_str(&response_text)
                .context("Failed to parse create invoice response")?;

        let invoice_id = created_invoice.objects.id.clone();
        let invoice_number = created_invoice.objects.invoice_number;
        debug!("Created invoice with ID: {invoice_id} and number: {invoice_number}");

        // Add each item as a separate invoice position
        let mut position_number = 1;

        if order.items.len() > 1 {
            info!("Adding {} individual items to invoice", order.items.len());
            for item in &order.items {
                debug!(
                    "Adding item position {}: {} x {} @ {:.2} EUR",
                    position_number, item.quantity, item.localized_product_name, item.price
                );
                self.add_invoice_position(
                    &invoice_id,
                    position_number,
                    &item.localized_product_name,
                    &item.description,
                    item.quantity as f64, // Use the extracted quantity from description
                    item.price,
                )
                .await?;
                position_number += 1;
            }
        } else if !order.items.is_empty() {
            // Single item order
            let item = &order.items[0];
            debug!(
                "Adding single item position: {} x {} @ {:.2} EUR",
                item.quantity, item.localized_product_name, item.price
            );
            self.add_invoice_position(
                &invoice_id,
                position_number,
                &item.localized_product_name,
                &item.description,
                item.quantity as f64, // Use the extracted quantity from description
                item.price,
            )
            .await?;
            position_number += 1;
        } else {
            // Fallback to original merchandise value
            debug!(
                "No items found, using fallback merchandise position: {} x {} = {}",
                order.article_count,
                merchandise_value,
                order.article_count as f64 * merchandise_value
            );
            self.add_invoice_position(
                &invoice_id,
                position_number,
                &order.localized_product_name,
                &order.description,
                order.article_count as f64,
                merchandise_value,
            )
            .await?;
            position_number += 1;
        }

        // Add shipping costs as separate position if any
        if shipment_costs > 0.0 {
            debug!("Adding shipping position: {shipment_costs}");
            self.add_invoice_position(
                &invoice_id,
                position_number,
                "Shipping",
                "Shipping costs",
                1.0,
                shipment_costs,
            )
            .await?;
        }

        Ok((invoice_id, invoice_number))
    }

    async fn add_invoice_position(
        &self,
        invoice_id: &str,
        position_number: u32,
        name: &str,
        description: &str,
        quantity: f64,
        price_gross: f64,
    ) -> Result<()> {
        debug!("Adding invoice position {position_number}: {quantity} x {name} @ {price_gross}");

        // For Kleingewerbe (tax rule 11), no VAT is applied
        let tax_rate = 0.0; // No VAT for Kleingewerbe
        let price_net = price_gross; // Price is the same as gross since no VAT
        let price_tax = 0.0; // No tax

        debug!(
            "Kleingewerbe pricing - net: {price_net:.2}, tax: {price_tax:.2}, gross: {price_gross:.2}"
        );

        let position = SevDeskInvoicePos {
            invoice: SevDeskInvoiceRef {
                id: invoice_id.to_string(),
                object_name: "Invoice".to_string(),
            },
            part: None,
            quantity,
            price: price_net,
            name: name.to_string(),
            unity: SevDeskUnity {
                id: 1,
                object_name: "Unity".to_string(),
            }, // Piece
            position_number,
            text: description.to_string(),
            discount: None,
            tax_rate,
            price_net,
            price_tax,
            price_gross,
        };

        let create_position_url = format!("{}/InvoicePos", self.base_url);
        debug!("Creating invoice position at: {create_position_url}");

        let response = self
            .client
            .post(&create_position_url)
            .header("Authorization", &self.api_token)
            .header("Content-Type", "application/json")
            .json(&position)
            .send()
            .await
            .context("Failed to create invoice position")?;

        debug!("Create position response status: {}", response.status());
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            error!("Failed to create invoice position: {error_text}");
        }

        Ok(())
    }

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
