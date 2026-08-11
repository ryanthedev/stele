//! The real-pty harness: spawn the real `stele` binary on a real terminal,
//! end it, and ask the *tty* what state it was left in.
//!
//! This is the one thing no unit test can reach. Raw mode and the alternate
//! screen are state on the **terminal**, not on the process, so they outlive
//! `stele` — a `Write`-seam test can prove the bytes were composed, and a
//! `Drop` test can prove they were sent on the paths `Drop` runs on, but only a
//! live tty can prove the line discipline came back after the kernel killed the
//! process outright. DW-5.1 is that claim.
//!
//! Two byte-identical copies of this harness used to live in
//! `tests/quit_restore.rs` and `tests/signal_restore.rs` — the whole
//! `posix_openpt`/`grantpt`/`unlockpt`/`ptsname` dance, twice, in an
//! `allow(unsafe_code)` context, so any portability fix had to be made twice.
//! The two ~85-line test bodies differed in four substantive lines: how the
//! viewer is ended, and what the exit status must then look like. That
//! difference is [`Exit`]; everything else is here.
//!
//! `crates/stele/tests/hardening.rs` asserts the `allow(unsafe_code)` escape
//! hatch appears exactly once under `src/**`. It scopes itself to `src/**`
//! deliberately: this file needs raw `posix_openpt`/`ioctl`/`kill`, and test
//! code is not shipped in the binary. Moving the harness here does not widen
//! the shipped unsafe surface by one line.
#![allow(unsafe_code)]

use std::ffi::{CStr, OsStr};
use std::io::Write as _;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

/// The exact bytes `terminal::RESTORE_SEQUENCE` must put on the wire:
/// end-synchronized-update, disable mouse reporting (SGR then normal
/// tracking), show-cursor, leave-alternate-screen.
///
/// The mouse pair is inside the restore rather than on a separate exit path
/// because mouse capture is state on the *terminal* (DW-6.6): a `kill -TERM`
/// that skipped it would leave a shell whose every click writes escape bytes
/// into the prompt. `assert_restores_the_terminal` compares the ending's whole
/// wire against this by **equality**, so this constant is what pins the order.
pub const RESTORE: &[u8] = b"\x1b[?2026l\x1b[?1006l\x1b[?1000l\x1b[?25h\x1b[?1049l";
pub const ENTER_ALT: &[u8] = b"\x1b[?1049h";
pub const HIDE_CURSOR: &[u8] = b"\x1b[?25l";

/// How the viewer is ended, and therefore what its exit status must be.
///
/// The two are one enum because they are two halves of one guarantee: a fatal
/// signal bypasses `Drop`, a Ctrl-C **keystroke** never becomes a signal at all
/// (raw mode clears `ISIG`, so `0x03` arrives as an ordinary byte and only key
/// handling can act on it), and both must end at byte-identical restores.
pub enum Exit {
    /// `kill -N` from outside: the process must still die *from* the signal.
    Signal(libc::c_int),
    /// Bytes typed at the viewer: the process must exit 0, by no signal.
    Key(&'static [u8]),
}

/// A pty pair. The child gets the slave as stdio; the test reads and writes
/// the master.
pub struct Pty {
    master: OwnedFd,
    slave: OwnedFd,
}

impl Pty {
    pub fn open() -> Pty {
        // SAFETY: the POSIX pty-allocation dance. `ptsname` returns a pointer
        // into a libc-owned static buffer; it is copied into an owned
        // `CString` before any other libc call can clobber it.
        unsafe {
            let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
            assert!(master >= 0, "posix_openpt failed");
            assert_eq!(libc::grantpt(master), 0, "grantpt failed");
            assert_eq!(libc::unlockpt(master), 0, "unlockpt failed");

            let name_ptr = libc::ptsname(master);
            assert!(!name_ptr.is_null(), "ptsname failed");
            let name = CStr::from_ptr(name_ptr).to_owned();

            let slave = libc::open(name.as_ptr(), libc::O_RDWR | libc::O_NOCTTY);
            assert!(slave >= 0, "open({name:?}) failed");

            // A deterministic viewport: an unsized pty reports 0x0 and the
            // painter has nothing to lay out.
            let size = libc::winsize {
                ws_row: 24,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            assert_eq!(
                libc::ioctl(master, libc::TIOCSWINSZ as _, &size),
                0,
                "TIOCSWINSZ failed"
            );

            Pty {
                master: OwnedFd::from_raw_fd(master),
                slave: OwnedFd::from_raw_fd(slave),
            }
        }
    }

    /// The master descriptor, for a test that needs to drive [`read_until`]
    /// (or its own bespoke read loop) directly rather than through
    /// [`assert_restores_the_terminal`].
    pub fn master_fd(&self) -> RawFd {
        self.master.as_raw_fd()
    }

    /// A fresh descriptor for the slave, for one of the child's stdio slots.
    pub fn slave_dup(&self) -> OwnedFd {
        // SAFETY: `dup` of a descriptor we own; the result is handed straight
        // to `OwnedFd`, which closes it exactly once.
        unsafe {
            let fd = libc::dup(self.slave.as_raw_fd());
            assert!(fd >= 0, "dup failed");
            OwnedFd::from_raw_fd(fd)
        }
    }

    /// The tty's local-mode flags (`ICANON`, `ECHO`, …).
    ///
    /// Read through the **master**, not the slave: once the child (the session
    /// leader) exits, macOS revokes the slave device and every `tcgetattr` on
    /// it fails with `ENOTTY`, which would make the after-the-exit half of
    /// these tests unaskable. The master is not revoked and reports the same
    /// line discipline.
    pub fn lflag(&self) -> libc::tcflag_t {
        // SAFETY: `tcgetattr` fills a `termios` we own, through a valid fd.
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            assert_eq!(
                libc::tcgetattr(self.master.as_raw_fd(), &mut t),
                0,
                "tcgetattr on the pty master failed"
            );
            t.c_lflag
        }
    }

