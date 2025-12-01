//! Unit tests for card/inventory CSV parsing.

use super::*;

mod parse_set_info_tests {
    use super::*;

    #[test]
    fn parses_complete_set_info() {
        let (set_name, collector_number, rarity, condition, language) =
            parse_set_info("Modern Horizons 2 - 142 - Rare - NM - English");

        assert_eq!(set_name, "Modern Horizons 2");
        assert_eq!(collector_number, "142");
        assert_eq!(rarity, "Rare");
        assert_eq!(condition, "NM");
        assert_eq!(language, "English");
    }

    #[test]
    fn handles_partial_set_info() {
        let (set_name, collector_number, rarity, condition, language) =
            parse_set_info("Modern Horizons 2 - 142");

        assert_eq!(set_name, "Modern Horizons 2");
        assert_eq!(collector_number, "142");
        assert_eq!(rarity, "");
        assert_eq!(condition, "NM"); // Default
        assert_eq!(language, "English"); // Default
    }

    #[test]
    fn handles_set_name_only() {
        let (set_name, collector_number, rarity, condition, language) =
            parse_set_info("Modern Horizons 2");

        assert_eq!(set_name, "Modern Horizons 2");
        assert_eq!(collector_number, "");
        assert_eq!(rarity, "");
        assert_eq!(condition, "NM");
        assert_eq!(language, "English");
    }

    #[test]
    fn handles_empty_string() {
        let (set_name, collector_number, rarity, condition, language) = parse_set_info("");

        assert_eq!(set_name, "");
        assert_eq!(collector_number, "");
        assert_eq!(rarity, "");
        assert_eq!(condition, "NM");
        assert_eq!(language, "English");
    }
}

mod parse_card_line_tests {
    use super::*;

    #[test]
    fn parses_tab_separated_card_line() {
        let line = "Modern Horizons 2 - 142 - Rare\t5,00 EUR\t12345\tCard Name";

        let card = parse_card_line(line).unwrap();

        assert_eq!(card.product_id, "12345");
        assert_eq!(card.card_name, "Card Name");
        assert_eq!(card.price, "5,00 EUR");
        assert_eq!(card.currency, "EUR");
        assert_eq!(card.set_name, "Modern Horizons 2");
    }

    #[test]
    fn parses_usd_currency() {
        let line = "Modern Horizons 2 - 142 - Rare\t$5.00\t12345\tCard Name";

        let card = parse_card_line(line).unwrap();

        assert_eq!(card.currency, "USD");
    }

    #[test]
    fn fails_with_insufficient_parts() {
        let line = "Card Name\t5,00 EUR";

        let result = parse_card_line(line);
        assert!(result.is_err());
    }

    #[test]
    fn fails_without_price_marker() {
        let line = "Card Name\t12345\tSome Info";

        let result = parse_card_line(line);
        // No EUR or $ marker means we can't find the price
        assert!(result.is_err());
    }
}

mod card_to_order_tests {
    use super::*;

    #[test]
    fn converts_card_to_order() {
        let card = CardRecord {
            product_id: "12345".to_string(),
            card_name: "Test Card".to_string(),
            set_name: "Test Set".to_string(),
            collector_number: "001".to_string(),
            rarity: "Rare".to_string(),
            condition: "NM".to_string(),
            language: "English".to_string(),
            price: "5,00".to_string(),
            currency: "EUR".to_string(),
        };

        let order = card_to_order(card);

        assert_eq!(order.order_id, "12345");
        assert_eq!(order.name, "Card Customer");
        assert_eq!(order.username, "Card Inventory");
        assert_eq!(order.country, "DE");
        assert_eq!(order.article_count, 1);
        assert_eq!(order.currency, "EUR");
        assert_eq!(order.items.len(), 1);
        assert_eq!(order.items[0].product_id, "12345");
        assert!((order.items[0].price - 5.0).abs() < 0.001);
    }

    #[test]
    fn handles_unparseable_price() {
        let card = CardRecord {
            product_id: "12345".to_string(),
            card_name: "Test Card".to_string(),
            set_name: "Test Set".to_string(),
            collector_number: "001".to_string(),
            rarity: "Rare".to_string(),
            condition: "NM".to_string(),
            language: "English".to_string(),
            price: "invalid price".to_string(),
            currency: "EUR".to_string(),
        };

        let order = card_to_order(card);

        // Should default to 0.0 when price can't be parsed
        assert!((order.items[0].price - 0.0).abs() < 0.001);
    }

    #[test]
    fn creates_proper_description() {
        let card = CardRecord {
            product_id: "12345".to_string(),
            card_name: "Test Card".to_string(),
            set_name: "Test Set".to_string(),
            collector_number: "001".to_string(),
            rarity: "Rare".to_string(),
            condition: "NM".to_string(),
            language: "English".to_string(),
            price: "5,00".to_string(),
            currency: "EUR".to_string(),
        };

        let order = card_to_order(card);

        assert!(order.description.contains("Test Card"));
        assert!(order.description.contains("Test Set"));
        assert!(order.description.contains("NM"));
    }
}
