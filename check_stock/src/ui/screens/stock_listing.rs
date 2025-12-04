use crate::scryfall::{fetch_card_cached, fetch_image, PriceGuide};
use crate::ui::state::{Screen, StockListingState};
use eframe::egui;
use log::{error, info};

pub struct StockListingScreen;

/// Parse a combined set code + collector number input like "hou120", "mh2130", or "2xm050"
/// Returns (set_code, collector_number) or None if invalid
///
/// Strategy: The collector number is always the last 3 digits, everything before is the set code.
fn parse_card_input(input: &str) -> Option<(String, String)> {
    let input = input.trim().to_lowercase();

    // Need at least 4 chars: 1 for set + 3 for collector number
    if input.len() < 4 {
        return None;
    }

    // Last 3 characters are the collector number
    let split_pos = input.len() - 3;
    let set_code = &input[..split_pos];
    let collector_number = &input[split_pos..];

    // Validate: collector number must be all digits
    if !collector_number.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    // Validate: set code must be non-empty
    if set_code.is_empty() {
        return None;
    }

    // Strip leading zeros from collector number (API requires e.g. "5" not "005")
    let collector_number = collector_number.trim_start_matches('0');
    // Handle edge case: if all zeros, keep one zero
    let collector_number = if collector_number.is_empty() {
        "0"
    } else {
        collector_number
    };

    Some((set_code.to_string(), collector_number.to_string()))
}

