//! Unit tests for the CSV processor module.
//!
//! These tests are kept separate from the production code but can still
//! access private functions via the `super::` import.

use super::*;

// ==================== Unit Tests ====================

mod parse_city_field {
    use super::*;

    #[test]
    fn parses_german_city_with_postal_code() {
        let processor = CsvProcessor::new();
        let (zip, city) = processor.parse_city_field("10557 Berlin").unwrap();
        assert_eq!(zip, "10557");
        assert_eq!(city, "Berlin");
    }

    #[test]
    fn parses_city_with_spaces_in_name() {
        let processor = CsvProcessor::new();
        let (zip, city) = processor.parse_city_field("12345 Bad Neustadt").unwrap();
        assert_eq!(zip, "12345");
        assert_eq!(city, "Bad Neustadt");
    }

    #[test]
    fn parses_international_postal_code() {
        let processor = CsvProcessor::new();
        let (zip, city) = processor.parse_city_field("SW1A London").unwrap();
        assert_eq!(zip, "SW1A");
        assert_eq!(city, "London");
    }

    #[test]
    fn handles_city_without_postal_code() {
        let processor = CsvProcessor::new();
        let (zip, city) = processor.parse_city_field("Berlin").unwrap();
        assert_eq!(zip, "");
        assert_eq!(city, "Berlin");
    }

    #[test]
    fn handles_empty_string() {
        let processor = CsvProcessor::new();
        let (zip, city) = processor.parse_city_field("").unwrap();
        assert_eq!(zip, "");
        assert_eq!(city, "");
    }

    #[test]
    fn trims_internal_whitespace() {
        let processor = CsvProcessor::new();
        // Note: Leading whitespace should be trimmed at CSV parsing level
        // This tests that internal whitespace is handled correctly
        let (zip, city) = processor.parse_city_field("10557   Berlin").unwrap();
        assert_eq!(zip, "10557");
        assert_eq!(city, "Berlin");
    }
}

mod extract_quantity_from_description {
    use super::*;

    #[test]
    fn extracts_single_digit_quantity() {
        let processor = CsvProcessor::new();
        let quantity = processor.extract_quantity_from_description("2x High Fae Trickster");
        assert_eq!(quantity, 2);
    }

    #[test]
    fn extracts_double_digit_quantity() {
        let processor = CsvProcessor::new();
        let quantity = processor.extract_quantity_from_description("10x Some Card Name");
        assert_eq!(quantity, 10);
    }

    #[test]
    fn defaults_to_one_without_quantity() {
        let processor = CsvProcessor::new();
        let quantity = processor.extract_quantity_from_description("High Fae Trickster - 1,87 EUR");
        assert_eq!(quantity, 1);
    }

    #[test]
    fn defaults_to_one_for_empty_string() {
        let processor = CsvProcessor::new();
        let quantity = processor.extract_quantity_from_description("");
        assert_eq!(quantity, 1);
    }

    #[test]
    fn handles_quantity_with_full_description() {
        let processor = CsvProcessor::new();
        let quantity = processor.extract_quantity_from_description(
            "1x High Fae Trickster (Magic: The Gathering Foundations) - 40 - Rare - NM - English - 1,87 EUR",
        );
        assert_eq!(quantity, 1);
    }
}

mod extract_price_from_description {
    use super::*;

    #[test]
    fn extracts_price_with_comma_decimal() {
        let processor = CsvProcessor::new();
        let price = processor
            .extract_price_from_description("Some Card - 1,87 EUR")
            .unwrap();
        assert!((price - 1.87).abs() < 0.001);
    }

    #[test]
    fn extracts_price_with_dot_decimal() {
        let processor = CsvProcessor::new();
        let price = processor
            .extract_price_from_description("Some Card - 5.35 EUR")
            .unwrap();
        assert!((price - 5.35).abs() < 0.001);
    }

    #[test]
    fn extracts_price_from_full_description() {
        let processor = CsvProcessor::new();
        let price = processor
            .extract_price_from_description(
                "1x High Fae Trickster (Magic: The Gathering Foundations) - 40 - Rare - NM - English - 1,87 EUR",
            )
            .unwrap();
        assert!((price - 1.87).abs() < 0.001);
    }

