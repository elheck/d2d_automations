//! History tab: every recorded lot, its status, and the few corrections a
//! lot may need (price, confirm removal, undo).

use super::widgets::{self, Tone};
use super::{BulkLotState, Confirm, Tab, STALE_PENDING_DAYS};
use crate::inventory_db::{self, BulkLotSummary, MAX_LOT_PRICE_EUR};
use crate::ui::state::AppState;
use crate::ui::style;
use chrono::NaiveDate;
use eframe::egui;

/// What the user did in the table this frame.
enum RowAction {
    EditPrice(i64, f64),
    SavePrice,
    CancelPrice,
    ToggleCards(i64),
    Confirm(Confirm),
}

pub(super) fn show(
    ctx: &egui::Context,
    app_state: &mut AppState,
    state: &mut BulkLotState,
    today: NaiveDate,
) {
    egui::CentralPanel::default().show(ctx, |ui| {
        egui::ScrollArea::vertical()
            .id_salt("bulk_lot_history_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(err) = state.history_error.clone() {
                    let mut dismiss = false;
                    widgets::callout(ui, Tone::Error, "Something went wrong", Some(&err), |ui| {
                        dismiss = ui.small_button("Dismiss").clicked();
                    });
                    if dismiss {
                        state.history_error = None;
                    }
                    ui.add_space(8.0);
                }
                if state.history.is_empty() {
                    show_empty(ui, state);
                    return;
                }
                show_totals(ui, state, today);
                ui.add_space(6.0);
                widgets::hint(
                    ui,
                    "Lot prices count as revenue in Stock Analysis (revenue per week, Lot Cost & \
                     Margin) and the welcome digest. Their cards never count as single sales or \
                     restock candidates.",
                );
                ui.add_space(8.0);
                if let Some(action) = show_table(ui, state, today) {
                    apply(app_state, state, action);
                }
                show_selected(ui, state);
            });
    });
}

fn show_empty(ui: &mut egui::Ui, state: &mut BulkLotState) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new("No bulk lots yet")
                .size(18.0)
                .strong()
                .color(style::TEXT_PRIMARY),
        );
        ui.add_space(4.0);
        widgets::hint(
            ui,
            "Lots you finish appear here, with their revenue and whether an inventory import \
             has seen the cards leave your stock.",
        );
        ui.add_space(12.0);
        if style::primary_button(ui, "Build a lot").clicked() {
            state.tab = Tab::NewLot;
        }
    });
}

fn show_totals(ui: &mut egui::Ui, state: &BulkLotState, today: NaiveDate) {
    let copies: i64 = state.history.iter().map(|l| l.copies).sum();
    let revenue: f64 = state.history.iter().map(|l| l.price).sum();
    let listed: f64 = state.history.iter().map(|l| l.listed_value).sum();
    let pending = state.pending_copies();
    let stale = state
        .history
        .iter()
        .any(|l| l.pending_copies > 0 && age_days(l, today) > STALE_PENDING_DAYS);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
        widgets::stat_tile(
            ui,
            "Lots",
            &widgets::count(state.history.len() as i64),
            "recorded",
            style::TEXT_PRIMARY,
        );
        widgets::stat_tile(
            ui,
            "Cards",
            &widgets::count(copies),
            &format!("listed at {} as singles", widgets::euro(listed)),
            style::TEXT_PRIMARY,
        );
        widgets::stat_tile(
            ui,
            "Revenue",
            &widgets::euro(revenue),
            &format!(
                "{} per card on average",
                widgets::share(revenue / copies.max(1) as f64)
            ),
            style::COLOR_SUCCESS,
        );
        widgets::stat_tile(
            ui,
            "Awaiting import",
            &widgets::count(pending),
            if pending == 0 {
                "all removals confirmed"
            } else {
                "cards still in your stock data"
            },
            if stale {
                style::COLOR_WARNING
            } else {
                style::TEXT_PRIMARY
            },
        )
        .on_hover_text(
            "Cards of recorded lots that no inventory import has seen leave your Cardmarket \
             stock yet. They are reserved and never offered for another lot.",
        );
    });
}

