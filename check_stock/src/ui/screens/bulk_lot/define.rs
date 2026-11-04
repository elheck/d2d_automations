//! Step 1 — choose the cards: rules on the left, live preview on the right.

use super::widgets::{self, Tone};
use super::{BulkLotScreen, BulkLotState};
use crate::bulk_lot::{CardType, FoilFilter, LotOrder, Preset, Rarity, CONDITIONS, MAX_LOT_SIZE};
use crate::card_types::LookupProgress;
use crate::inventory_db::{per_card_share, MAX_LOT_PRICE_EUR};
use crate::ui::components::FilePicker;
use crate::ui::state::AppState;
use crate::ui::style;
use eframe::egui;
use egui_extras::{Column, TableBuilder};

pub(super) fn show(ctx: &egui::Context, app_state: &mut AppState, state: &mut BulkLotState) {
    if !state.has_magic_source() {
        show_start(ctx, app_state, state);
        return;
    }
    show_action_bar(ctx, app_state, state);
    egui::SidePanel::left("bulk_lot_rules")
        .resizable(false)
        .exact_width(360.0)
        .frame(
            egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(16, 4)),
        )
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("bulk_lot_rules_scroll")
                .show(ui, |ui| {
                    show_rules(ui, app_state, state);
                    ui.add_space(16.0);
                });
        });
    egui::CentralPanel::default().show(ctx, |ui| show_preview(ui, app_state, state));
}

// ── Empty state ─────────────────────────────────────────────────────────────

fn show_start(ctx: &egui::Context, app_state: &mut AppState, state: &mut BulkLotState) {
    egui::CentralPanel::default().show(ctx, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.set_max_width(620.0);
                style::section_frame()
                    .inner_margin(egui::Margin::same(24))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            egui::RichText::new("Start with your Magic inventory export")
                                .size(18.0)
                                .strong()
                                .color(style::TEXT_PRIMARY),
                        );
                        ui.add_space(4.0);
                        widgets::hint(
                            ui,
                            "Load the inventory report you downloaded from Cardmarket \
                             (inventory-report-Magic.csv). Like every screen, loading it also \
                             brings the local inventory database up to date.",
                        );
                        ui.add_space(12.0);
                        let browsed = FilePicker::new("Inventory CSV:", &mut state.csv_path)
                            .with_filter("CSV", &["csv"])
                            .show(ui);
                        ui.add_space(8.0);
                        let ready = !state.csv_path.trim().is_empty();
                        if (style::primary_button_enabled(ui, "Load export", ready).clicked()
                            || browsed)
                            && ready
                        {
                            BulkLotScreen::load_csv(app_state, state);
                        }

                        if let Some(source) = &state.source {
                            if !source.is_magic() {
                                ui.add_space(10.0);
                                widgets::callout(
                                    ui,
                                    Tone::Error,
                                    &format!("“{}” is the {} export", source.file_name, source.category),
                                    Some(
                                        "Bulk lots are built from singles. Load the Magic \
                                         inventory report instead.",
                                    ),
                                    |_| {},
                                );
                            }
                        }
                        if let Some(err) = &state.load_error {
                            ui.add_space(10.0);
                            widgets::callout(ui, Tone::Error, "Could not load the export", Some(err), |_| {});
                        }

                        ui.add_space(18.0);
                        ui.separator();
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("How it works").strong().color(style::TEXT_PRIMARY));
                        ui.add_space(6.0);
                        for (n, title, text) in [
                            (
                                "1",
                                "Choose cards",
                                "Set the rules — say 100 commons, no repeats, at most €0.25 each. \
                                 The stock that has sat longest and is cheapest comes first.",
                            ),
                            (
                                "2",
                                "Pick",
                                "Walk the shelves with a picture list sorted by location. Swap any \
                                 card you can't find.",
                            ),
                            (
                                "3",
                                "Finish",
                                "Save the TCG Powertools CSV that removes the cards. The lot price is \
                                 booked as revenue, split per card — the cards never count as \
                                 single sales.",
                            ),
                        ] {
                            ui.horizontal_top(|ui| {
                                let (rect, _) =
                                    ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                                ui.painter().circle_filled(rect.center(), 11.0, style::ACCENT);
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    n,
                                    egui::FontId::proportional(12.0),
                                    egui::Color32::WHITE,
                                );
                                ui.vertical(|ui| {
                                    ui.label(egui::RichText::new(title).strong());
                                    widgets::hint(ui, text);
                                });
                            });
                            ui.add_space(6.0);
                        }
                    });
            });
        });
    });
}

