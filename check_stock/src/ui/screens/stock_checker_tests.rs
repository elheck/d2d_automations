//! Unit tests for stock_checker business logic functions.

use super::*;
use crate::models::Card;

fn make_card(name: &str, price: &str) -> Card {
    Card {
        name: name.to_string(),
        price: price.to_string(),
        ..Card::test_default()
    }
}

/// Build a single all_matches group: (name, needed_qty, [(card, qty, set_name)])
fn make_group(
    name: &str,
    needed: i32,
    cards: Vec<(Card, i32, &str)>,
) -> (String, i32, Vec<(Card, i32, String)>) {
    (
        name.to_string(),
        needed,
        cards
            .into_iter()
            .map(|(c, q, s)| (c, q, s.to_string()))
            .collect(),
    )
}

// ============================================================================
// total_card_count
// ============================================================================

mod total_card_count_tests {
    use super::*;

    #[test]
    fn empty_matches() {
        assert_eq!(total_card_count(&[]), 0);
    }

    #[test]
    fn single_group_single_card() {
        let card = make_card("Bolt", "1.00");
        let matches = vec![make_group("Bolt", 1, vec![(card, 1, "Alpha")])];
        assert_eq!(total_card_count(&matches), 1);
    }

    #[test]
    fn multiple_groups_multiple_cards() {
        let c1 = make_card("Bolt", "1.00");
        let c2 = make_card("Bolt", "0.50");
        let c3 = make_card("Counterspell", "2.00");
        let matches = vec![
            make_group("Bolt", 2, vec![(c1, 1, "Alpha"), (c2, 1, "Beta")]),
            make_group("Counterspell", 1, vec![(c3, 1, "Ice Age")]),
        ];
        assert_eq!(total_card_count(&matches), 3);
    }

    #[test]
    fn group_with_no_cards() {
        let matches = vec![make_group("Missing Card", 1, vec![])];
        assert_eq!(total_card_count(&matches), 0);
    }
}

// ============================================================================
// all_as_matched_cards
// ============================================================================

mod all_as_matched_cards_tests {
    use super::*;

    #[test]
    fn empty_input() {
        let result = all_as_matched_cards(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn single_card_preserved() {
        let card = make_card("Sol Ring", "2.00");
        let matches = vec![make_group("Sol Ring", 1, vec![(card, 3, "Commander")])];

        let result = all_as_matched_cards(&matches);

        assert_eq!(result.len(), 1);
        let (name, needed, cards) = &result[0];
        assert_eq!(name, "Sol Ring");
        assert_eq!(*needed, 1);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].card.name, "Sol Ring");
        assert_eq!(cards[0].quantity, 3);
        assert_eq!(cards[0].set_name, "Commander");
    }

    #[test]
    fn multiple_groups_all_preserved() {
        let c1 = make_card("Bolt", "1.00");
        let c2 = make_card("Counterspell", "2.00");
        let matches = vec![
            make_group("Bolt", 4, vec![(c1, 4, "Alpha")]),
            make_group("Counterspell", 2, vec![(c2, 2, "Ice Age")]),
        ];

        let result = all_as_matched_cards(&matches);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "Bolt");
        assert_eq!(result[1].0, "Counterspell");
    }

    #[test]
    fn group_with_multiple_printings() {
        let c1 = make_card("Bolt", "50.00");
        let c2 = make_card("Bolt", "1.00");
        let matches = vec![make_group(
            "Bolt",
            2,
            vec![(c1, 1, "Alpha"), (c2, 1, "M10")],
        )];

        let result = all_as_matched_cards(&matches);

        assert_eq!(result[0].2.len(), 2);
        assert_eq!(result[0].2[0].set_name, "Alpha");
        assert_eq!(result[0].2[1].set_name, "M10");
    }
}

// ============================================================================
// collect_selected_matched_cards
// ============================================================================

mod collect_selected_matched_cards_tests {
    use super::*;

    #[test]
    fn empty_input() {
        let result = collect_selected_matched_cards(&[], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn all_selected_returns_all() {
        let c1 = make_card("Bolt", "1.00");
        let c2 = make_card("Counterspell", "2.00");
        let matches = vec![
            make_group("Bolt", 1, vec![(c1, 1, "Alpha")]),
            make_group("Counterspell", 1, vec![(c2, 1, "Ice Age")]),
        ];
        let selected = vec![true, true];

        let result = collect_selected_matched_cards(&matches, &selected);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "Bolt");
        assert_eq!(result[1].0, "Counterspell");
    }

    #[test]
    fn none_selected_returns_empty() {
        let c1 = make_card("Bolt", "1.00");
        let c2 = make_card("Counterspell", "2.00");
        let matches = vec![
            make_group("Bolt", 1, vec![(c1, 1, "Alpha")]),
            make_group("Counterspell", 1, vec![(c2, 1, "Ice Age")]),
        ];
        let selected = vec![false, false];

        let result = collect_selected_matched_cards(&matches, &selected);

        assert!(result.is_empty());
    }

    #[test]
    fn partial_selection_across_groups() {
        let c1 = make_card("Bolt", "1.00");
        let c2 = make_card("Counterspell", "2.00");
        let c3 = make_card("Dark Ritual", "3.00");
        let matches = vec![
            make_group("Bolt", 1, vec![(c1, 1, "Alpha")]),
            make_group("Counterspell", 1, vec![(c2, 1, "Ice Age")]),
            make_group("Dark Ritual", 1, vec![(c3, 1, "Alpha")]),
        ];
        let selected = vec![true, false, true];

        let result = collect_selected_matched_cards(&matches, &selected);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, "Bolt");
        assert_eq!(result[1].0, "Dark Ritual");
    }

