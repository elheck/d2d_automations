use super::*;
use crate::api::mtgjson::{DeckRef, SealedPrecon};

fn card(name: &str, count: u32, is_foil: bool, mcm_id: Option<u64>, board: DeckBoard) -> DeckCard {
    DeckCard {
        name: name.to_string(),
        set_code: "BLC".to_string(),
        number: "1".to_string(),
        count,
        is_foil,
        mcm_id,
        board,
    }
}

fn main(name: &str, count: u32, mcm_id: u64) -> DeckCard {
    card(name, count, false, Some(mcm_id), DeckBoard::Main)
}

fn deck(name: &str, cards: Vec<DeckCard>) -> PreconDeck {
    PreconDeck {
        code: "BLC".to_string(),
        name: name.to_string(),
        release_date: Some("2024-08-02".to_string()),
        cards,
    }
}

fn price(id: u64, trend: Option<f64>, trend_foil: Option<f64>) -> LatestPrice {
    LatestPrice {
        id_product: id,
        price_date: "2026-10-16".to_string(),
        avg: None,
        low: Some(0.01),
        trend,
        avg1: None,
        avg7: None,
        avg30: None,
        avg_foil: None,
        low_foil: None,
        trend_foil,
        avg1_foil: None,
        avg7_foil: None,
        avg30_foil: None,
    }
}

fn snap(id: u64, date: &str, trend: f64) -> PriceSnapshot {
    PriceSnapshot {
        id_product: id,
        requested_date: date.to_string(),
        price_date: date.to_string(),
        avg: None,
        low: None,
        trend: Some(trend),
        avg1: None,
        avg7: None,
        avg30: None,
        avg_foil: None,
        low_foil: None,
        trend_foil: None,
        avg1_foil: None,
        avg7_foil: None,
        avg30_foil: None,
    }
}

fn product(id: u64, name: &str, category: &str) -> ProductSearchResult {
    ProductSearchResult {
        id_product: id,
        name: name.to_string(),
        category_name: category.to_string(),
        id_expansion: 5660,
        expansion_name: None,
    }
}

fn prices(rows: Vec<LatestPrice>) -> HashMap<u64, LatestPrice> {
    rows.into_iter().map(|p| (p.id_product, p)).collect()
}

fn values<'a>(decks: &[&'a PreconDeck], map: &HashMap<u64, LatestPrice>) -> Vec<CardValue<'a>> {
    value_cards(decks, |id| map.get(&id), PriceField::Trend)
}

const TREND: EvParams = EvParams {
    field: PriceField::Trend,
    cutoff: 0.50,
};

// ── Catalog ──────────────────────────────────────────────────────────────────

#[test]
fn catalog_filter_keeps_commander_intropacks_only() {
    let rows = vec![
        product(
            1,
            "Commander: Bloomburrow: \"\"Animated Army\"\" Commander Deck",
            "Magic Intropack",
        ),
        product(
            2,
            "Commander 2018: \"\"Exquisite Invention\"\" Deck",
            "Magic Intropack",
        ),
        product(3, "Commander's Plate", "Magic Single"),
        product(4, "Challenger Deck 2022", "Magic Intropack"),
        product(5, "Commander Deck Set", "Magic Theme Deck Display"),
        product(1, "duplicate commander", "Magic Intropack"),
        product(
            6,
            "COMMANDER LEGENDS: \"\"Arm for Battle\"\" Deck",
            "Magic Intropack",
        ),
    ];
    let ids: Vec<u64> = filter_precon_catalog(rows)
        .iter()
        .map(|p| p.id_product)
        .collect();
    assert_eq!(ids, vec![1, 2, 6]);
}

#[test]
fn catalog_truncation_and_display_name() {
    assert!(catalog_truncated(5_000, 5_000));
    assert!(!catalog_truncated(4_999, 5_000));
    assert_eq!(
        display_name(" Commander: Bloomburrow: \"\"Animated Army\"\" Commander Deck "),
        "Commander: Bloomburrow: \"Animated Army\" Commander Deck"
    );
}

// ── Prices ───────────────────────────────────────────────────────────────────

