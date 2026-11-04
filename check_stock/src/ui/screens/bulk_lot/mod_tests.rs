//! Tests for the Bulk Lots screen state (no egui involved).

use super::*;

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 4).unwrap()
}

/// A Magic inventory row: one copy, listed 200 days ago.
fn row(id: &str, name: &str, price: &str, location: &str) -> Card {
    Card {
        cardmarket_id: id.to_string(),
        name: name.to_string(),
        quantity: "1".to_string(),
        price: price.to_string(),
        cn: id.to_string(),
        set_code: "TST".to_string(),
        location: Some(location.to_string()),
        rarity: "Common".to_string(),
        listed_at: "2026-04-18".to_string(),
        ..Card::test_default()
    }
}

/// A state with a temp type cache and the given rows loaded and built.
fn loaded(cards: Vec<Card>) -> (BulkLotState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let mut state =
        BulkLotState::with_type_cache(CardTypeCache::with_dir(dir.path().to_path_buf()));
    state.set_source(
        "inventory.csv".to_string(),
        cards,
        DEFAULT_CATEGORY.to_string(),
    );
    state.rebuild_candidates(&HashMap::new(), &[], today());
    state.spec.size = 2;
    // These tests check the flow, not location gathering.
    state.spec.order = crate::bulk_lot::LotOrder::CheapestFirst;
    state.refresh_preview(today());
    (state, dir)
}

fn three_rows() -> Vec<Card> {
    vec![
        row("1", "Alpha", "0.10", "A-0-0-3-L1-R"),
        row("2", "Beta", "0.05", "A-0-0-1-L1-R"),
        row("3", "Gamma", "0.20", "A-0-0-2-L2-R"),
    ]
}

fn line_names(state: &BulkLotState) -> Vec<String> {
    state
        .lines
        .iter()
        .map(|l| state.candidates[l.pick.candidate].name.clone())
        .collect()
}

#[test]
fn a_loaded_export_yields_a_preview_sorted_by_location() {
    let (state, _dir) = loaded(three_rows());
    assert!(state.has_magic_source());
    assert_eq!(state.preview_copies(), 2);
    let names: Vec<&str> = state
        .preview
        .iter()
        .map(|p| state.candidates[p.candidate].name.as_str())
        .collect();
    // Same age: cheapest first picks Beta and Alpha; shown in shelf order.
    assert_eq!(names, ["Beta", "Alpha"]);
}

#[test]
fn a_generic_export_is_not_a_card_source() {
    let dir = tempfile::tempdir().unwrap();
    let mut state =
        BulkLotState::with_type_cache(CardTypeCache::with_dir(dir.path().to_path_buf()));
    state.set_source("generic.csv".into(), three_rows(), "Generic".into());
    assert!(!state.has_magic_source());
}

#[test]
fn languages_are_listed_by_copies() {
    let mut cards = three_rows();
    cards[0].language = "German".to_string();
    cards[1].quantity = "5".to_string();
    let (state, _dir) = loaded(cards);
    assert_eq!(
        state.languages,
        vec![("English".to_string(), 6), ("German".to_string(), 1)]
    );
}

#[test]
fn the_preview_is_only_recomputed_when_the_rules_change() {
    let (mut state, _dir) = loaded(three_rows());
    assert!(!state.refresh_preview(today()));
    state.spec.size = 3;
    assert!(state.refresh_preview(today()));
    assert_eq!(state.preview_copies(), 3);
}

#[test]
fn the_suggested_name_follows_the_rules_until_the_user_types_one() {
    let (mut state, _dir) = loaded(three_rows());
    assert_eq!(state.name, "2 Commons");
    state.spec.size = 3;
    state.refresh_preview(today());
    assert_eq!(state.name, "3 Commons");

    state.name = "My lot".to_string();
    state.name_touched = true;
    state.spec.size = 1;
    state.refresh_preview(today());
    assert_eq!(state.name, "My lot");
}

