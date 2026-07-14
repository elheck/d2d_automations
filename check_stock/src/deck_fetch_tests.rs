use super::*;

fn entry(quantity: i32, name: &str) -> WantsEntry {
    WantsEntry {
        quantity,
        name: name.to_string(),
    }
}

// ==================== parse_deck_url ====================

#[test]
fn recognises_moxfield_url_preserving_case() {
    assert_eq!(
        parse_deck_url("https://moxfield.com/decks/aeorrmvviUiIVihw0bcL3A"),
        Some(DeckSource::Moxfield("aeorrmvviUiIVihw0bcL3A".to_string()))
    );
}

#[test]
fn recognises_moxfield_url_with_www_and_trailing_path() {
    assert_eq!(
        parse_deck_url("https://www.moxfield.com/decks/abc123/primer?utm=x"),
        Some(DeckSource::Moxfield("abc123".to_string()))
    );
}

#[test]
fn recognises_archidekt_url_numeric_id() {
    assert_eq!(
        parse_deck_url("https://archidekt.com/decks/1234567/my_sweet_deck"),
        Some(DeckSource::Archidekt("1234567".to_string()))
    );
}

#[test]
fn plain_file_path_is_not_a_url() {
    assert_eq!(parse_deck_url("/home/user/deck.txt"), None);
    assert_eq!(parse_deck_url("C:\\decks\\deck.txt"), None);
    assert_eq!(parse_deck_url("4 Lightning Bolt"), None);
}

#[test]
fn unknown_host_is_not_recognised() {
    assert_eq!(parse_deck_url("https://example.com/decks/123"), None);
}

#[test]
fn url_without_id_is_none() {
    assert_eq!(parse_deck_url("https://moxfield.com/decks/"), None);
}

// ==================== parse_moxfield_json ====================

#[test]
fn moxfield_v2_collects_main_side_and_commanders() {
    let body = r#"{
        "mainboard": {
            "Lightning Bolt": { "quantity": 4, "card": { "name": "Lightning Bolt" } },
            "Mountain":       { "quantity": 20, "card": { "name": "Mountain" } }
        },
        "sideboard": {
            "Smash to Smithereens": { "quantity": 2, "card": { "name": "Smash to Smithereens" } }
        },
        "commanders": {
            "Krenko, Mob Boss": { "quantity": 1, "card": { "name": "Krenko, Mob Boss" } }
        },
        "maybeboard": {
            "Shock": { "quantity": 9, "card": { "name": "Shock" } }
        }
    }"#;
    let mut got = parse_moxfield_json(body).unwrap();
    got.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(
        got,
        vec![
            entry(1, "Krenko, Mob Boss"),
            entry(4, "Lightning Bolt"),
            entry(20, "Mountain"),
            entry(2, "Smash to Smithereens"),
        ]
    );
    // Maybeboard excluded.
    assert!(got.iter().all(|e| e.name != "Shock"));
}

#[test]
fn moxfield_v3_boards_shape() {
    let body = r#"{
        "boards": {
            "mainboard": {
                "count": 1,
                "cards": {
                    "id1": { "quantity": 3, "card": { "name": "Llanowar Elves" } }
                }
            }
        }
    }"#;
    assert_eq!(
        parse_moxfield_json(body).unwrap(),
        vec![entry(3, "Llanowar Elves")]
    );
}

#[test]
fn moxfield_merges_across_boards() {
    // Same card in main and side must sum.
    let body = r#"{
        "mainboard": { "Bolt": { "quantity": 4, "card": { "name": "Lightning Bolt" } } },
        "sideboard": { "Bolt": { "quantity": 1, "card": { "name": "Lightning Bolt" } } }
    }"#;
    let got = parse_moxfield_json(body).unwrap();
    assert_eq!(got, vec![entry(5, "Lightning Bolt")]);
}

#[test]
fn moxfield_falls_back_to_key_when_card_name_missing() {
    let body = r#"{ "mainboard": { "Opt": { "quantity": 2 } } }"#;
    assert_eq!(parse_moxfield_json(body).unwrap(), vec![entry(2, "Opt")]);
}

#[test]
fn moxfield_empty_deck_errors() {
    assert!(parse_moxfield_json(r#"{ "mainboard": {} }"#).is_err());
}

#[test]
fn moxfield_invalid_json_errors() {
    assert!(parse_moxfield_json("not json").is_err());
}

// ==================== parse_archidekt_json ====================

#[test]
fn archidekt_collects_cards_with_oracle_name() {
    let body = r#"{
        "cards": [
            { "quantity": 4, "card": { "oracleCard": { "name": "Lightning Bolt" } }, "categories": ["Removal"] },
            { "quantity": 1, "card": { "oracleCard": { "name": "Sol Ring" } }, "categories": [] }
        ]
    }"#;
    assert_eq!(
        parse_archidekt_json(body).unwrap(),
        vec![entry(4, "Lightning Bolt"), entry(1, "Sol Ring")]
    );
}

#[test]
fn archidekt_skips_maybeboard_cards() {
    let body = r#"{
        "cards": [
            { "quantity": 1, "card": { "oracleCard": { "name": "Keep Me" } }, "categories": ["Ramp"] },
            { "quantity": 9, "card": { "oracleCard": { "name": "Skip Me" } }, "categories": ["Maybeboard"] }
        ]
    }"#;
    let got = parse_archidekt_json(body).unwrap();
    assert_eq!(got, vec![entry(1, "Keep Me")]);
}

#[test]
fn archidekt_merges_duplicates() {
    let body = r#"{
        "cards": [
            { "quantity": 2, "card": { "oracleCard": { "name": "Forest" } }, "categories": [] },
            { "quantity": 3, "card": { "oracleCard": { "name": "Forest" } }, "categories": [] }
        ]
    }"#;
    assert_eq!(
        parse_archidekt_json(body).unwrap(),
        vec![entry(5, "Forest")]
    );
}

#[test]
fn archidekt_missing_cards_array_errors() {
    assert!(parse_archidekt_json(r#"{ "name": "x" }"#).is_err());
}