fn age_days(lot: &BulkLotSummary, today: NaiveDate) -> i64 {
    NaiveDate::parse_from_str(&lot.created_at, "%Y-%m-%d")
        .map(|d| (today - d).num_days())
        .unwrap_or(0)
}

fn show_table(ui: &mut egui::Ui, state: &mut BulkLotState, today: NaiveDate) -> Option<RowAction> {
    let mut action = None;
    let selected = state.selected_lot.as_ref().map(|(id, _)| *id);
    style::section_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::Grid::new("bulk_lot_history")
            .num_columns(8)
            .striped(true)
            .spacing([18.0, 8.0])
            .show(ui, |ui| {
                for title in [
                    "Date",
                    "Lot",
                    "Cards",
                    "Lot price",
                    "Per card",
                    "Single value",
                    "Status",
                    "",
                ] {
                    ui.label(egui::RichText::new(title).strong());
                }
                ui.end_row();

                for lot in &state.history {
                    ui.label(&lot.created_at);
                    ui.label(egui::RichText::new(&lot.name).strong());
                    ui.label(widgets::count(lot.copies));

                    // Price: click to edit in place.
                    match &mut state.price_edit {
                        Some((id, value)) if *id == lot.id => {
                            ui.horizontal(|ui| {
                                let field = ui.add(
                                    egui::DragValue::new(value)
                                        .range(0.0..=MAX_LOT_PRICE_EUR)
                                        .speed(0.05)
                                        .fixed_decimals(2)
                                        .prefix("€"),
                                );
                                if ui.small_button("✔").on_hover_text("Save").clicked()
                                    || (field.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                                {
                                    action = Some(RowAction::SavePrice);
                                }
                                if ui.small_button("✕").on_hover_text("Cancel").clicked() {
                                    action = Some(RowAction::CancelPrice);
                                }
                            });
                        }
                        _ => {
                            let price = ui
                                .add(
                                    egui::Label::new(format!("{}  ✎", widgets::euro(lot.price)))
                                        .sense(egui::Sense::click()),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text("Correct the price the lot sold for");
                            if price.clicked() {
                                action = Some(RowAction::EditPrice(lot.id, lot.price));
                            }
                        }
                    }

                    ui.label(widgets::share(lot.per_card()));
                    ui.label(
                        egui::RichText::new(widgets::euro(lot.listed_value))
                            .color(style::TEXT_MUTED),
                    );
                    status(ui, lot, today);

                    ui.horizontal(|ui| {
                        let open = selected == Some(lot.id);
                        if ui
                            .selectable_label(open, "Cards")
                            .on_hover_text("List the cards of this lot")
                            .clicked()
                        {
                            action = Some(RowAction::ToggleCards(lot.id));
                        }
                        if lot.pending_copies > 0
                            && ui
                                .small_button("Mark removed")
                                .on_hover_text("The cards already left your Cardmarket stock")
                                .clicked()
                        {
                            action = Some(RowAction::Confirm(Confirm::MarkRemoved(lot.clone())));
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Undo").color(style::COLOR_ERROR),
                                )
                                .small(),
                            )
                            .on_hover_text("Delete the lot and its revenue")
                            .clicked()
                        {
                            action = Some(RowAction::Confirm(Confirm::UndoLot(lot.clone())));
                        }
                    });
                    ui.end_row();
                }
            });
    });
    action
}

