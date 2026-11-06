//! Left panel: the ordered rule list with its editor, the guardrails, and
//! the condition/language factors.

use super::{muted, thousands, Note, PricingState, ReviewFilter};
use crate::api::inventory_sync::PriceField;
use crate::repricing::{
    euro, load_ruleset, rule_templates, save_ruleset, Combine, Condition, Direction, Finish,
    MarketMove, PriceAction, PriceBase, Rounding, Rule, Ruleset, CONDITIONS,
};
use crate::ui::style;
use eframe::egui;

/// A click in the rule list, applied after drawing it.
enum ListAction {
    Select(usize),
    Move(usize, bool),
    Duplicate(usize),
    Delete(usize),
    ShowInTable(usize),
}

pub(super) fn show(ctx: &egui::Context, state: &mut PricingState) {
    egui::SidePanel::left("pricing_rules")
        .resizable(true)
        .default_width(460.0)
        .width_range(360.0..=760.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("pricing_rules_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    show_ruleset_bar(ui, state);
                    ui.add_space(8.0);
                    show_rule_list(ui, state);
                    ui.add_space(10.0);
                    show_guardrails(ui, state);
                    ui.add_space(4.0);
                    show_adjustments(ui, state);
                    ui.add_space(12.0);
                });
        });
}

// ── Ruleset bar ─────────────────────────────────────────────────────────────

fn show_ruleset_bar(ui: &mut egui::Ui, state: &mut PricingState) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("3")
                .strong()
                .color(style::ACCENT)
                .size(15.0),
        );
        ui.label(
            egui::RichText::new("Rules")
                .strong()
                .size(14.0)
                .color(style::TEXT_PRIMARY),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("☰ Ruleset", |ui| {
                if ui.button("Open…").clicked() {
                    ui.close_menu();
                    open_ruleset(state);
                }
                if ui.button("Save a copy…").clicked() {
                    ui.close_menu();
                    save_copy(state);
                }
                ui.separator();
                if ui
                    .button("Replace with starter rules")
                    .on_hover_text("Your current rules can be restored with Undo")
                    .clicked()
                {
                    ui.close_menu();
                    state.replace_ruleset(
                        Ruleset::starter(),
                        Note::ok("Replaced with the starter rules"),
                    );
                }
            });
        });
    });
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.ruleset.name)
                .hint_text("Ruleset name")
                .desired_width(ui.available_width() - 4.0),
        );
    });
    muted(
        ui,
        "Each listing takes its price from the first rule that matches, top to bottom. \
         Changes are saved automatically.",
    )
    .on_hover_text(state.ruleset_path.display().to_string());
    if let Some(note) = state.ruleset_note.clone() {
        ui.horizontal_wrapped(|ui| {
            let color = if note.error {
                style::COLOR_ERROR
            } else {
                style::COLOR_SUCCESS
            };
            ui.label(egui::RichText::new(&note.text).size(11.5).color(color));
            if state.undo_ruleset.is_some() && ui.small_button("Undo").clicked() {
                state.undo_replace();
            }
            if ui.small_button("✖").on_hover_text("Dismiss").clicked() {
                state.ruleset_note = None;
                state.undo_ruleset = None;
            }
        });
    }
}

fn open_ruleset(state: &mut PricingState) {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Open repricing rules")
        .add_filter("JSON", &["json"])
        .pick_file()
    else {
        return;
    };
    match load_ruleset(&path) {
        Ok(rs) => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            state.replace_ruleset(rs, Note::ok(format!("Opened {name}")));
        }
        Err(e) => state.ruleset_note = Some(Note::error(e)),
    }
}

fn save_copy(state: &mut PricingState) {
    let file_name = format!(
        "{}.json",
        sanitize_file_stem(&state.ruleset.name).unwrap_or_else(|| "repricing_rules".to_string())
    );
    let Some(path) = rfd::FileDialog::new()
        .set_title("Save a copy of the rules")
        .add_filter("JSON", &["json"])
        .set_file_name(file_name)
        .save_file()
    else {
        return;
    };
    state.ruleset_note = Some(match save_ruleset(&path, &state.ruleset) {
        Ok(()) => Note::ok(format!("Saved a copy to {}", path.display())),
        Err(e) => Note::error(format!("Could not save {}: {e}", path.display())),
    });
}

