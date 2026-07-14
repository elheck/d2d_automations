//! Bin consolidation — suggest moving cards out of sparsely-filled bins so they
//! can be emptied and reused, and produce a Cardmarket-style update CSV of the
//! resulting location changes.
//!
//! ## Model
//! A physical **bin** is the first four `-`-separated segments of a location
//! (`A-0-0-25`), holding up to [`BIN_CAPACITY`] cards. The remainder of the
//! location string (`L0-R`) encodes the purchase **lot** and shelf side. A move
//! only rewrites the bin coordinates and keeps that lot/side suffix, so the
//! per-lot revenue tracking in [`crate::inventory_db`] is never disturbed.
//!
//! ## Strategy
//! Bins filled at or below a caller-chosen threshold are "sparse". Working from
//! the emptiest sparse bin outward, the planner relocates *all* of that bin's
//! piles into other bins that have room — a bin's contents may be split across
//! several targets. A bin is only emptied when **every** pile can be placed;
//! partial moves that leave a bin occupied add handling work for no benefit and
//! are skipped. A bin that has received cards is never later chosen as a source,
//! so no card is moved twice.
//!
//! Each pile's target is chosen to be, in order of importance:
//! 1. a **keeper** (a bin *not* itself scheduled for emptying) over a sparse bin,
//!    so we don't clog a bin we want to free;
//! 2. a bin that **already holds the same variant**, which de-fragments it;
//! 3. the **closest** bin by physical proximity (weighted aisle/shelf/row/column
//!    distance), to minimise walking;
//! 4. the bin left with the **least free space** (tightest pack);
//! 5. the lowest bin name (deterministic tie-break).
//!
//! All logic here is pure and free of I/O so it can be tested deterministically.

use crate::card_matching::parse_location_code;
use crate::models::Card;
use std::collections::{HashMap, HashSet};

/// Maximum cards a single bin holds. Mirrors `StockAnalysis::BIN_CAPACITY`.
pub const BIN_CAPACITY: i64 = 60;

/// A single suggested relocation of one pile of cards.
#[derive(Debug, Clone)]
pub struct Move {
    pub card: Card,
    pub quantity: i64,
    pub from_location: String,
    pub to_location: String,
    pub from_bin: String,
    pub to_bin: String,
    /// Weighted proximity distance between the source and target bins.
    pub distance: i64,
}

/// The full set of suggested moves plus reporting figures.
#[derive(Debug, Default, Clone)]
pub struct ConsolidationPlan {
    pub moves: Vec<Move>,
    /// Source bins that end up completely emptied by the plan.
    pub bins_freed: Vec<String>,
    /// How many sparse bins were examined as candidates.
    pub source_bins_considered: usize,
    /// Variants currently split across more than one bin (reporting only).
    pub fragmented_variants: usize,
    /// Total copies relocated.
    pub cards_moved: i64,
    /// Sum of every move's proximity distance (lower ⇒ less walking).
    pub total_move_distance: i64,
}

/// Per-axis weights for [`bin_distance`]: coarser axes dominate, so a different
/// aisle always costs more than any number of column steps.
const DISTANCE_WEIGHTS: [i64; 4] = [1000, 100, 10, 1];

/// Weighted Manhattan distance between two bins — a proxy for walking effort.
///
/// Coordinates come from [`parse_location_code`] (aisle, shelf, row, column).
/// Two identical bins have distance 0.
fn bin_distance(a: &str, b: &str) -> i64 {
    let ca = parse_location_code(a);
    let cb = parse_location_code(b);
    let n = ca.len().min(cb.len());
    (0..n)
        .map(|i| {
            let w = DISTANCE_WEIGHTS.get(i).copied().unwrap_or(1);
            (i64::from(ca[i]) - i64::from(cb[i])).abs() * w
        })
        .sum()
}

/// Splits a location into `(bin, lot_side_suffix)`, or `None` if it is not a
/// recognisable bin location (needs ≥4 segments with a numeric 4th segment).
fn split_location(loc: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = loc.split('-').collect();
    if parts.len() >= 4 && parts[3].parse::<i64>().is_ok() {
        let bin = parts[..4].join("-");
        let suffix = parts[4..].join("-");
        Some((bin, suffix))
    } else {
        None
    }
}

/// Rejoins a bin with a lot/side suffix into a full location string.
fn join_location(bin: &str, suffix: &str) -> String {
    if suffix.is_empty() {
        bin.to_string()
    } else {
        format!("{bin}-{suffix}")
    }
}