fn status(ui: &mut egui::Ui, lot: &BulkLotSummary, today: NaiveDate) {
    if lot.pending_copies == 0 {
        ui.label(egui::RichText::new("✔ Removed").color(style::COLOR_SUCCESS))
            .on_hover_text("An inventory import confirmed the cards left your Cardmarket stock.");
        return;
    }
    let days = age_days(lot, today);
    if days > STALE_PENDING_DAYS {
        ui.label(
            egui::RichText::new(format!(
                "⚠ {} awaiting import · {days} days",
                widgets::count(lot.pending_copies)
            ))
            .color(style::COLOR_WARNING),
        )
        .on_hover_text(format!(
            "No inventory import has seen these cards leave your stock in {days} days. Did you \
             import the CSV into TCG Powertools and load a fresh export afterwards? If your \
             Cardmarket stock is already right, use “Mark removed”."
        ));
    } else {
        ui.label(
            egui::RichText::new(format!(
                "⏳ {} of {} awaiting import",
                widgets::count(lot.pending_copies),
                widgets::count(lot.copies)
            ))
            .color(style::TEXT_MUTED),
        )
        .on_hover_text(
            "Import the CSV into TCG Powertools, then load a fresh inventory export here or in \
             any other screen. Until then the cards still show as stock elsewhere.",
        );
    }
}

fn apply(app_state: &mut AppState, state: &mut BulkLotState, action: RowAction) {
    match action {
        RowAction::EditPrice(id, price) => state.price_edit = Some((id, price)),
        RowAction::CancelPrice => state.price_edit = None,
        RowAction::SavePrice => {
            let Some((id, price)) = state.price_edit.take() else {
                return;
            };
            match inventory_db::set_bulk_lot_price(id, price) {
                Ok(()) => app_state.mark_db_changed(),
                Err(e) => state.history_error = Some(format!("Could not change the price: {e}")),
            }
        }
        RowAction::ToggleCards(id) => {
            state.selected_lot = match &state.selected_lot {
                Some((open, _)) if *open == id => None,
                _ => Some((
                    id,
                    inventory_db::get_bulk_lot_cards(id).map_err(|e| e.to_string()),
                )),
            };
        }
        RowAction::Confirm(confirm) => state.confirm = Some(confirm),
    }
}

fn show_selected(ui: &mut egui::Ui, state: &mut BulkLotState) {
    let Some((id, cards)) = &state.selected_lot else {
        return;
    };
    let name = state
        .history
        .iter()
        .find(|l| l.id == *id)
        .map(|l| l.name.clone())
        .unwrap_or_default();
    let mut close = false;
    ui.add_space(12.0);
    style::section_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::Sides::new().show(
            ui,
            |ui| {
                ui.label(
                    egui::RichText::new(format!("Cards in “{name}”"))
                        .size(15.0)
                        .strong()
                        .color(style::TEXT_PRIMARY),
                );
            },
            |ui| {
                close = ui.small_button("✕ Close").clicked();
            },
        );
        ui.add_space(6.0);
        match cards {
            Err(e) => style::status_error(ui, &format!("Could not load the cards: {e}")),
            Ok(cards) => {
                egui::Grid::new("bulk_lot_history_cards")
                    .num_columns(8)
                    .striped(true)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        for title in [
                            "Location",
                            "Card",
                            "Set",
                            "Cond.",
                            "Language",
                            "Copies",
                            "Listed at",
                            "Purchase lot",
                        ] {
                            ui.label(egui::RichText::new(title).strong());
                        }
                        ui.end_row();
                        for card in cards {
                            ui.label(egui::RichText::new(&card.location).monospace().size(12.0));
                            ui.horizontal(|ui| {
                                ui.label(&card.name);
                                if card.foil {
                                    widgets::badge(ui, "FOIL", style::ACCENT);
                                }
                                if card.pending_copies > 0 {
                                    widgets::badge(ui, "AWAITING IMPORT", style::TEXT_MUTED);
                                }
                            });
                            ui.label(egui::RichText::new(&card.set_name).color(style::TEXT_MUTED));
                            ui.label(&card.condition);
                            ui.label(&card.language);
                            ui.label(widgets::count(card.copies));
                            ui.label(widgets::euro(card.listed_price));
                            ui.label(if card.purchase_lot.is_empty() {
                                "—"
                            } else {
                                &card.purchase_lot
                            });
                            ui.end_row();
                        }
                    });
            }
        }
    });
    if close {
        state.selected_lot = None;
    }
}
