use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct OrderItem {
    pub description: String,
    #[allow(dead_code)]
    pub product_id: String,
    pub localized_product_name: String,
    pub price: f64,
    pub quantity: u32, // Quantity extracted from description (e.g., "2x" = 2)
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderRecord {
    pub order_id: String,
    #[allow(dead_code)]
    pub username: String,
    pub name: String,
    pub street: String,
    pub zip: String,  // Postal code
    pub city: String, // City name without postal code
    pub country: String,
    #[allow(dead_code)]
    pub is_professional: Option<String>,
    #[allow(dead_code)]
    pub vat_number: Option<String>,
    pub date_of_purchase: String,
    pub article_count: u32,
    pub merchandise_value: String,
    pub shipment_costs: String,
    pub total_value: String,
    #[allow(dead_code)]
    pub commission: String,
    pub currency: String,
    pub description: String,
    #[allow(dead_code)]
    pub product_id: String,
    pub localized_product_name: String,
    #[serde(skip)]
    pub items: Vec<OrderItem>, // Parsed individual items for multi-item orders
}

// Simplified structure for card inventory data
#[derive(Debug, Clone)]
pub struct CardRecord {
    pub product_id: String,
    pub card_name: String,
    pub set_name: String,
    #[allow(dead_code)]
    pub collector_number: String,
    #[allow(dead_code)]
    pub rarity: String,
    pub condition: String,
    #[allow(dead_code)]
    pub language: String,
    pub price: String,
    pub currency: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SevDeskContact {
    pub name: String,
    pub category: ContactCategory,
    pub addresses: Vec<SevDeskAddress>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ContactCategory {
    pub id: u32,
    #[serde(rename = "objectName")]
    pub object_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SevDeskAddress {
    pub street: String,
    pub zip: String,
    pub city: String,
    pub country: SevDeskCountry,
    pub category: AddressCategory,
}

#[derive(Debug, Clone, Serialize)]
pub struct SevDeskCountry {
    pub id: u32,
    #[serde(rename = "objectName")]
    pub object_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AddressCategory {
    pub id: u32,
    #[serde(rename = "objectName")]
    pub object_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SevDeskInvoice {
    #[serde(rename = "invoiceNumber")]
    pub invoice_number: Option<String>,
    pub contact: SevDeskContactRef, // Back to contact reference
    #[serde(rename = "invoiceDate")]
    pub invoice_date: String,
    pub header: String,
    #[serde(rename = "headText")]
    pub head_text: Option<String>,
    #[serde(rename = "footText")]
    pub foot_text: Option<String>,
    pub address: Option<String>, // Complete formatted address
    #[serde(rename = "addressCountry")]
    pub address_country: SevDeskCountry,
    #[serde(rename = "deliveryDate")]
    pub delivery_date: String,
    pub status: u32,
    #[serde(rename = "smallSettlement")]
    pub small_settlement: bool,
    #[serde(rename = "contactPerson")]
    pub contact_person: SevDeskUser,
    #[serde(rename = "taxRate")]
    pub tax_rate: f64,
    #[serde(rename = "taxText")]
    pub tax_text: String,
    #[serde(rename = "taxRule")]
    pub tax_rule: SevDeskTaxRule,
    #[serde(rename = "dunningLevel")]
    pub dunning_level: Option<u32>,
    #[serde(rename = "invoiceType")]
    pub invoice_type: String,
    pub currency: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SevDeskTaxRule {
    pub id: u32,
    #[serde(rename = "objectName")]
    pub object_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SevDeskContactRef {
    pub id: u32,
    #[serde(rename = "objectName")]
    pub object_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SevDeskUser {
    pub id: u32,
    #[serde(rename = "objectName")]
    pub object_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SevDeskInvoicePos {
    pub invoice: SevDeskInvoiceRef,
    pub part: Option<SevDeskPart>,
    pub quantity: f64,
    pub price: f64,
    pub name: String,
    pub unity: SevDeskUnity,
    #[serde(rename = "positionNumber")]
    pub position_number: u32,
    pub text: String,
    pub discount: Option<f64>,
    #[serde(rename = "taxRate")]
    pub tax_rate: f64,
    #[serde(rename = "priceNet")]
    pub price_net: f64,
    #[serde(rename = "priceTax")]
    pub price_tax: f64,
    #[serde(rename = "priceGross")]
    pub price_gross: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SevDeskInvoiceRef {
    pub id: String,
    #[serde(rename = "objectName")]
    pub object_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SevDeskPart {
    pub id: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SevDeskUnity {
    pub id: u32,
    #[serde(rename = "objectName")]
    pub object_name: String,
}

// API Response types
#[derive(Debug, Deserialize)]
pub struct SevDeskResponse<T> {
    #[allow(dead_code)]
    pub success: Option<bool>,
    pub objects: Option<Vec<T>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SevDeskObjectResponse<T> {
    pub success: bool,
    pub objects: T,
}

#[derive(Debug, Deserialize)]
pub struct SevDeskSingleObjectResponse<T> {
    pub objects: T,
}

#[derive(Debug, Deserialize)]
pub struct ContactResponse {
    pub id: String, // SevDesk returns ID as string
    #[allow(dead_code)]
    pub name: String,
    #[serde(rename = "objectName")]
    #[allow(dead_code)]
    pub object_name: String,
    // Add other fields that might be useful, but make them optional
    #[serde(rename = "customerNumber")]
    #[allow(dead_code)]
    pub customer_number: Option<String>,
    #[allow(dead_code)]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct InvoiceResponse {
    pub id: String, // SevDesk returns ID as string
    #[serde(rename = "invoiceNumber")]
    pub invoice_number: String,
}

#[derive(Debug, Deserialize)]
pub struct UserResponse {
    pub id: String, // SevDesk returns ID as string
    pub username: String,
    #[serde(rename = "objectName")]
    #[allow(dead_code)]
    pub object_name: String,
}

#[derive(Debug, Clone)]
pub struct InvoiceCreationResult {
    #[allow(dead_code)]
    pub order_id: String,
    pub customer_name: String,
    pub invoice_id: Option<u32>,
    pub invoice_number: Option<String>,
    pub error: Option<String>,
    /// Workflow status - tracks which steps have been completed
    pub workflow_status: Option<InvoiceWorkflowStatus>,
}

/// Status of the invoice workflow steps
#[derive(Debug, Clone, Default)]
pub struct InvoiceWorkflowStatus {
    /// Whether the invoice was finalized (sent/marked as sent)
    pub finalized: bool,
    /// Whether the invoice was enshrined (locked from changes)
    pub enshrined: bool,
    /// Whether the invoice was booked against a check account
    pub booked: bool,
    /// Path where the PDF was saved (if downloaded)
    pub pdf_path: Option<std::path::PathBuf>,
    /// Any error that occurred during workflow
    pub workflow_error: Option<String>,
}

/// Options for invoice workflow processing
#[derive(Debug, Clone, Default)]
pub struct InvoiceWorkflowOptions {
    /// Finalize invoices after creation (mark as sent, status 100 → 200)
    pub finalize: bool,
    /// Send type for finalization: VPR (print), VP (postal), VM (mail), VPDF (pdf download)
    pub send_type: SendType,
    /// Enshrine invoices after finalization (lock from changes)
    pub enshrine: bool,
    /// Book invoices against check account (mark as paid)
    pub book: bool,
    /// Check account ID to book against (required if book is true)
    pub check_account_id: Option<String>,
    /// Directory to save PDFs when using VPDF send type
    pub pdf_download_path: Option<std::path::PathBuf>,
    /// Date of purchase (used as payment date when booking)
    pub payment_date: Option<String>,
}

/// Send type for invoice finalization
#[derive(Debug, Clone, Default, PartialEq)]
pub enum SendType {
    /// Downloaded as PDF
    #[default]
    Vpdf,
    /// Printed
    Vpr,
    /// Sent by postal mail
    Vp,
    /// Sent by email
    Vm,
}

impl SendType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SendType::Vpdf => "VPDF",
            SendType::Vpr => "VPR",
            SendType::Vp => "VP",
            SendType::Vm => "VM",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            SendType::Vpdf => "Downloaded (PDF)",
            SendType::Vpr => "Printed",
            SendType::Vp => "Postal Mail",
            SendType::Vm => "Email",
        }
    }

    pub fn all() -> &'static [SendType] {
        &[SendType::Vpdf, SendType::Vpr, SendType::Vp, SendType::Vm]
    }
}

/// Response from /StaticCountry endpoint
#[derive(Debug, Clone, Deserialize)]
pub struct StaticCountryResponse {
    pub id: String,
    pub name: String,
    #[serde(rename = "nameEn")]
    pub name_en: Option<String>,
    #[serde(rename = "translationCode")]
    #[allow(dead_code)]
    pub translation_code: Option<String>,
    #[allow(dead_code)]
    pub locale: Option<String>,
    #[allow(dead_code)]
    pub priority: Option<String>,
}

/// Response from /CheckAccount endpoint - represents a payment/clearing account
#[derive(Debug, Clone, Deserialize)]
pub struct CheckAccountResponse {
    pub id: String,
    #[serde(rename = "objectName")]
    #[allow(dead_code)]
    pub object_name: String,
    pub name: String,
    #[serde(rename = "type")]
    pub account_type: String, // "online", "offline", or "register"
    pub currency: String,
    #[serde(rename = "defaultAccount")]
    pub default_account: Option<String>,
    pub status: Option<String>, // "0" = Archived, "100" = Active
    #[allow(dead_code)]
    pub iban: Option<String>,
    #[serde(rename = "accountingNumber")]
    pub accounting_number: Option<String>,
}

impl CheckAccountResponse {
    /// Returns a display name for the dropdown (e.g., "Iron Bank (EUR) - 1800")
    pub fn display_name(&self) -> String {
        let mut name = self.name.clone();
        name.push_str(&format!(" ({})", self.currency));
        if let Some(acc_num) = &self.accounting_number {
            name.push_str(&format!(" - {}", acc_num));
        }
        name
    }

    /// Returns true if this is the default account
    pub fn is_default(&self) -> bool {
        self.default_account.as_deref() == Some("1")
    }

    /// Returns true if this account is active (not archived)
    pub fn is_active(&self) -> bool {
        self.status.as_deref() != Some("0")
    }
}
