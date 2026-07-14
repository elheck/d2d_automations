//! Restock Recommendations screen.
//!
//! Read-only: lists sold-out variants (quantity 0, sold copies > 0) ranked by
//! how quickly they sold through, so the fastest sellers worth re-buying are on
//! top. One-off sales are filtered out via a minimum-copies threshold. The
//! ticked view can be exported as a buy-list CSV; nothing here ever writes to
//! the inventory DB.

use crate::{
    inventory_db::get_restock_candidates,
    restock::{format_buy_list_csv, rank_candidates, RankedRestock},
    ui::{
        state::{RestockSort, RestockState, Screen},
        style,
    },
};
use eframe::egui;
use log::{error, info};

/// Max rows rendered in the table (ranked so the fastest sellers are on top).
const MAX_ROWS: usize = 300;

pub struct RestockScreen;

impl RestockScreen {
    pub fn show(ctx: &egui::Context, current_screen: &mut Screen, state: &mut RestockState) {
        // All data is local (inventory DB), so build the report on first entry.
        if !state.loaded {
            state.loaded = true;
            Self::rebuild(state);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("restock_scroll")
                .show(ui, |ui| {
                    if style::back_button(ui, "Back") {
                        *current_screen = Screen::Welcome;
                    }
                    ui.add_space(8.0);
                    style::screen_heading(ui, "Restock Recommendations");

                    Self::show_controls(ui, state);
                    ui.add_space(10.0);

                    if let Some(err) = &state.error {
                        style::status_error(ui, err);
                        ui.add_space(6.0);
                    }

                    if state.rows.is_some() {
                        Self::show_summary(ui, state);
                        ui.add_space(8.0);
                        Self::show_table(ui, state);
                    }
                });
        });
    }

    fn show_controls(ui: &mut egui::Ui, state: &mut RestockState) {
        style::section_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("Min copies sold:");
                ui.add(
                    egui::DragValue::new(&mut state.min_copies)
                        .speed(0.2)
                        .range(1..=1000),
                );

                ui.add_space(12.0);
                if style::primary_button(ui, "Refresh").clicked() {
                    Self::rebuild(state);
                }

                ui.add_space(12.0);
                let has_rows = state.rows.as_ref().is_some_and(|r| !r.is_empty());
                if style::secondary_button_enabled(ui, "Export buy list…", has_rows).clicked() {
                    Self::export_buy_list(state);
                }
            });

            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(
                    "Sold-out cards ranked by sell-through speed — what to buy again. \
                     Raise the minimum to hide one-off sales.",
                )
                .size(11.0)
                .color(style::TEXT_MUTED),
            );
        });
    }

    fn show_summary(ui: &mut egui::Ui, state: &RestockState) {
        let Some(rows) = &state.rows else { return };
        let copies: i64 = rows.iter().map(|r| r.candidate.sold_copies).sum();
        let revenue: f64 = rows.iter().map(|r| r.candidate.realized_revenue).sum();

        style::section_frame().show(ui, |ui| {
            ui.label(
                egui::RichText::new("Summary")
                    .strong()
                    .size(14.0)
                    .color(style::TEXT_PRIMARY),
            );
            ui.add_space(4.0);
            egui::Grid::new("restock_summary")
                .num_columns(6)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Candidates:").strong());
                    ui.label(format!("{} variants", rows.len()));
                    ui.label(egui::RichText::new("Copies sold:").strong());
                    ui.label(format!("×{copies}"));
                    ui.label(egui::RichText::new("Realized revenue:").strong());
                    ui.label(
                        egui::RichText::new(format!("€{revenue:.2}")).color(style::COLOR_SUCCESS),
                    );
                    ui.end_row();
                });
        });
    }

    fn show_table(ui: &mut egui::Ui, state: &mut RestockState) {
        let Some(rows) = &state.rows else { return };
        let mut sorted: Vec<&RankedRestock> = rows.iter().collect();
        sorted.sort_by(|a, b| {
            let ord = compare(a, b, state.sort);
            if state.sort_desc {
                ord.reverse()
            } else {
                ord
            }
        });

        let total = sorted.len();
        let shown = total.min(MAX_ROWS);
        ui.label(
            egui::RichText::new(if total > shown {
                format!("Showing top {shown} of {total} variants")
            } else {
                format!("{total} variants")
            })
            .size(11.0)
            .color(style::TEXT_MUTED),
        );
        ui.add_space(2.0);

        let header = |ui: &mut egui::Ui,
                      label: &str,
                      col: RestockSort,
                      sort: &mut RestockSort,
                      desc: &mut bool| {
            let active = *sort == col;
            let arrow = if active {
                if *desc {
                    " \u{25BC}"
                } else {
                    " \u{25B2}"
                }
            } else {
                ""
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
                    *desc = true;
                }
            }
        };

        egui::Grid::new("restock_table")
            .num_columns(8)
            .striped(true)
            .spacing([12.0, 2.0])
            .show(ui, |ui| {
                header(
                    ui,
                    "Card",
                    RestockSort::Name,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                ui.label(egui::RichText::new("Set").strong());
                header(
                    ui,
                    "Sold",
                    RestockSort::SoldCopies,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Copies/wk",
                    RestockSort::Velocity,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Days",
                    RestockSort::Days,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Last price",
                    RestockSort::Price,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                header(
                    ui,
                    "Revenue",
                    RestockSort::Revenue,
                    &mut state.sort,
                    &mut state.sort_desc,
                );
                ui.label(egui::RichText::new("Sold out").strong());
                ui.end_row();

                for r in sorted.into_iter().take(MAX_ROWS) {
                    let c = &r.candidate;
                    let name = if c.is_foil {
                        format!("{} ✦", c.name)
                    } else {
                        c.name.clone()
                    };
                    ui.label(name);
                    ui.label(format!("{} {}", c.set_code, c.condition));
                    ui.label(format!("×{}", c.sold_copies));
                    ui.label(
                        egui::RichText::new(format!("{:.2}", r.copies_per_week))
                            .color(style::COLOR_SUCCESS),
                    );
                    ui.label(format!("{}", r.days_to_sell_out));
                    ui.label(format!("€{:.2}", c.last_price));
                    ui.label(format!("€{:.2}", c.realized_revenue));
                    ui.label(&c.sold_out_date);
                    ui.end_row();
                }
            });
    }

    // ── Actions ─────────────────────────────────────────────────────────────

    /// Rebuilds the report from the inventory DB with the current threshold.
    fn rebuild(state: &mut RestockState) {
        match get_restock_candidates() {
            Ok(candidates) => {
                state.rows = Some(rank_candidates(candidates, state.min_copies));
                state.error = None;
            }
            Err(e) => {
                state.error = Some(format!("Failed to read inventory: {e}"));
            }
        }
    }

    /// Exports the current view (respecting the active sort) as a buy-list CSV.
    fn export_buy_list(state: &RestockState) {
        let Some(rows) = &state.rows else { return };
        let mut sorted: Vec<RankedRestock> = rows.clone();
        sorted.sort_by(|a, b| {
            let ord = compare(a, b, state.sort);
            if state.sort_desc {
                ord.reverse()
            } else {
                ord
            }
        });
        let csv = format_buy_list_csv(&sorted);

        let Some(path) = rfd::FileDialog::new()
            .set_file_name("restock_buy_list.csv")
            .add_filter("CSV", &["csv"])
            .save_file()
        else {
            info!("Buy-list export cancelled: no file chosen");
            return;
        };
        match std::fs::write(&path, csv) {
            Ok(()) => info!("Buy list exported to {}", path.display()),
            Err(e) => error!("Failed to save buy list: {e}"),
        }
    }
}

/// Column comparator for the restock table, ascending.
fn compare(a: &RankedRestock, b: &RankedRestock, sort: RestockSort) -> std::cmp::Ordering {
    let eq = std::cmp::Ordering::Equal;
    match sort {
        RestockSort::Velocity => a.copies_per_week.partial_cmp(&b.copies_per_week),
        RestockSort::Revenue => a
            .candidate
            .realized_revenue
            .partial_cmp(&b.candidate.realized_revenue),
        RestockSort::Price => a.candidate.last_price.partial_cmp(&b.candidate.last_price),
        RestockSort::SoldCopies => Some(a.candidate.sold_copies.cmp(&b.candidate.sold_copies)),
        RestockSort::Days => Some(a.days_to_sell_out.cmp(&b.days_to_sell_out)),
        RestockSort::Name => Some(a.candidate.name.cmp(&b.candidate.name)),
    }
    .unwrap_or(eq)
}