    /// Changes the terminal's window size, which is what a window drag does.
    ///
    /// `TIOCSWINSZ` on the master makes the kernel deliver a real `SIGWINCH`
    /// to the slave's foreground process group — so this drives the viewer's
    /// resize path end to end (signal → crossterm's signal-hook source →
    /// `Event::Resize`), rather than faking an event the viewer would never
    /// receive that way. The size must actually *change* for the signal to be
    /// sent, so callers alternate between two sizes.
    pub fn resize(&self, rows: u16, cols: u16) {
        let size = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: `TIOCSWINSZ` against a descriptor we own, with a fully
        // initialised `winsize`.
        let rc = unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ as _, &size) };
        assert_eq!(
            rc,
            0,
            "TIOCSWINSZ failed: {}",
            std::io::Error::last_os_error()
        );
    }

    /// Type at the viewer, as a user would.
    pub fn type_bytes(&self, bytes: &[u8]) {
        // SAFETY: writing a borrowed slice, bounded by its own length, to a
        // descriptor we own.
        let n = unsafe { libc::write(self.master.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
        assert_eq!(
            n,
            bytes.len() as isize,
            "short write to the pty master: {}",
            std::io::Error::last_os_error()
        );
    }
}

/// Where the spawned viewer's **stdin** comes from.
///
/// The distinction is the whole of DW-2.1. With `Tty`, stdin is the terminal
/// and crossterm reads keys straight off it. With `Pipe`, stdin is a pipe
/// carrying the document (`stele -`), so crossterm has to fall back to
/// `/dev/tty` for keys — which only resolves because the child calls `setsid`
/// and claims the pty slave as its controlling terminal below.
pub enum ChildStdin<'a> {
    Tty,
    Pipe(&'a [u8]),
}

/// Whether the spawned viewer has the graphics path switched on.
///
/// stele gates images on `TERM_PROGRAM=ghostty`, so removing that variable
/// does not merely hide pictures — it makes `ImageSizer`, `GfxMediaSink` and
/// every path underneath them **unreachable from the test suite**. That is
/// not a hypothetical cost: a security review found an image-path hang
/// (`open(2)` on a FIFO named by `![x](pipe.png)`) that four rounds of gates
/// had missed, because every pty test in this suite ran with graphics off and
/// the media interface never executed at all.
///
/// [`Graphics::Off`] therefore stays the default — it is what keeps the
/// text-oriented tests asserting on frames that contain no base64 APC
/// payloads, and it skips the 250 ms startup cell-geometry query the pty
/// never answers — but [`Graphics::On`] exists so the media path is reachable
/// and is exercised by at least one test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Graphics {
    /// `TERM_PROGRAM` removed: no images, no startup geometry query.
    Off,
    /// `TERM_PROGRAM=ghostty`: `ImageSizer` and `GfxMediaSink` are live.
    On,
}

/// Spawns the real binary on `pty` with `args`, its stdout and stderr on the
/// pty slave and its stdin as `stdin` says. Graphics off — see
/// [`spawn_viewer_with`] when the media path is the thing under test.
pub fn spawn_viewer(pty: &Pty, args: &[&OsStr], stdin: ChildStdin<'_>) -> ViewerProcess {
    spawn_viewer_with(pty, args, stdin, Graphics::Off)
}