#[test]
fn unit_price_reads_matching_column() {
    let row = price(1, Some(2.0), Some(5.0));
    assert_eq!(
        unit_price(Some(&row), PriceField::Trend, false),
        Some(UnitPrice {
            eur: 2.0,
            foil_fallback: false
        })
    );
    assert_eq!(
        unit_price(Some(&row), PriceField::Trend, true),
        Some(UnitPrice {
            eur: 5.0,
            foil_fallback: false
        })
    );
}

#[test]
fn foil_falls_back_to_nonfoil_but_never_the_reverse() {
    let no_foil = price(1, Some(2.0), None);
    let zero_foil = price(1, Some(2.0), Some(0.0));
    let foil_only = price(1, None, Some(9.0));
    let fallback = Some(UnitPrice {
        eur: 2.0,
        foil_fallback: true,
    });
    assert_eq!(
        unit_price(Some(&no_foil), PriceField::Trend, true),
        fallback
    );
    assert_eq!(
        unit_price(Some(&zero_foil), PriceField::Trend, true),
        fallback
    );
    assert_eq!(unit_price(Some(&foil_only), PriceField::Trend, false), None);
}

#[test]
fn non_positive_or_missing_prices_are_missing() {
    let negative = price(1, Some(-1.0), None);
    let zero = price(1, Some(0.0), None);
    assert_eq!(unit_price(Some(&negative), PriceField::Trend, false), None);
    assert_eq!(unit_price(Some(&zero), PriceField::Trend, false), None);
    assert_eq!(
        unit_price::<LatestPrice>(None, PriceField::Trend, false),
        None
    );
    assert_eq!(sealed_price(Some(&zero), PriceField::Trend), None);
    // Sealed products always read the non-foil column.
    assert_eq!(
        sealed_price(Some(&price(1, Some(40.0), Some(0.0))), PriceField::Trend),
        Some(40.0)
    );
}

// ── EV ───────────────────────────────────────────────────────────────────────

#[test]
fn summarize_applies_cutoff_inclusively() {
    let d = deck(
        "D",
        vec![
            main("Expensive", 1, 1),
            main("AtCutoff", 1, 2),
            main("Bulk", 4, 3),
            main("Lands", 10, 4),
        ],
    );
    let map = prices(vec![
        price(1, Some(10.0), None),
        price(2, Some(0.50), None),
        price(3, Some(0.20), None),
        price(4, Some(0.05), None),
    ]);
    let ev = summarize(&values(&[&d], &map), 0.50);
    assert!((ev.ev - 10.50).abs() < 1e-9);
    assert!((ev.gross - (10.0 + 0.5 + 0.8 + 0.5)).abs() < 1e-9);
    assert_eq!(ev.total_copies, 16);
    assert_eq!(ev.priced_copies, 16);
    assert_eq!(ev.counted_copies, 2);
    assert_eq!(ev.below_cutoff_copies, 14);

    let no_cutoff = summarize(&values(&[&d], &map), 0.0);
    assert!((no_cutoff.ev - no_cutoff.gross).abs() < 1e-9);
}

#[test]
fn summarize_counts_unpriced_and_fallbacks_and_skips_display() {
    let d = deck(
        "D",
        vec![
            card("Commander", 1, true, Some(1), DeckBoard::Commander),
            card("No id", 1, false, None, DeckBoard::Main),
            main("No price row", 2, 99),
            card("Oversized", 1, true, Some(5), DeckBoard::Display),
        ],
    );
    let map = prices(vec![
        price(1, Some(3.0), None),
        price(5, Some(50.0), Some(80.0)),
    ]);
    let ev = summarize(&values(&[&d], &map), 0.5);
    assert_eq!(ev.total_copies, 4);
    assert_eq!(ev.priced_copies, 1);
    assert_eq!(ev.unpriced_copies, 3);
    assert_eq!(ev.foil_fallback_copies, 1);
    assert!((ev.ev - 3.0).abs() < 1e-9);
    assert!((ev.top5 - 3.0).abs() < 1e-9, "display commander excluded");
    assert!((ev.coverage() - 0.25).abs() < 1e-9);
    assert_eq!(DeckEv::default().coverage(), 0.0);
}

