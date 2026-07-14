use super::*;
use crate::bin_consolidation::Move;
use crate::models::Card;

fn mv(id: &str, from: &str, to: &str, qty: i64) -> Move {
    let card = Card {
        cardmarket_id: id.to_string(),
        set_code: "TST".to_string(),
        cn: "42".to_string(),
        ..Card::test_default()
    };
    Move {
        card,
        quantity: qty,
        from_location: format!("{from}-L0-R"),
        to_location: format!("{to}-L0-R"),
        from_bin: from.to_string(),
        to_bin: to.to_string(),
        distance: 0,
    }
}

#[test]
fn from_move_maps_fields() {
    let item = ConsolidationItem::from_move(&mv("1", "A-0-0-1", "B-0-0-1", 5));
    assert_eq!(item.card_name, "Test Card");
    assert_eq!(item.set_code, "TST");
    assert_eq!(item.collector_number, "42");
    assert_eq!(item.quantity, 5);
    assert_eq!(item.from_bin, "A-0-0-1");
    assert_eq!(item.to_bin, "B-0-0-1");
    assert_eq!(item.to_location, "B-0-0-1-L0-R");
    assert!(!item.done);
}

#[test]
fn from_moves_groups_and_sorts_by_source_bin() {
    let moves = vec![
        mv("1", "B-0-0-1", "X-0-0-1", 1),
        mv("2", "A-0-0-1", "Y-0-0-1", 1),
        mv("3", "A-0-0-1", "X-0-0-1", 1),
    ];
    let state = ConsolidationState::from_moves(&moves);
    let bins: Vec<&str> = state.items.iter().map(|i| i.from_bin.as_str()).collect();
    // Grouped: both A-0-0-1 first (contiguous), then B-0-0-1.
    assert_eq!(bins, vec!["A-0-0-1", "A-0-0-1", "B-0-0-1"]);
    // Within A-0-0-1, sorted by destination bin (X before Y).
    assert_eq!(state.items[0].to_bin, "X-0-0-1");
    assert_eq!(state.items[1].to_bin, "Y-0-0-1");
}

#[test]
fn done_counts_track_toggles() {
    let mut state = ConsolidationState::from_moves(&[
        mv("1", "A-0-0-1", "B-0-0-1", 1),
        mv("2", "A-0-0-1", "B-0-0-1", 1),
    ]);
    assert_eq!(state.total_count(), 2);
    assert_eq!(state.done_count(), 0);
    state.items[0].done = true;
    assert_eq!(state.done_count(), 1);
}

#[test]
fn empty_moves_yields_empty_state() {
    let state = ConsolidationState::from_moves(&[]);
    assert_eq!(state.total_count(), 0);
}

#[test]
fn moved_moves_returns_only_completed_items() {
    let mut state = ConsolidationState::from_moves(&[
        mv("1", "A-0-0-1", "B-0-0-1", 4),
        mv("2", "A-0-0-1", "B-0-0-1", 2),
        mv("3", "A-0-0-1", "C-0-0-1", 1),
    ]);
    // Nothing moved yet.
    assert!(state.moved_moves().is_empty());

    // Mark two of the three as moved.
    state.items[0].done = true;
    state.items[2].done = true;
    let moved = state.moved_moves();
    assert_eq!(moved.len(), 2);
    // The underlying moves carry the real card ids and destination locations.
    let ids: Vec<&str> = moved
        .iter()
        .map(|m| m.card.cardmarket_id.as_str())
        .collect();
    assert!(ids.contains(&"1"));
    assert!(ids.contains(&"3"));
    assert!(!ids.contains(&"2"));
}
