//! Minimal multi-series line chart drawn with the egui painter.
//!
//! Series share one x axis (one label per point) and one € y axis. `None`
//! values break the line into separate runs. Hovering shows a crosshair and
//! a tooltip with every series' value at that point.

use crate::ui::style;
use eframe::egui;

/// One line of the chart.
pub struct Series<'a> {
    pub label: &'a str,
    pub color: egui::Color32,
    /// One value per x label; `None` leaves a gap.
    pub values: &'a [Option<f64>],
}

/// Space reserved left of the plot for y labels and below it for x labels.
const Y_LABEL_W: f32 = 52.0;
const X_LABEL_H: f32 = 16.0;
const PAD: f32 = 8.0;

/// Y range covering every finite value with 5% headroom, floored at 0.
/// A flat series gets a ±0.5 band. `None` when there is nothing to plot.
pub fn y_bounds(series: &[Series]) -> Option<(f64, f64)> {
    let mut values = series
        .iter()
        .flat_map(|s| s.values.iter().flatten().copied())
        .filter(|v| v.is_finite())
        .peekable();
    values.peek()?;
    let (min, max) = values.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(v), hi.max(v))
    });
    let (min, max) = if (max - min).abs() < 1e-9 {
        (min - 0.5, max + 0.5)
    } else {
        (min, max)
    };
    let pad = (max - min) * 0.05;
    Some(((min - pad).max(0.0), max + pad))
}

/// Screen position of point `i` of `n` with value `v`.
pub fn to_screen(i: usize, n: usize, v: f64, bounds: (f64, f64), rect: egui::Rect) -> egui::Pos2 {
    let x = if n <= 1 {
        rect.center().x
    } else {
        rect.left() + rect.width() * i as f32 / (n - 1) as f32
    };
    let (lo, hi) = bounds;
    let t = if hi > lo {
        ((v - lo) / (hi - lo)) as f32
    } else {
        0.5
    };
    egui::pos2(x, rect.bottom() - rect.height() * t)
}

/// Index of the point nearest to screen x, or `None` outside the plot.
pub fn hover_index(x: f32, n: usize, rect: egui::Rect) -> Option<usize> {
    if n == 0 || x < rect.left() || x > rect.right() {
        return None;
    }
    if n == 1 || rect.width() <= 0.0 {
        return Some(0);
    }
    let i = ((x - rect.left()) / rect.width() * (n - 1) as f32).round() as usize;
    Some(i.min(n - 1))
}

fn euro(v: f64) -> String {
    format!("€{v:.2}")
}

/// Draws the chart (legend row + plot) and returns the plot's response.
pub fn line_chart(
    ui: &mut egui::Ui,
    x_labels: &[String],
    series: &[Series],
    height: f32,
) -> egui::Response {
    ui.horizontal(|ui| {
        for s in series {
            let (swatch, _) = ui.allocate_exact_size(egui::vec2(12.0, 3.0), egui::Sense::hover());
            ui.painter().rect_filled(swatch, 1.0, s.color);
            ui.label(
                egui::RichText::new(s.label)
                    .size(11.0)
                    .color(style::TEXT_MUTED),
            );
            ui.add_space(8.0);
        }
    });

    let desired = egui::vec2(ui.available_width(), height);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, style::PANEL_BG);

    let plot = egui::Rect::from_min_max(
        rect.min + egui::vec2(Y_LABEL_W, PAD),
        rect.max - egui::vec2(PAD, X_LABEL_H + PAD),
    );
    let font = egui::FontId::proportional(10.0);
    let n = x_labels.len();
    let Some(bounds) = y_bounds(series).filter(|_| n > 0) else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "No price history",
            font,
            style::TEXT_MUTED,
        );
        return response;
    };

    // Horizontal grid with € labels at bottom, middle and top.
    let grid = egui::Stroke::new(1.0_f32, style::PANEL_BORDER);
    for frac in [0.0, 0.5, 1.0] {
        let v = bounds.0 + (bounds.1 - bounds.0) * frac;
        let y = to_screen(0, n, v, bounds, plot).y;
        painter.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            grid,
        );
        painter.text(
            egui::pos2(plot.left() - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            euro(v),
            font.clone(),
            style::TEXT_MUTED,
        );
    }
    // First and last x labels.
    let x_label_y = plot.bottom() + 4.0;
    painter.text(
        egui::pos2(plot.left(), x_label_y),
        egui::Align2::LEFT_TOP,
        &x_labels[0],
        font.clone(),
        style::TEXT_MUTED,
    );
    if n > 1 {
        painter.text(
            egui::pos2(plot.right(), x_label_y),
            egui::Align2::RIGHT_TOP,
            &x_labels[n - 1],
            font.clone(),
            style::TEXT_MUTED,
        );
    }

    // Lines, broken at gaps; isolated points drawn as dots.
    for s in series {
        let stroke = egui::Stroke::new(1.8_f32, s.color);
        let mut run: Vec<egui::Pos2> = Vec::new();
        let flush = |run: &mut Vec<egui::Pos2>| {
            match run.len() {
                0 => {}
                1 => {
                    painter.circle_filled(run[0], 2.0, s.color);
                }
                _ => {
                    painter.add(egui::Shape::line(std::mem::take(run), stroke));
                }
            }
            run.clear();
        };
        for (i, v) in s.values.iter().enumerate().take(n) {
            match v.filter(|v| v.is_finite()) {
                Some(v) => run.push(to_screen(i, n, v, bounds, plot)),
                None => flush(&mut run),
            }
        }
        flush(&mut run);
    }

    // Crosshair + tooltip.
    let hovered = response
        .hover_pos()
        .and_then(|pos| hover_index(pos.x, n, plot));
    if let Some(i) = hovered {
        let x = to_screen(i, n, bounds.0, bounds, plot).x;
        painter.line_segment(
            [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            egui::Stroke::new(1.0_f32, style::TEXT_MUTED),
        );
        for s in series {
            if let Some(v) = s.values.get(i).copied().flatten() {
                painter.circle_filled(to_screen(i, n, v, bounds, plot), 3.0, s.color);
            }
        }
        return response.on_hover_ui_at_pointer(|ui| {
            ui.label(egui::RichText::new(&x_labels[i]).strong());
            for s in series {
                let value = s
                    .values
                    .get(i)
                    .copied()
                    .flatten()
                    .map_or_else(|| "—".to_string(), euro);
                ui.label(egui::RichText::new(format!("{}: {value}", s.label)).color(s.color));
            }
        });
    }
    response
}

#[cfg(test)]
#[path = "line_chart_tests.rs"]
mod tests;
