//! Integration tests for the CSV processor module.
//!
//! These tests verify end-to-end functionality by reading actual CSV files
//! and validating the complete parsing workflow.

use sevdesk_invoicing::CsvProcessor;
use std::path::PathBuf;

fn fixtures_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

// ==================== File Loading Tests ====================

mod load_orders_from_csv {
    use super::*;

    #[tokio::test]
    async fn loads_single_order_file() {
        let processor = CsvProcessor::new();
        let path = fixtures_path().join("valid_single_order.csv");

        let orders = processor.load_orders_from_csv(&path).await.unwrap();

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].order_id, "1218804750");
        assert_eq!(orders[0].name, "Lucas Cordeiro");
        assert_eq!(orders[0].street, "Hedwig-Porschütz-Straße 28");
        assert_eq!(orders[0].zip, "10557");
        assert_eq!(orders[0].city, "Berlin");
        assert_eq!(orders[0].country, "Germany");
        assert_eq!(orders[0].currency, "EUR");
    }

    #[tokio::test]
    async fn loads_multiple_orders_file() {
        let processor = CsvProcessor::new();
        let path = fixtures_path().join("valid_multiple_orders.csv");

        let orders = processor.load_orders_from_csv(&path).await.unwrap();

        assert_eq!(orders.len(), 3);

        // First order
        assert_eq!(orders[0].name, "John Doe");
        assert_eq!(orders[0].city, "Berlin");

        // Second order
        assert_eq!(orders[1].name, "Jane Smith");
        assert_eq!(orders[1].city, "Hamburg");
        assert_eq!(orders[1].article_count, 2);

        // Third order
        assert_eq!(orders[2].name, "Max Mustermann");
        assert_eq!(orders[2].city, "München");
    }

    #[tokio::test]
    async fn loads_multi_item_order() {
        let processor = CsvProcessor::new();
        let path = fixtures_path().join("multi_item_order.csv");

        let orders = processor.load_orders_from_csv(&path).await.unwrap();

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].article_count, 3);

        // Check that items were parsed correctly
        assert_eq!(orders[0].items.len(), 3);
        assert_eq!(orders[0].items[0].localized_product_name, "Card One");
        assert_eq!(orders[0].items[1].localized_product_name, "Card Two");
        assert_eq!(orders[0].items[2].localized_product_name, "Card Three");

        // Check prices
        assert!((orders[0].items[0].price - 2.50).abs() < 0.01);
        assert!((orders[0].items[1].price - 3.00).abs() < 0.01);
        assert!((orders[0].items[2].price - 3.00).abs() < 0.01);
    }

    #[tokio::test]
    async fn loads_professional_customer_order() {
        let processor = CsvProcessor::new();
        let path = fixtures_path().join("professional_customer.csv");

        let orders = processor.load_orders_from_csv(&path).await.unwrap();

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].is_professional, Some("yes".to_string()));
        assert_eq!(orders[0].vat_number, Some("DE123456789".to_string()));
        assert_eq!(orders[0].shipment_costs, "0,00");
    }

    #[tokio::test]
    async fn loads_international_orders() {
        let processor = CsvProcessor::new();
        let path = fixtures_path().join("international_orders.csv");

        let orders = processor.load_orders_from_csv(&path).await.unwrap();

        assert_eq!(orders.len(), 3);

        // UK order
        assert_eq!(orders[0].country, "United Kingdom");
        assert_eq!(orders[0].zip, "SW1A");
        assert_eq!(orders[0].city, "London");

        // French order
        assert_eq!(orders[1].country, "France");
        assert_eq!(orders[1].city, "Paris");

        // Polish order
        assert_eq!(orders[2].country, "Poland");
        assert_eq!(orders[2].city, "Warsaw");
    }

    #[tokio::test]
    async fn returns_empty_for_header_only_file() {
        let processor = CsvProcessor::new();
        let path = fixtures_path().join("empty_data.csv");

        let orders = processor.load_orders_from_csv(&path).await.unwrap();

        assert!(orders.is_empty());
    }

    #[tokio::test]
    async fn fails_for_nonexistent_file() {
        let processor = CsvProcessor::new();
        let path = fixtures_path().join("nonexistent.csv");

        let result = processor.load_orders_from_csv(&path).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn fails_for_invalid_column_count() {
        let processor = CsvProcessor::new();
        let path = fixtures_path().join("invalid_columns.csv");

        let result = processor.load_orders_from_csv(&path).await;

        assert!(result.is_err());
    }
}

// ==================== Validation Integration Tests ====================

mod validation_integration {
    use super::*;

