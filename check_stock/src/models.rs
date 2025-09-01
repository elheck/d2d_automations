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

#[derive(Debug)]
pub struct WantsEntry {
    pub quantity: i32,
    pub name: String,
}
