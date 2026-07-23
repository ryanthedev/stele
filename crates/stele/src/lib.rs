//! `stele` — a terminal markdown viewer for Ghostty, behaving like a PDF
//! viewer: open a complete document, lay it out, scroll it, resize it.
//!
//! This crate is both the shipped binary (`src/main.rs`, thin glue over
//! real terminal I/O) and a library P6/P7 build against: [`media`] and
//! [`decor`] define the hook seams they implement, and the paint-facing
//! types they need are re-exported here.

#![forbid(unsafe_code)]

pub mod app;
pub mod cli;
pub mod decor;
pub mod loader;
pub mod media;
pub mod painter;
pub mod terminal;

pub use ast::NodeId;
pub use layout::{CellSize, Reserved, Run, Semantic, StyleId};
pub use painter::{CellPos, CellRect, Color, Painter, Size, Style};
