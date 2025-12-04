use serde::Deserialize;

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
