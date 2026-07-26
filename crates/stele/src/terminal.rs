//! Terminal lifecycle: enters raw mode + the alternate screen on
//! [`TerminalGuard::enter`], and restores both on normal drop, on panic
//! (installed by [`install_panic_hook`]) *and* on a fatal signal — a leaked
//! raw-mode/alt-screen terminal is a hard failure (DW-5.1).

use std::io::{self, IsTerminal, Write};
use std::os::fd::AsFd as _;
use std::panic;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

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
/// (`?1006l` and `?1000l` turn mouse reporting back off — see
/// [`MOUSE_ENABLE`]. They are *inside* this constant, not on a separate exit
/// path, because mouse capture is state on the **terminal**: a panic or a
/// `kill -TERM` that skipped them would hand the user back a shell whose
/// every click writes escape bytes into their prompt. Emitting them when
/// capture was never on, or was toggled off with `m`, is a no-op.)
const RESTORE_SEQUENCE: &[u8] = b"\x1b[?2026l\x1b[?1006l\x1b[?1000l\x1b[?25h\x1b[?1049l";

/// Turn on mouse reporting: normal tracking (button press/release, which is
/// also how a wheel notch is reported) plus SGR extended coordinates.
///
/// Deliberately **not** crossterm's `EnableMouseCapture`, which additionally
/// sets `?1002h` and `?1003h` — button-event and any-event tracking. Those
/// report every pointer *motion*, so a reader moving the mouse across the
/// window would wake this event loop hundreds of times a second for gestures
/// it has no binding for. Wheel and click are what DW-6.6 acts on, and
/// `?1000h` reports both. `?1006h` lifts the 223-column coordinate ceiling,
/// and crossterm's parser reads SGR reports regardless of which modes we set.
pub const MOUSE_ENABLE: &[u8] = b"\x1b[?1000h\x1b[?1006h";

/// The inverse of [`MOUSE_ENABLE`], in reverse order. Also a substring of
/// [`RESTORE_SEQUENCE`], which `test_dw_6_6_...` pins.
pub const MOUSE_DISABLE: &[u8] = b"\x1b[?1006l\x1b[?1000l";

/// The most base64 bytes an OSC 52 clipboard write may carry (DW-6.7).
///
/// Terminals cap what they will accept — xterm's default is 8192 *bytes of
/// sequence*, and Ghostty's own ceiling is not documented — so a large code
/// block can be silently dropped or, worse, truncated by the terminal
/// mid-payload. 64 KiB of base64 (48 KiB of text) is comfortably above any
/// real code block and below the point where a terminal is likely to be
/// choosing for us. Past it [`osc52_copy`] returns `None` and the caller
/// reports a refusal, because a *silently truncated* shell command on the
/// clipboard is a hazard the reader cannot see, while "nothing was copied" is
/// one they can.
pub const MAX_CLIPBOARD_BASE64: usize = 64 * 1024;

/// Enter-alternate-screen then hide-cursor, written once by
/// [`TerminalGuard::enter`]. The cursor stays hidden for the session so it
/// does not visibly hop during repaints; [`RESTORE_SEQUENCE`]'s `?25h`
/// shows it again on the way out.
const ENTER_SEQUENCE: &[u8] = b"\x1b[?1049h\x1b[?25l";

/// Builds the OSC 52 sequence that puts `text` on the system clipboard
/// (DW-6.7), or `None` when the base64 payload would exceed
/// [`MAX_CLIPBOARD_BASE64`].
///
/// `ESC ] 52 ; c ; <base64> ST`. The `c` selects the clipboard proper (rather
/// than the primary selection), and the terminator is ST (`ESC \`) to match
/// every other OSC this codebase emits.
///
/// **Nothing needs sanitizing on the way in, and that is a property of base64
/// rather than luck.** The payload is encoded before it is interpolated, and
/// the standard base64 alphabet is `A-Za-z0-9+/=` — no ESC, no BEL, no `;`,
/// so a code block full of escape sequences cannot close this sequence early
/// or start another one. The one test that matters here scans the built
/// sequence for stray control bytes rather than trusting that argument.
pub fn osc52_copy(text: &str) -> Option<String> {
    use base64::Engine as _;

    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    if encoded.len() > MAX_CLIPBOARD_BASE64 {
        return None;
    }
    Some(format!("\x1b]52;c;{encoded}\x1b\\"))
}

