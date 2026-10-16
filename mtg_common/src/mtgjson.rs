//! MTGJSON client and compact precon decklist types.
//!
//! MTGJSON publishes the official Wizards decklists of every preconstructed
//! product. Each deck card carries `identifiers.mcmId`, which is the
//! Cardmarket product id — the same id the `inventory_sync` price tracker
//! uses — so decklists can be priced without any name matching.
//!
//! Three endpoints are used:
//! - `DeckList.json` (~0.7 MB): every published deck, `(code, name) → fileName`.
//! - `SetList.json` (~12 MB): sealed products per set, linking the sealed
//!   product's `mcmId` to the deck(s) it contains.
//! - `decks/{fileName}.json` (~0.5 MB each): the full decklist.
//!
//! All responses are untrusted: bodies are size-capped while streaming,
//! parsed into minimal typed structs, and bounds-checked. `fileName` is the
//! only remote value ever used in a URL path (and, by callers, in a cache
//! filename), so it must pass [`is_safe_deck_file_name`] first.

use crate::error::{MtgError, MtgResult};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// Base URL of the MTGJSON v5 API.
pub const MTGJSON_API: &str = "https://mtgjson.com/api/v5";

/// Response size caps (bytes). Generous multiples of today's sizes; MTGJSON
/// sends no `Content-Length`, so the caps are enforced while streaming.
pub const MAX_DECK_LIST_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_SET_LIST_BYTES: usize = 96 * 1024 * 1024;
pub const MAX_DECK_BYTES: usize = 8 * 1024 * 1024;

/// Longest accepted deck `fileName`.
pub const MAX_DECK_FILE_NAME_LEN: usize = 128;

/// Sealed product subtype that marks Commander precons.
const PRECON_SUBTYPE: &str = "commander";
/// Highest plausible count for a single decklist line (basic lands).
const MAX_CARD_COUNT: u32 = 250;
/// Highest plausible number of decklist lines.
const MAX_DECK_LINES: usize = 500;
/// Highest plausible number of card copies in one deck.
const MAX_DECK_COPIES: u32 = 1000;
/// Highest plausible number of decks in one sealed product (bundles).
const MAX_DECKS_PER_PRODUCT: usize = 16;
/// How deep bundle contents (`contents.sealed`) are followed.
const MAX_SEALED_DEPTH: usize = 2;
const MAX_REDIRECTS: usize = 3;
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const SET_LIST_TIMEOUT: Duration = Duration::from_secs(120);

// ── Compact public types (also the on-disk cache format) ────────────────────

/// One entry of `DeckList.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckListEntry {
    pub code: String,
    #[serde(rename = "fileName")]
    pub file_name: String,
    pub name: String,
    #[serde(rename = "releaseDate")]
    pub release_date: Option<String>,
}

/// A deck named inside a sealed product, identified by set code + deck name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeckKey {
    pub set_code: String,
    pub name: String,
}

/// A Commander sealed product and the decks it contains.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SealedPrecon {
    pub sealed_name: String,
    pub set_code: String,
    /// Every deck in the product. Bundle children are expanded and repeated
    /// per their `count`.
    pub deck_keys: Vec<DeckKey>,
}

/// Cardmarket product id of a sealed Commander product → its decks.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SealedIndex {
    pub mtgjson_version: Option<String>,
    pub products: HashMap<u64, SealedPrecon>,
}

/// A deck key resolved to a downloadable deck file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckRef {
    pub file_name: String,
    pub name: String,
    pub code: String,
    pub release_date: Option<String>,
}

/// Outcome of resolving a sealed product's decks against `DeckList.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeckResolution {
    /// Every deck has a published, safely named deck file.
    Ready(Vec<DeckRef>),
    /// Names of decks that are not (yet) published or cannot be fetched.
    Pending(Vec<String>),
}

/// Which part of the deck a card belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeckBoard {
    Commander,
    Main,
    Side,
    /// Oversized/display commander cards — physical cards with their own
    /// product id, kept for completeness but usually excluded from EV.
    Display,
}

