use super::{evaluate_counts, filter_indices};
use crate::{
    models::Card,
    ui::state::{
        ConditionFilter, FoilFilter, GraphNode, LanguageFilter, NodeGraph, NodeKind, RarityFilter,
        SavedGraph, Wire,
    },
};

// ── Helpers ───────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn make_card(
    name: &str,
    condition: &str,
    language: &str,
    is_foil: &str,
    price: &str,
    rarity: &str,
    set: &str,
    set_code: &str,
    location: Option<&str>,
) -> Card {
    Card {
        cardmarket_id: String::new(),
        quantity: "1".into(),
        name: name.into(),
        set: set.into(),
        set_code: set_code.into(),
        cn: String::new(),
        condition: condition.into(),
        language: language.into(),
        is_foil: is_foil.into(),
        is_playset: None,
        is_signed: "false".into(),
        price: price.into(),
        comment: String::new(),
        location: location.map(String::from),
        name_de: String::new(),
        name_es: String::new(),
        name_fr: String::new(),
        name_it: String::new(),
        rarity: rarity.into(),
        listed_at: String::new(),
    }
}

fn all_indices(cards: &[Card]) -> Vec<usize> {
    (0..cards.len()).collect()
}

// ── filter_indices: FilterCondition ──────────────────────────────────────────

#[test]
fn filter_condition_any_passes_all() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "PL", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterCondition {
            condition: ConditionFilter::Any,
        },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![0, 1]);
}

#[test]
fn filter_condition_nm_exact_match() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "EX", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "C", "PL", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterCondition {
            condition: ConditionFilter::Nm,
        },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![0]);
}

#[test]
fn filter_condition_pl_matches_only_pl() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "PL", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "C", "GD", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterCondition {
            condition: ConditionFilter::Pl,
        },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![1]);
}

#[test]
fn filter_condition_case_insensitive() {
    let cards = vec![
        make_card(
            "A", "nm", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterCondition {
            condition: ConditionFilter::Nm,
        },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![0, 1]);
}

// ── filter_indices: FilterLanguage ────────────────────────────────────────────

#[test]
fn filter_language_any_passes_all() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "German", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterLanguage {
            language: LanguageFilter::Any,
        },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![0, 1]);
}

#[test]
fn filter_language_english_only() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "German", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "C", "NM", "French", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterLanguage {
            language: LanguageFilter::English,
        },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![0]);
}

#[test]
fn filter_language_case_insensitive() {
    let cards = vec![
        make_card(
            "A", "NM", "english", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "ENGLISH", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "C", "NM", "German", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterLanguage {
            language: LanguageFilter::English,
        },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![0, 1]);
}

// ── filter_indices: FilterFoil ────────────────────────────────────────────────

#[test]
fn filter_foil_any_passes_all() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "true", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterFoil {
            mode: FoilFilter::Any,
        },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![0, 1]);
}

#[test]
fn filter_foil_only_returns_foils() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "true", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "C", "NM", "English", "1", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterFoil {
            mode: FoilFilter::FoilOnly,
        },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![0, 2]);
}

#[test]
fn filter_non_foil_only_returns_non_foils() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "true", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterFoil {
            mode: FoilFilter::NonFoilOnly,
        },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![1]);
}

// ── filter_indices: FilterPrice ───────────────────────────────────────────────

#[test]
fn filter_price_inclusive_range() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "0.5", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "C", "NM", "English", "false", "5.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "D", "NM", "English", "false", "10.0", "Common", "Set", "s1", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterPrice { min: 1.0, max: 5.0 },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![1, 2]);
}

#[test]
fn filter_price_unparseable_treated_as_zero() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "invalid", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "3.0", "Common", "Set", "s1", None,
        ),
    ];
    // "invalid" parses as 0.0, only within 0..=1 range
    let result = filter_indices(
        &NodeKind::FilterPrice { min: 0.0, max: 1.0 },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![0]);
}

// ── filter_indices: FilterRarity ──────────────────────────────────────────────

#[test]
fn filter_rarity_any_passes_all() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Rare", "Set", "s1", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterRarity {
            rarity: RarityFilter::Any,
        },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![0, 1]);
}