// ── Rules ───────────────────────────────────────────────────────────────────

fn show_rules(ui: &mut egui::Ui, app_state: &mut AppState, state: &mut BulkLotState) {
    widgets::group_title(ui, "Source");
    if let Some(source) = &state.source {
        let in_stock: i64 = state.candidates.iter().map(|c| c.available).sum();
        ui.label(egui::RichText::new(&source.file_name).strong());
        widgets::hint(
            ui,
            &if state.funnel.reserved > 0 {
                format!(
                    "{} in stock · {} reserved for lots awaiting an import",
                    widgets::cards(in_stock),
                    widgets::count(state.funnel.reserved)
                )
            } else {
                format!("{} in stock", widgets::cards(in_stock))
            },
        );
    }
    ui.add_space(4.0);
    let browsed = FilePicker::new("", &mut state.csv_path)
        .with_filter("CSV", &["csv"])
        .show(ui);
    if (style::secondary_button(ui, "Reload export").clicked() || browsed)
        && !state.csv_path.trim().is_empty()
    {
        BulkLotScreen::load_csv(app_state, state);
    }

    widgets::group_title(ui, "Start from");
    ui.horizontal_wrapped(|ui| {
        for preset in Preset::ALL {
            if style::secondary_button(ui, preset.label())
                .on_hover_text("Sets size, rarity, type and price cap; keeps the other rules")
                .clicked()
            {
                preset.apply(&mut state.spec);
            }
        }
    });

    let spec = &mut state.spec;

    widgets::group_title(ui, "Lot");
    ui.horizontal(|ui| {
        widgets::field_label(ui, "Name");
        let edit = ui.add(
            egui::TextEdit::singleline(&mut state.name)
                .hint_text("e.g. 100 Commons")
                .desired_width(170.0),
        );
        if edit.changed() {
            state.name_touched = true;
        }
        if state.name_touched
            && ui
                .small_button("↺")
                .on_hover_text("Use the suggested name")
                .clicked()
        {
            state.name_touched = false;
            state.name = crate::bulk_lot::default_lot_name(spec);
        }
    });
    ui.horizontal(|ui| {
        widgets::field_label(ui, "Size");
        ui.add(
            egui::DragValue::new(&mut spec.size)
                .range(1..=MAX_LOT_SIZE)
                .speed(1.0)
                .suffix(" cards"),
        );
    });
    ui.horizontal(|ui| {
        widgets::field_label(ui, "Lot price");
        ui.add(
            egui::DragValue::new(&mut state.price)
                .range(0.0..=MAX_LOT_PRICE_EUR)
                .speed(0.05)
                .fixed_decimals(2)
                .prefix("€"),
        )
        .on_hover_text("What the whole lot sells for on Cardmarket");
    });
    widgets::hint(
        ui,
        &format!(
            "= {} per card, booked as revenue the day you finish the lot.",
            widgets::share(per_card_share(state.price, spec.size as i64))
        ),
    );

    widgets::group_title(ui, "Cards");
    ui.horizontal(|ui| {
        widgets::field_label(ui, "Rarity");
        ui.horizontal_wrapped(|ui| {
            for rarity in Rarity::ALL {
                let mut on = spec.rarities.contains(&rarity);
                let chip = widgets::chip(ui, &mut on, rarity.label());
                let chip = if rarity == Rarity::Other {
                    chip.on_hover_text("Tokens, basic lands, specials and other rarities")
                } else {
                    chip
                };
                if chip.clicked() {
                    if on {
                        spec.rarities.insert(rarity);
                    } else {
                        spec.rarities.remove(&rarity);
                    }
                }
            }
        });
    });
    ui.horizontal(|ui| {
        widgets::field_label(ui, "Card type");
        egui::ComboBox::from_id_salt("bulk_lot_type")
            .selected_text(spec.card_type.label())
            .width(180.0)
            .show_ui(ui, |ui| {
                for t in CardType::ALL {
                    ui.selectable_value(&mut spec.card_type, t, t.label());
                }
            });
    });
    if spec.card_type.needs_type_line() {
        widgets::hint(
            ui,
            "Card types come from Scryfall — looked up once per printing, then cached.",
        );
    }
    ui.horizontal(|ui| {
        widgets::field_label(ui, "");
        ui.checkbox(&mut spec.exclude_basic_lands, "Leave out basic lands");
    });
    ui.horizontal(|ui| {
        widgets::field_label(ui, "Finish");
        egui::ComboBox::from_id_salt("bulk_lot_finish")
            .selected_text(spec.foil.label())
            .width(180.0)
            .show_ui(ui, |ui| {
                for f in FoilFilter::ALL {
                    ui.selectable_value(&mut spec.foil, f, f.label());
                }
            });
    });
    ui.horizontal(|ui| {
        widgets::field_label(ui, "Condition");
        ui.horizontal_wrapped(|ui| {
            for condition in CONDITIONS {
                let mut on = !spec.excluded_conditions.contains(condition);
                if widgets::chip(ui, &mut on, condition).clicked() {
                    if on {
                        spec.excluded_conditions.remove(condition);
                    } else {
                        spec.excluded_conditions.insert(condition.to_string());
                    }
                }
            }
        });
    });
    ui.horizontal_top(|ui| {
        widgets::field_label(ui, "Language");
        ui.horizontal_wrapped(|ui| {
            for (language, copies) in &state.languages {
                let mut on = !spec.excluded_languages.contains(language);
                let chip = widgets::chip(ui, &mut on, language)
                    .on_hover_text(format!("{} in stock", widgets::cards(*copies)));
                if chip.clicked() {
                    if on {
                        spec.excluded_languages.remove(language);
                    } else {
                        spec.excluded_languages.insert(language.clone());
                    }
                }
            }
        });
    });

    widgets::group_title(ui, "Value");
    ui.horizontal(|ui| {
        widgets::field_label(ui, "Max per card");
        ui.add(
            egui::DragValue::new(&mut spec.max_price)
                .range(0.0..=1000.0)
                .speed(0.01)
                .fixed_decimals(2)
                .prefix("€"),
        )
        .on_hover_text("Listed price cap for the filler cards");
    });
    ui.horizontal(|ui| {
        widgets::field_label(ui, "Guaranteed hits");
        let size = spec.size;
        ui.add(
            egui::DragValue::new(&mut spec.hits)
                .range(0..=size)
                .speed(0.2)
                .suffix(" cards"),
        );
        ui.label("worth ≥");
        ui.add(
            egui::DragValue::new(&mut spec.hit_min_price)
                .range(0.0..=1000.0)
                .speed(0.05)
                .fixed_decimals(2)
                .prefix("€"),
        );
    });
    widgets::hint(
        ui,
        "A selling point such as “5 cards worth €0.50+”. The cheapest qualifying cards are \
         used, so the promise costs as little as possible. 0 = none.",
    );

    widgets::group_title(ui, "Selection");
    ui.horizontal(|ui| {
        widgets::field_label(ui, "Order");
        ui.vertical(|ui| {
            for (order, hover) in [
                (
                    LotOrder::Nearby,
                    "Takes the cards from as few neighbouring bins as possible — the quickest \
                     lot to pick. The other rules (age, price, …) still decide which cards \
                     qualify.",
                ),
                (
                    LotOrder::OldestFirst,
                    "Cards in stock the longest first, then the cheapest — clears dead stock",
                ),
                (
                    LotOrder::CheapestFirst,
                    "Cheapest cards first, then the oldest",
                ),
                (LotOrder::Random, "A random mix of all matching cards"),
            ] {
                ui.radio_value(&mut spec.order, order, order.label())
                    .on_hover_text(hover);
            }
            if ui
                .small_button("🔀 Shuffle again")
                .on_hover_text(
                    "Draw a different mix among equally good cards (and a new random order)",
                )
                .clicked()
            {
                spec.seed = spec.seed.wrapping_add(1);
            }
        });
    });
    ui.horizontal(|ui| {
        widgets::field_label(ui, "In stock at least");
        ui.add(
            egui::DragValue::new(&mut spec.min_age_days)
                .range(0..=3650)
                .speed(1.0)
                .suffix(" days"),
        );
    });
    widgets::hint(
        ui,
        "Counted from the first inventory import that contained the card (Cardmarket's \
         export has no listing date). 0 = no minimum.",
    );
    ui.horizontal(|ui| {
        widgets::field_label(ui, "Copies per name");
        ui.add(
            egui::DragValue::new(&mut spec.max_copies_per_name)
                .range(1..=50)
                .speed(0.1),
        );
    });
    widgets::hint(ui, "1 = no repeated card names, across all printings.");
}

