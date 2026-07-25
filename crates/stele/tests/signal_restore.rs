//! DW-5.1 on the one path `Drop` cannot reach: a fatal signal.
//!
//! `TerminalGuard`'s `Drop` and the panic hook are both unit-tested against a
//! `Write` seam, but neither runs when the kernel kills the process. This
//! test drives the **real binary** on a **real pty** and asserts the actual
//! bytes on the wire plus the tty's own line discipline afterwards — raw mode
//! is state on the terminal, not on the process, so it outlives `stele` and
//! only a real tty can show whether it was put back.
//!
//! Run: `cargo test -p stele --test signal_restore -- --nocapture`

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
const HIDE_CURSOR: &[u8] = b"\x1b[?25l";

/// A pty pair. The child gets the slave as stdio; the test reads the master.
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

    /// The tty's local-mode flags (`ICANON`, `ECHO`, …).
    ///
    /// Read through the **master**, not the slave: once the child (the
    /// session leader) exits, macOS revokes the slave device and every
    /// `tcgetattr` on it fails with `ENOTTY`, which would make the
    /// after-the-signal half of this test unaskable. The master is not
    /// revoked and reports the same line discipline.
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
        assert_signal_restores_the_terminal(signo);
    }
}

fn assert_signal_restores_the_terminal(signo: libc::c_int) {
    let doc = std::env::temp_dir().join(format!("stele-signal-{}-{signo}.md", std::process::id()));
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
    assert_eq!(
        pty.lflag() & (libc::ICANON | libc::ECHO),
        0,
        "stele should have put the tty in raw mode before we signal it"
    );

    // SAFETY: `kill` on a pid we just spawned and have not reaped.
    let killed = unsafe { libc::kill(child.id() as libc::pid_t, signo) };
    assert_eq!(killed, 0, "kill({signo}) failed");

    let after = read_until(master, RESTORE, Duration::from_secs(10));
    let at = after.windows(RESTORE.len()).position(|w| w == RESTORE);
    assert!(
        at.is_some(),
        "signal {signo} must put the restore sequence on the wire.\n  \
         expected: {RESTORE:?}\n  after the signal the wire was: {after:?}"
    );
    // Nothing may follow it: restoring is the last thing the process does.
    let tail = &after[at.unwrap() + RESTORE.len()..];
    assert_eq!(
        tail, b"",
        "the restore sequence must be the last bytes on the wire"
    );

    let status = child.wait().expect("wait");
    assert_eq!(
        status.signal(),
        Some(signo),
        "the process must still die *from* signal {signo}, not exit normally: {status:?}"
    );

    assert_eq!(
        pty.lflag(),
        canonical,
        "signal {signo} must put the tty's line discipline back exactly as it found it \
         — raw mode is state on the terminal and outlives the process"
    );

    let _ = std::fs::remove_file(&doc);
}
