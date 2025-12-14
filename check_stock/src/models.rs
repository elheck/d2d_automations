use serde::Deserialize;

/// Represents the supported card languages
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    German,
    Spanish,
    French,
    Italian,
}

impl Language {
    /// Returns the full name of the language (e.g., "English", "German")
    pub fn as_str(&self) -> &'static str {
        match self {
            Language::English => "English",
            Language::German => "German",
            Language::Spanish => "Spanish",
            Language::French => "French",
            Language::Italian => "Italian",
        }
    }

    /// Returns the ISO 639-1 language code (e.g., "en", "de")
    #[allow(dead_code)]
    pub fn code(&self) -> &'static str {
        match self {
            Language::English => "en",
            Language::German => "de",
            Language::Spanish => "es",
            Language::French => "fr",
            Language::Italian => "it",
        }
    }

    /// Parse a language code (e.g., "en", "de") into a Language
    pub fn from_code(code: &str) -> Option<Self> {
        match code.to_lowercase().as_str() {
            "en" => Some(Language::English),
            "de" => Some(Language::German),
            "es" => Some(Language::Spanish),
            "fr" => Some(Language::French),
            "it" => Some(Language::Italian),
            _ => None,
        }
    }

    /// Parse a full language name (e.g., "English", "German") into a Language
    pub fn from_full_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "english" => Some(Language::English),
            "german" => Some(Language::German),
            "spanish" => Some(Language::Spanish),
            "french" => Some(Language::French),
            "italian" => Some(Language::Italian),
            _ => None,
        }
    }

    /// Parse either a language code or full name into a Language
    pub fn parse(s: &str) -> Option<Self> {
        Self::from_code(s).or_else(|| Self::from_full_name(s))
    }

    /// Returns all supported languages
    pub fn all() -> &'static [Language] {
        &[
            Language::English,
            Language::German,
            Language::Spanish,
            Language::French,
            Language::Italian,
        ]
    }
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct Card {
    #[serde(rename = "cardmarketId")]
    pub cardmarket_id: String,
    pub quantity: String,
    pub name: String,
    pub set: String,
    #[serde(rename = "setCode")]
    pub set_code: String,
    pub cn: String,
    pub condition: String,
    pub language: String,
    #[serde(rename = "isFoil")]
    pub is_foil: String,
    #[serde(rename = "isPlayset")]
    pub is_playset: Option<String>,
    #[serde(rename = "isSigned")]
    pub is_signed: String,
    pub price: String,
    pub comment: String,
    pub location: Option<String>,
    #[serde(rename = "nameDE")]
    pub name_de: String,
    #[serde(rename = "nameES")]
    pub name_es: String,
    #[serde(rename = "nameFR")]
    pub name_fr: String,
    #[serde(rename = "nameIT")]
    pub name_it: String,
    pub rarity: String,
    #[serde(rename = "listedAt")]
    pub listed_at: String,
}

impl Card {
    /// Returns true if this card is foil
    pub fn is_foil_card(&self) -> bool {
        self.is_foil == "1" || self.is_foil.eq_ignore_ascii_case("true")
    }

    /// Returns true if this card is signed
    pub fn is_signed_card(&self) -> bool {
        self.is_signed == "1" || self.is_signed.eq_ignore_ascii_case("true")
    }

    /// Returns true if this is a playset (4 cards)
    pub fn is_playset_card(&self) -> bool {
        self.is_playset
            .as_deref()
            .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    }

    /// Returns a list of special conditions for this card (e.g., "Foil", "Signed")
    pub fn special_conditions(&self) -> Vec<&'static str> {
        let mut conditions = Vec::new();
        if self.is_foil_card() {
            conditions.push("Foil");
        }
        if self.is_signed_card() {
            conditions.push("Signed");
        }
        conditions
    }

    /// Parse the price as f64, returning 0.0 if parsing fails
    pub fn price_f64(&self) -> f64 {
        self.price.parse::<f64>().unwrap_or(0.0)
    }
}

