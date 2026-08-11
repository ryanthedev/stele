//! `Mode::Explore` driven through the **real binary's** event loop (Phase
//! 3), not through `AppState` — the same reason `link_interaction.rs` and
//! `toc_key_routing.rs` exist: `main.rs` offers every press to
//! `handle_chrome_key` first, at two dispatch sites, and the document swap
//! and directory read both happen in `main.rs` where `AppState` cannot
//! reach.
//!
//! One fixture directory serves every test that needs one, laid out so its
//! own alphabetical listing order is part of the test's own documentation
//! (see [`fixture`]).

#![cfg(unix)]

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

mod common;

use common::pty::{
    ChildStdin, Exit, Graphics, Pty, RESTORE, assert_restores_the_terminal_in, contains,
    drain_to_quiet, read_one_frame, read_until, spawn_viewer, spawn_viewer_in_dir,
};
use common::render::render_row;

/// The harness opens an 80x24 pty, so the content viewport is 23 rows and the
/// status row is row 24.
const COLS: usize = 80;
const CONTENT_ROWS: u16 = 23;
const STATUS_ROW: u16 = 24;

const DEADLINE: Duration = Duration::from_secs(5);

/// A directory whose alphabetical listing order is exactly:
/// `../`, `alpha.md`, `blocked` (a FIFO — unopenable), `index.md`,
/// `sub/`, `zeta.md`.
///
/// `index.md` is the document every "open the explorer from a real
/// document" test opens first; `alpha.md`/`zeta.md` are ordinary markdown
/// with distinctive body markers; `blocked` proves the dim/unselectable row;
/// `sub/` holds one more document (`inner.md`) so descending and ascending
/// both have something to assert on.
fn fixture(tag: &str) -> PathBuf {
    let dir = common::fixtures::scratch_dir(&format!("explorer-{tag}"));
    std::fs::write(dir.join("alpha.md"), "alpha-document-marker\n").unwrap();
    std::fs::write(dir.join("index.md"), {
        let mut s = String::from("# Index\n\n");
        for i in 0..40 {
            s.push_str(&format!("index-filler-{i:02}\n\n"));
        }
        s.push_str("index-tail-marker\n");
        s
    })
    .unwrap();
    std::fs::create_dir(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub").join("inner.md"), "inner-document-marker\n").unwrap();
    std::fs::write(dir.join("zeta.md"), "zeta-document-marker\n").unwrap();
    // A NUL byte and non-UTF8 bytes: a regular file, so `Document` and
    // selectable, but refused by the barricade's sniff on `Enter` (DW-3.8).
    std::fs::write(dir.join("nulled.md"), b"before\0after\n").unwrap();
    // A FIFO: `Unopenable`, dim and unselectable (DW-3.2).
    let rc = unsafe {
        let c_path =
            std::ffi::CString::new(dir.join("blocked").as_os_str().as_encoded_bytes()).unwrap();
        libc::mkfifo(c_path.as_ptr(), 0o600)
    };
    assert_eq!(rc, 0, "mkfifo failed: {}", std::io::Error::last_os_error());
    dir
}

/// Every content row of `frame`, joined — what the reader is looking at.
fn screen(frame: &str) -> String {
    (1..=CONTENT_ROWS)
        .map(|row| render_row(frame, row, COLS))
        .collect::<Vec<_>>()
        .join("\n")
}

fn status(frame: &str) -> String {
    render_row(frame, STATUS_ROW, COLS).trim_end().to_string()
}

/// `frame`, with the status row's own bytes cut off — `paint_status_row`
/// (pre-existing, every mode) always wraps its text in `\x1b[2m`, which
/// would otherwise show up as a false positive in [`dimmed_text`] the
/// moment the ruler shows anything at all (the explorer's own status text is
/// the listed directory — see DW-3.10). The status row is always painted
/// last, at `\x1b[{STATUS_ROW};1H`, so everything before that escape is the
/// content rows alone.
fn content_only(frame: &str) -> &str {
    let marker = format!("\x1b[{STATUS_ROW};1H");
    frame.split(&marker).next().unwrap_or(frame)
}

/// Every run of glyphs painted with the reverse-video attribute in `frame`'s
/// *content* rows — the selected explorer row (DW-3.2). Mirrors
/// `link_interaction.rs`'s `reversed_text`: `\x1b[7m` opens immediately
/// before the run's clipped text and every following byte sequence resets or
/// repositions, so the text up to the next escape is exactly what the
/// terminal shows reversed.
fn reversed_text(frame: &str) -> Vec<String> {
    content_only(frame)
        .split("\u{1b}[7m")
        .skip(1)
        .map(|rest| rest.split('\u{1b}').next().unwrap_or("").to_string())
        .filter(|text| !text.is_empty())
        .collect()
}

/// The dim-attribute counterpart of [`reversed_text`] — `\x1b[2m` in the
/// content rows, what `overlay_body` writes before an `Unopenable` row.
fn dimmed_text(frame: &str) -> Vec<String> {
    content_only(frame)
        .split("\u{1b}[2m")
        .skip(1)
        .map(|rest| rest.split('\u{1b}').next().unwrap_or("").to_string())
        .filter(|text| !text.is_empty())
        .collect()
}

/// Every distinct SGR sequence in `frame` — the document's theme palette,
/// used the same way `toc_key_routing.rs` uses it: the explorer overlay has
/// no theme roles of its own, so a theme toggle that reached the *document*
/// is only visible once the explorer closes and the document repaints.
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

/// Types `keys` and returns the frame it produced, with the wire drained
/// first so the frame read back is this keystroke's and not a leftover.
fn press(pty: &Pty, keys: &[u8]) -> String {
    drain_to_quiet(pty.master_fd());
    pty.type_bytes(keys);
    read_one_frame(pty.master_fd(), DEADLINE)
}

/// An SGR mouse report, as a terminal in mode `?1006` sends it. `button` is
/// 0 for the left button and 65 for a wheel-down notch; `col`/`row` are
/// 1-based.
fn mouse_report(button: u16, col: u16, row: u16, press: bool) -> Vec<u8> {
    format!(
        "\x1b[<{button};{col};{row}{}",
        if press { 'M' } else { 'm' }
    )
    .into_bytes()
}

fn quit_and_reap(pty: &Pty, child: &mut common::pty::ViewerProcess) {
    drain_to_quiet(pty.master_fd());
    pty.type_bytes(b"q");
    let after = read_until(pty.master_fd(), RESTORE, Duration::from_secs(10));
    assert!(
        contains(&after, RESTORE),
        "`q` must end the session with the restore sequence"
    );
    let exit = child.wait().expect("wait");
    assert_eq!(exit.code(), Some(0), "clean quit expected: {exit:?}");
}

/// Opens `index.md`, scrolls it well away from the top (so DW-3.5's
/// "restores the scroll position" has something to be wrong about — 40
/// presses is enough to outrun the viewport's scrolloff on a document with
/// 40 filler paragraphs, which 10 was not), and presses `_`. Returns the
/// pty, child, the scrolled-but-pre-explorer screen (for an exact
/// round-trip comparison, the same idiom `toc_key_routing.rs` uses), and the
/// frame `_` produced.
fn open_from_index(tag: &str) -> (Pty, common::pty::ViewerProcess, String, String) {
    let dir = fixture(tag);
    let pty = Pty::open();
    let child = spawn_viewer(&pty, &[dir.join("index.md").as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"index-filler-00", DEADLINE);
    drain_to_quiet(master);
    let mut before = String::new();
    for _ in 0..40 {
        before = screen(&press(&pty, b"j"));
    }
    assert!(
        !before.contains("index-filler-00"),
        "40 presses must have scrolled past the first filler line:\n{before}"
    );

    let explorer = press(&pty, b"_");
    (pty, child, before, explorer)
}

/// **DW-3.1.** `_` opens the explorer at the open document's directory,
/// seated on that document's own row — `index.md`, not row zero
/// (`alpha.md`).
#[test]
fn test_dw_3_1_underscore_opens_seated_on_the_open_documents_row() {
    let (pty, mut child, _before, explorer) = open_from_index("dw-3-1");
    let body = screen(&explorer);
    for name in ["alpha.md", "blocked", "index.md", "sub", "zeta.md"] {
        assert!(
            body.contains(name),
            "the explorer must list every entry, missing {name}:\n{body}"
        );
    }
    assert!(
        !body.contains("index-filler") && !body.contains("index-tail"),
        "the explorer must cover the document:\n{body}"
    );
    assert_eq!(
        reversed_text(&explorer),
        vec!["index.md".to_string()],
        "`_` must seat the cursor on the open document's own row"
    );
    quit_and_reap(&pty, &mut child);
}

/// **DW-3.1, stdin half.** With a stdin document (no directory of its own),
/// `_` opens at the current working directory.
#[test]
fn test_dw_3_1_underscore_from_a_stdin_document_opens_at_cwd() {
    let dir = fixture("dw-3-1-stdin");
    let pty = Pty::open();
    let mut child = spawn_viewer_in_dir(
        &pty,
        &[std::ffi::OsStr::new("-")],
        ChildStdin::Pipe(b"# from stdin\n\nstdin-body-marker\n"),
        Graphics::Off,
        &dir,
    );
    let master = pty.master_fd();
    read_until(master, b"stdin-body-marker", DEADLINE);

    let explorer = screen(&press(&pty, b"_"));
    for name in ["alpha.md", "index.md", "sub", "zeta.md"] {
        assert!(
            explorer.contains(name),
            "`_` from stdin must list cwd, missing {name}:\n{explorer}"
        );
    }
    quit_and_reap(&pty, &mut child);
}

/// **DW-3.2.** The selected row paints reverse video and the unopenable
/// (`blocked`) row paints dim, asserted on the actual SGR bytes rather than
/// on `OverlayRow` — a painter regression that stopped honoring `RowStyle`
/// would still leave `explore_rows` correct and this test would still catch
/// it.
#[test]
fn test_dw_3_2_unselectable_rows_paint_dim_and_the_selected_row_paints_reverse() {
    let (pty, mut child, _before, explorer) = open_from_index("dw-3-2");
    assert_eq!(reversed_text(&explorer), vec!["index.md".to_string()]);
    assert_eq!(
        dimmed_text(&explorer),
        vec!["blocked".to_string()],
        "the FIFO row must be the only dimmed one"
    );
    quit_and_reap(&pty, &mut child);
}

/// **DW-3.3.** `Enter` on a markdown row installs that document;
/// `Backspace` returns to the previous document at its previous scroll.
#[test]
fn test_dw_3_3_enter_opens_and_backspace_returns_to_the_prior_scroll() {
    let (pty, mut child, before, explorer) = open_from_index("dw-3-3");
    assert_eq!(reversed_text(&explorer), vec!["index.md".to_string()]);

    // `k` once from `index.md` skips `blocked` and lands on `alpha.md`.
    let moved = press(&pty, b"k");
    assert_eq!(reversed_text(&moved), vec!["alpha.md".to_string()]);

    let opened = screen(&press(&pty, b"\r"));
    assert!(
        opened.contains("alpha-document-marker"),
        "Enter must open alpha.md:\n{opened}"
    );

    let back = screen(&press(&pty, &[0x7f]));
    assert_eq!(
        back, before,
        "Backspace must return to index.md at exactly the scroll position \
         the reader left it at"
    );

    quit_and_reap(&pty, &mut child);
}

/// **DW-3.4 / DW-3.6 (dash).** `Enter` on `sub/` re-lists it; `-` re-lists
/// the parent and reseats on `sub` by name, so `Enter` then `-` retraces the
/// same steps.
#[test]
fn test_dw_3_4_enter_on_a_directory_and_dash_both_relist() {
    let (pty, mut child, _before, explorer) = open_from_index("dw-3-4");
    // Fixture order from `index.md`: `nulled.md` then `sub` — two presses.
    assert_eq!(reversed_text(&explorer), vec!["index.md".to_string()]);
    press(&pty, b"j");
    let on_sub = press(&pty, b"j");
    assert_eq!(reversed_text(&on_sub), vec!["sub/".to_string()]);

    let inside = screen(&press(&pty, b"\r"));
    assert!(
        inside.contains("inner.md"),
        "Enter on sub/ must list its contents:\n{inside}"
    );
    assert!(
        !inside.contains("alpha.md"),
        "the parent's other entries must not still be listed:\n{inside}"
    );

    let up = press(&pty, b"-");
    let up_body = screen(&up);
    for name in ["alpha.md", "index.md", "sub", "zeta.md"] {
        assert!(
            up_body.contains(name),
            "`-` must re-list the parent:\n{up_body}"
        );
    }
    assert_eq!(
        reversed_text(&up),
        vec!["sub/".to_string()],
        "`-` must reseat on the directory just left, by name"
    );

    quit_and_reap(&pty, &mut child);
}

/// **DW-3.5.** `Esc` with an unrooted explorer closes it and restores the
/// scroll position the reader left.
#[test]
fn test_dw_3_5_esc_unrooted_restores_the_readers_scroll_position() {
    let (pty, mut child, before, explorer) = open_from_index("dw-3-5");
    assert_eq!(reversed_text(&explorer), vec!["index.md".to_string()]);

    let restored = screen(&press(&pty, b"\x1b"));
    assert_eq!(
        restored, before,
        "Esc must restore the exact scroll position the reader left"
    );
    quit_and_reap(&pty, &mut child);
}

/// **DW-3.6.** Chrome keys change nothing while exploring, on the ordinary
/// dispatch path — proved the way `toc_key_routing.rs` proves it: a round
/// trip back to the document, compared to itself.
#[test]
fn test_dw_3_6_chrome_keys_are_inert_while_exploring() {
    let (pty, mut child, before, explorer) = open_from_index("dw-3-6");
    assert_eq!(reversed_text(&explorer), vec!["index.md".to_string()]);

    for key in [&b"+"[..], b"T", b"z", b"R", b"M", b"#"] {
        let frame = press(&pty, key);
        let after = screen(&frame);
        assert_eq!(
            reversed_text(&frame),
            vec!["index.md".to_string()],
            "`{}` must leave the explorer open and unmoved:\n{after}",
            String::from_utf8_lossy(key)
        );
        for name in ["alpha.md", "blocked", "index.md", "sub", "zeta.md"] {
            assert!(
                after.contains(name),
                "`{}` must not have closed or changed the listing:\n{after}",
                String::from_utf8_lossy(key)
            );
        }
    }

    // Esc afterwards must still land on the *unrewrapped* document — proof
    // the chrome keys never reached it either.
    let restored = screen(&press(&pty, b"\x1b"));
    assert_eq!(
        restored, before,
        "the document must come back exactly as it was"
    );
    quit_and_reap(&pty, &mut child);
}

/// **DW-3.6, resize-drain path.** The same chrome table runs a second time
/// inside `main.rs`'s resize debounce loop; `T` read there must be just as
/// inert. Detected the way `toc_key_routing.rs` detects it: the *document's*
/// theme palette, compared before and after, once the explorer closes.
#[test]
fn test_dw_3_6_chrome_keys_stay_inert_on_the_resize_drain_dispatch_path() {
    let dir = fixture("dw-3-6-drain");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[dir.join("index.md").as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"index-filler-00", DEADLINE);

    drain_to_quiet(master);
    pty.resize(30, 100);
    let before_frame = read_one_frame(master, DEADLINE);
    let before_palette = sgr_palette(&before_frame);
    assert!(
        !before_palette.is_empty(),
        "the document must paint some colour, or this test has no oracle"
    );

    let explorer = press(&pty, b"_");
    assert!(
        screen(&explorer).contains("alpha.md"),
        "the explorer must open:\n{explorer}"
    );

    // A resize with a `T` hard on its heels: the sequence that lands a key
    // inside the drain loop rather than the main loop.
    drain_to_quiet(master);
    pty.resize(30, 100);
    pty.type_bytes(b"T");
    let after_explorer = read_one_frame(master, DEADLINE);
    assert!(
        screen(&after_explorer).contains("alpha.md"),
        "the explorer must survive the resize and the key that followed it:\n{after_explorer}"
    );

    let restored_frame = press(&pty, b"\x1b");
    assert!(
        screen(&restored_frame).contains("index-filler-00"),
        "Esc must restore the document"
    );
    assert_eq!(
        sgr_palette(&restored_frame),
        before_palette,
        "a `T` read inside the resize drain reached the document: it came \
         back in the other theme"
    );

    quit_and_reap(&pty, &mut child);
}

/// **DW-3.7.** A resize storm while exploring leaves the explorer addressing
/// a valid row rather than crashing or corrupting the listing; a re-list of
/// a directory that shrank on disk since it was read does the same.
#[test]
fn test_dw_3_7_a_resize_storm_and_a_shrunken_relist_both_leave_a_valid_row() {
    let dir = fixture("dw-3-7");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[dir.join("index.md").as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"index-filler-00", DEADLINE);

    let explorer = press(&pty, b"_");
    assert!(screen(&explorer).contains("alpha.md"));

    // A resize storm: several sizes in quick succession, coalesced by the
    // debounce into one relayout — the explorer must still show something
    // sane afterwards rather than a torn or panicked frame.
    drain_to_quiet(master);
    for (rows, cols) in [(20, 90), (40, 60), (24, 80)] {
        pty.resize(rows, cols);
    }
    let after_storm = read_one_frame(master, DEADLINE);
    assert!(
        screen(&after_storm).contains("alpha.md"),
        "the explorer must still list its entries after a resize storm:\n{after_storm}"
    );

    // Descend into `sub/`, then make it disappear from under the explorer,
    // then re-list it: the shrunk-listing shape. Walk to `sub` with `j`
    // rather than trusting the resize left the selection where `_` put it —
    // `reseat_explore` only clamps, it never renames.
    for _ in 0..5 {
        let frame = press(&pty, b"j");
        if reversed_text(&frame) == vec!["sub/".to_string()] {
            break;
        }
    }
    let inside = screen(&press(&pty, b"\r"));
    assert!(
        inside.contains("inner.md"),
        "must have entered sub/:\n{inside}"
    );

    std::fs::remove_file(dir.join("sub").join("inner.md")).unwrap();
    let up = press(&pty, b"-");
    assert!(
        screen(&up).contains("sub"),
        "ascending must still work after the child directory changed"
    );
    let relisted = screen(&press(&pty, b"\r"));
    assert!(
        !relisted.contains("inner.md"),
        "the removed entry must be gone from the fresh listing:\n{relisted}"
    );

    quit_and_reap(&pty, &mut child);
}

/// **DW-3.8.** `Enter` on a barricade-refused file (valid UTF-8 name, NUL
/// content), and on a path deleted since it was listed, each leave the
/// explorer open with the reason on the status row.
#[test]
fn test_dw_3_8_a_refused_file_and_a_deleted_file_leave_the_explorer_open() {
    let dir = fixture("dw-3-8");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[dir.join("index.md").as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"index-filler-00", DEADLINE);

    let explorer = press(&pty, b"_");
    assert!(screen(&explorer).contains("nulled.md"));
    // Fixture order: `.., alpha.md, blocked, index.md, nulled.md, sub, zeta.md`.
    // From `index.md`, `k` once lands on `nulled.md` (skipping `blocked`
    // going up would be wrong direction — `j` from alpha lands there, but
    // simplest is to walk down from index.md: `j` once).
    let on_nulled = press(&pty, b"j");
    assert_eq!(reversed_text(&on_nulled), vec!["nulled.md".to_string()]);

    let refused = press(&pty, b"\r");
    assert!(
        screen(&refused).contains("nulled.md"),
        "a refusal must leave the explorer open:\n{}",
        screen(&refused)
    );
    let refused_status = status(&refused);
    assert!(
        !refused_status.is_empty() && refused_status != dir.display().to_string(),
        "the refusal reason must be on the status row, not the bare \
         directory: {refused_status:?}"
    );

    // Now the deleted-since-listed case: select `zeta.md`, delete it from
    // disk, then press Enter.
    let mut frame = refused;
    for _ in 0..3 {
        frame = press(&pty, b"j");
        if reversed_text(&frame) == vec!["zeta.md".to_string()] {
            break;
        }
    }
    assert_eq!(reversed_text(&frame), vec!["zeta.md".to_string()]);
    std::fs::remove_file(dir.join("zeta.md")).unwrap();
    let deleted = press(&pty, b"\r");
    assert!(
        screen(&deleted).contains("index.md"),
        "the explorer must still be open after the deleted target:\n{}",
        screen(&deleted)
    );
    assert!(
        !status(&deleted).is_empty(),
        "a deleted target must also report a reason on the status row"
    );

    quit_and_reap(&pty, &mut child);
}

/// **DW-3.9.** A mouse wheel scroll and a left click while the explorer is
/// open change nothing: no scroll of the hidden document, and no link
/// activation at coordinates a link occupied in it.
#[test]
fn test_dw_3_9_mouse_wheel_and_click_are_inert_while_exploring() {
    let dir = fixture("dw-3-9");
    // `index.md` carries no links, so a click landing on the hidden
    // document could only ever scroll it — which is exactly what this test
    // must prove does not happen.
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[dir.join("index.md").as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"index-filler-00", DEADLINE);

    let explorer = screen(&press(&pty, b"_"));

    drain_to_quiet(master);
    pty.type_bytes(&mouse_report(65, 5, 5, true)); // wheel down
    pty.type_bytes(&mouse_report(0, 5, 5, true)); // left click
    pty.type_bytes(&mouse_report(0, 5, 5, false));
    let after_mouse = common::pty::drain_for(master, Duration::from_millis(300));
    assert!(
        after_mouse.is_empty() || !contains(&after_mouse, common::pty::FRAME_END),
        "mouse events must not even trigger a repaint while the explorer \
         owns the keyboard"
    );

    // The explorer is still exactly as it was — proved by a no-op keystroke
    // producing the same listing (a stale mouse-driven scroll of the
    // document behind it would not show here directly, but a corrupted
    // selection would).
    let still = screen(&press(&pty, b"j"));
    let _ = still;
    let back = screen(&press(&pty, b"k"));
    assert_eq!(
        back, explorer,
        "the explorer must be unchanged by the mouse events"
    );

    quit_and_reap(&pty, &mut child);
}

/// **DW-3.10.** The status row shows the listed directory, not the hidden
/// document's name and scroll percentage.
#[test]
fn test_dw_3_10_the_status_row_shows_the_listed_directory() {
    let dir = fixture("dw-3-10");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[dir.join("index.md").as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"index-filler-00", DEADLINE);

    let explorer = press(&pty, b"_");
    let row = status(&explorer);
    // The status row is one 80-column line and a scratch-dir path plus its
    // process-id suffix can run longer than that, so this checks a fragment
    // rather than the whole path — `clip_to_width` may have cut off
    // anything after it, including the trailing digits of the pid.
    assert!(
        row.contains("stele-test-explorer-dw-3-10"),
        "status row must name the listed directory, got {row:?}"
    );
    assert!(
        !row.contains('%'),
        "the explorer's status row must not show a scroll percentage: {row:?}"
    );
    assert!(
        !row.contains("index.md"),
        "the explorer's status row must not name the hidden document: {row:?}"
    );
    quit_and_reap(&pty, &mut child);
}

/// **DW-3.12.** Bare `stele` (no document argument) opens the explorer at
/// the current directory through the real binary; `q` exits and restores
/// the terminal via the extended harness.
///
/// **The frame assertions are the half this test was missing.** It used to
/// be the `assert_restores_the_terminal_in` call alone, which checks
/// alt-screen entry, raw mode and restore — every one of them mode-agnostic.
/// A review deleted the rooted `install_listing` call in a throwaway
/// worktree, so bare `stele` opened no explorer at all, and this test still
/// passed: it was named after a behavior it never looked at. So the frame
/// is read first, and the listing and the status row are asserted on
/// directly; the restore harness follows as the second, separate claim it
/// always was.
#[test]
fn test_dw_3_12_bare_stele_opens_the_explorer_at_cwd_and_q_restores_the_terminal() {
    let dir = fixture("dw-3-12");

    let pty = Pty::open();
    let mut child = spawn_viewer_in_dir(&pty, &[], ChildStdin::Tty, Graphics::Off, &dir);
    let frame = read_one_frame(pty.master_fd(), DEADLINE);
    let body = screen(&frame);
    for name in ["../", "alpha.md", "index.md", "sub/", "zeta.md"] {
        assert!(
            body.contains(name),
            "bare stele must paint the cwd's listing, missing {name}:\n{body}"
        );
    }
    assert_eq!(
        reversed_text(&frame),
        vec!["alpha.md".to_string()],
        "and seat the cursor on the first real entry, not on `../`"
    );
    // A fragment, not the whole path: `getcwd` resolves `/var` to
    // `/private/var` on macOS, and the result plus the scratch tag runs past
    // 80 columns, where `clip_to_width` cuts it.
    assert!(
        status(&frame).contains("stele-test-explorer"),
        "the status row must name the listed directory, got {:?}",
        status(&frame)
    );
    quit_and_reap(&pty, &mut child);

    assert_restores_the_terminal_in("q (bare stele)", Exit::Key(b"q"), &[], Some(&dir));
}

// ---------------------------------------------- relative arguments (F1, pty)
//
// `stele .` shipped a dead end: `Path::new(".").parent()` is `Some("")`, so
// the `../` row pointed at the empty path and one press of `-` — or one
// `Enter` on `../` — landed the reader in `cannot read directory: No such
// file or directory` with no key that could leave it. Every other pty
// fixture here passes an absolute path, which is why nothing caught it.
// These three drive the three relative doors: a relative directory
// argument, `.`, and `_` from a relatively-named document.

/// `stele sub` — a single-component relative directory — ascends with `-`.
#[test]
fn test_a_relative_directory_argument_ascends_with_dash() {
    let dir = fixture("relative-dir-arg");
    let pty = Pty::open();
    let mut child = spawn_viewer_in_dir(
        &pty,
        &[std::ffi::OsStr::new("sub")],
        ChildStdin::Tty,
        Graphics::Off,
        &dir,
    );
    let opened = screen(&read_one_frame(pty.master_fd(), DEADLINE));
    assert!(
        opened.contains("inner.md"),
        "`stele sub` must list `sub`:\n{opened}"
    );

    let up = screen(&press(&pty, b"-"));
    assert!(
        !up.contains("cannot read directory"),
        "`-` from a relative directory must ascend, not fail:\n{up}"
    );
    for name in ["alpha.md", "index.md", "sub/", "zeta.md"] {
        assert!(
            up.contains(name),
            "`-` must land in the parent directory, missing {name}:\n{up}"
        );
    }
    quit_and_reap(&pty, &mut child);
}

/// `stele .` ascends with `Enter` on the `../` row — the other key that
/// reaches the same target, and the one that reads it off the row rather
/// than recomputing it.
#[test]
fn test_a_dot_argument_ascends_with_enter_on_the_parent_row() {
    let dir = fixture("relative-dot-arg");
    let pty = Pty::open();
    let mut child = spawn_viewer_in_dir(
        &pty,
        &[std::ffi::OsStr::new(".")],
        ChildStdin::Tty,
        Graphics::Off,
        &dir.join("sub"),
    );
    let opened = read_one_frame(pty.master_fd(), DEADLINE);
    assert!(
        screen(&opened).contains("inner.md"),
        "`stele .` must list the current directory:\n{}",
        screen(&opened)
    );

    // `k` from the only real entry lands on `../`; `Enter` there ascends.
    let on_parent = press(&pty, b"k");
    assert_eq!(reversed_text(&on_parent), vec!["../".to_string()]);
    let up = screen(&press(&pty, b"\r"));
    assert!(
        !up.contains("cannot read directory"),
        "`Enter` on `../` from `.` must ascend, not fail:\n{up}"
    );
    for name in ["alpha.md", "index.md", "sub/", "zeta.md"] {
        assert!(
            up.contains(name),
            "`Enter` on `../` must land in the parent, missing {name}:\n{up}"
        );
    }
    quit_and_reap(&pty, &mut child);
}

/// `stele inner.md` — a document named with no directory at all, so
/// `base_dir()` answers `"."` — opens an explorer with a working `../`.
#[test]
fn test_underscore_from_a_relatively_named_document_ascends() {
    let dir = fixture("relative-doc-arg");
    let pty = Pty::open();
    let mut child = spawn_viewer_in_dir(
        &pty,
        &[std::ffi::OsStr::new("inner.md")],
        ChildStdin::Tty,
        Graphics::Off,
        &dir.join("sub"),
    );
    read_until(pty.master_fd(), b"inner-document-marker", DEADLINE);

    let explorer = press(&pty, b"_");
    assert_eq!(
        reversed_text(&explorer),
        vec!["inner.md".to_string()],
        "`_` seats on the open document's own row"
    );
    // `base_dir()` answers `"."` for a document named with no directory, and
    // `main.rs` normalizes it on the way in. The status row is where that
    // shows: a bare `.` here would mean the explorer is running on a path
    // whose own name it cannot report.
    assert!(
        status(&explorer).starts_with('/'),
        "the listed directory must be named absolutely, got {:?}",
        status(&explorer)
    );

    let up = press(&pty, b"-");
    let body = screen(&up);
    assert!(
        !body.contains("cannot read directory"),
        "`-` after `_` from a bare filename must ascend, not fail:\n{body}"
    );
    for name in ["alpha.md", "index.md", "sub/", "zeta.md"] {
        assert!(
            body.contains(name),
            "`-` must land in the parent, missing {name}:\n{body}"
        );
    }
    // And the retrace works: the directory just left takes the cursor, so
    // `Enter` goes straight back down. That lookup is by the leaving
    // directory's `file_name`, and `Path::new(".").file_name()` is `None` —
    // so an unnormalized `"."` loses the reader's place silently.
    assert_eq!(
        reversed_text(&up),
        vec!["sub/".to_string()],
        "`-` must seat the cursor on the directory just left"
    );
    quit_and_reap(&pty, &mut child);
}

/// **DW-3.13, pty half.** `stele <dir>` opens the explorer and the rendered
/// frame never shows the "Is a directory" text `load_with`/`read_to_end`
/// would have produced had the directory reached it.
#[test]
fn test_dw_3_13_a_directory_argument_never_shows_is_a_directory() {
    let dir = fixture("dw-3-13");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[dir.as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    let frame = read_one_frame(master, DEADLINE);
    let body = screen(&frame);
    assert!(
        body.contains("alpha.md"),
        "stele <dir> must open the explorer:\n{body}"
    );
    assert!(
        !frame.contains("Is a directory") && !frame.contains("is a directory"),
        "the loader's own error text leaked through:\n{frame}"
    );
    quit_and_reap(&pty, &mut child);
}

/// **DW-3.15.** In a rooted explorer, `Esc` and `q` both quit rather than
/// revealing the empty placeholder document, which is never painted; and
/// `Backspace` after opening one file from it reports `AtRoot` rather than
/// trying to re-read a `Scratch` source.
#[test]
fn test_dw_3_15_rooted_esc_and_q_quit_without_ever_painting_the_empty_document() {
    let dir = fixture("dw-3-15-esc");
    assert_restores_the_terminal_in("Esc (rooted)", Exit::Key(b"\x1b"), &[dir.as_os_str()], None);

    let dir2 = fixture("dw-3-15-q");
    assert_restores_the_terminal_in("q (rooted)", Exit::Key(b"q"), &[dir2.as_os_str()], None);
}

/// DW-3.15's second half: `Backspace` from the first file opened out of a
/// rooted explorer reports `AtRoot`, not an attempt to re-read a document
/// that was never on disk.
#[test]
fn test_dw_3_15_backspace_after_opening_one_file_rooted_reports_at_root() {
    let dir = fixture("dw-3-15-backspace");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[dir.as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    let startup = read_one_frame(master, DEADLINE);
    assert!(
        screen(&startup).contains("alpha.md"),
        "a rooted launch must open the explorer:\n{}",
        screen(&startup)
    );
    // `default_selection` (main.rs) seats a fresh, unseeded listing on the
    // first *real* entry rather than `../` — `alpha.md` here — which is what
    // makes a bare `Enter` below mean "open alpha.md" rather than "ascend
    // out of the fixture directory into its parent".
    assert_eq!(
        reversed_text(&startup),
        vec!["alpha.md".to_string()],
        "a rooted launch must seat the cursor on the first real entry, not `../`"
    );

    let opened = screen(&press(&pty, b"\r"));
    assert!(
        opened.contains("alpha-document-marker"),
        "Enter on alpha.md must open it:\n{opened}"
    );

    let back = press(&pty, &[0x7f]);
    let message = status(&back);
    assert!(
        message.to_lowercase().contains("first document"),
        "Backspace out of the first document opened from a rooted explorer \
         must report AtRoot, got: {message:?}"
    );
    quit_and_reap(&pty, &mut child);
}

/// **DW-3.16.** An unreadable (but existing) cwd shows a named notice
/// inside the explorer rather than a blank screen or a panic — the design
/// this phase chose over exiting before the terminal opens (see the phase
/// discovery notes): `explore::Listing::read`'s own total error handling is
/// what a running explorer already relies on for a directory that becomes
/// unreadable *during* the session, and this is the same mechanism reached
/// on the very first read.
#[test]
fn test_dw_3_16_an_unreadable_cwd_shows_a_named_notice_instead_of_a_blank_explorer() {
    let dir = common::fixtures::scratch_dir("dw-3-16-unreadable");
    // Execute-only: `chdir`/`spawn` into it still succeeds, but `read_dir`
    // does not — the "unreadable cwd" DW-3.16 names, as opposed to a
    // deleted one (which `spawn` itself could not survive to test through).
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o100)).unwrap();

    let pty = Pty::open();
    let mut child = spawn_viewer_in_dir(&pty, &[], ChildStdin::Tty, Graphics::Off, &dir);
    let master = pty.master_fd();
    let frame = read_one_frame(master, DEADLINE);

    assert!(
        frame.to_lowercase().contains("cannot read directory"),
        "an unreadable cwd must show a named notice, got:\n{}",
        screen(&frame)
    );

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
}

/// **DW-3.16, and the evidence behind this pass's decision to keep the
/// behavior the item's literal wording does not describe.**
///
/// The item says an unreadable cwd should "exit with a named error rather
/// than an empty explorer or a panic". What the binary does instead is open
/// the explorer, name the error on a notice row, and leave the `../` row
/// working. The two failure modes DW-3.16 actually names — an *empty*
/// explorer and a panic — are both avoided; refusing to start is a third
/// thing, and a worse one, because a reader whose cwd is unreadable would be
/// left with no way to reach a directory that is. This test is the proof
/// that the alternative is real: the reader ascends out under their own
/// power. The plan's wording is corrected to match rather than the code.
#[test]
fn test_dw_3_16_an_unreadable_cwd_can_still_be_escaped_upward() {
    let dir = fixture("dw-3-16-escape");
    let locked = dir.join("locked");
    std::fs::create_dir(&locked).unwrap();
    // Execute-only: `chdir` into it succeeds, `read_dir` does not.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o100)).unwrap();

    let pty = Pty::open();
    let mut child = spawn_viewer_in_dir(&pty, &[], ChildStdin::Tty, Graphics::Off, &locked);
    let frame = read_one_frame(pty.master_fd(), DEADLINE);
    assert!(
        frame.to_lowercase().contains("cannot read directory"),
        "the notice names the error:\n{}",
        screen(&frame)
    );
    assert_eq!(
        reversed_text(&frame),
        vec!["../".to_string()],
        "with nothing else selectable, the cursor seats on the way out"
    );

    let up = screen(&press(&pty, b"-"));
    for name in ["alpha.md", "index.md", "locked/", "zeta.md"] {
        assert!(
            up.contains(name),
            "`-` must escape an unreadable cwd into its readable parent, \
             missing {name}:\n{up}"
        );
    }

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700));
}
