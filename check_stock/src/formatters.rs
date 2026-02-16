use crate::card_matching::{get_card_name, parse_location_code, MatchedCard};
use crate::models::Language;

pub fn format_regular_output(
    matches: &[(String, i32, Vec<MatchedCard>)],
    discount_percent: f32,
) -> String {
    let mut output = String::new();
    let mut total_price = 0.0;
    let discount_factor = 1.0 - (discount_percent as f64 / 100.0);

    for (card_name, needed_quantity, matched_cards) in matches {
        if matched_cards.is_empty() {
            continue;
        }

        let card_total_cost: f64 = matched_cards
            .iter()
            .map(|mc| mc.card.price_f64() * mc.quantity as f64)
            .sum();
        let discounted_card_total_cost = card_total_cost * discount_factor;

        let total_found: i32 = matched_cards.iter().map(|mc| mc.quantity).sum();

        // Show "X of Y" if not all copies are available, otherwise just show X
        let quantity_display = if total_found < *needed_quantity {
            format!("{total_found} of {needed_quantity}")
        } else {
            format!("{total_found}")
        };

        if discount_percent > 0.0 {
            output.push_str(&format!("{quantity_display} x {card_name} (total: {discounted_card_total_cost:.2} € after {discount_percent:.1}% discount)\n"));
        } else {
            output.push_str(&format!(
                "{quantity_display} x {card_name} (total: {card_total_cost:.2} €)\n"
            ));
        }

        // Show copies from each set with their individual prices
        for matched_card in matched_cards {
            let location_info = matched_card
                .card
                .location
                .as_ref()
                .filter(|loc| !loc.trim().is_empty())
                .map(|loc| format!(" [Location: {loc}]"))
                .unwrap_or_default();

            // Add special conditions
            let special_conditions = matched_card.card.special_conditions();
            let special_info = if !special_conditions.is_empty() {
                format!(" ({})", special_conditions.join(", "))
            } else {
                String::new()
            };

            // Add comment if present
            let comment_info = if !matched_card.card.comment.trim().is_empty() {
                format!(" - Note: {}", matched_card.card.comment.trim())
            } else {
                String::new()
            };

            output.push_str(&format!(
                "    {} {} [{}]{} from {}, {} condition - {:.2} €{}{}\n",
                matched_card.quantity,
                if matched_card.quantity == 1 {
                    "copy"
                } else {
                    "copies"
                },
                matched_card.card.language,
                special_info,
                matched_card.set_name,
                matched_card.card.condition,
                matched_card.card.price_f64(),
                location_info,
                comment_info
            ));
        }

        if total_found < *needed_quantity {
            output.push_str(&format!(
                "    WARNING: Only {total_found} of {needed_quantity} copies available!\n"
            ));
        }

        output.push('\n');
        total_price += discounted_card_total_cost;
    }

    if !matches.is_empty() {
        let total_cards: i32 = matches
            .iter()
            .flat_map(|(_, _, cards)| cards.iter())
            .map(|mc| mc.quantity)
            .sum();

        output.push_str("========================\n");
        if discount_percent > 0.0 {
            output.push_str(&format!("Total price for available cards after {discount_percent:.1}% discount: {total_price:.2} €\n"));
        } else {
            output.push_str(&format!(
                "Total price for available cards: {total_price:.2} €\n"
            ));
        }
        output.push_str(&format!("Total cards picked: {total_cards}\n"));
    } else {
        output.push_str("No cards from your wantslist were found in stock.\n");
    }

    output
}

