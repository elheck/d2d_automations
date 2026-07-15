use serde::Deserialize;

/// Maps a CSV condition value (either short form "NM" or long form "near_mint")
/// to the canonical short form ("NM", "EX", "GD", "LP", "PL", "PO").
///
/// Cardmarket's legacy export used short codes; the inventory report format uses
/// snake_case full names. Both formats must be supported in filters and sort keys.
/// Unknown values are returned unchanged (uppercased + trimmed).
pub fn canonical_condition(s: &str) -> String {
    let trimmed = s.trim().to_lowercase();
    let trimmed = trimmed.replace([' ', '-'], "_");
    match trimmed.as_str() {
        "nm" | "mint" | "m" | "near_mint" => "NM".to_string(),
        "ex" | "excellent" => "EX".to_string(),
        "gd" | "good" => "GD".to_string(),
        "lp" | "light_played" | "lightly_played" => "LP".to_string(),
        "pl" | "played" => "PL".to_string(),
        "po" | "poor" => "PO".to_string(),
        _ => s.trim().to_uppercase(),
    }
}

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
    #[serde(rename = "isPlayset", default)]
    pub is_playset: Option<String>,
    #[serde(rename = "isSigned")]
    pub is_signed: String,
    #[serde(rename = "isFirstEd", default)]
    pub is_first_ed: Option<String>,
    #[serde(rename = "isReverseHolo", default)]
    pub is_reverse_holo: Option<String>,
    pub price: String,
    pub comment: String,
    pub location: Option<String>,
    #[serde(rename = "nameDE", default)]
    pub name_de: String,
    #[serde(rename = "nameES", default)]
    pub name_es: String,
    #[serde(rename = "nameFR", default)]
    pub name_fr: String,
    #[serde(rename = "nameIT", default)]
    pub name_it: String,
    pub rarity: String,
    #[serde(rename = "listedAt", default)]
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

    /// Returns true if this card is a first edition printing
    pub fn is_first_ed_card(&self) -> bool {
        self.is_first_ed
            .as_deref()
            .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    }

    /// Returns true if this card is reverse holographic
    pub fn is_reverse_holo_card(&self) -> bool {
        self.is_reverse_holo
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
        if self.is_first_ed_card() {
            conditions.push("1st Ed");
        }
        if self.is_reverse_holo_card() {
            conditions.push("Reverse Holo");
        }
        conditions
    }

    /// Parse the price as f64, returning 0.0 if parsing fails
    pub fn price_f64(&self) -> f64 {
        self.price.parse::<f64>().unwrap_or(0.0)
    }
}

#[cfg(test)]
impl Card {
    /// Creates a Card with sensible defaults for testing.
    /// Override individual fields as needed: `let mut c = Card::test_default(); c.name = "...".into();`
    pub fn test_default() -> Card {
        Card {
            cardmarket_id: "12345".to_string(),
            quantity: "1".to_string(),
            name: "Test Card".to_string(),
            set: "Test Set".to_string(),
            set_code: "TST".to_string(),
            cn: "1".to_string(),
            condition: "NM".to_string(),
            language: "English".to_string(),
            is_foil: "false".to_string(),
            is_playset: None,
            is_signed: "false".to_string(),
            is_first_ed: None,
            is_reverse_holo: None,
            price: "1.00".to_string(),
            comment: "".to_string(),
            location: None,
            name_de: "".to_string(),
            name_es: "".to_string(),
            name_fr: "".to_string(),
            name_it: "".to_string(),
            rarity: "common".to_string(),
            listed_at: "2024-01-01".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WantsEntry {
    pub quantity: i32,
    pub name: String,
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
