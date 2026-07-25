//! The `CSI 16t` cell-geometry round trip, driven against a *fake* terminal.
//!
//! `terminal::query_cell_px` cannot be tested through its own front door: it
//! reads the process's stdin, and a `cargo test` process has no terminal there.
//! So before this file the only falsifiable part of the query was the pure
//! parser, and *"the bytes go out, the reply comes back, and the answer is
//! believed"* was an argument from reading the code — exactly the shape this
//! whole effort exists to replace. `terminal::csi_16t_over` takes the
//! descriptor pair explicitly so a pty with a scripted reply can stand in for
//! Ghostty.
//!
//! **What this does and does not establish.** It establishes that the query is
//! written, that a reply is read back off a real file descriptor through a real
//! `poll(2)`, that a well-formed report is believed, that a wrong-shaped or
//! implausible one is not, and that a terminal which never answers costs the
//! timeout and no more. It does **not** establish that Ghostty answers — that
//! is `docs/spikes/ghostty-caps.md` item 7's measurement (`\x1b[6;48;24t`,
//! 24x48 px), and the reply this file scripts is those exact bytes so the two
//! are pinned to each other.

#![cfg(unix)]
// Raw `posix_openpt`/`ioctl`/`tcsetattr` to build the fake terminal.
// `tests/hardening.rs` scopes its single-escape-hatch assertion to `src/**`
// deliberately: test code is not shipped in the binary.
#![allow(unsafe_code)]

use std::ffi::CStr;
use std::fs::File;
use std::io::Write as _;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::time::{Duration, Instant};

/// How long the tests give a non-answering terminal. Short on purpose: the
/// production constant is 250 ms and these tests assert on elapsed time, so a
/// smaller budget keeps the timeout test fast without changing what it proves.
const TEST_TIMEOUT: Duration = Duration::from_millis(120);

/// A pty pair standing in for a terminal. The *slave* is what the code under
/// test holds (it is the app's stdio); the *master* is the fake terminal.
struct FakeTerminal {
    master: OwnedFd,
    slave: OwnedFd,
}

impl FakeTerminal {
    fn open() -> FakeTerminal {
        // SAFETY: the POSIX pty-allocation dance. `ptsname` returns a pointer
        // into a libc-owned static buffer; it is copied into an owned `CString`
        // before any other libc call can clobber it. `cfmakeraw` +
        // `tcsetattr` then put the slave in raw mode, which is not cosmetic:
        // an XTWINOPS reply carries no newline, so in canonical mode `read`
        // would never return it — the same reason `query_cell_px` must run
        // after `enable_raw_mode`.
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

            let mut termios: libc::termios = std::mem::zeroed();
            assert_eq!(libc::tcgetattr(slave, &mut termios), 0, "tcgetattr failed");
            libc::cfmakeraw(&mut termios);
            assert_eq!(
                libc::tcsetattr(slave, libc::TCSANOW, &termios),
                0,
                "tcsetattr failed"
            );

            FakeTerminal {
                master: OwnedFd::from_raw_fd(master),
                slave: OwnedFd::from_raw_fd(slave),
            }
        }
    }

    /// Queue `reply` so it is already waiting when the code under test reads.
    ///
    /// Pre-loading rather than answering from a thread keeps the test
    /// deterministic — no race, no sleep. The write half of the round trip is
    /// asserted separately, from [`FakeTerminal::received`], so nothing is lost
    /// by not literally answering after the fact.
    fn preload_reply(&self, reply: &[u8]) {
        let mut master =
            std::mem::ManuallyDrop::new(unsafe { File::from_raw_fd(self.master.as_raw_fd()) });
        master
            .write_all(reply)
            .expect("write reply to the pty master");
        master.flush().expect("flush reply");
    }

    /// Everything the code under test wrote toward the terminal, read without
    /// blocking.
    fn received(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut chunk = [0u8; 512];
        loop {
            let mut pfd = libc::pollfd {
                fd: self.master.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one initialised `pollfd`, count 1, no wait.
            let ready = unsafe { libc::poll(&mut pfd, 1, 0) };
            if ready <= 0 {
                break;
            }
            // SAFETY: reading into a stack buffer we own, bounded by its length.
            let n = unsafe {
                libc::read(
                    self.master.as_raw_fd(),
                    chunk.as_mut_ptr().cast(),
                    chunk.len(),
                )
            };
            if n <= 0 {
                break;
            }
            out.extend_from_slice(&chunk[..n as usize]);
        }
        out
    }

    /// Runs the query against this terminal. Returns the answer and how long it
    /// took.
    fn run_query(&self) -> (Option<(u32, u32)>, Duration) {
        let mut app_out =
            std::mem::ManuallyDrop::new(unsafe { File::from_raw_fd(self.slave.as_raw_fd()) });
        let started = Instant::now();
        let answer = stele::terminal::csi_16t_over(self.slave.as_fd(), &mut *app_out, TEST_TIMEOUT);
        (answer, started.elapsed())
    }
}