pub fn format_picking_list(matched_cards: &[MatchedCard]) -> String {
    let mut output_entries = Vec::new();
    let mut max_qty_len = 3; // Minimum width for "Qty"
    let mut max_loc_len = 0;
    let mut max_name_len = 0;
    let mut max_lang_len = 0;
    let mut max_rarity_len = 0;
    let mut max_cn_len = 0;
    let mut max_set_len = 0;

    // Calculate maximum lengths for alignment
    for matched_card in matched_cards {
        let card = matched_card.card;
        max_qty_len = max_qty_len.max(matched_card.quantity.to_string().len());
        max_loc_len = max_loc_len.max(card.location.as_deref().unwrap_or("").len());
        max_name_len = max_name_len.max(card.name.len());
        max_lang_len = max_lang_len.max(card.language.len());
        max_rarity_len = max_rarity_len.max(card.rarity.len());
        max_cn_len = max_cn_len.max(card.cn.len());
        max_set_len = max_set_len.max(matched_card.set_name.len());
    }

    // Create entries for each card
    for matched_card in matched_cards {
        let card = matched_card.card;
        let sort_key = card.location.as_deref().unwrap_or("").to_string();

        // Get localized name based on language
        let mut name = match Language::parse(&card.language) {
            Some(Language::German) => card.name_de.clone(),
            Some(Language::Spanish) => card.name_es.clone(),
            Some(Language::French) => card.name_fr.clone(),
            Some(Language::Italian) => card.name_it.clone(),
            _ => card.name.clone(),
        };

        // If localized name is empty, fall back to English name
        if name.trim().is_empty() {
            name = card.name.clone();
        }

        // Add special conditions to name
        let special_conditions = card.special_conditions();
        if !special_conditions.is_empty() {
            name = format!("{} ({})", name, special_conditions.join(", "));
        }

        // Handle playsets
        if card.is_playset_card() {
            name = format!("{name} [Playset]");
        }

        // Add comment if present
        if !card.comment.trim().is_empty() {
            name = format!("{} - Note: {}", name, card.comment.trim());
        }

        let entry = format!(
            "{:>width_qty$} | {:<width_loc$} | {:<width_name$} | {:<width_lang$} | {:<width_rarity$} | {:<width_cn$} | {:<width_set$}\n",
            matched_card.quantity,
            card.location.as_deref().unwrap_or(""),
            name,
            card.language,
            card.rarity,
            card.cn,
            matched_card.set_name,
            width_qty = max_qty_len,
            width_loc = max_loc_len,
            width_name = max_name_len,
            width_lang = max_lang_len,
            width_rarity = max_rarity_len,
            width_cn = max_cn_len,
            width_set = max_set_len,
        );
        output_entries.push((sort_key, entry));
    }

    // Sort by location
    output_entries.sort_by(|(loc_a, _), (loc_b, _)| {
        if loc_a.is_empty() && loc_b.is_empty() {
            std::cmp::Ordering::Equal
        } else if loc_a.is_empty() {
            std::cmp::Ordering::Greater
        } else if loc_b.is_empty() {
            std::cmp::Ordering::Less
        } else {
            let parts_a = parse_location_code(loc_a);
            let parts_b = parse_location_code(loc_b);
            parts_a.cmp(&parts_b)
        }
    });

    // Create header
    let header = format!(
        "{:>width_qty$} | {:<width_loc$} | {:<width_name$} | {:<width_lang$} | {:<width_rarity$} | {:<width_cn$} | {:<width_set$}\n",
        "Qty",
        "Location",
        "Name",
        "Language",
        "Rarity",
        "Collector Number",
        "Set",
        width_qty = max_qty_len,
        width_loc = max_loc_len,
        width_name = max_name_len,
        width_lang = max_lang_len,
        width_rarity = max_rarity_len,
        width_cn = max_cn_len,
        width_set = max_set_len,
    );

    // Create separator line
    let separator = format!(
        "{:->width_qty$}-+-{:-<width_loc$}-+-{:-<width_name$}-+-{:-<width_lang$}-+-{:-<width_rarity$}-+-{:-<width_cn$}-+-{:-<width_set$}\n",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        width_qty = max_qty_len,
        width_loc = max_loc_len,
        width_name = max_name_len,
        width_lang = max_lang_len,
        width_rarity = max_rarity_len,
        width_cn = max_cn_len,
        width_set = max_set_len,
    );

    // Combine all entries
    let mut output = String::new();
    output.push_str(&header);
    output.push_str(&separator);
    for (_, entry) in output_entries {
        output.push_str(&entry);
    }

    // Add total cards count and price
    let total_cards: i32 = matched_cards.iter().map(|mc| mc.quantity).sum();
    let total_price: f64 = matched_cards
        .iter()
        .map(|mc| mc.card.price_f64() * mc.quantity as f64)
        .sum();
    output.push_str(&separator);
    output.push_str(&format!("Total cards picked: {total_cards}\n"));
    output.push_str(&format!("Total price: {total_price:.2} €\n"));

    output
}