#[derive(Debug)]
pub struct WantsEntry {
    pub quantity: i32,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a test card with default values
    fn create_test_card() -> Card {
        Card {
            cardmarket_id: "12345".to_string(),
            quantity: "4".to_string(),
            name: "Lightning Bolt".to_string(),
            set: "Alpha".to_string(),
            set_code: "LEA".to_string(),
            cn: "123".to_string(),
            condition: "NM".to_string(),
            language: "English".to_string(),
            is_foil: "false".to_string(),
            is_playset: None,
            is_signed: "false".to_string(),
            price: "25.50".to_string(),
            comment: "".to_string(),
            location: Some("A-0-1-1".to_string()),
            name_de: "Blitzschlag".to_string(),
            name_es: "Rayo".to_string(),
            name_fr: "Éclair".to_string(),
            name_it: "Fulmine".to_string(),
            rarity: "common".to_string(),
            listed_at: "2024-01-01".to_string(),
        }
    }

    // ==================== Language Tests ====================

    #[test]
    fn test_language_as_str() {
        assert_eq!(Language::English.as_str(), "English");
        assert_eq!(Language::German.as_str(), "German");
        assert_eq!(Language::Spanish.as_str(), "Spanish");
        assert_eq!(Language::French.as_str(), "French");
        assert_eq!(Language::Italian.as_str(), "Italian");
    }

    #[test]
    fn test_language_code() {
        assert_eq!(Language::English.code(), "en");
        assert_eq!(Language::German.code(), "de");
        assert_eq!(Language::Spanish.code(), "es");
        assert_eq!(Language::French.code(), "fr");
        assert_eq!(Language::Italian.code(), "it");
    }

    #[test]
    fn test_language_from_code_valid() {
        assert_eq!(Language::from_code("en"), Some(Language::English));
        assert_eq!(Language::from_code("de"), Some(Language::German));
        assert_eq!(Language::from_code("es"), Some(Language::Spanish));
        assert_eq!(Language::from_code("fr"), Some(Language::French));
        assert_eq!(Language::from_code("it"), Some(Language::Italian));
    }

    #[test]
    fn test_language_from_code_case_insensitive() {
        assert_eq!(Language::from_code("EN"), Some(Language::English));
        assert_eq!(Language::from_code("De"), Some(Language::German));
        assert_eq!(Language::from_code("ES"), Some(Language::Spanish));
    }

    #[test]
    fn test_language_from_code_invalid() {
        assert_eq!(Language::from_code("xx"), None);
        assert_eq!(Language::from_code(""), None);
        assert_eq!(Language::from_code("english"), None); // full name, not code
    }

    #[test]
    fn test_language_from_full_name_valid() {
        assert_eq!(Language::from_full_name("English"), Some(Language::English));
        assert_eq!(Language::from_full_name("German"), Some(Language::German));
        assert_eq!(Language::from_full_name("Spanish"), Some(Language::Spanish));
        assert_eq!(Language::from_full_name("French"), Some(Language::French));
        assert_eq!(Language::from_full_name("Italian"), Some(Language::Italian));
    }

    #[test]
    fn test_language_from_full_name_case_insensitive() {
        assert_eq!(Language::from_full_name("ENGLISH"), Some(Language::English));
        assert_eq!(Language::from_full_name("german"), Some(Language::German));
        assert_eq!(Language::from_full_name("SpAnIsH"), Some(Language::Spanish));
    }

    #[test]
    fn test_language_from_full_name_invalid() {
        assert_eq!(Language::from_full_name("en"), None); // code, not full name
        assert_eq!(Language::from_full_name(""), None);
        assert_eq!(Language::from_full_name("Japanese"), None);
    }