/// One decklist line, reduced to what pricing needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeckCard {
    pub name: String,
    pub set_code: String,
    pub number: String,
    pub count: u32,
    pub is_foil: bool,
    /// Cardmarket product id; `None` when MTGJSON has not mapped it yet.
    pub mcm_id: Option<u64>,
    pub board: DeckBoard,
}

/// A compact precon decklist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreconDeck {
    pub code: String,
    pub name: String,
    pub release_date: Option<String>,
    pub cards: Vec<DeckCard>,
}

impl PreconDeck {
    /// True when every card has a Cardmarket id (safe to cache permanently).
    pub fn is_complete(&self) -> bool {
        self.cards.iter().all(|c| c.mcm_id.is_some())
    }

    /// Total card copies outside the display board.
    pub fn total_copies(&self) -> u32 {
        self.cards
            .iter()
            .filter(|c| c.board != DeckBoard::Display)
            .map(|c| c.count)
            .sum()
    }

    /// Cardmarket ids of all cards (any board; may contain duplicates).
    pub fn card_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.cards.iter().filter_map(|c| c.mcm_id)
    }

    /// Re-checks the bounds [`parse_deck`] enforces. Used when loading a deck
    /// from a local cache, which is user-writable and therefore untrusted.
    pub fn validate(&self) -> MtgResult<()> {
        if self.cards.len() > MAX_DECK_LINES {
            return Err(api_error(format!(
                "deck has {} lines (max {MAX_DECK_LINES})",
                self.cards.len()
            )));
        }
        let mut copies: u32 = 0;
        for card in &self.cards {
            if card.count == 0 || card.count > MAX_CARD_COUNT {
                return Err(api_error(format!(
                    "card count {} out of range for {:?}",
                    card.count, card.name
                )));
            }
            if card.mcm_id == Some(0) {
                return Err(api_error(format!(
                    "invalid Cardmarket id for {:?}",
                    card.name
                )));
            }
            copies += card.count;
        }
        if copies > MAX_DECK_COPIES {
            return Err(api_error(format!(
                "deck has {copies} copies (max {MAX_DECK_COPIES})"
            )));
        }
        Ok(())
    }
}

// ── Wire types (minimal; unknown fields are skipped) ────────────────────────

#[derive(Deserialize)]
struct Envelope<T> {
    meta: Option<Meta>,
    data: T,
}

#[derive(Deserialize)]
struct Meta {
    version: Option<String>,
}

#[derive(Deserialize)]
struct SetListEntryWire {
    code: Option<String>,
    #[serde(rename = "sealedProduct")]
    sealed_product: Option<Vec<SealedProductWire>>,
}

#[derive(Deserialize)]
struct SealedProductWire {
    uuid: Option<String>,
    name: Option<String>,
    subtype: Option<String>,
    identifiers: Option<IdentifiersWire>,
    contents: Option<SealedContentsWire>,
}

#[derive(Deserialize)]
struct IdentifiersWire {
    #[serde(rename = "mcmId")]
    mcm_id: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct SealedContentsWire {
    deck: Option<Vec<DeckContentWire>>,
    sealed: Option<Vec<SealedContentWire>>,
}

#[derive(Deserialize)]
struct DeckContentWire {
    name: Option<String>,
    set: Option<String>,
}

#[derive(Deserialize)]
struct SealedContentWire {
    uuid: Option<String>,
    count: Option<u32>,
}

#[derive(Deserialize)]
struct DeckFileWire {
    code: Option<String>,
    name: Option<String>,
    #[serde(rename = "releaseDate")]
    release_date: Option<String>,
    commander: Option<Vec<DeckCardWire>>,
    #[serde(rename = "mainBoard")]
    main_board: Option<Vec<DeckCardWire>>,
    #[serde(rename = "sideBoard")]
    side_board: Option<Vec<DeckCardWire>>,
    #[serde(rename = "displayCommander")]
    display_commander: Option<Vec<DeckCardWire>>,
}

#[derive(Deserialize)]
struct DeckCardWire {
    name: Option<String>,
    count: Option<u64>,
    #[serde(rename = "isFoil")]
    is_foil: Option<bool>,
    #[serde(rename = "setCode")]
    set_code: Option<String>,
    number: Option<String>,
    identifiers: Option<IdentifiersWire>,
}

/// A sealed product from `SetList.json`, flattened with its set code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedProduct {
    pub uuid: String,
    pub name: String,
    pub set_code: String,
    pub subtype: Option<String>,
    pub mcm_id: Option<u64>,
    /// Decks the product contains directly.
    pub decks: Vec<DeckKey>,
    /// Nested sealed products as `(uuid, count)`.
    pub children: Vec<(String, u32)>,
}

