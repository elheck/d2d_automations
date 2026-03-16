use super::constants::{HEADER_H, PARAM_H, PORT_ROW_H};
use crate::ui::state::{
    ConditionFilter, FoilFilter, GraphNode, InventoryPriceSource, LanguageFilter, NodeKind,
    RarityFilter,
};
use eframe::egui;

/// Absolute screen-space rect for one parameter row inside a node.
pub(super) fn param_row_rect(
    rect: egui::Rect,
    port_rows: usize,
    row: usize,
    zoom: f32,
) -> egui::Rect {
    let y = rect.min.y
        + HEADER_H * zoom
        + port_rows as f32 * PORT_ROW_H * zoom
        + 4.0
        + row as f32 * PARAM_H * zoom;
    egui::Rect::from_min_size(
        egui::pos2(rect.min.x + 8.0, y),
        egui::vec2(rect.width() - 16.0, PARAM_H * zoom - 8.0),
    )
}

pub(super) fn show_node_params(
    ui: &mut egui::Ui,
    node: &mut GraphNode,
    rect: egui::Rect,
    zoom: f32,
) {
    let port_rows = node.kind.input_count().max(node.kind.output_count());

    match &mut node.kind {
        NodeKind::FilterCondition { condition } => {
            let r = param_row_rect(rect, port_rows, 0, zoom);
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r), |ui| {
                egui::ComboBox::from_id_salt(("fcond", node.id))
                    .selected_text(condition.as_str())
                    .width(r.width())
                    .show_ui(ui, |ui| {
                        for &c in ConditionFilter::all() {
                            ui.selectable_value(condition, c, c.as_str());
                        }
                    });
            });
        }
        NodeKind::FilterLanguage { language } => {
            let r = param_row_rect(rect, port_rows, 0, zoom);
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r), |ui| {
                egui::ComboBox::from_id_salt(("flang", node.id))
                    .selected_text(language.as_str())
                    .width(r.width())
                    .show_ui(ui, |ui| {
                        for &l in LanguageFilter::all() {
                            ui.selectable_value(language, l, l.as_str());
                        }
                    });
            });
        }
        NodeKind::FilterFoil { mode } => {
            let r = param_row_rect(rect, port_rows, 0, zoom);
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r), |ui| {
                egui::ComboBox::from_id_salt(("ffoil", node.id))
                    .selected_text(mode.as_str())
                    .width(r.width())
                    .show_ui(ui, |ui| {
                        for &m in FoilFilter::all() {
                            ui.selectable_value(mode, m, m.as_str());
                        }
                    });
            });
        }
        NodeKind::FilterPrice { min, max } => {
            ui.put(
                param_row_rect(rect, port_rows, 0, zoom),
                egui::DragValue::new(min)
                    .prefix("≥ ")
                    .suffix(" €")
                    .speed(0.05)
                    .range(0.0..=99999.0),
            );
            ui.put(
                param_row_rect(rect, port_rows, 1, zoom),
                egui::DragValue::new(max)
                    .prefix("≤ ")
                    .suffix(" €")
                    .speed(0.5)
                    .range(0.0..=99999.0),
            );
        }
        NodeKind::FilterRarity { rarity } => {
            let r = param_row_rect(rect, port_rows, 0, zoom);
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r), |ui| {
                egui::ComboBox::from_id_salt(("frare", node.id))
                    .selected_text(rarity.as_str())
                    .width(r.width())
                    .show_ui(ui, |ui| {
                        for &rv in RarityFilter::all() {
                            ui.selectable_value(rarity, rv, rv.as_str());
                        }
                    });
            });
        }
        NodeKind::FilterName { term } => {
            ui.put(
                param_row_rect(rect, port_rows, 0, zoom),
                egui::TextEdit::singleline(term).hint_text("name contains…"),
            );
        }
        NodeKind::FilterSet { term } => {
            ui.put(
                param_row_rect(rect, port_rows, 0, zoom),
                egui::TextEdit::singleline(term).hint_text("set name or code…"),
            );
        }
        NodeKind::FilterLocation { term } => {
            ui.put(
                param_row_rect(rect, port_rows, 0, zoom),
                egui::TextEdit::singleline(term).hint_text("location contains…"),
            );
        }
        NodeKind::PriceFloor {
            common,
            uncommon,
            rare,
            mythic,
        } => {
            ui.put(
                param_row_rect(rect, port_rows, 0, zoom),
                egui::DragValue::new(common)
                    .prefix("C ≥ ")
                    .suffix(" €")
                    .speed(0.05)
                    .range(0.0..=99999.0),
            );
            ui.put(
                param_row_rect(rect, port_rows, 1, zoom),
                egui::DragValue::new(uncommon)
                    .prefix("U ≥ ")
                    .suffix(" €")
                    .speed(0.05)
                    .range(0.0..=99999.0),
            );
            ui.put(
                param_row_rect(rect, port_rows, 2, zoom),
                egui::DragValue::new(rare)
                    .prefix("R ≥ ")
                    .suffix(" €")
                    .speed(0.05)
                    .range(0.0..=99999.0),
            );
            ui.put(
                param_row_rect(rect, port_rows, 3, zoom),
                egui::DragValue::new(mythic)
                    .prefix("M ≥ ")
                    .suffix(" €")
                    .speed(0.05)
                    .range(0.0..=99999.0),
            );
        }
        NodeKind::InventoryPrice { source } => {
            let r = param_row_rect(rect, port_rows, 0, zoom);
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r), |ui| {
                egui::ComboBox::from_id_salt(("invprice", node.id))
                    .selected_text(source.as_str())
                    .width(r.width())
                    .show_ui(ui, |ui| {
                        for &s in InventoryPriceSource::all() {
                            ui.selectable_value(source, s, s.as_str());
                        }
                    });
            });
        }
        NodeKind::CsvSource
        | NodeKind::Output
        | NodeKind::LogicalAnd
        | NodeKind::LogicalOr
        | NodeKind::LogicalNot => {}
    }
}
