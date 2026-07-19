use super::*;

fn card(
    id: &str,
    condition: &str,
    is_foil: bool,
    language: &str,
    price: f64,
    location: &str,
) -> InStockCard {
    InStockCard {
        cardmarket_id: id.to_string(),
        name: format!("Card {id}"),
        set_code: "TST".to_string(),
        cn: "1".to_string(),
        condition: condition.to_string(),
        language: language.to_string(),
        is_foil,
        rarity: "rare".to_string(),
        quantity: 1,
        price,
        location: location.to_string(),
        effective_date: "2026-01-01".to_string(),
    }
}

#[test]
fn condition_inversion_flagged() {
    // PL copy priced above the NM copy of the same non-foil variant.
    let cards = vec![
        card("1", "NM", false, "English", 2.0, "A1"),
        card("1", "PL", false, "English", 3.0, "A2"),
    ];
    let issues = find_issues(&cards);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].kind, IssueKind::ConditionInversion);
    // Better condition is described first.
    assert!(issues[0].details.starts_with("NM"));
}

#[test]
fn worse_condition_cheaper_is_fine() {
    let cards = vec![
        card("1", "NM", false, "English", 3.0, "A1"),
        card("1", "PL", false, "English", 1.0, "A2"),
    ];
    assert!(find_issues(&cards).is_empty());
}

#[test]
fn foil_below_nonfoil_flagged() {
    let cards = vec![
        card("1", "NM", false, "English", 5.0, "A1"),
        card("1", "NM", true, "English", 3.0, "A2"),
    ];
    let issues = find_issues(&cards);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].kind, IssueKind::FoilInversion);
}

#[test]
fn foil_above_nonfoil_is_fine() {
    let cards = vec![
        card("1", "NM", false, "English", 5.0, "A1"),
        card("1", "NM", true, "English", 9.0, "A2"),
    ];
    assert!(find_issues(&cards).is_empty());
}

#[test]
fn foil_comparison_needs_equal_condition() {
    // A played foil below a NM non-foil is not a contradiction.
    let cards = vec![
        card("1", "NM", false, "English", 5.0, "A1"),
        card("1", "PL", true, "English", 3.0, "A2"),
    ];
    assert!(find_issues(&cards).is_empty());
}

#[test]
fn duplicate_variant_with_different_price_flagged() {
    let cards = vec![
        card("1", "NM", false, "English", 2.0, "A1"),
        card("1", "NM", false, "English", 4.0, "B3"),
    ];
    let issues = find_issues(&cards);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].kind, IssueKind::DuplicatePrice);
    // Cheaper listing described first.
    assert!(issues[0].details.contains("€2.00 @ A1 vs"));
}

#[test]
fn duplicate_variant_same_price_not_flagged() {
    // Same price in two bins is a consolidation matter, not a pricing error.
    let cards = vec![
        card("1", "NM", false, "English", 2.0, "A1"),
        card("1", "NM", false, "English", 2.0, "B3"),
    ];
    assert!(find_issues(&cards).is_empty());
}

#[test]
fn different_languages_never_clash() {
    // A Japanese PL copy above an English NM copy is allowed — language
    // premiums legitimately reorder prices.
    let cards = vec![
        card("1", "NM", false, "English", 2.0, "A1"),
        card("1", "PL", false, "Japanese", 5.0, "A2"),
        card("1", "NM", false, "French", 1.0, "A3"),
    ];
    assert!(find_issues(&cards).is_empty());

    // But two copies in the *same* non-EN language still clash normally.
    let cards = vec![
        card("1", "NM", false, "Japanese", 1.0, "A1"),
        card("1", "PL", false, "Japanese", 5.0, "A2"),
    ];
    assert_eq!(find_issues(&cards).len(), 1);
}

#[test]
fn different_products_never_clash() {
    let cards = vec![
        card("1", "NM", false, "English", 2.0, "A1"),
        card("2", "PL", false, "English", 9.0, "A2"),
    ];
    assert!(find_issues(&cards).is_empty());
}

#[test]
fn unknown_condition_is_skipped() {
    let cards = vec![
        card("1", "NM", false, "English", 2.0, "A1"),
        card("1", "wat", false, "English", 9.0, "A2"),
    ];
    assert!(find_issues(&cards).is_empty());
}

#[test]
fn long_form_conditions_are_canonicalized() {
    // Inventory-report exports use snake_case long forms.
    let cards = vec![
        card("1", "near_mint", false, "English", 2.0, "A1"),
        card("1", "played", false, "English", 3.0, "A2"),
    ];
    let issues = find_issues(&cards);
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].kind, IssueKind::ConditionInversion);
}

#[test]
fn issues_sorted_by_name() {
    let mut a = card("2", "NM", false, "English", 2.0, "A1");
    a.name = "Zebra".to_string();
    let mut b = card("2", "PL", false, "English", 3.0, "A2");
    b.name = "Zebra".to_string();
    let mut c = card("1", "NM", false, "English", 5.0, "B1");
    c.name = "Aardvark".to_string();
    let mut d = card("1", "NM", true, "English", 1.0, "B2");
    d.name = "Aardvark".to_string();
    let issues = find_issues(&[a, b, c, d]);
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].name, "Aardvark");
    assert_eq!(issues[1].name, "Zebra");
}
