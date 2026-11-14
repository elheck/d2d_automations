use super::*;
use crate::models::Card;

/// Builds an in-stock card of variant `id` at `location` with `qty` copies.
/// All other variant-defining fields are left at their test defaults (NM /
/// English / non-foil), so distinct ids mean distinct variants.
fn card_at(id: &str, location: &str, qty: i64) -> Card {
    Card {
        cardmarket_id: id.to_string(),
        quantity: qty.to_string(),
        location: Some(location.to_string()),
        ..Card::test_default()
    }
}

fn to_bins(plan: &ConsolidationPlan) -> Vec<(&str, &str)> {
    plan.moves
        .iter()
        .map(|m| (m.from_bin.as_str(), m.to_bin.as_str()))
        .collect()
}

// ==================== nothing to do ====================

#[test]
fn no_sparse_bins_yields_empty_plan() {
    // Both bins well above the threshold.
    let cards = vec![card_at("1", "A-0-0-1", 40), card_at("2", "B-0-0-1", 40)];
    let plan = plan_consolidation(&cards, 20);
    assert!(plan.moves.is_empty());
    assert!(plan.bins_freed.is_empty());
    assert_eq!(plan.cards_moved, 0);
}

#[test]
fn sparse_bin_with_no_room_anywhere_is_not_emptied() {
    // Only keeper is almost full: 58 + 5 > 60, so nothing can move.
    let cards = vec![card_at("1", "B-0-0-1", 58), card_at("2", "A-0-0-1", 5)];
    let plan = plan_consolidation(&cards, 20);
    assert!(plan.moves.is_empty());
    assert!(plan.bins_freed.is_empty());
}

// ==================== basic consolidation ====================

#[test]
fn sparse_bin_emptied_into_keeper() {
    let cards = vec![card_at("1", "B-0-0-1", 40), card_at("2", "A-0-0-1", 5)];
    let plan = plan_consolidation(&cards, 20);

    assert_eq!(plan.moves.len(), 1);
    assert_eq!(to_bins(&plan), vec![("A-0-0-1", "B-0-0-1")]);
    assert_eq!(plan.bins_freed, vec!["A-0-0-1"]);
    assert_eq!(plan.cards_moved, 5);
    assert_eq!(plan.moves[0].quantity, 5);
}

#[test]
fn lot_and_side_suffix_is_preserved_in_new_location() {
    let cards = vec![
        card_at("1", "B-0-0-1-L0-R", 40),
        card_at("2", "A-0-0-9-L4-R", 3),
    ];
    let plan = plan_consolidation(&cards, 20);

    assert_eq!(plan.moves.len(), 1);
    assert_eq!(plan.moves[0].from_location, "A-0-0-9-L4-R");
    // Bin coordinates change to the target; the lot/side suffix is kept.
    assert_eq!(plan.moves[0].to_location, "B-0-0-1-L4-R");
}

// ==================== target selection ====================

#[test]
fn prefers_keeper_already_holding_the_same_variant() {
    // Two equally-full keepers; C holds variant 7, A's card is variant 7.
    let cards = vec![
        card_at("1", "B-0-0-1", 40),
        card_at("7", "C-0-0-1", 40),
        card_at("7", "A-0-0-1", 5),
    ];
    let plan = plan_consolidation(&cards, 20);
    assert_eq!(to_bins(&plan), vec![("A-0-0-1", "C-0-0-1")]);
}

#[test]
fn without_variant_match_prefers_the_closer_bin() {
    // No shared variant, both keepers: B (aisle 2) is closer to source A than C
    // (aisle 3), so proximity chooses B.
    let cards = vec![
        card_at("1", "B-0-0-1", 40),
        card_at("2", "C-0-0-1", 40),
        card_at("3", "A-0-0-1", 5),
    ];
    let plan = plan_consolidation(&cards, 20);
    assert_eq!(to_bins(&plan), vec![("A-0-0-1", "B-0-0-1")]);
}

#[test]
fn proximity_chooses_closest_of_equally_good_keepers() {
    // Same aisle, adjacent column beats a different aisle.
    let cards = vec![
        card_at("1", "A-0-0-2", 40), // distance 1 from source
        card_at("2", "C-0-0-1", 40), // distance 2000 from source
        card_at("3", "A-0-0-1", 5),
    ];
    let plan = plan_consolidation(&cards, 20);
    assert_eq!(to_bins(&plan), vec![("A-0-0-1", "A-0-0-2")]);
    assert_eq!(plan.moves[0].distance, 1);
    assert_eq!(plan.total_move_distance, 1);
}

#[test]
fn keeper_is_preferred_over_a_closer_sparse_bin() {
    // A-0-0-2 is adjacent and sparse; C is a far keeper. We must not clog the
    // sparse bin we might want to free, so the far keeper wins.
    let cards = vec![
        card_at("1", "A-0-0-2", 10), // sparse, distance 1
        card_at("2", "C-0-0-1", 40), // keeper, distance 2000
        card_at("3", "A-0-0-1", 5),
    ];
    let plan = plan_consolidation(&cards, 20);
    let mv = plan.moves.iter().find(|m| m.from_bin == "A-0-0-1").unwrap();
    assert_eq!(mv.to_bin, "C-0-0-1");
}

