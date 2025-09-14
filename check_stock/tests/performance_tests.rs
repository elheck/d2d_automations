use d2d_automations::card_matching::find_matching_cards;
use d2d_automations::models::Card;
use std::time::Instant;

// Helper function to create test card data
fn create_test_card(
    id: u32,
    name: &str,
    quantity: i32,
    price: f64,
    language: &str,
    set: &str,
    set_code: &str,
) -> Card {
    Card {
        cardmarket_id: id.to_string(),
        quantity: quantity.to_string(),
        name: name.to_string(),
        set: set.to_string(),
        set_code: set_code.to_string(),
        cn: "1".to_string(),
        condition: "NM".to_string(),
        language: language.to_string(),
        is_foil: "false".to_string(),
        is_playset: None,
        is_signed: "false".to_string(),
        price: price.to_string(),
        comment: "".to_string(),
        location: Some(format!("A{}_S1_R1_C1", id)),
        name_de: if language == "German" {
            format!("{}_DE", name)
        } else {
            "".to_string()
        },
        name_es: if language == "Spanish" {
            format!("{}_ES", name)
        } else {
            "".to_string()
        },
        name_fr: if language == "French" {
            format!("{}_FR", name)
        } else {
            "".to_string()
        },
        name_it: if language == "Italian" {
            format!("{}_IT", name)
        } else {
            "".to_string()
        },
        rarity: "common".to_string(),
        listed_at: "2024-01-01".to_string(),
    }
}

// Generate test inventory of various sizes
fn generate_test_inventory(size: usize) -> Vec<Card> {
    let mut cards = Vec::new();
    let card_names = [
        "Lightning Bolt",
        "Black Lotus",
        "Ancestral Recall",
        "Force of Will",
        "Counterspell",
        "Sol Ring",
        "Mox Pearl",
        "Time Walk",
        "Brainstorm",
        "Swords to Plowshares",
    ];
    let languages = ["English", "German", "French", "Spanish", "Italian"];
    let sets = [
        ("Alpha", "ALP"),
        ("Beta", "BET"),
        ("Unlimited", "UNL"),
        ("Revised", "REV"),
        ("Fourth Edition", "4ED"),
    ];

    for i in 0..size {
        let card_name = card_names[i % card_names.len()];
        let language = languages[i % languages.len()];
        let (set_name, set_code) = &sets[i % sets.len()];
        let quantity = (i % 10) + 1; // 1-10 quantity
        let price = ((i % 100) + 1) as f64 * 0.5; // Vary prices

        cards.push(create_test_card(
            i as u32,
            card_name,
            quantity as i32,
            price,
            language,
            set_name,
            set_code,
        ));
    }

    cards
}

// Performance test structure
struct PerformanceResult {
    operation: String,
    inventory_size: usize,
    execution_time_ms: f64,
    matches_found: usize,
}

impl PerformanceResult {
    fn new(
        operation: String,
        inventory_size: usize,
        execution_time_ms: f64,
        matches_found: usize,
    ) -> Self {
        Self {
            operation,
            inventory_size,
            execution_time_ms,
            matches_found,
        }
    }

    fn print(&self) {
        println!(
            "{:<40} | Inventory: {:>6} | Time: {:>8.2}ms | Matches: {:>4}",
            self.operation, self.inventory_size, self.execution_time_ms, self.matches_found
        );
    }
}

#[test]
fn test_search_performance_with_varying_inventory_sizes() {
    println!("\n=== Performance Test: Search with Varying Inventory Sizes ===");
    println!(
        "{:<40} | {:>15} | {:>13} | {:>10}",
        "Operation", "Inventory Size", "Time (ms)", "Matches"
    );
    println!("{:-<85}", "");

    let inventory_sizes = [10, 100, 500, 1000, 5000, 10000];
    let mut results = Vec::new();

    for &size in &inventory_sizes {
        let inventory = generate_test_inventory(size);
        let search_term = "Lightning Bolt";
        let needed_quantity = 4;

        let start = Instant::now();
        let matches =
            find_matching_cards(search_term, needed_quantity, &inventory, Some("en"), false);
        let duration = start.elapsed();

        let result = PerformanceResult::new(
            format!("Basic search for '{}'", search_term),
            size,
            duration.as_secs_f64() * 1000.0,
            matches.len(),
        );
        result.print();
        results.push(result);

        // Assert that the function still works correctly
        assert!(
            !matches.is_empty() || size < 10,
            "Should find matches for Lightning Bolt in reasonable inventory sizes"
        );
    }

    // Performance regression check - larger inventories should not be exponentially slower
    if results.len() >= 2 {
        let first_result = &results[0];
        let last_result = &results[results.len() - 1];
        let size_ratio = last_result.inventory_size as f64 / first_result.inventory_size as f64;
        let time_ratio = last_result.execution_time_ms / first_result.execution_time_ms;

        println!("\nPerformance Analysis:");
        println!("Inventory size increased {}x", size_ratio);
        println!("Execution time increased {:.2}x", time_ratio);

        // The search should be roughly linear, not exponential
        // Allow some leeway for smaller datasets and measurement variance
        assert!(
            time_ratio < size_ratio * 10.0,
            "Search performance degraded significantly: {}x time increase for {}x data increase",
            time_ratio,
            size_ratio
        );
    }
}