pub fn format_invoice_list(matched_cards: &[MatchedCard]) -> String {
    let mut total_price = 0.0;
    let mut output_entries = Vec::new();
    let mut max_name_len = 0;
    let mut max_lang_len = 0;
    let mut max_cond_len = 0;

    // Calculate maximum lengths for alignment
    for matched_card in matched_cards {
        let card = matched_card.card;
        max_name_len = max_name_len.max(card.name.len());
        max_lang_len = max_lang_len.max(card.language.len());
        max_cond_len = max_cond_len.max(card.condition.len());
    }

    // Create entries for each card
    for matched_card in matched_cards {
        let card = matched_card.card;
        let price = card.price_f64();
        let line_total = price * matched_card.quantity as f64;
        total_price += line_total;
        // Discount will be applied in the wrapper function

        // Get localized name based on language
        let lang = Language::parse(&card.language);
        let mut name = get_card_name(card, lang).to_string();
        let special_conditions = card.special_conditions();
        if !special_conditions.is_empty() {
            name = format!("{} ({})", name, special_conditions.join(", "));
        }

        let entry = format!(
            "{:>3} x {:<width_name$} | {:<width_lang$} | {:<width_cond$} | {:>6.2} € | {:>7.2} €\n",
            matched_card.quantity,
            name,
            card.language,
            card.condition,
            price,
            line_total,
            width_name = max_name_len,
            width_lang = max_lang_len,
            width_cond = max_cond_len,
        );
        output_entries.push(entry);
    }

    // Create header
    let header = format!(
        "{:>3} x {:<width_name$} | {:<width_lang$} | {:<width_cond$} | {:>6} | {:>7}\n",
        "Qty",
        "Name",
        "Lang",
        "Cond",
        "Price",
        "Total",
        width_name = max_name_len,
        width_lang = max_lang_len,
        width_cond = max_cond_len,
    );

    // Create separator line
    let separator = format!(
        "{:-<4}-+-{:-<width_name$}-+-{:-<width_lang$}-+-{:-<width_cond$}-+-{:-<8}-+-{:-<9}\n",
        "",
        "",
        "",
        "",
        "",
        "",
        width_name = max_name_len,
        width_lang = max_lang_len,
        width_cond = max_cond_len,
    );

    // Combine all entries
    let mut output = String::new();
    output.push_str(&header);
    output.push_str(&separator);
    for entry in output_entries {
        output.push_str(&entry);
    }
    output.push_str(&separator);
    let total_width = max_name_len + max_lang_len + max_cond_len + 7;
    output.push_str(&format!(
        "{:<total_width$} {:>7.2} €\n",
        "Total:", total_price
    ));
    output
}

