//! Tests for the bulk lot builder.

use super::*;
use crate::category::DEFAULT_CATEGORY as CAT;

/// A candidate with sensible defaults: common, NM English non-foil, 1 copy.
fn cand(source: usize, name: &str, price: f64, age: Option<i64>) -> Candidate {
    Candidate {
        source,
        variant: format!("v{source}"),
        name: name.to_string(),
        name_key: name_key(name),
        set_name: "Test Set".to_string(),
        set_code: "TST".to_string(),
        cn: source.to_string(),
        rarity: Rarity::Common,
        condition: "NM".to_string(),
        language: "English".to_string(),
        is_foil: false,
        price,
        age_days: age,
        available: 1,
        reserved: 0,
        location: format!("A-0-0-{source}-L1-R"),
        purchase_lot: "L1".to_string(),
        front_type: None,
    }
}

/// Oldest-first spec: most tests check the age/price rules, not location.
fn spec(size: usize) -> LotSpec {
    LotSpec {
        size,
        min_age_days: 0,
        order: LotOrder::OldestFirst,
        ..LotSpec::default()
    }
}

/// A candidate in bin `A-0-0-{bin}`.
fn in_bin(source: usize, name: &str, bin: u32) -> Candidate {
    Candidate {
        location: format!("A-0-0-{bin}-L1-R"),
        ..cand(source, name, 0.05, Some(300))
    }
}

fn nearby(size: usize) -> LotSpec {
    LotSpec {
        order: LotOrder::Nearby,
        ..spec(size)
    }
}

fn bins(cands: &[Candidate], picks: &[LotPick]) -> Vec<String> {
    let mut bins: Vec<String> = picks
        .iter()
        .map(|p| {
            let loc = &cands[p.candidate].location;
            loc.split('-').take(4).collect::<Vec<_>>().join("-")
        })
        .collect();
    bins.sort();
    bins.dedup();
    bins
}

fn names(cands: &[Candidate], picks: &[LotPick]) -> Vec<String> {
    picks
        .iter()
        .map(|p| cands[p.candidate].name.clone())
        .collect()
}

fn copies(picks: &[LotPick]) -> i64 {
    picks.iter().map(|p| p.copies).sum()
}

fn csv_card(qty: &str, price: &str, location: &str) -> Card {
    Card {
        quantity: qty.to_string(),
        price: price.to_string(),
        location: Some(location.to_string()),
        listed_at: String::new(),
        ..Card::test_default()
    }
}

// ── Parsing and type matching ────────────────────────────────────────────────

#[test]
fn rarity_parses_cardmarket_values() {
    assert_eq!(Rarity::parse("Common"), Rarity::Common);
    assert_eq!(Rarity::parse(" uncommon "), Rarity::Uncommon);
    assert_eq!(Rarity::parse("Rare"), Rarity::Rare);
    assert_eq!(Rarity::parse("Mythic"), Rarity::Mythic);
    assert_eq!(Rarity::parse("Land"), Rarity::Other, "basic lands");
    assert_eq!(Rarity::parse("Token"), Rarity::Other);
    assert_eq!(Rarity::parse(""), Rarity::Other);
}

#[test]
fn non_basic_land_type_matching() {
    let t = CardType::NonBasicLand;
    assert!(t.matches("Land"));
    assert!(t.matches("Land — Gate"));
    assert!(t.matches("Artifact Land"));
    assert!(t.matches("Snow Land"));
    assert!(!t.matches("Basic Land — Forest"));
    assert!(!t.matches("Basic Snow Land — Island"));
    assert!(!t.matches("Creature — Elf"));
}

#[test]
fn type_matching_ignores_subtypes() {
    assert!(CardType::Artifact.matches("Artifact — Equipment"));
    assert!(CardType::Creature.matches("Artifact Creature — Golem"));
    assert!(CardType::InstantSorcery.matches("Tribal Instant — Elf"));
    assert!(CardType::InstantSorcery.matches("Sorcery"));
    assert!(!CardType::Enchantment.matches("Creature — Enchantment"));
    assert!(CardType::NonLand.matches("Instant"));
    assert!(!CardType::NonLand.matches("Land — Desert"));
    assert!(CardType::Any.matches(""));
}

