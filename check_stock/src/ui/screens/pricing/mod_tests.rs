use super::rules::sanitize_file_stem;
use super::*;
use crate::repricing::{Condition, Outcome, PriceAction, PriceBase, RarityFloors};

// ── Helpers ─────────────────────────────────────────────────────────────────

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 6).unwrap()
}

fn card(id: &str, name: &str, price: &str, qty: &str) -> Card {
    let mut c = Card::test_default();
    c.cardmarket_id = id.to_string();
    c.name = name.to_string();
    c.price = price.to_string();
    c.quantity = qty.to_string();
    c.rarity = "Rare".to_string();
    c
}

fn row(id: u64, trend: f64) -> LatestPrice {
    LatestPrice {
        id_product: id,
        price_date: "2026-11-05".to_string(),
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

/// A ruleset that follows the trend 1:1 with no floors or dead band.
fn trend_rules() -> Ruleset {
    let mut rs = Ruleset::starter();
    rs.rules = vec![Rule {
        name: "Follow".to_string(),
        enabled: true,
        conditions: Vec::new(),
        action: PriceAction {
            adjust: false,
            ..PriceAction::default()
        },
    }];
    rs.guardrails.floors = RarityFloors {
        common: 0.0,
        uncommon: 0.0,
        rare: 0.0,
        mythic: 0.0,
        other: 0.0,
    };
    rs.guardrails.min_change_eur = 0.0;
    rs.guardrails.min_change_pct = 0.0;
    rs
}

/// State with four listings priced by `trend_rules`:
/// 0 Alpha 1.00 → 1.50 (raise, ×2), 1 Beta 5.00 → 20.00 (raise, review),
/// 2 Gamma 3.00 → 2.50 (drop), 3 Delta 2.00 (no market), plus one sold-out row.
fn loaded(dir: &tempfile::TempDir) -> PricingState {
    let mut state = PricingState::new(dir.path().join("rules.json"));
    state.ruleset = trend_rules();
    let cards = vec![
        card("1", "Alpha", "1.00", "2"),
        card("2", "Beta", "5.00", "1"),
        card("9", "Sold out", "1.00", "0"),
        card("3", "Gamma", "3.00", "1"),
        card("4", "Delta", "2.00", "1"),
    ];
    state.set_source("stock.csv".to_string(), cards, "Magic".to_string());
    state.set_market(MarketData {
        rows: [row(1, 1.5), row(2, 20.0), row(3, 2.5)]
            .into_iter()
            .map(|r| (r.id_product, r))
            .collect(),
        source: "test".to_string(),
    });
    state.refresh(today());
    state
}

fn names(state: &PricingState) -> Vec<&str> {
    state
        .rows
        .iter()
        .map(|&i| state.listings[i].name.as_str())
        .collect()
}

// ── Construction ────────────────────────────────────────────────────────────

#[test]
fn new_without_saved_rules_starts_from_starter() {
    let dir = tempfile::tempdir().unwrap();
    let state = PricingState::new(dir.path().join("missing.json"));
    assert_eq!(state.ruleset, Ruleset::starter());
    assert!(state.ruleset_note.is_none());
}

#[test]
fn new_loads_saved_rules() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rules.json");
    let rs = trend_rules();
    crate::repricing::save_ruleset(&path, &rs).unwrap();
    assert_eq!(PricingState::new(path).ruleset, rs);
}

#[test]
fn unreadable_saved_rules_are_moved_aside_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rules.json");
    std::fs::write(&path, "{ not json").unwrap();
    let state = PricingState::new(path.clone());
    assert_eq!(state.ruleset, Ruleset::starter());
    assert!(state.ruleset_note.as_ref().is_some_and(|n| n.error));
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("rules.broken.json")).unwrap(),
        "{ not json"
    );
}

// ── Stock & facts ───────────────────────────────────────────────────────────

#[test]
fn set_source_keeps_only_stock_and_offers_found_values() {
    let dir = tempfile::tempdir().unwrap();
    let state = loaded(&dir);
    assert_eq!(state.listings.len(), 4);
    assert_eq!(state.sold_out_rows, 1);
    assert_eq!(state.rarities[0], "Rare", "stock values first");
    assert!(
        state.rarities.iter().any(|r| r == "Common"),
        "defaults kept"
    );
    assert_eq!(state.product_ids(), vec![1, 2, 3, 4]);
    assert_eq!(state.market_coverage(), 3);
}

#[test]
fn new_source_drops_hand_made_ticks_and_prices() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = loaded(&dir);
    state.set_tick(0, false);
    state.set_manual(1, 9.0);
    state.selected_row = Some(1);
    state.set_source("again.csv".into(), state.cards.clone(), "Magic".into());
    assert!(state.ticks.is_empty());
    assert!(state.manual.is_empty());
    assert_eq!(state.selected_row, None);
}