    #[test]
    fn partial_selection_within_group() {
        // One wanted card fulfilled by two printings — user selects only one
        let c1 = make_card("Bolt", "50.00");
        let c2 = make_card("Bolt", "1.00");
        let matches = vec![make_group(
            "Bolt",
            2,
            vec![(c1, 1, "Alpha"), (c2, 1, "M10")],
        )];
        let selected = vec![false, true]; // only M10 selected

        let result = collect_selected_matched_cards(&matches, &selected);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].2.len(), 1);
        assert_eq!(result[0].2[0].set_name, "M10");
    }

    #[test]
    fn group_entirely_unselected_is_omitted() {
        let c1 = make_card("Bolt", "1.00");
        let c2 = make_card("Counterspell", "2.00");
        let matches = vec![
            make_group("Bolt", 1, vec![(c1, 1, "Alpha")]),
            make_group("Counterspell", 1, vec![(c2, 1, "Ice Age")]),
        ];
        let selected = vec![false, true];

        let result = collect_selected_matched_cards(&matches, &selected);

        // Bolt group is omitted because its only card was unselected
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "Counterspell");
    }

    #[test]
    fn index_is_flat_across_groups() {
        // Groups have 2 and 1 cards respectively — indices are 0, 1, 2
        let c1 = make_card("Bolt", "1.00");
        let c2 = make_card("Bolt", "0.50");
        let c3 = make_card("Counterspell", "2.00");
        let matches = vec![
            make_group("Bolt", 2, vec![(c1, 1, "Alpha"), (c2, 1, "Beta")]),
            make_group("Counterspell", 1, vec![(c3, 1, "Ice Age")]),
        ];
        let selected = vec![false, true, true]; // skip Alpha Bolt, keep Beta Bolt + Counterspell

        let result = collect_selected_matched_cards(&matches, &selected);

        assert_eq!(result.len(), 2);
        // Bolt group kept but only the Beta printing
        assert_eq!(result[0].2.len(), 1);
        assert_eq!(result[0].2[0].set_name, "Beta");
        // Counterspell group fully kept
        assert_eq!(result[1].2.len(), 1);
    }
}

// ============================================================================
// perform_stock_check
// ============================================================================

mod perform_stock_check_tests {
    use super::*;
    use crate::models::WantsEntry;

    fn make_inventory_card(name: &str, quantity: i32, name_de: &str) -> Card {
        Card {
            name: name.to_string(),
            quantity: quantity.to_string(),
            name_de: name_de.to_string(),
            ..Card::test_default()
        }
    }

    fn wants(name: &str, qty: i32) -> WantsEntry {
        WantsEntry {
            name: name.to_string(),
            quantity: qty,
        }
    }

    #[test]
    fn empty_wantslist_returns_empty() {
        let inventory = vec![make_inventory_card("Bolt", 4, "")];
        let result = perform_stock_check(&inventory, &[], Language::English, false);

        assert!(result.all_matches.is_empty());
        assert_eq!(result.total_found, 0);
        assert_eq!(result.total_wanted, 0);
        assert!(result.missing_cards.is_empty());
    }

    #[test]
    fn card_fully_in_stock() {
        let inventory = vec![make_inventory_card("Lightning Bolt", 4, "")];
        let wantslist = vec![wants("Lightning Bolt", 4)];

        let result = perform_stock_check(&inventory, &wantslist, Language::English, false);

        assert_eq!(result.total_wanted, 4);
        assert_eq!(result.total_found, 4);
        assert!(result.missing_cards.is_empty());
        assert_eq!(result.all_matches.len(), 1);
    }

    #[test]
    fn card_partially_in_stock_reported_as_missing() {
        let inventory = vec![make_inventory_card("Lightning Bolt", 2, "")];
        let wantslist = vec![wants("Lightning Bolt", 4)];

        let result = perform_stock_check(&inventory, &wantslist, Language::English, false);

        assert_eq!(result.total_wanted, 4);
        assert_eq!(result.total_found, 2);
        assert_eq!(result.missing_cards.len(), 1);
        assert_eq!(result.missing_cards[0], ("Lightning Bolt".to_string(), 2));
    }

    #[test]
    fn card_not_in_stock_at_all() {
        let inventory = vec![make_inventory_card("Counterspell", 4, "")];
        let wantslist = vec![wants("Lightning Bolt", 2)];

        let result = perform_stock_check(&inventory, &wantslist, Language::English, false);

        assert_eq!(result.total_found, 0);
        assert_eq!(result.missing_cards.len(), 1);
        assert_eq!(result.missing_cards[0].1, 2); // fully missing
    }

    #[test]
    fn totals_accumulate_across_multiple_wants() {
        let inventory = vec![
            make_inventory_card("Lightning Bolt", 4, ""),
            make_inventory_card("Counterspell", 2, ""),
        ];
        let wantslist = vec![wants("Lightning Bolt", 4), wants("Counterspell", 4)];

        let result = perform_stock_check(&inventory, &wantslist, Language::English, false);

        assert_eq!(result.total_wanted, 8);
        assert_eq!(result.total_found, 6); // 4 + 2
        assert_eq!(result.missing_cards.len(), 1); // only Counterspell missing
        assert_eq!(result.all_matches.len(), 2);
    }

    #[test]
    fn all_matches_entry_structure() {
        let inventory = vec![make_inventory_card("Sol Ring", 3, "")];
        let wantslist = vec![wants("Sol Ring", 2)];

        let result = perform_stock_check(&inventory, &wantslist, Language::English, false);

        let (name, needed_qty, cards) = &result.all_matches[0];
        assert_eq!(name, "Sol Ring");
        assert_eq!(*needed_qty, 2);
        assert!(!cards.is_empty());
    }
}