/// A ruleset name reduced to a safe file stem (letters, digits, `-`, `_`).
pub(super) fn sanitize_file_stem(name: &str) -> Option<String> {
    let stem: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(60)
        .collect();
    let stem = stem.trim_matches('_').to_string();
    (!stem.is_empty()).then_some(stem)
}

// ── Rule list ───────────────────────────────────────────────────────────────

fn show_rule_list(ui: &mut egui::Ui, state: &mut PricingState) {
    let mut action: Option<ListAction> = None;
    let count = state.ruleset.rules.len();
    let reference = state.ruleset.guardrails.reference;
    let has_stock = !state.listings.is_empty();

    if count == 0 {
        muted(
            ui,
            "No rules yet: every listing keeps its price. Add one below.",
        );
    }

    for i in 0..count {
        let selected = state.selected_rule == Some(i);
        let stats = state.eval.rule_stats.get(i).copied().unwrap_or_default();
        let fill = if selected {
            style::BTN_SECONDARY
        } else {
            style::PANEL_BG
        };
        let stroke = if selected {
            style::ACCENT
        } else {
            style::PANEL_BORDER
        };
        egui::Frame::new()
            .fill(fill)
            .stroke(egui::Stroke::new(1.0_f32, stroke))
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let rule = &mut state.ruleset.rules[i];
                ui.horizontal(|ui| {
                    ui.checkbox(&mut rule.enabled, "")
                        .on_hover_text("Turn this rule on or off");
                    ui.label(
                        egui::RichText::new(format!("{}", i + 1))
                            .color(style::TEXT_MUTED)
                            .monospace(),
                    );
                    let name = if rule.name.trim().is_empty() {
                        "Unnamed rule"
                    } else {
                        rule.name.as_str()
                    };
                    let color = if rule.enabled {
                        style::TEXT_PRIMARY
                    } else {
                        style::TEXT_MUTED
                    };
                    let title = ui
                        .add(
                            egui::Label::new(egui::RichText::new(name).strong().color(color))
                                .sense(egui::Sense::click())
                                .truncate(),
                        )
                        .on_hover_text("Click to edit");
                    if title.clicked() {
                        action = Some(ListAction::Select(i));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("🗑").on_hover_text("Delete rule").clicked() {
                            action = Some(ListAction::Delete(i));
                        }
                        if ui
                            .small_button("🗐")
                            .on_hover_text("Duplicate rule")
                            .clicked()
                        {
                            action = Some(ListAction::Duplicate(i));
                        }
                        if ui
                            .add_enabled(i + 1 < count, egui::Button::new("⏷").small())
                            .on_hover_text("Move down")
                            .clicked()
                        {
                            action = Some(ListAction::Move(i, false));
                        }
                        if ui
                            .add_enabled(i > 0, egui::Button::new("⏶").small())
                            .on_hover_text("Move up")
                            .clicked()
                        {
                            action = Some(ListAction::Move(i, true));
                        }
                    });
                });
                for (word, text) in [
                    ("If", rule.describe_when(reference)),
                    ("Then", rule.action.describe()),
                ] {
                    let clicked = ui
                        .horizontal_top(|ui| {
                            ui.add_sized(
                                [30.0, 14.0],
                                egui::Label::new(
                                    egui::RichText::new(word).size(11.5).color(style::ACCENT),
                                ),
                            );
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(text)
                                        .size(11.5)
                                        .color(style::TEXT_MUTED),
                                )
                                .sense(egui::Sense::click()),
                            )
                            .clicked()
                        })
                        .inner;
                    if clicked {
                        action = Some(ListAction::Select(i));
                    }
                }
                if has_stock && rule.enabled {
                    ui.horizontal(|ui| {
                        rule_stats_line(ui, &stats, &rule.action.base);
                        if stats.matched > 0
                            && ui
                                .small_button("Show")
                                .on_hover_text("Show this rule's listings in the table")
                                .clicked()
                        {
                            action = Some(ListAction::ShowInTable(i));
                        }
                    });
                }
                if selected {
                    ui.add_space(6.0);
                    ui.separator();
                    let choices = Choices {
                        rarities: &state.rarities,
                        languages: &state.languages,
                        reference,
                    };
                    rule_editor(ui, &mut state.ruleset.rules[i], &choices, i);
                }
            });
        ui.add_space(4.0);
    }

    match action {
        Some(ListAction::Select(i)) => {
            state.selected_rule = if state.selected_rule == Some(i) {
                None
            } else {
                Some(i)
            };
        }
        Some(ListAction::Move(i, up)) => state.move_rule(i, up),
        Some(ListAction::Duplicate(i)) => state.duplicate_rule(i),
        Some(ListAction::Delete(i)) => state.delete_rule(i),
        Some(ListAction::ShowInTable(i)) => {
            state.rule_filter = Some(i);
            state.filter = ReviewFilter::All;
        }
        None => {}
    }

    ui.horizontal(|ui| {
        ui.menu_button(
            egui::RichText::new("+ Add rule")
                .strong()
                .color(style::ACCENT),
            |ui| {
                ui.set_min_width(300.0);
                if ui
                    .button("Empty rule")
                    .on_hover_text("Matches every listing until you add conditions")
                    .clicked()
                {
                    ui.close_menu();
                    state.add_rule(Rule::new("New rule"));
                }
                ui.separator();
                ui.label(
                    egui::RichText::new("Ready-made")
                        .small()
                        .color(style::TEXT_MUTED),
                );
                for t in rule_templates() {
                    if ui.button(&t.rule.name).on_hover_text(t.hint).clicked() {
                        ui.close_menu();
                        state.add_rule(t.rule);
                    }
                }
            },
        );
    });
}

