use super::{condition_rank, evaluate_counts, filter_indices, sort_preview};
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

fn make_wire_ports(from: usize, from_port: usize, to: usize, to_port: usize) -> Wire {
    Wire {
        from_node: from,
        from_port,
        to_node: to,
        to_port,
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

// ── filter_indices: remaining ConditionFilter variants ────────────────────────

#[test]
fn filter_condition_ex_gd_lp_variants() {
    let cards = vec![
        make_card(
            "A", "EX", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "GD", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "C", "LP", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "D", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    assert_eq!(
        filter_indices(
            &NodeKind::FilterCondition {
                condition: ConditionFilter::Ex
            },
            all_indices(&cards),
            &cards,
        ),
        vec![0]
    );
    assert_eq!(
        filter_indices(
            &NodeKind::FilterCondition {
                condition: ConditionFilter::Gd
            },
            all_indices(&cards),
            &cards,
        ),
        vec![1]
    );
    assert_eq!(
        filter_indices(
            &NodeKind::FilterCondition {
                condition: ConditionFilter::Lp
            },
            all_indices(&cards),
            &cards,
        ),
        vec![2]
    );
}

// ── filter_indices: remaining LanguageFilter variants ─────────────────────────

#[test]
fn filter_language_german_french_spanish_italian() {
    let cards = vec![
        make_card(
            "A", "NM", "German", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "French", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "C", "NM", "Spanish", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "D", "NM", "Italian", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "E", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    assert_eq!(
        filter_indices(
            &NodeKind::FilterLanguage {
                language: LanguageFilter::German
            },
            all_indices(&cards),
            &cards,
        ),
        vec![0]
    );
    assert_eq!(
        filter_indices(
            &NodeKind::FilterLanguage {
                language: LanguageFilter::French
            },
            all_indices(&cards),
            &cards,
        ),
        vec![1]
    );
    assert_eq!(
        filter_indices(
            &NodeKind::FilterLanguage {
                language: LanguageFilter::Spanish
            },
            all_indices(&cards),
            &cards,
        ),
        vec![2]
    );
    assert_eq!(
        filter_indices(
            &NodeKind::FilterLanguage {
                language: LanguageFilter::Italian
            },
            all_indices(&cards),
            &cards,
        ),
        vec![3]
    );
}

// ── filter_indices: remaining RarityFilter variants ───────────────────────────

#[test]
fn filter_rarity_common_uncommon_mythic() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Uncommon", "Set", "s1", None,
        ),
        make_card(
            "C", "NM", "English", "false", "1.0", "Mythic", "Set", "s1", None,
        ),
        make_card(
            "D", "NM", "English", "false", "1.0", "Rare", "Set", "s1", None,
        ),
    ];
    assert_eq!(
        filter_indices(
            &NodeKind::FilterRarity {
                rarity: RarityFilter::Common
            },
            all_indices(&cards),
            &cards,
        ),
        vec![0]
    );
    assert_eq!(
        filter_indices(
            &NodeKind::FilterRarity {
                rarity: RarityFilter::Uncommon
            },
            all_indices(&cards),
            &cards,
        ),
        vec![1]
    );
    assert_eq!(
        filter_indices(
            &NodeKind::FilterRarity {
                rarity: RarityFilter::Mythic
            },
            all_indices(&cards),
            &cards,
        ),
        vec![2]
    );
}

// ── NodeKind: logical node metadata ──────────────────────────────────────────

#[test]
fn node_kind_logical_titles() {
    assert_eq!(NodeKind::LogicalAnd.title(), "AND");
    assert_eq!(NodeKind::LogicalOr.title(), "OR");
    assert_eq!(NodeKind::LogicalNot.title(), "NOT");
}

#[test]
fn node_kind_logical_input_output_counts() {
    assert_eq!(NodeKind::LogicalAnd.input_count(), 2);
    assert_eq!(NodeKind::LogicalAnd.output_count(), 1);
    assert_eq!(NodeKind::LogicalOr.input_count(), 2);
    assert_eq!(NodeKind::LogicalOr.output_count(), 1);
    assert_eq!(NodeKind::LogicalNot.input_count(), 1);
    assert_eq!(NodeKind::LogicalNot.output_count(), 1);
}

#[test]
fn node_kind_logical_param_count() {
    assert_eq!(NodeKind::LogicalAnd.param_count(), 0);
    assert_eq!(NodeKind::LogicalOr.param_count(), 0);
    assert_eq!(NodeKind::LogicalNot.param_count(), 0);
}

// ── evaluate_counts: LogicalAnd ───────────────────────────────────────────────

#[test]
fn evaluate_counts_logical_and_intersection() {
    // Cards: A=NM+English, B=GD+English, C=NM+German, D=GD+German
    // AND(FilterNM, FilterEnglish) → only A passes both
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "GD", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "C", "NM", "German", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "D", "GD", "German", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let nodes = vec![
        make_node(0, NodeKind::CsvSource),
        make_node(
            1,
            NodeKind::FilterCondition {
                condition: ConditionFilter::Nm,
            },
        ),
        make_node(
            2,
            NodeKind::FilterLanguage {
                language: LanguageFilter::English,
            },
        ),
        make_node(3, NodeKind::LogicalAnd),
        make_node(4, NodeKind::Output),
    ];
    let wires = vec![
        make_wire(0, 1),
        make_wire(0, 2),
        make_wire_ports(1, 0, 3, 0),
        make_wire_ports(2, 0, 3, 1),
        make_wire(3, 4),
    ];
    let counts = evaluate_counts(&nodes, &wires, &cards);
    assert_eq!(counts[&3], 1);
    assert_eq!(counts[&4], 1);
}

#[test]
fn evaluate_counts_logical_and_unconnected_returns_zero() {
    let cards = vec![make_card(
        "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
    )];
    let nodes = vec![make_node(0, NodeKind::LogicalAnd)];
    let counts = evaluate_counts(&nodes, &[], &cards);
    assert_eq!(counts[&0], 0);
}

#[test]
fn evaluate_counts_logical_and_one_port_connected_returns_zero() {
    // Port 1 unconnected → intersection with empty set → 0
    let cards = vec![make_card(
        "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
    )];
    let nodes = vec![
        make_node(0, NodeKind::CsvSource),
        make_node(1, NodeKind::LogicalAnd),
    ];
    let wires = vec![make_wire_ports(0, 0, 1, 0)]; // only port 0 connected
    let counts = evaluate_counts(&nodes, &wires, &cards);
    assert_eq!(counts[&1], 0);
}

// ── evaluate_counts: LogicalOr ────────────────────────────────────────────────

#[test]
fn evaluate_counts_logical_or_union() {
    // Cards: A=NM+foil, B=GD+foil, C=NM+non-foil, D=GD+non-foil
    // OR(FilterNM, FilterFoil): A∪B∪C = 3 (D fails both)
    let cards = vec![
        make_card(
            "A", "NM", "English", "true", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "GD", "English", "true", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "C", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "D", "GD", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let nodes = vec![
        make_node(0, NodeKind::CsvSource),
        make_node(
            1,
            NodeKind::FilterCondition {
                condition: ConditionFilter::Nm,
            },
        ),
        make_node(
            2,
            NodeKind::FilterFoil {
                mode: FoilFilter::FoilOnly,
            },
        ),
        make_node(3, NodeKind::LogicalOr),
        make_node(4, NodeKind::Output),
    ];
    let wires = vec![
        make_wire(0, 1),
        make_wire(0, 2),
        make_wire_ports(1, 0, 3, 0),
        make_wire_ports(2, 0, 3, 1),
        make_wire(3, 4),
    ];
    let counts = evaluate_counts(&nodes, &wires, &cards);
    assert_eq!(counts[&3], 3);
    assert_eq!(counts[&4], 3);
}

#[test]
fn evaluate_counts_logical_or_deduplicates_overlap() {
    // Both filters match the same card — OR must not double-count it
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "GD", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    // FilterLanguage(English) → {A, B}; FilterName("A") → {A}; OR → {A, B} (not 3)
    let nodes = vec![
        make_node(0, NodeKind::CsvSource),
        make_node(
            1,
            NodeKind::FilterLanguage {
                language: LanguageFilter::English,
            },
        ),
        make_node(2, NodeKind::FilterName { term: "A".into() }),
        make_node(3, NodeKind::LogicalOr),
    ];
    let wires = vec![
        make_wire(0, 1),
        make_wire(0, 2),
        make_wire_ports(1, 0, 3, 0),
        make_wire_ports(2, 0, 3, 1),
    ];
    let counts = evaluate_counts(&nodes, &wires, &cards);
    assert_eq!(counts[&3], 2);
}

// ── evaluate_counts: LogicalNot ───────────────────────────────────────────────

#[test]
fn evaluate_counts_logical_not_complement() {
    // CSV(3 cards) → FilterNM → NOT → Output: NOT inverts, returns GD and PL
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "GD", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "C", "PL", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let nodes = vec![
        make_node(0, NodeKind::CsvSource),
        make_node(
            1,
            NodeKind::FilterCondition {
                condition: ConditionFilter::Nm,
            },
        ),
        make_node(2, NodeKind::LogicalNot),
        make_node(3, NodeKind::Output),
    ];
    let wires = vec![make_wire(0, 1), make_wire(1, 2), make_wire(2, 3)];
    let counts = evaluate_counts(&nodes, &wires, &cards);
    assert_eq!(counts[&2], 2); // B and C
    assert_eq!(counts[&3], 2);
}

#[test]
fn evaluate_counts_logical_not_unconnected_returns_all() {
    // Unconnected NOT: complement of empty input = all cards
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "GD", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let nodes = vec![make_node(0, NodeKind::LogicalNot)];
    let counts = evaluate_counts(&nodes, &[], &cards);
    assert_eq!(counts[&0], 2);
}

// ── condition_rank ────────────────────────────────────────────────────────────

#[test]
fn condition_rank_strict_ordering() {
    assert!(condition_rank("NM") < condition_rank("EX"));
    assert!(condition_rank("EX") < condition_rank("GD"));
    assert!(condition_rank("GD") < condition_rank("LP"));
    assert!(condition_rank("LP") < condition_rank("PL"));
}

#[test]
fn condition_rank_case_insensitive() {
    assert_eq!(condition_rank("nm"), condition_rank("NM"));
    assert_eq!(condition_rank("ex"), condition_rank("EX"));
    assert_eq!(condition_rank("gd"), condition_rank("GD"));
}

#[test]
fn condition_rank_unknown_is_lowest_priority() {
    assert_eq!(condition_rank("UNKNOWN"), 5);
    assert_eq!(condition_rank(""), 5);
    assert!(condition_rank("PL") < condition_rank("UNKNOWN"));
}

// ── sort_preview ──────────────────────────────────────────────────────────────

#[test]
fn sort_preview_by_name_ascending() {
    let cards = vec![
        make_card(
            "Zap", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "Bolt", "NM", "English", "false", "2.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "Aura", "NM", "English", "false", "3.0", "Common", "Set", "s1", None,
        ),
    ];
    let mut indices = vec![0, 1, 2];
    sort_preview(&mut indices, &cards, 0, true);
    assert_eq!(indices, vec![2, 1, 0]); // Aura, Bolt, Zap
}

#[test]
fn sort_preview_by_name_descending() {
    let cards = vec![
        make_card(
            "Aura", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "Bolt", "NM", "English", "false", "2.0", "Common", "Set", "s1", None,
        ),
    ];
    let mut indices = vec![0, 1];
    sort_preview(&mut indices, &cards, 0, false);
    assert_eq!(indices, vec![1, 0]); // Bolt, Aura
}

#[test]
fn sort_preview_by_set_ascending() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.0", "Common", "Zendikar", "zen", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Common", "Alpha", "lea", None,
        ),
    ];
    let mut indices = vec![0, 1];
    sort_preview(&mut indices, &cards, 1, true);
    assert_eq!(indices, vec![1, 0]); // Alpha, Zendikar
}

#[test]
fn sort_preview_by_condition_rank_ascending() {
    let cards = vec![
        make_card(
            "A", "PL", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "C", "GD", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let mut indices = vec![0, 1, 2];
    sort_preview(&mut indices, &cards, 2, true); // NM first
    assert_eq!(indices, vec![1, 2, 0]); // NM, GD, PL
}

#[test]
fn sort_preview_by_language_ascending() {
    let cards = vec![
        make_card(
            "A", "NM", "Spanish", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "C", "NM", "German", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let mut indices = vec![0, 1, 2];
    sort_preview(&mut indices, &cards, 3, true);
    assert_eq!(indices, vec![1, 2, 0]); // English, German, Spanish
}

#[test]
fn sort_preview_by_foil_ascending_non_foil_first() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "true", "1.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let mut indices = vec![0, 1];
    sort_preview(&mut indices, &cards, 4, true);
    assert_eq!(indices, vec![1, 0]); // non-foil first
}

#[test]
fn sort_preview_by_price_ascending() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "3.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "0.5", "Common", "Set", "s1", None,
        ),
        make_card(
            "C", "NM", "English", "false", "1.5", "Common", "Set", "s1", None,
        ),
    ];
    let mut indices = vec![0, 1, 2];
    sort_preview(&mut indices, &cards, 5, true);
    assert_eq!(indices, vec![1, 2, 0]); // 0.5, 1.5, 3.0
}

#[test]
fn sort_preview_by_price_descending() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "1.5", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "3.0", "Common", "Set", "s1", None,
        ),
        make_card(
            "C", "NM", "English", "false", "0.5", "Common", "Set", "s1", None,
        ),
    ];
    let mut indices = vec![0, 1, 2];
    sort_preview(&mut indices, &cards, 5, false);
    assert_eq!(indices, vec![1, 0, 2]); // 3.0, 1.5, 0.5
}

