//! # TradinationalTUI (`ttui`)
//!
//! A lightweight, cross-platform native Terminal User Interface IDE.
//!
//! The crate follows a strict layered dependency graph:
//!
//! - **Layer 0 — Foundation**: [`platform`], [`config`]
//! - **Layer 1 — Domain engines**: [`editor`], [`git`], [`search`], [`terminal`],
//!   [`lsp`], [`acp`], [`providers`]
//! - **Layer 2 — Coordination**: [`workspace`], [`explorer`], [`commands`],
//!   [`plugins`], [`ai`]
//! - **Layer 3 — Presentation & runtime**: [`ui`], [`app`]
//!
//! Domain engines never import `ratatui` or `crossterm`; only [`ui`] and
//! [`app`] are allowed to touch terminal rendering and input events.

#![forbid(unsafe_code)]
#![deny(clippy::print_stdout, clippy::print_stderr, clippy::dbg_macro)]

pub mod acp;
pub mod ai;
pub mod app;
pub mod commands;
pub mod config;
pub mod editor;
pub mod error;
pub mod explorer;
pub mod git;
pub mod lsp;
pub mod platform;
pub mod plugins;
pub mod providers;
pub mod search;
pub mod terminal;
pub mod ui;
pub mod workspace;

pub use error::{Result, TtuiError};

/// Crate version reported by `ttui --version` and the ACP `clientInfo` payload.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
