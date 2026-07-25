//! Terminal lifecycle: enters raw mode + the alternate screen on
//! [`TerminalGuard::enter`], and restores both on normal drop, on panic
//! (installed by [`install_panic_hook`]) *and* on a fatal signal — a leaked
//! raw-mode/alt-screen terminal is a hard failure (DW-5.1).

use std::io::{self, Write};
use std::panic;

use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

/// End-synchronized-update + show-cursor + leave-alternate-screen, in that
/// order. The single source of truth for "what restoring the terminal
/// means" — the guard's `Drop` and the panic hook both write exactly this.
///
/// `?2026l` leads for a reason. `Painter::frame` opens a mode-2026
/// synchronized-update block and `?`s on every write after it, so any I/O
/// error mid-frame — and any panic raised inside the media sink, which does
/// file I/O and image decode — unwinds with the block still open. Mode 2026
/// is a *global* terminal mode, not a screen-buffer one, so `?1049l` alone
/// leaves the user back at a shell that has stopped painting. Clearing it
/// first also guarantees the rest of this sequence (and any panic message
/// printed after it) is actually rendered rather than buffered into a frame
/// that never gets swapped in. Emitting `?2026l` when no block is open is a
/// no-op, so the unconditional reset costs nothing on the normal path.
const RESTORE_SEQUENCE: &[u8] = b"\x1b[?2026l\x1b[?25h\x1b[?1049l";

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
        // Snapshot the tty's canonical-mode settings BEFORE crossterm
        // replaces them, so the signal handler has something to put back,
        // then arm BEFORE anything is dirtied. Arming afterwards leaves a
        // real window — measured, not theoretical: a SIGTERM landing between
        // the alt-screen write and the arming call killed the process with
        // the default disposition and left the terminal wrecked. Restoring a
        // terminal that was never touched is a no-op in every component of
        // RESTORE_SEQUENCE, so arming early costs nothing.
        #[cfg(unix)]
        {
            signals::save_terminal_settings();
            signals::arm();
        }
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
        // Disarm first: a signal arriving between the write below and the
        // end of this function must not emit a second restore sequence.
        #[cfg(unix)]
        if self.manage_os_state {
            signals::disarm();
        }
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
                #[cfg(unix)]
                signals::disarm();
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
    #[cfg(unix)]
    signals::disarm();
    let _ = out.write_all(RESTORE_SEQUENCE);
    let _ = out.flush();
    let _ = disable_raw_mode();
}

/// Fatal-signal handling, the third way out of a dirty terminal.
///
/// [`TerminalGuard`]'s `Drop` covers a normal quit and an unwinding panic;
/// neither runs when the kernel kills the process. `SIGTERM` (`pkill stele`,
/// a supervisor teardown), `SIGHUP` (the terminal emulator or ssh session
/// going away) and `SIGINT` delivered as a *signal* all terminate the
/// process outright, leaving raw mode set, the alternate screen up and the
/// cursor hidden — the exact leak DW-5.1 calls a hard failure.
///
/// Note that installing a `SIGINT` handler does **not** change what Ctrl-C
/// does inside the viewer. Raw mode clears `ISIG`, so Ctrl-C arrives as an
/// ordinary key byte and never becomes a signal; this handler only fires for
/// an explicit `kill -INT`. Key handling is untouched.
///
/// Everything the handler calls is async-signal-safe per POSIX.1-2008:
/// `write`, `tcsetattr`, `raise`. Nothing allocates, locks, or reenters the
/// Rust runtime. The disposition is reset to `SIG_DFL` on entry
/// (`SA_RESETHAND`) and the signal re-raised, so the process still dies
/// *from that signal* — a wrapper or supervisor sees the same wait status it
/// saw before, only with the terminal put back first.
///
/// This is the crate's only `unsafe` module; see `lib.rs` for why the crate
/// lint is `deny` rather than `forbid`.
#[cfg(unix)]
#[allow(unsafe_code)]
mod signals {
    use std::cell::UnsafeCell;
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

    use super::RESTORE_SEQUENCE;

    /// Signals that kill the process while the terminal is dirty. `SIGQUIT`
    /// is deliberately absent: it is a "dump core now" request and honouring
    /// it verbatim is more useful than a tidy screen.
    const FATAL: [libc::c_int; 3] = [libc::SIGTERM, libc::SIGHUP, libc::SIGINT];