/// Cell geometry to use when the terminal will not say: `(width_px,
/// height_px)`.
///
/// It is the value `docs/spikes/ghostty-caps.md` item 7 measured on the
/// reference install (Ghostty 1.3.1, Darwin arm64) — so it is a real
/// measurement of *one* machine, not a universal truth. Anything that reads
/// this instead of [`CellGeometry::cell_px`] from a live answer is running on
/// a guess, which is why [`CellGeometry`] carries its [`CellPxSource`]:
/// "queried" and "assumed" must not look the same to a reader.
///
/// **Which way the fallback errs, and why that is the safer way.** Every
/// consumer multiplies a cell count by these numbers to get a *pixel* target
/// ([`crate::media::GfxMediaSink`]'s raster size) or divides a pixel count by
/// them to get a *cell* count (`ImageSizer::px_to_cells`). Claiming cells are
/// bigger than they are therefore oversizes every raster — more bytes
/// base64-encoded, transmitted inside a 1000 ms synchronized-update budget,
/// and stored against the terminal's 320 MB image quota — while claiming they
/// are smaller only costs resolution the terminal then scales up. 24×48 is at
/// the small end of plausible for a modern hidpi terminal (a 2x-scaled 12×24
/// logical cell), so it errs toward *under*-sizing on the machines where it is
/// wrong. That is deliberate.
pub const FALLBACK_CELL_PX: (u32, u32) = (24, 48);

/// Where a [`CellGeometry`] came from. Ordered best-first, and that order is
/// the ladder [`query_cell_px`] walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellPxSource {
    /// `CSI 16t` — the terminal answered with its own per-cell pixel size.
    /// The spike's primary source: exact, and cross-checked there against
    /// `CSI 14t` ÷ (rows, cols) to the pixel.
    Csi16t,
    /// `TIOCGWINSZ`'s `ws_xpixel`/`ws_ypixel` divided by the grid size, via
    /// `crossterm::terminal::window_size`. A local ioctl, no round trip, but
    /// the spike measured its pixel fields ~0.5–1.6% *high* — it reports the
    /// whole window including padding/chrome, not the character grid. The
    /// division truncates, which pulls that bias back down rather than
    /// compounding it.
    WindowSize,
    /// Nobody answered: [`FALLBACK_CELL_PX`].
    Fallback,
}

/// A cell size and the provenance of the number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellGeometry {
    pub cell_px: (u32, u32),
    pub source: CellPxSource,
}

impl CellGeometry {
    /// The fallback, labelled as such.
    pub const fn assumed() -> Self {
        CellGeometry {
            cell_px: FALLBACK_CELL_PX,
            source: CellPxSource::Fallback,
        }
    }
}

/// XTWINOPS "report cell size in pixels". `docs/spikes/ghostty-caps.md` item
/// 7 measured live Ghostty answering this with `CSI 6 ; 48 ; 24 t`, and
/// measured `OSC 1337 ReportCellSize` — iTerm2's mechanism — going unanswered,
/// which is why that one is not attempted here.
const CELL_SIZE_QUERY: &[u8] = b"\x1b[16t";

/// How long [`query_cell_px`] waits for the `CSI 16t` reply before giving up
/// and dropping to the next rung.
///
/// This is a round trip on the same descriptor crossterm is about to read key
/// events from, so the deadline is not just a liveness guard — it decides who
/// owns the reply bytes. Everything that arrives before it is consumed here;
/// anything that arrives after it lands in crossterm's input stream, where a
/// `CSI 6;48;24t` is not a key sequence crossterm parses. The budget is
/// therefore deliberately generous relative to the ~ms a local terminal needs
/// (the spike's own probe used 500 ms per query and never timed out against a
/// live Ghostty), and it is paid in full exactly once, at startup, only when
/// graphics are enabled at all. A terminal that answers *later* than this is
/// the one residual hazard and it cannot be designed away — any timeout has
/// the same tail.
const CELL_SIZE_TIMEOUT: Duration = Duration::from_millis(250);

