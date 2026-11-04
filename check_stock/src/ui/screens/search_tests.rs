//! Tests for the Search screen's discard warning.

use super::*;
use crate::models::Card;

/// Runs a few frames of the screen headlessly and returns every painted text.
fn painted_texts(app_state: &mut AppState, state: &mut SearchState) -> Vec<String> {
    let ctx = egui::Context::default();
    let mut picking = PickingState::default();
    let mut output = None;
    for _ in 0..2 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            ..Default::default()
        };
        output = Some(ctx.run(input, |ctx| {
            SearchScreen::show(ctx, app_state, state, &mut picking)
        }));
    }
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut texts = Vec::new();
    for clipped in &output.unwrap().shapes {
        walk(&clipped.shape, &mut texts);
    }
    texts
}

fn selected(quantity: i32, price: &str) -> SelectedSearchCard {
    SelectedSearchCard {
        card: Card {
            price: price.to_string(),
            ..Card::test_default()
        },
        quantity,
    }
}

#[test]
fn the_discard_warning_explains_the_import_order_and_waits() {
    let mut app = AppState::default();
    let mut state = SearchState {
        action_mode: SearchAction::Discard,
        discard_confirm: true,
        selected_cards: vec![selected(2, "0.50"), selected(1, "1,00")],
        ..SearchState::default()
    };
    let texts = painted_texts(&mut app, &mut state);
    let all = texts.join("\n");
    assert!(all.contains("Import order matters"), "{all}");
    assert!(all.contains("3 copies listed at €2.00"), "{all}");
    for step in DISCARD_STEPS {
        assert!(texts.iter().any(|t| t == step), "missing step {step:?}");
    }
    assert!(
        state.discard_confirm,
        "nothing is saved until the user confirms"
    );
    assert_eq!(state.selected_cards.len(), 2);
}

#[test]
fn the_discard_warning_closes_itself_without_a_selection() {
    let mut app = AppState::default();
    let mut state = SearchState {
        discard_confirm: true,
        ..SearchState::default()
    };
    painted_texts(&mut app, &mut state);
    assert!(!state.discard_confirm);
}
