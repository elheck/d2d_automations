//! Tests for CardTrader sales report parsing and consolidation.

use super::*;
use crate::models::{CardTraderSaleRow, ConsolidatedPosition};

const HEADER: &str = "ID;Order Code;Buyer username;Buyer name;Buyer address;Buyer country;Final buyer destination country;via Zero or direct?;Sold at (datetime);Sold at;Shipped at;Wallet credited at;Items count;Currency;Amount (cents);Amount for cancelation (cents);Amount for repurchase (cents);CardTrader fee (cents);CardTrader fee %;Item name;Item expansion;Item properties;Item game;Comment;Tag;User data Field";

/// Builds a single report row with the given variable fields.
#[allow(clippy::too_many_arguments)]
fn row(
    id: &str,
    amount: i64,
    cancelation: i64,
    fee: i64,
    name: &str,
    expansion: &str,
    properties: &str,
    sold_at: &str,
) -> String {
    format!(
        "{id};20260628v5hqam;buyer;Buyer Name;CardTrader Zero;Germany;Italy;zero;1 Jul 2026;{sold_at};2026-07-06 14:26:45 +0200;2026-07-14 09:31:06 +0200;1;EUR;{amount};{cancelation};0;{fee};7.0%;{name};{expansion};{properties};Magic;\"\";;123456"
    )
}

fn sample_report() -> String {
    let rows = [
        row(
            "1",
            2,
            0,
            -1,
            "Plains",
            "Murders at Karlov Manor",
            "Moderately Played - DE",
            "2026-07-01 09:30:30 +0200",
        ),
        row(
            "2",
            2,
            0,
            -1,
            "Plains",
            "Murders at Karlov Manor",
            "Moderately Played - DE",
            "2026-07-01 09:30:30 +0200",
        ),
        row(
            "3",
            98,
            0,
            -7,
            "Katara, Water Tribe's Hope",
            "Avatar: The Last Airbender Collectors",
            "Near Mint - EN",
            "2026-07-02 09:32:11 +0200",
        ),
    ];
    format!("{HEADER}\n{}\n", rows.join("\n"))
}

fn make_row(name: &str, expansion: &str, properties: &str, net: i64) -> CardTraderSaleRow {
    CardTraderSaleRow {
        id: "1".to_string(),
        order_code: "abc".to_string(),
        sold_at: "2026-07-01 09:30:30 +0200".to_string(),
        item_name: name.to_string(),
        item_expansion: expansion.to_string(),
        item_properties: properties.to_string(),
        amount_cents: net,
        cancelation_cents: 0,
        fee_cents: -1,
        currency: "EUR".to_string(),
    }
}

#[test]
fn detects_cardtrader_header() {
    assert!(is_cardtrader_report(HEADER));
}

#[test]
fn rejects_cardmarket_header() {
    let cardmarket = "OrderID;Username;Name;Street;City;Country;IsProfessional;VATNumber;DateOfPurchase;ArticleCount;MerchandiseValue;ShipmentCosts;TotalValue;Commission;Currency;Description;ProductID;LocalizedProductName";
    assert!(!is_cardtrader_report(cardmarket));
}

#[test]
fn rejects_unrelated_header() {
    assert!(!is_cardtrader_report("foo;bar;baz"));
}

#[test]
fn parses_all_rows() {
    let rows = parse_sales_report(&sample_report()).expect("should parse");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].item_name, "Plains");
    assert_eq!(rows[0].amount_cents, 2);
    assert_eq!(rows[0].fee_cents, -1);
    assert_eq!(rows[0].currency, "EUR");
    assert_eq!(rows[2].item_name, "Katara, Water Tribe's Hope");
    assert_eq!(rows[2].amount_cents, 98);
}

#[test]
fn parses_quoted_comment_column_without_splitting() {
    // A comment containing the delimiter must not shift the columns.
    let line = "9;code;buyer;Buyer;CardTrader Zero;Germany;Italy;zero;1 Jul 2026;2026-07-01 09:30:30 +0200;;;1;EUR;50;0;0;-4;7.0%;Card;Set;Near Mint - EN;Magic;\"a;b;c\";;1";
    let content = format!("{HEADER}\n{line}\n");
    let rows = parse_sales_report(&content).expect("should parse");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].amount_cents, 50);
    assert_eq!(rows[0].item_name, "Card");
    assert_eq!(rows[0].item_properties, "Near Mint - EN");
}

