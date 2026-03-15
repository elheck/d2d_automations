use serde::{Deserialize, Serialize};

/// Scryfall image URIs (superset of all fields used across projects).
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ImageUris {
    pub small: Option<String>,
    pub normal: Option<String>,
    pub large: Option<String>,
    pub png: Option<String>,
    pub art_crop: Option<String>,
    pub border_crop: Option<String>,
}

/// A single face of a double-faced card.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CardFace {
    pub name: String,
    #[serde(default)]
    pub image_uris: Option<ImageUris>,
    #[serde(default)]
    pub mana_cost: Option<String>,
    #[serde(default)]
    pub type_line: Option<String>,
    #[serde(default)]
    pub oracle_text: Option<String>,
}

/// Scryfall price data.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ScryfallPrices {
    pub eur: Option<String>,
    pub eur_foil: Option<String>,
    pub usd: Option<String>,
    pub usd_foil: Option<String>,
}

/// Purchase links from Scryfall.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PurchaseUris {
    pub cardmarket: Option<String>,
    pub tcgplayer: Option<String>,
}

/// Get the primary image URL (normal size) from image_uris/card_faces.
///
/// Shared logic: tries direct image_uris first, then falls back to
/// the front face of double-faced cards.
pub fn image_url<'a>(
    image_uris: Option<&'a ImageUris>,
    card_faces: Option<&'a [CardFace]>,
) -> Option<&'a str> {
    if let Some(uris) = image_uris {
        return uris.normal.as_deref();
    }
    if let Some(faces) = card_faces {
        if let Some(face) = faces.first() {
            if let Some(ref uris) = face.image_uris {
                return uris.normal.as_deref();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_url_from_direct_uris() {
        let uris = ImageUris {
            small: None,
            normal: Some("https://example.com/normal.jpg".to_string()),
            large: None,
            png: None,
            art_crop: None,
            border_crop: None,
        };
        assert_eq!(
            image_url(Some(&uris), None),
            Some("https://example.com/normal.jpg")
        );
    }

    #[test]
    fn image_url_from_card_face() {
        let faces = vec![CardFace {
            name: "Front".to_string(),
            image_uris: Some(ImageUris {
                small: None,
                normal: Some("https://example.com/front.jpg".to_string()),
                large: None,
                png: None,
                art_crop: None,
                border_crop: None,
            }),
            mana_cost: None,
            type_line: None,
            oracle_text: None,
        }];
        assert_eq!(
            image_url(None, Some(&faces)),
            Some("https://example.com/front.jpg")
        );
    }

    #[test]
    fn image_url_prefers_direct_over_faces() {
        let uris = ImageUris {
            small: None,
            normal: Some("https://example.com/direct.jpg".to_string()),
            large: None,
            png: None,
            art_crop: None,
            border_crop: None,
        };
        let faces = vec![CardFace {
            name: "Front".to_string(),
            image_uris: Some(ImageUris {
                small: None,
                normal: Some("https://example.com/face.jpg".to_string()),
                large: None,
                png: None,
                art_crop: None,
                border_crop: None,
            }),
            mana_cost: None,
            type_line: None,
            oracle_text: None,
        }];
        assert_eq!(
            image_url(Some(&uris), Some(&faces)),
            Some("https://example.com/direct.jpg")
        );
    }

    #[test]
    fn image_url_none_when_empty() {
        assert_eq!(image_url(None, None), None);
    }

    #[test]
    fn image_uris_deserializes_with_missing_fields() {
        let json = r#"{"normal": "https://example.com/img.jpg"}"#;
        let uris: ImageUris = serde_json::from_str(json).unwrap();
        assert_eq!(uris.normal.as_deref(), Some("https://example.com/img.jpg"));
        assert!(uris.small.is_none());
        assert!(uris.png.is_none());
    }
}
