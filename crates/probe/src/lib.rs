//! `crates/probe` — the PTY-driven Ghostty probe harness.
//!
//! Two directions of use:
//! - **Inside** a Ghostty-hosted PTY: [`GhosttyPty::from_current_process`]
//!   validates the environment, [`Probe`] then queries the terminal over
//!   its own stdio.
//! - **Outside**, driving Ghostty as a subprocess: [`Launcher`] spawns a
//!   probe binary inside a real Ghostty window and waits (with a hard
//!   timeout) for it to write its results.
//!
//! This is a cross-phase seam (see the plan's Phase 1 `Produces` block):
//! P3's width corpus, P5's frame assertions, and P6's placement assertions
//! all build directly against [`Probe::query`], [`Probe::cursor_pos`], and
//! [`Probe::measured_width`].

mod io_raw;
mod launch;
mod probe;
mod pty;

pub use launch::{LaunchError, Launcher};
pub use probe::Probe;
pub use pty::{GhosttyPty, GhosttyPtyError};