#[test]
fn filter_rarity_rare_only() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Rare", "Set", "s1", None,
        ),
        make_card(
            "C", "NM", "English", "false", "1.0", "Mythic", "Set", "s1", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterRarity {
            rarity: RarityFilter::Rare,
        },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![1]);
}

// ── filter_indices: FilterName ────────────────────────────────────────────────

#[test]
fn filter_name_empty_passes_all() {
    let cards = vec![
        make_card(
            "Lightning Bolt",
            "NM",
            "English",
            "false",
            "1.0",
            "Common",
            "Set",
            "s1",
            None,
        ),
        make_card(
            "Counterspell",
            "NM",
            "English",
            "false",
            "1.0",
            "Common",
            "Set",
            "s1",
            None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterName {
            term: String::new(),
        },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![0, 1]);
}

#[test]
fn filter_name_substring_case_insensitive() {
    let cards = vec![
        make_card(
            "Lightning Bolt",
            "NM",
            "English",
            "false",
            "1.0",
            "Common",
            "Set",
            "s1",
            None,
        ),
        make_card(
            "Counterspell",
            "NM",
            "English",
            "false",
            "1.0",
            "Common",
            "Set",
            "s1",
            None,
        ),
        make_card(
            "Lightning Helix",
            "NM",
            "English",
            "false",
            "1.0",
            "Common",
            "Set",
            "s1",
            None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterName {
            term: "lightning".into(),
        },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![0, 2]);
}

// ── filter_indices: FilterSet ─────────────────────────────────────────────────

#[test]
fn filter_set_empty_passes_all() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Alpha", "lea", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Common", "Beta", "leb", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterSet {
            term: String::new(),
        },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![0, 1]);
}

#[test]
fn filter_set_matches_set_name() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Alpha", "lea", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Common", "Beta", "leb", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterSet {
            term: "alpha".into(),
        },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![0]);
}

#[test]
fn filter_set_matches_set_code() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Alpha", "lea", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Common", "Beta", "leb", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterSet { term: "leb".into() },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![1]);
}

// ── filter_indices: FilterLocation ────────────────────────────────────────────

#[test]
fn filter_location_empty_passes_all() {
    let cards = vec![
        make_card(
            "A",
            "NM",
            "English",
            "false",
            "1.0",
            "Common",
            "Set",
            "s1",
            Some("A1_S1_R1_C1"),
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterLocation {
            term: String::new(),
        },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![0, 1]);
}

#[test]
fn filter_location_none_excluded() {
    let cards = vec![
        make_card(
            "A",
            "NM",
            "English",
            "false",
            "1.0",
            "Common",
            "Set",
            "s1",
            Some("A1_S1_R1_C1"),
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterLocation { term: "A1".into() },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![0]);
}

#[test]
fn filter_location_substring_case_insensitive() {
    let cards = vec![
        make_card(
            "A",
            "NM",
            "English",
            "false",
            "1.0",
            "Common",
            "Set",
            "s1",
            Some("A1_S1_R1_C1"),
        ),
        make_card(
            "B",
            "NM",
            "English",
            "false",
            "1.0",
            "Common",
            "Set",
            "s1",
            Some("B2_S3_R1_C1"),
        ),
    ];
    let result = filter_indices(
        &NodeKind::FilterLocation { term: "a1".into() },
        all_indices(&cards),
        &cards,
    );
    assert_eq!(result, vec![0]);
}

// ── filter_indices: pass-through nodes ───────────────────────────────────────

#[test]
fn csv_source_passes_all() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let result = filter_indices(&NodeKind::CsvSource, all_indices(&cards), &cards);
    assert_eq!(result, vec![0, 1]);
}

#[test]
fn output_passes_all() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let result = filter_indices(&NodeKind::Output, all_indices(&cards), &cards);
    assert_eq!(result, vec![0, 1]);
}

// ── evaluate_counts ───────────────────────────────────────────────────────────

fn make_node(id: usize, kind: NodeKind) -> GraphNode {
    GraphNode {
        id,
        kind,
        pos: eframe::egui::pos2(0.0, 0.0),
    }
}