#[test]
fn top5_expands_counts_and_ignores_cutoff() {
    let d = deck(
        "D",
        vec![
            main("A", 1, 1),
            main("B", 3, 2),
            main("C", 1, 3),
            main("D", 1, 4),
        ],
    );
    let map = prices(vec![
        price(1, Some(0.40), None),
        price(2, Some(0.30), None),
        price(3, Some(0.20), None),
        price(4, Some(0.10), None),
    ]);
    let v = values(&[&d], &map);
    // 0.40 + 3 × 0.30 + 0.20 — all below the 0.50 cutoff.
    assert!((top_n_copies(&v, 5) - 1.50).abs() < 1e-9);
    let ev = summarize(&v, 0.50);
    assert_eq!(ev.ev, 0.0);
    assert!((ev.top5 - 1.50).abs() < 1e-9);
    assert_eq!(top_n_lines(&v, 5), HashSet::from([0, 1, 2]));
}

#[test]
fn top5_with_fewer_priced_copies_sums_what_exists() {
    let d = deck("D", vec![main("A", 1, 1), main("Unpriced", 1, 2)]);
    let map = prices(vec![price(1, Some(4.0), None)]);
    let v = values(&[&d], &map);
    assert!((top_n_copies(&v, 5) - 4.0).abs() < 1e-9);
    assert_eq!(top_n_lines(&v, 5), HashSet::from([0]));
    assert_eq!(ranked_by_unit(&v), vec![0, 1]);
}

#[test]
fn bundle_ev_sums_decks_and_ratio_needs_sealed_price() {
    let a = deck("A", vec![main("X", 1, 1)]);
    let b = deck("B", vec![main("Y", 1, 2)]);
    let map = prices(vec![price(1, Some(6.0), None), price(2, Some(4.0), None)]);
    let v = values(&[&a, &b], &map);
    assert_eq!(v[1].deck_idx, 1);
    assert_eq!(v[0].line_total(), Some(6.0));
    let ev = summarize(&v, 0.5);
    assert!((ev.ev - 10.0).abs() < 1e-9);
    assert_eq!(ev_ratio(ev.ev, Some(20.0)), Some(0.5));
    assert_eq!(ev_ratio(ev.ev, None), None);
    assert_eq!(ev_ratio(ev.ev, Some(0.0)), None);
}

// ── Ranking ──────────────────────────────────────────────────────────────────

fn deck_ref(file: &str) -> DeckRef {
    DeckRef {
        file_name: file.to_string(),
        name: file.to_string(),
        code: "BLC".to_string(),
        release_date: Some("2024-08-02".to_string()),
    }
}

fn sample_data() -> PreconData {
    let mut data = PreconData {
        catalog: vec![
            product(10, "Ready \"\"Deck\"\" Commander", "Magic Intropack"),
            product(11, "Pending Commander", "Magic Intropack"),
            product(12, "Unknown Commander", "Magic Intropack"),
            product(13, "Failed Commander", "Magic Intropack"),
        ],
        ..PreconData::default()
    };
    data.index.products.insert(
        10,
        SealedPrecon {
            sealed_name: "Ready".to_string(),
            set_code: "BLC".to_string(),
            deck_keys: vec![],
        },
    );
    data.resolved
        .insert(10, DeckResolution::Ready(vec![deck_ref("Ready_BLC")]));
    data.resolved.insert(
        11,
        DeckResolution::Pending(vec!["We Are the Borg".to_string()]),
    );
    data.resolved
        .insert(13, DeckResolution::Ready(vec![deck_ref("Failed_BLC")]));
    data.decks.insert(
        "Ready_BLC".to_string(),
        deck("Ready", vec![main("Sol Ring", 1, 1), main("Bulk", 1, 2)]),
    );
    data.deck_errors
        .insert("Failed_BLC".to_string(), "HTTP 500".to_string());
    data.prices = prices(vec![
        price(1, Some(3.0), None),
        price(2, Some(0.1), None),
        price(10, Some(30.0), None),
        price(11, Some(25.0), None),
    ]);
    data
}