#[test]
fn a_type_filter_waits_for_type_lines_and_uses_them() {
    let (mut state, _dir) = loaded(three_rows());
    state.spec.card_type = CardType::NonBasicLand;
    state.refresh_preview(today());
    assert!(state.types_incomplete());
    assert_eq!(state.preview_copies(), 0, "unknown types never pass");
    assert_eq!(state.lookup_printings().len(), 3);
    assert_eq!(state.lookup_printings()[0].cardmarket_id, Some(1));

    state.apply_type_outcome(
        LookupOutcome {
            found: vec![
                ("TST".into(), "1".into(), "Land — Gate".into()),
                ("TST".into(), "2".into(), "Creature — Elf".into()),
            ],
            missing: vec![("TST".into(), "3".into())],
            ..LookupOutcome::default()
        },
        today(),
    );
    state.refresh_preview(today());
    assert!(!state.types_incomplete(), "found and missing are settled");
    assert_eq!(state.preview_copies(), 1);
    assert_eq!(state.candidates[state.preview[0].candidate].name, "Alpha");
}

#[test]
fn a_failed_lookup_pauses_until_resumed_or_the_type_changes() {
    let (mut state, _dir) = loaded(three_rows());
    state.spec.card_type = CardType::Creature;
    state.refresh_preview(today());
    state.apply_type_outcome(
        LookupOutcome {
            error: Some("Scryfall lookup failed: timeout".into()),
            ..LookupOutcome::default()
        },
        today(),
    );
    assert!(state.type_paused);
    assert!(state.type_error.is_some());

    state.spec.card_type = CardType::NonBasicLand;
    state.refresh_preview(today());
    assert!(!state.type_paused, "a new type is a new request");

    state.apply_type_outcome(
        LookupOutcome {
            cancelled: true,
            ..LookupOutcome::default()
        },
        today(),
    );
    assert!(state.type_paused && state.type_error.is_none());
    state.resume_types();
    assert!(!state.type_paused);
}

#[test]
fn picking_ticks_and_exports_the_picked_rows() {
    let (mut state, _dir) = loaded(three_rows());
    state.start_picking();
    assert_eq!(state.step, Step::Pick);
    assert_eq!(state.planned_copies(), 2);
    assert_eq!(state.picked_copies(), 0);

    state.toggle_picked(0);
    assert_eq!(state.picked_copies(), 1);
    let lines = state.picked_lines();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].0.name, "Beta");
    assert_eq!(lines[0].1, 1);

    state.set_all_picked(true);
    assert_eq!(state.picked_summary().copies, 2);
    state.set_all_picked(false);
    assert!(state.picked_lines().is_empty());
}

#[test]
fn replacing_a_card_swaps_in_the_next_match_in_place() {
    let (mut state, _dir) = loaded(three_rows());
    state.start_picking();
    assert_eq!(line_names(&state), ["Beta", "Alpha"]);

    let incoming = state.replace(0);
    assert_eq!(incoming.len(), 1);
    assert_eq!(line_names(&state), ["Gamma", "Alpha"]);
    assert!(state
        .pick_notice
        .as_ref()
        .unwrap()
        .contains("Beta replaced by Gamma"));

    // Nothing left to swap in: the lot shrinks and says so.
    assert!(state.replace(0).is_empty());
    assert_eq!(line_names(&state), ["Alpha"]);
    assert!(state.pick_notice.as_ref().unwrap().contains("now 1 card"));
    assert!(state.replace(5).is_empty(), "out of range is ignored");
}

