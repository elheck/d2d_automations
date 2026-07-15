//! Cardmarket price guide client — shared implementation lives in `mtg_common`.
//!
//! `PriceGuide::load`/`fetch_blocking` return `MtgError`, which converts into
//! this crate's `ApiError` via `From` at call sites.

pub use mtg_common::cardmarket::{PriceGuide, PriceGuideEntry, PriceGuideFile};