/// Bounds on a cell size this code will believe. A terminal answering `1;1`
/// (or a garbled reply parsed into something enormous) is worse than no
/// answer: the low end makes `px_to_cells` reserve a box hundreds of cells
/// tall for an ordinary badge, and the high end oversizes every raster, which
/// is the direction [`FALLBACK_CELL_PX`] explains we must not err in. Real
/// cells run roughly 4×8 (tiny bitmap fonts) to 60×140 (a large font at 3x
/// scaling); the range below is wider than that on both sides, so it rejects
/// only nonsense.
const MIN_PLAUSIBLE_CELL_PX: u32 = 2;
const MAX_PLAUSIBLE_CELL_PX: u32 = 256;

/// Whether [`TerminalGuard::enter`] interrogates the terminal for its cell
/// geometry on the way in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellQuery {
    /// Ask. One round trip, bounded by [`CELL_SIZE_TIMEOUT`].
    Ask,
    /// Don't: use [`FALLBACK_CELL_PX`] and put no query bytes on the wire.
    /// What the orchestrator passes when graphics are off — nothing would
    /// consume the answer, and under a multiplexer the query would only add
    /// bytes to a wire that will not answer it.
    Skip,
}

/// Resolves the terminal's cell size once, at startup, walking
/// [`CellPxSource`]'s ladder: ask the terminal (`CSI 16t`), else ask the
/// kernel (`TIOCGWINSZ`), else assume [`FALLBACK_CELL_PX`].
///
/// Called from [`TerminalGuard::enter`] rather than by the orchestrator
/// directly, and the position in that sequence is load-bearing in both
/// directions. It has to be **after** `enable_raw_mode` — the reply has no
/// newline in it, so a canonical-mode `read` would never return it — and
/// before `crossterm::event` is first polled, or the event parser gets the
/// reply instead of us.
///
/// It also has to be **before** the alternate-screen write, which is a
/// subtler constraint discovered by breaking it. The query is a quiet
/// interval: bytes out, then up to [`CELL_SIZE_TIMEOUT`] of nothing while a
/// non-answering terminal is waited on. Doing it after `ENTER_SEQUENCE` puts
/// that silence *between* the alt-screen switch and the first frame, so
/// anything watching the wire — `tests/common/pty.rs`'s `drain_quiet`, or a
/// human — reads the pause as "startup finished" while the first frame has
/// not been painted yet. Querying first makes "the alt-screen sequence is on
/// the wire" mean "the terminal interrogation is over", and costs the 250 ms
/// while the user is still looking at their shell rather than at a cleared
/// blank screen.
///
/// Never blocks longer than [`CELL_SIZE_TIMEOUT`], never panics, and writes
/// nothing at all unless both stdin and stdout are terminals (a redirected
/// stdout must not get an escape sequence spat into it).
pub fn query_cell_px() -> CellGeometry {
    if let Some(cell_px) = query_csi_16t() {
        return CellGeometry {
            cell_px,
            source: CellPxSource::Csi16t,
        };
    }
    if let Some(cell_px) = cell_px_from_window_size() {
        return CellGeometry {
            cell_px,
            source: CellPxSource::WindowSize,
        };
    }
    CellGeometry::assumed()
}

fn query_csi_16t() -> Option<(u32, u32)> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    if !stdin.is_terminal() || !stdout.is_terminal() {
        return None;
    }
    csi_16t_over(stdin.as_fd(), &mut stdout, CELL_SIZE_TIMEOUT)
}

/// The `CSI 16t` round trip itself, over an explicit descriptor pair: write the
/// query to `out`, then read `fd_in` until a cell-size report parses or
/// `timeout` elapses.
///
/// Split out from [`query_csi_16t`] so it can be driven against a *fake*
/// terminal — a pty pair with a scripted reply — rather than only against the
/// process's own stdio, which no test process has. Without this seam the only
/// falsifiable part of the query would be [`parse_cell_size_reply`], and
/// "the bytes go out, the reply comes back, and the answer is believed" would
/// be an argument rather than an assertion. See
/// `tests/cell_geometry_query.rs`.
#[doc(hidden)]
pub fn csi_16t_over(
    fd_in: std::os::fd::BorrowedFd<'_>,
    out: &mut dyn Write,
    timeout: Duration,
) -> Option<(u32, u32)> {
    out.write_all(CELL_SIZE_QUERY).ok()?;
    out.flush().ok()?;

    let deadline = Instant::now() + timeout;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 256];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }
        if !readable(&fd_in, remaining) {
            return None;
        }
        match rustix::io::read(fd_in, &mut chunk[..]) {
            Ok(0) | Err(_) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
        // Parse after every read rather than waiting for a quiet window: the
        // reply is short and self-terminating (`t`), so there is nothing to
        // gain by waiting once it has arrived, and every millisecond spent
        // waiting is a millisecond of the user's startup.
        if let Some(cell_px) = parse_cell_size_reply(&buf) {
            return plausible(cell_px);
        }
        // Something arrived and it was not a cell-size report — other terminal
        // chatter, or the user leaning on a key. Keep reading until the
        // deadline; bailing here would strand the real reply in crossterm's
        // input stream.
        if buf.len() > 4096 {
            return None;
        }
    }
}

