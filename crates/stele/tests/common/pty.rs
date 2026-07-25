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

use std::ffi::CStr;
use std::io::Write as _;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The exact bytes `terminal::RESTORE_SEQUENCE` must put on the wire:
/// end-synchronized-update, show-cursor, leave-alternate-screen.
pub const RESTORE: &[u8] = b"\x1b[?2026l\x1b[?25h\x1b[?1049l";
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

    /// Type at the viewer, as a user would.
    pub fn type_bytes(&self, bytes: &[u8]) {
        // SAFETY: writing a borrowed slice, bounded by its own length, to a
        // descriptor we own.
        let n = unsafe { libc::write(self.master.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
        assert_eq!(n, bytes.len() as isize, "short write to the pty master");
    }
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

    let pty = Pty::open();
    let canonical = pty.lflag();
    assert_ne!(
        canonical & (libc::ICANON | libc::ECHO),
        0,
        "the pty must start in canonical mode for this test to mean anything"
    );

    let mut cmd = Command::new(env!("CARGO_BIN_EXE_stele"));
    cmd.arg(&doc)
        .env("TERM", "xterm-256color")
        // Graphics are only enabled on Ghostty; either way stele enters the
        // alternate screen and raw mode, which is what is under test.
        .env("TERM_PROGRAM", "ghostty")
        .env_remove("TMUX")
        .stdin(Stdio::from(pty.slave_dup()))
        .stdout(Stdio::from(pty.slave_dup()))
        .stderr(Stdio::from(pty.slave_dup()));
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

    match exit {
        // SAFETY: `kill` on a pid we just spawned and have not reaped.
        Exit::Signal(signo) => {
            let killed = unsafe { libc::kill(child.id() as libc::pid_t, signo) };
            assert_eq!(killed, 0, "kill({signo}) failed");
        }
        Exit::Key(keystroke) => pty.type_bytes(keystroke),
    }

    let after = read_until(master, RESTORE, Duration::from_secs(10));
    let at = after.windows(RESTORE.len()).position(|w| w == RESTORE);
    assert!(
        at.is_some(),
        "{label} must put the restore sequence on the wire.\n  \
         expected: {RESTORE:?}\n  after {label} the wire was: {after:?}"
    );
    // Nothing may follow it: restoring is the last thing the process does.
    let tail = &after[at.unwrap() + RESTORE.len()..];
    assert_eq!(
        tail, b"",
        "the restore sequence must be the last bytes on the wire after {label}"
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

    let _ = std::fs::remove_file(&doc);
}