#[test]
fn skips_trailing_blank_lines() {
    let content = format!("{}\n\n", sample_report().trim_end());
    let rows = parse_sales_report(&content).expect("should parse");
    assert_eq!(rows.len(), 3);
}

#[test]
fn missing_required_column_is_an_error() {
    let content = "ID;Order Code;Amount (cents);CardTrader fee (cents);Item name\n1;a;2;-1;Card\n";
    let err = parse_sales_report(content).expect_err("should fail on an incomplete header");
    assert!(
        err.to_string().contains("is missing the"),
        "unexpected error: {err}"
    );
}

#[test]
fn missing_item_expansion_column_is_an_error() {
    // Header with everything except "Item expansion".
    let content = "ID;Order Code;Sold at;Currency;Amount (cents);Amount for cancelation (cents);CardTrader fee (cents);Item name;Item properties\n1;a;2026-07-01 09:30:30 +0200;EUR;2;0;-1;Card;NM\n";
    let err = parse_sales_report(content).expect_err("should fail without Item expansion");
    assert!(
        err.to_string().contains("Item expansion"),
        "unexpected error: {err}"
    );
}

#[test]
fn prefers_iso_sold_at_column_over_coarse_date() {
    // The real report puts "1 Jul 2026" in "Sold at (datetime)" and the ISO
    // timestamp in "Sold at"; the ISO column must win.
    let rows = parse_sales_report(&sample_report()).expect("should parse");
    assert_eq!(rows[0].sold_at, "2026-07-01 09:30:30 +0200");
}

#[test]
fn non_numeric_amount_is_an_error() {
    let line = "1;code;buyer;Buyer;CardTrader Zero;Germany;Italy;zero;1 Jul 2026;2026-07-01 09:30:30 +0200;;;1;EUR;abc;0;0;-1;7.0%;Card;Set;NM;Magic;\"\";;1";
    let content = format!("{HEADER}\n{line}\n");
    let err = parse_sales_report(&content).expect_err("should reject non-numeric amount");
    assert!(
        err.to_string().contains("Amount"),
        "unexpected error: {err}"
    );
}

#[test]
fn empty_cents_fields_default_to_zero() {
    let line = "1;code;buyer;Buyer;CardTrader Zero;Germany;Italy;zero;1 Jul 2026;2026-07-01 09:30:30 +0200;;;1;EUR;;;0;;7.0%;Card;Set;NM;Magic;\"\";;1";
    let content = format!("{HEADER}\n{line}\n");
    let rows = parse_sales_report(&content).expect("should parse");
    assert_eq!(rows[0].amount_cents, 0);
    assert_eq!(rows[0].cancelation_cents, 0);
    assert_eq!(rows[0].fee_cents, 0);
}

#[test]
fn net_cents_offsets_cancellation() {
    let mut r = make_row("Card", "Set", "NM", 105);
    r.cancelation_cents = -105;
    assert_eq!(r.net_cents(), 0);
    assert!(r.is_void());
}

#[test]
fn zero_amount_row_is_void() {
    let r = make_row("Card", "Set", "NM", 0);
    assert!(r.is_void());
}

#[test]
fn groups_identical_cards_into_one_position() {
    let rows = vec![
        make_row("Plains", "MKM", "MP - DE", 2),
        make_row("Plains", "MKM", "MP - DE", 2),
        make_row("Plains", "MKM", "MP - DE", 2),
    ];
    let invoice = consolidate(&rows, InvoiceRecipient::default()).expect("should consolidate");

    assert_eq!(invoice.positions.len(), 1);
    assert_eq!(invoice.positions[0].quantity, 3);
    assert_eq!(invoice.positions[0].total_cents, 6);
    assert_eq!(invoice.total_cents(), 6);
}

#[test]
fn different_conditions_are_separate_positions() {
    let rows = vec![
        make_row("Plains", "MKM", "Near Mint - EN", 2),
        make_row("Plains", "MKM", "Moderately Played - DE", 2),
    ];
    let invoice = consolidate(&rows, InvoiceRecipient::default()).expect("should consolidate");
    assert_eq!(invoice.positions.len(), 2);
}

#[test]
fn different_expansions_are_separate_positions() {
    let rows = vec![
        make_row("Evolving Wilds", "Foundations", "Near Mint - EN", 2),
        make_row("Evolving Wilds", "Khans of Tarkir", "Near Mint - EN", 2),
    ];
    let invoice = consolidate(&rows, InvoiceRecipient::default()).expect("should consolidate");
    assert_eq!(invoice.positions.len(), 2);
}

