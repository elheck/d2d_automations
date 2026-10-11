use super::*;

/// Builds a card row with only the fields category detection looks at.
fn card(set: &str, set_code: &str, cn: &str, rarity: &str) -> Card {
    Card {
        cardmarket_id: "1".to_string(),
        quantity: "1".to_string(),
        name: "Thing".to_string(),
        set: set.to_string(),
        set_code: set_code.to_string(),
        cn: cn.to_string(),
        condition: "NM".to_string(),
        language: "English".to_string(),
        is_foil: String::new(),
        is_playset: None,
        is_signed: String::new(),
        is_first_ed: None,
        is_reverse_holo: None,
        price: "1.0".to_string(),
        comment: String::new(),
        location: None,
        name_de: String::new(),
        name_es: String::new(),
        name_fr: String::new(),
        name_it: String::new(),
        rarity: rarity.to_string(),
        listed_at: String::new(),
    }
}

fn magic_card() -> Card {
    card("Alpha", "LEA", "233", "Rare")
}

fn generic_card() -> Card {
    card("", "", "", "")
}

#[test]
fn filename_magic_game_id_maps_to_magic() {
    assert_eq!(detect_category("inventory-report-45.csv", &[]), "Magic");
}

#[test]
fn filename_generic_suffix_maps_to_generic() {
    assert_eq!(
        detect_category("inventory-report-Generic.csv", &[]),
        "Generic"
    );
}

#[test]
fn filename_detection_wins_over_row_contents() {
    // A Magic export whose rows happen to lack metadata must still be scoped by
    // its filename — content inference is only the fallback.
    assert_eq!(
        detect_category("inventory-report-45.csv", &[generic_card()]),
        "Magic"
    );
}

#[test]
fn filename_detection_handles_full_paths() {
    assert_eq!(
        detect_category("/home/aron/Downloads/inventory-report-Generic.csv", &[]),
        "Generic"
    );
}

#[test]
fn filename_suffix_case_is_normalized() {
    // Cardmarket's casing has varied; all spellings must land in one scope,
    // otherwise a re-export would create a parallel set of rows.
    for name in [
        "inventory-report-generic.csv",
        "inventory-report-GENERIC.csv",
        "inventory-report-Generic.csv",
    ] {
        assert_eq!(detect_category(name, &[]), "Generic", "for {name}");
    }
}

#[test]
fn underscore_filename_variant_is_recognized() {
    assert_eq!(
        detect_category("inventory_report_Generic.csv", &[]),
        "Generic"
    );
}

#[test]
fn known_non_magic_game_ids_get_their_own_scope() {
    assert_eq!(detect_category("inventory-report-3.csv", &[]), "Pokemon");
    assert_eq!(detect_category("inventory-report-10.csv", &[]), "Lorcana");
}

#[test]
fn unknown_numeric_suffix_is_decided_by_rows() {
    // `inventory-report-5173.csv` is a plain Magic export: the number is not a
    // game ID, so it must not open a category of its own.
    assert_eq!(
        detect_category("inventory-report-5173.csv", &[magic_card()]),
        "Magic"
    );
    assert_eq!(
        detect_category("inventory-report-5173.csv", &[generic_card()]),
        "Generic"
    );
    assert_eq!(detect_category("inventory-report-5173.csv", &[]), "Magic");
}

#[test]
fn duplicate_download_suffix_does_not_open_a_new_category() {
    for name in [
        "inventory-report-Magic (1).csv",
        "inventory-report-Magic(2).csv",
        "inventory-report-Magic (1) (2).csv",
        "inventory-report-Magic - Copy.csv",
        "inventory-report-magic copy.csv",
        "inventory-report-45 (3).csv",
    ] {
        assert_eq!(detect_category(name, &[]), "Magic", "{name}");
    }
    assert_eq!(
        detect_category("inventory-report-Generic (1).csv", &[magic_card()]),
        "Generic"
    );
}

#[test]
fn canonical_category_folds_legacy_names() {
    assert_eq!(
        canonical_category("Magic (1)", true).as_deref(),
        Some("Magic")
    );
    assert_eq!(
        canonical_category("Generic (1)", false).as_deref(),
        Some("Generic")
    );
    // Legacy synthetic scopes carry no information; the rows decide.
    assert_eq!(
        canonical_category("Game5173", true).as_deref(),
        Some("Magic")
    );
    assert_eq!(
        canonical_category("Game5173", false).as_deref(),
        Some("Generic")
    );
    assert_eq!(
        canonical_category("Game3", true).as_deref(),
        Some("Pokemon")
    );
}

#[test]
fn canonical_category_leaves_canonical_names_alone() {
    for name in ["Magic", "Generic", "Pokemon", "Gamers"] {
        assert_eq!(canonical_category(name, true), None, "{name}");
    }
}

#[test]
fn rows_without_card_metadata_infer_generic() {
    // Filename carries no recognisable suffix, so the rows decide.
    assert_eq!(
        detect_category("my-export.csv", &[generic_card(), generic_card()]),
        "Generic"
    );
}

#[test]
fn rows_with_card_metadata_infer_default_category() {
    assert_eq!(detect_category("my-export.csv", &[magic_card()]), "Magic");
}

#[test]
fn a_single_row_with_metadata_is_enough_to_stay_magic() {
    // Accessories can be listed inside a Magic export; one real card row means
    // the file is not a Generic export.
    let cards = vec![generic_card(), generic_card(), magic_card()];
    assert_eq!(detect_category("my-export.csv", &cards), "Magic");
}

#[test]
fn each_metadata_field_alone_signals_a_card_export() {
    for c in [
        card("Alpha", "", "", ""),
        card("", "LEA", "", ""),
        card("", "", "233", ""),
        card("", "", "", "Rare"),
    ] {
        assert_eq!(detect_category("my-export.csv", &[c]), DEFAULT_CATEGORY);
    }
}

#[test]
fn whitespace_only_metadata_counts_as_missing() {
    let cards = vec![card("  ", " ", "", "  ")];
    assert_eq!(detect_category("my-export.csv", &cards), "Generic");
}

#[test]
fn empty_csv_falls_back_to_default_category() {
    // Nothing to infer from, and no rows to sync either — the default keeps
    // behaviour unchanged for existing setups.
    assert_eq!(detect_category("my-export.csv", &[]), DEFAULT_CATEGORY);
}

#[test]
fn report_prefix_without_suffix_falls_through_to_rows() {
    assert_eq!(
        detect_category("inventory-report-.csv", &[generic_card()]),
        "Generic"
    );
}
