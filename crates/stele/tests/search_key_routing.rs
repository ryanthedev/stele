//! DW-4.1, at the seam no unit test can reach: `main.rs`'s key routing.
//!
//! The event loop runs a chrome table (`+`, `-`, `T`) *before*
//! `AppState::handle_key_event`, because those keys need a `Painter` and a
//! `LayoutContext` that `AppState` does not own. `AppState`'s own "while the
//! prompt is open, every key is text" guard therefore cannot defend the
//! query: a key the chrome table claims never arrives. It claimed three, and
//! `/The` searched for `he` while flipping the theme.
//!
//! Two gates passed that defect because every search test drove
//! `AppState::handle_key_event` directly — the one entry point the bug is not
//! on. So this file drives the **real binary** over a **real pty** and reads
//! the answer off the wire: the whole query in the status row, and the
//! heading still painted in the colour it started in. Nothing short of the
//! shipped routing is under test here, which is the point.

#![cfg(unix)]

mod common;

use std::io::Write as _;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::Duration;

use common::pty::{ENTER_ALT, Pty, read_until};

/// Closes a mode-2026 synchronized-update block — the end of one painted
/// frame.
const SYNC_END: &[u8] = b"\x1b[?2026l";

/// The pty the harness opens is 24x80, so the reserved status row (DW-1.1,
/// `rows - 1` of content) is row 24.
const ROWS: u16 = 24;
const STATUS_ROW: u16 = ROWS;

/// A running viewer on a pty, plus everything read off it so far.
struct Session {
    pty: Pty,
    child: std::process::Child,
    doc: std::path::PathBuf,
    wire: Vec<u8>,
}