/// [`spawn_viewer`] with an explicit [`Graphics`] setting.
///
/// Environment is fixed rather than inherited so a developer's own exports
/// cannot silently change what these tests see on the wire.
pub fn spawn_viewer_with(
    pty: &Pty,
    args: &[&OsStr],
    stdin: ChildStdin<'_>,
    graphics: Graphics,
) -> ViewerProcess {
    spawn_viewer_in(pty, args, stdin, graphics, None)
}

/// [`spawn_viewer_with`] with an explicit working directory — for `stele`
/// invoked with no document argument (Phase 3, DW-3.12): what the explorer
/// opens on depends on the process's cwd, which every other test here leaves
/// at the test runner's own, and which none of them needed to name until now.
pub fn spawn_viewer_in_dir(
    pty: &Pty,
    args: &[&OsStr],
    stdin: ChildStdin<'_>,
    graphics: Graphics,
    cwd: &std::path::Path,
) -> ViewerProcess {
    spawn_viewer_in(pty, args, stdin, graphics, Some(cwd))
}

fn spawn_viewer_in(
    pty: &Pty,
    args: &[&OsStr],
    stdin: ChildStdin<'_>,
    graphics: Graphics,
    cwd: Option<&std::path::Path>,
) -> ViewerProcess {
    let mut cmd = super::viewer_command();
    cmd.args(args)
        .env("TERM", "xterm-256color")
        .env_remove("TMUX");
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    match graphics {
        Graphics::Off => {
            cmd.env_remove("TERM_PROGRAM");
        }
        Graphics::On => {
            cmd.env("TERM_PROGRAM", "ghostty");
        }
    }
    cmd
        // Truecolor is the default unless `NO_COLOR` is set, and a developer
        // who exports it would otherwise silently change what these tests see
        // on the wire.
        .env_remove("NO_COLOR")
        .stdout(Stdio::from(pty.slave_dup()))
        .stderr(Stdio::from(pty.slave_dup()));

    // Which descriptor is the pty, and therefore which one `TIOCSCTTY` must be
    // asked on: fd 0 when stdin is the terminal, fd 1 (stdout) when stdin has
    // been taken over by the document pipe. Getting this wrong is not a subtle
    // failure — `/dev/tty` would resolve to nothing and the viewer could never
    // read a key.
    let ctty_fd = match stdin {
        ChildStdin::Tty => {
            cmd.stdin(Stdio::from(pty.slave_dup()));
            0
        }
        ChildStdin::Pipe(_) => {
            cmd.stdin(Stdio::piped());
            1
        }
    };
    // SAFETY: `pre_exec` runs between fork and exec, so it may only call
    // async-signal-safe functions. `setsid` and `ioctl(TIOCSCTTY)` both are;
    // together they make the pty the child's controlling terminal, which is
    // what lets crossterm open `/dev/tty`.
    unsafe {
        cmd.pre_exec(move || {
            libc::setsid();
            libc::ioctl(ctty_fd, libc::TIOCSCTTY as _, 0);
            Ok(())
        });
    }

    let mut child = cmd.spawn().expect("spawn stele");
    if let ChildStdin::Pipe(bytes) = stdin {
        let mut pipe = child.stdin.take().expect("piped stdin");
        pipe.write_all(bytes).expect("write document to stdin");
        // Dropping the writer closes the pipe: without the EOF, a viewer
        // reading stdin to end would wait forever and the test would hang
        // rather than fail.
        drop(pipe);
    }
    ViewerProcess(child)
}

/// A spawned viewer that is **killed and reaped when it goes out of scope**.
///
/// `std::process::Child` deliberately does not kill on drop, so every one of
/// these tests that failed an assertion left a live `stele` holding a pty
/// open — nine orphans were counted on one machine after a few review runs.
/// A test that panics is exactly when the child is least likely to have been
/// quit cleanly, so the cleanup has to be on the unwinding path.
///
/// Derefs to [`Child`], so `child.wait()` and `&mut child` keep working at
/// every existing call site.
pub struct ViewerProcess(Child);

impl std::ops::Deref for ViewerProcess {
    type Target = Child;

    fn deref(&self) -> &Child {
        &self.0
    }
}

impl std::ops::DerefMut for ViewerProcess {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}