#[test]
fn same_variant_outranks_proximity_among_keepers() {
    // Near keeper lacks the variant; far keeper holds it → de-fragment wins.
    let cards = vec![
        card_at("1", "A-0-0-2", 40), // near, different variant
        card_at("7", "C-0-0-1", 40), // far, same variant as source
        card_at("7", "A-0-0-1", 5),
    ];
    let plan = plan_consolidation(&cards, 20);
    assert_eq!(to_bins(&plan), vec![("A-0-0-1", "C-0-0-1")]);
}

#[test]
fn capacity_is_respected_when_choosing_target() {
    // B can't fit (55+8>60) but C can (40+8=48) → must pick C.
    let cards = vec![
        card_at("1", "B-0-0-1", 55),
        card_at("2", "C-0-0-1", 40),
        card_at("3", "A-0-0-1", 8),
    ];
    let plan = plan_consolidation(&cards, 20);
    assert_eq!(to_bins(&plan), vec![("A-0-0-1", "C-0-0-1")]);
}

// ==================== whole-bin / no double move ====================

#[test]
fn bin_only_emptied_when_all_piles_fit() {
    // A holds two piles (5 + 4 = 9). Only keeper B has room for 5 but then
    // 55+5=60 leaves no room for the 4 → the whole bin stays put.
    let cards = vec![
        card_at("1", "B-0-0-1", 55),
        card_at("2", "A-0-0-1", 5),
        card_at("3", "A-0-0-1", 4),
    ];
    let plan = plan_consolidation(&cards, 20);
    assert!(plan.moves.is_empty(), "partial emptying must be skipped");
    assert!(plan.bins_freed.is_empty());
}

#[test]
fn received_bin_is_not_emptied_again() {
    // High threshold makes both A and B sparse. A (emptier) merges into B; B has
    // now received cards and must not itself be emptied afterwards.
    let cards = vec![card_at("1", "A-0-0-1", 5), card_at("2", "B-0-0-1", 8)];
    let plan = plan_consolidation(&cards, 50);

    assert_eq!(plan.bins_freed, vec!["A-0-0-1"]);
    assert_eq!(to_bins(&plan), vec![("A-0-0-1", "B-0-0-1")]);
    // No move originates from B.
    assert!(plan.moves.iter().all(|m| m.from_bin != "B-0-0-1"));
}

// ==================== input hygiene & reporting ====================

#[test]
fn cards_without_valid_bin_location_are_ignored() {
    let cards = vec![
        card_at("1", "B-0-0-1", 40),
        Card {
            location: None,
            ..card_at("2", "unused", 3)
        },
        card_at("3", "A-0-1", 3),   // too few segments
        card_at("4", "A-0-0-1", 5), // valid sparse
    ];
    let plan = plan_consolidation(&cards, 20);
    // Only the valid sparse bin can move.
    assert_eq!(to_bins(&plan), vec![("A-0-0-1", "B-0-0-1")]);
    assert_eq!(plan.moves.len(), 1);
}

#[test]
fn fragmented_variants_are_counted() {
    // Variant 5 lives in two bins; variant 1 in one.
    let cards = vec![
        card_at("1", "B-0-0-1", 40),
        card_at("5", "B-0-0-1", 2),
        card_at("5", "C-0-0-1", 40),
    ];
    let plan = plan_consolidation(&cards, 20);
    assert_eq!(plan.fragmented_variants, 1);
}

#[test]
fn zero_quantity_rows_are_ignored() {
    let cards = vec![card_at("1", "B-0-0-1", 40), card_at("2", "A-0-0-1", 0)];
    let plan = plan_consolidation(&cards, 20);
    assert!(plan.moves.is_empty());
}

// ==================== CSV export ====================

#[test]
fn update_csv_has_header_new_location_and_positive_quantity() {
    let cards = vec![
        card_at("111", "B-0-0-1", 40),
        card_at("222", "A-0-0-9-L4-R", 3),
    ];
    let plan = plan_consolidation(&cards, 20);
    let csv = to_update_csv(&plan.moves);

    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 2, "header + one move");
    assert!(lines[0].starts_with("cardmarketId,quantity,name"));
    assert!(lines[1].contains("222"));
    assert!(
        lines[1].contains("B-0-0-1-L4-R"),
        "row carries the new location"
    );
    // Quantity is positive (relocation, not removal).
    assert!(lines[1].contains(",3,"));
}

#[test]
fn update_csv_empty_moves_is_header_only() {
    let csv = to_update_csv(&[]);
    assert_eq!(csv.lines().count(), 1);
}

// ==================== multi-strategy repack ====================