fn make_wire(from: usize, to: usize) -> Wire {
    Wire {
        from_node: from,
        from_port: 0,
        to_node: to,
        to_port: 0,
    }
}

#[test]
fn evaluate_counts_empty_cards_returns_empty() {
    let graph = NodeGraph::default();
    let counts = evaluate_counts(&graph.nodes, &graph.wires, &[]);
    assert!(counts.is_empty());
}

#[test]
fn evaluate_counts_csv_source_shows_all_cards() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "C", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let nodes = vec![make_node(0, NodeKind::CsvSource)];
    let counts = evaluate_counts(&nodes, &[], &cards);
    assert_eq!(counts[&0], 3);
}

#[test]
fn evaluate_counts_filter_flows_through_wires() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "GD", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "C", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    // CsvSource(0) → FilterCondition(1)[NM] → Output(2)
    let nodes = vec![
        make_node(0, NodeKind::CsvSource),
        make_node(
            1,
            NodeKind::FilterCondition {
                condition: ConditionFilter::Nm,
            },
        ),
        make_node(2, NodeKind::Output),
    ];
    let wires = vec![make_wire(0, 1), make_wire(1, 2)];
    let counts = evaluate_counts(&nodes, &wires, &cards);
    assert_eq!(counts[&0], 3); // source sees all
    assert_eq!(counts[&1], 2); // only NM cards
    assert_eq!(counts[&2], 2); // output mirrors filter
}

#[test]
fn evaluate_counts_unconnected_filter_shows_zero() {
    let cards = vec![make_card(
        "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
    )];
    // FilterCondition with no input wire gets empty input → 0
    let nodes = vec![make_node(
        0,
        NodeKind::FilterCondition {
            condition: ConditionFilter::Nm,
        },
    )];
    let counts = evaluate_counts(&nodes, &[], &cards);
    assert_eq!(counts[&0], 0);
}

#[test]
fn evaluate_counts_cycle_nodes_excluded() {
    let cards = vec![make_card(
        "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
    )];
    // Two nodes wired in a cycle — neither should appear (or both appear as 0)
    let nodes = vec![
        make_node(
            0,
            NodeKind::FilterCondition {
                condition: ConditionFilter::Any,
            },
        ),
        make_node(
            1,
            NodeKind::FilterCondition {
                condition: ConditionFilter::Any,
            },
        ),
    ];
    let wires = vec![make_wire(0, 1), make_wire(1, 0)];
    let counts = evaluate_counts(&nodes, &wires, &cards);
    // Both nodes are in the cycle, so Kahn's algorithm never adds them to topo order
    assert!(!counts.contains_key(&0));
    assert!(!counts.contains_key(&1));
}

// ── NodeKind methods ──────────────────────────────────────────────────────────

#[test]
fn node_kind_titles() {
    assert_eq!(NodeKind::CsvSource.title(), "CSV Source");
    assert_eq!(NodeKind::Output.title(), "Output");
    assert_eq!(
        NodeKind::FilterCondition {
            condition: ConditionFilter::Any
        }
        .title(),
        "Filter Condition"
    );
    assert_eq!(
        NodeKind::FilterName {
            term: String::new()
        }
        .title(),
        "Filter Name"
    );
}

#[test]
fn node_kind_input_output_counts() {
    assert_eq!(NodeKind::CsvSource.input_count(), 0);
    assert_eq!(NodeKind::CsvSource.output_count(), 1);
    assert_eq!(NodeKind::Output.input_count(), 1);
    assert_eq!(NodeKind::Output.output_count(), 0);
    assert_eq!(
        NodeKind::FilterCondition {
            condition: ConditionFilter::Any
        }
        .input_count(),
        1
    );
    assert_eq!(
        NodeKind::FilterCondition {
            condition: ConditionFilter::Any
        }
        .output_count(),
        1
    );
}

