//! DW-3.2, at the level the routing lives: the TOC overlay driven through the
//! **real binary's** key path, not through `AppState`.
//!
//! `AppState::handle_key_event` is not where a key arrives. `main.rs` offers
//! every press to `handle_chrome_key` first, in two places — the main loop and
//! the resize drain — and a `true` from there means `AppState` never sees the
//! key at all. So an `AppState` test can prove the TOC's key table is correct
//! and still miss the overlay losing `T`, `+` and `-` to a table it cannot
//! see. Phase 2 found the mirror image from the other side: a `T` pressed
//! during a resize drain was offered only to `handle_key_event`, which does not
//! know it, and was dropped.
//!
//! **What these tests assert, and why it is not "the overlay is still up".**
//! An ungated chrome key does not close the overlay — it relays out or
//! re-themes the document *behind* it, and the next frame is still the
//! overlay, so a screenshot of the overlay is identical either way. The
//! observable difference is on the far side of `Esc`: the document the reader
//! comes back to. Both tests therefore do a round trip and compare the
//! document to itself.

#![cfg(unix)]

use std::collections::BTreeSet;
use std::time::Duration;

mod common;

use common::pty::{
    ChildStdin, Pty, RESTORE, contains, drain_to_quiet, read_one_frame, read_until, spawn_viewer,
};
use common::render::render_row;

/// The harness opens an 80x24 pty, so the content viewport is 23 rows and the
/// status row is row 24.
const COLS: usize = 80;
const CONTENT_ROWS: u16 = 23;

const DEADLINE: Duration = Duration::from_secs(5);

/// Headings, distinctive body text, and one paragraph long enough to wrap —
/// the wrap is the oracle. Its line breaks are a function of the layout width,
/// so "did a `+`/`-` pressed under the overlay reach the document" is a
/// question the painted rows can answer.
///
/// Body lines are deliberately not substrings of heading text: the overlay
/// lists headings and the document shows both, so a needle appearing in each
/// could not tell them apart.
fn fixture() -> String {
    let mut source = String::from(
        "Wrapping paragraph whose line breaks depend entirely on the layout width in \
         effect when it was laid out, carrying enough distinct words that a change of \
         a dozen cells moves several of them onto different rows than before.\n\n",
    );
    for i in 0..8 {
        let hashes = "#".repeat(1 + i % 3);
        source.push_str(&format!("{hashes} Chapter {i:02}\n\nparagraph-{i:02}\n\n"));
    }
    source
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("stele-toc-{tag}-{}.md", std::process::id()));
    std::fs::write(&path, fixture()).expect("write fixture");
    path
}

/// Every content row of `frame`, joined — what the reader is looking at.
fn screen(frame: &str, rows: u16, cols: usize) -> String {
    (1..=rows)
        .map(|row| render_row(frame, row, cols))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every distinct SGR sequence in `frame`. The theme's palette reaches the
/// wire only as colour escapes, and `render_row` deliberately throws those
/// away — so this is the channel a `T` that reached the document shows up on.
fn sgr_palette(frame: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let bytes: Vec<char> = frame.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '\u{1b}' && bytes.get(i + 1) == Some(&'[') {
            let mut j = i + 2;
            while j < bytes.len() && !bytes[j].is_ascii_alphabetic() {
                j += 1;
            }
            if bytes.get(j) == Some(&'m') {
                found.insert(bytes[i + 2..j].iter().collect::<String>());
            }
            i = j + 1;
            continue;
        }
        i += 1;
    }
    found
}

/// Types `keys` and returns the frame it produced, with the wire drained first
/// so the frame read back is this keystroke's and not a leftover.
fn press(pty: &Pty, keys: &[u8]) -> String {
    drain_to_quiet(pty.master_fd());
    pty.type_bytes(keys);
    read_one_frame(pty.master_fd(), DEADLINE)
}

fn quit_and_reap(pty: &Pty, child: &mut std::process::Child) {
    pty.type_bytes(b"q");
    let after = read_until(pty.master_fd(), RESTORE, Duration::from_secs(10));
    assert!(
        contains(&after, RESTORE),
        "`q` must end the session with the restore sequence"
    );
    let exit = child.wait().expect("wait");
    assert_eq!(exit.code(), Some(0), "clean quit expected: {exit:?}");
}

