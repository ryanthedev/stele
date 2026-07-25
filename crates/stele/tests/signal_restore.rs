//! DW-5.1 on the one path `Drop` cannot reach: a fatal signal.
//!
//! `TerminalGuard`'s `Drop` and the panic hook are both unit-tested against a
//! `Write` seam, but neither runs when the kernel kills the process. This
//! test drives the **real binary** on a **real pty** and asserts the actual
//! bytes on the wire plus the tty's own line discipline afterwards — raw mode
//! is state on the terminal, not on the process, so it outlives `stele` and
//! only a real tty can show whether it was put back. The harness is
//! `common::pty`, shared with `tests/quit_restore.rs`, which drives the same
//! contract through a keystroke instead.
//!
//! Run: `cargo test -p stele --test signal_restore -- --nocapture`

#![cfg(unix)]

mod common;

use common::pty::{Exit, assert_restores_the_terminal};

/// `SIGTERM` (`pkill stele`, supervisor teardown), `SIGHUP` (the emulator or
/// ssh session going away) and `SIGINT` (`kill -INT`) all kill the process
/// outright, bypassing `Drop`. All three must leave the terminal usable.
///
/// One test, looping, rather than three parallel ones: `ptsname(3)` returns a
/// pointer into a libc-owned static buffer and is not thread-safe, and
/// libtest runs `#[test]`s on parallel threads.
#[test]
fn test_dw_5_1_a_fatal_signal_restores_the_terminal_instead_of_leaking_raw_mode() {
    for signo in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
        assert_restores_the_terminal(&format!("signal {signo}"), Exit::Signal(signo));
    }
}