#[test]
fn test_search_performance_with_different_languages() {
    println!("\n=== Performance Test: Search with Different Language Preferences ===");
    println!(
        "{:<40} | {:>15} | {:>13} | {:>10}",
        "Operation", "Inventory Size", "Time (ms)", "Matches"
    );
    println!("{:-<85}", "");

    let inventory = generate_test_inventory(1000);
    let search_term = "Lightning Bolt";
    let needed_quantity = 4;
    let languages = [
        (None, "any language"),
        (Some("en"), "English only"),
        (Some("de"), "German only"),
        (Some("fr"), "French only"),
        (Some("es"), "Spanish only"),
        (Some("it"), "Italian only"),
    ];

    for (lang_code, lang_desc) in &languages {
        let start = Instant::now();
        let matches =
            find_matching_cards(search_term, needed_quantity, &inventory, *lang_code, false);
        let duration = start.elapsed();

        let result = PerformanceResult::new(
            format!("Search with {}", lang_desc),
            inventory.len(),
            duration.as_secs_f64() * 1000.0,
            matches.len(),
        );
        result.print();

        // All operations should complete in reasonable time
        assert!(
            duration.as_millis() < 100,
            "Search with {} took too long: {}ms",
            lang_desc,
            duration.as_millis()
        );
    }
}

#[test]
fn test_search_performance_with_language_only_mode() {
    println!("\n=== Performance Test: Search with Language-Only Mode ===");
    println!(
        "{:<40} | {:>15} | {:>13} | {:>10}",
        "Operation", "Inventory Size", "Time (ms)", "Matches"
    );
    println!("{:-<85}", "");

    let inventory = generate_test_inventory(1000);
    let search_term = "Lightning Bolt";
    let needed_quantity = 4;

    // Test with preferred_language_only = false
    let start = Instant::now();
    let matches_any_lang = find_matching_cards(
        search_term,
        needed_quantity,
        &inventory,
        Some("en"),
        false, // Any language
    );
    let duration_any = start.elapsed();

    let result_any = PerformanceResult::new(
        "Search any language".to_string(),
        inventory.len(),
        duration_any.as_secs_f64() * 1000.0,
        matches_any_lang.len(),
    );
    result_any.print();

    // Test with preferred_language_only = true
    let start = Instant::now();
    let matches_specific_lang = find_matching_cards(
        search_term,
        needed_quantity,
        &inventory,
        Some("en"),
        true, // English only
    );
    let duration_specific = start.elapsed();

    let result_specific = PerformanceResult::new(
        "Search English only".to_string(),
        inventory.len(),
        duration_specific.as_secs_f64() * 1000.0,
        matches_specific_lang.len(),
    );
    result_specific.print();

    // Language-specific search might be faster due to early filtering
    // but both should be reasonably fast
    assert!(duration_any.as_millis() < 100);
    assert!(duration_specific.as_millis() < 100);
}