#[test]
fn basic_land_names() {
    for name in ["Plains", "island", "Snow-Covered Swamp", "Wastes", "Forest"] {
        assert!(is_basic_land_name(name), "{name}");
    }
    for name in ["Forest Bear", "Plains Walker", "Thriving Grove", ""] {
        assert!(!is_basic_land_name(name), "{name}");
    }
}

#[test]
fn name_key_ignores_case_and_spacing() {
    assert_eq!(name_key("  Llanowar   Elves "), name_key("llanowar elves"));
}

// ── Candidates from the CSV ──────────────────────────────────────────────────

#[test]
fn candidates_skip_rows_without_stock_or_price() {
    let cards = vec![
        csv_card("2", "0.10", "A-0-0-1-L1-R"),
        csv_card("0", "0.10", "A-0-0-2-L1-R"),
        csv_card("1", "", "A-0-0-3-L1-R"),
        csv_card("1", "n/a", "A-0-0-4-L1-R"),
        csv_card("1", "0,20", "A-0-0-5-L3-R"),
    ];
    let today = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
    let c = build_candidates(&cards, CAT, &HashMap::new(), &[], today);
    assert_eq!(c.len(), 2);
    assert_eq!((c[0].source, c[0].available), (0, 2));
    assert_eq!(c[1].source, 4);
    assert!((c[1].price - 0.20).abs() < 1e-9, "comma decimal");
    assert_eq!(c[1].purchase_lot, "L3");
}

#[test]
fn candidate_age_prefers_listed_at_then_the_database_date() {
    let mut listed = csv_card("1", "0.10", "A-0-0-1-L1-R");
    listed.listed_at = "2026-10-05 12:00:00".to_string();
    let mut unlisted = csv_card("1", "0.10", "A-0-0-2-L1-R");
    unlisted.cardmarket_id = "999".to_string();
    let mut unknown = csv_card("1", "0.10", "A-0-0-3-L1-R");
    unknown.cardmarket_id = "555".to_string();

    let dates = HashMap::from([(
        crate::inventory_db::card_variant_key(&unlisted, CAT),
        "2026-02-24".to_string(),
    )]);
    let today = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
    let c = build_candidates(&[listed, unlisted, unknown], CAT, &dates, &[], today);
    assert_eq!(c[0].age_days, Some(30));
    assert_eq!(c[1].age_days, Some(253));
    assert_eq!(c[2].age_days, None);
}

#[test]
fn candidates_read_foil_from_finish_type() {
    let mut foil = csv_card("1", "0.10", "A-0-0-1-L1-R");
    foil.is_foil = String::new();
    foil.finish_type = "Foil".to_string();
    let today = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
    let c = build_candidates(&[foil], CAT, &HashMap::new(), &[], today);
    assert!(c[0].is_foil);
}

#[test]
fn copies_promised_to_pending_lots_are_reserved_same_location_first() {
    let a = csv_card("3", "0.10", "A-0-0-1-L1-R");
    let b = csv_card("2", "0.10", "A-0-0-2-L2-R");
    let variant = crate::inventory_db::card_variant_key(&a, CAT);
    let pending = vec![PendingRemoval {
        id: 1,
        bulk_lot_id: 1,
        variant,
        purchase_lot: "L2".to_string(),
        location: "A-0-0-2-L2-R".to_string(),
        pending: 3,
    }];
    let today = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
    let c = build_candidates(&[a, b], CAT, &HashMap::new(), &pending, today);
    assert_eq!((c[1].available, c[1].reserved), (0, 2), "its own row first");
    assert_eq!(
        (c[0].available, c[0].reserved),
        (2, 1),
        "then the variant's other row"
    );
}

// ── Selection ────────────────────────────────────────────────────────────────

#[test]
fn oldest_cheap_cards_go_first_by_default() {
    let cands = vec![
        cand(0, "New", 0.05, Some(10)),
        cand(1, "Old pricey", 0.20, Some(300)),
        cand(2, "Old cheap", 0.05, Some(300)),
        cand(3, "Too expensive", 0.80, Some(400)),
        cand(4, "Unknown age", 0.01, None),
    ];
    let picks = build_lot(&cands, &spec(3));
    assert_eq!(names(&cands, &picks), ["Old cheap", "Old pricey", "New"]);
}