#[test]
fn leaving_and_finishing_reset_the_picking_list() {
    let (mut state, _dir) = loaded(three_rows());
    state.start_picking();
    state.toggle_picked(0);
    state.back_to_define();
    assert_eq!(state.step, Step::Define);
    assert!(state.lines.is_empty());

    state.start_picking();
    state.finish = Some(FinishDialog {
        name: "Lot".into(),
        price: 2.5,
        error: None,
        saved_csv: None,
    });
    state.finish_recorded(DoneInfo {
        name: "Lot".into(),
        copies: 2,
        price: 2.5,
        csv: PathBuf::from("lot.csv"),
    });
    assert_eq!(state.step, Step::Done);
    assert!(state.finish.is_none() && state.lines.is_empty());
    assert_eq!(state.price, 2.5, "the next lot starts from the last price");

    state.build_another();
    assert_eq!(state.step, Step::Define);
    assert!(state.done.is_none());
    assert!(!state.name_touched);
}

#[test]
fn copies_promised_to_pending_lots_are_left_out() {
    let (mut state, _dir) = loaded(three_rows());
    let beta = state.source.as_ref().unwrap().cards[1].clone();
    let pending = vec![PendingRemoval {
        id: 1,
        bulk_lot_id: 1,
        variant: inventory_db::card_variant_key(&beta, DEFAULT_CATEGORY),
        purchase_lot: "L1".into(),
        location: "A-0-0-1-L1-R".into(),
        pending: 1,
    }];
    state.rebuild_candidates(&HashMap::new(), &pending, today());
    state.refresh_preview(today());
    assert_eq!(state.funnel.reserved, 1);
    let names: Vec<&str> = state
        .preview
        .iter()
        .map(|p| state.candidates[p.candidate].name.as_str())
        .collect();
    assert_eq!(names, ["Gamma", "Alpha"], "Beta is reserved");
}

#[test]
fn pending_copies_sum_the_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut state =
        BulkLotState::with_type_cache(CardTypeCache::with_dir(dir.path().to_path_buf()));
    let lot = |id, pending| BulkLotSummary {
        id,
        name: "Lot".into(),
        created_at: "2026-11-01".into(),
        price: 1.0,
        copies: 10,
        pending_copies: pending,
        listed_value: 2.0,
    };
    state.history = vec![lot(1, 3), lot(2, 0), lot(3, 4)];
    assert_eq!(state.pending_copies(), 7);
}

#[test]
fn widget_number_formatting() {
    assert_eq!(widgets::count(0), "0");
    assert_eq!(widgets::count(999), "999");
    assert_eq!(widgets::count(14_435), "14,435");
    assert_eq!(widgets::count(-1_234_567), "-1,234,567");
    assert_eq!(widgets::cards(1), "1 card");
    assert_eq!(widgets::cards(2), "2 cards");
    assert_eq!(widgets::euro(1.0), "€1.00");
    assert_eq!(widgets::euro(0.05), "€0.05");
    assert_eq!(widgets::share(0.01), "€0.01");
    assert_eq!(widgets::share(0.0125), "€0.0125");
    assert_eq!(widgets::share(1.0 / 300.0), "€0.0033");
    assert_eq!(widgets::share(0.5), "€0.50");
    assert_eq!(widgets::share(0.0), "€0.00");
}

// ── Headless render ──────────────────────────────────────────────────────────

/// Runs a few frames of the whole screen without a window, so every layout
/// path executes. Generations are pinned so nothing touches the real database.
fn render(app_state: &mut AppState, state: &mut BulkLotState) -> egui::FullOutput {
    state.history_generation = Some(app_state.db_generation);
    state.candidates_generation = Some(app_state.db_generation);
    let ctx = egui::Context::default();
    let mut output = None;
    for _ in 0..3 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 900.0),
            )),
            ..Default::default()
        };
        output = Some(ctx.run(input, |ctx| BulkLotScreen::show(ctx, app_state, state)));
    }
    output.expect("rendered at least once")
}