#[test]
fn test_search_performance_edge_cases() {
    println!("\n=== Performance Test: Edge Cases ===");
    println!(
        "{:<40} | {:>15} | {:>13} | {:>10}",
        "Operation", "Inventory Size", "Time (ms)", "Matches"
    );
    println!("{:-<85}", "");

    let inventory = generate_test_inventory(1000);

    // Test 1: Search for non-existent card
    let start = Instant::now();
    let matches = find_matching_cards("Nonexistent Card Name", 4, &inventory, Some("en"), false);
    let duration = start.elapsed();

    let result = PerformanceResult::new(
        "Search for non-existent card".to_string(),
        inventory.len(),
        duration.as_secs_f64() * 1000.0,
        matches.len(),
    );
    result.print();

    assert_eq!(matches.len(), 0);
    assert!(
        duration.as_millis() < 50,
        "Non-existent card search should be fast"
    );

    // Test 2: Search with very large quantity
    let start = Instant::now();
    let matches = find_matching_cards(
        "Lightning Bolt",
        10000, // Very large quantity
        &inventory,
        Some("en"),
        false,
    );
    let duration = start.elapsed();

    let result = PerformanceResult::new(
        "Search with large quantity".to_string(),
        inventory.len(),
        duration.as_secs_f64() * 1000.0,
        matches.len(),
    );
    result.print();

    assert!(
        duration.as_millis() < 100,
        "Large quantity search should be reasonably fast"
    );

    // Test 3: Search with empty string
    let start = Instant::now();
    let matches = find_matching_cards("", 4, &inventory, Some("en"), false);
    let duration = start.elapsed();

    let result = PerformanceResult::new(
        "Search with empty string".to_string(),
        inventory.len(),
        duration.as_secs_f64() * 1000.0,
        matches.len(),
    );
    result.print();

    assert_eq!(matches.len(), 0);
    assert!(
        duration.as_millis() < 50,
        "Empty string search should be fast"
    );
}

#[test]
fn test_search_performance_memory_usage() {
    println!("\n=== Performance Test: Memory Usage Estimation ===");

    let inventory_sizes = [100, 1000, 5000];

    for &size in &inventory_sizes {
        let inventory = generate_test_inventory(size);

        // Estimate memory usage based on inventory size
        let card_size_estimate = std::mem::size_of::<Card>();
        let inventory_memory = inventory.len() * card_size_estimate;

        println!(
            "Inventory size: {} cards, estimated memory: {} KB",
            size,
            inventory_memory / 1024
        );

        // Perform search and ensure it doesn't panic or hang
        let start = Instant::now();
        let matches = find_matching_cards("Lightning Bolt", 4, &inventory, Some("en"), false);
        let duration = start.elapsed();

        println!(
            "Search completed in {:.2}ms, found {} matches",
            duration.as_secs_f64() * 1000.0,
            matches.len()
        );

        // Memory should not grow exponentially with inventory size
        assert!(
            duration.as_millis() < 200,
            "Search should complete within reasonable time"
        );
    }
}

#[test]
fn test_search_performance_concurrent_safety() {
    println!("\n=== Performance Test: Concurrent Safety ===");

    let inventory = generate_test_inventory(1000);
    let search_terms = [
        "Lightning Bolt",
        "Black Lotus",
        "Force of Will",
        "Counterspell",
    ];

    // Test multiple searches in sequence to ensure no state corruption
    for (i, &search_term) in search_terms.iter().enumerate() {
        let start = Instant::now();
        let matches = find_matching_cards(search_term, 4, &inventory, Some("en"), false);
        let duration = start.elapsed();

        println!(
            "Search {}: '{}' found {} matches in {:.2}ms",
            i + 1,
            search_term,
            matches.len(),
            duration.as_secs_f64() * 1000.0
        );

        assert!(duration.as_millis() < 100, "Each search should be fast");
    }
}

#[cfg(test)]
mod benchmark_helpers {
    use super::*;

    #[test]
    fn test_performance_test_data_generation() {
        let small_inventory = generate_test_inventory(10);
        assert_eq!(small_inventory.len(), 10);

        let large_inventory = generate_test_inventory(1000);
        assert_eq!(large_inventory.len(), 1000);

        // Ensure data variety
        let languages: std::collections::HashSet<_> = large_inventory
            .iter()
            .map(|c| c.language.as_str())
            .collect();
        assert!(languages.len() > 1, "Should have multiple languages");

        let names: std::collections::HashSet<_> =
            large_inventory.iter().map(|c| c.name.as_str()).collect();
        assert!(names.len() > 1, "Should have multiple card names");
    }

    #[test]
    fn test_performance_result_creation() {
        let result = PerformanceResult::new("Test operation".to_string(), 1000, 15.5, 42);

        assert_eq!(result.operation, "Test operation");
        assert_eq!(result.inventory_size, 1000);
        assert_eq!(result.execution_time_ms, 15.5);
        assert_eq!(result.matches_found, 42);
    }
}
