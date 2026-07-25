//! `stele` — a terminal markdown viewer for Ghostty, behaving like a PDF
//! viewer: open a complete document, lay it out, scroll it, resize it.
//!
//! This crate is both the shipped binary (`src/main.rs`, thin glue over
//! real terminal I/O) and a library P6/P7 build against: [`media`] and
//! [`decor`] define the hook seams they implement, and the paint-facing
//! types they need are re-exported here.

// `deny`, not `forbid`: exactly one module opts out — `terminal::signals`,
// which must install a POSIX signal handler so a `SIGTERM`/`SIGHUP` can put
// the terminal back (DW-5.1). Every libc call is `unsafe` by definition and
// crossterm exposes no signal seam, so the choice is a scoped, commented
// `#[allow]` on ~30 lines of handler or a leaked raw-mode terminal on every
// `pkill`. `deny` keeps the rest of the crate exactly as strict as `forbid`
// did — an `unsafe` block anywhere else is still a hard error.
#![deny(unsafe_code)]

pub mod app;
pub mod cli;
pub mod decor;
pub mod loader;
pub mod media;
pub mod painter;
pub mod terminal;

pub use ast::NodeId;
pub use layout::{CellSize, Reserved, Run, Semantic, StyleId};
pub use media::{GfxMediaSink, ImageSizer};
pub use painter::{CellPos, CellRect, Color, Painter, Size, Style};