// ── Preview ─────────────────────────────────────────────────────────────────

fn show_preview(ui: &mut egui::Ui, app_state: &AppState, state: &mut BulkLotState) {
    let summary = state.preview_summary();
    let size = state.spec.size as i64;

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
        widgets::stat_tile(
            ui,
            "Cards",
            &format!(
                "{} / {}",
                widgets::count(summary.copies),
                widgets::count(size)
            ),
            &format!(
                "{} different names",
                widgets::count(summary.distinct_names as i64)
            ),
            if summary.copies >= size {
                style::COLOR_SUCCESS
            } else {
                style::COLOR_WARNING
            },
        );
        widgets::stat_tile(
            ui,
            "Single value",
            &widgets::euro(summary.listed_value),
            "listed price of these cards",
            style::TEXT_PRIMARY,
        );
        widgets::stat_tile(
            ui,
            "Lot price",
            &widgets::euro(state.price),
            &format!(
                "{} per card",
                widgets::share(per_card_share(state.price, summary.copies))
            ),
            style::TEXT_PRIMARY,
        );
        let (age, age_sub) = match (summary.oldest_days, summary.newest_days) {
            (Some(old), Some(new)) if old != new => {
                (format!("{old} days"), format!("newest: {new} days"))
            }
            (Some(old), _) => (format!("{old} days"), "all the same age".to_string()),
            _ => ("—".to_string(), "no listing date known".to_string()),
        };
        widgets::stat_tile(ui, "In stock for", &age, &age_sub, style::TEXT_PRIMARY);
        if state.spec.hits > 0 {
            widgets::stat_tile(
                ui,
                "Hits",
                &widgets::count(summary.hits),
                &format!("worth ≥ {}", widgets::euro(state.spec.hit_min_price)),
                if summary.hits >= state.spec.hits as i64 {
                    style::COLOR_SUCCESS
                } else {
                    style::COLOR_WARNING
                },
            );
        }
    });
    ui.add_space(8.0);

    show_notices(ui, app_state, state, summary.copies, summary.hits);
    show_funnel(ui, state);
    ui.add_space(6.0);
    show_table(ui, state);
}

