//! Desktop (egui) front end of the MTG stock checker.
//!
//! All logic and data access live in [`check_stock_core`]; it is re-exported
//! here so `crate::repricing`, `crate::inventory_db`, … resolve as before.
//! This crate only adds the UI and the platform paths ([`paths`]).

pub use check_stock_core::*;

pub mod paths;
pub mod ui;
