//! Right side: summary tiles, the review table, the detail pane for the
//! selected listing, and the export bar.

use super::rules::signed_euro;
use super::{export_file_name, muted, thousands, Note, PricingState, ReviewFilter, ReviewSort};
use crate::api::cardmarket::product_url;
use crate::api::inventory_sync::{PriceField, PriceFields};
use crate::repricing::{euro, explain, Outcome, Proposal, CARDMARKET_MIN_PRICE, MAX_PRICE_EUR};
use crate::ui::style;
use chrono::NaiveDate;
use eframe::egui;
use egui_extras::{Column, TableBuilder};

const RAISE_COLOR: egui::Color32 = egui::Color32::from_rgb(110, 200, 140);
const LOWER_COLOR: egui::Color32 = egui::Color32::from_rgb(235, 140, 110);

pub(super) fn show(ctx: &egui::Context, state: &mut PricingState, today: NaiveDate) {
    show_export_bar(ctx, state, today);
    if state.selected_row.is_some_and(|i| i < state.listings.len()) {
        show_detail(ctx, state, today);
    } else {
        state.selected_row = None;
    }
    egui::CentralPanel::default().show(ctx, |ui| {
        if state.listings.is_empty() {
            show_empty(ui, state);
            return;
        }
        show_summary(ui, state);
        ui.add_space(8.0);
        show_filters(ui, state);
        ui.add_space(4.0);
        show_table(ui, state);
    });
}

fn show_empty(ui: &mut egui::Ui, state: &PricingState) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        let title = if state.cards.is_empty() {
            "Load your inventory CSV to start"
        } else {
            "This export has no cards in stock"
        };
        ui.label(
            egui::RichText::new(title)
                .size(18.0)
                .strong()
                .color(style::TEXT_PRIMARY),
        );
        ui.add_space(8.0);
        for line in [
            "1  Load the Cardmarket inventory export.",
            "2  Fetch market prices from the tracker (or the price guide).",
            "3  Set up your rules on the left; every listing takes the first rule that matches.",
            "4  Check the proposed prices here, then export them.",
        ] {
            ui.label(egui::RichText::new(line).color(style::TEXT_MUTED));
        }
    });
}

// ── Summary ─────────────────────────────────────────────────────────────────

fn show_summary(ui: &mut egui::Ui, state: &mut PricingState) {
    let s = state.summary.clone();
    let mut jump: Option<ReviewFilter> = None;
    let diff = s.value_after - s.value_before;
    let pct = if s.value_before > 0.0 {
        diff / s.value_before * 100.0
    } else {
        0.0
    };
    let mut reasons: Vec<(&str, usize)> = s.skipped.iter().map(|(k, v)| (*k, *v)).collect();
    reasons.sort_by_key(|r| std::cmp::Reverse(r.1));
    let unchanged: usize = reasons.iter().map(|r| r.1).sum();
    let top_reason = reasons
        .first()
        .map(|(k, v)| format!("{} {}", thousands(*v as i64), k.to_lowercase()))
        .unwrap_or_default();
    let all_reasons = reasons
        .iter()
        .map(|(k, v)| format!("{k}: {}", thousands(*v as i64)))
        .collect::<Vec<_>>()
        .join("\n");

    ui.columns(5, |cols| {
        if tile(
            &mut cols[0],
            "Changes",
            &thousands(s.changes() as i64),
            &format!(
                "{} up · {} down",
                thousands(s.raises as i64),
                thousands(s.lowers as i64)
            ),
            style::TEXT_PRIMARY,
        )
        .on_hover_text("Show all changes")
        .clicked()
        {
            jump = Some(ReviewFilter::Changes);
        }
        tile(
            &mut cols[1],
            "Ticked for export",
            &thousands(s.selected as i64),
            &format!("{} copies", thousands(s.selected_copies)),
            style::ACCENT,
        );
        tile(
            &mut cols[2],
            "Stock value change",
            &format!("{} ({pct:+.1}%)", signed_euro(diff)),
            &format!("{} to {}", euro(s.value_before), euro(s.value_after)),
            if diff >= 0.0 {
                RAISE_COLOR
            } else {
                LOWER_COLOR
            },
        )
        .on_hover_text("Listed value of all stock, before and after the ticked changes");
        if tile(
            &mut cols[3],
            "Needs review",
            &thousands(s.review as i64),
            "big moves wait for a tick",
            if s.review > 0 {
                style::COLOR_WARNING
            } else {
                style::TEXT_MUTED
            },
        )
        .on_hover_text("Show changes waiting for a tick")
        .clicked()
        {
            jump = Some(ReviewFilter::NeedsReview);
        }
        if tile(
            &mut cols[4],
            "Unchanged",
            &thousands(unchanged as i64),
            &top_reason,
            style::TEXT_MUTED,
        )
        .on_hover_text(all_reasons)
        .clicked()
        {
            jump = Some(ReviewFilter::Unchanged);
        }
    });
    if let Some(f) = jump {
        state.filter = f;
        state.rule_filter = None;
    }
}