#[test]
fn apply_facts_joins_by_variant_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = loaded(&dir);
    let key = card_variant_key(&state.cards[0], "Magic");
    let dates: HashMap<String, String> = [(key.clone(), "2026-01-01".to_string())].into();
    let sales: HashMap<String, Vec<(String, i64)>> =
        [(key, vec![("2026-11-01".to_string(), 2)])].into();
    state.apply_facts(Ok(dates), Ok(sales), today());

    assert_eq!(state.facts_status, FactsStatus::Loaded { known: 1 });
    let alpha = &state.listings[0].facts;
    assert_eq!(alpha.age_days, Some(309));
    assert_eq!(alpha.sold_30d, 2);
    assert_eq!(alpha.days_since_sale, Some(5));
    assert_eq!(state.listings[1].facts, ListingFacts::default());
}

#[test]
fn failed_facts_leave_history_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = loaded(&dir);
    state.apply_facts(Err("locked".into()), Ok(HashMap::new()), today());
    assert_eq!(state.facts_status, FactsStatus::Failed("locked".into()));
    assert!(state.listings.iter().all(|l| l.facts.age_days.is_none()));
}

// ── Evaluation ──────────────────────────────────────────────────────────────

#[test]
fn refresh_prices_everything_and_summarizes() {
    let dir = tempfile::tempdir().unwrap();
    let state = loaded(&dir);
    let outcomes: Vec<Outcome> = state.eval.proposals.iter().map(|p| p.outcome).collect();
    assert_eq!(
        outcomes,
        vec![
            Outcome::Raise,
            Outcome::Raise,
            Outcome::Lower,
            Outcome::NoMarketData
        ]
    );
    assert_eq!(state.summary.changes(), 3);
    assert_eq!(state.summary.review, 1, "Beta +€15 needs a tick");
    assert_eq!(state.summary.selected, 2);
}

#[test]
fn refresh_only_re_evaluates_when_something_changed() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = loaded(&dir);
    let v = state.eval_version;
    state.refresh(today());
    assert_eq!(state.eval_version, v, "nothing changed");
    state.ruleset.rules[0].action.percent = 50.0;
    state.refresh(today());
    assert_eq!(state.eval_version, v + 1, "rule edit");
    state.set_manual(3, 2.5);
    state.refresh(today());
    assert_eq!(state.eval_version, v + 2, "manual price");
    state.refresh(today().succ_opt().unwrap());
    assert_eq!(state.eval_version, v + 3, "new day");
}

// ── Review rows ─────────────────────────────────────────────────────────────

#[test]
fn default_view_lists_changes_by_impact() {
    let dir = tempfile::tempdir().unwrap();
    let state = loaded(&dir);
    // Impact: Beta +€15, Alpha +€1 (0.50 × 2), Gamma −€0.50.
    assert_eq!(names(&state), vec!["Beta", "Alpha", "Gamma"]);
}

#[test]
fn filters_narrow_the_rows() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = loaded(&dir);
    let view = |state: &mut PricingState, f: ReviewFilter| {
        state.filter = f;
        state.refresh(today());
        names(state)
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        view(&mut state, ReviewFilter::Raises),
        vec!["Beta", "Alpha"]
    );
    assert_eq!(view(&mut state, ReviewFilter::Lowers), vec!["Gamma"]);
    assert_eq!(view(&mut state, ReviewFilter::NeedsReview), vec!["Beta"]);
    assert_eq!(view(&mut state, ReviewFilter::Unchanged), vec!["Delta"]);
    assert_eq!(view(&mut state, ReviewFilter::All).len(), 4);

    // Ticking the reviewed change takes it off the "needs review" list.
    state.set_tick(1, true);
    assert!(view(&mut state, ReviewFilter::NeedsReview).is_empty());
}

#[test]
fn search_and_rule_filter() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = loaded(&dir);
    state.filter = ReviewFilter::All;
    state.search = "gam, DEL".to_string();
    state.refresh(today());
    assert_eq!(
        names(&state),
        vec!["Gamma", "Delta"],
        "a change outranks no change"
    );

    state.search.clear();
    state.ruleset.rules.insert(
        0,
        Rule {
            name: "Keep Alpha".into(),
            enabled: true,
            conditions: vec![Condition::Name("alpha".into())],
            action: PriceAction {
                base: PriceBase::Keep,
                ..PriceAction::default()
            },
        },
    );
    state.rule_filter = Some(0);
    state.refresh(today());
    assert_eq!(names(&state), vec!["Alpha"]);
}

#[test]
fn rule_filter_is_cleared_when_its_rule_disappears() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = loaded(&dir);
    state.rule_filter = Some(0);
    state.ruleset.rules.clear();
    state.refresh(today());
    assert_eq!(state.rule_filter, None);
}

