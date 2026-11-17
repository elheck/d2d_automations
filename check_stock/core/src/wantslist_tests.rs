use super::*;

fn entry(quantity: i32, name: &str) -> WantsEntry {
    WantsEntry {
        quantity,
        name: name.to_string(),
    }
}

// ==================== parse_line: quantity formats ====================

#[test]
fn plain_quantity_and_name() {
    assert_eq!(
        parse_line("4 Lightning Bolt"),
        ParsedLine::Entry {
            quantity: 4,
            name: "Lightning Bolt".to_string()
        }
    );
}

#[test]
fn x_suffix_lowercase_and_uppercase() {
    assert_eq!(
        parse_line("4x Lightning Bolt"),
        ParsedLine::Entry {
            quantity: 4,
            name: "Lightning Bolt".to_string()
        }
    );
    assert_eq!(
        parse_line("1X Sol Ring"),
        ParsedLine::Entry {
            quantity: 1,
            name: "Sol Ring".to_string()
        }
    );
}

#[test]
fn multi_digit_quantity() {
    assert_eq!(
        parse_line("12 Forest"),
        ParsedLine::Entry {
            quantity: 12,
            name: "Forest".to_string()
        }
    );
}

// ==================== parse_line: annotation stripping ====================

#[test]
fn arena_set_and_collector_stripped() {
    assert_eq!(
        parse_line("4 Lightning Bolt (2XM) 123"),
        ParsedLine::Entry {
            quantity: 4,
            name: "Lightning Bolt".to_string()
        }
    );
}

#[test]
fn moxfield_foil_marker_stripped() {
    assert_eq!(
        parse_line("1 Lightning Bolt (2XM) 123 *F*"),
        ParsedLine::Entry {
            quantity: 1,
            name: "Lightning Bolt".to_string()
        }
    );
    // Etched marker too.
    assert_eq!(
        parse_line("1 Mishra's Factory (ATQ) 80 *E*"),
        ParsedLine::Entry {
            quantity: 1,
            name: "Mishra's Factory".to_string()
        }
    );
}

#[test]
fn archidekt_category_and_tags_stripped() {
    assert_eq!(
        parse_line("1x Lightning Bolt (2XM) 123 [Removal] ^Have,#7bb662^"),
        ParsedLine::Entry {
            quantity: 1,
            name: "Lightning Bolt".to_string()
        }
    );
}

#[test]
fn mtggoldfish_sideboard_prefix_stripped() {
    assert_eq!(
        parse_line("SB: 2 Lightning Bolt"),
        ParsedLine::Entry {
            quantity: 2,
            name: "Lightning Bolt".to_string()
        }
    );
}

#[test]
fn set_code_with_digits_and_dashed_collector() {
    // "The List" style collector numbers with a dash.
    assert_eq!(
        parse_line("1 Counterspell (PLST) MMQ-77"),
        ParsedLine::Entry {
            quantity: 1,
            name: "Counterspell".to_string()
        }
    );
}

#[test]
fn name_with_hyphen_preserved() {
    assert_eq!(
        parse_line("1 Fable of the Mirror-Breaker (NEO) 141"),
        ParsedLine::Entry {
            quantity: 1,
            name: "Fable of the Mirror-Breaker".to_string()
        }
    );
}

#[test]
fn name_with_apostrophe_and_comma_preserved() {
    assert_eq!(
        parse_line("2 Urza's Saga"),
        ParsedLine::Entry {
            quantity: 2,
            name: "Urza's Saga".to_string()
        }
    );
}

#[test]
fn genuine_parenthetical_name_with_spaces_not_stripped() {
    // Paren content has spaces, so it is not a set code and must be kept.
    assert_eq!(
        parse_line("1 B.F.M. (Big Furry Monster)"),
        ParsedLine::Entry {
            quantity: 1,
            name: "B.F.M. (Big Furry Monster)".to_string()
        }
    );
}

// ==================== parse_line: skips & rejects ====================

#[test]
fn blank_line_skipped() {
    assert_eq!(parse_line(""), ParsedLine::Skip);
    assert_eq!(parse_line("   "), ParsedLine::Skip);
}

#[test]
fn headers_skipped_case_insensitively() {
    for h in ["Deck", "SIDEBOARD", "Commander", "About", "Maybeboard"] {
        assert_eq!(parse_line(h), ParsedLine::Skip, "header {h} should skip");
    }
}

#[test]
fn arena_name_preamble_skipped() {
    assert_eq!(parse_line("Name My Sweet Deck"), ParsedLine::Skip);
}

#[test]
fn comment_lines_skipped() {
    assert_eq!(parse_line("// maindeck"), ParsedLine::Skip);
    assert_eq!(parse_line("# a note"), ParsedLine::Skip);
}

#[test]
fn line_without_quantity_is_unparseable() {
    assert_eq!(parse_line("Lightning Bolt"), ParsedLine::Unparseable);
}

#[test]
fn quantity_only_is_unparseable() {
    assert_eq!(parse_line("60"), ParsedLine::Unparseable);
}

// ==================== parse_wantslist: whole documents ====================

#[test]
fn parses_arena_export_with_sections() {
    let doc = "About\nName Mono Red\n\nDeck\n4 Lightning Bolt (2XM) 123\n20 Mountain (2XM) 272\n\nSideboard\n2 Smash to Smithereens (BRO) 137\n";
    let parsed = parse_wantslist(doc);
    assert_eq!(
        parsed.entries,
        vec![
            entry(4, "Lightning Bolt"),
            entry(20, "Mountain"),
            entry(2, "Smash to Smithereens"),
        ]
    );
    assert!(parsed.unparseable.is_empty());
}

#[test]
fn parses_moxfield_export() {
    let doc = "1 Lightning Bolt (2XM) 123 *F*\n1 Sol Ring (LTC) 284\n";
    let parsed = parse_wantslist(doc);
    assert_eq!(
        parsed.entries,
        vec![entry(1, "Lightning Bolt"), entry(1, "Sol Ring")]
    );
}

#[test]
fn duplicate_names_are_merged_summing_quantities() {
    // Same card in maindeck and sideboard, different annotations/casing.
    let doc = "4 Lightning Bolt (2XM) 123\nSB: 1 lightning bolt\n";
    let parsed = parse_wantslist(doc);
    assert_eq!(parsed.entries.len(), 1);
    assert_eq!(parsed.entries[0].quantity, 5);
    // First-seen spelling is preserved.
    assert_eq!(parsed.entries[0].name, "Lightning Bolt");
}

#[test]
fn unparseable_lines_collected() {
    let doc = "4 Lightning Bolt\nthis is not a card\n\n3 Shock\n";
    let parsed = parse_wantslist(doc);
    assert_eq!(
        parsed.entries,
        vec![entry(4, "Lightning Bolt"), entry(3, "Shock")]
    );
    assert_eq!(parsed.unparseable, vec!["this is not a card".to_string()]);
}

#[test]
fn plain_legacy_format_still_works() {
    // The original supported format must keep working.
    let doc = "Deck\n2 Black Lotus\n1 Ancestral Recall\n";
    let parsed = parse_wantslist(doc);
    assert_eq!(
        parsed.entries,
        vec![entry(2, "Black Lotus"), entry(1, "Ancestral Recall")]
    );
}

#[test]
fn empty_document_yields_nothing() {
    assert_eq!(parse_wantslist(""), WantslistParse::default());
}