/// Stable key identifying a unique card variant (same key ⇒ same shelf pile).
fn variant_key(c: &Card) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
        c.cardmarket_id,
        c.condition.trim().to_lowercase(),
        c.language.trim().to_lowercase(),
        c.is_foil_card(),
        c.is_signed_card(),
    )
}

/// A single physical pile of one variant at one location.
struct Placement<'a> {
    card: &'a Card,
    bin: String,
    suffix: String,
    qty: i64,
    variant: String,
}

/// Extracts the valid, in-stock, bin-located piles from a card list. Cards
/// without a recognisable bin location and rows with a non-positive quantity are
/// dropped.
fn build_placements(cards: &[Card]) -> Vec<Placement<'_>> {
    let mut placements = Vec::new();
    for c in cards {
        let Some(loc) = c.location.as_deref().map(str::trim) else {
            continue;
        };
        if loc.is_empty() {
            continue;
        }
        let Some((bin, suffix)) = split_location(loc) else {
            continue;
        };
        let qty = c.quantity.trim().parse::<i64>().unwrap_or(0);
        if qty <= 0 {
            continue;
        }
        placements.push(Placement {
            card: c,
            bin,
            suffix,
            qty,
            variant: variant_key(c),
        });
    }
    placements
}

/// Source-bin processing order for a single greedy pass. The greedy result is
/// order-sensitive, so [`plan_consolidation`] tries all of these and keeps the best.
#[derive(Clone, Copy)]
enum SourceOrder {
    Emptiest,
    FullestSparse,
    FewestPiles,
    MostPiles,
}

/// One greedy consolidation pass over the sparse bins in a fixed `order`.
fn run_greedy(cards: &[Card], max_fill_to_merge: i64, order: SourceOrder) -> ConsolidationPlan {
    // 1. Gather valid placements.
    let placements = build_placements(cards);

    // 2. Current bin loads and which variants each bin holds.
    let mut bin_load: HashMap<String, i64> = HashMap::new();
    let mut bin_variants: HashMap<String, HashSet<String>> = HashMap::new();
    let mut variant_bins: HashMap<String, HashSet<String>> = HashMap::new();
    for p in &placements {
        *bin_load.entry(p.bin.clone()).or_default() += p.qty;
        bin_variants
            .entry(p.bin.clone())
            .or_default()
            .insert(p.variant.clone());
        variant_bins
            .entry(p.variant.clone())
            .or_default()
            .insert(p.bin.clone());
    }
    let fragmented_variants = variant_bins.values().filter(|s| s.len() > 1).count();

    // 3. Sparse source bins, ordered per the chosen strategy (name breaks ties).
    let mut pile_count: HashMap<String, usize> = HashMap::new();
    for p in &placements {
        *pile_count.entry(p.bin.clone()).or_default() += 1;
    }
    let mut sources: Vec<String> = bin_load
        .iter()
        .filter(|(_, &l)| l > 0 && l <= max_fill_to_merge)
        .map(|(b, _)| b.clone())
        .collect();
    sources.sort_by(|a, b| {
        let key = match order {
            SourceOrder::Emptiest => bin_load[a].cmp(&bin_load[b]),
            SourceOrder::FullestSparse => bin_load[b].cmp(&bin_load[a]),
            SourceOrder::FewestPiles => pile_count[a]
                .cmp(&pile_count[b])
                .then_with(|| bin_load[a].cmp(&bin_load[b])),
            SourceOrder::MostPiles => pile_count[b]
                .cmp(&pile_count[a])
                .then_with(|| bin_load[a].cmp(&bin_load[b])),
        };
        key.then_with(|| a.cmp(b))
    });
    // "Keeper" bins are everything not scheduled for emptying; targets prefer them.
    let sparse_set: HashSet<String> = sources.iter().cloned().collect();

    let mut plan = ConsolidationPlan {
        fragmented_variants,
        source_bins_considered: sources.len(),
        ..Default::default()
    };

    let mut current_bin: Vec<String> = placements.iter().map(|p| p.bin.clone()).collect();
    let mut moved = vec![false; placements.len()];
    let mut received: HashSet<String> = HashSet::new();
    let mut freed: HashSet<String> = HashSet::new();

    for source in &sources {
        // A bin that already received cards must not be moved again; and it must
        // still be non-empty and sparse (an earlier pass may have changed it).
        if received.contains(source) {
            continue;
        }
        let load = *bin_load.get(source).unwrap_or(&0);
        if load == 0 || load > max_fill_to_merge {
            continue;
        }

        let src_idxs: Vec<usize> = (0..placements.len())
            .filter(|&i| !moved[i] && &current_bin[i] == source)
            .collect();
        if src_idxs.is_empty() {
            continue;
        }

        // Try to place every pile on a trial copy of the loads; commit only if all fit.
        let mut trial_load = bin_load.clone();
        let mut trial_variants = bin_variants.clone();
        let mut proposed: Vec<(usize, String)> = Vec::new();
        let mut all_placed = true;

        // Largest piles first pack more tightly.
        let mut ordered = src_idxs.clone();
        ordered.sort_by(|&a, &b| placements[b].qty.cmp(&placements[a].qty).then(a.cmp(&b)));

        for &i in &ordered {
            let p = &placements[i];
            let Some(target) =
                pick_target(p, source, &trial_load, &trial_variants, &freed, &sparse_set)
            else {
                all_placed = false;
                break;
            };
            *trial_load.get_mut(&target).unwrap() += p.qty;
            *trial_load.get_mut(source).unwrap() -= p.qty;
            trial_variants
                .entry(target.clone())
                .or_default()
                .insert(p.variant.clone());
            proposed.push((i, target));
        }

        if !all_placed || proposed.is_empty() {
            continue;
        }

        // Commit.
        for (i, target) in proposed {
            let p = &placements[i];
            *bin_load.get_mut(&target).unwrap() += p.qty;
            *bin_load.get_mut(source).unwrap() -= p.qty;
            bin_variants
                .entry(target.clone())
                .or_default()
                .insert(p.variant.clone());
            current_bin[i] = target.clone();
            moved[i] = true;
            received.insert(target.clone());
            plan.cards_moved += p.qty;
            let distance = bin_distance(&p.bin, &target);
            plan.total_move_distance += distance;
            plan.moves.push(Move {
                card: p.card.clone(),
                quantity: p.qty,
                from_location: join_location(&p.bin, &p.suffix),
                to_location: join_location(&target, &p.suffix),
                from_bin: p.bin.clone(),
                to_bin: target,
                distance,
            });
        }
        freed.insert(source.clone());
        plan.bins_freed.push(source.clone());
    }

    // Present moves grouped by source then target for readability.
    plan.moves
        .sort_by(|a, b| a.from_bin.cmp(&b.from_bin).then(a.to_bin.cmp(&b.to_bin)));
    plan.bins_freed.sort();
    plan
}