#[test]
fn multi_strategy_frees_more_than_naive_emptiest_first() {
    // Emptiest-first sends the small pile B into the near keeper, blocking the
    // large pile A which then fits nowhere (only 1 bin freed). Processing A first
    // frees both — plan_consolidation must discover the 2-bin plan.
    let cards = vec![
        card_at("100", "A-0-0-3", 50), // Knear: room 10, close to the sources
        card_at("200", "D-0-0-1", 54), // Kfar:  room 6, far away
        card_at("1", "A-0-0-1", 10),   // A: only fits a full 10-room keeper
        card_at("2", "A-0-0-2", 6),    // B: fits either keeper
    ];
    let plan = plan_consolidation(&cards, 20);
    assert_eq!(plan.bins_freed.len(), 2);
    assert!(plan.bins_freed.contains(&"A-0-0-1".to_string()));
    assert!(plan.bins_freed.contains(&"A-0-0-2".to_string()));
}

#[test]
fn plan_consolidation_is_deterministic() {
    let cards = vec![
        card_at("100", "A-0-0-3", 50),
        card_at("200", "D-0-0-1", 54),
        card_at("1", "A-0-0-1", 10),
        card_at("2", "A-0-0-2", 6),
        card_at("3", "B-0-0-1", 5),
    ];
    let p1 = plan_consolidation(&cards, 20);
    let p2 = plan_consolidation(&cards, 20);
    assert_eq!(to_bins(&p1), to_bins(&p2));
    assert_eq!(p1.bins_freed, p2.bins_freed);
    assert_eq!(p1.total_move_distance, p2.total_move_distance);
}

// ==================== fragmented-variant report ====================

#[test]
fn fragmented_variants_reports_only_split_variants() {
    let cards = vec![
        card_at("7", "A-0-0-1", 3),
        card_at("7", "B-0-0-1", 2),
        card_at("9", "A-0-0-1", 5), // single bin — not fragmented
    ];
    let frags = fragmented_variants(&cards);
    assert_eq!(frags.len(), 1);
    let f = &frags[0];
    assert_eq!(f.cardmarket_id, "7");
    assert_eq!(f.total_copies, 5);
    assert_eq!(f.bin_count(), 2);
    assert_eq!(f.placements.len(), 2);
}

#[test]
fn fragmented_variants_sorted_most_fragmented_first() {
    let cards = vec![
        card_at("1", "A-0-0-1", 1),
        card_at("1", "B-0-0-1", 1), // 2 bins
        card_at("2", "A-0-0-1", 1),
        card_at("2", "B-0-0-1", 1),
        card_at("2", "C-0-0-1", 1), // 3 bins
    ];
    let frags = fragmented_variants(&cards);
    assert_eq!(frags[0].cardmarket_id, "2"); // 3 bins ranks first
    assert_eq!(frags[1].cardmarket_id, "1");
}

// ==================== variant defrag plan ====================

#[test]
fn variant_defrag_gathers_into_bin_with_most_copies() {
    let cards = vec![card_at("7", "A-0-0-1", 3), card_at("7", "B-0-0-1", 2)];
    let plan = plan_variant_defrag(&cards);

    assert_eq!(plan.moves.len(), 1);
    assert_eq!(plan.moves[0].from_bin, "B-0-0-1");
    assert_eq!(plan.moves[0].to_bin, "A-0-0-1");
    assert_eq!(plan.moves[0].quantity, 2);
    assert_eq!(plan.bins_freed, vec!["B-0-0-1"]);
    assert_eq!(plan.fragmented_variants, 1);
}

#[test]
fn variant_defrag_independent_of_bin_fill() {
    // Both bins are nearly empty (a sparse-bin plan with a low threshold would
    // still act, but defrag runs regardless of any threshold).
    let cards = vec![card_at("7", "A-0-0-1", 1), card_at("7", "C-0-0-1", 1)];
    let plan = plan_variant_defrag(&cards);
    assert_eq!(plan.moves.len(), 1);
    assert_eq!(plan.cards_moved, 1);
}

#[test]
fn variant_defrag_skips_variant_that_cannot_fit() {
    // Variant 7 is split, but both its bins are packed with other cards, so
    // neither can absorb the other's pile.
    let cards = vec![
        card_at("7", "A-0-0-1", 3),
        card_at("98", "A-0-0-1", 56),
        card_at("7", "B-0-0-1", 2),
        card_at("99", "B-0-0-1", 57),
    ];
    let plan = plan_variant_defrag(&cards);
    assert!(plan.moves.is_empty());
    assert_eq!(plan.fragmented_variants, 1);
}

#[test]
fn variant_defrag_empty_when_nothing_fragmented() {
    let cards = vec![card_at("1", "A-0-0-1", 4), card_at("2", "B-0-0-1", 4)];
    let plan = plan_variant_defrag(&cards);
    assert!(plan.moves.is_empty());
    assert_eq!(plan.fragmented_variants, 0);
}
