//! The cross-phase seam: [`Probe`]. Signature pinned by the plan —
//! `Probe::open(GhosttyPty) -> Probe`, `query`, `cursor_pos`,
//! `measured_width` — because P3's width corpus, P5's frame assertions and
//! P6's placement assertions all build against it directly.

use std::os::fd::RawFd;
use std::time::Duration;

use crate::io_raw::{read_until_quiet, write_all};
use crate::pty::GhosttyPty;

/// How long [`Probe::cursor_pos`] and [`Probe::measured_width`] wait for a
/// `CSI 6n` reply before falling back to the documented `(0, 0)` sentinel.
/// `CSI 6n` (Device Status Report — cursor position) is near-universally
/// supported; a real Ghostty session answers it in well under this budget.
/// Reaching the timeout here signals a broken PTY session, not an
/// anticipated "terminal doesn't support this" case — see the doc comment
/// on `cursor_pos` for why that distinction determines the sentinel design.
const CURSOR_QUERY_TIMEOUT: Duration = Duration::from_millis(800);

/// Once at least one byte of a response has arrived, how long to wait for
/// more before deciding the response is complete. Collects multi-chunk
/// escape-sequence replies without paying the full timeout on every call.
const QUIET_WINDOW: Duration = Duration::from_millis(50);

/// A live channel to a terminal's stdio, opened only from a validated
/// [`GhosttyPty`]. Every write goes to the terminal; every read comes back
/// off the same fd, bounded by an explicit timeout so a non-answering
/// terminal can never hang a caller.
pub struct Probe {
    fd_in: RawFd,
    fd_out: RawFd,
    /// `Some` iff this `Probe` enabled raw mode itself and therefore owns
    /// disabling it again — set for `Probe::open` (real stdio), unset for
    /// the fd-pair constructor tests use (a bare pty pair isn't the
    /// process's controlling terminal, so crossterm's raw-mode calls don't
    /// apply to it). Never read — held purely so `RawModeGuard::drop`
    /// fires when `Probe` is dropped (including during a panic unwind).
    #[allow(dead_code)]
    raw_mode_guard: Option<RawModeGuard>,
}

struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> Self {
        // Pinned signature (`Probe::open(GhosttyPty) -> Probe`, no Result)
        // leaves no channel to propagate failure. GhosttyPty already proved
        // stdio is a real tty; raw-mode enable failing past that point is
        // an unrecoverable environment fault for a probe binary, not an
        // anticipated condition — hence `expect`, not error handling. See
        // the design note in the phase-1 discovery doc.
        crossterm::terminal::enable_raw_mode()
            .expect("enable_raw_mode failed after GhosttyPty already validated a real tty");
        RawModeGuard
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        // Best-effort: restoring terminal state must never itself panic
        // during unwind (including the panic path — this runs on a panic
        // too, since Drop runs during unwinding).
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

impl Probe {
    /// Opens a probe session against the current process's own stdio,
    /// which — because `pty` only exists if [`GhosttyPty::from_current_process`]
    /// already validated it — is a real Ghostty-hosted PTY. Enables raw
    /// mode for the duration of the returned `Probe`; restored on drop
    /// (including on panic, since `Drop::drop` runs during unwinding).
    pub fn open(pty: GhosttyPty) -> Probe {
        let _ = pty; // consumed only for the proof it represents
        Probe {
            fd_in: 0,
            fd_out: 1,
            raw_mode_guard: Some(RawModeGuard::enable()),
        }
    }

    /// Test/internal constructor over an arbitrary fd pair (e.g. one side
    /// of a `posix_openpt` pair), with no raw-mode management — a bare pty
    /// pair created in-process is not this process's controlling terminal,
    /// so crossterm's raw-mode calls (which operate on the controlling tty)
    /// don't apply and would be misleading to invoke.
    #[doc(hidden)]
    pub fn from_raw_fds_for_test(fd_in: RawFd, fd_out: RawFd) -> Probe {
        Probe {
            fd_in,
            fd_out,
            raw_mode_guard: None,
        }
    }

    /// Writes `seq`, then waits up to `timeout` for a reply. Returns `None`
    /// if nothing arrived before the deadline — the documented, expected
    /// outcome for a capability the terminal doesn't implement, never a
    /// hang. Returns `Some(bytes)` once at least one byte has arrived and a
    /// short quiet window passes with no further bytes, so multi-chunk
    /// replies are collected without waiting the full `timeout`.
    pub fn query(&mut self, seq: &[u8], timeout: Duration) -> Option<Vec<u8>> {
        // A write failure means the fd is gone (terminal closed under us);
        // that's the same "no answer" outcome as a timeout from the
        // caller's point of view.
        write_all(self.fd_out, seq).ok()?;
        read_until_quiet(self.fd_in, timeout, QUIET_WINDOW)
    }

    /// Cursor position via `CSI 6n` (Device Status Report), parsed from the
    /// terminal's `CSI row ; col R` reply.
    ///
    /// Returns `(0, 0)` if no valid reply arrives within
    /// [`CURSOR_QUERY_TIMEOUT`]. `CSI 6n` is near-universal — unlike the
    /// Spike-A capability probes, a non-response here indicates a broken
    /// session, not a capability gap, so the pinned infallible signature
    /// treats it as a sentinel rather than a per-call `Option`. Callers
    /// that must distinguish "really at (0,0)" from "no reply" should call
    /// [`Probe::query`] with `CSI 6n` directly.
    pub fn cursor_pos(&mut self) -> (u16, u16) {
        match self.query(b"\x1b[6n", CURSOR_QUERY_TIMEOUT) {
            Some(bytes) => parse_cursor_report(&bytes).unwrap_or((0, 0)),
            None => (0, 0),
        }
    }

    /// Measures the on-screen cell width `s` actually occupies in the live
    /// terminal: cursor position before, write `s`, cursor position after,
    /// return the delta. This is the standard CPR-based measurement
    /// technique (see `ucs-detect`'s `measure_width`, cited in the
    /// project's grounding research) — it asks the terminal, rather than a
    /// local Unicode table, so it is correct by construction for whatever
    /// terminal is on the other end.
    ///
    /// Intended for single grapheme clusters / short runs that do not wrap
    /// the line; a string long enough to wrap will under-report because the
    /// cursor's column resets at the wrap boundary.
    pub fn measured_width(&mut self, s: &str) -> u16 {
        let (_, col_before) = self.cursor_pos();
        // A write failure leaves col_after == col_before, reporting width
        // 0 — an honest answer ("nothing was measurably written") rather
        // than a fabricated one.
        if write_all(self.fd_out, s.as_bytes()).is_ok() {
            let (_, col_after) = self.cursor_pos();
            col_after.saturating_sub(col_before)
        } else {
            0
        }
    }
}

/// Parses a `CSI row ; col R` cursor position report. Tolerant of a leading
/// DA1 (`CSI ? ... c`) or other reply arriving first in the same buffer —
/// scans for the `R`-terminated form rather than assuming it is the only
/// thing in `bytes`.
fn parse_cursor_report(bytes: &[u8]) -> Option<(u16, u16)> {
    let text = String::from_utf8_lossy(bytes);
    for start in text.match_indices("\x1b[").map(|(i, _)| i) {
        let rest = &text[start + 2..];
        let Some(r_pos) = rest.find('R') else {
            continue;
        };
        let body = &rest[..r_pos];
        let mut parts = body.splitn(2, ';');
        let (Some(row), Some(col)) = (parts.next(), parts.next()) else {
            continue;
        };
        if let (Ok(row), Ok(col)) = (row.parse::<u16>(), col.parse::<u16>()) {
            return Some((row, col));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_clean_cursor_report() {
        assert_eq!(parse_cursor_report(b"\x1b[12;34R"), Some((12, 34)));
    }

    #[test]
    fn parses_a_cursor_report_after_a_leading_da1_reply() {
        // DA1 reply followed by the CPR reply in the same read buffer —
        // realistic when both were requested back-to-back (Spike A's
        // kitty-a=q + DA1 synchronization technique).
        let bytes = b"\x1b[?62;c\x1b[5;9R";
        assert_eq!(parse_cursor_report(bytes), Some((5, 9)));
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_cursor_report(b"not an escape sequence"), None);
        assert_eq!(parse_cursor_report(b"\x1b[R"), None);
        assert_eq!(parse_cursor_report(b"\x1b[12;abcR"), None);
    }
}