#[test]
fn node_kind_param_count() {
    assert_eq!(NodeKind::CsvSource.param_count(), 0);
    assert_eq!(NodeKind::Output.param_count(), 0);
    assert_eq!(
        NodeKind::FilterPrice {
            min: 0.0,
            max: 10.0
        }
        .param_count(),
        2
    );
    assert_eq!(
        NodeKind::FilterCondition {
            condition: ConditionFilter::Any
        }
        .param_count(),
        1
    );
    assert_eq!(
        NodeKind::FilterName {
            term: String::new()
        }
        .param_count(),
        1
    );
}

// ── NodeGraph CRUD ────────────────────────────────────────────────────────────

#[test]
fn node_graph_default_has_csv_source_and_output() {
    let g = NodeGraph::default();
    assert_eq!(g.nodes.len(), 2);
    assert!(g
        .nodes
        .iter()
        .any(|n| matches!(n.kind, NodeKind::CsvSource)));
    assert!(g.nodes.iter().any(|n| matches!(n.kind, NodeKind::Output)));
    assert!(g.wires.is_empty());
}

#[test]
fn node_graph_add_node_assigns_incrementing_ids() {
    let mut g = NodeGraph::default();
    let existing = g.nodes.len();
    let id_a = g.add_node(
        NodeKind::FilterName {
            term: String::new(),
        },
        eframe::egui::pos2(0.0, 0.0),
    );
    let id_b = g.add_node(
        NodeKind::FilterName {
            term: String::new(),
        },
        eframe::egui::pos2(0.0, 0.0),
    );
    assert_ne!(id_a, id_b);
    assert_eq!(g.nodes.len(), existing + 2);
}

#[test]
fn node_graph_remove_node_cleans_up_wires() {
    let mut g = NodeGraph::default();
    let csv_id = g
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::CsvSource))
        .unwrap()
        .id;
    let out_id = g
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Output))
        .unwrap()
        .id;
    let filter_id = g.add_node(
        NodeKind::FilterCondition {
            condition: ConditionFilter::Any,
        },
        eframe::egui::pos2(0.0, 0.0),
    );
    g.wires.push(Wire {
        from_node: csv_id,
        from_port: 0,
        to_node: filter_id,
        to_port: 0,
    });
    g.wires.push(Wire {
        from_node: filter_id,
        from_port: 0,
        to_node: out_id,
        to_port: 0,
    });
    assert_eq!(g.wires.len(), 2);

    g.remove_node(filter_id);

    assert!(!g.nodes.iter().any(|n| n.id == filter_id));
    assert!(g.wires.is_empty());
}

#[test]
fn node_graph_remove_permanent_nodes_allowed_by_graph() {
    // remove_node itself has no guard — permanence is enforced by the UI right-click handler
    let mut g = NodeGraph::default();
    let csv_id = g
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::CsvSource))
        .unwrap()
        .id;
    g.remove_node(csv_id);
    assert!(!g
        .nodes
        .iter()
        .any(|n| matches!(n.kind, NodeKind::CsvSource)));
}

#[test]
fn node_graph_node_mut_returns_correct_node() {
    let mut g = NodeGraph::default();
    let csv_id = g
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::CsvSource))
        .unwrap()
        .id;
    let node = g.node_mut(csv_id).unwrap();
    assert!(matches!(node.kind, NodeKind::CsvSource));
    assert!(g.node_mut(9999).is_none());
}

// ── Filter enum: as_str / all ─────────────────────────────────────────────────

#[test]
fn condition_filter_all_covered() {
    let all = ConditionFilter::all();
    assert_eq!(all.len(), 6);
    assert!(all.contains(&ConditionFilter::Any));
    assert!(all.contains(&ConditionFilter::Pl));
}

#[test]
fn condition_filter_as_str() {
    assert_eq!(ConditionFilter::Nm.as_str(), "NM");
    assert_eq!(ConditionFilter::Pl.as_str(), "PL");
    assert_eq!(ConditionFilter::Any.as_str(), "Any");
}

#[test]
fn language_filter_all_covered() {
    let all = LanguageFilter::all();
    assert!(all.contains(&LanguageFilter::Any));
    assert!(all.contains(&LanguageFilter::German));
}