#[test]
fn cancelled_rows_are_excluded() {
    let mut cancelled = make_row("Ride's End", "Aetherdrift", "NM - EN - Foil", 2);
    cancelled.cancelation_cents = -2;
    let zero_dup = make_row("Ride's End", "Aetherdrift", "NM - EN - Foil", 0);

    let rows = vec![make_row("Plains", "MKM", "MP - DE", 2), cancelled, zero_dup];
    let invoice = consolidate(&rows, InvoiceRecipient::default()).expect("should consolidate");

    assert_eq!(invoice.positions.len(), 1);
    assert_eq!(invoice.row_count, 1);
    assert_eq!(invoice.skipped_row_count, 2);
    assert_eq!(invoice.total_cents(), 2);
}

#[test]
fn fees_are_never_included_in_the_total() {
    // Fees are invoiced separately by CardTrader, so a large fee must not move the total.
    let mut r = make_row("Card", "Set", "NM", 1000);
    r.fee_cents = -700;
    let invoice = consolidate(&[r], InvoiceRecipient::default()).expect("should consolidate");
    assert_eq!(invoice.total_cents(), 1000);
    assert!((invoice.total() - 10.0).abs() < f64::EPSILON);
}

#[test]
fn empty_report_is_rejected() {
    let err = consolidate(&[], InvoiceRecipient::default()).expect_err("should reject empty");
    assert!(err.to_string().contains("no rows"), "unexpected: {err}");
}

#[test]
fn fully_cancelled_report_is_rejected() {
    let mut r = make_row("Card", "Set", "NM", 105);
    r.cancelation_cents = -105;
    let err = consolidate(&[r], InvoiceRecipient::default()).expect_err("should reject");
    assert!(
        err.to_string().contains("no billable rows"),
        "unexpected: {err}"
    );
}

#[test]
fn derives_date_range_and_invoice_date() {
    let mut early = make_row("A", "Set", "NM", 10);
    early.sold_at = "2026-07-01 09:30:30 +0200".to_string();
    let mut late = make_row("B", "Set", "NM", 10);
    late.sold_at = "2026-07-05 12:46:02 +0200".to_string();

    let invoice = consolidate(&[late, early], InvoiceRecipient::default()).expect("consolidate");

    assert_eq!(invoice.invoice_date, "2026-07-05");
    assert_eq!(invoice.period_label, "2026-07-01 - 2026-07-05");
}

#[test]
fn single_day_report_uses_bare_date_as_period() {
    let invoice = consolidate(
        &[make_row("A", "Set", "NM", 10)],
        InvoiceRecipient::default(),
    )
    .unwrap();
    assert_eq!(invoice.period_label, "2026-07-01");
    assert_eq!(invoice.invoice_date, "2026-07-01");
}

#[test]
fn uses_provided_recipient() {
    let recipient = InvoiceRecipient {
        name: "OTHER COMPANY".to_string(),
        street: "Somewhere 1".to_string(),
        zip: "12345".to_string(),
        city: "Berlin".to_string(),
        country: "Deutschland".to_string(),
    };
    let invoice =
        consolidate(&[make_row("A", "Set", "NM", 10)], recipient.clone()).expect("consolidate");
    assert_eq!(invoice.recipient, recipient);
}

#[test]
fn preserves_first_seen_order_of_positions() {
    let rows = vec![
        make_row("Zebra", "Set", "NM", 10),
        make_row("Alpha", "Set", "NM", 10),
        make_row("Zebra", "Set", "NM", 10),
    ];
    let invoice = consolidate(&rows, InvoiceRecipient::default()).unwrap();
    assert_eq!(invoice.positions[0].item_name, "Zebra");
    assert_eq!(invoice.positions[0].quantity, 2);
    assert_eq!(invoice.positions[1].item_name, "Alpha");
}

#[test]
fn end_to_end_matches_real_report_fixture() {
    let content = include_str!("../../tests/fixtures/cardtrader_sales_report.csv");
    let rows = parse_sales_report(content).expect("fixture should parse");
    assert_eq!(rows.len(), 8);

    let invoice = consolidate(&rows, InvoiceRecipient::default()).expect("should consolidate");

    // 3x Plains group + Katara + Carnage Tyrant; cancelled rows excluded
    assert_eq!(invoice.positions.len(), 3);
    assert_eq!(invoice.row_count, 5);
    assert_eq!(invoice.skipped_row_count, 3);

    // 3*2 + 98 + 342 = 446 cents
    assert_eq!(invoice.total_cents(), 446);
    assert!((invoice.total() - 4.46).abs() < 1e-9);

    let plains = invoice
        .positions
        .iter()
        .find(|p| p.item_name == "Plains")
        .expect("Plains position");
    assert_eq!(plains.quantity, 3);
    assert_eq!(plains.total_cents, 6);

    assert!(
        validate_consolidated(&invoice).is_empty(),
        "errors: {:?}",
        validate_consolidated(&invoice)
    );
}