    #[test]
    fn extracts_larger_price() {
        let processor = CsvProcessor::new();
        let price = processor
            .extract_price_from_description("Expensive Card - 125,99 EUR")
            .unwrap();
        assert!((price - 125.99).abs() < 0.001);
    }

    #[test]
    fn fails_without_eur_marker() {
        let processor = CsvProcessor::new();
        let result = processor.extract_price_from_description("Some Card - 1,87");
        assert!(result.is_err());
    }

    #[test]
    fn fails_for_empty_string() {
        let processor = CsvProcessor::new();
        let result = processor.extract_price_from_description("");
        assert!(result.is_err());
    }
}

mod parse_price {
    use super::*;

    #[test]
    fn parses_comma_decimal() {
        let processor = CsvProcessor::new();
        let price = processor.parse_price("1,87").unwrap();
        assert!((price - 1.87).abs() < 0.001);
    }

    #[test]
    fn parses_dot_decimal() {
        let processor = CsvProcessor::new();
        let price = processor.parse_price("1.87").unwrap();
        assert!((price - 1.87).abs() < 0.001);
    }

    #[test]
    fn parses_integer() {
        let processor = CsvProcessor::new();
        let price = processor.parse_price("100").unwrap();
        assert!((price - 100.0).abs() < 0.001);
    }

    #[test]
    fn parses_zero() {
        let processor = CsvProcessor::new();
        let price = processor.parse_price("0").unwrap();
        assert!((price - 0.0).abs() < 0.001);
    }

    #[test]
    fn fails_for_invalid_input() {
        let processor = CsvProcessor::new();
        let result = processor.parse_price("not a number");
        assert!(result.is_err());
    }

    #[test]
    fn fails_for_empty_string() {
        let processor = CsvProcessor::new();
        let result = processor.parse_price("");
        assert!(result.is_err());
    }
}

mod parse_order_items {
    use super::*;

    #[test]
    fn parses_single_item() {
        let processor = CsvProcessor::new();
        let items = processor
            .parse_order_items("1x Card Name - 1,87 EUR", "12345", "Card Name")
            .unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].product_id, "12345");
        assert_eq!(items[0].localized_product_name, "Card Name");
        assert_eq!(items[0].quantity, 1);
        assert!((items[0].price - 1.87).abs() < 0.001);
    }

    #[test]
    fn parses_multiple_items() {
        let processor = CsvProcessor::new();
        let items = processor
            .parse_order_items(
                "1x Card One - 1,50 EUR | 2x Card Two - 3,00 EUR",
                "111 | 222",
                "Card One | Card Two",
            )
            .unwrap();

        assert_eq!(items.len(), 2);

        assert_eq!(items[0].product_id, "111");
        assert_eq!(items[0].localized_product_name, "Card One");
        assert_eq!(items[0].quantity, 1);
        assert!((items[0].price - 1.50).abs() < 0.001);

        assert_eq!(items[1].product_id, "222");
        assert_eq!(items[1].localized_product_name, "Card Two");
        assert_eq!(items[1].quantity, 2);
        assert!((items[1].price - 3.00).abs() < 0.001);
    }

    #[test]
    fn handles_mismatched_counts_as_single() {
        let processor = CsvProcessor::new();
        // When counts don't match, treat as single item
        let items = processor
            .parse_order_items(
                "1x Card One - 1,50 EUR | 2x Card Two - 3,00 EUR",
                "111", // Only one ID
                "Card One | Card Two",
            )
            .unwrap();

        assert_eq!(items.len(), 1);
    }
}

mod parse_order_line {
    use super::*;

    #[test]
    fn parses_valid_order_line() {
        let processor = CsvProcessor::new();
        let line = "1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

        let order = processor.parse_order_line(line).unwrap();

        assert_eq!(order.order_id, "1234567");
        assert_eq!(order.username, "user123");
        assert_eq!(order.name, "John Doe");
        assert_eq!(order.street, "Main Street 1");
        assert_eq!(order.zip, "10557");
        assert_eq!(order.city, "Berlin");
        assert_eq!(order.country, "Germany");
        assert_eq!(order.date_of_purchase, "2025-01-15");
        assert_eq!(order.article_count, 1);
        assert_eq!(order.merchandise_value, "5,00");
        assert_eq!(order.shipment_costs, "1,50");
        assert_eq!(order.total_value, "6,50");
        assert_eq!(order.currency, "EUR");
        assert_eq!(order.product_id, "98765");
        assert_eq!(order.localized_product_name, "Card Name");
    }

