//! Unit tests for the CSV processor facade.

use super::*;

#[test]
fn csv_processor_default_works() {
    let processor = CsvProcessor;
    // Just ensure it can be created
    let _ = processor;
}

#[test]
fn csv_processor_new_works() {
    let processor = CsvProcessor::new();
    let _ = processor;
}

#[test]
fn parse_empty_content_returns_empty_vec() {
    let processor = CsvProcessor::new();
    let orders = processor.parse_csv_content("").unwrap();
    assert!(orders.is_empty());
}

#[test]
fn detects_csv_format_with_order_id_header() {
    let processor = CsvProcessor::new();
    let content = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName\n\
                  1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

    let orders = processor.parse_csv_content(content).unwrap();

    assert_eq!(orders.len(), 1);
    assert_eq!(orders[0].order_id, "1234567");
}

#[test]
fn validate_orders_delegates_to_validator() {
    let processor = CsvProcessor::new();
    let orders = vec![OrderRecord {
        order_id: "12345".to_string(),
        username: "user".to_string(),
        name: "".to_string(), // Empty name should cause validation error
        street: "Street".to_string(),
        zip: "12345".to_string(),
        city: "City".to_string(),
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
        description: "Test".to_string(),
        product_id: "98765".to_string(),
        localized_product_name: "Test Card".to_string(),
        items: vec![],
    }];

    let errors = processor.validate_orders(&orders);

    assert!(!errors.is_empty());
    assert!(errors[0].contains("Customer name is empty"));
}