    /// True between [`arm`] and [`disarm`] — i.e. exactly while the terminal
    /// owes a restore. Also the handler's one-shot latch, so a restore
    /// racing a signal cannot emit the sequence twice.
    static ARMED: AtomicBool = AtomicBool::new(false);

    /// The descriptor [`SAVED`] was read from and must be written back to,
    /// or `-1` if we never found a tty. Resolved once, outside the handler,
    /// because opening `/dev/tty` is not something to do while a signal is
    /// being delivered.
    static TTY_FD: AtomicI32 = AtomicI32::new(-1);

    /// The tty's line discipline as it was before raw mode. Restoring the
    /// escape sequence alone would still leave the user at a shell with no
    /// echo and no line editing: raw mode is state on the *tty*, and it
    /// outlives the process that set it.
    struct TermiosSlot(UnsafeCell<libc::termios>);

    // SAFETY: written exactly once, by `save_terminal_settings`, before
    // `TTY_FD` is ever set to a real descriptor and before `arm` publishes
    // the handler; read only behind a `TTY_FD >= 0` acquire load thereafter.
    // There is no path that writes it while a reader can observe it — enforced
    // by `save_terminal_settings`'s own `TTY_FD >= 0` early return, not by the
    // call graph happening to call it once.
    unsafe impl Sync for TermiosSlot {}

    static SAVED: TermiosSlot = TermiosSlot(UnsafeCell::new(unsafe { std::mem::zeroed() }));

    /// How many times [`save_terminal_settings`] has run its body past the
    /// idempotence guard. Test-only instrumentation, and the only way to assert
    /// that guard: a successful resolution's sole external trace is a
    /// descriptor this module never closes — invisible from inside the process —
    /// and the resolution only happens at all when the host has a tty, which a
    /// `cargo test` process may not (it has none in a sandbox). Counting the
    /// body's executions makes the guard assertable with neither.
    #[cfg(test)]
    static RESOLUTIONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// Snapshot the controlling terminal's settings. Must be called *before*
    /// raw mode is enabled; if no tty can be found (stdin redirected and no
    /// controlling terminal) the handler simply does the escape-sequence half
    /// of the job.
    ///
    /// The fd is resolved the way crossterm resolves it — stdin if it is a
    /// tty, otherwise `/dev/tty` — so the settings we put back are the ones
    /// `enable_raw_mode` took away. The `/dev/tty` descriptor is deliberately
    /// never closed: it must stay valid for the whole session, including
    /// inside a signal handler.
    ///
    /// **Does nothing once a descriptor has been resolved.** That is not an
    /// optimization: [`TerminalGuard::enter`] is `pub`, so nothing stops a
    /// second call, and a second call would open a second never-closed
    /// `/dev/tty` *and* write `SAVED` while the armed handler can already read
    /// it through a live `TTY_FD` — the exact data race `TermiosSlot`'s
    /// `unsafe impl Sync` claims cannot happen. The early return is what makes
    /// that claim true.
    pub(super) fn save_terminal_settings() {
        if TTY_FD.load(Ordering::Acquire) >= 0 {
            return;
        }
        #[cfg(test)]
        RESOLUTIONS.fetch_add(1, Ordering::AcqRel);
        // SAFETY: `isatty`/`open` on a constant fd and a constant path;
        // `tcgetattr` writes one `termios` through a pointer to a static of
        // exactly that type, which nothing can be reading yet — the early
        // return above guarantees `TTY_FD` is still -1.
        unsafe {
            let fd = if libc::isatty(libc::STDIN_FILENO) == 1 {
                libc::STDIN_FILENO
            } else {
                libc::open(c"/dev/tty".as_ptr(), libc::O_RDWR | libc::O_NOCTTY)
            };
            if fd >= 0 && libc::tcgetattr(fd, SAVED.0.get()) == 0 {
                TTY_FD.store(fd, Ordering::Release);
            }
        }
    }

    /// Install the handler for every signal in [`FATAL`] and mark the
    /// terminal as owing a restore. Idempotent.
    pub(super) fn arm() {
        // SAFETY: `act` is a fully initialised `sigaction`; the third
        // argument is a null "old action" pointer, which `sigaction(2)`
        // documents as "do not report the previous action".
        unsafe {
            let mut act: libc::sigaction = std::mem::zeroed();
            act.sa_sigaction = handler as *const () as libc::sighandler_t;
            libc::sigemptyset(&mut act.sa_mask);
            // SA_RESETHAND: the disposition is back to SIG_DFL by the time
            // the handler body runs, so `raise` below terminates the process
            // instead of recursing.
            act.sa_flags = libc::SA_RESETHAND;
            for signo in FATAL {
                libc::sigaction(signo, &act, std::ptr::null_mut());
            }
        }
        ARMED.store(true, Ordering::Release);
    }