// ── Pure functions ──────────────────────────────────────────────────────────

fn api_error(details: impl Into<String>) -> MtgError {
    MtgError::Api {
        code: "mtgjson".to_string(),
        details: details.into(),
    }
}

/// Whitelist for deck `fileName`s: 1..=128 ASCII letters, digits or `_`.
///
/// Rejects anything that could change a URL path or escape a cache
/// directory. Names are never sanitized — a failing name is simply refused.
pub fn is_safe_deck_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_DECK_FILE_NAME_LEN
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Builds `{base_url}/decks/{file_name}.json` after validating `file_name`.
pub fn deck_url(base_url: &str, file_name: &str) -> MtgResult<String> {
    if !is_safe_deck_file_name(file_name) {
        return Err(api_error(format!(
            "unsupported deck file name {file_name:?}"
        )));
    }
    Ok(format!(
        "{}/decks/{file_name}.json",
        base_url.trim_end_matches('/')
    ))
}

/// Parses a Cardmarket id (MTGJSON sends it as a string); rejects 0.
pub fn parse_mcm_id(raw: &str) -> Option<u64> {
    raw.trim().parse::<u64>().ok().filter(|&id| id > 0)
}

/// Accepts an id given as a JSON string or number.
fn mcm_id_from_value(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::String(s) => parse_mcm_id(s),
        serde_json::Value::Number(n) => n.as_u64().filter(|&id| id > 0),
        _ => None,
    }
}

fn identifiers_mcm_id(identifiers: &Option<IdentifiersWire>) -> Option<u64> {
    identifiers
        .as_ref()
        .and_then(|i| i.mcm_id.as_ref())
        .and_then(mcm_id_from_value)
}

/// Parses `DeckList.json` into `(mtgjson version, entries)`.
pub fn parse_deck_list(bytes: &[u8]) -> MtgResult<(Option<String>, Vec<DeckListEntry>)> {
    let envelope: Envelope<Vec<DeckListEntry>> = serde_json::from_slice(bytes)?;
    Ok((envelope.meta.and_then(|m| m.version), envelope.data))
}

