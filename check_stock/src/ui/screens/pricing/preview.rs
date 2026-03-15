use crate::models::Card;
use crate::ui::state::PricingState;
use eframe::egui;

// Column widths (px) for the preview table — must match header and body.
const PREVIEW_COLS: [f32; 7] = [190.0, 120.0, 52.0, 85.0, 35.0, 62.0, 110.0];
const PREVIEW_HEADERS: [&str; 7] = [
    "Name",
    "Set",
    "Cond.",
    "Lang.",
    "Foil",
    "Price €",
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

pub(super) fn sort_preview(indices: &mut [usize], cards: &[Card], col: usize, asc: bool) {
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
                let pa = ca.price.parse::<f64>().unwrap_or(0.0);
                let pb = cb.price.parse::<f64>().unwrap_or(0.0);
                pa.partial_cmp(&pb).unwrap_or(std::cmp::Ordering::Equal)
            }
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

            // Clickable header row — click to sort, click again to reverse
            let header_color = egui::Color32::from_rgb(160, 185, 220);
            let active_color = egui::Color32::from_rgb(220, 210, 120);
            ui.horizontal(|ui| {
                for (col, (&w, &label)) in
                    PREVIEW_COLS.iter().zip(PREVIEW_HEADERS.iter()).enumerate()
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
                            col,
                            state.preview_sort_asc,
                        );
                    }
                }
            });
            ui.separator();

            // Virtual-scrolling body — only visible rows are rendered
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
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
                        let cells: [(&str, egui::Color32); 7] = [
                            (c.name.as_str(), egui::Color32::WHITE),
                            (c.set.as_str(), egui::Color32::from_rgb(140, 155, 180)),
                            (c.condition.as_str(), egui::Color32::WHITE),
                            (c.language.as_str(), egui::Color32::WHITE),
                            (
                                if c.is_foil_card() { "✓" } else { "" },
                                egui::Color32::from_rgb(180, 215, 255),
                            ),
                            (c.price.as_str(), egui::Color32::from_rgb(160, 215, 140)),
                            (
                                c.location.as_deref().unwrap_or("—"),
                                egui::Color32::from_rgb(140, 155, 180),
                            ),
                        ];
                        for (&w, (text, color)) in PREVIEW_COLS.iter().zip(cells) {
                            let cell_rect = egui::Rect::from_min_size(
                                egui::pos2(x + 2.0, row_rect.min.y),
                                egui::vec2(w - 4.0, PREVIEW_ROW_H),
                            );
                            ui.painter().text(
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
        });

    state.show_preview = open;
}
