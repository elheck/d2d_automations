//! Tests for the card image loader.

use super::*;

#[test]
fn key_lowercases_the_set_code() {
    assert_eq!(CardImages::key("CMD", "123"), "cmd_123");
    assert_eq!(CardImages::key("MH2", "ABC"), "mh2_ABC");
    assert_eq!(CardImages::key("uno", "1★"), "uno_1★");
}

#[test]
fn a_new_loader_is_empty() {
    let images = CardImages::new();
    assert_eq!(images.loading_count(), 0);
    assert!(images.texture("lea", "1").is_none());
}

#[test]
fn unsafe_printings_are_never_requested() {
    let ctx = eframe::egui::Context::default();
    let mut images = CardImages::new();
    images.request(&ctx, "tst", "T 1/2");
    images.request(&ctx, "..", "1");
    images.request(&ctx, "", "");
    assert_eq!(images.loading_count(), 0);
    assert!(images.is_unavailable("tst", "T 1/2"));
    assert!(!images.is_unavailable("lea", "1"));
}

#[test]
fn decode_rejects_garbage() {
    assert!(decode(b"not an image").is_none());
    assert!(decode(&[]).is_none());
}
