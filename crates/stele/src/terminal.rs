//! Terminal lifecycle: enters raw mode + the alternate screen on
//! [`TerminalGuard::enter`], and restores both on normal drop *and* on
//! panic (installed by [`install_panic_hook`]) — a leaked raw-mode/alt-
//! screen terminal is a hard failure (DW-5.1).

use std::io::{self, Write};
use std::panic;

use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

/// Show-cursor + leave-alternate-screen, in that order. The single source
/// of truth for "what restoring the terminal means" — the guard's `Drop`
/// and the panic hook both write exactly this.
const RESTORE_SEQUENCE: &[u8] = b"\x1b[?25h\x1b[?1049l";

/// Enter-alternate-screen then hide-cursor, written once by
/// [`TerminalGuard::enter`]. The cursor stays hidden for the session so it
/// does not visibly hop during repaints; [`RESTORE_SEQUENCE`]'s `?25h`
/// shows it again on the way out.
const ENTER_SEQUENCE: &[u8] = b"\x1b[?1049h\x1b[?25l";

/// RAII guard: raw mode + the alternate screen are active for as long as
/// one of these is alive. Restores on drop, including during panic
/// unwinding.
pub struct TerminalGuard {
    writer: Box<dyn Write + Send>,
    manage_os_state: bool,
    restored: bool,
}

impl TerminalGuard {
    /// Enters raw mode and the alternate screen against the real terminal.
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        // Construct the guard BEFORE the fallible alt-screen write, so a
        // failure there still restores raw mode via Drop rather than leaking
        // it (DW-5.1). Restoring when the alt screen was never entered is
        // harmless — `?1049l` on the primary screen is a no-op.
        let mut guard = TerminalGuard {
            writer: Box::new(io::stdout()),
            manage_os_state: true,
            restored: false,
        };
        guard.writer.write_all(ENTER_SEQUENCE)?;
        guard.writer.flush()?;
        Ok(guard)
    }

    /// Test seam: a guard that writes its restore sequence to `writer`
    /// instead of the real terminal, and never touches raw-mode/alt-screen
    /// OS state — there is no real tty in a unit-test process.
    #[cfg(test)]
    fn for_test(writer: impl Write + Send + 'static) -> Self {
        TerminalGuard {
            writer: Box::new(writer),
            manage_os_state: false,
            restored: false,
        }
    }

    fn restore(&mut self) {
        if self.restored {
            return;
        }
        self.restored = true;
        let _ = self.writer.write_all(RESTORE_SEQUENCE);
        let _ = self.writer.flush();
        if self.manage_os_state {
            let _ = disable_raw_mode();
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // During panic unwinding the installed hook (see `install_panic_hook`)
        // has already emitted the restore sequence — before the message
        // printed — so re-emitting here would fight it and can leave a stray
        // `?1049l` that re-restores a stale cursor over the panic message.
        // Undo OS state (idempotent) but do not write the sequence again.
        if self.manage_os_state && std::thread::panicking() {
            if !self.restored {
                self.restored = true;
                let _ = disable_raw_mode();
            }
            return;
        }
        self.restore();
    }
}

/// The exact action taken when the terminal must be restored from a panic:
/// write [`RESTORE_SEQUENCE`] to `out`, then best-effort disable raw mode.
/// Factored out of the installed hook closure so it can be called directly
/// from a test without ever touching the process-global panic hook.
fn on_panic(out: &mut dyn Write) {
    let _ = out.write_all(RESTORE_SEQUENCE);
    let _ = out.flush();
    let _ = disable_raw_mode();
}

/// Installs a panic hook that restores the terminal before running the
/// previously installed hook, so a panic's message prints to a normal,
/// readable terminal instead of being lost inside the alternate screen /
/// raw mode.
pub fn install_panic_hook() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        on_panic(&mut io::stdout());
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A `Write` that appends into a shared buffer, so a test can inspect
    /// what was written after the writer has been moved into a
    /// `Box<dyn Write + Send>`.
    #[derive(Clone)]
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_restore_sequence_shows_cursor_before_leaving_alt_screen() {
        // Order matters: showing the cursor after leaving the alternate
        // screen would flash it in the wrong buffer.
        let seq = std::str::from_utf8(RESTORE_SEQUENCE).unwrap();
        let show_pos = seq.find("?25h").unwrap();
        let leave_pos = seq.find("?1049l").unwrap();
        assert!(show_pos < leave_pos);
    }

    #[test]
    fn test_dw_5_1_guard_drop_emits_restore_sequence() {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let guard = TerminalGuard::for_test(SharedBuf(buf.clone()));
        drop(guard);
        assert_eq!(buf.lock().unwrap().as_slice(), RESTORE_SEQUENCE);
    }

    #[test]
    fn test_restore_is_idempotent_across_repeated_calls() {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let mut guard = TerminalGuard::for_test(SharedBuf(buf.clone()));
        guard.restore();
        guard.restore();
        drop(guard);
        // A second (or third) restore must not duplicate the bytes on the
        // wire — this matters because both a panic-hook restore and the
        // subsequent Drop-during-unwind restore can fire for one event.
        assert_eq!(buf.lock().unwrap().as_slice(), RESTORE_SEQUENCE);
    }

    #[test]
    fn test_dw_5_1_panic_hook_body_emits_restore_sequence() {
        // Exercises the exact closure body `install_panic_hook` installs,
        // without installing a real process-global panic hook (which would
        // leak into every other test's failure reporting under `cargo
        // test`'s shared-process, multi-threaded runner).
        let mut buf = Vec::new();
        on_panic(&mut buf);
        assert_eq!(buf, RESTORE_SEQUENCE);
    }
}
