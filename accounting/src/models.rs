use serde::{Deserialize, Serialize};
use chrono::{DateTime, NaiveDateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderData {
    #[serde(rename = "OrderID")]
    pub order_id: String,
    
    #[serde(rename = "Username")]
    pub username: String,
    
    #[serde(rename = "Name")]
    pub name: String,
    
    #[serde(rename = "Street")]
    pub street: String,
    
    #[serde(rename = "City")]
    pub city: String,
    
    #[serde(rename = "Country")]
    pub country: String,
    
    #[serde(rename = "Is Professional")]
    pub is_professional: String,
    
    #[serde(rename = "VAT Number")]
    pub vat_number: String,
    
    #[serde(rename = "Date of Purchase")]
    pub date_of_purchase: String,
    
    #[serde(rename = "Article Count")]
    pub article_count: i32,
    
    #[serde(rename = "Merchandise Value")]
    pub merchandise_value: String,
    
    #[serde(rename = "Shipment Costs")]
    pub shipment_costs: String,
    
    #[serde(rename = "Total Value")]
    pub total_value: String,
    
    #[serde(rename = "Commission")]
    pub commission: String,
    
    #[serde(rename = "Currency")]
    pub currency: String,
    
    #[serde(rename = "Description")]
    pub description: String,
    
    #[serde(rename = "Product ID")]
    pub product_id: String,
    
    #[serde(rename = "Localized Product Name")]
    pub localized_product_name: String,
}

impl OrderData {
    pub fn parse_date(&self) -> Option<DateTime<Utc>> {
        NaiveDateTime::parse_from_str(&self.date_of_purchase, "%Y-%m-%d %H:%M:%S")
            .ok()
            .map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc))
    }
    
    pub fn parse_total_value(&self) -> Option<f64> {
        self.total_value
            .replace(',', ".")
            .parse::<f64>()
            .ok()
    }
    
    pub fn parse_merchandise_value(&self) -> Option<f64> {
        self.merchandise_value
            .replace(',', ".")
            .parse::<f64>()
            .ok()
    }
    
    pub fn parse_shipment_costs(&self) -> Option<f64> {
        self.shipment_costs
            .replace(',', ".")
            .parse::<f64>()
            .ok()
    }
    
    pub fn parse_commission(&self) -> Option<f64> {
        self.commission
            .replace(',', ".")
            .parse::<f64>()
            .ok()
    }

    pub fn get_full_address(&self) -> String {
        format!("{}\n{}\n{}", self.name, self.street, self.city)
    }
}

#[derive(Debug, Clone)]
pub struct InvoiceData {
    pub invoice_number: String,
    pub invoice_date: String,
    pub service_date: String,
    pub customer_name: String,
    pub customer_address: String,
    pub description: String,
    pub net_amount: f64,
    pub shipping_cost: f64,
    pub total_amount: f64,
    pub currency: String,
}