fn rule_stats_line(ui: &mut egui::Ui, s: &crate::repricing::RuleStats, base: &PriceBase) {
    let mut parts = vec![format!("{} listings", thousands(s.matched as i64))];
    if matches!(base, PriceBase::Keep) {
        parts.push("kept".to_string());
    } else {
        if s.raised > 0 {
            parts.push(format!("{} up", thousands(s.raised as i64)));
        }
        if s.lowered > 0 {
            parts.push(format!("{} down", thousands(s.lowered as i64)));
        }
        if s.raised + s.lowered > 0 {
            parts.push(signed_euro(s.value_delta));
        }
    }
    let color = if s.matched == 0 {
        style::COLOR_WARNING
    } else {
        style::TEXT_MUTED
    };
    let label = ui.label(
        egui::RichText::new(parts.join(" · "))
            .size(11.5)
            .color(color),
    );
    if s.matched == 0 {
        label.on_hover_text(
            "No listing reaches this rule: its conditions match nothing, or rules above it \
             take every match first",
        );
    }
}

pub(super) fn signed_euro(v: f64) -> String {
    if v < 0.0 {
        format!("−{}", euro(-v))
    } else {
        format!("+{}", euro(v))
    }
}

// ── Rule editor ─────────────────────────────────────────────────────────────

/// Values the chips offer.
struct Choices<'a> {
    rarities: &'a [String],
    languages: &'a [String],
    reference: PriceField,
}

