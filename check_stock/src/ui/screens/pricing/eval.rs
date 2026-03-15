use crate::models::Card;
use crate::ui::state::{
    ConditionFilter, FoilFilter, GraphNode, LanguageFilter, NodeId, NodeKind, RarityFilter, Wire,
};
use std::collections::{HashMap, VecDeque};

/// Evaluates the full graph and returns the card index set for every node.
/// Returns an empty map if no cards are loaded.
pub(super) fn evaluate_all(
    nodes: &[GraphNode],
    wires: &[Wire],
    all_cards: &[Card],
) -> HashMap<NodeId, Vec<usize>> {
    if all_cards.is_empty() {
        return HashMap::new();
    }

    // incoming[(to_node, to_port)] = from_node
    let incoming: HashMap<(NodeId, usize), NodeId> = wires
        .iter()
        .map(|w| ((w.to_node, w.to_port), w.from_node))
        .collect();

    // Topological sort (Kahn's algorithm)
    let mut adj: HashMap<NodeId, Vec<NodeId>> = nodes.iter().map(|n| (n.id, vec![])).collect();
    let mut in_deg: HashMap<NodeId, usize> = nodes.iter().map(|n| (n.id, 0)).collect();
    for w in wires {
        adj.entry(w.from_node).or_default().push(w.to_node);
        *in_deg.entry(w.to_node).or_insert(0) += 1;
    }
    let mut queue: VecDeque<NodeId> = in_deg
        .iter()
        .filter(|(_, &d)| d == 0)
        .map(|(&id, _)| id)
        .collect();
    let mut topo: Vec<NodeId> = Vec::with_capacity(nodes.len());
    let mut deg = in_deg.clone();
    while let Some(id) = queue.pop_front() {
        topo.push(id);
        if let Some(neighbors) = adj.get(&id) {
            for &next in neighbors {
                let d = deg.entry(next).or_insert(1);
                *d -= 1;
                if *d == 0 {
                    queue.push_back(next);
                }
            }
        }
    }

    let all_indices: Vec<usize> = (0..all_cards.len()).collect();
    let mut outputs: HashMap<NodeId, Vec<usize>> = HashMap::new();

    for id in topo {
        let node = match nodes.iter().find(|n| n.id == id) {
            Some(n) => n,
            None => continue,
        };

        let output = if node.kind.input_count() == 0 {
            filter_indices(&node.kind, all_indices.clone(), all_cards)
        } else {
            let inputs: Vec<Vec<usize>> = (0..node.kind.input_count())
                .map(|port| {
                    incoming
                        .get(&(id, port))
                        .and_then(|&from| outputs.get(&from))
                        .cloned()
                        .unwrap_or_default()
                })
                .collect();

            match &node.kind {
                NodeKind::LogicalAnd => {
                    let mut result = inputs[0].clone();
                    for other in &inputs[1..] {
                        let set: std::collections::HashSet<usize> = other.iter().copied().collect();
                        result.retain(|i| set.contains(i));
                    }
                    result
                }
                NodeKind::LogicalOr => {
                    let mut seen = std::collections::HashSet::new();
                    let mut result = Vec::new();
                    for input in &inputs {
                        for &i in input {
                            if seen.insert(i) {
                                result.push(i);
                            }
                        }
                    }
                    result.sort_unstable();
                    result
                }
                NodeKind::LogicalNot => {
                    let excluded: std::collections::HashSet<usize> =
                        inputs[0].iter().copied().collect();
                    all_indices
                        .iter()
                        .copied()
                        .filter(|i| !excluded.contains(i))
                        .collect()
                }
                _ => filter_indices(
                    &node.kind,
                    inputs.into_iter().next().unwrap_or_default(),
                    all_cards,
                ),
            }
        };

        outputs.insert(id, output);
    }

    outputs
}

#[cfg(test)]
pub(super) fn evaluate_counts(
    nodes: &[GraphNode],
    wires: &[Wire],
    all_cards: &[Card],
) -> HashMap<NodeId, usize> {
    evaluate_all(nodes, wires, all_cards)
        .into_iter()
        .map(|(id, v)| (id, v.len()))
        .collect()
}

/// Apply a node's filtering logic to a set of card indices.
/// Logical nodes (AND/OR/NOT) are handled in `evaluate_all`; this function is only
/// called for source/sink and filter nodes.
pub(super) fn filter_indices(kind: &NodeKind, indices: Vec<usize>, cards: &[Card]) -> Vec<usize> {
    match kind {
        NodeKind::CsvSource
        | NodeKind::Output
        | NodeKind::LogicalAnd
        | NodeKind::LogicalOr
        | NodeKind::LogicalNot => indices,

        NodeKind::FilterCondition { condition } => {
            if matches!(condition, ConditionFilter::Any) {
                return indices;
            }
            let target = condition.as_str();
            indices
                .into_iter()
                .filter(|&i| cards[i].condition.eq_ignore_ascii_case(target))
                .collect()
        }

        NodeKind::FilterLanguage { language } => {
            if matches!(language, LanguageFilter::Any) {
                return indices;
            }
            let t = language.as_str();
            indices
                .into_iter()
                .filter(|&i| cards[i].language.eq_ignore_ascii_case(t))
                .collect()
        }

        NodeKind::FilterFoil { mode } => match mode {
            FoilFilter::Any => indices,
            FoilFilter::FoilOnly => indices
                .into_iter()
                .filter(|&i| cards[i].is_foil_card())
                .collect(),
            FoilFilter::NonFoilOnly => indices
                .into_iter()
                .filter(|&i| !cards[i].is_foil_card())
                .collect(),
        },

        NodeKind::FilterPrice { min, max } => indices
            .into_iter()
            .filter(|&i| {
                let p = cards[i].price.parse::<f64>().unwrap_or(0.0);
                p >= *min && p <= *max
            })
            .collect(),

        NodeKind::FilterRarity { rarity } => {
            if matches!(rarity, RarityFilter::Any) {
                return indices;
            }
            let t = rarity.as_str();
            indices
                .into_iter()
                .filter(|&i| cards[i].rarity.eq_ignore_ascii_case(t))
                .collect()
        }

        NodeKind::FilterName { term } => {
            if term.is_empty() {
                return indices;
            }
            let t = term.to_lowercase();
            indices
                .into_iter()
                .filter(|&i| cards[i].name.to_lowercase().contains(&t))
                .collect()
        }

        NodeKind::FilterSet { term } => {
            if term.is_empty() {
                return indices;
            }
            let t = term.to_lowercase();
            indices
                .into_iter()
                .filter(|&i| {
                    cards[i].set.to_lowercase().contains(&t)
                        || cards[i].set_code.to_lowercase().contains(&t)
                })
                .collect()
        }

        NodeKind::FilterLocation { term } => {
            if term.is_empty() {
                return indices;
            }
            let t = term.to_lowercase();
            indices
                .into_iter()
                .filter(|&i| {
                    cards[i]
                        .location
                        .as_deref()
                        .map(|l| l.to_lowercase().contains(&t))
                        .unwrap_or(false)
                })
                .collect()
        }
    }
}
