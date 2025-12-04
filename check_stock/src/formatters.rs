use crate::card_matching::{parse_location_code, MatchedCard};
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

        if discount_percent > 0.0 {
            output.push_str(&format!("{needed_quantity} x {card_name} (total: {discounted_card_total_cost:.2} € after {discount_percent:.1}% discount)\n"));
        } else {
            output.push_str(&format!(
                "{needed_quantity} x {card_name} (total: {card_total_cost:.2} €)\n"
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

        // Add special conditions to name
        let mut name = card.name.to_string();
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
