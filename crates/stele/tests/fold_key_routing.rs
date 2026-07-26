//! DW-5.1, at the level the routing lives: folding driven through the
//! **real binary's** key path, not through `AppState`.
//!
//! `z`/`R`/`M` are chrome keys exactly like `+`/`-`/`T` (Phase 1) — they need
//! a `LayoutContext` to relay out against, which `AppState::handle_key_event`
//! does not have, so `main.rs::handle_chrome_key` offers them to
//! `AppState::chrome_action` first, in two places (the main loop and the
//! resize drain), before `AppState` ever sees the key. Three phases already
//! found the mirror-image defect this shape invites — a chrome key silently
//! reaching a mode that should have swallowed it, or a mode's own key falling
//! through to chrome — from three different directions, none of them caught
//! by a test that only drove `AppState::handle_key_event` directly. This file
//! drives the real binary over a real pty instead: that `z` actually folds
//! the document on screen, and that the TOC overlay swallows `z`/`R`/`M`
//! exactly as it already does `+`/`-`/`T`.

#![cfg(unix)]

mod common;

use std::time::Duration;

use common::pty::{
    ChildStdin, Pty, RESTORE, contains, drain_to_quiet, read_one_frame, read_until, spawn_viewer,
};
use common::render::render_row;

/// The harness opens an 80x24 pty, so the content viewport is 23 rows and the
/// status row is row 24.
const COLS: usize = 80;
const CONTENT_ROWS: u16 = 23;

const DEADLINE: Duration = Duration::from_secs(5);

fn fixture() -> String {
    "# Chapter One\n\nfirst-chapter-body text here.\n\n# Chapter Two\n\nsecond-chapter-body text.\n"
        .to_string()
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("stele-fold-{tag}-{}.md", std::process::id()));
    std::fs::write(&path, fixture()).expect("write fixture");
    path
}

fn screen(frame: &str, rows: u16, cols: usize) -> String {
    (1..=rows)
        .map(|row| render_row(frame, row, cols))
        .collect::<Vec<_>>()
        .join("\n")
}

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

/// **The core claim.** `z` pressed on the real binary folds the section under
/// the cursor to a marked line, and `z` again restores it byte-for-byte.
#[test]
fn test_dw_5_1_toggling_a_fold_through_the_real_binary_collapses_and_restores_the_section() {
    let path = scratch("toggle");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[path.as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();

    let start = read_until(master, b"second-chapter-body", DEADLINE);
    let before = screen(&String::from_utf8_lossy(&start), CONTENT_ROWS, COLS);
    assert!(
        before.contains("first-chapter-body") && before.contains("Chapter One"),
        "the fixture must be fully expanded on open:\n{before}"
    );

    let folded = screen(&press(&pty, b"z"), CONTENT_ROWS, COLS);
    assert!(
        folded.contains("Chapter One") && folded.contains("hidden"),
        "`z` must leave a marker naming the folded heading and a hidden count:\n{folded}"
    );
    assert!(
        !folded.contains("first-chapter-body"),
        "`z` must remove the folded section's body from the screen:\n{folded}"
    );
    assert!(
        folded.contains("Chapter Two") && folded.contains("second-chapter-body"),
        "the sibling section must be unaffected:\n{folded}"
    );

    let restored = screen(&press(&pty, b"z"), CONTENT_ROWS, COLS);
    assert_eq!(
        restored, before,
        "`z` pressed again must restore the document exactly"
    );

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::remove_file(&path);
}

/// **The routing claim.** `z`/`R`/`M` pressed while the TOC is up must not
/// reach the document underneath it — the same gate `+`/`-`/`T` already have
/// (`toc_key_routing.rs`), now extended to the fold keys `chrome_action`
/// gained this phase.
#[test]
fn test_fold_keys_pressed_under_the_toc_do_not_reach_the_document() {
    let path = scratch("toc-reachthrough");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[path.as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();

    read_until(master, b"Chapter One", DEADLINE);
    drain_to_quiet(master);
    let before = screen(&press(&pty, b"k"), CONTENT_ROWS, COLS);
    assert!(before.contains("first-chapter-body"));

    let toc = screen(&press(&pty, b"t"), CONTENT_ROWS, COLS);
    assert!(
        toc.contains("Chapter One") && toc.contains("Chapter Two"),
        "the overlay must list both headings:\n{toc}"
    );
    assert!(
        !toc.contains("first-chapter-body"),
        "the overlay must cover the document:\n{toc}"
    );

    for key in [&b"z"[..], b"M", b"R"] {
        let after = screen(&press(&pty, key), CONTENT_ROWS, COLS);
        assert!(
            after.contains("Chapter One") && after.contains("Chapter Two"),
            "`{}` must leave the overlay up:\n{after}",
            String::from_utf8_lossy(key)
        );
    }

    let restored = screen(&press(&pty, b"\x1b"), CONTENT_ROWS, COLS);
    assert_eq!(
        restored, before,
        "a fold key pressed under the overlay reached the document: it came back folded"
    );

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::remove_file(&path);
}
