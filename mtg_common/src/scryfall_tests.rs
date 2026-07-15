//! Tests for scryfall.

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