#[test]
fn build_rows_produces_every_status() {
    let rows = build_rows(&sample_data(), TREND);
    assert_eq!(rows.len(), 4);

    let ready = &rows[0];
    assert_eq!(ready.status, PreconStatus::Ready { decks: 1 });
    assert_eq!(ready.name, "Ready \"Deck\" Commander");
    assert_eq!(ready.set_label, "BLC");
    assert_eq!(ready.release_date.as_deref(), Some("2024-08-02"));
    assert_eq!(ready.sealed_price, Some(30.0));
    assert!((ready.ev.as_ref().unwrap().ev - 3.0).abs() < 1e-9);
    assert!((ready.ratio.unwrap() - 0.1).abs() < 1e-9);

    assert_eq!(
        rows[1].status,
        PreconStatus::DecklistPending(vec!["We Are the Borg".to_string()])
    );
    assert_eq!(rows[1].ev, None);
    assert_eq!(rows[1].sealed_price, Some(25.0));
    assert_eq!(rows[2].status, PreconStatus::NotOnMtgjson);
    assert_eq!(rows[2].set_label, "");
    assert_eq!(
        rows[3].status,
        PreconStatus::DeckUnavailable("HTTP 500".to_string())
    );
}

#[test]
fn build_rows_prefers_tracker_expansion_name() {
    let mut data = sample_data();
    data.catalog[0].expansion_name = Some("Bloomburrow Commander".to_string());
    assert_eq!(
        build_rows(&data, TREND)[0].set_label,
        "Bloomburrow Commander"
    );
}

#[test]
fn rows_recompute_locally_for_new_params() {
    let data = sample_data();
    let strict = build_rows(&data, TREND);
    let loose = build_rows(
        &data,
        EvParams {
            field: PriceField::Trend,
            cutoff: 0.0,
        },
    );
    let strict_ev = strict[0].ev.as_ref().unwrap();
    let loose_ev = loose[0].ev.as_ref().unwrap();
    assert!(loose_ev.ev > strict_ev.ev);
    assert_eq!(loose_ev.top5, strict_ev.top5);
}

fn row(name: &str, ratio: Option<f64>, release: Option<&str>) -> PreconRow {
    PreconRow {
        id_product: 0,
        name: name.to_string(),
        set_label: String::new(),
        release_date: release.map(str::to_string),
        status: PreconStatus::NotOnMtgjson,
        sealed_price: None,
        ev: None,
        ratio,
    }
}

#[test]
fn sort_puts_missing_values_last_in_both_directions() {
    let mut rows = vec![
        row("b", None, None),
        row("a", Some(1.2), Some("2024-01-01")),
        row("c", Some(0.8), Some("2025-01-01")),
    ];
    sort_rows(&mut rows, PreconSort::Ratio, true);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["a", "c", "b"]);

    sort_rows(&mut rows, PreconSort::Ratio, false);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["c", "a", "b"]);

    sort_rows(&mut rows, PreconSort::Release, true);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["c", "a", "b"]);

    sort_rows(&mut rows, PreconSort::Name, false);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, vec!["a", "b", "c"]);
}

#[test]
fn visible_rows_filters_text_and_decklist() {
    let rows = build_rows(&sample_data(), TREND);
    assert_eq!(visible_rows(&rows, "", false).len(), 4);
    assert_eq!(visible_rows(&rows, "", true).len(), 1);
    assert_eq!(visible_rows(&rows, "  PENDING ", false).len(), 1);
    assert_eq!(visible_rows(&rows, "blc", false).len(), 1);
}

#[test]
fn price_ids_are_sorted_and_unique() {
    let data = sample_data();
    assert_eq!(
        price_ids(&data.catalog, &data.decks),
        vec![1, 2, 10, 11, 12, 13]
    );
}

#[test]
fn decks_for_requires_all_decks_loaded() {
    let data = sample_data();
    assert_eq!(data.decks_for(10).map(|d| d.len()), Some(1));
    assert!(data.decks_for(11).is_none());
    assert!(data.decks_for(12).is_none());
    assert!(data.decks_for(13).is_none());
}