/// Parses `SetList.json` into `(mtgjson version, all sealed products)`.
pub fn parse_set_list_sealed(bytes: &[u8]) -> MtgResult<(Option<String>, Vec<SealedProduct>)> {
    let envelope: Envelope<Vec<SetListEntryWire>> = serde_json::from_slice(bytes)?;
    let mut out = Vec::new();
    for set in envelope.data {
        let set_code = set.code.unwrap_or_default().to_uppercase();
        for p in set.sealed_product.unwrap_or_default() {
            let Some(uuid) = p.uuid else { continue };
            let mcm_id = identifiers_mcm_id(&p.identifiers);
            let contents = p.contents;
            let decks = contents
                .as_ref()
                .and_then(|c| c.deck.as_ref())
                .map(|decks| {
                    decks
                        .iter()
                        .filter_map(|d| {
                            let name = d.name.as_deref()?.trim();
                            let set = d.set.as_deref()?.trim();
                            (!name.is_empty() && !set.is_empty()).then(|| DeckKey {
                                set_code: set.to_uppercase(),
                                name: name.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let children = contents
                .as_ref()
                .and_then(|c| c.sealed.as_ref())
                .map(|sealed| {
                    sealed
                        .iter()
                        .filter_map(|s| Some((s.uuid.clone()?, s.count.unwrap_or(1))))
                        .collect()
                })
                .unwrap_or_default();
            out.push(SealedProduct {
                uuid,
                name: p.name.unwrap_or_default(),
                set_code: set_code.clone(),
                subtype: p.subtype,
                mcm_id,
                decks,
                children,
            });
        }
    }
    Ok((envelope.meta.and_then(|m| m.version), out))
}

/// Collects a product's decks: its own `contents.deck` if non-empty,
/// otherwise each nested product's decks repeated `count` times. `path`
/// holds the uuids currently being expanded, which breaks cycles.
fn collect_decks<'a>(
    product: &'a SealedProduct,
    by_uuid: &HashMap<&'a str, &'a SealedProduct>,
    depth: usize,
    path: &mut HashSet<&'a str>,
    out: &mut Vec<DeckKey>,
) {
    if !product.decks.is_empty() {
        out.extend(product.decks.iter().cloned());
        return;
    }
    if depth >= MAX_SEALED_DEPTH {
        return;
    }
    for (uuid, count) in &product.children {
        let Some(child) = by_uuid.get(uuid.as_str()) else {
            continue;
        };
        if !path.insert(child.uuid.as_str()) {
            continue;
        }
        let mut child_decks = Vec::new();
        collect_decks(child, by_uuid, depth + 1, path, &mut child_decks);
        path.remove(child.uuid.as_str());
        for _ in 0..(*count).min(MAX_DECKS_PER_PRODUCT as u32 + 1) {
            out.extend(child_decks.iter().cloned());
            if out.len() > MAX_DECKS_PER_PRODUCT {
                return;
            }
        }
    }
}

/// Builds the sealed-product → decks index for Commander products.
///
/// Keeps products whose subtype is `commander`, that have a Cardmarket id,
/// and that resolve to 1..=16 decks. When two products share an id, the one
/// with more decks wins (ties: first seen), so the result is deterministic.
pub fn build_sealed_index(version: Option<String>, sealed: &[SealedProduct]) -> SealedIndex {
    let by_uuid: HashMap<&str, &SealedProduct> =
        sealed.iter().map(|p| (p.uuid.as_str(), p)).collect();
    let mut products: HashMap<u64, SealedPrecon> = HashMap::new();

    for product in sealed {
        let is_precon = product
            .subtype
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case(PRECON_SUBTYPE));
        let Some(mcm_id) = product.mcm_id.filter(|_| is_precon) else {
            continue;
        };

        let mut path = HashSet::from([product.uuid.as_str()]);
        let mut deck_keys = Vec::new();
        collect_decks(product, &by_uuid, 0, &mut path, &mut deck_keys);
        if deck_keys.is_empty() || deck_keys.len() > MAX_DECKS_PER_PRODUCT {
            continue;
        }

        let replace = products
            .get(&mcm_id)
            .is_none_or(|existing| deck_keys.len() > existing.deck_keys.len());
        if replace {
            products.insert(
                mcm_id,
                SealedPrecon {
                    sealed_name: product.name.clone(),
                    set_code: product.set_code.clone(),
                    deck_keys,
                },
            );
        }
    }

    SealedIndex {
        mtgjson_version: version,
        products,
    }
}

/// Resolves every indexed product's deck keys against `DeckList.json`,
/// matching `(code, name)` case-insensitively.
pub fn resolve_decks(
    index: &SealedIndex,
    deck_list: &[DeckListEntry],
) -> HashMap<u64, DeckResolution> {
    let lookup: HashMap<(String, String), &DeckListEntry> = deck_list
        .iter()
        .map(|d| ((d.code.to_lowercase(), d.name.to_lowercase()), d))
        .collect();

    index
        .products
        .iter()
        .map(|(&id, product)| {
            let mut refs = Vec::with_capacity(product.deck_keys.len());
            let mut missing = Vec::new();
            for key in &product.deck_keys {
                let entry = lookup.get(&(key.set_code.to_lowercase(), key.name.to_lowercase()));
                match entry {
                    Some(d) if is_safe_deck_file_name(&d.file_name) => refs.push(DeckRef {
                        file_name: d.file_name.clone(),
                        name: d.name.clone(),
                        code: d.code.clone(),
                        release_date: d.release_date.clone(),
                    }),
                    Some(_) => missing.push(format!("{} (unsupported deck file name)", key.name)),
                    None => missing.push(key.name.clone()),
                }
            }
            let resolution = if missing.is_empty() {
                DeckResolution::Ready(refs)
            } else {
                DeckResolution::Pending(missing)
            };
            (id, resolution)
        })
        .collect()
}

fn push_cards(
    out: &mut Vec<DeckCard>,
    cards: Option<Vec<DeckCardWire>>,
    board: DeckBoard,
) -> MtgResult<()> {
    for card in cards.unwrap_or_default() {
        let name = card.name.unwrap_or_default().trim().to_string();
        if name.is_empty() {
            continue;
        }
        let count = card.count.unwrap_or(1);
        if count == 0 {
            continue;
        }
        if count > u64::from(MAX_CARD_COUNT) {
            return Err(api_error(format!(
                "card count {count} out of range for {name:?}"
            )));
        }
        out.push(DeckCard {
            mcm_id: identifiers_mcm_id(&card.identifiers),
            name,
            set_code: card.set_code.unwrap_or_default().to_uppercase(),
            number: card.number.unwrap_or_default(),
            count: count as u32,
            is_foil: card.is_foil.unwrap_or(false),
            board,
        });
    }
    Ok(())
}

/// Parses a `decks/{fileName}.json` body into a compact [`PreconDeck`].
///
/// Commander, main board, sideboard and display commanders are kept; tokens,
/// planes and schemes are dropped (not sold as singles).
pub fn parse_deck(bytes: &[u8]) -> MtgResult<PreconDeck> {
    let envelope: Envelope<DeckFileWire> = serde_json::from_slice(bytes)?;
    let wire = envelope.data;
    let mut cards = Vec::new();
    push_cards(&mut cards, wire.commander, DeckBoard::Commander)?;
    push_cards(&mut cards, wire.main_board, DeckBoard::Main)?;
    push_cards(&mut cards, wire.side_board, DeckBoard::Side)?;
    push_cards(&mut cards, wire.display_commander, DeckBoard::Display)?;
    let deck = PreconDeck {
        code: wire.code.unwrap_or_default().to_uppercase(),
        name: wire.name.unwrap_or_default(),
        release_date: wire.release_date,
        cards,
    };
    deck.validate()?;
    Ok(deck)
}

// ── Client ──────────────────────────────────────────────────────────────────

fn too_large(cap: usize) -> MtgError {
    api_error(format!("response exceeds {cap} bytes"))
}

/// Thin HTTP client for MTGJSON.
///
/// Async methods are primary; blocking variants (for GUI background threads)
/// are behind the `blocking` feature.
#[derive(Debug, Clone)]
pub struct MtgjsonClient {
    base_url: String,
}

impl Default for MtgjsonClient {
    fn default() -> Self {
        Self::new(MTGJSON_API)
    }
}

impl MtgjsonClient {
    /// Creates a client for the given base URL (trailing slashes are trimmed).
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// The normalized base URL this client talks to.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// HTTPS-only unless the base URL itself is plain HTTP (mock servers),
    /// so a redirect can never downgrade a production request.
    fn https_only(&self) -> bool {
        self.base_url.starts_with("https://")
    }

    // ── Async API ────────────────────────────────────────────────────────────

    async fn get_capped(&self, url: &str, timeout: Duration, cap: usize) -> MtgResult<Vec<u8>> {
        let mut response = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
            .https_only(self.https_only())
            .build()?
            .get(url)
            .header("User-Agent", crate::USER_AGENT)
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(MtgError::HttpStatus(response.status()));
        }
        if response
            .content_length()
            .is_some_and(|len| len > cap as u64)
        {
            return Err(too_large(cap));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if body.len() + chunk.len() > cap {
                return Err(too_large(cap));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }

    /// `GET /DeckList.json` — every published deck.
    pub async fn deck_list(&self) -> MtgResult<(Option<String>, Vec<DeckListEntry>)> {
        let body = self
            .get_capped(
                &self.url("/DeckList.json"),
                HTTP_TIMEOUT,
                MAX_DECK_LIST_BYTES,
            )
            .await?;
        parse_deck_list(&body)
    }

    /// `GET /SetList.json` and build the Commander sealed-product index.
    pub async fn sealed_index(&self) -> MtgResult<SealedIndex> {
        let body = self
            .get_capped(
                &self.url("/SetList.json"),
                SET_LIST_TIMEOUT,
                MAX_SET_LIST_BYTES,
            )
            .await?;
        let (version, sealed) = parse_set_list_sealed(&body)?;
        Ok(build_sealed_index(version, &sealed))
    }

    /// `GET /decks/{file_name}.json` — one decklist. Refuses unsafe names
    /// without sending a request.
    pub async fn deck(&self, file_name: &str) -> MtgResult<PreconDeck> {
        let url = deck_url(&self.base_url, file_name)?;
        let body = self.get_capped(&url, HTTP_TIMEOUT, MAX_DECK_BYTES).await?;
        parse_deck(&body)
    }

    // ── Blocking API (GUI background threads) ────────────────────────────────

    #[cfg(feature = "blocking")]
    fn get_capped_blocking(&self, url: &str, timeout: Duration, cap: usize) -> MtgResult<Vec<u8>> {
        use std::io::Read;

        let response = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
            .https_only(self.https_only())
            .build()?
            .get(url)
            .header("User-Agent", crate::USER_AGENT)
            .send()?;
        if !response.status().is_success() {
            return Err(MtgError::HttpStatus(response.status()));
        }
        if response
            .content_length()
            .is_some_and(|len| len > cap as u64)
        {
            return Err(too_large(cap));
        }
        let mut body = Vec::new();
        response.take(cap as u64 + 1).read_to_end(&mut body)?;
        if body.len() > cap {
            return Err(too_large(cap));
        }
        Ok(body)
    }

    /// Blocking variant of [`Self::deck_list`].
    #[cfg(feature = "blocking")]
    pub fn deck_list_blocking(&self) -> MtgResult<(Option<String>, Vec<DeckListEntry>)> {
        let body = self.get_capped_blocking(
            &self.url("/DeckList.json"),
            HTTP_TIMEOUT,
            MAX_DECK_LIST_BYTES,
        )?;
        parse_deck_list(&body)
    }

    /// Blocking variant of [`Self::sealed_index`].
    #[cfg(feature = "blocking")]
    pub fn sealed_index_blocking(&self) -> MtgResult<SealedIndex> {
        let body = self.get_capped_blocking(
            &self.url("/SetList.json"),
            SET_LIST_TIMEOUT,
            MAX_SET_LIST_BYTES,
        )?;
        let (version, sealed) = parse_set_list_sealed(&body)?;
        Ok(build_sealed_index(version, &sealed))
    }

    /// Blocking variant of [`Self::deck`].
    #[cfg(feature = "blocking")]
    pub fn deck_blocking(&self, file_name: &str) -> MtgResult<PreconDeck> {
        let url = deck_url(&self.base_url, file_name)?;
        let body = self.get_capped_blocking(&url, HTTP_TIMEOUT, MAX_DECK_BYTES)?;
        parse_deck(&body)
    }
}

#[cfg(test)]
#[path = "mtgjson_tests.rs"]
mod tests;