#[test]
fn sort_preview_price_unparseable_treated_as_zero() {
    let cards = vec![
        make_card(
            "A", "NM", "English", "false", "invalid", "Common", "Set", "s1", None,
        ),
        make_card(
            "B", "NM", "English", "false", "2.0", "Common", "Set", "s1", None,
        ),
    ];
    let mut indices = vec![0, 1];
    sort_preview(&mut indices, &cards, 5, true);
    assert_eq!(indices, vec![0, 1]); // invalid→0.0 < 2.0
}

#[test]
fn sort_preview_by_location_ascending() {
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
            Some("C1"),
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
            Some("A1"),
        ),
        make_card(
            "C", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
        ),
    ];
    let mut indices = vec![0, 1, 2];
    sort_preview(&mut indices, &cards, 6, true);
    assert_eq!(indices, vec![2, 1, 0]); // ""(None) < "A1" < "C1"
}

#[test]
fn sort_preview_empty_indices_is_noop() {
    let cards = vec![make_card(
        "A", "NM", "English", "false", "1.0", "Common", "Set", "s1", None,
    )];
    let mut indices: Vec<usize> = vec![];
    sort_preview(&mut indices, &cards, 0, true);
    assert!(indices.is_empty());
}

// ── filter enum as_str ────────────────────────────────────────────────────────