fn rule_editor(ui: &mut egui::Ui, rule: &mut Rule, choices: &Choices, id: usize) {
    ui.horizontal(|ui| {
        ui.label("Name");
        ui.add(egui::TextEdit::singleline(&mut rule.name).desired_width(ui.available_width()));
    });
    ui.add_space(6.0);
    section_label(ui, "When", "all conditions must match");
    if rule.conditions.is_empty() {
        muted(ui, "No conditions: matches every listing that reaches it.");
    }
    let mut remove: Option<usize> = None;
    for (ci, cond) in rule.conditions.iter_mut().enumerate() {
        ui.horizontal_top(|ui| {
            ui.add_sized(
                [96.0, 20.0],
                egui::Label::new(egui::RichText::new(cond.kind_label()).strong()),
            );
            ui.vertical(|ui| {
                ui.set_max_width(ui.available_width() - 28.0);
                condition_widget(ui, cond, choices);
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                if ui
                    .small_button("✖")
                    .on_hover_text("Remove condition")
                    .clicked()
                {
                    remove = Some(ci);
                }
            });
        });
        ui.add_space(2.0);
    }
    if let Some(ci) = remove {
        rule.conditions.remove(ci);
    }
    ui.menu_button("+ Add condition", |ui| {
        ui.set_min_width(220.0);
        for template in Condition::templates() {
            let taken = rule.conditions.iter().any(|c| c.same_kind(&template));
            if ui
                .add_enabled(!taken, egui::Button::new(template.kind_label()))
                .on_hover_text(condition_help(&template))
                .clicked()
            {
                ui.close_menu();
                rule.conditions.push(template);
            }
        }
    });

    ui.add_space(8.0);
    section_label(ui, "Then", "the new price");
    price_editor(ui, &mut rule.action, id);
}

fn section_label(ui: &mut egui::Ui, title: &str, sub: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .strong()
                .color(style::ACCENT)
                .size(13.0),
        );
        muted(ui, sub);
    });
}

fn condition_help(c: &Condition) -> &'static str {
    match c {
        Condition::Rarity(_) => "Rarity is one of the picked ones",
        Condition::CardCondition(_) => "Card condition is one of the picked ones",
        Condition::Language(_) => "Card language is one of the picked ones",
        Condition::Finish(_) => "Foil or non-foil",
        Condition::ListedPrice { .. } => "Your current price is in a range",
        Condition::MarketPrice { .. } => {
            "The market price (column set in the guardrails) is in a range"
        }
        Condition::Name(_) => "Card name contains a word (comma-separate several)",
        Condition::Set(_) => "Set name or code contains a word (comma-separate several)",
        Condition::Location(_) => "Storage location contains a word (comma-separate several)",
        Condition::AgeDays { .. } => "Days since the listing first appeared in an imported export",
        Condition::NoSaleDays(_) => {
            "Days since this exact listing last sold (or appeared, if it never sold)"
        }
        Condition::MinSold30d(_) => "Copies of this exact listing sold in the last 30 days",
        Condition::MinQuantity(_) => "Copies of this listing in stock",
        Condition::Moving(_) => "Market direction from Cardmarket's 1/7/30-day averages",
    }
}

fn condition_widget(ui: &mut egui::Ui, cond: &mut Condition, choices: &Choices) {
    match cond {
        Condition::Rarity(list) => chips(ui, list, choices.rarities),
        Condition::CardCondition(list) => {
            let all: Vec<String> = CONDITIONS.iter().map(|c| c.to_string()).collect();
            chips(ui, list, &all);
        }
        Condition::Language(list) => chips(ui, list, choices.languages),
        Condition::Finish(f) => {
            ui.horizontal(|ui| {
                for option in Finish::ALL {
                    ui.selectable_value(f, option, option.label());
                }
            });
        }
        Condition::ListedPrice { min, max } => price_range(ui, min, max),
        Condition::MarketPrice { min, max } => {
            price_range(ui, min, max);
            muted(
                ui,
                &format!("Uses the {} column", choices.reference.as_str()),
            );
        }
        Condition::Name(t) | Condition::Set(t) | Condition::Location(t) => {
            ui.add(
                egui::TextEdit::singleline(t)
                    .hint_text("contains… (comma-separated)")
                    .desired_width(ui.available_width()),
            );
        }
        Condition::AgeDays { min, max } => {
            ui.horizontal(|ui| {
                opt_days(ui, "at least", min, 90);
                opt_days(ui, "at most", max, 365);
            });
        }
        Condition::NoSaleDays(d) => {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(d).range(0..=3650).suffix(" days"));
                muted(ui, "or more");
            });
        }
        Condition::MinSold30d(n) => {
            ui.horizontal(|ui| {
                ui.label("at least");
                ui.add(egui::DragValue::new(n).range(1..=1000));
                muted(ui, "copies in 30 days");
            });
        }
        Condition::MinQuantity(n) => {
            ui.horizontal(|ui| {
                ui.label("at least");
                ui.add(egui::DragValue::new(n).range(1..=10000));
                muted(ui, "copies");
            });
        }
        Condition::Moving(m) => {
            ui.horizontal(|ui| {
                for option in MarketMove::ALL {
                    ui.selectable_value(m, option, option.label());
                }
            });
        }
    }
}