pub fn format_update_stock_csv(matched_cards: &[MatchedCard]) -> String {
    use csv::WriterBuilder;

    let mut wtr = WriterBuilder::new().has_headers(true).from_writer(vec![]);

    // Write header
    let _ = wtr.write_record([
        "cardmarketId",
        "quantity",
        "name",
        "set",
        "setCode",
        "cn",
        "condition",
        "language",
        "isFoil",
        "isPlayset",
        "isSigned",
        "price",
        "comment",
        "location",
        "nameDE",
        "nameES",
        "nameFR",
        "nameIT",
        "rarity",
    ]);

    for matched_card in matched_cards {
        let card = matched_card.card;
        let quantity_str = (-matched_card.quantity).to_string();
        let _ = wtr.write_record([
            &card.cardmarket_id,
            &quantity_str,
            &card.name,
            &card.set,
            &card.set_code,
            &card.cn,
            &card.condition,
            &card.language,
            &card.is_foil,
            card.is_playset.as_deref().unwrap_or(""),
            &card.is_signed,
            &card.price,
            &card.comment,
            card.location.as_deref().unwrap_or(""),
            &card.name_de,
            &card.name_es,
            &card.name_fr,
            &card.name_it,
            &card.rarity,
        ]);
    }

    let data = wtr.into_inner().unwrap();
    String::from_utf8(data).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Card;

    // Helper to create a test card
    fn create_test_card(name: &str, price: &str, quantity: i32) -> Card {
        Card {
            cardmarket_id: "12345".to_string(),
            quantity: quantity.to_string(),
            name: name.to_string(),
            set: "Alpha".to_string(),
            set_code: "LEA".to_string(),
            cn: "123".to_string(),
            condition: "NM".to_string(),
            language: "English".to_string(),
            is_foil: "false".to_string(),
            is_playset: None,
            is_signed: "false".to_string(),
            price: price.to_string(),
            comment: "".to_string(),
            location: Some("A-0-1-1".to_string()),
            name_de: "".to_string(),
            name_es: "".to_string(),
            name_fr: "".to_string(),
            name_it: "".to_string(),
            rarity: "common".to_string(),
            listed_at: "2024-01-01".to_string(),
        }
    }

    fn create_matched_card<'a>(card: &'a Card, quantity: i32, set_name: &str) -> MatchedCard<'a> {
        MatchedCard {
            card,
            quantity,
            set_name: set_name.to_string(),
        }
    }

    // ==================== format_regular_output Tests ====================

    #[test]
    fn test_format_regular_output_empty() {
        let matches: Vec<(String, i32, Vec<MatchedCard>)> = vec![];
        let output = format_regular_output(&matches, 0.0);
        assert!(output.contains("No cards from your wantslist were found"));
    }

    #[test]
    fn test_format_regular_output_single_card() {
        let card = create_test_card("Lightning Bolt", "10.00", 4);
        let matched = create_matched_card(&card, 1, "Alpha (LEA)");

        let matches = vec![("Lightning Bolt".to_string(), 1, vec![matched])];
        let output = format_regular_output(&matches, 0.0);

        assert!(output.contains("1 x Lightning Bolt"));
        assert!(output.contains("10.00 €"));
        assert!(output.contains("Alpha (LEA)"));
        assert!(output.contains("NM condition"));
    }

    #[test]
    fn test_format_regular_output_with_discount() {
        let card = create_test_card("Lightning Bolt", "100.00", 4);
        let matched = create_matched_card(&card, 1, "Alpha (LEA)");

        let matches = vec![("Lightning Bolt".to_string(), 1, vec![matched])];
        let output = format_regular_output(&matches, 10.0);

        assert!(output.contains("90.00 €")); // 10% discount
        assert!(output.contains("10.0% discount"));
    }

    #[test]
    fn test_format_regular_output_partial_availability() {
        let card = create_test_card("Lightning Bolt", "10.00", 2);
        let matched = create_matched_card(&card, 2, "Alpha (LEA)");

        let matches = vec![("Lightning Bolt".to_string(), 4, vec![matched])];
        let output = format_regular_output(&matches, 0.0);

        assert!(output.contains("2 of 4"));
        assert!(output.contains("WARNING: Only 2 of 4 copies available"));
    }

    #[test]
    fn test_format_regular_output_foil_card() {
        let mut card = create_test_card("Lightning Bolt", "50.00", 4);
        card.is_foil = "true".to_string();
        let matched = create_matched_card(&card, 1, "Alpha (LEA)");

        let matches = vec![("Lightning Bolt".to_string(), 1, vec![matched])];
        let output = format_regular_output(&matches, 0.0);

        assert!(output.contains("(Foil)"));
    }

    #[test]
    fn test_format_regular_output_signed_card() {
        let mut card = create_test_card("Lightning Bolt", "100.00", 4);
        card.is_signed = "true".to_string();
        let matched = create_matched_card(&card, 1, "Alpha (LEA)");

        let matches = vec![("Lightning Bolt".to_string(), 1, vec![matched])];
        let output = format_regular_output(&matches, 0.0);

        assert!(output.contains("(Signed)"));
    }

    #[test]
    fn test_format_regular_output_with_comment() {
        let mut card = create_test_card("Lightning Bolt", "10.00", 4);
        card.comment = "Great condition!".to_string();
        let matched = create_matched_card(&card, 1, "Alpha (LEA)");

        let matches = vec![("Lightning Bolt".to_string(), 1, vec![matched])];
        let output = format_regular_output(&matches, 0.0);

        assert!(output.contains("Note: Great condition!"));
    }

    #[test]
    fn test_format_regular_output_with_location() {
        let card = create_test_card("Lightning Bolt", "10.00", 4);
        let matched = create_matched_card(&card, 1, "Alpha (LEA)");

        let matches = vec![("Lightning Bolt".to_string(), 1, vec![matched])];
        let output = format_regular_output(&matches, 0.0);

        assert!(output.contains("[Location: A-0-1-1]"));
    }

    #[test]
    fn test_format_regular_output_total_price() {
        let card1 = create_test_card("Lightning Bolt", "10.00", 4);
        let card2 = create_test_card("Black Lotus", "90.00", 1);
        let matched1 = create_matched_card(&card1, 2, "Alpha (LEA)");
        let matched2 = create_matched_card(&card2, 1, "Alpha (LEA)");

        let matches = vec![
            ("Lightning Bolt".to_string(), 2, vec![matched1]),
            ("Black Lotus".to_string(), 1, vec![matched2]),
        ];
        let output = format_regular_output(&matches, 0.0);

        assert!(output.contains("Total price for available cards: 110.00 €"));
        assert!(output.contains("Total cards picked: 3"));
    }

    // ==================== format_picking_list Tests ====================

    #[test]
    fn test_format_picking_list_empty() {
        let cards: Vec<MatchedCard> = vec![];
        let output = format_picking_list(&cards);

        // Should have header row
        assert!(output.contains("Qty"));
        assert!(output.contains("Location"));
    }

    #[test]
    fn test_format_picking_list_single_card() {
        let card = create_test_card("Lightning Bolt", "10.00", 4);
        let matched = create_matched_card(&card, 2, "Alpha (LEA)");

        let output = format_picking_list(&[matched]);

        assert!(output.contains("Lightning Bolt"));
        assert!(output.contains("A-0-1-1"));
        assert!(output.contains("2")); // quantity
    }

    #[test]
    fn test_format_picking_list_sorted_by_location() {
        let mut card1 = create_test_card("Card A", "10.00", 4);
        card1.location = Some("B-0-1-1".to_string());
        let mut card2 = create_test_card("Card B", "10.00", 4);
        card2.location = Some("A-0-1-1".to_string());

        let matched1 = create_matched_card(&card1, 1, "Set");
        let matched2 = create_matched_card(&card2, 1, "Set");

        let output = format_picking_list(&[matched1, matched2]);

        // A location should come before B location
        let a_pos = output.find("A-0-1-1").unwrap();
        let b_pos = output.find("B-0-1-1").unwrap();
        assert!(a_pos < b_pos);
    }

    #[test]
    fn test_format_picking_list_german_card_uses_german_name() {
        let mut card = create_test_card("Lightning Bolt", "10.00", 4);
        card.language = "German".to_string();
        card.name_de = "Blitzschlag".to_string();

        let matched = create_matched_card(&card, 1, "Alpha (LEA)");
        let output = format_picking_list(&[matched]);

        assert!(output.contains("Blitzschlag"));
    }

    // ==================== format_invoice_list Tests ====================

    #[test]
    fn test_format_invoice_list_empty() {
        let cards: Vec<MatchedCard> = vec![];
        let output = format_invoice_list(&cards);

        assert!(output.contains("Qty"));
        assert!(output.contains("Name"));
    }

    #[test]
    fn test_format_invoice_list_single_card() {
        let card = create_test_card("Lightning Bolt", "100.00", 4);
        let matched = create_matched_card(&card, 1, "Alpha (LEA)");

        let output = format_invoice_list(&[matched]);

        assert!(output.contains("Lightning Bolt"));
        assert!(output.contains("100.00"));
    }

    #[test]
    fn test_format_invoice_list_total() {
        let card = create_test_card("Lightning Bolt", "25.00", 4);
        let matched = create_matched_card(&card, 2, "Alpha (LEA)");

        let output = format_invoice_list(&[matched]);

        // 2 cards at 25.00 = 50.00 total
        assert!(output.contains("50.00"));
    }
}