    /// The terminal no longer owes a restore: a later signal must die
    /// quietly rather than write escapes over a clean screen.
    pub(super) fn disarm() {
        ARMED.store(false, Ordering::Release);
    }

    /// The handler. Async-signal-safe end to end.
    extern "C" fn handler(signo: libc::c_int) {
        if ARMED.swap(false, Ordering::AcqRel) {
            // SAFETY: writing a 'static byte slice to fd 1 and, if we have
            // one, restoring a `termios` we captured ourselves. Both calls
            // are on the POSIX async-signal-safe list.
            unsafe {
                write_all(libc::STDOUT_FILENO, RESTORE_SEQUENCE);
                let tty = TTY_FD.load(Ordering::Acquire);
                if tty >= 0 {
                    libc::tcsetattr(tty, libc::TCSANOW, SAVED.0.get());
                }
            }
        }
        // SA_RESETHAND already put the default disposition back, so this
        // kills us with the signal we were sent — same wait status the
        // process had before this handler existed.
        // SAFETY: `raise` is async-signal-safe and takes no pointers.
        unsafe {
            libc::raise(signo);
        }
    }

    /// `write(2)` until the buffer is drained. A short write to a tty is
    /// vanishingly unlikely for a dozen bytes, but a partial restore
    /// sequence is exactly the failure this whole module exists to prevent.
    ///
    /// # Safety
    /// `fd` must be a valid file descriptor.
    unsafe fn write_all(fd: libc::c_int, buf: &[u8]) {
        let mut written = 0usize;
        while written < buf.len() {
            // SAFETY: the pointer stays inside `buf` because `written <
            // buf.len()`, and the length is the exact remaining tail.
            let n =
                unsafe { libc::write(fd, buf.as_ptr().add(written).cast(), buf.len() - written) };
            if n <= 0 {
                // EINTR or a closed/errored fd. Retrying an EINTR here could
                // spin inside a handler; the terminal is already lost in the
                // error case, so stop.
                return;
            }
            written += n as usize;
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// With a descriptor already resolved, `save_terminal_settings` must not
        /// run its body at all.
        ///
        /// Both halves of a second run are defects. It `open`s a second
        /// `/dev/tty` that is never closed; and it writes `SAVED` while the
        /// armed handler can already read it through a live `TTY_FD`, which is
        /// exactly the data race `TermiosSlot`'s `unsafe impl Sync` asserts
        /// cannot happen. `enter()` is `pub`, so "only ever called once" is a
        /// property of today's call graph, not of the code.
        ///
        /// The pre-set descriptor is `i32::MAX` — no `open`/`STDIN_FILENO`
        /// result can be that — so this test needs no tty of its own and gives
        /// the same verdict on a developer's terminal, in CI, and in a sandbox.
        /// It fails without the guard: the body's first statement is the counter
        /// bump.
        #[test]
        fn test_save_terminal_settings_does_not_re_resolve_once_set() {
            const SENTINEL: libc::c_int = libc::c_int::MAX;
            let previous = TTY_FD.swap(SENTINEL, Ordering::AcqRel);
            let before = RESOLUTIONS.load(Ordering::Acquire);

            save_terminal_settings();

            let runs = RESOLUTIONS.load(Ordering::Acquire) - before;
            let observed = TTY_FD.load(Ordering::Acquire);
            TTY_FD.store(previous, Ordering::Release);

            assert_eq!(
                runs, 0,
                "save_terminal_settings re-resolved the tty: it leaks a second \
                 /dev/tty fd and rewrites SAVED under the armed handler"
            );
            assert_eq!(observed, SENTINEL, "TTY_FD was clobbered");
        }
    }
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
    fn test_restore_sequence_ends_synchronized_update_before_anything_else() {
        // A frame that errors (or panics) part-way through has already
        // written `?2026h` and will never write its matching `?2026l`.
        // Restoring the terminal without clearing mode 2026 leaves the
        // user's shell frozen — the alt-screen exit does not clear it,
        // because 2026 is a global mode, not a screen-buffer one.
        let seq = std::str::from_utf8(RESTORE_SEQUENCE).unwrap();
        assert!(
            seq.starts_with("\x1b[?2026l"),
            "restore must end synchronized update first: {seq:?}"
        );
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
