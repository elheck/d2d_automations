//! Detail view of one precon product: KPIs, EV history chart, card table.

use crate::{
    api::mtgjson::DeckBoard,
    precon_ev::{ranked_by_unit, summarize, top_n_lines, value_cards, CardValue, PreconRow, TOP_N},
    ui::{
        components::{line_chart, Series},
        state::PreconEvState,
        style,
    },
};
use eframe::egui;
use std::collections::HashSet;

const SEALED_COLOR: egui::Color32 = egui::Color32::from_rgb(230, 160, 60);
const TOP5_COLOR: egui::Color32 = egui::Color32::from_rgb(150, 110, 210);

fn euro(v: Option<f64>) -> String {
    v.map_or_else(|| "—".to_string(), |v| format!("€{v:.2}"))
}

fn board_label(board: DeckBoard) -> &'static str {
    match board {
        DeckBoard::Commander => "Commander",
        DeckBoard::Main => "Main",
        DeckBoard::Side => "Side",
        DeckBoard::Display => "Display",
    }
}

pub(super) fn show(ui: &mut egui::Ui, state: &PreconEvState, id: u64) {
    let (Some(data), Some(row)) = (&state.data, state.rows.iter().find(|r| r.id_product == id))
    else {
        style::status_error(ui, "Product not found — reload the precons.");
        return;
    };

    ui.label(
        egui::RichText::new(&row.name)
            .size(18.0)
            .strong()
            .color(style::TEXT_PRIMARY),
    );
    let Some(decks) = data.decks_for(id) else {
        style::status_error(ui, &row.notes().0);
        return;
    };
    let deck_names: Vec<&str> = decks.iter().map(|d| d.name.as_str()).collect();
    ui.label(
        egui::RichText::new(format!(
            "{} · released {} · Cardmarket #{id} · {}",
            if row.set_label.is_empty() {
                "—"
            } else {
                &row.set_label
            },
            row.release_date.as_deref().unwrap_or("—"),
            deck_names.join(", ")
        ))
        .size(11.0)
        .color(style::TEXT_MUTED),
    );
    ui.add_space(8.0);

    show_kpis(ui, row, state.cutoff);
    ui.add_space(10.0);
    show_history(ui, state);
    ui.add_space(10.0);

    let values = value_cards(&decks, |i| data.prices.get(&i), state.field);
    let top = top_n_lines(&values, TOP_N);
    let order = ranked_by_unit(&values);
    if decks.len() == 1 {
        card_table(ui, "precon_cards_0", &values, &order, &top, state.cutoff);
    } else {
        for (deck_idx, deck) in decks.iter().enumerate() {
            let deck_order: Vec<usize> = order
                .iter()
                .copied()
                .filter(|&i| values[i].deck_idx == deck_idx)
                .collect();
            let deck_values: Vec<CardValue> =
                deck_order.iter().map(|&i| values[i].clone()).collect();
            let deck_ev = summarize(&deck_values, state.cutoff).ev;
            egui::CollapsingHeader::new(format!("{} — EV €{deck_ev:.2}", deck.name))
                .id_salt(("precon_deck", deck_idx))
                .default_open(true)
                .show(ui, |ui| {
                    card_table(
                        ui,
                        &format!("precon_cards_{deck_idx}"),
                        &values,
                        &deck_order,
                        &top,
                        state.cutoff,
                    );
                });
        }
    }
}

fn kpi(ui: &mut egui::Ui, label: &str, value: String, color: egui::Color32) {
    kpi_with(ui, label, |ui| {
        ui.label(egui::RichText::new(value).size(16.0).strong().color(color));
    });
}

fn kpi_with(ui: &mut egui::Ui, label: &str, value: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(11.0)
                .color(style::TEXT_MUTED),
        );
        value(ui);
    });
    ui.add_space(18.0);
}

