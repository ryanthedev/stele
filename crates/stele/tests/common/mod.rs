//! Fixtures and harnesses shared by `stele`'s integration tests.
//!
//! Rust compiles every file directly under `tests/` into its own crate, so
//! four waves of bug-bash tests each grew their own copy of the same helpers:
//! two byte-identical pty harnesses, two terminal models (one of which wrote
//! base64 raster payloads into the cell grid as text), and a `write_png` that
//! had already drifted three ways. Everything reusable lives here now, and a
//! test file declares `mod common;` and takes what it needs.
//!
//! **What must not happen here.** These tests exist because of one defect
//! archetype: *a count cannot see a position*. Four green tests asserting
//! counts, balances, and bounds-that-hold-by-construction hid four real
//! defects. So this module holds only *oracles* — a pty, a terminal model, a
//! PNG of a known pixel size — never a softened assertion. Sharing a helper is
//! worth doing; sharing it by turning a byte assertion into a count is not.
#![allow(dead_code)] // Each test binary uses a subset; the rest is not dead.

pub mod fixtures;
pub mod render;

#[cfg(unix)]
pub mod pty;
