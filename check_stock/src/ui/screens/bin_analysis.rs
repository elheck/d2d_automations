use crate::{
    bin_consolidation::{
        fragmented_variants, plan_consolidation, plan_variant_defrag, ConsolidationPlan,
        FragmentedVariant, Move,
    },
    io::read_csv,
    stock_analysis::{format_stock_analysis_with_sort, SortOrder, StockAnalysis},
    ui::{
        components::FilePicker,
        screens::ConsolidationState,
        state::{BinAnalysisState, Screen},
        style,
    },
};
use eframe::egui;
use log::info;

pub struct BinAnalysisScreen;

impl BinAnalysisScreen {
    pub fn show(
        ctx: &egui::Context,
        current_screen: &mut Screen,
        state: &mut BinAnalysisState,
        consolidation_state: &mut ConsolidationState,
    ) {
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("bin_analysis_scroll")
                .show(ui, |ui| {
                    if style::back_button(ui, "Back") {
                        *current_screen = Screen::Welcome;
                    }
                    ui.add_space(8.0);

                    style::screen_heading(ui, "Bin Capacity Analysis");

                    // ── File picker ─────────────────────────────────────────
                    // Read-only screen: it analyses whatever CSV is given (which
                    // may be a partial file such as a consolidation move export)
                    // and must NEVER sync to the inventory DB — doing so would
                    // zero every variant not present in a partial file.
                    style::section_frame().show(ui, |ui| {
                        FilePicker::new("Inventory CSV:", &mut state.inventory_path)
                            .with_filter("CSV", &["csv"])
                            .show(ui);
                    });

                    ui.add_space(10.0);

                    // ── Controls ────────────────────────────────────────────
                    style::section_frame().show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Minimum Free Slots:");
                            ui.add(egui::Slider::new(&mut state.free_slots, 1..=30).text("slots"));
                        });

                        ui.add_space(5.0);

                        ui.horizontal(|ui| {
                            ui.label("Sort by:");
                            egui::ComboBox::from_label("")
                                .selected_text(match state.sort_order {
                                    SortOrder::ByFreeSlots => "Free Slots (Descending)",
                                    SortOrder::ByLocation => "Location (Ascending)",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut state.sort_order,
                                        SortOrder::ByFreeSlots,
                                        "Free Slots (Descending)",
                                    );
                                    ui.selectable_value(
                                        &mut state.sort_order,
                                        SortOrder::ByLocation,
                                        "Location (Ascending)",
                                    );
                                });
                        });

                        ui.add_space(10.0);