#[test]
fn cheapest_first_order() {
    let cands = vec![
        cand(0, "A", 0.20, Some(300)),
        cand(1, "B", 0.05, Some(10)),
        cand(2, "C", 0.10, Some(50)),
    ];
    let s = LotSpec {
        order: LotOrder::CheapestFirst,
        ..spec(2)
    };
    assert_eq!(names(&cands, &build_lot(&cands, &s)), ["B", "C"]);
}

#[test]
fn no_repeats_across_printings_by_default() {
    let mut cands = vec![
        cand(0, "Llanowar Elves", 0.05, Some(300)),
        cand(1, "llanowar elves", 0.05, Some(300)),
        cand(2, "Giant Growth", 0.05, Some(200)),
    ];
    cands[0].available = 4;
    let picks = build_lot(&cands, &spec(10));
    assert_eq!(copies(&picks), 2, "one Elves, one Giant Growth");
}

#[test]
fn repeat_limit_allows_several_copies_of_a_name() {
    let mut cands = vec![
        cand(0, "Elves", 0.05, Some(300)),
        cand(1, "Elves", 0.05, Some(300)),
    ];
    cands[0].available = 2;
    cands[1].available = 2;
    let s = LotSpec {
        max_copies_per_name: 3,
        ..spec(10)
    };
    let picks = build_lot(&cands, &s);
    assert_eq!(copies(&picks), 3);
    assert_eq!(picks[0].copies, 2, "both copies of the first row");
}

#[test]
fn never_takes_more_copies_than_are_free() {
    let mut cands = vec![cand(0, "Elves", 0.05, Some(300))];
    cands[0].available = 1;
    cands[0].reserved = 3;
    let s = LotSpec {
        max_copies_per_name: 10,
        ..spec(10)
    };
    assert_eq!(copies(&build_lot(&cands, &s)), 1);
}

#[test]
fn filters_rarity_condition_language_finish_and_age() {
    let mut cands: Vec<Candidate> = (0..7)
        .map(|i| cand(i, &format!("Card {i}"), 0.05, Some(200)))
        .collect();
    cands[1].rarity = Rarity::Uncommon;
    cands[2].condition = "PL".to_string();
    cands[3].language = "Japanese".to_string();
    cands[4].is_foil = true;
    cands[5].age_days = Some(20);
    cands[6].age_days = None;
    let s = LotSpec {
        excluded_conditions: BTreeSet::from(["PL".to_string()]),
        excluded_languages: BTreeSet::from(["Japanese".to_string()]),
        min_age_days: 90,
        ..spec(10)
    };
    assert_eq!(names(&cands, &build_lot(&cands, &s)), ["Card 0"]);
}

#[test]
fn foil_only_lot() {
    let mut cands = vec![cand(0, "A", 0.05, Some(9)), cand(1, "B", 0.05, Some(9))];
    cands[1].is_foil = true;
    let s = LotSpec {
        foil: FoilFilter::FoilOnly,
        ..spec(5)
    };
    assert_eq!(names(&cands, &build_lot(&cands, &s)), ["B"]);
}

#[test]
fn basic_lands_are_left_out_by_name_or_type() {
    let mut cands = vec![
        cand(0, "Forest", 0.01, Some(300)),
        cand(1, "Odd Basic", 0.01, Some(300)),
        cand(2, "Elves", 0.01, Some(300)),
    ];
    cands[1].front_type = Some("Basic Land — Plains".to_string());
    assert_eq!(names(&cands, &build_lot(&cands, &spec(5))), ["Elves"]);
}

#[test]
fn a_type_filter_needs_a_known_matching_type_line() {
    let mut cands = vec![
        cand(0, "Gate", 0.05, Some(300)),
        cand(1, "Unknown", 0.05, Some(300)),
        cand(2, "Elves", 0.05, Some(300)),
    ];
    cands[0].front_type = Some("Land — Gate".to_string());
    cands[2].front_type = Some("Creature — Elf".to_string());
    let s = LotSpec {
        card_type: CardType::NonBasicLand,
        ..spec(5)
    };
    assert_eq!(names(&cands, &build_lot(&cands, &s)), ["Gate"]);
}

