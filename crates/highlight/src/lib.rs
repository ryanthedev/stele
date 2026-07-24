//! `crates/highlight` — stele's owned syntax-highlight theme layer (P7):
//! per-line highlighting via lumis (`docs/spikes/highlight-engine.md`),
//! built-in dark/light themes with truecolor + 256-color downsample, and
//! the sanitized OSC 8 emission this phase's hyperlink/footnote-anchor
//! scope needs.
//!
//! **What this crate produces, and what it deliberately does not wire
//! end-to-end** (see the phase discovery doc for the full reasoning):
//! [`Theme::resolve`] and [`highlight_line`] are complete, independently
//! tested building blocks that satisfy this crate's own contract. Whether
//! the shipped `stele` binary's *current* paint path exercises every one of
//! their capabilities is a separate, already-fixed question this crate
//! cannot change (P4's `Run`/`Line` types and P5's `Painter` call site are
//! out of this phase's file scope) — notably, `Painter::paint_runs`
//! currently calls `Decor::highlight(text, None)` unconditionally (no fence
//! language ever reaches it, because `crates/layout` discards
//! `BlockKind::CodeBlock`'s `info` string before constructing a `Run`), and
//! no `Run`/`Style` carries a link URL or footnote label. This crate's
//! `highlight_line`/`Theme`/hyperlink/footnote functions are built and
//! tested to be correct *when given real inputs* (a real `lang`, a real
//! URL, a real label) — ready for a small follow-up seam change in
//! `crates/layout`/`crates/stele/src/painter.rs` to actually supply them.
#![forbid(unsafe_code)]

mod color;
mod footnote;
mod highlighter;
mod hyperlink;
mod role;
mod theme;

pub use color::{Color, ColorMode};
pub use footnote::{CLOSE as FOOTNOTE_CLOSE, backref_osc8, def_anchor_id, ref_anchor_id, ref_osc8};
pub use highlighter::highlight_line;
pub use hyperlink::{CLOSE as HYPERLINK_CLOSE, open as hyperlink_open, sanitize_url};
pub use role::Capture;
pub use theme::{Style, Theme, Variant, role_count, variant_from_osc11_reply};

pub use layout::StyleId;