fn show_kpis(ui: &mut egui::Ui, row: &PreconRow, cutoff: f64) {
    let Some(ev) = &row.ev else { return };
    style::section_frame().show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            kpi_with(ui, "Sealed", |ui| {
                let text = egui::RichText::new(euro(row.sealed_price))
                    .size(16.0)
                    .strong()
                    .color(SEALED_COLOR);
                super::cardmarket_link(ui, text, row.id_product);
            });
            kpi(
                ui,
                &format!("EV (≥ €{cutoff:.2})"),
                euro(Some(ev.ev)),
                style::ACCENT,
            );
            let (ratio, color) = match row.ratio {
                Some(r) if r >= 1.0 => (format!("{r:.2}×"), style::COLOR_SUCCESS),
                Some(r) if r < 0.8 => (format!("{r:.2}×"), style::COLOR_ERROR),
                Some(r) => (format!("{r:.2}×"), style::TEXT_PRIMARY),
                None => ("—".to_string(), style::TEXT_MUTED),
            };
            kpi(ui, "EV/Sealed", ratio, color);
            kpi(ui, "Top 5", euro(Some(ev.top5)), TOP5_COLOR);
            kpi(
                ui,
                "Gross (no cutoff)",
                euro(Some(ev.gross)),
                style::TEXT_PRIMARY,
            );
            kpi(
                ui,
                "Priced copies",
                format!(
                    "{}/{} ({:.0}%)",
                    ev.priced_copies,
                    ev.total_copies,
                    ev.coverage() * 100.0
                ),
                style::TEXT_PRIMARY,
            );
            kpi(
                ui,
                "Counted in EV",
                format!("{} copies", ev.counted_copies),
                style::TEXT_PRIMARY,
            );
        });
    });
}

fn show_history(ui: &mut egui::Ui, state: &PreconEvState) {
    ui.label(egui::RichText::new("EV history (weekly)").strong());
    if state.history_rx.is_some() {
        ui.horizontal(|ui| {
            ui.spinner();
            style::status_loading(ui, "Loading price history…");
        });
        return;
    }
    if let Some(err) = &state.history_error {
        style::status_error(ui, err);
        return;
    }
    let points = &state.history_points;
    let dates: Vec<String> = points.iter().map(|p| p.date.clone()).collect();
    let ev: Vec<Option<f64>> = points.iter().map(|p| p.ev).collect();
    let sealed: Vec<Option<f64>> = points.iter().map(|p| p.sealed).collect();
    let top5: Vec<Option<f64>> = points.iter().map(|p| p.top5).collect();
    line_chart(
        ui,
        &dates,
        &[
            Series {
                label: "EV",
                color: style::ACCENT,
                values: &ev,
            },
            Series {
                label: "Sealed",
                color: SEALED_COLOR,
                values: &sealed,
            },
            Series {
                label: "Top 5",
                color: TOP5_COLOR,
                values: &top5,
            },
        ],
        180.0,
    );
    ui.label(
        egui::RichText::new(
            "Weeks where under 90% of copies had a price (e.g. before release) are left blank.",
        )
        .size(10.0)
        .color(style::TEXT_MUTED),
    );
}

fn card_table(
    ui: &mut egui::Ui,
    id_salt: &str,
    values: &[CardValue],
    order: &[usize],
    top: &HashSet<usize>,
    cutoff: f64,
) {
    egui::Grid::new(id_salt)
        .num_columns(8)
        .striped(true)
        .spacing([12.0, 2.0])
        .show(ui, |ui| {
            for title in ["#", "Card", "Set #", "Board", "Qty", "Unit", "Line", "Note"] {
                ui.label(egui::RichText::new(title).strong());
            }
            ui.end_row();

            for (rank, &i) in order.iter().enumerate() {
                let v = &values[i];
                let counted = v.unit.is_some_and(|u| u.eur >= cutoff);
                let color = if counted {
                    style::TEXT_PRIMARY
                } else {
                    style::TEXT_MUTED
                };
                let cell = |ui: &mut egui::Ui, text: String| {
                    ui.label(egui::RichText::new(text).color(color));
                };
                let marker = if top.contains(&i) { " ★" } else { "" };
                cell(ui, format!("{}{marker}", rank + 1));
                let foil = if v.card.is_foil { " ✦" } else { "" };
                let name = format!("{}{foil}", v.card.name);
                match v.card.mcm_id {
                    Some(id) => {
                        let text = egui::RichText::new(name);
                        let text = if counted { text } else { text.color(color) };
                        super::cardmarket_link(ui, text, id);
                    }
                    None => cell(ui, name),
                }
                cell(ui, format!("{} #{}", v.card.set_code, v.card.number));
                cell(ui, board_label(v.card.board).to_string());
                cell(ui, format!("×{}", v.card.count));
                cell(ui, euro(v.unit.map(|u| u.eur)));
                cell(ui, euro(v.line_total()));
                let note = match (v.unit, v.card.mcm_id) {
                    (None, None) => "no Cardmarket id in MTGJSON",
                    (None, Some(_)) => "no tracker price",
                    (Some(u), _) if u.foil_fallback => "no foil price — non-foil used",
                    (Some(_), _) if !counted => "below cutoff",
                    _ => "",
                };
                ui.label(
                    egui::RichText::new(note)
                        .size(11.0)
                        .color(style::TEXT_MUTED),
                );
                ui.end_row();
            }
        });
}
