use crate::ui::{
    state::{
        ConditionFilter, FoilFilter, InventoryPriceSource, LanguageFilter, NodeGraph, NodeKind,
        PricingState, RarityFilter, SavedGraph,
    },
    style,
};
use eframe::egui;
use log::{error, info};

pub(super) fn show_save_load_toolbar(ui: &mut egui::Ui, state: &mut PricingState) {
    ui.horizontal(|ui| {
        let preview_label = if state.show_preview {
            "▼ Hide Preview"
        } else {
            "▶ Preview Output"
        };
        if style::secondary_button(ui, preview_label).clicked() {
            state.show_preview = !state.show_preview;
        }

        ui.add_space(16.0);

        if style::secondary_button(ui, "Save Graph").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Save node graph")
                .add_filter("JSON", &["json"])
                .set_file_name("node_graph.json")
                .save_file()
            {
                match serde_json::to_string_pretty(&state.graph.save()) {
                    Ok(json) => {
                        if let Err(e) = std::fs::write(&path, json) {
                            error!("Failed to save graph: {e}");
                            state.load_error = Some(format!("Save failed: {e}"));
                        } else {
                            info!("Saved graph to {}", path.display());
                        }
                    }
                    Err(e) => {
                        error!("Failed to serialize graph: {e}");
                        state.load_error = Some(format!("Serialize failed: {e}"));
                    }
                }
            }
        }

        if style::secondary_button(ui, "Load Graph").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Load node graph")
                .add_filter("JSON", &["json"])
                .pick_file()
            {
                match std::fs::read_to_string(&path) {
                    Ok(json) => match serde_json::from_str::<SavedGraph>(&json) {
                        Ok(saved) => {
                            state.graph = NodeGraph::load(saved);
                            state.load_error = None;
                            info!("Loaded graph from {}", path.display());
                        }
                        Err(e) => {
                            error!("Failed to parse graph file: {e}");
                            state.load_error = Some(format!("Load failed: {e}"));
                        }
                    },
                    Err(e) => {
                        error!("Failed to read graph file: {e}");
                        state.load_error = Some(format!("Load failed: {e}"));
                    }
                }
            }
        }
    });
}

pub(super) fn show_add_toolbar(ui: &mut egui::Ui, graph: &mut NodeGraph) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            egui::RichText::new("Filter:")
                .color(style::TEXT_MUTED)
                .size(12.0),
        );
        if style::secondary_button(ui, "▼ Condition")
            .on_hover_text("Filter cards by condition (NM, EX, GD, LP, PL)")
            .clicked()
        {
            graph.add_node(
                NodeKind::FilterCondition {
                    condition: ConditionFilter::Any,
                },
                free_pos(graph),
            );
        }
        if style::secondary_button(ui, "▼ Language")
            .on_hover_text("Filter cards by language (English, German, French, …)")
            .clicked()
        {
            graph.add_node(
                NodeKind::FilterLanguage {
                    language: LanguageFilter::Any,
                },
                free_pos(graph),
            );
        }
        if style::secondary_button(ui, "▼ Foil")
            .on_hover_text("Filter to foil-only or non-foil-only cards")
            .clicked()
        {
            graph.add_node(
                NodeKind::FilterFoil {
                    mode: FoilFilter::Any,
                },
                free_pos(graph),
            );
        }
        if style::secondary_button(ui, "▼ Price Range")
            .on_hover_text("Filter cards whose price falls within a min–max range (€)")
            .clicked()
        {
            graph.add_node(
                NodeKind::FilterPrice {
                    min: 0.0,
                    max: 999.0,
                },
                free_pos(graph),
            );
        }
        if style::secondary_button(ui, "▼ Rarity")
            .on_hover_text("Filter cards by rarity (Common, Uncommon, Rare, Mythic)")
            .clicked()
        {
            graph.add_node(
                NodeKind::FilterRarity {
                    rarity: RarityFilter::Any,
                },
                free_pos(graph),
            );
        }
        if style::secondary_button(ui, "▼ Name")
            .on_hover_text("Filter cards whose name contains a search term (case-insensitive)")
            .clicked()
        {
            graph.add_node(
                NodeKind::FilterName {
                    term: String::new(),
                },
                free_pos(graph),
            );
        }
        if style::secondary_button(ui, "▼ Set")
            .on_hover_text("Filter cards by set name or set code (case-insensitive)")
            .clicked()
        {
            graph.add_node(
                NodeKind::FilterSet {
                    term: String::new(),
                },
                free_pos(graph),
            );
        }
        if style::secondary_button(ui, "▼ Location")
            .on_hover_text("Filter cards by storage location (e.g. A1_S1_R1_C1)")
            .clicked()
        {
            graph.add_node(
                NodeKind::FilterLocation {
                    term: String::new(),
                },
                free_pos(graph),
            );
        }

        ui.add_space(16.0);
        ui.label(
            egui::RichText::new("Transform:")
                .color(style::TEXT_MUTED)
                .size(12.0),
        );
        if style::secondary_button(ui, "⌊ Price Floor")
            .on_hover_text(
                "Set minimum prices per rarity — cards below the floor are shown at the floor price",
            )
            .clicked()
        {
            graph.add_node(
                NodeKind::PriceFloor {
                    common: 0.0,
                    uncommon: 0.0,
                    rare: 0.0,
                    mythic: 0.0,
                },
                free_pos(graph),
            );
        }
        if style::secondary_button(ui, "⇅ Inventory Price")
            .on_hover_text(
                "Override card prices with market data from the inventory_sync server (trend, avg, low, …)",
            )
            .clicked()
        {
            graph.add_node(
                NodeKind::InventoryPrice {
                    source: InventoryPriceSource::Trend,
                },
                free_pos(graph),
            );
        }

        ui.add_space(16.0);
        ui.label(
            egui::RichText::new("Logic:")
                .color(style::TEXT_MUTED)
                .size(12.0),
        );
        if style::secondary_button(ui, "⊓ AND")
            .on_hover_text("Intersection: outputs only cards present in ALL connected inputs")
            .clicked()
        {
            graph.add_node(NodeKind::LogicalAnd, free_pos(graph));
        }
        if style::secondary_button(ui, "⊔ OR")
            .on_hover_text("Union: outputs cards present in ANY connected input")
            .clicked()
        {
            graph.add_node(NodeKind::LogicalOr, free_pos(graph));
        }
        if style::secondary_button(ui, "¬ NOT")
            .on_hover_text("Complement: outputs all cards NOT in the connected input")
            .clicked()
        {
            graph.add_node(NodeKind::LogicalNot, free_pos(graph));
        }

        ui.add_space(16.0);
        ui.label(
            egui::RichText::new(
                "Right-click to remove  |  Drag header to move  |  Drag output port to wire",
            )
            .color(style::TEXT_MUTED)
            .size(11.0),
        );
    });
}

pub(super) fn free_pos(graph: &NodeGraph) -> egui::Pos2 {
    let n = graph.nodes.len() as f32;
    egui::pos2(180.0 + (n * 24.0) % 100.0, 80.0 + n * 32.0)
}