/// `poll(2)` for readability on `fd`, bounded by `timeout`.
///
/// `rustix`'s wrapper rather than a hand-written `libc::poll`: the crate is
/// already linked in (crossterm 0.29 uses it for termios) and its API is
/// safe, so asking the terminal a question costs this crate no new `unsafe`
/// surface — `terminal::signals` stays the only one, which is what
/// `tests/hardening.rs` asserts.
fn readable(fd: &impl std::os::fd::AsFd, timeout: Duration) -> bool {
    use rustix::event::{PollFd, PollFlags, Timespec};
    let Ok(spec) = Timespec::try_from(timeout) else {
        return false;
    };
    let mut fds = [PollFd::new(fd, PollFlags::IN)];
    match rustix::event::poll(&mut fds, Some(&spec)) {
        Ok(0) | Err(_) => false,
        Ok(_) => fds[0].revents().contains(PollFlags::IN),
    }
}

/// Parses an XTWINOPS cell-size report, `CSI 6 ; height ; width t`, into
/// `(width_px, height_px)`.
///
/// Note the argument order: the wire form is **height first**, and swapping
/// them yields a plausible-looking pair that mis-sizes every raster by the
/// cell's aspect ratio. Scans for the report rather than assuming it is the
/// only thing in `bytes` — an unrelated reply, or a keystroke the user typed
/// during startup, can share the buffer.
fn parse_cell_size_reply(bytes: &[u8]) -> Option<(u32, u32)> {
    let text = String::from_utf8_lossy(bytes);
    for start in text.match_indices("\x1b[").map(|(i, _)| i) {
        let rest = &text[start + 2..];
        let Some(end) = rest.find('t') else {
            continue;
        };
        let mut parts = rest[..end].split(';');
        // `6` is XTWINOPS's "this is a cell-size report" discriminator. `4`
        // (window size) and `6`'s sibling `8` (text area in cells) use the
        // same `t` terminator, so the discriminator is what makes this parse
        // a cell size rather than whatever else answered.
        if parts.next() != Some("6") {
            continue;
        }
        let (Some(height), Some(width)) = (parts.next(), parts.next()) else {
            continue;
        };
        if parts.next().is_some() {
            continue;
        }
        if let (Ok(height), Ok(width)) = (height.parse::<u32>(), width.parse::<u32>()) {
            return Some((width, height));
        }
    }
    None
}

/// `TIOCGWINSZ`'s pixel fields divided by the grid size, or `None` when the
/// terminal (or multiplexer) reports zeros — which is the common case, and the
/// one that would divide by zero if taken at face value.
fn cell_px_from_window_size() -> Option<(u32, u32)> {
    let size = crossterm::terminal::window_size().ok()?;
    cell_px_from_window_size_parts(size.columns, size.rows, size.width, size.height)
}

fn cell_px_from_window_size_parts(
    columns: u16,
    rows: u16,
    width_px: u16,
    height_px: u16,
) -> Option<(u32, u32)> {
    if columns == 0 || rows == 0 || width_px == 0 || height_px == 0 {
        return None;
    }
    plausible((
        u32::from(width_px) / u32::from(columns),
        u32::from(height_px) / u32::from(rows),
    ))
}