#[test]
fn sort_by_toggles_and_picks_natural_direction() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = loaded(&dir);
    state.sort_by(ReviewSort::Name);
    assert!(!state.sort_desc, "text sorts A→Z first");
    state.refresh(today());
    assert_eq!(names(&state), vec!["Alpha", "Beta", "Gamma"]);
    state.sort_by(ReviewSort::Name);
    assert!(state.sort_desc);
    state.sort_by(ReviewSort::Change);
    assert!(state.sort_desc, "numbers sort largest first");
    state.refresh(today());
    assert_eq!(names(&state), vec!["Beta", "Alpha", "Gamma"]);
}

// ── Ticks & manual prices ───────────────────────────────────────────────────

#[test]
fn ticks_drive_summary() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = loaded(&dir);
    state.set_tick(0, false);
    assert_eq!(state.summary.selected, 1);
    state.tick_shown(true);
    assert_eq!(state.summary.selected, 3);
    assert_eq!(state.summary.review, 0);
    state.reset_ticks();
    assert_eq!(state.summary.selected, 2);
    assert!(!state.is_ticked(3), "unchanged rows are never ticked");
}

#[test]
fn manual_price_overrides_rules() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = loaded(&dir);
    state.set_manual(3, 2.75);
    state.refresh(today());
    let p = &state.eval.proposals[3];
    assert!(p.manual);
    assert_eq!(p.new_price, 2.75);
    assert!(state.is_ticked(3));
    state.set_manual(3, f64::NAN);
    assert_eq!(state.manual.get(&3), Some(&2.75), "invalid input ignored");
    state.clear_manual(3);
    state.refresh(today());
    assert_eq!(state.eval.proposals[3].outcome, Outcome::NoMarketData);
}

// ── Rule list edits ─────────────────────────────────────────────────────────

fn named(name: &str, conditions: Vec<Condition>) -> Rule {
    Rule {
        name: name.to_string(),
        conditions,
        ..Rule::new(name)
    }
}

fn rule_names(state: &PricingState) -> Vec<&str> {
    state
        .ruleset
        .rules
        .iter()
        .map(|r| r.name.as_str())
        .collect()
}

#[test]
fn add_rule_goes_before_a_trailing_catch_all() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = PricingState::new(dir.path().join("r.json"));
    state.ruleset.rules = vec![named("all", vec![])];
    state.add_rule(named("rares", vec![Condition::Rarity(vec!["Rare".into()])]));
    assert_eq!(rule_names(&state), vec!["rares", "all"]);
    assert_eq!(state.selected_rule, Some(0));
    state.add_rule(named("another all", vec![]));
    assert_eq!(rule_names(&state), vec!["rares", "all", "another all"]);
}

#[test]
fn move_duplicate_delete_keep_selection_in_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = PricingState::new(dir.path().join("r.json"));
    state.ruleset.rules = vec![named("a", vec![]), named("b", vec![]), named("c", vec![])];
    state.selected_rule = Some(0);
    state.rule_filter = Some(1);

    state.move_rule(0, false);
    assert_eq!(rule_names(&state), vec!["b", "a", "c"]);
    assert_eq!(state.selected_rule, Some(1));
    assert_eq!(state.rule_filter, Some(0));
    state.move_rule(0, true);
    assert_eq!(
        rule_names(&state),
        vec!["b", "a", "c"],
        "top rule can't go up"
    );

    state.duplicate_rule(2);
    assert_eq!(rule_names(&state), vec!["b", "a", "c", "c (copy)"]);
    assert_eq!(state.selected_rule, Some(3));

    state.selected_rule = Some(2);
    state.rule_filter = Some(3);
    state.delete_rule(0);
    assert_eq!(rule_names(&state), vec!["a", "c", "c (copy)"]);
    assert_eq!(state.selected_rule, Some(1));
    assert_eq!(state.rule_filter, Some(2));
    state.delete_rule(1);
    assert_eq!(state.selected_rule, None, "deleted rule deselected");
}

#[test]
fn replace_ruleset_can_be_undone() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = PricingState::new(dir.path().join("r.json"));
    let mine = trend_rules();
    state.ruleset = mine.clone();
    state.replace_ruleset(Ruleset::starter(), Note::ok("replaced"));
    assert_eq!(state.ruleset, Ruleset::starter());
    state.undo_replace();
    assert_eq!(state.ruleset, mine);
    assert!(state.undo_ruleset.is_none());
}

// ── Autosave ────────────────────────────────────────────────────────────────

