use d2d_automations::io::{read_csv, read_wantslist};
use d2d_automations::models::Card;
use std::io::Write;
use tempfile::NamedTempFile;

// Test fixtures - sample data for testing

fn create_sample_csv_content() -> String {
    r#"cardmarketId,quantity,name,set,setCode,cn,condition,language,isFoil,isPlayset,isSigned,price,comment,location,nameDE,nameES,nameFR,nameIT,rarity,listedAt
12345,4,Lightning Bolt,Limited Edition Alpha,LEA,123,NM,EN,false,,false,25.00,,A1_S1_R1_C1,Blitzschlag,Rayo,Éclair,Fulmine,common,2024-01-01
67890,1,Black Lotus,Limited Edition Alpha,LEA,456,NM,EN,false,,false,15000.00,,A1_S1_R1_C2,Schwarzer Lotus,Loto Negro,Lotus noir,Loto Nero,rare,2024-01-01
11111,2,Ancestral Recall,Limited Edition Alpha,LEA,789,EX,EN,false,,false,3000.00,,A2_S1_R1_C1,,,,,rare,2024-01-01
22222,,Force of Will,Alliances,ALL,321,NM,EN,false,,false,100.00,,,,,,,rare,2024-01-01
33333,3,Counterspell,Alpha,ALP,654,,EN,false,,false,,Nice card,A3_S1_R1_C1,,,,,common,2024-01-01"#.to_string()
}

fn create_invalid_csv_content() -> String {
    r#"invalid,csv,format
missing,required,fields
not,enough,columns"#
        .to_string()
}

fn create_sample_wantslist_content() -> String {
    r#"4 Lightning Bolt
1 Black Lotus
2 Force of Will
3 Counterspell

Deck
1 Sol Ring"#
        .to_string()
}

fn create_invalid_wantslist_content() -> String {
    r#"invalid_format_line
not_a_number Force of Will
abc Lightning Bolt
just_one_word"#
        .to_string()
}

// Tests for read_csv function

#[test]
fn test_read_csv_valid_file() {
    let mut temp_file = NamedTempFile::new().unwrap();
    write!(temp_file, "{}", create_sample_csv_content()).unwrap();

    let cards = read_csv(temp_file.path().to_str().unwrap()).unwrap();

    // Should have 3 cards (excluding empty quantity and empty price ones)
    assert_eq!(cards.len(), 3);

    // Test first card
    assert_eq!(cards[0].cardmarket_id, "12345");
    assert_eq!(cards[0].quantity, "4");
    assert_eq!(cards[0].name, "Lightning Bolt");
    assert_eq!(cards[0].set, "Limited Edition Alpha");
    assert_eq!(cards[0].set_code, "LEA");
    assert_eq!(cards[0].cn, "123");
    assert_eq!(cards[0].condition, "NM");
    assert_eq!(cards[0].language, "EN");
    assert_eq!(cards[0].is_foil, "false");
    assert_eq!(cards[0].is_playset, None);
    assert_eq!(cards[0].is_signed, "false");
    assert_eq!(cards[0].price, "25.00");
    assert_eq!(cards[0].comment, "");
    assert_eq!(cards[0].location, Some("A1_S1_R1_C1".to_string()));
    assert_eq!(cards[0].name_de, "Blitzschlag");
    assert_eq!(cards[0].rarity, "common");
    assert_eq!(cards[0].listed_at, "2024-01-01");
}

#[test]
fn test_read_csv_filters_empty_price_and_quantity() {
    let mut temp_file = NamedTempFile::new().unwrap();
    write!(temp_file, "{}", create_sample_csv_content()).unwrap();

    let cards = read_csv(temp_file.path().to_str().unwrap()).unwrap();

    // Should filter out cards with empty quantity (Force of Will) and empty price (Counterspell)
    assert_eq!(cards.len(), 3);

    // Verify the filtered cards are the ones with both price and quantity
    let card_names: Vec<&str> = cards.iter().map(|c| c.name.as_str()).collect();
    assert!(card_names.contains(&"Lightning Bolt"));
    assert!(card_names.contains(&"Black Lotus"));
    assert!(card_names.contains(&"Ancestral Recall"));
}

