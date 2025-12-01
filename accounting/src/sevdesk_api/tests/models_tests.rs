//! Tests for model types used by the SevDesk API.

use crate::models::{InvoiceCreationResult, OrderItem, OrderRecord};

fn create_test_order() -> OrderRecord {
    OrderRecord {
        order_id: "12345".to_string(),
        username: "testuser".to_string(),
        name: "John Doe".to_string(),
        street: "Main Street 1".to_string(),
        zip: "10557".to_string(),
        city: "Berlin".to_string(),
        country: "Germany".to_string(),
        is_professional: None,
        vat_number: None,
        date_of_purchase: "2025-01-15".to_string(),
        article_count: 1,
        merchandise_value: "5,00".to_string(),
        shipment_costs: "1,50".to_string(),
        total_value: "6,50".to_string(),
        commission: "0,10".to_string(),
        currency: "EUR".to_string(),
        description: "1x Test Card - 5,00 EUR".to_string(),
        product_id: "98765".to_string(),
        localized_product_name: "Test Card".to_string(),
        items: vec![OrderItem {
            description: "1x Test Card - 5,00 EUR".to_string(),
            product_id: "98765".to_string(),
            localized_product_name: "Test Card".to_string(),
            price: 5.0,
            quantity: 1,
        }],
    }
}

#[test]
fn order_record_has_required_fields() {
    let order = create_test_order();
    assert_eq!(order.order_id, "12345");
    assert_eq!(order.name, "John Doe");
    assert_eq!(order.country, "Germany");
    assert_eq!(order.article_count, 1);
}

#[test]
fn order_with_multiple_items() {
    let mut order = create_test_order();
    order.items = vec![
        OrderItem {
            description: "1x Card A".to_string(),
            product_id: "111".to_string(),
            localized_product_name: "Card A".to_string(),
            price: 3.0,
            quantity: 1,
        },
        OrderItem {
            description: "2x Card B".to_string(),
            product_id: "222".to_string(),
            localized_product_name: "Card B".to_string(),
            price: 3.5,
            quantity: 2,
        },
    ];
    order.article_count = 3;

    assert_eq!(order.items.len(), 2);
    assert_eq!(order.article_count, 3);
}

#[test]
fn order_professional_customer() {
    let mut order = create_test_order();
    order.is_professional = Some("true".to_string());
    order.vat_number = Some("DE123456789".to_string());

    assert_eq!(order.is_professional.as_ref().unwrap(), "true");
    assert_eq!(order.vat_number.as_ref().unwrap(), "DE123456789");
}

#[test]
fn invoice_creation_result_success() {
    let result = InvoiceCreationResult {
        order_id: "12345".to_string(),
        customer_name: "Test Customer".to_string(),
        invoice_id: Some(100),
        invoice_number: Some("INV-001".to_string()),
        error: None,
    };

    assert_eq!(result.order_id, "12345");
    assert!(result.invoice_id.is_some());
    assert!(result.error.is_none());
}

#[test]
fn invoice_creation_result_error() {
    let result = InvoiceCreationResult {
        order_id: "12345".to_string(),
        customer_name: "Test Customer".to_string(),
        invoice_id: None,
        invoice_number: None,
        error: Some("API error".to_string()),
    };

    assert!(result.invoice_id.is_none());
    assert!(result.error.is_some());
}
