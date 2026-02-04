//! Unit tests for order CSV parsing.

use super::*;

mod parse_order_items_tests {
    use super::*;

    #[test]
    fn parses_single_item() {
        let items = parse_order_items("1x Card Name - 1,87 EUR", "12345", "Card Name").unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].product_id, "12345");
        assert_eq!(items[0].localized_product_name, "Card Name");
        assert_eq!(items[0].quantity, 1);
        assert!((items[0].price - 1.87).abs() < 0.001);
    }

    #[test]
    fn parses_multiple_items() {
        let items = parse_order_items(
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
        // When counts don't match, treat as single item
        let items = parse_order_items(
            "1x Card One - 1,50 EUR | 2x Card Two - 3,00 EUR",
            "111", // Only one ID
            "Card One | Card Two",
        )
        .unwrap();

        assert_eq!(items.len(), 1);
    }

    #[test]
    fn handles_pipe_in_card_name() {
        // Card names can contain " | " (e.g., "Magic: The Gathering | Marvel's Spider-Man")
        // The parser should use product ID count as authoritative
        let items = parse_order_items(
            "1x Moss Diamond - 0,02 EUR | 1x Robot Token (Magic: The Gathering | Marvel's Spider-Man) - 0,02 EUR",
            "512140 | 848235",
            "Moss Diamond | Robot Token",
        )
        .unwrap();

        assert_eq!(items.len(), 2);
        assert_eq!(items[0].product_id, "512140");
        assert_eq!(items[0].localized_product_name, "Moss Diamond");
        assert!((items[0].price - 0.02).abs() < 0.001);

        assert_eq!(items[1].product_id, "848235");
        assert_eq!(items[1].localized_product_name, "Robot Token");
        assert!(items[1].description.contains("Marvel's Spider-Man"));
        assert!((items[1].price - 0.02).abs() < 0.001);
    }

    #[test]
    fn handles_real_cardmarket_format() {
        // Real Cardmarket format: "1x Card (Set) - CollectorNum - Rarity - Condition - Language - Price EUR"
        let items = parse_order_items(
            "1x Moss Diamond (Commander Legends) - 327 - Common - NM - English - 0,02 EUR | 2x Gift of Paradise (Commander Legends) - 229 - Common - NM - English - 0,04 EUR",
            "512140 | 510645",
            "Moss Diamond | Gift of Paradise",
        )
        .unwrap();

        assert_eq!(items.len(), 2);

        assert_eq!(items[0].quantity, 1);
        assert!((items[0].price - 0.02).abs() < 0.001);

        assert_eq!(items[1].quantity, 2);
        assert!((items[1].price - 0.04).abs() < 0.001);
    }
}

mod split_descriptions_by_count_tests {
    use super::*;

    #[test]
    fn simple_split_matches_count() {
        let result = split_descriptions_by_count("1x Card A - 1,00 EUR | 1x Card B - 2,00 EUR", 2);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "1x Card A - 1,00 EUR");
        assert_eq!(result[1], "1x Card B - 2,00 EUR");
    }

    #[test]
    fn handles_embedded_pipe_in_card_name() {
        let desc = "1x Card A - 1,00 EUR | 1x Token (Set | Subset) - 2,00 EUR";
        let result = split_descriptions_by_count(desc, 2);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "1x Card A - 1,00 EUR");
        assert!(result[1].contains("Set | Subset"));
    }

    #[test]
    fn returns_single_for_count_one() {
        let result = split_descriptions_by_count("1x Card - 1,00 EUR", 1);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "1x Card - 1,00 EUR");
    }

    #[test]
    fn handles_multiple_pipes_in_description() {
        // Two items, but description has 3 pipe separators due to embedded pipe
        let desc = "1x Card A - 1,00 EUR | 1x Token (A | B | C) - 2,00 EUR";
        let result = split_descriptions_by_count(desc, 2);
        assert_eq!(result.len(), 2);
        assert!(result[1].contains("A | B | C"));
    }
}

mod parse_order_line_tests {
    use super::*;

    #[test]
    fn parses_valid_order_line() {
        let line = "1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

        let order = parse_order_line(line).unwrap();

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
        let line = "1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

        let order = parse_order_line(line).unwrap();

        assert!(order.is_professional.is_none());
        assert!(order.vat_number.is_none());
    }

    #[test]
    fn parses_order_with_professional_flag() {
        let line = "1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;yes;DE123456789;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

        let order = parse_order_line(line).unwrap();

        assert_eq!(order.is_professional, Some("yes".to_string()));
        assert_eq!(order.vat_number, Some("DE123456789".to_string()));
    }

    #[test]
    fn fails_with_insufficient_columns() {
        let line = "1234567;user123;John Doe";

        let result = parse_order_line(line);
        assert!(result.is_err());
    }

    #[test]
    fn fails_with_invalid_article_count() {
        let line = "1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;not_a_number;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

        let result = parse_order_line(line);
        assert!(result.is_err());
    }
}

mod parse_csv_with_headers_tests {
    use super::*;

    #[test]
    fn parses_csv_with_headers() {
        let content = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName\n\
                      1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name";

        let orders = parse_csv_with_headers(content).unwrap();

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].order_id, "1234567");
        assert_eq!(orders[0].name, "John Doe");
    }

    #[test]
    fn parses_multiple_orders() {
        let content = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName\n\
                      1234567;user1;John Doe;Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card One\n\
                      1234568;user2;Jane Doe;Street 2;20095 Hamburg;Germany;;;2025-01-16;2;10,00;1,50;11,50;0,20;EUR;2x Card - 5,00 EUR;98766;Card Two";

        let orders = parse_csv_with_headers(content).unwrap();

        assert_eq!(orders.len(), 2);
        assert_eq!(orders[0].name, "John Doe");
        assert_eq!(orders[1].name, "Jane Doe");
    }

    #[test]
    fn returns_empty_for_empty_content() {
        // Empty content has no header, so parse_csv_with_headers won't be called
        // But if it is, it should return empty
        let orders = parse_csv_with_headers("").unwrap();
        assert!(orders.is_empty());
    }

    #[test]
    fn returns_empty_for_header_only() {
        let content = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName";

        let orders = parse_csv_with_headers(content).unwrap();
        assert!(orders.is_empty());
    }

    #[test]
    fn skips_empty_lines() {
        let content = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName\n\
                      \n\
                      1234567;user123;John Doe;Main Street 1;10557 Berlin;Germany;;;2025-01-15;1;5,00;1,50;6,50;0,10;EUR;1x Card - 5,00 EUR;98765;Card Name\n\
                      ";

        let orders = parse_csv_with_headers(content).unwrap();
        assert_eq!(orders.len(), 1);
    }
}