fn show_notices(
    ui: &mut egui::Ui,
    app_state: &AppState,
    state: &mut BulkLotState,
    copies: i64,
    hits: i64,
) {
    if app_state.sync_guard.is_some() {
        widgets::callout(
            ui,
            Tone::Warning,
            "An inventory import is waiting for your decision",
            Some("Apply or cancel it in the safety-check dialog before finishing a lot."),
            |_| {},
        );
        ui.add_space(6.0);
    }
    if let Some(err) = &state.load_error {
        widgets::callout(
            ui,
            Tone::Error,
            "Problem with the inventory",
            Some(err),
            |_| {},
        );
        ui.add_space(6.0);
    }

    if let Some(job) = &state.type_job {
        let (label, fraction) = match job.progress {
            Some(LookupProgress::Batches { done, total }) => (
                format!("Batch {done} of {total}"),
                done as f32 / total.max(1) as f32,
            ),
            Some(LookupProgress::Singles { done, total }) => (
                format!("Checking cards Scryfall lists under other set codes: {done} of {total}"),
                done as f32 / total.max(1) as f32,
            ),
            None => ("Starting…".to_string(), 0.0),
        };
        let mut cancel = false;
        widgets::callout(
            ui,
            Tone::Info,
            "Looking up card types on Scryfall…",
            Some(&format!(
                "Needed once for {} printings; the result is cached. The preview fills in as \
                 soon as it is done.",
                widgets::count(job.printings as i64)
            )),
            |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .text(label)
                            .desired_width(ui.available_width() - 90.0),
                    );
                    cancel = ui.small_button("Cancel").clicked();
                });
            },
        );
        if cancel {
            BulkLotScreen::cancel_type_lookup(state);
        }
        ui.add_space(6.0);
    } else if state.types_incomplete() && state.type_paused {
        let mut resume = false;
        let (tone, title, body) = match &state.type_error {
            Some(err) => (Tone::Error, "Card type lookup failed", err.clone()),
            None => (
                Tone::Warning,
                "Card type lookup paused",
                "Only cards whose type is already known are in the preview.".to_string(),
            ),
        };
        widgets::callout(ui, tone, title, Some(&body), |ui| {
            ui.add_space(4.0);
            resume = ui.small_button("Try again").clicked();
        });
        if resume {
            state.resume_types();
        }
        ui.add_space(6.0);
    } else if state.funnel.types_unknown > 0 && state.spec.card_type.needs_type_line() {
        widgets::hint(
            ui,
            &format!(
                "{} have no known card type (Scryfall doesn't list them under these codes) and \
                 are left out.",
                widgets::cards(state.funnel.types_unknown)
            ),
        );
        ui.add_space(6.0);
    }

    let size = state.spec.size as i64;
    if copies < size && state.type_job.is_none() {
        let body = match state.funnel.bottleneck(state.spec.size) {
            Some(step) if step.copies == 0 && step.label == "In stock" => {
                "Nothing is free to put in a lot.".to_string()
            }
            Some(step) => format!(
                "The tightest rule is “{}” — {} left after it. Loosen it, allow more copies \
                 per name, or make the lot smaller.",
                step.label,
                widgets::cards(step.copies)
            ),
            None => "Allow more copies per name or make the lot smaller.".to_string(),
        };
        widgets::callout(
            ui,
            Tone::Warning,
            &format!(
                "Only {} of {} match",
                widgets::cards(copies),
                widgets::count(size)
            ),
            Some(&body),
            |_| {},
        );
        ui.add_space(6.0);
    }
    let wanted_hits = state.spec.hits as i64;
    if wanted_hits > 0 && hits < wanted_hits && state.type_job.is_none() {
        widgets::callout(
            ui,
            Tone::Warning,
            &format!("Only {} hits available", widgets::count(hits)),
            Some(&format!(
                "{} worth at least {} pass the other rules. Lower the threshold or promise fewer.",
                widgets::cards(state.funnel.hit_pool),
                widgets::euro(state.spec.hit_min_price)
            )),
            |_| {},
        );
        ui.add_space(6.0);
    }
}

