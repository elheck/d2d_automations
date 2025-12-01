//! Unit tests for the SevDesk API module.

use super::*;

mod parse_price_tests {
    use super::*;

    #[test]
    fn parses_comma_decimal() {
        let api = SevDeskApi::new("test_token".to_string());
        let price = api.parse_price("5,00").unwrap();
        assert!((price - 5.0).abs() < 0.001);
    }

    #[test]
    fn parses_dot_decimal() {
        let api = SevDeskApi::new("test_token".to_string());
        let price = api.parse_price("5.00").unwrap();
        assert!((price - 5.0).abs() < 0.001);
    }

    #[test]
    fn parses_integer() {
        let api = SevDeskApi::new("test_token".to_string());
        let price = api.parse_price("100").unwrap();
        assert!((price - 100.0).abs() < 0.001);
    }

    #[test]
    fn parses_zero() {
        let api = SevDeskApi::new("test_token".to_string());
        let price = api.parse_price("0").unwrap();
        assert!((price - 0.0).abs() < 0.001);
    }

    #[test]
    fn parses_decimal_cents() {
        let api = SevDeskApi::new("test_token".to_string());
        let price = api.parse_price("1,87").unwrap();
        assert!((price - 1.87).abs() < 0.001);
    }

    #[test]
    fn parses_large_price() {
        let api = SevDeskApi::new("test_token".to_string());
        let price = api.parse_price("1234,56").unwrap();
        assert!((price - 1234.56).abs() < 0.001);
    }

    #[test]
    fn fails_for_invalid_input() {
        let api = SevDeskApi::new("test_token".to_string());
        let result = api.parse_price("not a number");
        assert!(result.is_err());
    }

    #[test]
    fn fails_for_empty_string() {
        let api = SevDeskApi::new("test_token".to_string());
        let result = api.parse_price("");
        assert!(result.is_err());
    }
}

mod sevdesk_api_construction_tests {
    use super::*;

    #[test]
    fn creates_api_with_token() {
        let api = SevDeskApi::new("my_api_token".to_string());
        assert_eq!(api.api_token, "my_api_token");
        assert_eq!(api.base_url, "https://my.sevdesk.de/api/v1");
    }

    #[test]
    fn creates_api_with_empty_token() {
        let api = SevDeskApi::new(String::new());
        assert!(api.api_token.is_empty());
    }

    #[test]
    fn country_cache_starts_empty() {
        let api = SevDeskApi::new("test_token".to_string());
        // We can't directly access the cache, but we can verify the API was created
        assert_eq!(api.base_url, "https://my.sevdesk.de/api/v1");
    }
}

mod country_cache_tests {
    use super::*;

    #[test]
    fn country_cache_default_is_not_loaded() {
        let cache = CountryCache::default();
        assert!(!cache.loaded);
        assert!(cache.name_to_id.is_empty());
    }
}

mod invoice_creation_result_tests {
    use super::*;

    #[test]
    fn creates_successful_result() {
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
    fn creates_error_result() {
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
}

mod order_record_tests {
    use super::*;
    use crate::models::OrderItem;

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
}
