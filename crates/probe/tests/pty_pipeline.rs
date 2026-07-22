//! Deterministic, CI-safe tests of `Probe`'s read/write/timeout behavior
//! over a real POSIX pty pair (`posix_openpt`/`grantpt`/`unlockpt`). No
//! Ghostty involved — these exercise the crate's own defensive-timeout
//! guarantee, which is the one edge case this phase's skill assignment
//! (cc-defensive-programming) calls out by name: "probe read timeouts (a
//! non-answering terminal must not hang the harness)."

use std::ffi::CStr;
use std::os::fd::RawFd;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use probe::Probe;

/// `ptsname(3)` returns a pointer into a static, process-wide buffer — it
/// is explicitly not thread-safe. cargo test runs `#[test]` functions
/// concurrently on separate OS threads by default, and two threads racing
/// through `open_pty_pair()` at once will otherwise read each other's
/// result and open the wrong slave path, silently pairing one test's probe
/// with another test's pty and hanging both (this was diagnosed the hard
/// way: every test passed in isolation, but hung when run together). Guard
/// the whole non-reentrant sequence with a mutex rather than reaching for
/// `ptsname_r` — this is test-only code, so simplicity and portability win
/// over avoiding one short-lived lock.
static PTSNAME_LOCK: Mutex<()> = Mutex::new(());

/// Opens a fresh POSIX pty pair. Returns `(master_fd, slave_fd)` — the test
/// plays "the terminal" on `master`, `Probe` plays "the program attached to
/// the tty" on `slave`, mirroring the real Ghostty case where the probe
/// binary is the process attached to a Ghostty-allocated pty slave.
fn open_pty_pair() -> (RawFd, RawFd) {
    let _guard = PTSNAME_LOCK.lock().unwrap();
    unsafe {
        let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        assert!(
            master >= 0,
            "posix_openpt failed: {}",
            std::io::Error::last_os_error()
        );
        assert_eq!(libc::grantpt(master), 0, "grantpt failed");
        assert_eq!(libc::unlockpt(master), 0, "unlockpt failed");

        let name_ptr = libc::ptsname(master);
        assert!(!name_ptr.is_null(), "ptsname returned null");
        let name = CStr::from_ptr(name_ptr).to_owned();

        let slave = libc::open(name.as_ptr(), libc::O_RDWR | libc::O_NOCTTY);
        assert!(
            slave >= 0,
            "open(slave) failed: {}",
            std::io::Error::last_os_error()
        );

        // A freshly opened pty defaults to canonical (line-buffered) mode:
        // the slave's read(2) would block until a newline crosses the line
        // discipline, which every escape-sequence reply in these tests
        // lacks. `Probe::open`'s real-world counterpart gets this from
        // crossterm putting the *process's controlling terminal* in raw
        // mode; a bare pty pair created here isn't that, so the test rig
        // has to set it directly.
        let mut term: libc::termios = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(slave, &mut term), 0, "tcgetattr failed");
        libc::cfmakeraw(&mut term);
        assert_eq!(
            libc::tcsetattr(slave, libc::TCSANOW, &term),
            0,
            "tcsetattr failed"
        );

        (master, slave)
    }
}

/// The defensive guarantee under test: a query the far end never answers
/// returns `None` — and does so close to the requested timeout, not after
/// an unbounded wait. This is the harness-hang scenario the plan's edge
/// cases explicitly name.
#[test]
fn query_times_out_instead_of_hanging_when_nothing_answers() {
    let (_master, slave) = open_pty_pair();
    let mut probe = Probe::from_raw_fds_for_test(slave, slave);

    let budget = Duration::from_millis(300);
    let start = Instant::now();
    let result = probe.query(b"\x1b[6n", budget);
    let elapsed = start.elapsed();

    assert_eq!(
        result, None,
        "nothing was written on the master side; a reply should be impossible"
    );
    assert!(
        elapsed < budget + Duration::from_millis(500),
        "query() took {elapsed:?} against a {budget:?} budget — looks like it hung rather than \
         timing out"
    );
}