/// A clickable stat tile filling its column.
fn tile(
    ui: &mut egui::Ui,
    title: &str,
    value: &str,
    sub: &str,
    color: egui::Color32,
) -> egui::Response {
    let inner = egui::Frame::new()
        .fill(style::PANEL_BG)
        .stroke(egui::Stroke::new(1.0_f32, style::PANEL_BORDER))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new(title)
                    .size(11.0)
                    .color(style::TEXT_MUTED),
            );
            ui.label(egui::RichText::new(value).size(18.0).strong().color(color));
            ui.label(egui::RichText::new(sub).size(11.0).color(style::TEXT_MUTED));
        });
    ui.interact(
        inner.response.rect,
        ui.id().with(("pricing_tile", title)),
        egui::Sense::click(),
    )
}

// ── Filters ─────────────────────────────────────────────────────────────────

fn show_filters(ui: &mut egui::Ui, state: &mut PricingState) {
    ui.horizontal_wrapped(|ui| {
        for f in ReviewFilter::ALL {
            let count = match f {
                ReviewFilter::Changes => Some(state.summary.changes()),
                ReviewFilter::Raises => Some(state.summary.raises),
                ReviewFilter::Lowers => Some(state.summary.lowers),
                ReviewFilter::NeedsReview => Some(state.summary.review),
                _ => None,
            };
            let label = match count {
                Some(n) => format!("{} {}", f.label(), thousands(n as i64)),
                None => f.label().to_string(),
            };
            if ui.selectable_label(state.filter == f, label).clicked() {
                state.filter = f;
            }
        }
        ui.separator();
        let rule_text = match state.rule_filter {
            Some(r) => state
                .ruleset
                .rules
                .get(r)
                .map(|rule| format!("{}. {}", r + 1, rule.name))
                .unwrap_or_else(|| "All rules".to_string()),
            None => "All rules".to_string(),
        };
        egui::ComboBox::from_id_salt("pricing_rule_filter")
            .selected_text(rule_text)
            .width(170.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.rule_filter, None, "All rules");
                for (i, rule) in state.ruleset.rules.iter().enumerate() {
                    ui.selectable_value(
                        &mut state.rule_filter,
                        Some(i),
                        format!("{}. {}", i + 1, rule.name),
                    );
                }
            });
        ui.add(
            egui::TextEdit::singleline(&mut state.search)
                .hint_text("Search card or set…")
                .desired_width(160.0),
        );
    });
    ui.horizontal(|ui| {
        muted(
            ui,
            &format!("{} listings shown", thousands(state.rows.len() as i64)),
        );
        ui.add_space(8.0);
        if ui
            .small_button("Tick shown")
            .on_hover_text("Tick every change in the table")
            .clicked()
        {
            state.tick_shown(true);
        }
        if ui.small_button("Untick shown").clicked() {
            state.tick_shown(false);
        }
        if !state.ticks.is_empty()
            && ui
                .small_button("Reset ticks")
                .on_hover_text("Back to: every change ticked except those needing review")
                .clicked()
        {
            state.reset_ticks();
        }
    });
}

// ── Table ───────────────────────────────────────────────────────────────────