impl Session {
    /// Spawns the real `stele` on `source` and reads up to its first frame.
    fn start(tag: &str, source: &str) -> Session {
        let doc =
            std::env::temp_dir().join(format!("stele-routing-{}-{tag}.md", std::process::id()));
        std::fs::File::create(&doc)
            .and_then(|mut f| f.write_all(source.as_bytes()))
            .expect("write fixture");

        let pty = Pty::open();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_stele"));
        cmd.arg(&doc)
            .env("TERM", "xterm-256color")
            .env("TERM_PROGRAM", "ghostty")
            .env_remove("TMUX")
            // The theme half of this test reads truecolor SGR off the wire;
            // an ambient NO_COLOR in the runner's environment would strip it
            // and quietly make that assertion unaskable.
            .env_remove("NO_COLOR")
            .stdin(Stdio::from(pty.slave_dup()))
            .stdout(Stdio::from(pty.slave_dup()))
            .stderr(Stdio::from(pty.slave_dup()));
        // SAFETY: `pre_exec` runs between fork and exec, so it may only call
        // async-signal-safe functions. `setsid` and `ioctl(TIOCSCTTY)` both
        // are; together they make the pty the child's controlling terminal.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                libc::ioctl(0, libc::TIOCSCTTY as _, 0);
                Ok(())
            });
        }
        let child = cmd.spawn().expect("spawn stele");

        let mut session = Session {
            pty,
            child,
            doc,
            wire: Vec::new(),
        };
        assert!(
            session.wait_for(0, ENTER_ALT, Duration::from_secs(10)),
            "stele never entered the alternate screen; wire was {:?}",
            session.text()
        );
        // Entering the alternate screen happens *before* the first paint, so
        // stopping there leaves nothing on the wire worth asserting about.
        assert!(
            session.wait_for(0, SYNC_END, Duration::from_secs(10)),
            "stele never finished its first frame; wire was {:?}",
            session.text()
        );
        session
    }

    /// Reads until `needle` appears in the bytes at or after offset `from`,
    /// returning whether it did.
    ///
    /// Searching the **accumulated** wire from an explicit offset, rather
    /// than whatever one read happened to return, is what makes sequential
    /// waits work at all. `common::pty::read_until` stops once the needle is
    /// in the chunk it is building, but it reads in 8 KiB blocks and
    /// routinely swallows the rest of the frame with it — so a following
    /// "wait for the end of the frame" would sit waiting for bytes already
    /// sitting in this buffer, and burn its whole deadline before every
    /// assertion. Callers take `from = self.wire.len()` before typing, so
    /// each wait is about what the keystroke produced and not what the
    /// startup frame left behind.
    fn wait_for(&mut self, from: usize, needle: &[u8], deadline: Duration) -> bool {
        let expiry = std::time::Instant::now() + deadline;
        loop {
            if common::pty::contains(&self.wire[from.min(self.wire.len())..], needle) {
                return true;
            }
            if std::time::Instant::now() >= expiry {
                return false;
            }
            let chunk = read_until(self.pty.master_fd(), needle, Duration::from_millis(200));
            self.wire.extend_from_slice(&chunk);
        }
    }

    /// Types `keys` and waits for the repaint they trigger to complete.
    /// Returns the wire offset the keystroke's own output starts at.
    fn type_and_settle(&mut self, keys: &str) -> usize {
        let from = self.wire.len();
        self.type_str(keys);
        self.wait_for(from, SYNC_END, Duration::from_secs(10));
        from
    }

    fn type_str(&self, keys: &str) {
        self.pty.type_bytes(keys.as_bytes());
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.wire).into_owned()
    }

    /// The last complete frame on the wire. Frames are delimited by the
    /// mode-2026 synchronized-update opener, so the final one is what the
    /// reader is looking at now — replaying the whole buffer would mix a
    /// dozen repaints together.
    fn last_frame(&self) -> String {
        let text = self.text();
        match text.rfind("\x1b[?2026h") {
            Some(at) => text[at..].to_string(),
            None => text,
        }
    }

    /// What the reserved status row reads.
    ///
    /// Read straight off the bytes rather than through
    /// `common::render::render_row`: the painter writes the status row as a
    /// cursor-position to row `ROWS` followed by one dim SGR and the text, so
    /// the bytes *are* the oracle here, with no cell grid to replay. That
    /// also keeps this test off the shared terminal model, which does not
    /// terminate on the startup wire this session produces.
    fn status_row(&self) -> String {
        let frame = self.last_frame();
        let cup = format!("\x1b[{STATUS_ROW};1H");
        let at = frame
            .rfind(&cup)
            .unwrap_or_else(|| panic!("no status row in the last frame: {frame:?}"));
        let rest = &frame[at + cup.len()..];
        // The row is painted dim; the text follows that one SGR.
        let rest = rest.strip_prefix("\x1b[2m").unwrap_or(rest);
        let end = rest.find('\x1b').unwrap_or(rest.len());
        rest[..end].to_string()
    }

    /// Ends the session cleanly and reaps the child.
    ///
    /// Ctrl-C rather than `Esc` then `q`, and the reason is a real hazard
    /// rather than taste: a terminal encodes `Alt`-<key> as `ESC` <key>, and
    /// crossterm disambiguates a lone `ESC` from the start of a sequence by
    /// *time*. These writes land microseconds apart, so `Esc` followed by `q`
    /// parses as a single `Alt+q`, which is bound to nothing — the viewer
    /// stays up and this harness blocks until its deadline. Ctrl-C is one
    /// byte with no such ambiguity, and it quits from the prompt as well as
    /// from normal mode, so it works whatever state the test left behind.
    fn quit(mut self) {
        let from = self.wire.len();
        self.type_str("\x03");

        // Keep draining while waiting. This is not politeness: the child is
        // still painting into the pty, and a pty whose output nobody reads
        // fills and blocks the writer. A `try_wait` loop that does not read
        // deadlocks — the viewer is stuck in `write`, so it never reaches the
        // `read` that would see the Ctrl-C, and both sides wait forever.
        // Waiting on the restore sequence drains and asserts at once.
        assert!(
            self.wait_for(from, common::pty::RESTORE, Duration::from_secs(10)),
            "stele never restored the terminal after Ctrl-C"
        );

        // Bounded, so a viewer that does not quit fails this test rather than
        // hanging the suite.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let status = loop {
            match self.child.try_wait().expect("try_wait") {
                Some(status) => break status,
                None if std::time::Instant::now() >= deadline => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    let _ = std::fs::remove_file(&self.doc);
                    panic!("stele did not quit within 10s of Ctrl-C");
                }
                None => std::thread::sleep(Duration::from_millis(20)),
            }
        };
        assert_eq!(
            status.code(),
            Some(0),
            "stele must exit cleanly: {status:?}"
        );
        let _ = std::fs::remove_file(&self.doc);
    }
}