                        if style::primary_button(ui, "Analyze Stock").clicked() {
                            if let Err(e) = Self::analyze_stock(state) {
                                state.output = format!("Error: {e}");
                            }
                        }
                    });

                    ui.add_space(8.0);
                    ui.separator();

                    if !state.output.is_empty() {
                        ui.add_space(6.0);
                        if style::secondary_button(ui, "Save Analysis to File").clicked() {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_file_name("bin_analysis.txt")
                                .add_filter("Text Files", &["txt"])
                                .save_file()
                            {
                                if let Err(e) = std::fs::write(&path, &state.output) {
                                    state.output = format!("Error saving file: {e}");
                                }
                            }
                        }
                        ui.add_space(4.0);
                        ui.add(
                            egui::TextEdit::multiline(&mut state.output)
                                .desired_width(f32::INFINITY)
                                .desired_rows(20)
                                .font(egui::TextStyle::Monospace),
                        );
                    }

                    ui.add_space(12.0);
                    Self::show_consolidation(ui, current_screen, state, consolidation_state);
                });
        });
    }

    /// Consolidation section: suggest emptying sparse bins, then open the moves
    /// in the interactive list (which exports only the piles you tick as moved).
    fn show_consolidation(
        ui: &mut egui::Ui,
        current_screen: &mut Screen,
        state: &mut BinAnalysisState,
        consolidation_state: &mut ConsolidationState,
    ) {
        style::screen_heading(ui, "Bin Consolidation");
        style::section_frame().show(ui, |ui| {
            ui.label(
                egui::RichText::new(
                    "Empties sparse bins into fuller ones (preferring bins that already hold the \
                     same card), so bins can be reused. Each card keeps its lot/side; only the bin \
                     coordinates change, so per-lot revenue is untouched. Open the moves in the \
                     interactive list and tick each pile as you move it — the CSV export there \
                     includes only the piles you actually moved. It never writes to the inventory \
                     database; re-load an updated inventory CSV to apply.",
                )
                .size(11.0)
                .color(style::TEXT_MUTED),
            );
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label("Merge bins filled up to:");
                ui.add(egui::Slider::new(&mut state.merge_threshold, 1..=60).text("cards"));
            });

            ui.add_space(8.0);
            if style::primary_button(ui, "Suggest Consolidation").clicked() {
                if let Err(e) = Self::suggest_consolidation(state) {
                    state.consolidation_output = format!("Error: {e}");
                    state.consolidation_moves.clear();
                }
            }

            if !state.consolidation_moves.is_empty() {
                ui.add_space(6.0);
                Self::pile_chooser(
                    ui,
                    "sparse_pile_chooser",
                    current_screen,
                    consolidation_state,
                    &state.consolidation_moves,
                );
            }

            if !state.consolidation_output.is_empty() {
                ui.add_space(6.0);
                ui.add(
                    egui::TextEdit::multiline(&mut state.consolidation_output)
                        .desired_width(f32::INFINITY)
                        .desired_rows(14)
                        .font(egui::TextStyle::Monospace),
                );
            }
        });

        ui.add_space(12.0);
        Self::show_fragmented(ui, current_screen, state, consolidation_state);
    }

    /// Fragmented-variant report + defrag move list (independent of bin fill).
    fn show_fragmented(
        ui: &mut egui::Ui,
        current_screen: &mut Screen,
        state: &mut BinAnalysisState,
        consolidation_state: &mut ConsolidationState,
    ) {
        style::screen_heading(ui, "Fragmented Variants");
        style::section_frame().show(ui, |ui| {
            ui.label(
                egui::RichText::new(
                    "Finds card variants scattered across multiple bins (regardless of how full \
                     the bins are) and gathers each into a single bin. Same read-only rules: \
                     lot/side preserved, applied only by re-loading an updated CSV.",
                )
                .size(11.0)
                .color(style::TEXT_MUTED),
            );
            ui.add_space(6.0);

            if style::primary_button(ui, "Find Fragmented Variants").clicked() {
                if let Err(e) = Self::find_fragmented(state) {
                    state.fragmented_output = format!("Error: {e}");
                    state.defrag_moves.clear();
                }
            }

            if !state.defrag_moves.is_empty() {
                ui.add_space(6.0);
                Self::pile_chooser(
                    ui,
                    "defrag_pile_chooser",
                    current_screen,
                    consolidation_state,
                    &state.defrag_moves,
                );
            }

            if !state.fragmented_output.is_empty() {
                ui.add_space(6.0);
                ui.add(
                    egui::TextEdit::multiline(&mut state.fragmented_output)
                        .desired_width(f32::INFINITY)
                        .desired_rows(16)
                        .font(egui::TextStyle::Monospace),
                );
            }
        });
    }

    /// Lets the user open the whole move list or just one source bin's piles in
    /// the interactive list.
    fn pile_chooser(
        ui: &mut egui::Ui,
        grid_id: &str,
        current_screen: &mut Screen,
        consolidation_state: &mut ConsolidationState,
        moves: &[Move],
    ) {
        // Source bins in stable order with pile/card counts.
        let mut bins: Vec<String> = moves.iter().map(|m| m.from_bin.clone()).collect();
        bins.sort();
        bins.dedup();

        ui.add_space(4.0);
        ui.label(egui::RichText::new("Open interactive move list:").strong());
        ui.label(
            egui::RichText::new(
                "Tick each pile as you move it there; export the CSV of moved piles.",
            )
            .size(11.0)
            .color(style::TEXT_MUTED),
        );
        if style::primary_button(ui, &format!("All piles ({} moves)", moves.len())).clicked() {
            *consolidation_state = ConsolidationState::from_moves(moves);
            *current_screen = Screen::Consolidation;
        }
        egui::Grid::new(grid_id)
            .num_columns(2)
            .spacing([8.0, 2.0])
            .show(ui, |ui| {
                for bin in &bins {
                    let subset: Vec<Move> = moves
                        .iter()
                        .filter(|m| &m.from_bin == bin)
                        .cloned()
                        .collect();
                    let cards: i64 = subset.iter().map(|m| m.quantity).sum();
                    if ui
                        .button(format!("Open {bin}"))
                        .on_hover_text("Open just this bin's piles")
                        .clicked()
                    {
                        *consolidation_state = ConsolidationState::from_moves(&subset);
                        *current_screen = Screen::Consolidation;
                    }
                    ui.label(
                        egui::RichText::new(format!("{} piles · {cards} cards", subset.len()))
                            .size(11.0)
                            .color(style::TEXT_MUTED),
                    );
                    ui.end_row();
                }
            });
    }

    fn find_fragmented(state: &mut BinAnalysisState) -> Result<(), Box<dyn std::error::Error>> {
        if state.inventory_path.is_empty() {
            return Err("Please select an inventory file".into());
        }
        let inventory = read_csv(&state.inventory_path)?;
        let fragmented = fragmented_variants(&inventory);
        let plan = plan_variant_defrag(&inventory);
        info!(
            "Fragmented: {} variants, defrag {} moves",
            fragmented.len(),
            plan.moves.len()
        );
        state.fragmented_output = Self::format_fragmented(&fragmented, &plan);
        state.defrag_moves = plan.moves;
        state.fragmented = fragmented;
        Ok(())
    }

    /// Renders the fragmented-variant report as a monospace summary.
    fn format_fragmented(fragmented: &[FragmentedVariant], plan: &ConsolidationPlan) -> String {
        let mut out = String::new();
        out.push_str(&format!("Fragmented variants: {}\n", fragmented.len()));
        out.push_str(&format!(
            "Defrag moves: {} ({} cards), bins freed: {}, distance: {}\n",
            plan.moves.len(),
            plan.cards_moved,
            plan.bins_freed.len(),
            plan.total_move_distance
        ));

        if fragmented.is_empty() {
            out.push_str("\nNo fragmented variants — every variant sits in a single bin.\n");
            return out;
        }

        out.push('\n');
        // Cap the listing so a huge inventory doesn't produce an unwieldy dump.
        const MAX_ROWS: usize = 200;
        for f in fragmented.iter().take(MAX_ROWS) {
            let foil = if f.is_foil { " (Foil)" } else { "" };
            let bins: Vec<String> = f
                .placements
                .iter()
                .map(|(_, loc, q)| format!("{loc}×{q}"))
                .collect();
            out.push_str(&format!(
                "  {:<28}{}  ×{} across {} bins: {}\n",
                truncate(&f.name, 28),
                foil,
                f.total_copies,
                f.bin_count(),
                bins.join(", ")
            ));
        }
        if fragmented.len() > MAX_ROWS {
            out.push_str(&format!("  … and {} more\n", fragmented.len() - MAX_ROWS));
        }
        out
    }

    fn suggest_consolidation(
        state: &mut BinAnalysisState,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if state.inventory_path.is_empty() {
            return Err("Please select an inventory file".into());
        }
        let inventory = read_csv(&state.inventory_path)?;
        let plan = plan_consolidation(&inventory, state.merge_threshold as i64);
        info!(
            "Consolidation: {} moves, {} bins freed",
            plan.moves.len(),
            plan.bins_freed.len()
        );
        state.consolidation_output = Self::format_plan(&plan);
        state.consolidation_moves = plan.moves;
        Ok(())
    }

    /// Renders a consolidation plan as a monospace report.
    fn format_plan(plan: &ConsolidationPlan) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Sparse bins considered: {}\n",
            plan.source_bins_considered
        ));
        out.push_str(&format!(
            "Variants split across bins: {}\n",
            plan.fragmented_variants
        ));
        out.push_str(&format!(
            "Bins that can be freed: {}\n",
            plan.bins_freed.len()
        ));
        out.push_str(&format!(
            "Cards to move: {} ({} moves)\n",
            plan.cards_moved,
            plan.moves.len()
        ));
        out.push_str(&format!(
            "Total move distance (lower = less walking): {}\n",
            plan.total_move_distance
        ));

        if plan.moves.is_empty() {
            out.push_str("\nNo consolidation moves found for this threshold.\n");
            return out;
        }

        out.push_str("\nFreed bins: ");
        out.push_str(&plan.bins_freed.join(", "));
        out.push_str("\n\nMoves:\n");
        for m in &plan.moves {
            out.push_str(&format!(
                "  {:>3} x {:<28} {}  ->  {}\n",
                m.quantity,
                truncate(&m.card.name, 28),
                m.from_location,
                m.to_location,
            ));
        }
        out
    }

    fn analyze_stock(state: &mut BinAnalysisState) -> Result<(), Box<dyn std::error::Error>> {
        if state.inventory_path.is_empty() {
            return Err("Please select an inventory file".into());
        }

        info!(
            "Starting bin analysis with {} free slots threshold",
            state.free_slots
        );

        let inventory = read_csv(&state.inventory_path)?;
        let analyzer = StockAnalysis::new(inventory);
        let stats = analyzer.analyze_with_free_slots(state.free_slots);

        info!(
            "Found {} bins with {} or more free slots",
            stats.available_bins.len(),
            state.free_slots
        );

        state.output = format_stock_analysis_with_sort(&stats, state.sort_order);
        Ok(())
    }
}

/// Truncates `s` to at most `max` characters, adding an ellipsis when cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let kept: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{kept}…")
    }
}