#[test]
fn test_read_csv_nonexistent_file() {
    let result = read_csv("/this/file/does/not/exist.csv");
    assert!(result.is_err());
}

#[test]
fn test_read_csv_invalid_csv_format() {
    let mut temp_file = NamedTempFile::new().unwrap();
    write!(temp_file, "{}", create_invalid_csv_content()).unwrap();

    let result = read_csv(temp_file.path().to_str().unwrap());
    assert!(result.is_err());
}

#[test]
fn test_read_csv_empty_file() {
    let temp_file = NamedTempFile::new().unwrap();
    // File is empty, no content written

    let cards = read_csv(temp_file.path().to_str().unwrap()).unwrap();
    assert_eq!(cards.len(), 0);
}

#[test]
fn test_read_csv_only_headers() {
    let mut temp_file = NamedTempFile::new().unwrap();
    write!(temp_file, "cardmarketId,quantity,name,set,setCode,cn,condition,language,isFoil,isPlayset,isSigned,price,comment,location,nameDE,nameES,nameFR,nameIT,rarity,listedAt").unwrap();

    let cards = read_csv(temp_file.path().to_str().unwrap()).unwrap();
    assert_eq!(cards.len(), 0);
}

#[test]
fn test_read_csv_with_whitespace() {
    let mut temp_file = NamedTempFile::new().unwrap();
    let content = r#"cardmarketId,quantity,name,set,setCode,cn,condition,language,isFoil,isPlayset,isSigned,price,comment,location,nameDE,nameES,nameFR,nameIT,rarity,listedAt
  12345  ,  4  ,  Lightning Bolt  ,  LEA  ,LEA,123,NM,EN,false,,false,  25.00  ,,A1_S1_R1_C1,,,,,common,2024-01-01"#;
    write!(temp_file, "{}", content).unwrap();

    let cards = read_csv(temp_file.path().to_str().unwrap()).unwrap();
    assert_eq!(cards.len(), 1);

    // CSV reader should trim whitespace
    assert_eq!(cards[0].cardmarket_id, "12345");
    assert_eq!(cards[0].quantity, "4");
    assert_eq!(cards[0].name, "Lightning Bolt");
    assert_eq!(cards[0].price, "25.00");
}

// Tests for read_wantslist function

#[test]
fn test_read_wantslist_valid_file() {
    let mut temp_file = NamedTempFile::new().unwrap();
    write!(temp_file, "{}", create_sample_wantslist_content()).unwrap();

    let wants = read_wantslist(temp_file.path().to_str().unwrap()).unwrap();

    assert_eq!(wants.len(), 5);

    assert_eq!(wants[0].quantity, 4);
    assert_eq!(wants[0].name, "Lightning Bolt");

    assert_eq!(wants[1].quantity, 1);
    assert_eq!(wants[1].name, "Black Lotus");

    assert_eq!(wants[2].quantity, 2);
    assert_eq!(wants[2].name, "Force of Will");

    assert_eq!(wants[3].quantity, 3);
    assert_eq!(wants[3].name, "Counterspell");

    // Should also parse the line after "Deck"
    assert_eq!(wants[4].quantity, 1);
    assert_eq!(wants[4].name, "Sol Ring");
}

#[test]
fn test_read_wantslist_nonexistent_file() {
    let result = read_wantslist("/this/file/does/not/exist.txt");
    assert!(result.is_err());
}

#[test]
fn test_read_wantslist_empty_file() {
    let temp_file = NamedTempFile::new().unwrap();
    // Empty file

    let wants = read_wantslist(temp_file.path().to_str().unwrap()).unwrap();
    assert_eq!(wants.len(), 0);
}