#[test]
fn autosave_waits_for_edits_to_settle() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rules.json");
    let mut state = PricingState::new(path.clone());
    let t0 = Instant::now();
    assert_eq!(state.autosave(t0), None, "nothing changed");

    state.ruleset.name = "Edited".to_string();
    assert!(state.autosave(t0).is_some());
    assert!(!path.exists(), "not saved mid-edit");
    assert_eq!(state.autosave(t0 + AUTOSAVE_DELAY), None);
    assert_eq!(
        crate::repricing::load_ruleset(&path).unwrap().name,
        "Edited"
    );
}

#[test]
fn autosave_reports_write_errors_and_keeps_edits() {
    let dir = tempfile::tempdir().unwrap();
    // A file where the folder should be makes the write fail.
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, "").unwrap();
    let mut state = PricingState::new(blocker.join("rules.json"));
    state.ruleset.name = "Edited".to_string();
    let t0 = Instant::now();
    state.autosave(t0);
    assert!(
        state.autosave(t0 + AUTOSAVE_DELAY).is_some(),
        "retries later"
    );
    assert!(state.ruleset_note.as_ref().is_some_and(|n| n.error));
    assert_eq!(state.ruleset.name, "Edited");
}

// ── Export ──────────────────────────────────────────────────────────────────

#[test]
fn export_writes_new_prices_and_undo_file() {
    let dir = tempfile::tempdir().unwrap();
    let state = loaded(&dir);
    let path = dir.path().join("out.csv");
    let report = state.export_to(&path).unwrap();
    assert_eq!(report.changes, 2, "Alpha and Gamma; Beta waits for review");
    assert_eq!(report.undo, dir.path().join("out_undo.csv"));

    let read = |p: &Path| -> Vec<(String, String)> {
        let mut rdr = csv::Reader::from_path(p).unwrap();
        rdr.records()
            .map(|r| {
                let r = r.unwrap();
                (r[2].to_string(), r[12].to_string())
            })
            .collect()
    };
    assert_eq!(
        read(&report.csv),
        vec![
            ("Alpha".to_string(), "1.50".to_string()),
            ("Gamma".to_string(), "2.50".to_string())
        ]
    );
    assert_eq!(
        read(&report.undo),
        vec![
            ("Alpha".to_string(), "1.00".to_string()),
            ("Gamma".to_string(), "3.00".to_string())
        ]
    );
}

#[test]
fn export_with_nothing_ticked_fails_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = loaded(&dir);
    state.filter = ReviewFilter::All;
    state.refresh(today());
    state.tick_shown(false);
    let path = dir.path().join("out.csv");
    assert!(state.export_to(&path).is_err());
    assert!(!path.exists());
}

// ── Helpers ─────────────────────────────────────────────────────────────────

#[test]
fn file_name_helpers() {
    assert_eq!(
        undo_path(Path::new("/tmp/x/prices.csv")),
        PathBuf::from("/tmp/x/prices_undo.csv")
    );
    assert_eq!(export_file_name(today()), "repricing_2026-11-06.csv");
    assert_eq!(
        sanitize_file_stem(" My rules / v2 ").as_deref(),
        Some("My_rules___v2")
    );
    assert_eq!(sanitize_file_stem("../.."), None);
}

#[test]
fn thousands_separator() {
    assert_eq!(thousands(0), "0");
    assert_eq!(thousands(999), "999");
    assert_eq!(thousands(1000), "1,000");
    assert_eq!(thousands(1234567), "1,234,567");
    assert_eq!(thousands(-12345), "-12,345");
}

#[test]
fn guide_rows_cover_requested_ids_with_guide_date() {
    use crate::api::cardmarket::PriceGuideEntry;
    let entry = |id: u64, trend: f64| PriceGuideEntry {
        id_product: id,
        id_category: 1,
        avg: None,
        low: Some(trend - 0.5),
        trend: Some(trend),
        avg1: None,
        avg7: None,
        avg30: None,
        avg_foil: None,
        low_foil: None,
        trend_foil: Some(trend * 2.0),
        avg1_foil: None,
        avg7_foil: None,
        avg30_foil: None,
    };
    let guide = PriceGuide::from_entries(
        vec![entry(1, 2.0), entry(2, 3.0)],
        "2026-11-05T02:00:00+0100",
    );
    let rows = rows_from_guide(&guide, &[1, 7]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[&1].price_date, "2026-11-05");
    assert_eq!(rows[&1].trend_foil, Some(4.0));
    assert_eq!(rows[&1].low, Some(1.5));
    assert_eq!(newest_date(&rows).as_deref(), Some("2026-11-05"));
}

#[test]
fn merge_defaults_appends_missing_values() {
    let found = vec!["German".to_string(), "english".to_string()];
    assert_eq!(
        merge_defaults(found, &["English", "French"]),
        vec!["German", "english", "French"]
    );
}