#[test]
fn guaranteed_hits_are_the_cheapest_cards_above_the_threshold() {
    let cands = vec![
        cand(0, "Bulk A", 0.05, Some(300)),
        cand(1, "Bulk B", 0.05, Some(300)),
        cand(2, "Hit pricey", 2.00, Some(300)),
        cand(3, "Hit cheap", 0.60, Some(10)),
        cand(4, "Below hit", 0.40, Some(300)),
    ];
    let s = LotSpec {
        hits: 1,
        hit_min_price: 0.50,
        ..spec(3)
    };
    let picks = build_lot(&cands, &s);
    let mut got = names(&cands, &picks);
    assert_eq!(got[0], "Hit cheap");
    got[1..].sort();
    assert_eq!(got, ["Hit cheap", "Bulk A", "Bulk B"]);
    assert!(picks[0].hit && !picks[1].hit);
}

#[test]
fn hits_count_towards_the_size_and_the_name_limit() {
    let cands = vec![
        cand(0, "Elves", 0.05, Some(300)),
        cand(1, "Elves", 0.90, Some(300)),
        cand(2, "Growth", 0.05, Some(300)),
    ];
    let s = LotSpec {
        hits: 5,
        hit_min_price: 0.50,
        ..spec(2)
    };
    let picks = build_lot(&cands, &s);
    assert_eq!(copies(&picks), 2);
    assert_eq!(names(&cands, &picks), ["Elves", "Growth"]);
    assert!(picks[0].hit);
    assert_eq!(cands[picks[0].candidate].price, 0.90);
}

#[test]
fn a_short_pool_gives_a_smaller_lot() {
    let cands = vec![cand(0, "A", 0.05, Some(9)), cand(1, "B", 0.05, Some(9))];
    assert_eq!(copies(&build_lot(&cands, &spec(100))), 2);
}

#[test]
fn equally_good_cards_are_drawn_at_random_not_by_name() {
    // Regression: identical age and price fell back to name order, so lots
    // were all cards starting with "A" or "B".
    let cands: Vec<Candidate> = (0..26u8)
        .map(|i| {
            cand(
                i as usize,
                &format!("{} card", (b'A' + i) as char),
                0.05,
                Some(300),
            )
        })
        .collect();
    let alphabetical: Vec<String> = cands[..5].iter().map(|c| c.name.clone()).collect();
    let lot = |seed| {
        let mut n = names(&cands, &build_lot(&cands, &LotSpec { seed, ..spec(5) }));
        n.sort();
        n
    };
    assert_ne!(lot(1), alphabetical);
    assert_eq!(lot(1), lot(1), "same seed, same lot");
    assert_ne!(lot(1), lot(2), "a new seed draws other cards");
}

#[test]
fn closest_together_is_the_default() {
    assert_eq!(LotSpec::default().order, LotOrder::Nearby);
}

#[test]
fn closest_together_takes_the_most_compact_run_of_bins() {
    let cands = vec![
        in_bin(0, "A", 1),
        in_bin(1, "B", 9),
        in_bin(2, "C", 20),
        in_bin(3, "D", 21),
        in_bin(4, "E", 21),
        in_bin(5, "F", 40),
    ];
    let picks = build_lot(&cands, &nearby(3));
    assert_eq!(bins(&cands, &picks), ["A-0-0-20", "A-0-0-21"]);
    // Picked in shelf order.
    assert_eq!(names(&cands, &picks)[0], "C");
}

#[test]
fn a_single_full_bin_beats_neighbours() {
    let cands = vec![
        in_bin(0, "A", 1),
        in_bin(1, "B", 2),
        in_bin(2, "C", 3),
        in_bin(3, "D", 30),
        in_bin(4, "E", 30),
        in_bin(5, "F", 30),
    ];
    assert_eq!(bins(&cands, &build_lot(&cands, &nearby(3))), ["A-0-0-30"]);
}

#[test]
fn closest_together_counts_repeats_only_once_per_name() {
    // Bin 5 holds three Elves but only one may go in; it can't fill the lot alone.
    let cands = vec![
        in_bin(0, "Elves", 5),
        in_bin(1, "Elves", 5),
        in_bin(2, "Elves", 5),
        in_bin(3, "Growth", 8),
        in_bin(4, "Bolt", 50),
    ];
    let picks = build_lot(&cands, &nearby(2));
    assert_eq!(bins(&cands, &picks), ["A-0-0-5", "A-0-0-8"]);
    assert_eq!(copies(&picks), 2);
}

