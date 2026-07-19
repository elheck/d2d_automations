//! Internal consistency check — finds listings that contradict *each other*,
//! independent of any market data:
//!
//! - a worse-condition copy priced above a better-condition copy of the same
//!   variant,
//! - a foil copy priced below its non-foil counterpart,
//! - the same variant listed multiple times at different prices.
//!
//! These are almost always data-entry mistakes; no market-based report can
//! catch them because every individual price may still sit inside the fair
//! band. Pure module: no database or UI access.

use crate::inventory_db::InStockCard;
use crate::models::canonical_condition;
use std::collections::HashMap;

/// What kind of contradiction a pair of listings exhibits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    /// Worse condition priced above a better condition (same product/foil/language).
    ConditionInversion,
    /// Foil priced below the non-foil copy (same product/condition/language).
    FoilInversion,
    /// Identical variant (product/foil/condition/language) listed at
    /// different prices.
    DuplicatePrice,
}

impl IssueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IssueKind::ConditionInversion => "Condition inversion",
            IssueKind::FoilInversion => "Foil below non-foil",
            IssueKind::DuplicatePrice => "Duplicate at different price",
        }
    }
}

/// One detected contradiction between two of our own listings.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsistencyIssue {
    pub kind: IssueKind,
    pub name: String,
    pub set_code: String,
    /// Human-readable description of the two clashing listings
    /// (condition/foil, price, location each).
    pub details: String,
}

/// Rank of a canonical condition on the Cardmarket scale, best first.
/// `None` for unrecognized conditions (excluded from inversion checks).
fn condition_rank(condition: &str) -> Option<u8> {
    match canonical_condition(condition).as_str() {
        "NM" => Some(0),
        "EX" => Some(1),
        "GD" => Some(2),
        "LP" => Some(3),
        "PL" => Some(4),
        "PO" => Some(5),
        _ => None,
    }
}

fn describe(card: &InStockCard) -> String {
    format!(
        "{}{} €{:.2} @ {}",
        canonical_condition(&card.condition),
        if card.is_foil { " ✦" } else { "" },
        card.price,
        card.location
    )
}

/// Scans the inventory for internal contradictions. Results are grouped per
/// product and deduplicated: each clashing pair appears once.
pub fn find_issues(cards: &[InStockCard]) -> Vec<ConsistencyIssue> {
    let mut by_product: HashMap<&str, Vec<&InStockCard>> = HashMap::new();
    for card in cards {
        by_product
            .entry(card.cardmarket_id.as_str())
            .or_default()
            .push(card);
    }

    let mut issues = Vec::new();
    for group in by_product.values() {
        for (i, a) in group.iter().enumerate() {
            for b in group.iter().skip(i + 1) {
                if let Some(kind) = pair_issue(a, b) {
                    // Order the pair so the description reads better/worse
                    // (or non-foil/foil) consistently.
                    let (first, second) = orient_pair(kind, a, b);
                    issues.push(ConsistencyIssue {
                        kind,
                        name: first.name.clone(),
                        set_code: first.set_code.clone(),
                        details: format!("{} vs {}", describe(first), describe(second)),
                    });
                }
            }
        }
    }

    // Stable presentation: by card name, then kind.
    issues.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.details.cmp(&b.details)));
    issues
}

/// Checks one pair of same-product listings for a contradiction.
fn pair_issue(a: &InStockCard, b: &InStockCard) -> Option<IssueKind> {
    let same_language = a.language.eq_ignore_ascii_case(&b.language);
    if !same_language {
        return None; // language differences legitimately change value
    }
    let (rank_a, rank_b) = (condition_rank(&a.condition)?, condition_rank(&b.condition)?);

    if a.is_foil == b.is_foil {
        if rank_a == rank_b {
            // Identical variant: flag only when the prices differ.
            if (a.price - b.price).abs() > 0.005 {
                return Some(IssueKind::DuplicatePrice);
            }
            return None;
        }
        // Worse condition strictly above the better one.
        let (better, worse) = if rank_a < rank_b { (a, b) } else { (b, a) };
        if worse.price > better.price {
            return Some(IssueKind::ConditionInversion);
        }
        return None;
    }

    // Foil vs non-foil: only comparable at equal condition; a foil should not
    // be cheaper than its non-foil counterpart.
    if rank_a == rank_b {
        let (foil, nonfoil) = if a.is_foil { (a, b) } else { (b, a) };
        if foil.price < nonfoil.price {
            return Some(IssueKind::FoilInversion);
        }
    }
    None
}

/// Returns the pair ordered for display: better/cheaper reference first.
fn orient_pair<'c>(
    kind: IssueKind,
    a: &'c InStockCard,
    b: &'c InStockCard,
) -> (&'c InStockCard, &'c InStockCard) {
    match kind {
        IssueKind::ConditionInversion => {
            // Better condition first.
            let (ra, rb) = (
                condition_rank(&a.condition).unwrap_or(u8::MAX),
                condition_rank(&b.condition).unwrap_or(u8::MAX),
            );
            if ra <= rb {
                (a, b)
            } else {
                (b, a)
            }
        }
        IssueKind::FoilInversion => {
            // Non-foil first.
            if a.is_foil {
                (b, a)
            } else {
                (a, b)
            }
        }
        IssueKind::DuplicatePrice => {
            if a.price <= b.price {
                (a, b)
            } else {
                (b, a)
            }
        }
    }
}

#[path = "consistency_tests.rs"]
#[cfg(test)]
mod tests;