/// Toggle chips for a list condition. Values already in the list but not
/// offered (e.g. from an opened file) stay visible so they can be removed.
fn chips(ui: &mut egui::Ui, list: &mut Vec<String>, offered: &[String]) {
    let mut options: Vec<String> = offered.to_vec();
    for v in list.iter() {
        if !options.iter().any(|o| o.eq_ignore_ascii_case(v)) {
            options.push(v.clone());
        }
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for option in &options {
            let on = list.iter().any(|v| v.eq_ignore_ascii_case(option));
            if ui.selectable_label(on, option).clicked() {
                if on {
                    list.retain(|v| !v.eq_ignore_ascii_case(option));
                } else {
                    list.push(option.clone());
                }
            }
        }
    });
    if list.is_empty() {
        muted(ui, "none picked = any");
    }
}

fn price_range(ui: &mut egui::Ui, min: &mut Option<f64>, max: &mut Option<f64>) {
    ui.horizontal(|ui| {
        opt_euro(ui, "from", min, 1.0);
        opt_euro(ui, "to", max, 10.0);
    });
}

/// Checkbox + € field for an optional bound.
fn opt_euro(ui: &mut egui::Ui, label: &str, value: &mut Option<f64>, default: f64) {
    let mut on = value.is_some();
    if ui.checkbox(&mut on, label).changed() {
        *value = on.then_some(value.unwrap_or(default));
    }
    if let Some(v) = value {
        ui.add(euro_drag(v));
    }
}

fn opt_days(ui: &mut egui::Ui, label: &str, value: &mut Option<u32>, default: u32) {
    let mut on = value.is_some();
    if ui.checkbox(&mut on, label).changed() {
        *value = on.then_some(value.unwrap_or(default));
    }
    if let Some(v) = value {
        ui.add(egui::DragValue::new(v).range(0..=3650).suffix(" d"));
    }
}

fn euro_drag(v: &mut f64) -> egui::DragValue<'_> {
    egui::DragValue::new(v)
        .range(0.0..=crate::repricing::MAX_PRICE_EUR)
        .speed(0.01)
        .fixed_decimals(2)
        .prefix("€")
}

fn field_combo(ui: &mut egui::Ui, salt: (&str, usize), field: &mut PriceField) {
    egui::ComboBox::from_id_salt(salt)
        .selected_text(field.as_str())
        .width(110.0)
        .show_ui(ui, |ui| {
            for f in PriceField::all() {
                ui.selectable_value(field, *f, f.as_str());
            }
        });
}