#[test]
fn closest_together_still_applies_the_other_rules() {
    let mut cands = vec![in_bin(0, "A", 1), in_bin(1, "B", 1), in_bin(2, "C", 7)];
    cands[1].price = 3.0; // over the price cap
    let picks = build_lot(&cands, &nearby(2));
    let mut got = names(&cands, &picks);
    got.sort();
    assert_eq!(got, ["A", "C"]);
}

#[test]
fn a_replacement_comes_from_the_nearest_bin() {
    let cands = vec![
        in_bin(0, "A", 10),
        in_bin(1, "B", 10),
        in_bin(2, "Far", 90),
        in_bin(3, "Near", 11),
    ];
    let s = nearby(2);
    let mut picks = build_lot(&cands, &s);
    assert_eq!(bins(&cands, &picks), ["A-0-0-10"]);
    let gone = picks.remove(0);
    let added = top_up(&cands, &s, &picks, &HashSet::from([gone.candidate]));
    assert_eq!(names(&cands, &added), ["Near"]);
}

#[test]
fn cards_without_a_bin_come_last_when_gathering_by_location() {
    let mut cands = vec![in_bin(0, "Shelf", 3), in_bin(1, "Nowhere", 0)];
    cands[1].location = String::new();
    let picks = build_lot(&cands, &nearby(1));
    assert_eq!(names(&cands, &picks), ["Shelf"]);
}

#[test]
fn random_order_is_reproducible_per_seed() {
    let cands: Vec<Candidate> = (0..50)
        .map(|i| cand(i, &format!("Card {i}"), 0.05, Some(300)))
        .collect();
    let random = |seed| LotSpec {
        order: LotOrder::Random,
        seed,
        ..spec(10)
    };
    let a = build_lot(&cands, &random(7));
    assert_eq!(a, build_lot(&cands, &random(7)));
    assert_ne!(a, build_lot(&cands, &random(8)));
    assert_ne!(
        names(&cands, &a),
        names(&cands, &build_lot(&cands, &spec(10))),
        "not just the oldest-first order"
    );
}

// ── Replacing a card ─────────────────────────────────────────────────────────

#[test]
fn replacing_a_card_swaps_in_the_next_match() {
    let cands = vec![
        cand(0, "A", 0.05, Some(300)),
        cand(1, "B", 0.05, Some(200)),
        cand(2, "C", 0.05, Some(100)),
    ];
    let s = spec(2);
    let mut picks = build_lot(&cands, &s);
    let missing = picks.remove(0);
    let excluded = HashSet::from([missing.candidate]);
    let added = top_up(&cands, &s, &picks, &excluded);
    assert_eq!(names(&cands, &added), ["C"]);
}

#[test]
fn replacing_a_hit_brings_in_another_hit() {
    let cands = vec![
        cand(0, "Bulk", 0.05, Some(300)),
        cand(1, "Hit 1", 0.60, Some(300)),
        cand(2, "Hit 2", 0.70, Some(300)),
    ];
    let s = LotSpec {
        hits: 1,
        hit_min_price: 0.50,
        ..spec(2)
    };
    let mut picks = build_lot(&cands, &s);
    assert_eq!(names(&cands, &picks), ["Hit 1", "Bulk"]);
    let hit = picks.remove(0);
    let added = top_up(&cands, &s, &picks, &HashSet::from([hit.candidate]));
    assert_eq!(names(&cands, &added), ["Hit 2"]);
    assert!(added[0].hit);
}

#[test]
fn top_up_of_a_full_lot_adds_nothing() {
    let cands = vec![cand(0, "A", 0.05, Some(300)), cand(1, "B", 0.05, Some(300))];
    let s = spec(1);
    let picks = build_lot(&cands, &s);
    assert!(top_up(&cands, &s, &picks, &HashSet::new()).is_empty());
}

#[test]
fn top_up_with_an_empty_pool_adds_nothing() {
    let cands = vec![cand(0, "A", 0.05, Some(300))];
    let s = spec(2);
    let picks = build_lot(&cands, &s);
    let rest: Vec<LotPick> = Vec::new();
    assert!(top_up(&cands, &s, &rest, &HashSet::from([picks[0].candidate])).is_empty());
}

// ── Funnel and type lookups ──────────────────────────────────────────────────