/// The whole round trip, on the exact bytes the spike measured live Ghostty
/// sending. `24x48`, in `(width, height)` order.
#[test]
fn test_the_query_is_written_and_a_real_ghostty_reply_is_read_back_and_believed() {
    let term = FakeTerminal::open();
    term.preload_reply(b"\x1b[6;48;24t");
    let (answer, _) = term.run_query();

    assert_eq!(
        answer,
        Some((24, 48)),
        "the reply Ghostty was measured sending must resolve to 24x48 px per cell"
    );
    // The write half: the query really went out, and it is the sequence the
    // spike found answered — not `OSC 1337 ReportCellSize`, which it found
    // unanswered, and not `CSI 14t`.
    let sent = term.received();
    assert_eq!(
        sent,
        b"\x1b[16t",
        "the query on the wire must be exactly CSI 16t and nothing else, got {:?}",
        String::from_utf8_lossy(&sent)
    );
}

/// A terminal that never answers must cost the timeout and then fall through,
/// not hang. This is the case every pty in this test suite actually hits, and
/// the case a multiplexer hits in production.
#[test]
fn test_a_silent_terminal_costs_the_timeout_and_returns_no_answer() {
    let term = FakeTerminal::open();
    let (answer, elapsed) = term.run_query();

    assert_eq!(
        answer, None,
        "silence must not be resolved into a cell size"
    );
    assert!(
        elapsed >= TEST_TIMEOUT,
        "the query must actually wait for its deadline before giving up, waited {elapsed:?}"
    );
    assert!(
        elapsed < TEST_TIMEOUT * 8,
        "the query must not wait appreciably past its deadline, waited {elapsed:?}"
    );
    // Still wrote the query: a silent terminal is not a reason to have skipped
    // asking.
    assert_eq!(term.received(), b"\x1b[16t");
}

/// A different XTWINOPS report is not a cell size. `CSI 8 ; 42 ; 155 t` is the
/// *text area in cells* — a perfectly plausible-looking 155x42 that would
/// silently mis-size every image and formula in the document if believed.
#[test]
fn test_a_neighbouring_xtwinops_report_is_not_believed() {
    let term = FakeTerminal::open();
    term.preload_reply(b"\x1b[8;42;155t");
    let (answer, _) = term.run_query();
    assert_eq!(answer, None);
}

/// An answer outside the plausible range is refused rather than clamped, so it
/// cannot be recorded as "the terminal told us" when it is really a constant.
#[test]
fn test_an_implausible_reply_is_refused_over_the_wire_too() {
    for reply in [
        &b"\x1b[6;1;1t"[..],      // one pixel per cell
        &b"\x1b[6;99999;7t"[..],  // absurd height
        &b"\x1b[6;48;0t"[..],     // zero width
        &b"\x1b[6;forty;24t"[..], // not a number
        &b"\x1b[6;48;24"[..],     // unterminated
        &b"garbage, not escapes"[..],
    ] {
        let term = FakeTerminal::open();
        term.preload_reply(reply);
        let (answer, _) = term.run_query();
        assert_eq!(
            answer,
            None,
            "must not believe {:?}",
            String::from_utf8_lossy(reply)
        );
    }
}

/// The reply need not arrive alone, or first. A DA1 answer and a stray
/// keystroke ahead of it must not stop the real report being found — bailing on
/// the first non-matching read would strand it in crossterm's input stream.
#[test]
fn test_the_reply_is_found_behind_unrelated_terminal_chatter() {
    let term = FakeTerminal::open();
    term.preload_reply(b"\x1b[?62;1;6c\x1b[A\x1b[6;40;20t");
    let (answer, _) = term.run_query();
    assert_eq!(answer, Some((20, 40)));
}

/// The same claim, but forcing the read loop to actually go round twice.
///
/// The test above does not: a pre-loaded reply arrives in one `read`, the parse
/// succeeds on the first pass, and the "keep reading" branch is never reached —
/// verified by mutation, deleting that branch left it green. So this case sends
/// more chatter than the 256-byte read buffer can take in one call, which puts
/// the report beyond the first read by construction. Bailing after a
/// non-matching read now fails here.
#[test]
fn test_a_reply_beyond_the_first_read_is_still_found() {
    let term = FakeTerminal::open();
    // 100 x 3 bytes of cursor-up: no `t`, so no read of it can parse, and 300
    // bytes is comfortably past the 256-byte chunk the query reads into.
    let mut chatter = b"\x1b[A".repeat(100);
    assert!(chatter.len() > 256, "the chatter must outrun one read");
    chatter.extend_from_slice(b"\x1b[6;40;20t");
    term.preload_reply(&chatter);

    let (answer, _) = term.run_query();
    assert_eq!(
        answer,
        Some((20, 40)),
        "a report that lands after the first read must still be found — \
         giving up on it strands the real reply in crossterm's input stream"
    );
}
