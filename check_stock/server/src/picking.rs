//! Picking lists (4b): the cards of an order, sorted by location, ticked as
//! they are taken from the shelves (on the phone while the list was made on
//! the PC). One list per user (`srv_picking_sessions`); it stores which
//! card, from where and how many, with the names and prices to show. It
//! never changes the stock.

use crate::migrations::format_utc;
use crate::stock::Source;
use check_stock_core::card_matching::parse_location_code;
use check_stock_core::models::Card;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// One card of a picking list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PickItem {
    pub pile: String,
    pub quantity: i64,
    pub picked: bool,
    pub name: String,
    pub set_name: String,
    pub set_code: String,
    pub collector_number: String,
    pub condition: String,
    pub language: String,
    pub foil: bool,
    pub location: String,
    pub price: f64,
}

impl PickItem {
    pub fn of(pile: &str, card: &Card, quantity: i64) -> Self {
        Self {
            pile: pile.to_string(),
            quantity,
            picked: false,
            name: card.name.clone(),
            set_name: card.set.clone(),
            set_code: card.set_code.clone(),
            collector_number: card.cn.clone(),
            condition: card.condition.clone(),
            language: card.language.clone(),
            foil: card.is_foil_card(),
            location: card.location.clone().unwrap_or_default(),
            price: card.price_f64(),
        }
    }
}

/// A user's picking list.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub created_at: String,
    /// The stock it was made on.
    pub version: i64,
    pub discard_seq: i64,
    pub items: Vec<PickItem>,
}

/// Merges piles listed twice and sorts by location (shelf order).
pub fn prepare(mut items: Vec<PickItem>) -> Vec<PickItem> {
    let mut merged: Vec<PickItem> = Vec::new();
    for item in items.drain(..) {
        match merged.iter_mut().find(|m| m.pile == item.pile) {
            Some(m) => m.quantity += item.quantity,
            None => merged.push(item),
        }
    }
    merged.sort_by_cached_key(|i| {
        (
            parse_location_code(&i.location),
            i.location.clone(),
            i.name.clone(),
        )
    });
    merged
}

/// Saves a new picking list for `user_id` (replacing the old one).
pub fn create(
    conn: &Connection,
    user_id: i64,
    source: &Source,
    items: Vec<PickItem>,
    now: DateTime<Utc>,
) -> rusqlite::Result<Session> {
    let items = prepare(items);
    let json = serde_json::to_string(&items).expect("items serialize");
    conn.execute(
        "INSERT INTO srv_picking_sessions (user_id, created_at, version_id, discard_seq, items)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (user_id) DO UPDATE SET created_at = excluded.created_at,
             version_id = excluded.version_id, discard_seq = excluded.discard_seq,
             items = excluded.items",
        params![
            user_id,
            format_utc(now),
            source.version,
            source.discard_seq,
            json
        ],
    )?;
    Ok(Session {
        created_at: format_utc(now),
        version: source.version,
        discard_seq: source.discard_seq,
        items,
    })
}

pub fn load(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<Session>> {
    let row: Option<(String, i64, i64, String)> = conn
        .query_row(
            "SELECT created_at, version_id, discard_seq, items FROM srv_picking_sessions
             WHERE user_id = ?1",
            [user_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    Ok(
        row.map(|(created_at, version, discard_seq, items)| Session {
            created_at,
            version,
            discard_seq,
            items: serde_json::from_str(&items).unwrap_or_default(),
        }),
    )
}

/// Marks `piles` as picked (or not). `None` when there is no list or a pile
/// is not in it (nothing changed); otherwise how many are picked now.
pub fn set_picked(
    conn: &Connection,
    user_id: i64,
    piles: &[String],
    picked: bool,
) -> rusqlite::Result<Option<usize>> {
    let Some(mut session) = load(conn, user_id)? else {
        return Ok(None);
    };
    if piles
        .iter()
        .any(|p| !session.items.iter().any(|i| &i.pile == p))
    {
        return Ok(None);
    }
    for item in &mut session.items {
        if piles.contains(&item.pile) {
            item.picked = picked;
        }
    }
    conn.execute(
        "UPDATE srv_picking_sessions SET items = ?2 WHERE user_id = ?1",
        params![
            user_id,
            serde_json::to_string(&session.items).expect("items serialize")
        ],
    )?;
    Ok(Some(session.items.iter().filter(|i| i.picked).count()))
}

pub fn discard(conn: &Connection, user_id: i64) -> rusqlite::Result<bool> {
    Ok(conn.execute(
        "DELETE FROM srv_picking_sessions WHERE user_id = ?1",
        [user_id],
    )? > 0)
}

#[cfg(test)]
#[path = "picking_tests.rs"]
mod tests;
