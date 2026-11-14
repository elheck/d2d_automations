//! MTGJSON client and precon decklist types — shared implementation lives in
//! `mtg_common`.
//!
//! Client methods return `MtgError`, which converts into this crate's
//! `ApiError` via `From` at call sites.

pub use mtg_common::mtgjson::{
    is_safe_deck_file_name, resolve_decks, DeckBoard, DeckCard, DeckRef, DeckResolution,
    MtgjsonClient, PreconDeck, SealedIndex, SealedPrecon,
};