/// Builds the best consolidation plan for `cards`.
///
/// `max_fill_to_merge` is the highest bin load (in cards) that still counts as
/// "sparse" and therefore a candidate to be emptied. Cards without a valid bin
/// location, and rows with a non-positive quantity, are ignored.
///
/// A single greedy pass is order-sensitive, so the planner runs several
/// source-ordering strategies and returns the best plan by: most bins freed, then
/// least total move distance, then fewest cards moved. Ties keep the earliest
/// strategy, so the result is deterministic. This is a strong heuristic, not a
/// proven global optimum (bin packing is NP-hard).
pub fn plan_consolidation(cards: &[Card], max_fill_to_merge: i64) -> ConsolidationPlan {
    const ORDERS: [SourceOrder; 4] = [
        SourceOrder::Emptiest,
        SourceOrder::FullestSparse,
        SourceOrder::FewestPiles,
        SourceOrder::MostPiles,
    ];
    let mut best: Option<ConsolidationPlan> = None;
    for order in ORDERS {
        let plan = run_greedy(cards, max_fill_to_merge, order);
        let take = match &best {
            None => true,
            Some(b) => is_better(&plan, b),
        };
        if take {
            best = Some(plan);
        }
    }
    best.unwrap_or_default()
}

/// True when `a` is a strictly better plan than `b`: more bins freed, then less
/// walking, then fewer cards moved.
fn is_better(a: &ConsolidationPlan, b: &ConsolidationPlan) -> bool {
    use std::cmp::Ordering::{Equal, Greater, Less};
    match a.bins_freed.len().cmp(&b.bins_freed.len()) {
        Greater => true,
        Less => false,
        Equal => match a.total_move_distance.cmp(&b.total_move_distance) {
            Less => true,
            Greater => false,
            Equal => a.cards_moved < b.cards_moved,
        },
    }
}

/// One card variant that is currently split across more than one bin.
#[derive(Debug, Clone, PartialEq)]
pub struct FragmentedVariant {
    pub name: String,
    pub cardmarket_id: String,
    pub condition: String,
    pub language: String,
    pub is_foil: bool,
    pub total_copies: i64,
    /// `(bin, full location, quantity)` for each pile, sorted by bin.
    pub placements: Vec<(String, String, i64)>,
}

