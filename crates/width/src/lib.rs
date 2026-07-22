//! `crates/width` — Ghostty-correct cell-width measurement.
//!
//! Three functions hide everything else: grapheme segmentation
//! ([`graphemes`]), per-cluster cell width ([`WidthEngine::cluster_width`]),
//! and whole-string cell width ([`WidthEngine::display_width`]). Callers
//! never see the UCD base-width tables, the Ghostty correction layer
//! (VS15/VS16, ZWJ sequences, regional-indicator flag pairs, Fitzpatrick
//! skin-tone modifiers), or the ambiguous-width policy — those live
//! entirely inside [`WidthEngine`].
//!
//! Pure and total: no I/O, no terminal access, no panics on any input.
//! Ghostty is queried only by the separate, feature-gated corpus tool
//! (`src/bin/measure_corpus.rs`, built only under the `corpus-tool`
//! feature) that produced `corpus/ghostty-*-widths.json` — the engine
//! itself never talks to a terminal.
#![forbid(unsafe_code)]

mod correction;
mod engine;

pub use engine::{WidthConfig, WidthEngine, graphemes};
