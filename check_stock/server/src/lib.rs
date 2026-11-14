//! Web backend of the MTG stock checker.
//!
//! Serves the JSON API (and from step 1c the web UI) on the home LAN. All
//! logic and data access come from [`check_stock_core`]; this crate adds the
//! HTTP layer, the server's own tables and the admin commands.
//!
//! Safety rules that hold for every route:
//! - Binds to `127.0.0.1` unless the address is configured explicitly.
//! - Only requests for an allowed host name are answered (DNS-rebinding
//!   protection, [`config::HostAllowlist`]).
//! - Error responses never contain internal details; panics become a plain
//!   500. Every response carries strict security headers.
//! - One process per data folder ([`data_dir::DataDirLock`]).

pub mod app;
pub mod cli;
pub mod config;
pub mod data_dir;
pub mod db;
pub mod http;
pub mod import;
pub mod migrations;