#[test]
fn funnel_counts_copies_after_each_filter() {
    let mut cands = vec![
        cand(0, "A", 0.05, Some(300)),
        cand(1, "A", 0.05, Some(300)),
        cand(2, "Rare", 0.05, Some(300)),
        cand(3, "Pricey", 0.90, Some(300)),
        cand(4, "Forest", 0.01, Some(300)),
    ];
    cands[2].rarity = Rarity::Rare;
    cands[0].reserved = 2;
    let f = funnel(&cands, &spec(100));
    let counts: Vec<i64> = f.steps.iter().map(|s| s.copies).collect();
    // In stock, Common, basic lands out, finish, ≤ price, repeat limit.
    assert_eq!(counts, [5, 4, 3, 3, 2, 1]);
    assert_eq!(f.reserved, 2);
    assert_eq!(f.bottleneck(100).unwrap().label, "In stock");
    assert_eq!(f.bottleneck(4).unwrap().label, "Basic lands excluded");
    assert!(f.bottleneck(1).is_none());
}

#[test]
fn funnel_reports_unknown_types_and_the_hit_pool() {
    let mut cands = vec![
        cand(0, "Gate", 0.05, Some(300)),
        cand(1, "Unknown", 0.05, Some(300)),
        cand(2, "Dual", 3.00, Some(300)),
    ];
    cands[0].front_type = Some("Land — Gate".to_string());
    cands[2].front_type = Some("Land — Forest Island".to_string());
    let s = LotSpec {
        card_type: CardType::NonBasicLand,
        hits: 1,
        min_age_days: 30,
        ..spec(10)
    };
    let f = funnel(&cands, &s);
    assert_eq!(f.types_unknown, 1);
    assert_eq!(f.hit_pool, 1);
    assert!(f
        .steps
        .iter()
        .any(|s| s.label == "Non-basic lands" && s.copies == 2));
    assert!(f.steps.iter().any(|s| s.label == "Listed ≥ 30 days"));
}

#[test]
fn type_lookups_only_for_relevant_untyped_printings() {
    let mut cands = vec![
        cand(0, "A", 0.05, Some(300)),
        cand(1, "A again", 0.05, Some(300)),
        cand(2, "Typed", 0.05, Some(300)),
        cand(3, "Pricey", 5.00, Some(300)),
        cand(4, "Forest", 0.01, Some(300)),
        cand(5, "Rare", 0.05, Some(300)),
    ];
    cands[1].cn = "0".to_string(); // same printing as #0
    cands[1].set_code = "tst".to_string();
    cands[2].front_type = Some("Land".to_string());
    cands[5].rarity = Rarity::Rare;

    assert!(
        type_lookups_needed(&cands, &spec(10)).is_empty(),
        "type: any"
    );
    let s = LotSpec {
        card_type: CardType::NonBasicLand,
        ..spec(10)
    };
    assert_eq!(type_lookups_needed(&cands, &s), [0], "one per printing");
    let with_hits = LotSpec { hits: 1, ..s };
    assert_eq!(type_lookups_needed(&cands, &with_hits).len(), 2);
}

// ── Summary, naming, picking order, export ───────────────────────────────────

#[test]
fn summary_of_a_lot() {
    let mut cands = vec![
        cand(0, "A", 0.10, Some(300)),
        cand(1, "B", 0.60, Some(20)),
        cand(2, "C", 0.20, None),
    ];
    cands[1].purchase_lot = "L10".to_string();
    cands[2].purchase_lot = "L2".to_string();
    let picks = [
        LotPick {
            candidate: 0,
            copies: 2,
            hit: false,
        },
        LotPick {
            candidate: 1,
            copies: 1,
            hit: true,
        },
        LotPick {
            candidate: 2,
            copies: 1,
            hit: false,
        },
    ];
    let s = summarize(&cands, &picks, 0.50);
    assert_eq!(s.copies, 4);
    assert!((s.listed_value - 1.0).abs() < 1e-9);
    assert_eq!(s.hits, 1);
    assert_eq!(s.distinct_names, 3);
    assert_eq!((s.oldest_days, s.newest_days), (Some(300), Some(20)));
    assert_eq!(
        s.purchase_lots,
        [
            ("L1".to_string(), 2),
            ("L2".to_string(), 1),
            ("L10".to_string(), 1)
        ]
    );
}