fn show_table(ui: &mut egui::Ui, state: &mut PricingState) {
    if state.rows.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            muted(ui, "Nothing to show with these filters.");
        });
        return;
    }
    let mut toggle: Option<(usize, bool)> = None;
    let mut select: Option<usize> = None;
    let mut sort: Option<ReviewSort> = None;
    let headers: [(&str, Option<ReviewSort>); 12] = [
        ("", None),
        ("Card", Some(ReviewSort::Name)),
        ("Set", Some(ReviewSort::Set)),
        ("Cond.", None),
        ("Lang.", None),
        ("Qty", Some(ReviewSort::Quantity)),
        ("Market", Some(ReviewSort::Market)),
        ("Now", Some(ReviewSort::Current)),
        ("New", Some(ReviewSort::New)),
        ("Change", Some(ReviewSort::Change)),
        ("Impact", Some(ReviewSort::Impact)),
        ("Rule / reason", Some(ReviewSort::Rule)),
    ];
    let reference = state.ruleset.guardrails.reference;
    egui::ScrollArea::horizontal()
        .id_salt("pricing_review_hscroll")
        .show(ui, |ui| {
            TableBuilder::new(ui)
                .id_salt("pricing_review")
                .striped(true)
                .resizable(true)
                .sense(egui::Sense::click())
                .auto_shrink([false, false])
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::exact(22.0))
                .column(Column::initial(200.0).at_least(110.0).clip(true))
                .column(Column::initial(48.0).clip(true))
                .column(Column::initial(40.0))
                .column(Column::initial(58.0).clip(true))
                .column(Column::initial(36.0))
                .column(Column::initial(60.0))
                .column(Column::initial(60.0))
                .column(Column::initial(80.0))
                .column(Column::initial(56.0))
                .column(Column::initial(64.0))
                .column(Column::remainder().at_least(110.0).clip(true))
                .header(22.0, |mut header| {
                    for (title, column) in headers {
                        header.col(|ui| {
                            let Some(column) = column else {
                                ui.label(egui::RichText::new(title).strong());
                                return;
                            };
                            let arrow = if state.sort == column {
                                if state.sort_desc {
                                    " ⏷"
                                } else {
                                    " ⏶"
                                }
                            } else {
                                ""
                            };
                            let text = egui::RichText::new(format!("{title}{arrow}")).strong();
                            let resp = ui.add(egui::Label::new(text).sense(egui::Sense::click()));
                            let resp = match column {
                                ReviewSort::Market => resp.on_hover_text(format!(
                                    "{} (foil column for foils)",
                                    reference.as_str()
                                )),
                                ReviewSort::Impact => resp.on_hover_text("Change × copies"),
                                _ => resp,
                            };
                            if resp.clicked() {
                                sort = Some(column);
                            }
                        });
                    }
                })
                .body(|body| {
                    body.rows(22.0, state.rows.len(), |mut row| {
                        let i = state.rows[row.index()];
                        let l = &state.listings[i];
                        let p = &state.eval.proposals[i];
                        let ticked = state.is_ticked(i);
                        row.set_selected(state.selected_row == Some(i));
                        row.col(|ui| {
                            if p.outcome.is_change() {
                                let mut on = ticked;
                                if ui.checkbox(&mut on, "").changed() {
                                    toggle = Some((i, on));
                                }
                            }
                        });
                        row.col(|ui| {
                            ui.label(&l.name);
                            if l.is_foil {
                                badge(ui, "FOIL", style::ACCENT);
                            }
                            if p.manual {
                                badge(ui, "HAND", style::COLOR_WARNING);
                            }
                        });
                        row.col(|ui| {
                            ui.label(egui::RichText::new(&l.set_code).color(style::TEXT_MUTED))
                                .on_hover_text(&l.set);
                        });
                        row.col(|ui| {
                            ui.label(&l.condition);
                        });
                        row.col(|ui| {
                            ui.label(&l.language);
                        });
                        row.col(|ui| {
                            ui.label(l.quantity.to_string());
                        });
                        row.col(|ui| {
                            ui.label(
                                egui::RichText::new(
                                    p.market.map(euro).unwrap_or_else(|| "—".into()),
                                )
                                .color(style::TEXT_MUTED),
                            );
                        });
                        row.col(|ui| {
                            ui.label(euro(p.old_price));
                        });
                        row.col(|ui| new_price_cell(ui, p, ticked));
                        row.col(|ui| {
                            if p.outcome.is_change() {
                                let color = if p.delta() > 0.0 {
                                    RAISE_COLOR
                                } else {
                                    LOWER_COLOR
                                };
                                ui.label(
                                    egui::RichText::new(format!("{:+.0}%", p.delta_pct()))
                                        .color(color),
                                );
                            }
                        });
                        row.col(|ui| {
                            if p.outcome.is_change() {
                                ui.label(
                                    egui::RichText::new(signed_euro(p.delta() * l.quantity as f64))
                                        .color(style::TEXT_MUTED),
                                )
                                .on_hover_text("Change × copies");
                            }
                        });
                        row.col(|ui| {
                            let rule_name = p
                                .rule
                                .and_then(|r| state.ruleset.rules.get(r))
                                .map(|r| r.name.as_str());
                            let text = if p.manual {
                                "Set by hand".to_string()
                            } else if p.outcome.is_change() {
                                rule_name.unwrap_or("").to_string()
                            } else {
                                match (p.outcome, rule_name) {
                                    (Outcome::NoRule, _) | (_, None) => {
                                        p.outcome.label().to_string()
                                    }
                                    (o, Some(name)) => format!("{} ({name})", o.label()),
                                }
                            };
                            let color = if p.outcome.is_change() {
                                style::TEXT_PRIMARY
                            } else {
                                style::TEXT_MUTED
                            };
                            ui.label(egui::RichText::new(text).color(color));
                        });
                        if row.response().clicked() {
                            select = Some(i);
                        }
                    });
                });
        });

    if let Some((i, on)) = toggle {
        state.set_tick(i, on);
    }
    if let Some(column) = sort {
        state.sort_by(column);
    }
    if let Some(i) = select {
        if state.selected_row == Some(i) {
            state.selected_row = None;
        } else {
            state.selected_row = Some(i);
            state.manual_input = state.eval.proposals[i].new_price;
        }
    }
}