    #[tokio::test]
    async fn validates_correct_orders_successfully() {
        let processor = CsvProcessor::new();
        let path = fixtures_path().join("valid_multiple_orders.csv");

        let orders = processor.load_orders_from_csv(&path).await.unwrap();
        let errors = processor.validate_orders(&orders);

        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[tokio::test]
    async fn detects_validation_errors_in_file() {
        let processor = CsvProcessor::new();
        let path = fixtures_path().join("validation_errors.csv");

        let orders = processor.load_orders_from_csv(&path).await.unwrap();
        let errors = processor.validate_orders(&orders);

        // Should detect: empty name in first order, empty country and total_value in second
        assert!(!errors.is_empty(), "Expected validation errors");

        // Check that we found the empty name error
        assert!(
            errors.iter().any(|e| e.contains("Customer name is empty")),
            "Should detect empty customer name"
        );
    }

    #[tokio::test]
    async fn validates_international_orders() {
        let processor = CsvProcessor::new();
        let path = fixtures_path().join("international_orders.csv");

        let orders = processor.load_orders_from_csv(&path).await.unwrap();
        let errors = processor.validate_orders(&orders);

        assert!(
            errors.is_empty(),
            "International orders should be valid: {:?}",
            errors
        );
    }
}

// ==================== Edge Cases ====================

mod edge_cases {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn handles_utf8_characters() {
        let processor = CsvProcessor::new();
        let path = fixtures_path().join("valid_single_order.csv");

        let orders = processor.load_orders_from_csv(&path).await.unwrap();

        // Should handle German umlauts and special chars
        assert!(orders[0].street.contains("ü")); // Hedwig-Porschütz-Straße
    }

    #[tokio::test]
    async fn handles_empty_optional_fields() {
        let processor = CsvProcessor::new();
        let path = fixtures_path().join("valid_single_order.csv");

        let orders = processor.load_orders_from_csv(&path).await.unwrap();

        // Optional fields should be None when empty
        assert!(orders[0].is_professional.is_none());
        assert!(orders[0].vat_number.is_none());
    }

    #[tokio::test]
    async fn handles_csv_with_trailing_newlines() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName").unwrap();
        writeln!(temp_file, "12345;user;Test Name;Street 1;10557 Berlin;Germany;;;2025-01-01;1;5,00;1,00;6,00;0,10;EUR;1x Card - 5,00 EUR;111;Card").unwrap();
        writeln!(temp_file).unwrap(); // Trailing newline
        writeln!(temp_file).unwrap(); // Another trailing newline
        temp_file.flush().unwrap();

        let processor = CsvProcessor::new();
        let orders = processor
            .load_orders_from_csv(temp_file.path())
            .await
            .unwrap();

        assert_eq!(orders.len(), 1);
    }

    #[tokio::test]
    async fn handles_whitespace_in_fields() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName").unwrap();
        writeln!(temp_file, "  12345  ;  user  ;  Test Name  ;  Street 1  ;  10557 Berlin  ;  Germany  ;;;2025-01-01;1;5,00;1,00;6,00;0,10;EUR;1x Card - 5,00 EUR;111;Card").unwrap();
        temp_file.flush().unwrap();

        let processor = CsvProcessor::new();
        let orders = processor
            .load_orders_from_csv(temp_file.path())
            .await
            .unwrap();

        // Fields should be trimmed
        assert_eq!(orders[0].order_id, "12345");
        assert_eq!(orders[0].username, "user");
        assert_eq!(orders[0].name, "Test Name");
    }

    #[tokio::test]
    async fn parses_quantity_from_description() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName").unwrap();
        writeln!(temp_file, "12345;user;Test;Street;10557 Berlin;Germany;;;2025-01-01;5;25,00;1,00;26,00;0,50;EUR;5x Bulk Card - 5,00 EUR;111;Bulk Card").unwrap();
        temp_file.flush().unwrap();

        let processor = CsvProcessor::new();
        let orders = processor
            .load_orders_from_csv(temp_file.path())
            .await
            .unwrap();

        assert_eq!(orders[0].items[0].quantity, 5);
    }

    #[tokio::test]
    async fn handles_large_order_count() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName").unwrap();

        // Write 100 orders
        for i in 0..100 {
            writeln!(temp_file, "{};user{};Customer {};Street {};{} City{};Germany;;;2025-01-01;1;5,00;1,00;6,00;0,10;EUR;1x Card - 5,00 EUR;{};Card {}", 
                i, i, i, i, 10000 + i, i, i, i).unwrap();
        }
        temp_file.flush().unwrap();

        let processor = CsvProcessor::new();
        let orders = processor
            .load_orders_from_csv(temp_file.path())
            .await
            .unwrap();

        assert_eq!(orders.len(), 100);

        // Validate all orders pass validation
        let errors = processor.validate_orders(&orders);
        assert!(errors.is_empty());
    }
}

// ==================== Price and Currency Tests ====================

mod price_handling {
    use super::*;

    #[tokio::test]
    async fn correctly_parses_comma_decimal_prices() {
        let processor = CsvProcessor::new();
        let path = fixtures_path().join("valid_single_order.csv");

        let orders = processor.load_orders_from_csv(&path).await.unwrap();

        assert_eq!(orders[0].merchandise_value, "1,87");
        assert_eq!(orders[0].shipment_costs, "1,25");
        assert_eq!(orders[0].total_value, "3,12");

        // Item price should be extracted correctly
        assert!((orders[0].items[0].price - 1.87).abs() < 0.01);
    }

    #[tokio::test]
    async fn handles_zero_shipping_cost() {
        let processor = CsvProcessor::new();
        let path = fixtures_path().join("professional_customer.csv");

        let orders = processor.load_orders_from_csv(&path).await.unwrap();

        assert_eq!(orders[0].shipment_costs, "0,00");
    }
}