#[test]
fn unit_price_derives_from_group_total() {
    let position = ConsolidatedPosition {
        item_name: "Plains".to_string(),
        item_expansion: "MKM".to_string(),
        item_properties: "MP - DE".to_string(),
        quantity: 4,
        total_cents: 100,
    };
    assert!((position.unit_price() - 0.25).abs() < 1e-9);
    assert!((position.total_price() - 1.0).abs() < 1e-9);
}

#[test]
fn unit_price_of_empty_quantity_is_zero() {
    let position = ConsolidatedPosition {
        item_name: "X".to_string(),
        item_expansion: String::new(),
        item_properties: String::new(),
        quantity: 0,
        total_cents: 100,
    };
    assert_eq!(position.unit_price(), 0.0);
}

#[test]
fn display_name_omits_empty_expansion() {
    let position = ConsolidatedPosition {
        item_name: "Plains".to_string(),
        item_expansion: String::new(),
        item_properties: "NM".to_string(),
        quantity: 1,
        total_cents: 2,
    };
    assert_eq!(position.display_name(), "Plains");
}

#[test]
fn display_name_includes_expansion() {
    let position = ConsolidatedPosition {
        item_name: "Plains".to_string(),
        item_expansion: "MKM".to_string(),
        item_properties: "NM".to_string(),
        quantity: 1,
        total_cents: 2,
    };
    assert_eq!(position.display_name(), "Plains (MKM)");
}

#[test]
fn validation_rejects_empty_recipient_fields() {
    let recipient = InvoiceRecipient {
        name: "  ".to_string(),
        street: String::new(),
        zip: "20124".to_string(),
        city: String::new(),
        country: String::new(),
    };
    let invoice = consolidate(&[make_row("A", "Set", "NM", 10)], recipient).unwrap();
    let errors = validate_consolidated(&invoice);

    assert!(errors.iter().any(|e| e.contains("name")));
    assert!(errors.iter().any(|e| e.contains("street")));
    assert!(errors.iter().any(|e| e.contains("city")));
    assert!(errors.iter().any(|e| e.contains("country")));
}

#[test]
fn validation_accepts_default_recipient() {
    let invoice = consolidate(
        &[make_row("A", "Set", "NM", 10)],
        InvoiceRecipient::default(),
    )
    .unwrap();
    assert!(
        validate_consolidated(&invoice).is_empty(),
        "errors: {:?}",
        validate_consolidated(&invoice)
    );
}

#[test]
fn default_recipient_is_gray_fox() {
    let recipient = InvoiceRecipient::default();
    assert_eq!(recipient.name, "GRAY FOX SRL");
    assert_eq!(recipient.street, "Via San Gregorio 55");
    assert_eq!(recipient.zip, "20124");
    assert_eq!(recipient.city, "Milano");
    assert_eq!(recipient.country, "Italien");
    assert_eq!(
        recipient.formatted_address(),
        "GRAY FOX SRL\nVia San Gregorio 55\n20124 Milano\nItalien"
    );
}

#[test]
fn total_quantity_sums_all_positions() {
    let rows = vec![
        make_row("A", "Set", "NM", 10),
        make_row("A", "Set", "NM", 10),
        make_row("B", "Set", "NM", 10),
    ];
    let invoice = consolidate(&rows, InvoiceRecipient::default()).unwrap();
    assert_eq!(invoice.total_quantity(), 3);
}

#[test]
fn large_report_sums_without_float_drift() {
    // 1000 rows of 1 cent must total exactly 10.00, which naive f64 accumulation
    // of 0.01 would not guarantee.
    let rows: Vec<CardTraderSaleRow> = (0..1000)
        .map(|i| make_row(&format!("Card {i}"), "Set", "NM", 1))
        .collect();
    let invoice = consolidate(&rows, InvoiceRecipient::default()).unwrap();
    assert_eq!(invoice.total_cents(), 1000);
    assert_eq!(invoice.total(), 10.0);
}