// ── History ──────────────────────────────────────────────────────────────────

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[test]
fn weekly_dates_end_today_and_step_weekly() {
    let dates = weekly_dates(date("2026-02-01"), date("2026-10-16"));
    assert_eq!(dates.len(), 37);
    assert_eq!(dates.last().unwrap(), "2026-10-16");
    assert!(dates[0].as_str() >= "2026-02-01");
    assert_eq!(dates[dates.len() - 2], "2026-10-09");
    assert_eq!(
        weekly_dates(date("2026-10-16"), date("2026-10-16")),
        vec!["2026-10-16"]
    );
    assert!(weekly_dates(date("2026-10-17"), date("2026-10-16")).is_empty());
}

#[test]
fn history_start_is_release_capped_to_window() {
    let today = date("2026-10-16");
    assert_eq!(history_start(Some("2026-10-02"), today), date("2026-10-02"));
    assert_eq!(history_start(Some("2020-01-01"), today), date("2025-10-17"));
    assert_eq!(history_start(None, today), date("2025-10-17"));
    assert_eq!(history_start(Some("garbage"), today), date("2025-10-17"));
}

#[test]
fn ev_history_groups_by_requested_date() {
    let d = deck("D", vec![main("A", 1, 1), main("B", 1, 2)]);
    let dates = vec![
        "2026-01-01".to_string(),
        "2026-01-08".to_string(),
        "2026-01-15".to_string(),
    ];
    let snaps = vec![
        // 01-01: only sealed known → EV gap.
        snap(99, "2026-01-01", 40.0),
        // 01-08: half the copies priced (< 90%) → EV gap.
        snap(1, "2026-01-08", 5.0),
        // 01-15: fully priced.
        snap(1, "2026-01-15", 6.0),
        snap(2, "2026-01-15", 1.0),
        snap(99, "2026-01-15", 42.0),
        // Unknown date is ignored.
        snap(1, "2030-01-01", 1000.0),
    ];
    let points = ev_history(&[&d], 99, &snaps, &dates, TREND);
    assert_eq!(points.len(), 3);
    assert_eq!(points[0].ev, None);
    assert_eq!(points[0].sealed, Some(40.0));
    assert_eq!(points[1].ev, None);
    assert!((points[1].coverage - 0.5).abs() < 1e-9);
    assert_eq!(points[2].ev, Some(7.0));
    assert_eq!(points[2].top5, Some(7.0));
    assert_eq!(points[2].sealed, Some(42.0));
}

#[test]
fn trim_leading_gaps_drops_empty_prefix() {
    let point = |date: &str, ev: Option<f64>| EvPoint {
        date: date.to_string(),
        ev,
        top5: ev,
        sealed: None,
        coverage: 0.0,
    };
    let trimmed = trim_leading_gaps(vec![
        point("1", None),
        point("2", Some(1.0)),
        point("3", None),
    ]);
    let dates: Vec<&str> = trimmed.iter().map(|p| p.date.as_str()).collect();
    assert_eq!(dates, vec!["2", "3"]);
    assert!(trim_leading_gaps(vec![point("1", None)]).is_empty());
}

#[test]
fn row_notes_describe_status() {
    let rows = build_rows(&sample_data(), TREND);
    assert_eq!(rows[0].notes(), (String::new(), None));
    assert_eq!(
        rows[1].notes(),
        (
            "decklist not published yet".to_string(),
            Some("We Are the Borg".to_string())
        )
    );
    assert_eq!(rows[2].notes().0, "not on MTGJSON yet");
    assert_eq!(
        rows[3].notes(),
        (
            "decklist download failed".to_string(),
            Some("HTTP 500".to_string())
        )
    );

    let mut bundle = rows[0].clone();
    bundle.status = PreconStatus::Ready { decks: 4 };
    bundle.ev = Some(DeckEv {
        foil_fallback_copies: 2,
        unpriced_copies: 1,
        ..DeckEv::default()
    });
    assert_eq!(
        bundle.notes().0,
        "bundle of 4 decks · 2 foil→non-foil · 1 unpriced"
    );
}
