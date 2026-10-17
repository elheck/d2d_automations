//! Ranking table of all precons.

use crate::{
    precon_ev::{visible_rows, PreconRow, PreconSort},
    ui::{state::PreconEvState, style},
};
use eframe::egui;

/// What the user did in the table this frame.
#[derive(Default)]
pub(super) struct RankingAction {
    /// Product whose detail view should open.
    pub open: Option<u64>,
    /// The sort column or direction changed.
    pub resorted: bool,
}

fn euro(v: Option<f64>) -> String {
    v.map_or_else(|| "—".to_string(), |v| format!("€{v:.2}"))
}

/// EV/Sealed cell: green ≥ 1, red < 0.8; muted with ⚠ when the sealed
/// price is stale (the ratio is then overstated).
pub(super) fn ratio_text(row: &PreconRow) -> (String, egui::Color32) {
    let Some(r) = row.ratio else {
        return ("—".to_string(), style::TEXT_MUTED);
    };
    if row.stale_sealed().is_some() {
        return (format!("{r:.2}× ⚠"), style::TEXT_MUTED);
    }
    let color = if r >= 1.0 {
        style::COLOR_SUCCESS
    } else if r < 0.8 {
        style::COLOR_ERROR
    } else {
        style::TEXT_PRIMARY
    };
    (format!("{r:.2}×"), color)
}

fn ratio_label(ui: &mut egui::Ui, row: &PreconRow) {
    let (text, color) = ratio_text(row);
    let label = ui.label(egui::RichText::new(text).color(color).strong());
    if let (Some(low), Some(price)) = (row.stale_sealed(), row.sealed_price) {
        label.on_hover_text(format!(
            "Sealed €{price:.2} but the cheapest offer is €{low:.2} — ratio overstated"
        ));
    }
}

fn header(ui: &mut egui::Ui, label: &str, col: PreconSort, sort: &mut PreconSort, desc: &mut bool) {
    let active = *sort == col;
    let arrow = match (active, *desc) {
        (true, true) => " \u{25BC}",
        (true, false) => " \u{25B2}",
        _ => "",
    };
    let text = egui::RichText::new(format!("{label}{arrow}")).strong();
    if ui
        .add(egui::Label::new(text).sense(egui::Sense::click()))
        .clicked()
    {
        if active {
            *desc = !*desc;
        } else {
            *sort = col;
            // Names read best A→Z; numbers best high→low.
            *desc = col != PreconSort::Name;
        }
    }
}

pub(super) fn show_table(ui: &mut egui::Ui, state: &mut PreconEvState) -> RankingAction {
    let mut action = RankingAction::default();
    let before = (state.sort, state.sort_desc);
    let rows: Vec<&PreconRow> = visible_rows(&state.rows, &state.filter, state.hide_no_decklist);
    if rows.is_empty() {
        ui.label(
            egui::RichText::new("No precons match the filter.")
                .size(12.0)
                .color(style::TEXT_MUTED),
        );
        return action;
    }

    let sort = &mut state.sort;
    let desc = &mut state.sort_desc;
    egui::Grid::new("precon_ranking")
        .num_columns(10)
        .striped(true)
        .spacing([14.0, 3.0])
        .show(ui, |ui| {
            header(ui, "Product", PreconSort::Name, sort, desc);
            ui.label(egui::RichText::new("Set").strong());
            header(ui, "Released", PreconSort::Release, sort, desc);
            header(ui, "Sealed", PreconSort::Sealed, sort, desc);
            header(ui, "EV", PreconSort::Ev, sort, desc);
            header(ui, "EV/Sealed", PreconSort::Ratio, sort, desc);
            header(ui, "Liquid EV", PreconSort::Liquid, sort, desc);
            header(ui, "Top 5", PreconSort::Top5, sort, desc);
            header(ui, "Priced", PreconSort::Coverage, sort, desc);
            ui.label(egui::RichText::new("Notes").strong());
            ui.end_row();

            for row in rows {
                if row.is_ready() {
                    if ui
                        .link(&row.name)
                        .on_hover_text("Show card breakdown and EV history")
                        .clicked()
                    {
                        action.open = Some(row.id_product);
                    }
                } else {
                    ui.label(egui::RichText::new(&row.name).color(style::TEXT_MUTED));
                }
                ui.label(&row.set_label);
                ui.label(row.release_date.as_deref().unwrap_or("—"));
                super::cardmarket_link(ui, euro(row.sealed_price), row.id_product);
                ui.label(euro(row.ev.as_ref().map(|e| e.ev)));
                ratio_label(ui, row);
                ui.label(euro(row.ev.as_ref().and_then(|e| e.liquid)));
                ui.label(euro(row.ev.as_ref().map(|e| e.top5)));
                match &row.ev {
                    Some(ev) => {
                        let pct = ev.coverage() * 100.0;
                        let color = if pct < 95.0 {
                            style::COLOR_ERROR
                        } else {
                            style::TEXT_MUTED
                        };
                        ui.label(egui::RichText::new(format!("{pct:.0}%")).color(color));
                    }
                    None => {
                        ui.label("—");
                    }
                }
                let (note, hover) = row.notes();
                let note = ui.label(
                    egui::RichText::new(note)
                        .size(11.0)
                        .color(style::TEXT_MUTED),
                );
                if let Some(hover) = hover {
                    note.on_hover_text(hover);
                }
                ui.end_row();
            }
        });

    action.resorted = before != (state.sort, state.sort_desc);
    action
}
