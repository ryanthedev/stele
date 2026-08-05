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

use std::process::Command;

pub mod fixtures;
pub mod render;
pub mod termgfx;

#[cfg(unix)]
pub mod pty;

/// The real binary, with the developer's own configuration held off it.
///
/// Every spawn of `stele` in this crate's tests goes through here, and the one
/// thing it does beyond `Command::new` is the reason why. `stele` reads a
/// theme from `$XDG_CONFIG_HOME/stele/theme.toml` (else `$HOME/.config/...`)
/// when `--theme` does not name one, and a `[layout]` table in that file
/// changes the frame: `line_numbers = true` puts a gutter down the left of
/// every row. Inherited, that turns a developer's personal preference into a
/// test result — `test_a_watched_file_truncated_to_empty_repaints_instead_of_crashing`
/// asserted an empty first row and got `   │`, and the piped-stdin test hung
/// rather than failing, because after the gutter shifted the frame the bytes
/// it was waiting on never arrived and the child never reached its quit.
///
/// It belongs here rather than in [`pty`] because a child does not need a
/// terminal to read a config file: `cli_errors.rs` spawns without a pty and
/// asserts on stderr, which a warning from a developer's own broken theme
/// would just as quietly rewrite.
///
/// `XDG_CONFIG_HOME` alone is enough: `theme_source::default_config_path`
/// checks it first and returns without consulting `HOME`. The directory is
/// created rather than merely named so the variable points at a real, empty
/// config home instead of a path that happens not to exist — the same
/// configuration CI runs under, which is why CI never saw either failure.
pub fn viewer_command() -> Command {
    let empty = std::env::temp_dir().join("stele-tests-empty-config-home");
    std::fs::create_dir_all(&empty).expect("create an empty config home for the child");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_stele"));
    cmd.env("XDG_CONFIG_HOME", &empty);
    cmd
}