    #[test]
    fn parses_order_with_optional_fields_empty() {
        let processor = CsvProcessor::new();
        let line = "1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

        let order = processor.parse_order_line(line).unwrap();

        assert!(order.is_professional.is_none());
        assert!(order.vat_number.is_none());
    }

    #[test]
    fn parses_order_with_professional_flag() {
        let processor = CsvProcessor::new();
        let line = "1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;yes;DE123456789;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

        let order = processor.parse_order_line(line).unwrap();

        assert_eq!(order.is_professional, Some("yes".to_string()));
        assert_eq!(order.vat_number, Some("DE123456789".to_string()));
    }

    #[test]
    fn fails_with_insufficient_columns() {
        let processor = CsvProcessor::new();
        let line = "1234567;user123;John Doe";

        let result = processor.parse_order_line(line);
        assert!(result.is_err());
    }

    #[test]
    fn fails_with_invalid_article_count() {
        let processor = CsvProcessor::new();
        let line = "1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;not_a_number;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

        let result = processor.parse_order_line(line);
        assert!(result.is_err());
    }
}

mod parse_csv_content {
    use super::*;

    #[test]
    fn parses_csv_with_headers() {
        let processor = CsvProcessor::new();
        let content = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName\n\
                      1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

        let orders = processor.parse_csv_content(content).unwrap();

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].order_id, "1234567");
        assert_eq!(orders[0].name, "John Doe");
    }

    #[test]
    fn parses_multiple_orders() {
        let processor = CsvProcessor::new();
        let content = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName\n\
                      1234567;user1;John Doe;Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card One\n\
                      1234568;user2;Jane Doe;Street 2;20095 Hamburg;Germany;;;2025-01-16;2;10,00;1,50;11,50;0,20;EUR;2x Card - 5,00 EUR;98766;Card Two";

        let orders = processor.parse_csv_content(content).unwrap();

        assert_eq!(orders.len(), 2);
        assert_eq!(orders[0].name, "John Doe");
        assert_eq!(orders[1].name, "Jane Doe");
    }

    #[test]
    fn returns_empty_for_empty_content() {
        let processor = CsvProcessor::new();
        let orders = processor.parse_csv_content("").unwrap();
        assert!(orders.is_empty());
    }

    #[test]
    fn returns_empty_for_header_only() {
        let processor = CsvProcessor::new();
        let content = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName";

        let orders = processor.parse_csv_content(content).unwrap();
        assert!(orders.is_empty());
    }

    #[test]
    fn skips_empty_lines() {
        let processor = CsvProcessor::new();
        let content = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName\n\
                      \n\
                      1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name\n\
                      ";

        let orders = processor.parse_csv_content(content).unwrap();
        assert_eq!(orders.len(), 1);
    }
}

mod validate_orders {
    use super::*;
    use crate::models::OrderItem;

    fn create_valid_order() -> OrderRecord {
        OrderRecord {
            order_id: "12345".to_string(),
            username: "user".to_string(),
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
            description: "1x Card".to_string(),
            product_id: "98765".to_string(),
            localized_product_name: "Card Name".to_string(),
            items: vec![OrderItem {
                description: "1x Card".to_string(),
                product_id: "98765".to_string(),
                localized_product_name: "Card Name".to_string(),
                price: 5.0,
                quantity: 1,
            }],
        }
    }

    #[test]
    fn validates_correct_order() {
        let processor = CsvProcessor::new();
        let orders = vec![create_valid_order()];

        let errors = processor.validate_orders(&orders);
        assert!(errors.is_empty());
    }