    #[test]
    fn test_language_parse_accepts_both_code_and_name() {
        // Codes
        assert_eq!(Language::parse("en"), Some(Language::English));
        assert_eq!(Language::parse("de"), Some(Language::German));
        // Full names
        assert_eq!(Language::parse("English"), Some(Language::English));
        assert_eq!(Language::parse("German"), Some(Language::German));
        // Invalid
        assert_eq!(Language::parse("xx"), None);
        assert_eq!(Language::parse(""), None);
    }

    #[test]
    fn test_language_all() {
        let all = Language::all();
        assert_eq!(all.len(), 5);
        assert!(all.contains(&Language::English));
        assert!(all.contains(&Language::German));
        assert!(all.contains(&Language::Spanish));
        assert!(all.contains(&Language::French));
        assert!(all.contains(&Language::Italian));
    }

    // ==================== Card Tests ====================

    #[test]
    fn test_card_is_foil_false() {
        let card = create_test_card();
        assert!(!card.is_foil_card());
    }

    #[test]
    fn test_card_is_foil_true_with_1() {
        let mut card = create_test_card();
        card.is_foil = "1".to_string();
        assert!(card.is_foil_card());
    }

    #[test]
    fn test_card_is_foil_true_with_true() {
        let mut card = create_test_card();
        card.is_foil = "true".to_string();
        assert!(card.is_foil_card());
    }

    #[test]
    fn test_card_is_foil_true_case_insensitive() {
        let mut card = create_test_card();
        card.is_foil = "TRUE".to_string();
        assert!(card.is_foil_card());
    }

    #[test]
    fn test_card_is_signed_false() {
        let card = create_test_card();
        assert!(!card.is_signed_card());
    }

    #[test]
    fn test_card_is_signed_true() {
        let mut card = create_test_card();
        card.is_signed = "1".to_string();
        assert!(card.is_signed_card());
    }

    #[test]
    fn test_card_is_playset_none() {
        let card = create_test_card();
        assert!(!card.is_playset_card());
    }

    #[test]
    fn test_card_is_playset_false() {
        let mut card = create_test_card();
        card.is_playset = Some("false".to_string());
        assert!(!card.is_playset_card());
    }

    #[test]
    fn test_card_is_playset_true() {
        let mut card = create_test_card();
        card.is_playset = Some("1".to_string());
        assert!(card.is_playset_card());
    }

    #[test]
    fn test_card_special_conditions_none() {
        let card = create_test_card();
        assert!(card.special_conditions().is_empty());
    }

    #[test]
    fn test_card_special_conditions_foil_only() {
        let mut card = create_test_card();
        card.is_foil = "true".to_string();
        let conditions = card.special_conditions();
        assert_eq!(conditions, vec!["Foil"]);
    }

    #[test]
    fn test_card_special_conditions_signed_only() {
        let mut card = create_test_card();
        card.is_signed = "true".to_string();
        let conditions = card.special_conditions();
        assert_eq!(conditions, vec!["Signed"]);
    }

    #[test]
    fn test_card_special_conditions_both() {
        let mut card = create_test_card();
        card.is_foil = "true".to_string();
        card.is_signed = "true".to_string();
        let conditions = card.special_conditions();
        assert_eq!(conditions, vec!["Foil", "Signed"]);
    }

    #[test]
    fn test_card_price_f64_valid() {
        let card = create_test_card();
        assert!((card.price_f64() - 25.50).abs() < 0.001);
    }

    #[test]
    fn test_card_price_f64_integer() {
        let mut card = create_test_card();
        card.price = "100".to_string();
        assert!((card.price_f64() - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_card_price_f64_invalid() {
        let mut card = create_test_card();
        card.price = "not_a_number".to_string();
        assert_eq!(card.price_f64(), 0.0);
    }

    #[test]
    fn test_card_price_f64_empty() {
        let mut card = create_test_card();
        card.price = "".to_string();
        assert_eq!(card.price_f64(), 0.0);
    }
}