fn new_price_cell(ui: &mut egui::Ui, p: &Proposal, ticked: bool) {
    match p.outcome {
        Outcome::Raise | Outcome::Lower => {
            let color = if p.outcome == Outcome::Raise {
                RAISE_COLOR
            } else {
                LOWER_COLOR
            };
            let mut text = egui::RichText::new(euro(p.new_price)).strong().color(color);
            if !ticked {
                text = text.strikethrough();
            }
            ui.label(text);
            if p.review {
                ui.label(egui::RichText::new("⚠").color(style::COLOR_WARNING))
                    .on_hover_text("Big change: export only when ticked");
            }
        }
        Outcome::SmallChange | Outcome::Held => {
            ui.label(
                egui::RichText::new(format!("({})", euro(p.new_price)))
                    .color(style::TEXT_MUTED)
                    .italics(),
            )
            .on_hover_text("The rule's price, not applied");
        }
        _ => {
            ui.label(egui::RichText::new("—").color(style::TEXT_MUTED));
        }
    }
}

fn badge(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    egui::Frame::new()
        .fill(color.linear_multiply(0.2))
        .stroke(egui::Stroke::new(1.0_f32, color.linear_multiply(0.8)))
        .inner_margin(egui::Margin::symmetric(4, 0))
        .corner_radius(egui::CornerRadius::same(3))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(9.5).strong().color(color));
        });
}

// ── Detail pane ─────────────────────────────────────────────────────────────

