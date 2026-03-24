use crate::formatters::format_price_diff_csv;
use crate::models::Card;
use crate::ui::{state::PricingState, style};
use eframe::egui;
use std::collections::HashMap;

// Proportional column weights — scaled to fill the window width at render time.
// Order: Name, Set, Cond., Lang., Foil, Price €, Rarity, Location
const PREVIEW_COL_WEIGHTS: [f32; 8] = [160.0, 110.0, 46.0, 68.0, 32.0, 95.0, 68.0, 95.0];
const PREVIEW_HEADERS: [&str; 8] = [
    "Name",
    "Set",
    "Cond.",
    "Lang.",
    "Foil",
    "Price €",
    "Rarity",
    "Location",
];
const PREVIEW_ROW_H: f32 = 18.0;

pub(super) fn condition_rank(cond: &str) -> u8 {
    match cond.to_uppercase().as_str() {
        "NM" => 0,
        "EX" => 1,
        "GD" => 2,
        "LP" => 3,
        "PL" => 4,
        _ => 5,
    }
}

pub(super) fn sort_preview(
    indices: &mut [usize],
    cards: &[Card],
    overrides: &HashMap<usize, f64>,
    col: usize,
    asc: bool,
) {
    indices.sort_by(|&a, &b| {
        let ca = &cards[a];
        let cb = &cards[b];
        let ord = match col {
            0 => ca.name.cmp(&cb.name),
            1 => ca.set.cmp(&cb.set),
            2 => condition_rank(&ca.condition).cmp(&condition_rank(&cb.condition)),
            3 => ca.language.cmp(&cb.language),
            4 => ca.is_foil_card().cmp(&cb.is_foil_card()),
            5 => {
                let pa = overrides.get(&a).copied().unwrap_or_else(|| ca.price_f64());
                let pb = overrides.get(&b).copied().unwrap_or_else(|| cb.price_f64());
                pa.partial_cmp(&pb).unwrap_or(std::cmp::Ordering::Equal)
            }
            6 => ca.rarity.cmp(&cb.rarity),
            _ => ca
                .location
                .as_deref()
                .unwrap_or("")
                .cmp(cb.location.as_deref().unwrap_or("")),
        };
        if asc {
            ord
        } else {
            ord.reverse()
        }
    });
}

