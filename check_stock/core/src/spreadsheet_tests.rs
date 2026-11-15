use super::*;

#[test]
fn plain_text_is_unchanged() {
    for text in [
        "Lightning Bolt",
        "",
        "Fire // Ice",
        "100 TCG Guru Sleeves",
        "Æther Vial",
        "#1",
        "(sic)",
    ] {
        assert!(
            matches!(spreadsheet_safe(text), Cow::Borrowed(_)),
            "{text:?}"
        );
    }
}

#[test]
fn formula_starts_become_text() {
    for (text, safe) in [
        ("=HYPERLINK(\"http://x\")", "'=HYPERLINK(\"http://x\")"),
        ("+2 Mace", "'+2 Mace"),
        ("-1+1", "'-1+1"),
        ("@SUM(A1)", "'@SUM(A1)"),
        ("  =1+1", "'  =1+1"),
        ("＝1+1", "'＝1+1"),
        ("\t=1", "'\t=1"),
        ("\r1", "'\r1"),
        ("\nx", "'\nx"),
    ] {
        assert_eq!(spreadsheet_safe(text), safe, "{text:?}");
    }
}
