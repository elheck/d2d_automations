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