#[test]
fn foil_filter_all_covered() {
    let all = FoilFilter::all();
    assert_eq!(all.len(), 3);
    assert!(all.contains(&FoilFilter::Any));
    assert!(all.contains(&FoilFilter::FoilOnly));
    assert!(all.contains(&FoilFilter::NonFoilOnly));
}

#[test]
fn rarity_filter_all_covered() {
    let all = crate::ui::state::RarityFilter::all();
    assert!(all.contains(&RarityFilter::Any));
    assert!(all.contains(&RarityFilter::Mythic));
}

// ── NodeGraph::save / NodeGraph::load ─────────────────────────────────────────

fn make_graph_with_filter() -> (NodeGraph, usize, usize, usize) {
    let mut g = NodeGraph::default();
    let csv_id = g
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::CsvSource))
        .unwrap()
        .id;
    let out_id = g
        .nodes
        .iter()
        .find(|n| matches!(n.kind, NodeKind::Output))
        .unwrap()
        .id;
    let filter_id = g.add_node(
        NodeKind::FilterCondition {
            condition: ConditionFilter::Nm,
        },
        eframe::egui::pos2(200.0, 50.0),
    );
    g.wires.push(Wire {
        from_node: csv_id,
        from_port: 0,
        to_node: filter_id,
        to_port: 0,
    });
    g.wires.push(Wire {
        from_node: filter_id,
        from_port: 0,
        to_node: out_id,
        to_port: 0,
    });
    (g, csv_id, filter_id, out_id)
}

#[test]
fn save_captures_node_count_and_positions() {
    let (g, _, filter_id, _) = make_graph_with_filter();
    let saved = g.save();
    assert_eq!(saved.nodes.len(), 3);
    let saved_filter = saved.nodes.iter().find(|n| n.id == filter_id).unwrap();
    assert!((saved_filter.x - 200.0).abs() < f32::EPSILON);
    assert!((saved_filter.y - 50.0).abs() < f32::EPSILON);
}

#[test]
fn save_captures_wires() {
    let (g, csv_id, filter_id, out_id) = make_graph_with_filter();
    let saved = g.save();
    assert_eq!(saved.wires.len(), 2);
    assert!(saved
        .wires
        .iter()
        .any(|w| w.from_node == csv_id && w.to_node == filter_id));
    assert!(saved
        .wires
        .iter()
        .any(|w| w.from_node == filter_id && w.to_node == out_id));
}

#[test]
fn save_captures_canvas_state() {
    let mut g = NodeGraph::default();
    g.canvas_offset = eframe::egui::vec2(42.0, -7.5);
    g.canvas_zoom = 1.5;
    let saved = g.save();
    assert!((saved.canvas_offset_x - 42.0).abs() < f32::EPSILON);
    assert!((saved.canvas_offset_y - (-7.5)).abs() < f32::EPSILON);
    assert!((saved.canvas_zoom - 1.5).abs() < f32::EPSILON);
}

#[test]
fn load_restores_nodes_and_wires() {
    let (original, csv_id, filter_id, out_id) = make_graph_with_filter();
    let saved = original.save();
    let restored = NodeGraph::load(saved);

    assert_eq!(restored.nodes.len(), 3);
    assert!(restored.nodes.iter().any(|n| n.id == csv_id));
    assert!(restored.nodes.iter().any(|n| n.id == filter_id));
    assert!(restored.nodes.iter().any(|n| n.id == out_id));
    assert_eq!(restored.wires.len(), 2);
}

#[test]
fn load_restores_node_positions() {
    let (original, _, filter_id, _) = make_graph_with_filter();
    let saved = original.save();
    let restored = NodeGraph::load(saved);
    let node = restored.nodes.iter().find(|n| n.id == filter_id).unwrap();
    assert!((node.pos.x - 200.0).abs() < f32::EPSILON);
    assert!((node.pos.y - 50.0).abs() < f32::EPSILON);
}

#[test]
fn load_restores_canvas_state() {
    let mut g = NodeGraph::default();
    g.canvas_offset = eframe::egui::vec2(100.0, 30.0);
    g.canvas_zoom = 0.75;
    let restored = NodeGraph::load(g.save());
    assert!((restored.canvas_offset.x - 100.0).abs() < f32::EPSILON);
    assert!((restored.canvas_offset.y - 30.0).abs() < f32::EPSILON);
    assert!((restored.canvas_zoom - 0.75).abs() < f32::EPSILON);
}