fn show_funnel(ui: &mut egui::Ui, state: &BulkLotState) {
    let steps = &state.funnel.steps;
    let Some(first) = steps.first() else { return };
    let base = first.copies.max(1) as f32;
    let bottleneck = state
        .funnel
        .bottleneck(state.spec.size)
        .map(|s| s.label.clone());
    egui::CollapsingHeader::new(
        egui::RichText::new("How the cards were chosen").color(style::TEXT_PRIMARY),
    )
    .id_salt("bulk_lot_funnel")
    .default_open(true)
    .show(ui, |ui| {
        widgets::hint(ui, "Cards left after each rule, in order:");
        ui.add_space(4.0);
        egui::Grid::new("bulk_lot_funnel_grid")
            .num_columns(3)
            .spacing([14.0, 4.0])
            .show(ui, |ui| {
                for step in steps {
                    let tight = bottleneck.as_deref() == Some(step.label.as_str());
                    let text = egui::RichText::new(&step.label);
                    ui.label(if tight {
                        text.color(style::COLOR_WARNING).strong()
                    } else {
                        text
                    });
                    ui.label(egui::RichText::new(widgets::count(step.copies)).monospace());
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(180.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(
                        rect,
                        egui::CornerRadius::same(3),
                        style::PANEL_BORDER,
                    );
                    let mut filled = rect;
                    filled.set_width(rect.width() * (step.copies as f32 / base).clamp(0.0, 1.0));
                    ui.painter().rect_filled(
                        filled,
                        egui::CornerRadius::same(3),
                        if tight {
                            style::COLOR_WARNING
                        } else {
                            style::ACCENT
                        },
                    );
                    ui.end_row();
                }
            });
    });
}

fn show_table(ui: &mut egui::Ui, state: &BulkLotState) {
    if state.preview.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("No cards match these rules")
                    .size(15.0)
                    .color(style::TEXT_MUTED),
            );
        });
        return;
    }
    let candidates = &state.candidates;
    let picks = &state.preview;
    TableBuilder::new(ui)
        .id_salt("bulk_lot_preview")
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(36.0))
        .column(Column::remainder().at_least(180.0).clip(true))
        .column(Column::initial(150.0).clip(true))
        .column(Column::initial(76.0))
        .column(Column::initial(48.0))
        .column(Column::initial(82.0))
        .column(Column::initial(64.0))
        .column(Column::initial(84.0))
        .column(Column::initial(120.0))
        .header(22.0, |mut header| {
            for title in [
                "#", "Card", "Set", "Rarity", "Cond.", "Language", "Price", "In stock", "Location",
            ] {
                header.col(|ui| {
                    ui.label(egui::RichText::new(title).strong());
                });
            }
        })
        .body(|body| {
            body.rows(24.0, picks.len(), |mut row| {
                let index = row.index();
                let pick = picks[index];
                let c = &candidates[pick.candidate];
                row.col(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{}", index + 1)).color(style::TEXT_MUTED),
                    );
                });
                row.col(|ui| {
                    ui.label(&c.name);
                    if pick.copies > 1 {
                        widgets::badge(ui, &format!("×{}", pick.copies), style::ACCENT);
                    }
                    if pick.hit {
                        widgets::badge(ui, "HIT", style::COLOR_WARNING);
                    }
                    if c.is_foil {
                        widgets::badge(ui, "FOIL", style::ACCENT);
                    }
                });
                row.col(|ui| {
                    ui.label(egui::RichText::new(&c.set_name).color(style::TEXT_MUTED));
                });
                row.col(|ui| {
                    ui.label(c.rarity.label());
                });
                row.col(|ui| {
                    ui.label(&c.condition);
                });
                row.col(|ui| {
                    ui.label(&c.language);
                });
                row.col(|ui| {
                    ui.label(widgets::euro(c.price));
                });
                row.col(|ui| {
                    ui.label(
                        c.age_days
                            .map(|d| format!("{d} days"))
                            .unwrap_or_else(|| "—".to_string()),
                    );
                });
                row.col(|ui| {
                    ui.label(
                        egui::RichText::new(if c.location.is_empty() {
                            "—"
                        } else {
                            &c.location
                        })
                        .monospace()
                        .size(12.0),
                    );
                });
            });
        });
}