fn price_editor(ui: &mut egui::Ui, a: &mut PriceAction, id: usize) {
    // Combo boxes cannot wrap, so the market columns get their own line.
    ui.horizontal(|ui| {
        ui.label("Start from");
        egui::ComboBox::from_id_salt(("pricing_base", id))
            .selected_text(a.base.label())
            .width(120.0)
            .show_ui(ui, |ui| {
                let options = [
                    PriceBase::market(PriceField::Trend),
                    PriceBase::Current,
                    PriceBase::Fixed(1.0),
                    PriceBase::Keep,
                ];
                for option in options {
                    let same = std::mem::discriminant(&option) == std::mem::discriminant(&a.base);
                    if ui.selectable_label(same, option.label()).clicked() && !same {
                        a.base = option;
                    }
                }
            });
        if let PriceBase::Fixed(p) = &mut a.base {
            ui.add(euro_drag(p));
        }
    });
    if let PriceBase::Market {
        field,
        combine,
        other,
    } = &mut a.base
    {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            field_combo(ui, ("pricing_field", id), field);
            egui::ComboBox::from_id_salt(("pricing_combine", id))
                .selected_text(combine.label())
                .width(96.0)
                .show_ui(ui, |ui| {
                    for c in Combine::ALL {
                        ui.selectable_value(combine, c, c.label());
                    }
                })
                .response
                .on_hover_text("Combine two columns, e.g. never below the 30-day average");
            if *combine != Combine::Only {
                field_combo(ui, ("pricing_other", id), other);
            }
        });
    }
    if matches!(a.base, PriceBase::Keep) {
        muted(
            ui,
            "Matching listings keep their price, and the rules below never see them.",
        );
        return;
    }
    ui.horizontal(|ui| {
        ui.label("×");
        ui.add(
            egui::DragValue::new(&mut a.percent)
                .range(0.0..=1000.0)
                .speed(0.5)
                .suffix(" %"),
        );
        ui.label("+");
        ui.add(
            egui::DragValue::new(&mut a.offset)
                .range(-1000.0..=1000.0)
                .speed(0.01)
                .fixed_decimals(2)
                .prefix("€"),
        )
        .on_hover_text("Negative to undercut, e.g. −0.01");
    });
    if matches!(a.base, PriceBase::Market { .. }) {
        ui.checkbox(&mut a.adjust, "Adjust for condition and language")
            .on_hover_text(
                "The price guide is roughly near-mint and ignores language. Uses the factors \
                 under \"Condition & language\" below.",
            );
    }
    ui.horizontal(|ui| {
        ui.label("Direction");
        egui::ComboBox::from_id_salt(("pricing_direction", id))
            .selected_text(a.direction.label())
            .width(130.0)
            .show_ui(ui, |ui| {
                for d in Direction::ALL {
                    ui.selectable_value(&mut a.direction, d, d.label());
                }
            });
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Limits");
        opt_euro(ui, "min", &mut a.min_price, 0.10);
        opt_euro(ui, "max", &mut a.max_price, 100.0);
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Per run");
        opt_pct(ui, "raise at most", &mut a.max_raise_pct, 25.0, 1000.0);
        opt_pct(ui, "drop at most", &mut a.max_drop_pct, 20.0, 100.0);
    });
}

fn opt_pct(ui: &mut egui::Ui, label: &str, value: &mut Option<f64>, default: f64, max: f64) {
    let mut on = value.is_some();
    if ui.checkbox(&mut on, label).changed() {
        *value = on.then_some(value.unwrap_or(default));
    }
    if let Some(v) = value {
        ui.add(
            egui::DragValue::new(v)
                .range(0.0..=max)
                .speed(0.5)
                .suffix(" %"),
        );
    }
}

// ── Guardrails ──────────────────────────────────────────────────────────────