#[test]
fn test_read_wantslist_with_empty_lines() {
    let mut temp_file = NamedTempFile::new().unwrap();
    let content = r#"4 Lightning Bolt

2 Force of Will


1 Black Lotus

"#;
    write!(temp_file, "{}", content).unwrap();

    let wants = read_wantslist(temp_file.path().to_str().unwrap()).unwrap();
    assert_eq!(wants.len(), 3);

    assert_eq!(wants[0].name, "Lightning Bolt");
    assert_eq!(wants[1].name, "Force of Will");
    assert_eq!(wants[2].name, "Black Lotus");
}

#[test]
fn test_read_wantslist_skips_deck_line() {
    let mut temp_file = NamedTempFile::new().unwrap();
    let content = r#"4 Lightning Bolt
Deck
2 Force of Will"#;
    write!(temp_file, "{}", content).unwrap();

    let wants = read_wantslist(temp_file.path().to_str().unwrap()).unwrap();
    assert_eq!(wants.len(), 2);

    assert_eq!(wants[0].name, "Lightning Bolt");
    assert_eq!(wants[1].name, "Force of Will");
}

#[test]
fn test_read_wantslist_ignores_invalid_lines() {
    let mut temp_file = NamedTempFile::new().unwrap();
    write!(temp_file, "{}", create_invalid_wantslist_content()).unwrap();

    let wants = read_wantslist(temp_file.path().to_str().unwrap()).unwrap();
    // All lines are invalid format, so should be empty
    assert_eq!(wants.len(), 0);
}

#[test]
fn test_read_wantslist_mixed_valid_invalid() {
    let mut temp_file = NamedTempFile::new().unwrap();
    let content = r#"4 Lightning Bolt
invalid_line
2 Force of Will
not_a_number Card Name
1 Black Lotus"#;
    write!(temp_file, "{}", content).unwrap();

    let wants = read_wantslist(temp_file.path().to_str().unwrap()).unwrap();
    assert_eq!(wants.len(), 3); // Only valid lines

    assert_eq!(wants[0].name, "Lightning Bolt");
    assert_eq!(wants[1].name, "Force of Will");
    assert_eq!(wants[2].name, "Black Lotus");
}

#[test]
fn test_read_wantslist_with_whitespace() {
    let mut temp_file = NamedTempFile::new().unwrap();
    let content = r#"  4   Lightning Bolt  
2 Force of Will   
   1    Black Lotus"#;
    write!(temp_file, "{}", content).unwrap();

    let wants = read_wantslist(temp_file.path().to_str().unwrap()).unwrap();
    assert_eq!(wants.len(), 3);

    assert_eq!(wants[0].quantity, 4);
    assert_eq!(wants[0].name, "  Lightning Bolt"); // Leading spaces preserved in name
    assert_eq!(wants[1].quantity, 2);
    assert_eq!(wants[1].name, "Force of Will");
    assert_eq!(wants[2].quantity, 1);
    assert_eq!(wants[2].name, "   Black Lotus"); // Leading spaces preserved in name
}

#[test]
fn test_read_wantslist_card_names_with_spaces() {
    let mut temp_file = NamedTempFile::new().unwrap();
    let content = r#"1 Lightning Bolt
2 Black Lotus Petal
3 Time Walk Through The Planes"#;
    write!(temp_file, "{}", content).unwrap();

    let wants = read_wantslist(temp_file.path().to_str().unwrap()).unwrap();
    assert_eq!(wants.len(), 3);

    assert_eq!(wants[0].name, "Lightning Bolt");
    assert_eq!(wants[1].name, "Black Lotus Petal");
    assert_eq!(wants[2].name, "Time Walk Through The Planes");
}

