use super::*;

fn card(set: &str, number: &str) -> Option<CardRef> {
    Some(CardRef {
        set_code: set.to_string(),
        collector_number: number.to_string(),
    })
}

#[test]
fn set_code_and_three_digits() {
    assert_eq!(parse_card_input("hou120"), card("hou", "120"));
    assert_eq!(parse_card_input("MH2130"), card("mh2", "130"));
    assert_eq!(parse_card_input("2xm050"), card("2xm", "50"));
    assert_eq!(parse_card_input("  lea005 "), card("lea", "5"));
    assert_eq!(parse_card_input("lea000"), card("lea", "0"));
}

#[test]
fn malformed_input_is_refused() {
    for bad in [
        "",
        "120",
        "hou",
        "hou12a",
        "hou-12",
        "abc",
        "äöü120",
        "hou 120x",
    ] {
        assert_eq!(parse_card_input(bad), None, "{bad:?}");
    }
}

#[test]
fn three_digits_use_the_default_set() {
    assert_eq!(resolve_card_input("120", "HOU"), card("hou", "120"));
    assert_eq!(resolve_card_input("007", " dmu "), card("dmu", "7"));
    // Without a default set three digits are not enough.
    assert_eq!(resolve_card_input("120", ""), None);
    // A full input wins over the default set.
    assert_eq!(resolve_card_input("mh2130", "hou"), card("mh2", "130"));
    assert_eq!(resolve_card_input("   ", "hou"), None);
}