fn show_detail(ctx: &egui::Context, state: &mut PricingState, today: NaiveDate) {
    let Some(i) = state.selected_row else { return };
    egui::TopBottomPanel::bottom("pricing_detail")
        .resizable(true)
        .default_height(200.0)
        .min_height(120.0)
        .show(ctx, |ui| {
            let l = state.listings[i].clone();
            let p = state.eval.proposals[i].clone();
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&l.name)
                        .strong()
                        .size(15.0)
                        .color(style::TEXT_PRIMARY),
                );
                let mut facts = vec![
                    format!("{} ({})", l.set, l.set_code),
                    l.condition.clone(),
                    l.language.clone(),
                ];
                if l.is_foil {
                    facts.push("Foil".to_string());
                }
                facts.push(format!("×{}", l.quantity));
                if !l.location.is_empty() {
                    facts.push(l.location.clone());
                }
                muted(ui, &facts.join(" · "));
                if let Some(id) = l.product_id {
                    ui.hyperlink_to("Open on Cardmarket ↗", product_url(id));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✖").on_hover_text("Close").clicked() {
                        state.selected_row = None;
                    }
                });
            });
            ui.separator();
            ui.columns(3, |cols| {
                // How the price was found.
                cols[0].label(egui::RichText::new("How the price was found").strong());
                let steps = explain(
                    &state.ruleset,
                    &l,
                    &state.market,
                    state.manual.get(&i).copied(),
                    today,
                );
                egui::ScrollArea::vertical()
                    .id_salt("pricing_explain")
                    .max_height(140.0)
                    .show(&mut cols[0], |ui| {
                        for (n, step) in steps.iter().enumerate() {
                            ui.label(egui::RichText::new(format!("{}. {step}", n + 1)).size(12.0));
                        }
                    });

                // Market columns for this finish.
                let finish = if l.is_foil { "foil" } else { "non-foil" };
                cols[1].label(egui::RichText::new(format!("Market ({finish})")).strong());
                match l.product_id.and_then(|id| state.market.get(&id)) {
                    Some(row) => {
                        egui::Grid::new("pricing_detail_market")
                            .num_columns(4)
                            .spacing([10.0, 2.0])
                            .show(&mut cols[1], |ui| {
                                for (n, f) in PriceField::all().iter().enumerate() {
                                    ui.label(
                                        egui::RichText::new(f.as_str()).color(style::TEXT_MUTED),
                                    );
                                    ui.label(
                                        row.price_for(*f, l.is_foil)
                                            .map(euro)
                                            .unwrap_or_else(|| "—".to_string()),
                                    );
                                    if n % 2 == 1 {
                                        ui.end_row();
                                    }
                                }
                            });
                        muted(&mut cols[1], &format!("as of {}", row.price_date));
                    }
                    None => {
                        muted(&mut cols[1], "No market row for this product.");
                    }
                }

                // History + hand price.
                cols[2].label(egui::RichText::new("Your history").strong());
                let f = l.facts;
                let lines = [
                    f.age_days
                        .map(|d| format!("In stock {d} days"))
                        .unwrap_or_else(|| "Time in stock unknown".to_string()),
                    match (f.last_sale, f.days_since_sale) {
                        (Some(date), _) => format!("Last sold {date}"),
                        (None, Some(_)) => "Never sold since listing".to_string(),
                        _ => "No sales recorded".to_string(),
                    },
                    format!("{} sold in the last 30 days", f.sold_30d),
                ];
                for line in lines {
                    cols[2].label(egui::RichText::new(line).size(12.0));
                }
                cols[2].add_space(6.0);
                cols[2].horizontal(|ui| {
                    ui.label("Price by hand");
                    ui.add(
                        egui::DragValue::new(&mut state.manual_input)
                            .range(CARDMARKET_MIN_PRICE..=MAX_PRICE_EUR)
                            .speed(0.01)
                            .fixed_decimals(2)
                            .prefix("€"),
                    );
                    if ui
                        .button("Use")
                        .on_hover_text(
                            "This listing gets exactly this price, whatever the rules say",
                        )
                        .clicked()
                    {
                        let price = state.manual_input;
                        state.set_manual(i, price);
                    }
                    if p.manual && ui.button("Back to rules").clicked() {
                        state.clear_manual(i);
                    }
                });
            });
        });
}

// ── Export ──────────────────────────────────────────────────────────────────

fn show_export_bar(ctx: &egui::Context, state: &mut PricingState, today: NaiveDate) {
    egui::TopBottomPanel::bottom("pricing_export")
        .frame(
            egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(16, 8)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let n = state.summary.selected;
                let label = format!(
                    "Export {} price change{}…",
                    thousands(n as i64),
                    if n == 1 { "" } else { "s" }
                );
                let clicked = style::primary_button_enabled(ui, &label, n > 0)
                    .on_hover_text(
                        "Writes a CSV with the inventory export's columns and the new prices of \
                         the ticked listings, plus an _undo.csv with their current prices. \
                         Check the import with a few cards first.",
                    )
                    .clicked();
                if clicked {
                    export(state, today);
                }
                match &state.export_note {
                    Some(note) => {
                        let color = if note.error {
                            style::COLOR_ERROR
                        } else {
                            style::COLOR_SUCCESS
                        };
                        ui.label(egui::RichText::new(&note.text).color(color));
                    }
                    None if n > 0 => {
                        muted(
                            ui,
                            &format!(
                                "{} copies · an undo file with the current prices is written \
                                 next to it",
                                thousands(state.summary.selected_copies)
                            ),
                        );
                    }
                    None => {
                        muted(ui, "Tick changes in the table to export them.");
                    }
                }
            });
        });
}

fn export(state: &mut PricingState, today: NaiveDate) {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Export price changes")
        .add_filter("CSV", &["csv"])
        .set_file_name(export_file_name(today))
        .save_file()
    else {
        return;
    };
    state.export_note = Some(match state.export_to(&path) {
        Ok(report) => {
            log::info!(
                "Repricing: exported {} changes to {}",
                report.changes,
                report.csv.display()
            );
            Note::ok(format!(
                "Saved {} changes to {}. To undo, import {}.",
                thousands(report.changes as i64),
                report.csv.display(),
                report
                    .undo
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ))
        }
        Err(e) => Note::error(e),
    });
}