pub(super) fn show_preview_window(ctx: &egui::Context, state: &mut PricingState) {
    // Clone indices so the window closure can also mutate state (sort on header click)
    let card_indices = state.cached_output.clone();
    let count = card_indices.len();
    let mut open = state.show_preview;

    egui::Window::new(format!("Output Preview — {count} cards"))
        .open(&mut open)
        .resizable(true)
        .default_size([700.0, 420.0])
        .show(ctx, |ui| {
            if state.cards.is_empty() {
                ui.label(
                    egui::RichText::new("Load a CSV file first.")
                        .color(egui::Color32::from_rgb(160, 100, 60)),
                );
                return;
            }
            if count == 0 {
                ui.label(
                    egui::RichText::new("No cards in output. Connect filters to the Output node.")
                        .color(egui::Color32::from_rgb(160, 100, 60)),
                );
                return;
            }

            // Scale column weights to fill the available width (subtract scrollbar ~12 px).
            let total_weight: f32 = PREVIEW_COL_WEIGHTS.iter().sum();
            let scale = (ui.available_width() - 12.0).max(100.0) / total_weight;
            let col_widths: [f32; 8] = PREVIEW_COL_WEIGHTS.map(|w| w * scale);

            // Clickable header row — click to sort, click again to reverse
            let header_color = egui::Color32::from_rgb(160, 185, 220);
            let active_color = egui::Color32::from_rgb(220, 210, 120);
            ui.horizontal(|ui| {
                for (col, (&w, &label)) in col_widths.iter().zip(PREVIEW_HEADERS.iter()).enumerate()
                {
                    let is_active = state.preview_sort_col == Some(col);
                    let indicator = if is_active {
                        if state.preview_sort_asc {
                            " ▲"
                        } else {
                            " ▼"
                        }
                    } else {
                        ""
                    };
                    let color = if is_active {
                        active_color
                    } else {
                        header_color
                    };
                    let text = format!("{label}{indicator}");
                    let resp = ui.add_sized(
                        [w, PREVIEW_ROW_H],
                        egui::Button::new(egui::RichText::new(text).strong().color(color))
                            .frame(false),
                    );
                    if resp.clicked() {
                        if is_active {
                            state.preview_sort_asc = !state.preview_sort_asc;
                        } else {
                            state.preview_sort_col = Some(col);
                            state.preview_sort_asc = true;
                        }
                        sort_preview(
                            &mut state.cached_output,
                            &state.cards,
                            &state.cached_price_overrides,
                            col,
                            state.preview_sort_asc,
                        );
                    }
                }
            });
            ui.separator();

            // ── Generate Diff CSV button (pinned to bottom) ────────────
            let changed_count = state.cached_price_overrides.len();
            let button_height = 36.0;
            let available = ui.available_height();
            let scroll_height = (available - button_height - 14.0).max(60.0);

            // Virtual-scrolling body — only visible rows are rendered
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(scroll_height)
                .show_rows(ui, PREVIEW_ROW_H, count, |ui, row_range| {
                    for (row_offset, &idx) in card_indices[row_range.clone()].iter().enumerate() {
                        let c = &state.cards[idx];
                        let stripe = (row_range.start + row_offset) % 2 == 0;
                        let row_rect = ui
                            .allocate_space(egui::vec2(ui.available_width(), PREVIEW_ROW_H))
                            .1;
                        if stripe {
                            ui.painter().rect_filled(
                                row_rect,
                                egui::CornerRadius::ZERO,
                                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 6),
                            );
                        }
                        // Place cells inside the allocated row rect
                        let mut x = row_rect.min.x;
                        let (price_str, price_color) =
                            if let Some(&floor) = state.cached_price_overrides.get(&idx) {
                                (
                                    format!("{} → {:.2}*", c.price, floor),
                                    egui::Color32::from_rgb(220, 200, 100),
                                )
                            } else {
                                (c.price.clone(), egui::Color32::from_rgb(160, 215, 140))
                            };
                        let cells: [(&str, egui::Color32); 8] = [
                            (c.name.as_str(), egui::Color32::WHITE),
                            (c.set.as_str(), egui::Color32::from_rgb(140, 155, 180)),
                            (c.condition.as_str(), egui::Color32::WHITE),
                            (c.language.as_str(), egui::Color32::WHITE),
                            (
                                if c.is_foil_card() { "✓" } else { "" },
                                egui::Color32::from_rgb(180, 215, 255),
                            ),
                            (price_str.as_str(), price_color),
                            (c.rarity.as_str(), egui::Color32::from_rgb(190, 165, 100)),
                            (
                                c.location.as_deref().unwrap_or("—"),
                                egui::Color32::from_rgb(140, 155, 180),
                            ),
                        ];
                        for (&w, (text, color)) in col_widths.iter().zip(cells) {
                            let cell_rect = egui::Rect::from_min_size(
                                egui::pos2(x + 2.0, row_rect.min.y),
                                egui::vec2(w - 4.0, PREVIEW_ROW_H),
                            );
                            ui.painter().with_clip_rect(cell_rect).text(
                                cell_rect.left_center(),
                                egui::Align2::LEFT_CENTER,
                                text,
                                egui::FontId::proportional(12.0),
                                color,
                            );
                            x += w;
                        }
                    }
                });

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(2.0);
            let label = format!("Generate Diff CSV ({changed_count} changed)");
            if style::primary_button_enabled(ui, &label, changed_count > 0).clicked() {
                state.diff_output_content = format_price_diff_csv(
                    &state.cards,
                    &state.cached_output,
                    &state.cached_price_overrides,
                );
                state.show_diff_output = true;
            }
        });

    state.show_preview = open;
}
