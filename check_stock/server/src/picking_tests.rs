use super::*;

fn item(pile: &str, location: &str, quantity: i64) -> PickItem {
    PickItem {
        pile: pile.into(),
        quantity,
        picked: false,
        name: format!("Card {pile}"),
        set_name: "Test Set".into(),
        set_code: "TST".into(),
        collector_number: "1".into(),
        condition: "NM".into(),
        language: "English".into(),
        foil: false,
        location: location.into(),
        price: 1.0,
    }
}

#[test]
fn piles_are_merged_and_sorted_in_shelf_order() {
    let items = prepare(vec![
        item("c", "A-0-0-10-L1-R", 1),
        item("a", "A-0-0-9-L1-R", 1),
        item("b", "B-0-0-1", 2),
        item("a", "A-0-0-9-L1-R", 2),
    ]);
    let order: Vec<(&str, i64)> = items
        .iter()
        .map(|i| (i.pile.as_str(), i.quantity))
        .collect();
    assert_eq!(order, [("a", 3), ("c", 1), ("b", 2)]);
}