/// **The routing claim, main-loop path.** A chrome key pressed while the TOC
/// is up must not reach the document underneath it.
///
/// `+`/`-` are handled in `main.rs` ahead of `AppState`, so ungated they relay
/// out at a new width while the reader is looking at a list of headings. The
/// reader sees nothing happen, presses `Esc`, and the document has rewrapped.
/// The round trip below is what makes that visible: same document, same scroll,
/// so the rows must come back byte-identical.
#[test]
fn test_dw_3_2_a_chrome_key_pressed_under_the_toc_does_not_reach_the_document() {
    let path = scratch("chrome-reachthrough");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[path.as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();

    read_until(master, b"Chapter 00", DEADLINE);
    drain_to_quiet(master);
    // A no-op keystroke, purely to get a full frame of the document to
    // compare against. `k` at the top of the document moves nothing.
    let before = screen(&press(&pty, b"k"), CONTENT_ROWS, COLS);
    assert!(
        before.contains("Wrapping paragraph"),
        "the fixture's wrapping paragraph must be on screen:\n{before}"
    );

    // Open the overlay: every heading listed, document body covered.
    let toc = screen(&press(&pty, b"t"), CONTENT_ROWS, COLS);
    for i in 0..8 {
        assert!(
            toc.contains(&format!("Chapter {i:02}")),
            "the overlay must list every heading, missing {i:02}:\n{toc}"
        );
    }
    assert!(
        !toc.contains("paragraph-"),
        "the overlay must cover the document:\n{toc}"
    );

    // Three narrows and three widens under the overlay. Ungated, each one
    // relays out; `-` clamps at `min_width` so the pair does not cancel.
    for key in [&b"-"[..], b"-", b"-", b"+"] {
        let after = screen(&press(&pty, key), CONTENT_ROWS, COLS);
        assert!(
            after.contains("Chapter 00") && after.contains("Chapter 07"),
            "`{}` must leave the overlay up:\n{after}",
            String::from_utf8_lossy(key)
        );
    }

    // Back to the document. Nothing the reader pressed was about the
    // document, so nothing about it may have changed.
    let restored = screen(&press(&pty, b"\x1b"), CONTENT_ROWS, COLS);
    assert_eq!(
        restored, before,
        "a `+`/`-` pressed under the overlay reached the document: it came back \
         laid out differently"
    );

    // ...and the overlay is still functional afterwards, which is the opposite
    // failure: a mode gate that swallowed navigation too.
    let moved = screen(&press(&pty, b"tjjj"), CONTENT_ROWS, COLS);
    assert!(
        moved.contains("Chapter 03"),
        "the overlay must still answer its own keys:\n{moved}"
    );
    let jumped = screen(&press(&pty, b"\r"), CONTENT_ROWS, COLS);
    assert!(
        jumped.contains("Chapter 03") && jumped.contains("paragraph-03"),
        "Enter must leave the overlay and land on the selected heading:\n{jumped}"
    );

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::remove_file(&path);
}

/// **The routing claim, resize-drain path.** `main.rs` runs the identical
/// chrome-then-app ordering inside the resize debounce, and a key read there
/// takes the same route — so the gate has to hold in both, and Phase 2's own
/// history is the reason to check rather than assume.
///
/// `T` rather than `+`/`-` here, deliberately: `apply_resize_burst` resyncs
/// `content_width` to the new terminal width immediately after the drain, so a
/// width toggle read inside it is overridden either way and could not fail
/// this test. A theme swap survives, and reaches the wire as the document's
/// colour escapes.
#[test]
fn test_dw_3_2_a_chrome_key_read_during_a_resize_drain_does_not_reach_the_document() {
    let path = scratch("chrome-reachthrough-drain");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[path.as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"Chapter 00", DEADLINE);

    // Settle at the post-resize geometry first, so the before/after comparison
    // is between two frames of the same size.
    drain_to_quiet(master);
    pty.resize(30, 100);
    let before_frame = read_one_frame(master, DEADLINE);
    let before_palette = sgr_palette(&before_frame);
    assert!(
        !before_palette.is_empty(),
        "the document must paint some colour, or this test has no oracle"
    );

    let toc = screen(&press(&pty, b"t"), 29, 100);
    assert!(toc.contains("Chapter 00"), "the overlay must open:\n{toc}");

    // A resize with a `T` hard on its heels: the sequence that lands a key
    // inside the drain loop rather than the main loop.
    drain_to_quiet(master);
    pty.resize(30, 100);
    pty.type_bytes(b"T");
    let after_toc = read_one_frame(master, DEADLINE);
    assert!(
        screen(&after_toc, 29, 100).contains("Chapter 00"),
        "the overlay must survive the resize and the key that followed it"
    );

    // Back to the document: same geometry, same scroll, so the same palette.
    let restored_frame = press(&pty, b"\x1b");
    assert!(
        screen(&restored_frame, 29, 100).contains("paragraph-00"),
        "Esc must restore the document at the prior scroll position"
    );
    assert_eq!(
        sgr_palette(&restored_frame),
        before_palette,
        "a `T` read inside the resize drain reached the document: it came back \
         in the other theme"
    );

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::remove_file(&path);
}
