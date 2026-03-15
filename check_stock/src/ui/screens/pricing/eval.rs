use crate::models::Card;
use crate::ui::state::{
    ConditionFilter, FoilFilter, GraphNode, LanguageFilter, NodeId, NodeKind, RarityFilter, Wire,
};
use std::collections::{HashMap, HashSet, VecDeque};

/// Per-node evaluation result: the set of card indices that reach this node, plus any
/// price overrides accumulated from upstream `PriceFloor` nodes.
#[derive(Default)]
pub(super) struct NodeOutput {
    pub indices: Vec<usize>,
    /// Maps card index → effective (floored) price. Only present when a PriceFloor node
    /// raised the card's price above its CSV value.
    pub overrides: HashMap<usize, f64>,
}

/// Evaluates the full graph and returns a `NodeOutput` for every node.
/// Returns an empty map if no cards are loaded.
pub(super) fn evaluate_all(
    nodes: &[GraphNode],
    wires: &[Wire],
    all_cards: &[Card],
) -> HashMap<NodeId, NodeOutput> {
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
    let mut outputs: HashMap<NodeId, NodeOutput> = HashMap::new();

    for id in topo {
        let node = match nodes.iter().find(|n| n.id == id) {
            Some(n) => n,
            None => continue,
        };

        let output = if node.kind.input_count() == 0 {
            NodeOutput {
                indices: filter_indices(&node.kind, all_indices.clone(), all_cards),
                overrides: HashMap::new(),
            }
        } else {
            let inputs: Vec<NodeOutput> = (0..node.kind.input_count())
                .map(|port| {
                    incoming
                        .get(&(id, port))
                        .and_then(|&from| outputs.get(&from))
                        .map(|out| NodeOutput {
                            indices: out.indices.clone(),
                            overrides: out.overrides.clone(),
                        })
                        .unwrap_or_default()
                })
                .collect();

            match &node.kind {
                NodeKind::LogicalAnd => {
                    let mut result = inputs[0].indices.clone();
                    for other in &inputs[1..] {
                        let set: HashSet<usize> = other.indices.iter().copied().collect();
                        result.retain(|i| set.contains(i));
                    }
                    let surviving: HashSet<usize> = result.iter().copied().collect();
                    let overrides = merge_overrides(&inputs, &surviving);
                    NodeOutput {
                        indices: result,
                        overrides,
                    }
                }
                NodeKind::LogicalOr => {
                    let mut seen = HashSet::new();
                    let mut result = Vec::new();
                    for input in &inputs {
                        for &i in &input.indices {
                            if seen.insert(i) {
                                result.push(i);
                            }
                        }
                    }
                    result.sort_unstable();
                    let surviving: HashSet<usize> = result.iter().copied().collect();
                    let overrides = merge_overrides(&inputs, &surviving);
                    NodeOutput {
                        indices: result,
                        overrides,
                    }
                }
                NodeKind::LogicalNot => {
                    let excluded: HashSet<usize> = inputs[0].indices.iter().copied().collect();
                    let result: Vec<usize> = all_indices
                        .iter()
                        .copied()
                        .filter(|i| !excluded.contains(i))
                        .collect();
                    // Cards coming out of NOT didn't flow through any PriceFloor on this path.
                    NodeOutput {
                        indices: result,
                        overrides: HashMap::new(),
                    }
                }
                NodeKind::PriceFloor {
                    common,
                    uncommon,
                    rare,
                    mythic,
                } => {
                    let input = inputs.into_iter().next().unwrap_or_default();
                    let mut overrides = input.overrides;
                    for &idx in &input.indices {
                        let card = &all_cards[idx];
                        let floor = match card.rarity.to_lowercase().as_str() {
                            "common" => *common,
                            "uncommon" => *uncommon,
                            "rare" => *rare,
                            "mythic" => *mythic,
                            _ => 0.0,
                        };
                        let current = overrides
                            .get(&idx)
                            .copied()
                            .unwrap_or_else(|| card.price_f64());
                        if current < floor {
                            overrides.insert(idx, floor);
                        }
                    }
                    NodeOutput {
                        indices: input.indices,
                        overrides,
                    }
                }
                _ => {
                    // Filter nodes: apply filter, then propagate overrides for surviving indices.
                    let input = inputs.into_iter().next().unwrap_or_default();
                    let filtered = filter_indices(&node.kind, input.indices, all_cards);
                    let surviving: HashSet<usize> = filtered.iter().copied().collect();
                    let overrides = input
                        .overrides
                        .into_iter()
                        .filter(|(k, _)| surviving.contains(k))
                        .collect();
                    NodeOutput {
                        indices: filtered,
                        overrides,
                    }
                }
            }
        };

        outputs.insert(id, output);
    }

    outputs
}

/// Merge price overrides from multiple inputs, keeping only entries for `surviving` indices.
/// When two inputs both override the same card, the higher floor wins.
fn merge_overrides(inputs: &[NodeOutput], surviving: &HashSet<usize>) -> HashMap<usize, f64> {
    let mut merged: HashMap<usize, f64> = HashMap::new();
    for input in inputs {
        for (&idx, &price) in &input.overrides {
            if surviving.contains(&idx) {
                let entry = merged.entry(idx).or_insert(price);
                if price > *entry {
                    *entry = price;
                }
            }
        }
    }
    merged
}

#[cfg(test)]
pub(super) fn evaluate_counts(
    nodes: &[GraphNode],
    wires: &[Wire],
    all_cards: &[Card],
) -> HashMap<NodeId, usize> {
    evaluate_all(nodes, wires, all_cards)
        .into_iter()
        .map(|(id, out)| (id, out.indices.len()))
        .collect()
}

/// Apply a node's filtering logic to a set of card indices.
/// Logical nodes (AND/OR/NOT) and PriceFloor are handled in `evaluate_all`; this function
/// is only called for source/sink and filter nodes.
pub(super) fn filter_indices(kind: &NodeKind, indices: Vec<usize>, cards: &[Card]) -> Vec<usize> {
    match kind {
        NodeKind::CsvSource
        | NodeKind::Output
        | NodeKind::LogicalAnd
        | NodeKind::LogicalOr
        | NodeKind::LogicalNot
        | NodeKind::PriceFloor { .. } => indices,

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
