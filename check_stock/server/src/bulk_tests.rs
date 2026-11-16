use super::*;

fn view() -> SpecView {
    SpecView::from(&LotSpec::default())
}

#[test]
fn the_default_spec_round_trips() {
    let spec = view().to_spec().unwrap();
    assert_eq!(spec, LotSpec::default());
}

/// A change to the rules and the message it must give.
type Case = (fn(&mut SpecView), &'static str);

#[test]
fn every_rule_is_checked() {
    let cases: Vec<Case> = vec![
        (|v| v.size = 0, "The lot size must be 1 to 1,000 copies"),
        (|v| v.size = 1001, "The lot size must be 1 to 1,000 copies"),
        (|v| v.rarities.clear(), "Choose at least one rarity"),
        (
            |v| v.excluded_conditions = vec!["MINT".into()],
            "Unknown condition",
        ),
        (
            |v| v.excluded_languages = vec![String::new()],
            "Too many or too long languages",
        ),
        (
            |v| v.excluded_languages = vec!["x".repeat(41)],
            "Too many or too long languages",
        ),
        (
            |v| v.excluded_languages = vec!["German".into(); 21],
            "Too many or too long languages",
        ),
        (
            |v| v.max_price = -0.01,
            "The price per card must be 0 to 10,000 €",
        ),
        (
            |v| v.max_price = f64::NAN,
            "The price per card must be 0 to 10,000 €",
        ),
        (
            |v| v.min_age_days = 3651,
            "The time in stock must be 0 to 3,650 days",
        ),
        (
            |v| v.max_copies_per_name = 0,
            "Copies per card name must be 1 to 1,000",
        ),
        (
            |v| v.hits = v.size + 1,
            "More guaranteed hits than cards in the lot",
        ),
        (
            |v| v.hit_min_price = f64::INFINITY,
            "The hit price must be 0 to 100,000 €",
        ),
    ];
    for (change, message) in cases {
        let mut v = view();
        change(&mut v);
        assert_eq!(v.to_spec().unwrap_err(), message);
    }
    let mut ok = view();
    ok.excluded_conditions = vec!["PL".into(), "PO".into()];
    ok.excluded_languages = vec!["Japanese".into()];
    ok.order = OrderKey::Random;
    ok.card_type = CardTypeKey::NonBasicLand;
    ok.foil = FoilKey::Any;
    ok.seed = 7;
    let spec = ok.to_spec().unwrap();
    assert_eq!(SpecView::from(&spec), ok);
}
