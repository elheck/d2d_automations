//! Unit tests for field parsing utilities.

use super::*;

mod parse_price_tests {
    use super::*;

    #[test]
    fn parses_comma_decimal() {
        let price = parse_price("1,87").unwrap();
        assert!((price - 1.87).abs() < 0.001);
    }

    #[test]
    fn parses_dot_decimal() {
        let price = parse_price("1.87").unwrap();
        assert!((price - 1.87).abs() < 0.001);
    }

    #[test]
    fn parses_integer() {
        let price = parse_price("100").unwrap();
        assert!((price - 100.0).abs() < 0.001);
    }

    #[test]
    fn parses_zero() {
        let price = parse_price("0").unwrap();
        assert!((price - 0.0).abs() < 0.001);
    }

    #[test]
    fn fails_for_invalid_input() {
        let result = parse_price("not a number");
        assert!(result.is_err());
    }

    #[test]
    fn fails_for_empty_string() {
        let result = parse_price("");
        assert!(result.is_err());
    }
}

mod parse_city_field_tests {
    use super::*;

    #[test]
    fn parses_german_city_with_postal_code() {
        let (zip, city) = parse_city_field("10557 Berlin").unwrap();
        assert_eq!(zip, "10557");
        assert_eq!(city, "Berlin");
    }

    #[test]
    fn parses_city_with_spaces_in_name() {
        let (zip, city) = parse_city_field("12345 Bad Neustadt").unwrap();
        assert_eq!(zip, "12345");
        assert_eq!(city, "Bad Neustadt");
    }

    #[test]
    fn parses_international_postal_code() {
        let (zip, city) = parse_city_field("SW1A London").unwrap();
        assert_eq!(zip, "SW1A");
        assert_eq!(city, "London");
    }

    #[test]
    fn handles_city_without_postal_code() {
        let (zip, city) = parse_city_field("Berlin").unwrap();
        assert_eq!(zip, "");
        assert_eq!(city, "Berlin");
    }

    #[test]
    fn handles_empty_string() {
        let (zip, city) = parse_city_field("").unwrap();
        assert_eq!(zip, "");
        assert_eq!(city, "");
    }

    #[test]
    fn trims_internal_whitespace() {
        let (zip, city) = parse_city_field("10557   Berlin").unwrap();
        assert_eq!(zip, "10557");
        assert_eq!(city, "Berlin");
    }
}

mod extract_quantity_tests {
    use super::*;

    #[test]
    fn extracts_single_digit_quantity() {
        let quantity = extract_quantity_from_description("2x High Fae Trickster");
        assert_eq!(quantity, 2);
    }

    #[test]
    fn extracts_double_digit_quantity() {
        let quantity = extract_quantity_from_description("10x Some Card Name");
        assert_eq!(quantity, 10);
    }

    #[test]
    fn defaults_to_one_without_quantity() {
        let quantity = extract_quantity_from_description("High Fae Trickster - 1,87 EUR");
        assert_eq!(quantity, 1);
    }

    #[test]
    fn defaults_to_one_for_empty_string() {
        let quantity = extract_quantity_from_description("");
        assert_eq!(quantity, 1);
    }

    #[test]
    fn handles_quantity_with_full_description() {
        let quantity = extract_quantity_from_description(
            "1x High Fae Trickster (Magic: The Gathering Foundations) - 40 - Rare - NM - English - 1,87 EUR",
        );
        assert_eq!(quantity, 1);
    }
}

mod extract_price_tests {
    use super::*;

    #[test]
    fn extracts_price_with_comma_decimal() {
        let price = extract_price_from_description("Some Card - 1,87 EUR").unwrap();
        assert!((price - 1.87).abs() < 0.001);
    }

    #[test]
    fn extracts_price_with_dot_decimal() {
        let price = extract_price_from_description("Some Card - 5.35 EUR").unwrap();
        assert!((price - 5.35).abs() < 0.001);
    }

    #[test]
    fn extracts_price_from_full_description() {
        let price = extract_price_from_description(
            "1x High Fae Trickster (Magic: The Gathering Foundations) - 40 - Rare - NM - English - 1,87 EUR",
        )
        .unwrap();
        assert!((price - 1.87).abs() < 0.001);
    }

    #[test]
    fn extracts_larger_price() {
        let price = extract_price_from_description("Expensive Card - 125,99 EUR").unwrap();
        assert!((price - 125.99).abs() < 0.001);
    }

    #[test]
    fn fails_without_eur_marker() {
        let result = extract_price_from_description("Some Card - 1,87");
        assert!(result.is_err());
    }

    #[test]
    fn fails_for_empty_string() {
        let result = extract_price_from_description("");
        assert!(result.is_err());
    }
}
