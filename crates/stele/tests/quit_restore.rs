//! Ctrl-C quits, and quitting leaves the terminal exactly as it was found.
//!
//! Raw mode clears `ISIG`, so a Ctrl-C **keystroke** never becomes a signal:
//! it arrives as an ordinary `0x03` byte, crossterm reports it as
//! `Char('c')` + `CONTROL`, and only key handling can act on it. That makes
//! this a different path from `tests/signal_restore.rs`, which covers an
//! externally delivered `kill -INT`; both must end in the same clean restore,
//! and this test is the half that no unit test can reach — `AppState` can say
//! "quit", but only a real pty shows what the wire and the tty look like
//! afterwards.
//!
//! `q` is driven through the same harness as the control: whatever `q` does on
//! exit, Ctrl-C must do byte for byte. The harness itself — and the assertion
//! that raw mode was really on before the keystroke, without which this would
//! be the signal path wearing a keyboard's clothes — is `common::pty`.
//!
//! Run: `cargo test -p stele --test quit_restore -- --nocapture`

#![cfg(unix)]

mod common;

use common::pty::{Exit, assert_restores_the_terminal};

/// Ctrl-C must quit and restore, and it must do so identically to `q`.
///
/// One test, looping, rather than two parallel ones: `ptsname(3)` returns a
/// pointer into a libc-owned static buffer and is not thread-safe, and libtest
/// runs `#[test]`s on parallel threads.
#[test]
fn test_ctrl_c_quits_and_restores_the_terminal_exactly_as_q_does() {
    // `q` first: it is the documented quit key and the control for what a
    // clean exit looks like. `\x03` is the byte a terminal in raw mode
    // delivers for Ctrl-C, with no `SIGINT` involved.
    assert_restores_the_terminal("q", Exit::Key(b"q"));
    assert_restores_the_terminal("Ctrl-C", Exit::Key(b"\x03"));
}