    #[test]
    fn detects_empty_customer_name() {
        let processor = CsvProcessor::new();
        let mut order = create_valid_order();
        order.name = "".to_string();

        let errors = processor.validate_orders(&[order]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Customer name is empty"));
    }

    #[test]
    fn detects_empty_country() {
        let processor = CsvProcessor::new();
        let mut order = create_valid_order();
        order.country = "".to_string();

        let errors = processor.validate_orders(&[order]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Country is empty"));
    }

    #[test]
    fn detects_empty_total_value() {
        let processor = CsvProcessor::new();
        let mut order = create_valid_order();
        order.total_value = "".to_string();

        let errors = processor.validate_orders(&[order]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Total value is empty"));
    }

    #[test]
    fn detects_invalid_total_value_format() {
        let processor = CsvProcessor::new();
        let mut order = create_valid_order();
        order.total_value = "not a price".to_string();

        let errors = processor.validate_orders(&[order]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Invalid total value format"));
    }

    #[test]
    fn accepts_comma_decimal_in_total_value() {
        let processor = CsvProcessor::new();
        let mut order = create_valid_order();
        order.total_value = "6,50".to_string();

        let errors = processor.validate_orders(&[order]);
        assert!(errors.is_empty());
    }

    #[test]
    fn detects_empty_currency() {
        let processor = CsvProcessor::new();
        let mut order = create_valid_order();
        order.currency = "".to_string();

        let errors = processor.validate_orders(&[order]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Currency is empty"));
    }

    #[test]
    fn detects_empty_date() {
        let processor = CsvProcessor::new();
        let mut order = create_valid_order();
        order.date_of_purchase = "".to_string();

        let errors = processor.validate_orders(&[order]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Purchase date is empty"));
    }

    #[test]
    fn detects_multiple_errors() {
        let processor = CsvProcessor::new();
        let mut order = create_valid_order();
        order.name = "".to_string();
        order.country = "".to_string();
        order.currency = "".to_string();

        let errors = processor.validate_orders(&[order]);
        assert_eq!(errors.len(), 3);
    }

    #[test]
    fn validates_multiple_orders() {
        let processor = CsvProcessor::new();
        let mut order1 = create_valid_order();
        order1.name = "".to_string();

        let mut order2 = create_valid_order();
        order2.country = "".to_string();

        let errors = processor.validate_orders(&[order1, order2]);
        assert_eq!(errors.len(), 2);
        assert!(errors[0].contains("Line 2")); // First order
        assert!(errors[1].contains("Line 3")); // Second order
    }

    #[test]
    fn allows_empty_street_and_city() {
        let processor = CsvProcessor::new();
        let mut order = create_valid_order();
        order.street = "".to_string();
        order.city = "".to_string();

        let errors = processor.validate_orders(&[order]);
        assert!(errors.is_empty()); // Should be warnings, not errors
    }
}

mod parse_set_info {
    use super::*;

    #[test]
    fn parses_full_set_info() {
        let processor = CsvProcessor::new();
        let (set_name, collector_num, rarity, condition, language) =
            processor.parse_set_info("Magic Foundations - 40 - Rare - NM - English");

        assert_eq!(set_name, "Magic Foundations");
        assert_eq!(collector_num, "40");
        assert_eq!(rarity, "Rare");
        assert_eq!(condition, "NM");
        assert_eq!(language, "English");
    }

    #[test]
    fn handles_partial_set_info() {
        let processor = CsvProcessor::new();
        let (set_name, collector_num, rarity, condition, language) =
            processor.parse_set_info("Magic Foundations - 40");

        assert_eq!(set_name, "Magic Foundations");
        assert_eq!(collector_num, "40");
        assert_eq!(rarity, "");
        assert_eq!(condition, "NM"); // Default
        assert_eq!(language, "English"); // Default
    }

    #[test]
    fn handles_set_name_only() {
        let processor = CsvProcessor::new();
        let (set_name, collector_num, rarity, condition, language) =
            processor.parse_set_info("Magic Foundations");

        assert_eq!(set_name, "Magic Foundations");
        assert_eq!(collector_num, "");
        assert_eq!(rarity, "");
        assert_eq!(condition, "NM");
        assert_eq!(language, "English");
    }

    #[test]
    fn handles_empty_string() {
        let processor = CsvProcessor::new();
        let (set_name, _, _, condition, language) = processor.parse_set_info("");

        assert_eq!(set_name, "");
        assert_eq!(condition, "NM");
        assert_eq!(language, "English");
    }
}
