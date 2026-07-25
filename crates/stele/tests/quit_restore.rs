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
//! exit, Ctrl-C must do byte for byte.
//!
//! Run: `cargo test -p stele --test quit_restore -- --nocapture`

#![cfg(unix)]
#![allow(unsafe_code)]

use std::ffi::CStr;
use std::io::Write;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The exact bytes `terminal::RESTORE_SEQUENCE` must put on the wire:
/// end-synchronized-update, show-cursor, leave-alternate-screen.
const RESTORE: &[u8] = b"\x1b[?2026l\x1b[?25h\x1b[?1049l";
const ENTER_ALT: &[u8] = b"\x1b[?1049h";

/// A pty pair. The child gets the slave as stdio; the test reads and writes
/// the master.
struct Pty {
    master: OwnedFd,
    slave: OwnedFd,
}

impl Pty {
    fn open() -> Pty {
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
    fn slave_dup(&self) -> OwnedFd {
        // SAFETY: `dup` of a descriptor we own; the result is handed straight
        // to `OwnedFd`, which closes it exactly once.
        unsafe {
            let fd = libc::dup(self.slave.as_raw_fd());
            assert!(fd >= 0, "dup failed");
            OwnedFd::from_raw_fd(fd)
        }
    }

    /// The tty's local-mode flags (`ICANON`, `ECHO`, …), read through the
    /// **master**: macOS revokes the slave device once the session leader
    /// exits, so `tcgetattr` on the slave fails with `ENOTTY` afterwards and
    /// the after-the-quit half of this test would be unaskable.
    fn lflag(&self) -> libc::tcflag_t {
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
    fn type_bytes(&self, bytes: &[u8]) {
        // SAFETY: writing a borrowed slice, bounded by its own length, to a
        // descriptor we own.
        let n = unsafe { libc::write(self.master.as_raw_fd(), bytes.as_ptr().cast(), bytes.len()) };
        assert_eq!(n, bytes.len() as isize, "short write to the pty master");
    }
}

/// Drain the master until `needle` shows up or `deadline` elapses. Returns
/// everything read.
fn read_until(master: RawFd, needle: &[u8], deadline: Duration) -> Vec<u8> {
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

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

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
    assert_quits_cleanly("q", b"q");
    assert_quits_cleanly("Ctrl-C", b"\x03");
}

fn assert_quits_cleanly(name: &str, keystroke: &[u8]) {
    let doc = std::env::temp_dir().join(format!(
        "stele-quit-{}-{}.md",
        std::process::id(),
        keystroke[0]
    ));
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
    // Only meaningful once raw mode is on: in canonical mode the line
    // discipline would turn `\x03` into a SIGINT and we would be testing the
    // signal path by accident.
    assert_eq!(
        pty.lflag() & (libc::ICANON | libc::ECHO),
        0,
        "stele must be in raw mode before we type at it — otherwise ISIG is \
         still set and {name} would arrive as a signal, not a key"
    );

    pty.type_bytes(keystroke);

    let after = read_until(master, RESTORE, Duration::from_secs(10));
    let at = after.windows(RESTORE.len()).position(|w| w == RESTORE);
    assert!(
        at.is_some(),
        "{name} must quit and put the restore sequence on the wire.\n  \
         expected: {RESTORE:?}\n  after the keystroke the wire was: {after:?}"
    );
    // Nothing may follow it: restoring is the last thing the process does.
    let tail = &after[at.unwrap() + RESTORE.len()..];
    assert_eq!(
        tail, b"",
        "the restore sequence must be the last bytes on the wire after {name}"
    );

    let status = child.wait().expect("wait");
    assert_eq!(
        status.code(),
        Some(0),
        "{name} must be a clean quit, exit 0: {status:?}"
    );
    assert_eq!(
        status.signal(),
        None,
        "{name} must not kill the process by signal: {status:?}"
    );

    assert_eq!(
        pty.lflag(),
        canonical,
        "{name} must put the tty's line discipline back exactly as it found it \
         — raw mode is state on the terminal and outlives the process"
    );

    let _ = std::fs::remove_file(&doc);
}