/// `Some` only for a cell size inside [`MIN_PLAUSIBLE_CELL_PX`] ..=
/// [`MAX_PLAUSIBLE_CELL_PX`] on both axes. Rejecting outright, rather than
/// clamping, is deliberate: a clamped answer would be recorded as
/// [`CellPxSource::Csi16t`] — "the terminal told us" — when it is really
/// half the terminal's answer and half a constant.
fn plausible(cell_px: (u32, u32)) -> Option<(u32, u32)> {
    let sane = |v: u32| (MIN_PLAUSIBLE_CELL_PX..=MAX_PLAUSIBLE_CELL_PX).contains(&v);
    (sane(cell_px.0) && sane(cell_px.1)).then_some(cell_px)
}

/// RAII guard: raw mode + the alternate screen are active for as long as
/// one of these is alive. Restores on drop, including during panic
/// unwinding.
pub struct TerminalGuard {
    writer: Box<dyn Write + Send>,
    manage_os_state: bool,
    restored: bool,
    cell_geometry: CellGeometry,
}

impl TerminalGuard {
    /// Enters raw mode and the alternate screen against the real terminal,
    /// resolving the terminal's cell geometry on the way in when `query` asks
    /// for it (see [`query_cell_px`] for why that happens here and not in the
    /// caller). Read the answer back with [`TerminalGuard::cell_geometry`].
    pub fn enter(query: CellQuery) -> io::Result<Self> {
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
            cell_geometry: CellGeometry::assumed(),
        };
        // Between raw mode and the alternate screen, on purpose — see
        // `query_cell_px`. Infallible by design, so it cannot strand a
        // half-entered terminal here.
        if query == CellQuery::Ask {
            guard.cell_geometry = query_cell_px();
        }
        guard.writer.write_all(ENTER_SEQUENCE)?;
        // Mouse capture starts on (DW-6.6) and `m` turns it off. Written here
        // rather than by the event loop so it is bracketed by the same guard
        // that owns the restore — there is no window in which reporting is on
        // and nothing is armed to turn it back off.
        guard.writer.write_all(MOUSE_ENABLE)?;
        guard.writer.flush()?;
        Ok(guard)
    }

    /// Turns mouse reporting on or off mid-session (DW-6.6).
    ///
    /// Writes to the guard's own writer rather than to the frame writer: this
    /// is terminal *mode*, not frame content, and routing it through the
    /// painter's `BufWriter` would leave it sitting unflushed until the next
    /// frame ended.
    pub fn set_mouse_capture(&mut self, on: bool) -> io::Result<()> {
        let sequence = if on { MOUSE_ENABLE } else { MOUSE_DISABLE };
        self.writer.write_all(sequence)?;
        self.writer.flush()
    }

    /// The cell geometry resolved by [`TerminalGuard::enter`]: the terminal's
    /// own answer when it gave one, [`CellGeometry::assumed`] otherwise. The
    /// [`CellPxSource`] is carried so a caller can tell those apart.
    pub fn cell_geometry(&self) -> CellGeometry {
        self.cell_geometry
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
            cell_geometry: CellGeometry::assumed(),
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

/// Set by [`install_panic_hook`]'s closure, before it does anything else, and
/// read by every [`PanicGuardedWriter`] the running session constructed
/// against [`frame_poison_flag`]. See [`PanicGuardedWriter`] for why this
/// exists; kept private specifically so nothing outside this module can
/// poison the *process-wide* instance from a unit test and leak that state
/// into an unrelated one — tests construct their own local `AtomicBool` and
/// pass a reference to it instead.
static FRAME_POISONED: AtomicBool = AtomicBool::new(false);

/// The process-wide flag [`install_panic_hook`] poisons. `main.rs` passes
/// this to the [`PanicGuardedWriter`] wrapped around the real `stdout` —
/// there is exactly one real session per process, so a single `'static`
/// flag is correct for production; [`PanicGuardedWriter::new`] itself is
/// generic over the flag precisely so tests need not share it.
pub fn frame_poison_flag() -> &'static AtomicBool {
    &FRAME_POISONED
}

/// A [`Write`] that silently discards every byte once `poisoned` reads
/// `true` (DW-1.6).
///
/// `run_session` wraps its `BufWriter` around one of these rather than
/// around stdout directly. Without it, a panic that unwinds through a
/// half-written frame leaves that frame's bytes sitting in the
/// `BufWriter`'s own internal buffer; `BufWriter`'s `Drop` best-effort
/// flushes that buffer regardless of *why* the stack is unwinding, so those
/// stale escape bytes would land on stdout **after** [`on_panic`] has
/// already written [`RESTORE_SEQUENCE`] — a half-painted frame appearing on
/// a terminal the user has just been told is restored. Poisoning happens
/// before `on_panic` runs (see [`install_panic_hook`]), so by the time
/// unwinding reaches the `BufWriter`'s `drop`, every write it attempts
/// (including that flush) is already a no-op here.
pub struct PanicGuardedWriter<'a, W> {
    inner: W,
    poisoned: &'a AtomicBool,
}