fn show_guardrails(ui: &mut egui::Ui, state: &mut PricingState) {
    let g = &mut state.ruleset.guardrails;
    egui::CollapsingHeader::new(egui::RichText::new("Guardrails (apply to every rule)").strong())
        .id_salt("pricing_guardrails")
        .default_open(true)
        .show(ui, |ui| {
            ui.label("Never below");
            egui::Grid::new("pricing_floors")
                .num_columns(4)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    let f = &mut g.floors;
                    ui.label("Common");
                    ui.add(euro_drag(&mut f.common));
                    ui.label("Uncommon");
                    ui.add(euro_drag(&mut f.uncommon));
                    ui.end_row();
                    ui.label("Rare");
                    ui.add(euro_drag(&mut f.rare));
                    ui.label("Mythic");
                    ui.add(euro_drag(&mut f.mythic));
                    ui.end_row();
                    ui.label("Other")
                        .on_hover_text("Tokens, lands, specials and unknown rarities");
                    ui.add(euro_drag(&mut f.other));
                    ui.end_row();
                });
            muted(
                ui,
                "Floors win over every rule limit and direction; Cardmarket's minimum is €0.02.",
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Rounding");
                egui::ComboBox::from_id_salt("pricing_rounding")
                    .selected_text(g.rounding.label())
                    .show_ui(ui, |ui| {
                        for r in Rounding::ALL {
                            ui.selectable_value(&mut g.rounding, r, r.label());
                        }
                    });
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Ignore changes under");
                ui.add(
                    egui::DragValue::new(&mut g.min_change_eur)
                        .range(0.0..=100.0)
                        .speed(0.01)
                        .fixed_decimals(2)
                        .prefix("€"),
                );
                ui.label("or");
                ui.add(
                    egui::DragValue::new(&mut g.min_change_pct)
                        .range(0.0..=100.0)
                        .speed(0.25)
                        .suffix(" %"),
                );
            })
            .response
            .on_hover_text(
                "Avoids re-uploading thousands of listings for a cent. Floor lifts always apply.",
            );
            ui.horizontal_wrapped(|ui| {
                ui.label("Skip market prices older than");
                ui.add(
                    egui::DragValue::new(&mut g.stale_days)
                        .range(0..=3650)
                        .suffix(" days"),
                );
                muted(ui, "(0 = never skip)");
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Ask before changes over");
                ui.add(
                    egui::DragValue::new(&mut g.review_pct)
                        .range(0.0..=1000.0)
                        .speed(0.5)
                        .suffix(" %"),
                );
                ui.label("and");
                ui.add(
                    egui::DragValue::new(&mut g.review_min_eur)
                        .range(0.0..=10000.0)
                        .speed(0.05)
                        .fixed_decimals(2)
                        .prefix("€"),
                );
            })
            .response
            .on_hover_text("Such changes start unticked in the review table until you tick them.");
            ui.horizontal(|ui| {
                ui.label("\"Market price\" column");
                field_combo(ui, ("pricing_reference", 0), &mut g.reference);
            })
            .response
            .on_hover_text("Used by the market price condition and the table's Market column");
        });
}

// ── Condition & language factors ───────────────────────────────────────────

fn show_adjustments(ui: &mut egui::Ui, state: &mut PricingState) {
    let languages = state.languages.clone();
    let adj = &mut state.ruleset.adjustments;
    egui::CollapsingHeader::new(egui::RichText::new("Condition & language").strong())
        .id_salt("pricing_adjustments")
        .default_open(false)
        .show(ui, |ui| {
            muted(
                ui,
                "Market prices are multiplied by these when a rule has \"Adjust for condition \
                 and language\" ticked.",
            );
            egui::Grid::new("pricing_condition_factors")
                .num_columns(6)
                .spacing([6.0, 4.0])
                .show(ui, |ui| {
                    for (i, c) in CONDITIONS.iter().enumerate() {
                        let v = adj.condition_pct.entry(c.to_string()).or_insert(100.0);
                        ui.label(*c);
                        ui.add(pct_drag(v));
                        if i % 3 == 2 {
                            ui.end_row();
                        }
                    }
                });
            ui.add_space(4.0);
            let mut all: Vec<String> = languages;
            for k in adj.language_pct.keys() {
                if !all.iter().any(|l| l.eq_ignore_ascii_case(k)) {
                    all.push(k.clone());
                }
            }
            egui::Grid::new("pricing_language_factors")
                .num_columns(6)
                .spacing([6.0, 4.0])
                .show(ui, |ui| {
                    for (i, lang) in all.iter().enumerate() {
                        let key = adj
                            .language_pct
                            .keys()
                            .find(|k| k.eq_ignore_ascii_case(lang))
                            .cloned()
                            .unwrap_or_else(|| lang.clone());
                        let mut v = adj.language_pct.get(&key).copied().unwrap_or(100.0);
                        ui.label(lang);
                        if ui.add(pct_drag(&mut v)).changed() {
                            adj.language_pct.insert(key, v);
                        }
                        if i % 3 == 2 {
                            ui.end_row();
                        }
                    }
                });
        });
}

fn pct_drag(v: &mut f64) -> egui::DragValue<'_> {
    egui::DragValue::new(v)
        .range(0.0..=300.0)
        .speed(0.5)
        .suffix(" %")
}
