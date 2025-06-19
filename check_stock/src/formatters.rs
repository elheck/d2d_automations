use crate::card_matching::{MatchedCard, parse_location_code};

pub fn format_regular_output(matches: &[(String, Vec<MatchedCard>)]) -> String {
    let mut output = String::new();
    let mut total_price = 0.0;

    for (card_name, matched_cards) in matches {
        if matched_cards.is_empty() {
            continue;
        }

        let card_total_cost: f64 = matched_cards.iter()
            .map(|mc| mc.card.price.parse::<f64>().unwrap_or(0.0) * mc.quantity as f64)
            .sum();

        let total_found: i32 = matched_cards.iter().map(|mc| mc.quantity).sum();
        let needed_quantity = matched_cards.first().map(|mc| mc.quantity).unwrap_or(0);

        output.push_str(&format!("{} x {} (total: {:.2} €)\n", needed_quantity, card_name, card_total_cost));

        // Show copies from each set with their individual prices
        for matched_card in matched_cards {
            let location_info = matched_card.card.location.as_ref()
                .filter(|loc| !loc.trim().is_empty())
                .map(|loc| format!(" [Location: {}]", loc))
                .unwrap_or_default();

            // Add special conditions
            let mut special_conditions = Vec::new();
            if matched_card.card.is_foil == "1" || matched_card.card.is_foil.to_lowercase() == "true" {
                special_conditions.push("Foil");
            }
            if matched_card.card.is_signed == "1" || matched_card.card.is_signed.to_lowercase() == "true" {
                special_conditions.push("Signed");
            }
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

            output.push_str(&format!("    {} {} [{}]{} from {}, {} condition - {:.2} €{}{}\n",
                matched_card.quantity,
                if matched_card.quantity == 1 { "copy" } else { "copies" },
                matched_card.card.language,
                special_info,
                matched_card.set_name,
                matched_card.card.condition,
                matched_card.card.price.parse::<f64>().unwrap_or(0.0),
                location_info,
                comment_info
            ));
        }

        if total_found < needed_quantity {
            output.push_str(&format!("    WARNING: Only {} of {} copies available!\n", 
                total_found, needed_quantity));
        }

        output.push('\n');
        total_price += card_total_cost;
    }

    if !matches.is_empty() {
        let total_cards: i32 = matches.iter()
            .flat_map(|(_, cards)| cards.iter())
            .map(|mc| mc.quantity)
            .sum();

        output.push_str("========================\n");
        output.push_str(&format!("Total price for available cards: {:.2} €\n", total_price));
        output.push_str(&format!("Total cards picked: {}\n", total_cards));
    } else {
        output.push_str("No cards from your wantslist were found in stock.\n");
    }

    output
}

pub fn format_picking_list(matched_cards: &[MatchedCard]) -> String {
    let mut output_entries = Vec::new();
    let mut max_loc_len = 0;
    let mut max_name_len = 0;
    let mut max_lang_len = 0;
    let mut max_rarity_len = 0;
    let mut max_cn_len = 0;
    let mut max_set_len = 0;

    // Calculate maximum lengths for alignment
    for matched_card in matched_cards {
        let card = matched_card.card;
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
        let mut name = match card.language.as_str() {
            "German" | "de" => card.name_de.clone(),
            "Spanish" | "es" => card.name_es.clone(),
            "French" | "fr" => card.name_fr.clone(),
            "Italian" | "it" => card.name_it.clone(),
            _ => card.name.clone()
        };

        // If localized name is empty, fall back to English name
        if name.trim().is_empty() {
            name = card.name.clone();
        }

        // Add special conditions to name
        let mut special_conditions = Vec::new();
        if card.is_foil == "1" || card.is_foil.to_lowercase() == "true" {
            special_conditions.push("Foil");
        }
        if card.is_signed == "1" || card.is_signed.to_lowercase() == "true" {
            special_conditions.push("Signed");
        }
        if !special_conditions.is_empty() {
            name = format!("{} ({})", name, special_conditions.join(", "));
        }

        // Handle playsets
        let is_playset = card.is_playset.as_deref().map(|s| s == "1" || s.eq_ignore_ascii_case("true")).unwrap_or(false);
        if is_playset {
            name = format!("{} [Playset]", name);
        }

        // Add comment if present
        if !card.comment.trim().is_empty() {
            name = format!("{} - Note: {}", name, card.comment.trim());
        }

        let entry = format!(
            "{:<width_loc$} | {:<width_name$} | {:<width_lang$} | {:<width_rarity$} | {:<width_cn$} | {:<width_set$}\n",
            card.location.as_deref().unwrap_or(""),
            name,
            card.language,
            card.rarity,
            card.cn,
            matched_card.set_name,
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
        "{:<width_loc$} | {:<width_name$} | {:<width_lang$} | {:<width_rarity$} | {:<width_cn$} | {:<width_set$}\n",
        "Location",
        "Name",
        "Language",
        "Rarity",
        "Collector Number",
        "Set",
        width_loc = max_loc_len,
        width_name = max_name_len,
        width_lang = max_lang_len,
        width_rarity = max_rarity_len,
        width_cn = max_cn_len,
        width_set = max_set_len,
    );

    // Create separator line
    let separator = format!(
        "{:-<width_loc$}-+-{:-<width_name$}-+-{:-<width_lang$}-+-{:-<width_rarity$}-+-{:-<width_cn$}-+-{:-<width_set$}\n",
        "",
        "",
        "",
        "",
        "",
        "",
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

    // Add total cards count
    let total_cards: i32 = matched_cards.iter().map(|mc| mc.quantity).sum();
    output.push_str(&separator);
    output.push_str(&format!("Total cards picked: {}\n", total_cards));

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
        let price = card.price.parse::<f64>().unwrap_or(0.0);
        let line_total = price * matched_card.quantity as f64;
        total_price += line_total;

        // Add special conditions to name
        let mut name = card.name.to_string();
        let mut special_conditions = Vec::new();
        if card.is_foil == "1" || card.is_foil.to_lowercase() == "true" {
            special_conditions.push("Foil");
        }
        if card.is_signed == "1" || card.is_signed.to_lowercase() == "true" {
            special_conditions.push("Signed");
        }
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
    output.push_str(&format!("{:<total_width$} {:>7.2} €\n", "Total:", total_price));

    output
}

pub fn format_update_stock_csv(matched_cards: &[MatchedCard]) -> String {
    let mut output = String::new();
    
    // Header with all columns from the Card struct
    output.push_str("cardmarketId,quantity,name,set,setCode,cn,condition,language,isFoil,isPlayset,isSigned,price,comment,location,nameDE,nameES,nameFR,nameIT,rarity\n");
    
    // Add each card with negative quantity to indicate removal, preserving all other fields
    for matched_card in matched_cards {
        let card = matched_card.card;
        output.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            card.cardmarket_id,
            -matched_card.quantity, // Negative quantity for stock reduction
            card.name,
            card.set,
            card.set_code,
            card.cn,
            card.condition,
            card.language,
            card.is_foil,
            card.is_playset.as_deref().unwrap_or(""),
            card.is_signed,
            card.price,
            card.comment.replace(",", "\\,"), // Escape commas in comments
            card.location.as_deref().unwrap_or(""),
            card.name_de,
            card.name_es,
            card.name_fr,
            card.name_it,
            card.rarity
        ));
    }
    
    output
}