/// Every painted text with its on-screen rectangle.
fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(text) => {
                out.push((text.galley.text().to_string(), shape.visual_bounding_rect()))
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

fn text_rect(texts: &[(String, egui::Rect)], wanted: &str) -> egui::Rect {
    texts
        .iter()
        .find(|(t, _)| t == wanted)
        .map(|(_, r)| *r)
        .unwrap_or_else(|| panic!("{wanted:?} not painted; got {texts:?}"))
}

#[test]
fn every_step_renders_headlessly() {
    // No set codes: tiles never ask Scryfall for images during the test.
    let mut cards = three_rows();
    for c in &mut cards {
        c.set_code.clear();
    }
    let (mut state, _dir) = loaded(cards);
    let mut app = AppState::default();

    render(&mut app, &mut state);
    assert_eq!(state.step, Step::Define);

    state.spec.hits = 1;
    state.spec.hit_min_price = 0.15;
    state.spec.size = 50; // short: shows the shortfall notice and funnel
    render(&mut app, &mut state);

    state.start_picking();
    render(&mut app, &mut state);
    state.toggle_picked(0);
    state.replace(1);
    render(&mut app, &mut state);

    finish::open(&mut state);
    render(&mut app, &mut state);
    assert!(
        state.finish.is_some(),
        "the dialog stays open until answered"
    );

    state.finish_recorded(DoneInfo {
        name: "50 Commons".into(),
        copies: 1,
        price: 1.0,
        csv: PathBuf::from("/tmp/lot.csv"),
    });
    render(&mut app, &mut state);

    let lot = BulkLotSummary {
        id: 1,
        name: "50 Commons".into(),
        created_at: "2026-10-01".into(),
        price: 1.0,
        copies: 50,
        pending_copies: 12,
        listed_value: 6.0,
    };
    state.tab = Tab::History;
    state.history = vec![lot.clone()];
    state.selected_lot = Some((1, Ok(Vec::new())));
    state.price_edit = Some((1, 2.0));
    render(&mut app, &mut state);
    state.confirm = Some(Confirm::UndoLot(lot));
    render(&mut app, &mut state);
    assert!(state.confirm.is_some(), "the question stays until answered");
}

#[test]
fn the_start_page_renders_without_an_export() {
    let dir = tempfile::tempdir().unwrap();
    let mut state =
        BulkLotState::with_type_cache(CardTypeCache::with_dir(dir.path().to_path_buf()));
    let mut app = AppState::default();
    render(&mut app, &mut state);
    state.set_source("generic.csv".into(), three_rows(), "Generic".into());
    render(&mut app, &mut state);
    state.tab = Tab::History;
    render(&mut app, &mut state);
}

#[test]
fn a_pick_tile_stacks_location_above_its_buttons() {
    // Regression: the tile inherited the grid cell's left-to-right layout, so
    // the location was painted on the same line as (under) the buttons.
    let mut cards = three_rows();
    for c in &mut cards {
        c.set_code.clear();
    }
    let (mut state, _dir) = loaded(cards);
    state.spec.size = 1;
    state.refresh_preview(today());
    state.start_picking();
    let mut app = AppState::default();
    let painted = texts(&render(&mut app, &mut state));

    let location = text_rect(&painted, "A-0-0-1-L1-R");
    let name = text_rect(&painted, "Beta");
    let mark = text_rect(&painted, "Mark picked");
    let replace = text_rect(&painted, "⟲ Replace");
    assert!(location.max.y <= name.min.y, "location above the name");
    assert!(name.max.y <= mark.min.y, "name above the buttons");
    assert!(location.max.y <= replace.min.y, "location above Replace");
    assert!(
        (location.min.x - mark.min.x).abs() < 40.0,
        "same column: {location:?} vs {mark:?}"
    );
}

#[test]
fn stat_tiles_stack_caption_above_value() {
    let (mut state, _dir) = loaded(three_rows());
    let mut app = AppState::default();
    let painted = texts(&render(&mut app, &mut state));
    let caption = text_rect(&painted, "LOT PRICE");
    let value = text_rect(&painted, "€1.00");
    assert!(caption.max.y <= value.min.y, "{caption:?} vs {value:?}");
}