impl<'a, W> PanicGuardedWriter<'a, W> {
    pub fn new(inner: W, poisoned: &'a AtomicBool) -> Self {
        PanicGuardedWriter { inner, poisoned }
    }
}

impl<W: Write> Write for PanicGuardedWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.poisoned.load(Ordering::Acquire) {
            return Ok(buf.len());
        }
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.poisoned.load(Ordering::Acquire) {
            return Ok(());
        }
        self.inner.flush()
    }
}

/// Installs a panic hook that restores the terminal before running the
/// previously installed hook, so a panic's message prints to a normal,
/// readable terminal instead of being lost inside the alternate screen /
/// raw mode. Also poisons [`frame_poison_flag`] first — see
/// [`PanicGuardedWriter`] for why that order matters.
pub fn install_panic_hook() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        FRAME_POISONED.store(true, Ordering::Release);
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

    /// DW-6.6: mouse capture must come off on **every** exit, and the exits
    /// this crate has are `Drop`, the panic hook, and the signal handler —
    /// all three of which write exactly [`RESTORE_SEQUENCE`] and nothing else.
    ///
    /// Derived rather than restated: for each mode [`MOUSE_ENABLE`] sets, the
    /// matching reset must be in the restore. Pinning the literal bytes twice
    /// would pass just as happily if someone added a `?1003h` to the enable
    /// side and forgot the `l`.
    #[test]
    fn test_dw_6_6_every_mouse_mode_enabled_on_entry_is_disabled_by_the_restore_sequence() {
        let enable = std::str::from_utf8(MOUSE_ENABLE).unwrap();
        let restore = std::str::from_utf8(RESTORE_SEQUENCE).unwrap();
        let disable = std::str::from_utf8(MOUSE_DISABLE).unwrap();

        let modes: Vec<&str> = enable
            .split("\x1b[?")
            .skip(1)
            .map(|part| part.trim_end_matches('h'))
            .collect();
        assert!(!modes.is_empty(), "the enable sequence must set something");
        for mode in &modes {
            let reset = format!("\x1b[?{mode}l");
            assert!(
                restore.contains(&reset),
                "mode {mode} is enabled on entry but never reset by the restore \
                 sequence — a `kill -TERM` would leave the terminal reporting \
                 mouse events into the user's shell"
            );
            assert!(
                disable.contains(&reset),
                "mode {mode} is enabled but `m` cannot turn it off"
            );
        }
    }

    #[test]
    fn test_dw_6_6_the_restore_disables_the_mouse_before_leaving_the_alt_screen() {
        let seq = std::str::from_utf8(RESTORE_SEQUENCE).unwrap();
        let mouse = seq.find("\x1b[?1000l").expect("the restore disables mouse");
        let alt = seq
            .find("\x1b[?1049l")
            .expect("the restore leaves alt screen");
        assert!(
            mouse < alt,
            "mouse reporting is a global mode, so it must be cleared while we \
             still own the screen: {seq:?}"
        );
    }

    #[test]
    fn test_dw_6_6_toggling_capture_writes_the_enable_and_disable_sequences() {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let mut guard = TerminalGuard::for_test(SharedBuf(buf.clone()));
        guard.set_mouse_capture(false).expect("write");
        assert_eq!(buf.lock().unwrap().as_slice(), MOUSE_DISABLE);
        buf.lock().unwrap().clear();
        guard.set_mouse_capture(true).expect("write");
        assert_eq!(buf.lock().unwrap().as_slice(), MOUSE_ENABLE);
    }

    // ---------------------------------------------------------------- DW-6.7

    #[test]
    fn test_dw_6_7_osc_52_wraps_exactly_the_payload_bytes_in_base64() {
        use base64::Engine as _;

        for payload in [
            "curl https://example.com | sh\n",
            "",
            "unicode: 漢字 — ✓\n",
            "trailing whitespace   \n\n",
        ] {
            let seq = osc52_copy(payload).expect("well under the ceiling");
            let body = seq
                .strip_prefix("\x1b]52;c;")
                .and_then(|rest| rest.strip_suffix("\x1b\\"))
                .unwrap_or_else(|| panic!("malformed sequence for {payload:?}: {seq:?}"));
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(body)
                .expect("the body must be valid base64");
            assert_eq!(
                decoded,
                payload.as_bytes(),
                "the clipboard must carry exactly the block's bytes"
            );
        }
    }

    /// The injection claim, scanned rather than argued: a code block full of
    /// escape sequences must not be able to close this OSC or open another.
    #[test]
    fn test_dw_6_7_the_sequence_carries_no_raw_control_bytes() {
        let hostile = "\x1b]52;c;AAAA\x07 rm -rf / \x1b\\ \u{9b}0m \0 ;;;";
        let seq = osc52_copy(hostile).expect("small");
        let body = seq
            .strip_prefix("\x1b]52;c;")
            .and_then(|rest| rest.strip_suffix("\x1b\\"))
            .expect("exactly one open/close pair");
        for byte in body.bytes() {
            assert!(
                byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='),
                "byte {byte:#04x} is outside the base64 alphabet: {seq:?}"
            );
        }
        assert_eq!(
            seq.matches('\x1b').count(),
            2,
            "exactly the opening OSC and the closing ST: {seq:?}"
        );
    }

    #[test]
    fn test_dw_6_7_an_oversized_payload_is_refused_rather_than_truncated() {
        // Base64 is 4 bytes out per 3 in, so this is one encoded byte past the
        // ceiling — the exact boundary, not "somewhere large".
        let over = "a".repeat(MAX_CLIPBOARD_BASE64 / 4 * 3 + 1);
        assert_eq!(
            osc52_copy(&over),
            None,
            "a truncated shell command on the clipboard looks complete; \
             refusing is the honest failure"
        );
        let under = "a".repeat(MAX_CLIPBOARD_BASE64 / 4 * 3);
        let seq = osc52_copy(&under).expect("exactly at the ceiling still copies");
        assert!(seq.len() > MAX_CLIPBOARD_BASE64);
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

    /// The exact bytes the spike recorded live Ghostty answering with
    /// (`docs/spikes/ghostty-caps.md` item 7: `\x1b[6;48;24t` → height 48,
    /// width 24), parsed back into the `(width, height)` order every consumer
    /// uses. If this ever returns `(48, 24)` the wire order has been read
    /// backwards and every raster is off by the cell's aspect ratio.
    #[test]
    fn test_parses_the_cell_size_report_ghostty_actually_sent() {
        assert_eq!(parse_cell_size_reply(b"\x1b[6;48;24t"), Some((24, 48)));
    }

    #[test]
    fn test_cell_size_report_is_found_alongside_other_terminal_chatter() {
        // A DA1 reply, then a keystroke the user got in during startup, then
        // the report. All three can share one read buffer.
        assert_eq!(
            parse_cell_size_reply(b"\x1b[?62;c\x1b[A\x1b[6;40;20t"),
            Some((20, 40))
        );
        // Split arrival: the first half alone must not parse into anything.
        assert_eq!(parse_cell_size_reply(b"\x1b[6;40;"), None);
    }

    /// The other XTWINOPS reports share the `t` terminator. Accepting them
    /// would read a *window* size (thousands of pixels) or a *text area* size
    /// (in cells) as a cell size — the first is rejected by `plausible`, but
    /// the second (`CSI 8 ; 42 ; 155 t`) is a perfectly plausible-looking
    /// 155×42 "cell" and would silently mis-size everything.
    #[test]
    fn test_other_xtwinops_reports_are_not_mistaken_for_a_cell_size() {
        assert_eq!(parse_cell_size_reply(b"\x1b[8;42;155t"), None);
        assert_eq!(parse_cell_size_reply(b"\x1b[4;2016;3720t"), None);
        assert_eq!(parse_cell_size_reply(b"\x1b[6;48t"), None);
        assert_eq!(parse_cell_size_reply(b"\x1b[6;48;24;9t"), None);
        assert_eq!(parse_cell_size_reply(b"\x1b[6;x;yt"), None);
        assert_eq!(parse_cell_size_reply(b"no escapes here"), None);
    }

    /// A terminal that answers with nonsense must fall through the ladder
    /// rather than be believed. The high end is the one that matters: it
    /// oversizes every raster, which is the direction `FALLBACK_CELL_PX`'s
    /// comment commits to never erring in.
    #[test]
    fn test_an_implausible_answer_is_rejected_not_clamped() {
        assert_eq!(plausible((24, 48)), Some((24, 48)));
        assert_eq!(plausible((0, 48)), None);
        assert_eq!(plausible((1, 48)), None);
        assert_eq!(plausible((24, 0)), None);
        assert_eq!(plausible((10_000, 10_000)), None);
        assert_eq!(plausible((24, 4096)), None);
        assert_eq!(
            plausible((MAX_PLAUSIBLE_CELL_PX, MAX_PLAUSIBLE_CELL_PX)),
            Some((MAX_PLAUSIBLE_CELL_PX, MAX_PLAUSIBLE_CELL_PX))
        );
    }

    /// The `TIOCGWINSZ` rung, on the numbers the spike actually measured:
    /// rows=42 cols=155 xpixel=3740 ypixel=2048. Those pixel fields are
    /// ~0.5–1.6% high (they include window chrome), and the truncating
    /// division is what absorbs the bias — 3740/155 = 24.13 → 24 and
    /// 2048/42 = 48.76 → 48, both landing exactly on `CSI 16t`'s answer.
    #[test]
    fn test_window_size_rung_reproduces_the_measured_cell_size() {
        assert_eq!(
            cell_px_from_window_size_parts(155, 42, 3740, 2048),
            Some((24, 48))
        );
    }

    /// The failure mode §5.6 of the wave-4 research asked to confirm: a
    /// multiplexer (or a pty with no pixel size set, which is every pty in
    /// this test suite) reports zeros. Taking those at face value divides by
    /// zero; reporting them as a cell size hands `px_to_cells` a divisor of 0.
    #[test]
    fn test_window_size_rung_declines_rather_than_dividing_by_zero() {
        assert_eq!(cell_px_from_window_size_parts(80, 24, 0, 0), None);
        assert_eq!(cell_px_from_window_size_parts(0, 0, 3740, 2048), None);
        assert_eq!(cell_px_from_window_size_parts(80, 0, 3740, 2048), None);
        assert_eq!(cell_px_from_window_size_parts(155, 42, 3740, 0), None);
        // Plausibility still applies on this rung: a window reporting one
        // pixel per cell is not a cell size.
        assert_eq!(cell_px_from_window_size_parts(155, 42, 155, 42), None);
    }

    #[test]
    fn test_the_assumed_geometry_labels_itself_as_a_fallback() {
        // Provenance is the point of the type: a consumer must be able to
        // tell "the terminal said 24x48" from "nobody answered, so we
        // guessed 24x48" — the two are byte-identical in `cell_px`.
        let assumed = CellGeometry::assumed();
        assert_eq!(assumed.cell_px, FALLBACK_CELL_PX);
        assert_eq!(assumed.source, CellPxSource::Fallback);
        assert_ne!(CellPxSource::Fallback, CellPxSource::Csi16t);
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

    /// DW-1.6, the writer in isolation: a *local* `AtomicBool`, never
    /// `FRAME_POISONED` — sharing the process-wide flag across parallel
    /// `cargo test` threads would make this test's poisoning leak into an
    /// unrelated one.
    #[test]
    fn test_dw_1_6_panic_guarded_writer_passes_bytes_through_until_poisoned_then_discards() {
        let mut buf = Vec::new();
        let poisoned = AtomicBool::new(false);
        {
            let mut w = PanicGuardedWriter::new(&mut buf, &poisoned);
            w.write_all(b"before").unwrap();
            w.flush().unwrap();
        }
        assert_eq!(buf, b"before");

        poisoned.store(true, Ordering::Release);
        {
            let mut w = PanicGuardedWriter::new(&mut buf, &poisoned);
            w.write_all(b"after-poison").unwrap();
            w.flush().unwrap();
        }
        assert_eq!(
            buf, b"before",
            "a poisoned writer must silently discard, never append, to the wire underneath it"
        );
    }
}
