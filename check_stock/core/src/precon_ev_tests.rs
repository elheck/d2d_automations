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
    value_cards(decks, |id| map.get(&id), None, TREND)
}

const TREND: EvParams = EvParams {
    field: PriceField::Trend,
    sealed_field: PriceField::Trend,
    cutoff: 0.50,
    foil_needs_sales: true,
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
            cutoff: 0.0,
            ..TREND
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
        sealed_low: None,
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
    let points = ev_history(&[&d], 99, &snaps, &dates, None, TREND);
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

// ── Sealed price basis ───────────────────────────────────────────────────────

#[test]
fn sealed_fields_are_those_cardmarket_publishes() {
    assert_eq!(
        SEALED_FIELDS,
        [PriceField::Trend, PriceField::Low, PriceField::Avg]
    );
}

#[test]
fn sealed_price_uses_its_own_field() {
    let mut data = sample_data();
    // Cards priced by their 7-day average; sealed products have no such column.
    for p in data.prices.values_mut() {
        p.avg7 = p.trend;
    }
    data.prices.get_mut(&10).unwrap().avg7 = None;
    let split = EvParams {
        field: PriceField::Avg7,
        ..TREND
    };
    let rows = build_rows(&data, split);
    assert_eq!(rows[0].sealed_price, Some(30.0));
    assert!(rows[0].ratio.is_some());

    // Pricing sealed by the card column leaves it blank (the old behaviour).
    let same = build_rows(
        &data,
        EvParams {
            sealed_field: PriceField::Avg7,
            ..split
        },
    );
    assert_eq!(same[0].sealed_price, None);
    assert_eq!(same[0].ratio, None);
}

#[test]
fn stale_below_offer_rules() {
    assert_eq!(stale_below_offer(Some(43.18), Some(88.0)), Some(88.0));
    // Exactly the factor is not flagged.
    assert_eq!(stale_below_offer(Some(40.0), Some(50.0)), None);
    assert_eq!(stale_below_offer(Some(80.0), Some(60.0)), None);
    assert_eq!(stale_below_offer(None, Some(88.0)), None);
    assert_eq!(stale_below_offer(Some(43.0), None), None);
}

#[test]
fn stale_sealed_price_is_flagged_in_notes() {
    let mut data = sample_data();
    let sealed = data.prices.get_mut(&10).unwrap();
    sealed.trend = Some(43.18);
    sealed.low = Some(88.0);
    let rows = build_rows(&data, TREND);
    assert_eq!(rows[0].sealed_low, Some(88.0));
    assert_eq!(rows[0].stale_sealed(), Some(88.0));
    let (note, hover) = rows[0].notes();
    assert!(note.contains("cheapest offer €88.00"), "{note}");
    assert!(hover.unwrap().contains("€43.18"));

    // Pricing sealed at the cheapest offer removes the flag.
    let at_low = build_rows(
        &data,
        EvParams {
            sealed_field: PriceField::Low,
            ..TREND
        },
    );
    assert_eq!(at_low[0].sealed_price, Some(88.0));
    assert_eq!(at_low[0].stale_sealed(), None);
}

// ── Sales signal ─────────────────────────────────────────────────────────────

fn day_row(
    id: u64,
    requested: &str,
    price_date: &str,
    avg1: Option<f64>,
    avg1_foil: Option<f64>,
) -> PriceSnapshot {
    PriceSnapshot {
        price_date: price_date.to_string(),
        avg1,
        avg1_foil,
        ..snap(id, requested, 1.0)
    }
}

fn test_window(days: usize) -> SalesWindow {
    SalesWindow {
        lookback: "L".to_string(),
        daily: (0..days).map(|d| format!("d{d}")).collect(),
    }
}

/// Non-foil sales on `days` of `observed` days; foil never sold.
fn sold(days: u8, observed: u8) -> SalesStats {
    SalesStats {
        nonfoil_days: days,
        foil_days: 0,
        observed_days: observed,
        nonfoil_recent: days > 0,
        foil_recent: false,
    }
}

#[test]
fn sales_window_covers_lookback_and_daily_days() {
    let w = SalesWindow::ending(date("2026-10-17"));
    assert_eq!(w.lookback, "2026-08-29");
    assert_eq!(w.daily.len(), SALES_DAYS as usize + 1);
    assert_eq!(w.daily.first().unwrap(), "2026-10-03");
    assert_eq!(w.daily.last().unwrap(), "2026-10-17");
    let dates = w.dates();
    assert_eq!(dates.len(), 16, "fits two 8-date tracker requests");
    assert_eq!(dates[0], "2026-08-29");
}

#[test]
fn sales_stats_count_days_where_the_daily_average_moved() {
    let w = test_window(5);
    let mut snaps = Vec::new();
    for (d, v) in w.daily.iter().zip([5.0, 5.0, 6.0, 6.0, 7.0]) {
        snaps.push(day_row(1, d, d, Some(v), None));
    }
    // First sale mid-window (None → value counts); 0.0 means no sale.
    for (d, v) in w
        .daily
        .iter()
        .zip([None, Some(0.0), Some(3.0), Some(3.0), Some(4.0)])
    {
        snaps.push(day_row(2, d, d, v, None));
    }
    // Tracker gap: d2 repeats d1's row, so d1 → d2 is not observed.
    for (d, (pd, v)) in w.daily.iter().zip([
        ("d0", 1.0),
        ("d1", 2.0),
        ("d1", 2.0),
        ("d3", 3.0),
        ("d4", 3.0),
    ]) {
        snaps.push(day_row(3, d, pd, Some(v), None));
    }
    snaps.push(day_row(1, "other", "other", Some(99.0), None));
    let stats = sales_stats(&snaps, &w);
    assert_eq!(stats[&1], sold(2, 4));
    assert_eq!(stats[&2], sold(2, 4));
    assert_eq!(stats[&3], sold(2, 3));
}

#[test]
fn foil_recent_looks_back_beyond_the_daily_window() {
    let w = test_window(3);
    let mut snaps = Vec::new();
    // (id, lookback foil avg1, daily foil avg1)
    for (id, back, now) in [
        (1, Some(10.0), 12.0),
        (2, Some(12.0), 12.0),
        (3, None, 12.0),
    ] {
        snaps.push(day_row(id, "L", "L", None, back));
        for d in &w.daily {
            snaps.push(day_row(id, d, d, None, Some(now)));
        }
    }
    let stats = sales_stats(&snaps, &w);
    assert_eq!(stats[&1].foil_days, 0);
    assert!(stats[&1].foil_recent, "sold between lookback and window");
    assert!(!stats[&2].foil_recent, "unchanged for the whole lookback");
    assert!(stats[&3].foil_recent, "first foil sale since the lookback");
}

#[test]
fn sales_stats_skip_days_where_the_whole_market_froze() {
    let w = test_window(4);
    // 30 cards whose averages move every day except d1 → d2 (a freeze).
    let mut snaps = Vec::new();
    for id in 1..=30u64 {
        for (k, d) in w.daily.iter().enumerate() {
            let v = id as f64 + [1.0, 2.0, 2.0, 3.0][k];
            snaps.push(PriceSnapshot {
                avg30: Some(v),
                ..day_row(id, d, d, Some(v), None)
            });
        }
    }
    assert_eq!(sales_stats(&snaps, &w)[&1], sold(2, 2));
}

#[test]
fn sales_candidates_use_the_finish_in_the_deck() {
    let d = deck(
        "D",
        vec![
            main("Non-foil, only the foil is pricey", 1, 1),
            card("Foil, pricey foil", 1, true, Some(2), DeckBoard::Main),
            main("At the floor", 1, 3),
            card("Oversized", 1, true, Some(4), DeckBoard::Display),
            card("No id", 1, false, None, DeckBoard::Main),
            main("At the floor again", 1, 3),
        ],
    );
    let decks = HashMap::from([("D".to_string(), d)]);
    let map = prices(vec![
        price(1, Some(0.10), Some(3.0)),
        price(2, Some(0.10), Some(3.0)),
        price(3, Some(SALES_MIN_PRICE_EUR), None),
        price(4, Some(50.0), Some(80.0)),
    ]);
    assert_eq!(sales_candidates(&decks, &map), vec![2, 3]);
}

#[test]
fn liquidity_weight_is_full_from_the_threshold_share() {
    assert_eq!(liquidity_weight(0, 10), 0.0);
    assert_eq!(liquidity_weight(5, 10), 1.0);
    assert!((liquidity_weight(2, 10) - 0.4).abs() < 1e-12);
    assert_eq!(liquidity_weight(10, 10), 1.0);
    // Nothing observed (unchecked card), or out-of-range counts.
    assert_eq!(liquidity_weight(1, 0), 0.0);
    assert_eq!(liquidity_weight(200, 10), 1.0);
}

#[test]
fn card_unit_gates_unsold_foil_premiums() {
    let row = price(1, Some(2.0), Some(9.0));
    let foil = Some(UnitPrice {
        eur: 9.0,
        foil_fallback: false,
    });
    let nonfoil = Some(UnitPrice {
        eur: 2.0,
        foil_fallback: true,
    });
    let foil_sold = SalesStats {
        foil_recent: true,
        ..sold(0, 10)
    };
    // Foil sold recently → foil price; never sold → non-foil price, flagged.
    assert_eq!(card_unit(Some(&row), Some(foil_sold), TREND, true), foil);
    assert_eq!(
        card_unit(Some(&row), Some(sold(5, 10)), TREND, true),
        nonfoil
    );
    // Gate off, or no sales history loaded → foil price.
    let off = EvParams {
        foil_needs_sales: false,
        ..TREND
    };
    assert_eq!(card_unit(Some(&row), Some(sold(5, 10)), off, true), foil);
    assert_eq!(card_unit(Some(&row), None, TREND, true), foil);
    // Non-foil cards are unaffected.
    assert_eq!(
        card_unit(Some(&row), Some(sold(0, 10)), TREND, false),
        Some(UnitPrice {
            eur: 2.0,
            foil_fallback: false
        })
    );
    // Foil-only printing keeps its foil price.
    let foil_only = price(1, None, Some(9.0));
    assert_eq!(
        card_unit(Some(&foil_only), Some(sold(0, 10)), TREND, true),
        foil
    );
}

#[test]
fn value_cards_attach_sales_of_the_finish_used() {
    let d = deck(
        "D",
        vec![
            card("Sold foil", 1, true, Some(1), DeckBoard::Main),
            card("Unsold foil", 1, true, Some(2), DeckBoard::Main),
            main("Cheap", 1, 3),
        ],
    );
    let map = prices(vec![
        price(1, Some(1.0), Some(10.0)),
        price(2, Some(2.0), Some(20.0)),
        price(3, Some(0.10), None),
    ]);
    let sold_foil = SalesStats {
        foil_days: 3,
        foil_recent: true,
        ..sold(10, 10)
    };
    let sales = SalesMap::from([(1, sold_foil), (2, sold(4, 10))]);
    let v = value_cards(&[&d], |id| map.get(&id), Some(&sales), TREND);
    assert_eq!(v[0].unit.unwrap().eur, 10.0);
    assert_eq!(v[0].sale_days, Some(3));
    assert_eq!(v[1].unit.unwrap().eur, 2.0);
    assert_eq!(v[1].sale_days, Some(4), "fallback uses non-foil sales");
    assert_eq!(v[1].liquidity, Some(liquidity_weight(4, 10)));
    assert_eq!(v[2].sale_days, Some(0), "unchecked cards count as unsold");
    assert_eq!(v[2].observed_days, Some(0));
    assert_eq!(v[2].liquidity, Some(0.0));

    let none = value_cards(&[&d], |id| map.get(&id), None, TREND);
    assert_eq!(none[0].sale_days, None);
    assert_eq!(none[0].liquidity, None);
}

#[test]
fn liquid_ev_weights_counted_copies_by_sales_frequency() {
    let d = deck(
        "D",
        vec![
            main("A", 2, 1),
            main("B", 1, 2),
            main("C", 1, 3),
            main("Bulk", 1, 4),
        ],
    );
    let map = prices(vec![
        price(1, Some(5.0), None),
        price(2, Some(3.0), None),
        price(3, Some(4.0), None),
        price(4, Some(0.20), None),
    ]);
    let sales = SalesMap::from([
        (1, sold(10, 10)),
        (2, sold(0, 10)),
        (3, sold(1, 10)),
        (4, sold(10, 10)),
    ]);
    let v = value_cards(&[&d], |id| map.get(&id), Some(&sales), TREND);
    let ev = summarize(&v, 0.50);
    assert!((ev.ev - 17.0).abs() < 1e-9);
    // A counts fully, B not at all, C at 20%, Bulk is below the cutoff.
    assert!((ev.liquid.unwrap() - (10.0 + 0.8)).abs() < 1e-9);

    let without = summarize(&values(&[&d], &map), 0.50);
    assert_eq!(without.liquid, None);
}

#[test]
fn rows_carry_liquid_ev_and_sort_by_it() {
    let mut data = sample_data();
    data.sales = Some(SalesMap::from([(1, sold(10, 10))]));
    let mut rows = build_rows(&data, TREND);
    assert_eq!(rows[0].ev.as_ref().unwrap().liquid, Some(3.0));
    sort_rows(&mut rows, PreconSort::Liquid, true);
    assert_eq!(rows[0].id_product, 10, "rows without Liquid EV sort last");
}