#[test]
fn load_sets_next_id_beyond_max_existing() {
    let (original, _, _, _) = make_graph_with_filter();
    let max_id = original.nodes.iter().map(|n| n.id).max().unwrap();
    let mut restored = NodeGraph::load(original.save());
    // Adding a new node must get an id higher than all restored ids
    let new_id = restored.add_node(NodeKind::CsvSource, eframe::egui::pos2(0.0, 0.0));
    assert!(new_id > max_id);
}

#[test]
fn load_then_add_node_ids_are_unique() {
    let (original, _, _, _) = make_graph_with_filter();
    let mut restored = NodeGraph::load(original.save());
    let id_a = restored.add_node(NodeKind::Output, eframe::egui::pos2(0.0, 0.0));
    let id_b = restored.add_node(NodeKind::Output, eframe::egui::pos2(0.0, 0.0));
    let all_ids: Vec<usize> = restored.nodes.iter().map(|n| n.id).collect();
    assert_ne!(id_a, id_b);
    // All ids in the graph are unique
    let unique: std::collections::HashSet<usize> = all_ids.iter().copied().collect();
    assert_eq!(unique.len(), restored.nodes.len());
}

#[test]
fn round_trip_preserves_all_node_kinds() {
    use crate::ui::state::{FoilFilter, LanguageFilter, RarityFilter};
    let mut g = NodeGraph::default();
    let kinds = vec![
        NodeKind::FilterCondition {
            condition: ConditionFilter::Gd,
        },
        NodeKind::FilterLanguage {
            language: LanguageFilter::German,
        },
        NodeKind::FilterFoil {
            mode: FoilFilter::FoilOnly,
        },
        NodeKind::FilterPrice {
            min: 1.5,
            max: 9.99,
        },
        NodeKind::FilterRarity {
            rarity: RarityFilter::Mythic,
        },
        NodeKind::FilterName {
            term: "bolt".into(),
        },
        NodeKind::FilterSet { term: "lea".into() },
        NodeKind::FilterLocation {
            term: "A1_S2".into(),
        },
    ];
    for kind in &kinds {
        g.add_node(kind.clone(), eframe::egui::pos2(0.0, 0.0));
    }
    let restored = NodeGraph::load(g.save());

    // Spot-check a few variants survive the round-trip
    assert!(restored.nodes.iter().any(
        |n| matches!(&n.kind, NodeKind::FilterLanguage { language } if *language == LanguageFilter::German)
    ));
    assert!(restored.nodes.iter().any(
        |n| matches!(&n.kind, NodeKind::FilterPrice { min, .. } if (*min - 1.5).abs() < 1e-6)
    ));
    assert!(restored
        .nodes
        .iter()
        .any(|n| matches!(&n.kind, NodeKind::FilterName { term } if term == "bolt")));
    assert!(restored
        .nodes
        .iter()
        .any(|n| matches!(&n.kind, NodeKind::FilterLocation { term } if term == "A1_S2")));
}

#[test]
fn json_round_trip_is_valid() {
    let (g, _, _, _) = make_graph_with_filter();
    let saved = g.save();
    let json = serde_json::to_string(&saved).expect("serialize failed");
    let deserialized: SavedGraph = serde_json::from_str(&json).expect("deserialize failed");
    let restored = NodeGraph::load(deserialized);
    assert_eq!(restored.nodes.len(), 3);
    assert_eq!(restored.wires.len(), 2);
}

#[test]
fn json_is_human_readable() {
    let mut g = NodeGraph::default();
    g.add_node(
        NodeKind::FilterName {
            term: "Jace".into(),
        },
        eframe::egui::pos2(0.0, 0.0),
    );
    let json = serde_json::to_string_pretty(&g.save()).expect("serialize failed");
    assert!(json.contains("FilterName"));
    assert!(json.contains("Jace"));
    assert!(json.contains("canvas_zoom"));
}