impl Drop for ViewerProcess {
    fn drop(&mut self) {
        // Both calls are best-effort and both are needed: `kill` is a no-op
        // error once the child has exited, and `wait` is what actually reaps
        // the zombie — including in the ordinary case where the test already
        // waited, since `Child::wait` caches its answer.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The bytes that close a frame. Every repaint is wrapped in a mode-2026
/// synchronized update, so this is the one reliable "one whole frame has
/// arrived" marker on the wire.
pub const FRAME_END: &[u8] = b"\x1b[?2026l";

/// Reads exactly one frame off `master`, as a lossy string ready for
/// [`super::render::render_row`].
///
/// Only meaningful directly after [`drain_to_quiet`]: with an earlier frame
/// still buffered this returns that one instead.
pub fn read_one_frame(master: RawFd, deadline: Duration) -> String {
    let bytes = read_until(master, FRAME_END, deadline);
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Reads whatever arrives on `master` for `window`, without waiting for any
/// particular content. For a test that must keep the wire drained while it does
/// something else — feeding keys, say — and then inspect what came back.
pub fn drain_for(master: RawFd, window: Duration) -> Vec<u8> {
    let deadline = Instant::now() + window;
    let mut buf = Vec::new();
    while Instant::now() < deadline {
        let mut pfd = libc::pollfd {
            fd: master,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one initialised `pollfd`, count 1, 10 ms timeout.
        let ready = unsafe { libc::poll(&mut pfd, 1, 10) };
        if ready <= 0 {
            continue;
        }
        let mut chunk = [0u8; 8192];
        // SAFETY: reading into a stack buffer we own, bounded by its length.
        let n = unsafe { libc::read(master, chunk.as_mut_ptr().cast(), chunk.len()) };
        if n > 0 {
            buf.extend_from_slice(&chunk[..n as usize]);
        } else {
            break;
        }
    }
    buf
}

/// Drain the master until `needle` shows up or `deadline` elapses. Returns
/// everything read.
pub fn read_until(master: RawFd, needle: &[u8], deadline: Duration) -> Vec<u8> {
    let start = Instant::now();
    let mut buf: Vec<u8> = Vec::new();
    while start.elapsed() < deadline {
        if contains(&buf, needle) {
            break;
        }
        let mut pfd = libc::pollfd {
            fd: master,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one initialised `pollfd`, count 1, 50 ms timeout.
        let ready = unsafe { libc::poll(&mut pfd, 1, 50) };
        if ready <= 0 {
            continue;
        }
        let mut chunk = [0u8; 8192];
        // SAFETY: reading into a stack buffer we own, bounded by its length.
        let n = unsafe { libc::read(master, chunk.as_mut_ptr().cast(), chunk.len()) };
        if n > 0 {
            buf.extend_from_slice(&chunk[..n as usize]);
        } else {
            break;
        }
    }
    buf
}

/// Read and discard everything already on the wire, then wait for it to fall
/// quiet.
///
/// Load-bearing, and proved so by mutation. An ordinary frame ends with the
/// synchronized-update end `\x1b[?2026l`, and `RESTORE_SEQUENCE` *begins* with
/// the same bytes — deliberately, because a global mode left set outlives the
/// alternate screen. With the startup frame still sitting in the buffer, a
/// wire reading `…?2026l` + `?25h?1049l` contains the full restore sequence as
/// a substring even when the restore itself never emitted the `?2026l` at all:
/// deleting it from `RESTORE_SEQUENCE` left the keystroke half of this harness
/// green. Draining first is what makes the assertion "this ending emitted the
/// restore sequence" instead of "these bytes appear somewhere on the wire".
pub fn drain_to_quiet(master: RawFd) {
    drain_quiet(master);
}

fn drain_quiet(master: RawFd) {
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut idle = 0;
    while Instant::now() < deadline && idle < 3 {
        let mut pfd = libc::pollfd {
            fd: master,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one initialised `pollfd`, count 1, 50 ms timeout.
        let ready = unsafe { libc::poll(&mut pfd, 1, 50) };
        if ready <= 0 {
            idle += 1;
            continue;
        }
        idle = 0;
        let mut chunk = [0u8; 8192];
        // SAFETY: reading into a stack buffer we own, bounded by its length.
        let n = unsafe { libc::read(master, chunk.as_mut_ptr().cast(), chunk.len()) };
        if n <= 0 {
            break;
        }
    }
}

pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Runs the real binary on a real pty, ends it with `exit`, and asserts the
/// full restore contract: the exact `RESTORE` bytes on the wire, nothing after
/// them, the exit status `exit` implies, and the tty's line discipline back
/// bit-for-bit as it was found.
///
/// `label` names the ending in failure messages ("Ctrl-C", "SIGHUP").
pub fn assert_restores_the_terminal(label: &str, exit: Exit) {
    let tag: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let doc = std::env::temp_dir().join(format!("stele-restore-{}-{tag}.md", std::process::id()));
    std::fs::File::create(&doc)
        .and_then(|mut f| f.write_all(b"# hi\n\nbody text\n"))
        .expect("write fixture");

    assert_restores_the_terminal_in(label, exit, &[doc.as_os_str()], None);

    let _ = std::fs::remove_file(&doc);
}

/// [`assert_restores_the_terminal`] with explicit `args` and an optional
/// `cwd`, for a launch that opens the explorer instead of a named document
/// (Phase 3, DW-3.12: bare `stele`, no document argument at all).
///
/// The rest of the contract is identical: raw mode and the alternate screen
/// must be entered, `exit` must end the process the way it says, and the
/// wire must carry exactly [`RESTORE`] and nothing else once it does.
pub fn assert_restores_the_terminal_in(
    label: &str,
    exit: Exit,
    args: &[&OsStr],
    cwd: Option<&std::path::Path>,
) {
    let pty = Pty::open();
    let canonical = pty.lflag();
    assert_ne!(
        canonical & (libc::ICANON | libc::ECHO),
        0,
        "the pty must start in canonical mode for this test to mean anything"
    );

    let mut cmd = super::viewer_command();
    cmd.args(args)
        .env("TERM", "xterm-256color")
        // Graphics are only enabled on Ghostty; either way stele enters the
        // alternate screen and raw mode, which is what is under test.
        .env("TERM_PROGRAM", "ghostty")
        .env_remove("TMUX")
        .stdin(Stdio::from(pty.slave_dup()))
        .stdout(Stdio::from(pty.slave_dup()))
        .stderr(Stdio::from(pty.slave_dup()));
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    // SAFETY: `pre_exec` runs between fork and exec, so it may only call
    // async-signal-safe functions. `setsid` and `ioctl(TIOCSCTTY)` both are;
    // together they make the pty the child's controlling terminal, which is
    // what lets crossterm open `/dev/tty`.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            libc::ioctl(0, libc::TIOCSCTTY as _, 0);
            Ok(())
        });
    }
    let mut child = cmd.spawn().expect("spawn stele");

    let master = pty.master.as_raw_fd();
    let before = read_until(master, ENTER_ALT, Duration::from_secs(10));
    assert!(
        contains(&before, ENTER_ALT),
        "stele never entered the alternate screen; wire was {before:?}"
    );
    assert!(
        contains(&before, HIDE_CURSOR),
        "stele never hid the cursor; wire was {before:?}"
    );
    // Raw mode has to be on *before* the ending is delivered. For a signal
    // that is the precondition of the claim (the terminal was really left
    // dirty). For a keystroke it is what makes the test about keys at all:
    // raw mode clears `ISIG`, so with it off the line discipline would turn
    // `\x03` into a SIGINT and we would be testing the signal path by
    // accident.
    assert_eq!(
        pty.lflag() & (libc::ICANON | libc::ECHO),
        0,
        "stele must be in raw mode before {label} is delivered"
    );

    // Everything the startup frame put on the wire is still buffered; take it
    // out of the way so the assertion below is about the *ending* alone.
    drain_quiet(master);

    match exit {
        // SAFETY: `kill` on a pid we just spawned and have not reaped.
        Exit::Signal(signo) => {
            let killed = unsafe { libc::kill(child.id() as libc::pid_t, signo) };
            assert_eq!(killed, 0, "kill({signo}) failed");
        }
        Exit::Key(keystroke) => pty.type_bytes(keystroke),
    }

    let after = read_until(master, RESTORE, Duration::from_secs(10));
    // Equality, not "contains", and not "ends with": after the wire was drained
    // the *only* bytes this ending may produce are the restore sequence — in
    // that order, with nothing before it and nothing after it. Restoring is the
    // last thing the process does.
    assert_eq!(
        after, RESTORE,
        "{label} must put exactly the restore sequence on the wire and nothing \
         else.\n  expected: {RESTORE:?}\n  got:      {after:?}"
    );

    let status = child.wait().expect("wait");
    match exit {
        Exit::Signal(signo) => assert_eq!(
            status.signal(),
            Some(signo),
            "the process must still die *from* signal {signo}, not exit normally: {status:?}"
        ),
        Exit::Key(_) => {
            assert_eq!(
                status.code(),
                Some(0),
                "{label} must be a clean quit, exit 0: {status:?}"
            );
            assert_eq!(
                status.signal(),
                None,
                "{label} must not kill the process by signal: {status:?}"
            );
        }
    }

    assert_eq!(
        pty.lflag(),
        canonical,
        "{label} must put the tty's line discipline back exactly as it found it \
         — raw mode is state on the terminal and outlives the process"
    );
}