impl FragmentedVariant {
    /// Number of distinct bins this variant is spread across.
    pub fn bin_count(&self) -> usize {
        self.placements
            .iter()
            .map(|(bin, _, _)| bin)
            .collect::<HashSet<_>>()
            .len()
    }
}

/// Finds every variant split across more than one bin — **independent of bin
/// fill**. Sorted most-fragmented first (bin count), then most copies, then name.
pub fn fragmented_variants(cards: &[Card]) -> Vec<FragmentedVariant> {
    #[allow(clippy::type_complexity)]
    let mut map: HashMap<String, (&Card, Vec<(String, String, i64)>, HashSet<String>)> =
        HashMap::new();
    for p in &build_placements(cards) {
        let entry = map
            .entry(p.variant.clone())
            .or_insert_with(|| (p.card, Vec::new(), HashSet::new()));
        entry
            .1
            .push((p.bin.clone(), join_location(&p.bin, &p.suffix), p.qty));
        entry.2.insert(p.bin.clone());
    }

    let mut out: Vec<FragmentedVariant> = map
        .into_values()
        .filter(|(_, _, bins)| bins.len() > 1)
        .map(|(card, mut placements, _)| {
            placements.sort();
            FragmentedVariant {
                name: card.name.clone(),
                cardmarket_id: card.cardmarket_id.clone(),
                condition: card.condition.clone(),
                language: card.language.clone(),
                is_foil: card.is_foil_card(),
                total_copies: placements.iter().map(|(_, _, q)| q).sum(),
                placements,
            }
        })
        .collect();

    out.sort_by(|a, b| {
        b.bin_count()
            .cmp(&a.bin_count())
            .then_with(|| b.total_copies.cmp(&a.total_copies))
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

/// Consolidates every fragmented variant into a **single** bin, independent of
/// bin fill.
///
/// For each variant (most-fragmented first), its scattered piles are gathered
/// into whichever bin it already occupies that minimises total move distance
/// while respecting [`BIN_CAPACITY`]. A variant that cannot fit into any one of
/// its bins is left untouched. Complements [`plan_consolidation`], which instead
/// empties sparse bins to reclaim space.
pub fn plan_variant_defrag(cards: &[Card]) -> ConsolidationPlan {
    let placements = build_placements(cards);

    let mut bin_load: HashMap<String, i64> = HashMap::new();
    for p in &placements {
        *bin_load.entry(p.bin.clone()).or_default() += p.qty;
    }

    let mut variant_piles: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, p) in placements.iter().enumerate() {
        variant_piles.entry(p.variant.clone()).or_default().push(i);
    }

    let bins_of = |idxs: &[usize]| -> HashSet<String> {
        idxs.iter().map(|&i| placements[i].bin.clone()).collect()
    };

    // Fragmented variants only, ordered: most bins, then most copies, then key.
    let mut frags: Vec<(String, Vec<usize>)> = variant_piles
        .into_iter()
        .filter(|(_, idxs)| bins_of(idxs).len() > 1)
        .collect();
    frags.sort_by(|a, b| {
        let copies = |idxs: &[usize]| idxs.iter().map(|&i| placements[i].qty).sum::<i64>();
        bins_of(&b.1)
            .len()
            .cmp(&bins_of(&a.1).len())
            .then_with(|| copies(&b.1).cmp(&copies(&a.1)))
            .then_with(|| a.0.cmp(&b.0))
    });

    let mut plan = ConsolidationPlan {
        fragmented_variants: frags.len(),
        ..Default::default()
    };

    for (_variant, idxs) in &frags {
        // Candidate targets: the bins this variant already occupies.
        let mut occupied: Vec<String> = bins_of(idxs).into_iter().collect();
        occupied.sort();

        // Pick the feasible target with the least total move distance, then the
        // most copies already there (fewest cards to move), then bin name.
        let mut best: Option<(i64, i64, String)> = None;
        for t in &occupied {
            let needed: i64 = idxs
                .iter()
                .filter(|&&i| &placements[i].bin != t)
                .map(|&i| placements[i].qty)
                .sum();
            if bin_load[t] + needed > BIN_CAPACITY {
                continue;
            }
            let dist: i64 = idxs
                .iter()
                .filter(|&&i| &placements[i].bin != t)
                .map(|&i| bin_distance(&placements[i].bin, t))
                .sum();
            let existing: i64 = idxs
                .iter()
                .filter(|&&i| &placements[i].bin == t)
                .map(|&i| placements[i].qty)
                .sum();
            let cand = (dist, -existing, t.clone());
            best = Some(match best {
                Some(cur) if cur <= cand => cur,
                _ => cand,
            });
        }
        let Some((_, _, target)) = best else { continue };

        for &i in idxs {
            let p = &placements[i];
            if p.bin == target {
                continue;
            }
            let distance = bin_distance(&p.bin, &target);
            *bin_load.get_mut(&target).unwrap() += p.qty;
            if let Some(l) = bin_load.get_mut(&p.bin) {
                *l -= p.qty;
            }
            plan.cards_moved += p.qty;
            plan.total_move_distance += distance;
            plan.moves.push(Move {
                card: p.card.clone(),
                quantity: p.qty,
                from_location: join_location(&p.bin, &p.suffix),
                to_location: join_location(&target, &p.suffix),
                from_bin: p.bin.clone(),
                to_bin: target.clone(),
                distance,
            });
        }
    }

    // Any bin emptied entirely by the moves is freed.
    let mut freed: Vec<String> = bin_load
        .iter()
        .filter(|(_, &l)| l == 0)
        .map(|(b, _)| b.clone())
        .collect();
    freed.sort();
    plan.bins_freed = freed;

    plan.moves
        .sort_by(|a, b| a.from_bin.cmp(&b.from_bin).then(a.to_bin.cmp(&b.to_bin)));
    plan
}

/// Chooses the best target bin for a pile, or `None` if none has room.
///
/// Candidates are every bin other than the source that is not already emptied
/// and can fit the pile. They are ranked by the lexicographic cost
/// `(is_sparse, not_same_variant, distance, remaining_after, name)` — a total
/// order (bin names are unique), so the result is deterministic regardless of
/// hash-map iteration order. See the module docs for what each factor means.
fn pick_target(
    p: &Placement,
    source: &str,
    trial_load: &HashMap<String, i64>,
    trial_variants: &HashMap<String, HashSet<String>>,
    freed: &HashSet<String>,
    sparse_set: &HashSet<String>,
) -> Option<String> {
    trial_load
        .iter()
        .filter(|(bin, &load)| {
            bin.as_str() != source && !freed.contains(*bin) && load + p.qty <= BIN_CAPACITY
        })
        .map(|(bin, &load)| {
            let is_sparse = u8::from(sparse_set.contains(bin)); // 0 keeper, 1 sparse
            let not_same_variant = u8::from(
                !trial_variants
                    .get(bin)
                    .is_some_and(|s| s.contains(&p.variant)),
            );
            let distance = bin_distance(source, bin);
            let remaining_after = BIN_CAPACITY - (load + p.qty);
            (
                is_sparse,
                not_same_variant,
                distance,
                remaining_after,
                bin.clone(),
            )
        })
        .min()
        .map(|t| t.4)
}

/// Serialises the moves into a Cardmarket-style stock-update CSV.
///
/// Each row carries the card's identity, its quantity, and the **new** location,
/// so importing the file relocates those copies. Quantities are positive — this
/// updates listings in place, it does not remove stock.
pub fn to_update_csv(moves: &[Move]) -> String {
    use csv::WriterBuilder;

    let mut wtr = WriterBuilder::new().has_headers(true).from_writer(vec![]);
    let _ = wtr.write_record([
        "cardmarketId",
        "quantity",
        "name",
        "set",
        "setCode",
        "cn",
        "condition",
        "language",
        "isFoil",
        "isSigned",
        "isFirstEd",
        "isReverseHolo",
        "price",
        "comment",
        "location",
        "rarity",
    ]);

    for m in moves {
        let c = &m.card;
        let qty = m.quantity.to_string();
        let _ = wtr.write_record([
            &c.cardmarket_id,
            &qty,
            &c.name,
            &c.set,
            &c.set_code,
            &c.cn,
            &c.condition,
            &c.language,
            &c.is_foil,
            &c.is_signed,
            c.is_first_ed.as_deref().unwrap_or("false"),
            c.is_reverse_holo.as_deref().unwrap_or("false"),
            &c.price,
            &c.comment,
            &m.to_location,
            &c.rarity,
        ]);
    }

    String::from_utf8(wtr.into_inner().unwrap()).unwrap()
}

#[path = "bin_consolidation_tests.rs"]
#[cfg(test)]
mod tests;
