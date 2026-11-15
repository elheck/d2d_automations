use super::*;

fn card() -> Card {
    Card {
        name: "Lightning Bolt".into(),
        set: "Magic 2010".into(),
        condition: "NM".into(),
        language: "German".into(),
        rarity: "Common".into(),
        price: "1.50".into(),
        comment: "Signed by Christopher".into(),
        location: Some("A-0-1-5-L2-R".into()),
        name_de: "Blitzschlag".into(),
        ..Card::test_default()
    }
}

fn matches(term: &str, options: SearchOptions) -> bool {
    card_matches(&card(), &prepare_term(term, &options), &options)
}

#[test]
fn name_and_set_by_default_ignoring_case() {
    let o = SearchOptions::default();
    assert!(matches("bolt", o));
    assert!(matches("MAGIC 2010", o));
    assert!(!matches("german", o), "language is not searched by default");
    assert!(matches("", o), "an empty search shows everything");
}

#[test]
fn case_sensitive_search() {
    let o = SearchOptions {
        case_sensitive: true,
        ..SearchOptions::default()
    };
    assert!(matches("Bolt", o));
    assert!(!matches("bolt", o));
}

#[test]
fn each_field_only_when_chosen() {
    let only = |set: fn(&mut SearchFields)| {
        let mut fields = SearchFields {
            name: false,
            set: false,
            ..SearchFields::default()
        };
        set(&mut fields);
        SearchOptions {
            fields,
            ..SearchOptions::default()
        }
    };
    assert!(matches("a-0-1-5", only(|f| f.location = true)));
    assert!(!matches("a-0-1-5", only(|f| f.name = true)));
    assert!(matches("nm", only(|f| f.condition = true)));
    assert!(matches("german", only(|f| f.language = true)));
    assert!(matches("common", only(|f| f.rarity = true)));
    assert!(matches("1.5", only(|f| f.price = true)));
    assert!(matches("christopher", only(|f| f.comment = true)));
    assert!(!matches("bolt", only(|f| f.comment = true)));
}

#[test]
fn translated_names_only_with_all_languages() {
    let o = SearchOptions::default();
    assert!(!matches("blitz", o));
    assert!(matches(
        "blitz",
        SearchOptions {
            all_languages: true,
            ..o
        }
    ));
}