/// `cursor_pos`'s documented `(0, 0)` sentinel on non-response — same
/// non-hanging guarantee, through the pinned infallible signature.
#[test]
fn cursor_pos_falls_back_to_sentinel_instead_of_hanging() {
    let (_master, slave) = open_pty_pair();
    let mut probe = Probe::from_raw_fds_for_test(slave, slave);

    let start = Instant::now();
    let pos = probe.cursor_pos();
    let elapsed = start.elapsed();

    assert_eq!(pos, (0, 0));
    assert!(
        elapsed < Duration::from_secs(2),
        "cursor_pos() took {elapsed:?} — should bail out at its internal ~800ms budget"
    );
}

/// The positive path: when the far end *does* answer, `query` returns the
/// real bytes, and does so quickly rather than waiting out the timeout —
/// proving the quiet-window logic doesn't just get lucky on the timeout
/// case above.
#[test]
fn query_returns_a_real_reply_promptly_when_the_other_end_answers() {
    let (master, slave) = open_pty_pair();
    let mut probe = Probe::from_raw_fds_for_test(slave, slave);

    // Play "the terminal": once the query arrives on master, write back a
    // canned CSI 6n reply.
    let responder = std::thread::spawn(move || {
        let mut buf = [0u8; 64];
        // SAFETY: `buf` is a valid, correctly-sized stack buffer for the
        // duration of the call; `master` is a valid, open fd owned by this
        // test.
        let n = unsafe { libc::read(master, buf.as_mut_ptr().cast(), buf.len()) };
        assert!(n > 0, "expected the probe's query to arrive on master");
        let reply = b"\x1b[7;3R";
        // SAFETY: `reply` is a valid slice for its own length.
        unsafe {
            libc::write(master, reply.as_ptr().cast(), reply.len());
        }
    });

    let budget = Duration::from_secs(2);
    let start = Instant::now();
    let result = probe.query(b"\x1b[6n", budget);
    let elapsed = start.elapsed();

    responder.join().unwrap();

    assert_eq!(result, Some(b"\x1b[7;3R".to_vec()));
    assert!(
        elapsed < Duration::from_millis(500),
        "a real reply arrived almost immediately, but query() took {elapsed:?} — the quiet-window \
         logic should not wait anywhere near the full {budget:?} timeout once bytes are flowing"
    );
}

/// `measured_width` end to end: two `cursor_pos` round trips bracketing a
/// write, over the same fake-terminal responder pattern.
///
/// The responder is deliberately stream-aware rather than one-read-per-
/// write: a pty is a byte stream, not a message channel, so nothing
/// guarantees `probe`'s two writes (the `CSI 6n` query and the sample
/// string) arrive as two separate `read()`s on `master` — the kernel is
/// free to coalesce them into one. An earlier version of this test assumed
/// a 1:1 read/write correspondence and deadlocked under that coalescing
/// (diagnosed by running each test alone vs. together). Scanning an
/// accumulating buffer for the query marker, rather than counting reads,
/// is correct regardless of how the bytes happen to be chunked.
#[test]
fn measured_width_computes_the_delta_between_two_cursor_reports() {
    let (master, slave) = open_pty_pair();
    let mut probe = Probe::from_raw_fds_for_test(slave, slave);

    let responder = std::thread::spawn(move || {
        let mut acc = Vec::new();
        let mut chunk = [0u8; 256];
        let mut queries_seen = 0u8;
        loop {
            let n = unsafe { libc::read(master, chunk.as_mut_ptr().cast(), chunk.len()) };
            assert!(n > 0, "responder: read from master failed or hit EOF");
            acc.extend_from_slice(&chunk[..n as usize]);
            while let Some(pos) = acc.windows(4).position(|w| w == b"\x1b[6n") {
                acc.drain(..pos + 4);
                queries_seen += 1;
                let reply: &[u8] = if queries_seen == 1 {
                    b"\x1b[1;1R" // before the write: column 1
                } else {
                    b"\x1b[1;3R" // after the write: column 3 (a 2-cell grapheme)
                };
                unsafe { libc::write(master, reply.as_ptr().cast(), reply.len()) };
                if queries_seen == 2 {
                    return;
                }
            }
        }
    });

    let width = probe.measured_width("字");
    responder.join().unwrap();

    assert_eq!(width, 2);
}