#[test]
fn test_read_wantslist_large_quantities() {
    let mut temp_file = NamedTempFile::new().unwrap();
    let content = r#"999 Basic Land
1000000 Common Card
1 Expensive Card"#;
    write!(temp_file, "{}", content).unwrap();

    let wants = read_wantslist(temp_file.path().to_str().unwrap()).unwrap();
    assert_eq!(wants.len(), 3);

    assert_eq!(wants[0].quantity, 999);
    assert_eq!(wants[1].quantity, 1000000);
    assert_eq!(wants[2].quantity, 1);
}

#[test]
fn test_read_wantslist_zero_quantity() {
    let mut temp_file = NamedTempFile::new().unwrap();
    let content = r#"0 Zero Quantity Card
4 Normal Card"#;
    write!(temp_file, "{}", content).unwrap();

    let wants = read_wantslist(temp_file.path().to_str().unwrap()).unwrap();
    assert_eq!(wants.len(), 2);

    assert_eq!(wants[0].quantity, 0);
    assert_eq!(wants[0].name, "Zero Quantity Card");
    assert_eq!(wants[1].quantity, 4);
    assert_eq!(wants[1].name, "Normal Card");
}

// Integration tests

#[test]
fn test_csv_and_wantslist_integration() {
    // Create CSV file
    let mut csv_file = NamedTempFile::new().unwrap();
    write!(csv_file, "{}", create_sample_csv_content()).unwrap();

    // Create wantslist file
    let mut wants_file = NamedTempFile::new().unwrap();
    write!(wants_file, "{}", create_sample_wantslist_content()).unwrap();

    // Read both files
    let cards = read_csv(csv_file.path().to_str().unwrap()).unwrap();
    let wants = read_wantslist(wants_file.path().to_str().unwrap()).unwrap();

    assert!(!cards.is_empty());
    assert!(!wants.is_empty());

    // Test that we can find matching cards
    let wanted_card = &wants[0]; // Lightning Bolt
    let matching_cards: Vec<&Card> = cards
        .iter()
        .filter(|card| card.name == wanted_card.name)
        .collect();

    assert_eq!(matching_cards.len(), 1);
    assert_eq!(matching_cards[0].name, "Lightning Bolt");
}

#[cfg(test)]
mod edge_cases {
    use super::*;

    #[test]
    fn test_read_csv_with_special_characters() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let content = r#"cardmarketId,quantity,name,set,setCode,cn,condition,language,isFoil,isPlayset,isSigned,price,comment,location,nameDE,nameES,nameFR,nameIT,rarity,listedAt
12345,1,"Card with ""quotes""",Set,SET,123,NM,EN,false,,false,10.00,"Comment with, comma",A1_S1_R1_C1,,,,,common,2024-01-01"#;
        write!(temp_file, "{}", content).unwrap();

        let cards = read_csv(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].name, r#"Card with "quotes""#);
        assert_eq!(cards[0].comment, "Comment with, comma");
    }

    #[test]
    fn test_read_wantslist_with_negative_quantity() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let content = "-1 Negative Card\n4 Normal Card";
        write!(temp_file, "{}", content).unwrap();

        let wants = read_wantslist(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(wants.len(), 2);
        assert_eq!(wants[0].quantity, -1); // Negative quantities are parsed
        assert_eq!(wants[1].quantity, 4);
    }

    #[test]
    fn test_read_csv_with_unicode_characters() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let content = r#"cardmarketId,quantity,name,set,setCode,cn,condition,language,isFoil,isPlayset,isSigned,price,comment,location,nameDE,nameES,nameFR,nameIT,rarity,listedAt
12345,1,Überkarte,Sét Spéciał,ŞET,123,NM,EN,false,,false,10.00,Çomment with ñ,A1_S1_R1_C1,,,,,common,2024-01-01"#;
        write!(temp_file, "{}", content).unwrap();

        let cards = read_csv(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].name, "Überkarte");
        assert_eq!(cards[0].set, "Sét Spéciał");
    }
}