// ── Action bar ──────────────────────────────────────────────────────────────

fn show_action_bar(ctx: &egui::Context, app_state: &AppState, state: &mut BulkLotState) {
    let copies = state.preview_copies();
    let blocker = if copies == 0 {
        Some("No cards match the rules yet")
    } else if state.type_job.is_some() {
        Some("Wait for the card type lookup to finish")
    } else if app_state.sync_guard.is_some() {
        Some("Resolve the pending inventory import first")
    } else {
        None
    };
    let summary = format!(
        "{} · lot price {} · {} per card",
        widgets::cards(copies),
        widgets::euro(state.price),
        widgets::share(per_card_share(state.price, copies))
    );
    let mut start = false;
    egui::TopBottomPanel::bottom("bulk_lot_define_actions")
        .frame(
            egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(16, 10)),
        )
        .show(ctx, |ui| {
            egui::Sides::new().show(
                ui,
                |ui| {
                    ui.label(egui::RichText::new(&summary).color(style::TEXT_MUTED));
                },
                |ui| {
                    let button =
                        style::primary_button_enabled(ui, "Start picking  →", blocker.is_none());
                    let button = match blocker {
                        Some(reason) => button.on_disabled_hover_text(reason),
                        None => {
                            button.on_hover_text("Open the picture list sorted by shelf location")
                        }
                    };
                    start = button.clicked();
                },
            );
        });
    if start {
        state.start_picking();
    }
}