impl StockListingScreen {
    pub fn show(ctx: &egui::Context, current_screen: &mut Screen, state: &mut StockListingState) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("← Back to Welcome Screen").clicked() {
                    *current_screen = Screen::Welcome;
                }
            });
            ui.add_space(10.0);

            ui.heading("Card Lookup");
            ui.add_space(10.0);

            // Price guide - fetch from Cardmarket once
            if state.price_guide.is_none() && !state.price_guide_loading {
                ui.horizontal(|ui| {
                    if ui.button("Load Cardmarket Prices").clicked() {
                        state.price_guide_loading = true;
                        state.error = None;
                        match PriceGuide::fetch() {
                            Ok(guide) => {
                                info!("Fetched price guide with {} entries", guide.len());
                                state.price_guide = Some(guide);
                            }
                            Err(e) => {
                                error!("Failed to fetch price guide: {}", e);
                                state.error = Some(format!("Price guide error: {}", e));
                            }
                        }
                        state.price_guide_loading = false;
                    }
                    ui.label("(Downloads ~50MB price data from Cardmarket)");
                });
            } else if state.price_guide_loading {
                ui.label("⏳ Loading price guide...");
            } else if state.price_guide.is_some() {
                ui.label("✓ Price guide loaded");
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(10.0);

            // Card lookup input - combined field
            ui.horizontal(|ui| {
                ui.label("Card:");
                let response = ui.add(
                    egui::TextEdit::singleline(&mut state.card_input)
                        .desired_width(100.0)
                        .hint_text("e.g. hou120"),
                );

                ui.add_space(10.0);

                let parsed = parse_card_input(&state.card_input);
                let can_fetch = parsed.is_some() && !state.image_loading;

                // Fetch on Enter key or button click
                let enter_pressed =
                    response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                if ui
                    .add_enabled(can_fetch, egui::Button::new("Fetch"))
                    .clicked()
                    || (enter_pressed && can_fetch)
                {
                    if let Some((set_code, collector_number)) = parsed {
                        Self::fetch_card_data(ctx, state, &set_code, &collector_number);
                    }
                }

                // Show parsed result as hint
                if let Some((set, num)) = parse_card_input(&state.card_input) {
                    ui.label(format!("→ {} #{}", set.to_uppercase(), num));
                }
            });

            // Error display
            if let Some(ref err) = state.error {
                ui.add_space(10.0);
                ui.colored_label(egui::Color32::RED, err);
            }

            ui.add_space(10.0);
            ui.separator();

            // Card display
            if let Some(ref card) = state.card {
                Self::show_card_details(ui, state, card.clone());
            }
        });
    }

    fn fetch_card_data(
        ctx: &egui::Context,
        state: &mut StockListingState,
        set_code: &str,
        collector_number: &str,
    ) {
        state.error = None;
        state.card = None;
        state.card_image = None;
        state.image_loading = true;

        // Fetch card data (uses cache if available)
        match fetch_card_cached(&mut state.card_cache, set_code, collector_number) {
            Ok(card) => {
                info!("Fetched card: {} ({})", card.name, card.set_name);

                // Fetch image if available
                if let Some(image_url) = card.image_url() {
                    match fetch_image(image_url) {
                        Ok(bytes) => {
                            if let Ok(image) = image::load_from_memory(&bytes) {
                                let rgba = image.to_rgba8();
                                let size = [rgba.width() as usize, rgba.height() as usize];
                                let pixels = rgba.into_raw();
                                let color_image =
                                    egui::ColorImage::from_rgba_unmultiplied(size, &pixels);

                                state.card_image = Some(ctx.load_texture(
                                    format!("card_{}_{}", set_code, collector_number),
                                    color_image,
                                    egui::TextureOptions::LINEAR,
                                ));
                            }
                        }
                        Err(e) => {
                            error!("Failed to fetch image: {}", e);
                        }
                    }
                }

                state.card = Some(card);
            }
            Err(e) => {
                error!("Failed to fetch card: {}", e);
                state.error = Some(e);
            }
        }

        state.image_loading = false;
    }

    fn show_card_details(
        ui: &mut egui::Ui,
        state: &StockListingState,
        card: crate::scryfall::ScryfallCard,
    ) {
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            // Card image on the left
            if let Some(ref texture) = state.card_image {
                let max_height = 400.0;
                let aspect = texture.size()[0] as f32 / texture.size()[1] as f32;
                let width = max_height * aspect;
                ui.image((texture.id(), egui::vec2(width, max_height)));
            } else {
                ui.label("(No image available)");
            }

            ui.add_space(20.0);

            // Card details on the right
            ui.vertical(|ui| {
                ui.heading(&card.name);
                ui.add_space(5.0);

                ui.label(format!("{} ({})", card.set_name, card.set.to_uppercase()));
                ui.label(format!(
                    "#{} • {}",
                    card.collector_number,
                    card.rarity.to_uppercase()
                ));

                if let Some(ref mana_cost) = card.mana_cost {
                    ui.label(format!("Mana: {}", mana_cost));
                }

                if let Some(ref type_line) = card.type_line {
                    ui.label(type_line);
                }

                ui.add_space(10.0);

                // Scryfall prices
                ui.label("Scryfall Prices:");
                ui.horizontal(|ui| {
                    if let Some(ref eur) = card.prices.eur {
                        ui.label(format!("EUR: {} €", eur));
                    }
                    if let Some(ref eur_foil) = card.prices.eur_foil {
                        ui.label(format!("EUR Foil: {} €", eur_foil));
                    }
                });

                // Cardmarket prices from price guide
                if let Some(cardmarket_id) = card.cardmarket_id {
                    ui.add_space(10.0);
                    ui.label(format!("Cardmarket ID: {}", cardmarket_id));

                    if let Some(ref guide) = state.price_guide {
                        if let Some(prices) = guide.get(cardmarket_id) {
                            ui.add_space(5.0);
                            ui.heading("Cardmarket Price Guide");

                            egui::Grid::new("price_grid")
                                .num_columns(3)
                                .spacing([30.0, 4.0])
                                .striped(true)
                                .show(ui, |ui| {
                                    // Header row
                                    ui.label("");
                                    ui.strong("Regular");
                                    ui.strong("Foil");
                                    ui.end_row();

                                    // Trend
                                    ui.label("Trend:");
                                    ui.label(format_price(prices.trend));
                                    ui.label(format_price(prices.trend_foil));
                                    ui.end_row();

                                    // Low
                                    ui.label("Low:");
                                    ui.label(format_price(prices.low));
                                    ui.label(format_price(prices.low_foil));
                                    ui.end_row();

                                    // Average
                                    ui.label("Average:");
                                    ui.label(format_price(prices.avg));
                                    ui.label(format_price(prices.avg_foil));
                                    ui.end_row();

                                    // 1-day average
                                    ui.label("Avg (1 day):");
                                    ui.label(format_price(prices.avg1));
                                    ui.label(format_price(prices.avg1_foil));
                                    ui.end_row();

                                    // 7-day average
                                    ui.label("Avg (7 days):");
                                    ui.label(format_price(prices.avg7));
                                    ui.label(format_price(prices.avg7_foil));
                                    ui.end_row();

                                    // 30-day average
                                    ui.label("Avg (30 days):");
                                    ui.label(format_price(prices.avg30));
                                    ui.label(format_price(prices.avg30_foil));
                                    ui.end_row();
                                });
                        } else {
                            ui.label("(Not found in price guide)");
                        }
                    }
                }
            });
        });
    }
}

fn format_price(price: Option<f64>) -> String {
    match price {
        Some(p) => format!("{:.2} €", p),
        None => "—".to_string(),
    }
}