/// The truecolor `38;2;r;g;b` foreground in effect for `needle` — the colour
/// the painter actually used, read off the bytes rather than assumed from the
/// theme table.
fn fg_before(wire: &str, needle: &str) -> String {
    let at = wire
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not found on the wire: {wire:?}"));
    let prefix = &wire[..at];
    let start = prefix
        .rfind("38;2;")
        .unwrap_or_else(|| panic!("no truecolor fg SGR before {needle:?}: {wire:?}"));
    let rest = &prefix[start..];
    let end = rest.find('m').unwrap_or(rest.len());
    rest[..end].to_string()
}

/// Every printable key typed into a query must reach the query — including
/// the three the chrome table claims for itself.
///
/// `T`, `+` and `-` are not an arbitrary sample: they are exactly the keys
/// `handle_chrome_key` binds, so they are the complete set of characters that
/// could be stolen. `T` matters most in practice — it opens a large fraction
/// of capitalised English words, so ordinary queries were being corrupted.
#[test]
fn test_dw_4_1_every_printable_key_reaches_the_query_including_the_chrome_keys() {
    // The query is spelled out of the chrome bindings themselves plus a
    // letter that is not one, so a pass means all three were routed and a
    // failure names which survived.
    const QUERY: &str = "T+-z";
    let mut session = Session::start(
        "chrome-keys",
        "# Heading\n\nA line containing T+-z and more words.\n",
    );

    session.type_and_settle("/");
    let from = session.wire.len();
    session.type_str(QUERY);
    // Waited on with its leading `/` so it cannot be satisfied by the body
    // text, which contains the same characters. If a key is stolen this never
    // arrives and the wait simply expires; the assertion below is what
    // reports it, naming the row that did appear.
    session.wait_for(from, b"/T+-z", Duration::from_secs(5));

    let row = session.status_row();
    assert!(
        row.starts_with(&format!("/{QUERY}")),
        "the status row must show the whole query. Row {STATUS_ROW} reads \
         {row:?}, expected it to start with {:?}. A missing character is a key \
         the chrome table in main.rs consumed before AppState saw it.",
        format!("/{QUERY}")
    );

    session.quit();
}

/// The other half of the same defect: `T` must not *do* anything while a
/// query is open. Reaching the query is necessary but not sufficient — a
/// routing fix that let the character through while still swapping the theme
/// would satisfy the test above and still repaint the viewer mid-search.
///
/// Asserted on the heading's painted foreground, which is the one thing on
/// screen that changes when and only when the variant flips.
#[test]
fn test_dw_4_1_typing_t_into_a_query_does_not_swap_the_theme() {
    let mut session = Session::start("theme-key", "# Heading\n\nBody text with The word.\n");

    let before = fg_before(&session.last_frame(), "Heading");

    let from = session.wire.len();
    session.type_str("/The");
    session.wait_for(from, b"/The", Duration::from_secs(5));

    let after = fg_before(&session.last_frame(), "Heading");
    assert_eq!(
        before, after,
        "typing `T` into a search query must not swap the theme variant — the \
         heading's foreground changed from {before:?} to {after:?}"
    );

    session.quit();
}

/// `T` still swaps the theme in normal mode. The guard must scope the chrome
/// table to the mode, not disable it — a fix that simply stopped routing `T`
/// would pass both tests above and silently delete DW-1.5.
#[test]
fn test_t_still_swaps_the_theme_when_no_query_is_open() {
    let mut session = Session::start("theme-normal", "# Heading\n\nBody text.\n");

    let before = fg_before(&session.last_frame(), "Heading");

    // The repaint that follows the swap is what carries the new colour.
    session.type_and_settle("T");

    let after = fg_before(&session.last_frame(), "Heading");
    assert_ne!(
        before, after,
        "`T` in normal mode must still swap the theme (DW-1.5): the heading's \
         foreground stayed {before:?}"
    );

    session.quit();
}