#[test]
fn language_filter_as_str() {
    assert_eq!(LanguageFilter::Any.as_str(), "Any");
    assert_eq!(LanguageFilter::English.as_str(), "English");
    assert_eq!(LanguageFilter::German.as_str(), "German");
    assert_eq!(LanguageFilter::French.as_str(), "French");
    assert_eq!(LanguageFilter::Spanish.as_str(), "Spanish");
    assert_eq!(LanguageFilter::Italian.as_str(), "Italian");
}

#[test]
fn foil_filter_as_str() {
    assert_eq!(FoilFilter::Any.as_str(), "Any");
    assert_eq!(FoilFilter::FoilOnly.as_str(), "Foil only");
    assert_eq!(FoilFilter::NonFoilOnly.as_str(), "Non-foil only");
}

#[test]
fn rarity_filter_as_str() {
    assert_eq!(RarityFilter::Any.as_str(), "Any");
    assert_eq!(RarityFilter::Common.as_str(), "Common");
    assert_eq!(RarityFilter::Uncommon.as_str(), "Uncommon");
    assert_eq!(RarityFilter::Rare.as_str(), "Rare");
    assert_eq!(RarityFilter::Mythic.as_str(), "Mythic");
}

#[test]
fn condition_filter_as_str_all_variants() {
    assert_eq!(ConditionFilter::Ex.as_str(), "EX");
    assert_eq!(ConditionFilter::Gd.as_str(), "GD");
    assert_eq!(ConditionFilter::Lp.as_str(), "LP");
}

// ── round-trip: logical node kinds survive serialization ──────────────────────

#[test]
fn round_trip_preserves_logical_node_kinds() {
    let mut g = NodeGraph::default();
    g.add_node(NodeKind::LogicalAnd, eframe::egui::pos2(100.0, 0.0));
    g.add_node(NodeKind::LogicalOr, eframe::egui::pos2(200.0, 0.0));
    g.add_node(NodeKind::LogicalNot, eframe::egui::pos2(300.0, 0.0));
    let restored = NodeGraph::load(g.save());
    assert!(restored
        .nodes
        .iter()
        .any(|n| matches!(n.kind, NodeKind::LogicalAnd)));
    assert!(restored
        .nodes
        .iter()
        .any(|n| matches!(n.kind, NodeKind::LogicalOr)));
    assert!(restored
        .nodes
        .iter()
        .any(|n| matches!(n.kind, NodeKind::LogicalNot)));
}