#[test]
fn default_names() {
    assert_eq!(default_lot_name(&spec(100)), "100 Commons");
    let lands = LotSpec {
        card_type: CardType::NonBasicLand,
        rarities: Rarity::ALL.into_iter().collect(),
        ..spec(100)
    };
    assert_eq!(default_lot_name(&lands), "100 Non-Basic Lands");
    let mixed = LotSpec {
        rarities: BTreeSet::from([Rarity::Common, Rarity::Uncommon]),
        foil: FoilFilter::FoilOnly,
        ..spec(50)
    };
    assert_eq!(default_lot_name(&mixed), "50 Cards (Foil)");
    let creatures = LotSpec {
        card_type: CardType::Creature,
        ..spec(30)
    };
    assert_eq!(default_lot_name(&creatures), "30 Common Creatures");
}

#[test]
fn presets_set_the_lot_definition_only() {
    let mut s = LotSpec {
        excluded_languages: BTreeSet::from(["German".to_string()]),
        order: LotOrder::Random,
        ..LotSpec::default()
    };
    Preset::NonBasicLands.apply(&mut s);
    assert_eq!(s.card_type, CardType::NonBasicLand);
    assert_eq!(s.size, 100);
    assert!(s.rarities.contains(&Rarity::Rare));
    assert_eq!(s.order, LotOrder::Random, "kept");
    assert!(s.excluded_languages.contains("German"), "kept");
    Preset::Uncommons.apply(&mut s);
    assert_eq!((s.size, s.card_type), (50, CardType::Any));
    assert_eq!(s.rarities, BTreeSet::from([Rarity::Uncommon]));
}

#[test]
fn locations_sort_like_the_shelf() {
    let mut locs = vec![
        "A-0-0-10-L1-R",
        "",
        "A-0-0-9-L1-R",
        "B-0-0-1-L1-R",
        "A-0-1-2-L1-R",
    ];
    locs.sort_by_key(|l| location_sort_key(l));
    assert_eq!(
        locs,
        [
            "A-0-0-9-L1-R",
            "A-0-0-10-L1-R",
            "A-0-1-2-L1-R",
            "B-0-0-1-L1-R",
            ""
        ]
    );
}

#[test]
fn picks_are_sorted_for_walking_the_shelves() {
    let mut cands = vec![cand(0, "A", 0.05, None), cand(1, "B", 0.05, None)];
    cands[0].location = "A-0-0-12-L1-R".to_string();
    cands[1].location = "A-0-0-3-L1-R".to_string();
    let mut picks = [
        LotPick {
            candidate: 0,
            copies: 1,
            hit: false,
        },
        LotPick {
            candidate: 1,
            copies: 1,
            hit: false,
        },
    ];
    sort_for_picking(&cands, &mut picks);
    assert_eq!(picks[0].candidate, 1);
}

#[test]
fn powertools_csv_removes_the_copies_and_marks_foils() {
    let plain = csv_card("5", "0.10", "A-0-0-1-L1-R");
    let mut foil = csv_card("2", "0.30", "A-0-0-2-L1-R");
    foil.is_foil = String::new();
    foil.finish_type = "Foil".to_string();
    let csv = powertools_csv(&[(plain, 3), (foil, 1)]);

    let mut rdr = csv::Reader::from_reader(csv.as_bytes());
    let headers = rdr.headers().unwrap().clone();
    let col = |name: &str| headers.iter().position(|h| h == name).unwrap();
    let rows: Vec<csv::StringRecord> = rdr.records().map(Result::unwrap).collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(&rows[0][col("quantity")], "-3");
    assert_eq!(&rows[0][col("isFoil")], "false");
    assert_eq!(&rows[0][col("location")], "A-0-0-1-L1-R");
    assert_eq!(&rows[1][col("quantity")], "-1");
    assert_eq!(&rows[1][col("isFoil")], "true");
}

#[test]
fn export_file_names_are_safe() {
    assert_eq!(
        export_file_name("100 Commons", "2026-11-04"),
        "bulk-lot_2026-11-04_100-commons.csv"
    );
    assert_eq!(
        export_file_name("../../etc/passwd", "2026-11-04"),
        "bulk-lot_2026-11-04_etc-passwd.csv"
    );
    assert_eq!(
        export_file_name("€€€", "2026-11-04"),
        "bulk-lot_2026-11-04.csv"
    );
    let long = export_file_name(&"a".repeat(200), "2026-11-04");
    assert!(long.len() < 100);
}
