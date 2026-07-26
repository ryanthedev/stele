//! DW-6.1 / DW-6.2 / DW-6.6 / DW-6.8 at the level the routing lives: link
//! selection, following, `Backspace`, the wheel and the capture toggle driven
//! through the **real binary's** event loop, not through `AppState`.
//!
//! An `AppState` test can prove the key table and still miss everything that
//! makes these features work: `main.rs` offers every press to
//! `handle_chrome_key` first (a `true` there means `AppState` never sees the
//! key), the document swap happens in `Session::install_document` where
//! `AppState` cannot reach, and mouse reports only exist because
//! `TerminalGuard::enter` put the terminal into a mode that emits them. All
//! four are outside the unit tests by construction, and `toc_key_routing.rs`
//! exists because the first three of those seams had already produced a defect
//! once.
//!
//! Harness hazards observed here, all documented in `common/pty.rs`: every
//! keystroke goes through `press`, which drains the wire first so the frame
//! read back is the one that keystroke produced rather than a leftover; and
//! sessions end with `q` after a drain rather than `Esc`-then-`q`, which the
//! terminal parses as a single `Alt+q`.

#![cfg(unix)]

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
const STATUS_ROW: u16 = 24;

const DEADLINE: Duration = Duration::from_secs(5);

/// A scratch directory holding a small linked document set.
///
/// `index.md` opens with 40 filler lines so the link paragraph is only
/// reachable by scrolling — which is what makes DW-6.2's "returns to the
/// previous scroll position" a claim with something to be wrong about. Every
/// marker is unique to its document so a frame can never be mistaken for
/// another one's.
fn fixture(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("stele-link-it-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create fixture dir");

    let mut index = String::from("# Index\n\n");
    for i in 0..40 {
        index.push_str(&format!("index-filler-{i:02}\n\n"));
    }
    index.push_str("Follow [chapter two](second.md) or [the notes](notes.txt) now.\n\n");
    index.push_str("index-tail-marker\n");
    std::fs::write(dir.join("index.md"), index).expect("write index.md");

    std::fs::write(
        dir.join("second.md"),
        "# Second\n\nsecond-document-marker\n",
    )
    .expect("write second.md");
    std::fs::write(dir.join("notes.txt"), "notes-plain-text-marker\n").expect("write notes.txt");

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

/// Every run of glyphs painted with the reverse-video attribute.
///
/// `Painter::paint_run` writes `\x1b[7m` immediately before the run's clipped
/// text and every following run opens with its own SGR reset, so the
/// characters between a `\x1b[7m` and the next escape byte are exactly what
/// the terminal shows reversed. Searching for the attribute alone would not
/// be the same claim — it can be set and reset around nothing at all.
fn reversed_text(frame: &str) -> Vec<String> {
    frame
        .split("\u{1b}[7m")
        .skip(1)
        .map(|rest| rest.split('\u{1b}').next().unwrap_or("").to_string())
        .filter(|text| !text.is_empty())
        .collect()
}

/// Types `keys` and returns the frame it produced, with the wire drained first
/// so the frame read back is this keystroke's and not a leftover.
fn press(pty: &Pty, keys: &[u8]) -> String {
    drain_to_quiet(pty.master_fd());
    pty.type_bytes(keys);
    read_one_frame(pty.master_fd(), DEADLINE)
}

/// An SGR mouse report, as a terminal in mode `?1006` sends it. `button` is
/// 0 for the left button and 64/65 for a wheel notch up/down; `col`/`row` are
/// 1-based.
fn mouse_report(button: u16, col: u16, row: u16, press: bool) -> Vec<u8> {
    format!(
        "\x1b[<{button};{col};{row}{}",
        if press { 'M' } else { 'm' }
    )
    .into_bytes()
}

fn quit_and_reap(pty: &Pty, child: &mut std::process::Child) {
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

/// **DW-6.1, through the real key path.** `Tab` selects a visible link and
/// paints an indicator on it; `Shift-Tab` moves the indicator to the other
/// link on the row rather than merely toggling something.
#[test]
fn test_dw_6_1_tab_selects_a_link_through_the_real_binarys_key_path() {
    let dir = fixture("select");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[dir.join("index.md").as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"index-filler-00", DEADLINE);

    // Scroll to the tail, where both links are on screen.
    let tail = press(&pty, b"G");
    assert!(
        screen(&tail).contains("index-tail-marker"),
        "the fixture's tail must be on screen:\n{}",
        screen(&tail)
    );
    assert!(
        reversed_text(&tail).is_empty(),
        "nothing is selected before `Tab`: {:?}",
        reversed_text(&tail)
    );

    let selected = press(&pty, b"\t");
    let reversed = reversed_text(&selected);
    assert!(
        reversed.iter().any(|text| text.contains("chapter")),
        "`Tab` must put a visible indicator on the first link, got {reversed:?}"
    );
    assert!(
        status(&selected).contains("second.md"),
        "the status row must name where Enter would go:\n{}",
        status(&selected)
    );

    // Shift-Tab (CSI Z) moves to the other link on the row.
    let backward = press(&pty, b"\x1b[Z");
    assert!(
        reversed_text(&backward)
            .iter()
            .any(|text| text.contains("notes")),
        "`Shift-Tab` must move the indicator, got {:?}",
        reversed_text(&backward)
    );

    // Esc leaves selection with the document unchanged.
    let dismissed = press(&pty, b"\x1b");
    assert!(
        reversed_text(&dismissed).is_empty(),
        "`Esc` must take the indicator away: {:?}",
        reversed_text(&dismissed)
    );
    assert_eq!(
        screen(&dismissed),
        screen(&tail),
        "and must leave the reader exactly where they were"
    );

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **DW-6.2, end to end.** `Enter` on a relative markdown link opens it in
/// place, and `Backspace` comes back to the previous document *at the scroll
/// position it was left at* — asserted by comparing the returned screen to the
/// one the reader left, byte for byte.
#[test]
fn test_dw_6_2_enter_opens_a_relative_markdown_link_and_backspace_returns_to_the_scroll_position() {
    let dir = fixture("follow");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[dir.join("index.md").as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"index-filler-00", DEADLINE);

    let before = press(&pty, b"G");
    let before_screen = screen(&before);
    assert!(
        before_screen.contains("index-tail-marker"),
        "the departure point must be a scrolled position, not the top:\n{before_screen}"
    );
    assert!(
        !before_screen.contains("index-filler-00"),
        "…and the top must genuinely be off screen, or the round trip proves \
         nothing:\n{before_screen}"
    );

    press(&pty, b"\t");
    let opened = press(&pty, b"\r");
    let opened_screen = screen(&opened);
    assert!(
        opened_screen.contains("second-document-marker"),
        "`Enter` must open the linked document in place:\n{opened_screen}"
    );
    assert!(
        !opened_screen.contains("index-tail-marker"),
        "…and the previous document must be gone from the screen:\n{opened_screen}"
    );
    // The status row's document name is deliberately not asserted here: it is
    // the full path, and the scratch directory's path is already wider than
    // the 80-cell row, so the name is clipped off every time. The unit test
    // `test_open_document_lands_at_the_requested_scroll_and_clears_the_mode`
    // is where the name is checked, unclipped.

    // 0x7F is what a terminal sends for Backspace.
    let returned = press(&pty, b"\x7f");
    assert_eq!(
        screen(&returned),
        before_screen,
        "DW-6.2: `Backspace` must return to the previous document at the \
         previous scroll position — same document, same offset, same rows"
    );

    // And at the root there is nowhere left to go, said rather than ignored.
    let at_root = press(&pty, b"\x7f");
    assert!(
        status(&at_root).contains("already at the first document"),
        "`Backspace` at the root must report:\n{}",
        status(&at_root)
    );
    assert_eq!(
        screen(&at_root),
        before_screen,
        "…and must not move the reader"
    );

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **DW-6.8, end to end.** A `.txt` target is the permissive policy's third
/// leg: it opens in place and joins the same stack a markdown target does.
#[test]
fn test_dw_6_8_a_non_markdown_target_opens_in_place_and_joins_the_document_stack() {
    let dir = fixture("txt");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[dir.join("index.md").as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"index-filler-00", DEADLINE);

    let before = press(&pty, b"G");
    let before_screen = screen(&before);

    // The second link on the row is the `.txt` one.
    press(&pty, b"\t");
    press(&pty, b"\t");
    let opened = press(&pty, b"\r");
    assert!(
        screen(&opened).contains("notes-plain-text-marker"),
        "a .txt target must open in place, exactly as markdown does:\n{}",
        screen(&opened)
    );

    let returned = press(&pty, b"\x7f");
    assert_eq!(
        screen(&returned),
        before_screen,
        "DW-6.8: it joins the document stack, so Backspace comes back"
    );

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **DW-6.4, end to end.** A refused target leaves the current document
/// rendered and says why — the property that matters is that the reader does
/// not lose their place to a bad link.
#[test]
fn test_dw_6_4_a_refused_target_reports_and_leaves_the_document_rendered() {
    let dir = std::env::temp_dir().join(format!("stele-link-refuse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    std::fs::create_dir_all(dir.join("chapters")).expect("create a directory target");
    std::fs::write(
        dir.join("index.md"),
        "# Index\n\nrefusal-body-marker\n\nA [missing one](nope.md), a \
         [directory](chapters) and a [scheme](mailto:a@example.com).\n",
    )
    .expect("write index.md");

    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[dir.join("index.md").as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"refusal-body-marker", DEADLINE);
    drain_to_quiet(master);
    let before = screen(&press(&pty, b"k"));

    for (nth, expected) in [
        (1usize, "no such file"),
        (2, "not a regular file"),
        (3, "http/https only"),
    ] {
        let mut frame = press(&pty, b"\t");
        for _ in 1..nth {
            frame = press(&pty, b"\t");
        }
        let _ = frame;
        let refused = press(&pty, b"\r");
        assert!(
            status(&refused).contains(expected),
            "link {nth} must be refused with {expected:?}, status was:\n{}",
            status(&refused)
        );
        assert!(
            screen(&refused).contains("refusal-body-marker"),
            "DW-6.4: the current document must still be rendered:\n{}",
            screen(&refused)
        );
    }

    // The document is untouched after all three refusals.
    let after = press(&pty, b"k");
    assert_eq!(
        screen(&after),
        before,
        "three refused links must have changed nothing about the document"
    );

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **DW-6.6, end to end.** A wheel report scrolls the viewport, a click on a
/// link activates it, a click on a non-link cell does nothing, and `m` toggles
/// capture with the mode-setting bytes actually reaching the wire.
#[test]
fn test_dw_6_6_the_wheel_scrolls_and_a_click_activates_a_link_in_the_real_binary() {
    let dir = fixture("mouse");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[dir.join("index.md").as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"index-filler-00", DEADLINE);
    drain_to_quiet(master);

    // The wheel scrolls, and the direction is right.
    let top = press(&pty, b"k");
    assert!(render_row(&top, 1, COLS).contains("Index"));
    let scrolled = press(&pty, &mouse_report(65, 1, 1, true));
    assert_ne!(
        render_row(&scrolled, 1, COLS),
        render_row(&top, 1, COLS),
        "a wheel-down report must scroll the viewport"
    );
    let back = press(&pty, &mouse_report(64, 1, 1, true));
    assert_eq!(
        render_row(&back, 1, COLS),
        render_row(&top, 1, COLS),
        "a wheel-up report must scroll back, and clamp at the top"
    );

    // Find the link on the tail screen and click it. Row and column are
    // 1-based on the wire; the search below reads them off the painted grid,
    // so the click lands where the glyphs actually are.
    let tail = press(&pty, b"G");
    let (row, col) = (1..=CONTENT_ROWS)
        .find_map(|row| {
            let text = render_row(&tail, row, COLS);
            text.find("chapter two").map(|col| (row, col as u16 + 1))
        })
        .expect("the link text must be on the tail screen");

    let clicked = press(&pty, &mouse_report(0, col, row, true));
    assert!(
        screen(&clicked).contains("second-document-marker"),
        "DW-6.6: a click on a link must open it:\n{}",
        screen(&clicked)
    );

    // Back, then a click on a cell with no link changes nothing at all.
    let returned = press(&pty, b"\x7f");
    let returned_screen = screen(&returned);
    drain_to_quiet(master);
    pty.type_bytes(&mouse_report(0, 1, 1, true));
    pty.type_bytes(&mouse_report(0, 1, 1, false));
    // A click that changes nothing earns no frame, so there is nothing to wait
    // for — a following no-op keystroke is what produces one.
    let after_dead_click = press(&pty, b"G");
    assert_eq!(
        screen(&after_dead_click),
        returned_screen,
        "a click on a cell with no link must do nothing"
    );

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **DW-6.6's toggle**, asserted on the wire rather than on a status message:
/// `m` must actually put the mode-reset bytes on the terminal, or the reader's
/// text selection does not come back however cheerful the status row is.
#[test]
fn test_dw_6_6_m_puts_the_mouse_mode_bytes_on_the_wire_both_ways() {
    let dir = fixture("toggle");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[dir.join("index.md").as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"index-filler-00", DEADLINE);
    drain_to_quiet(master);

    pty.type_bytes(b"m");
    let off = read_until(master, b"\x1b[?1000l", DEADLINE);
    assert!(
        contains(&off, b"\x1b[?1006l") && contains(&off, b"\x1b[?1000l"),
        "`m` must disable every mode entry enabled; wire was {off:?}"
    );

    drain_to_quiet(master);
    pty.type_bytes(b"m");
    let on = read_until(master, b"\x1b[?1000h", DEADLINE);
    assert!(
        contains(&on, b"\x1b[?1000h") && contains(&on, b"\x1b[?1006h"),
        "`m` again must re-enable them; wire was {on:?}"
    );

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **The routing claim**, the shape `toc_key_routing.rs` established: a chrome
/// key pressed while a link is selected must not reach the document
/// underneath it.
///
/// `+`/`-`/`T` and Phase 5's `z`/`R`/`M` are all handled in `main.rs` ahead of
/// `AppState`; ungated they relay out while the reader is looking at a live
/// selection whose index was computed from the old line layout. The observable
/// difference is on the far side of `Esc`: the document the reader comes back
/// to. `M` is the sharpest of them — an ungated collapse-all would take the
/// selected link off the screen on a keystroke that was never about links.
#[test]
fn test_dw_6_1_a_chrome_key_pressed_under_a_link_selection_does_not_reach_the_document() {
    let dir = fixture("routing");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[dir.join("index.md").as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"index-filler-00", DEADLINE);

    let before = screen(&press(&pty, b"G"));
    assert!(before.contains("index-tail-marker"));

    let selected = press(&pty, b"\t");
    assert!(
        reversed_text(&selected)
            .iter()
            .any(|text| text.contains("chapter")),
        "the selection must be up before the chrome keys are tried"
    );

    // Every chrome key, tried under the selection. `-` clamps at `min_width`
    // so the widths do not cancel; `z`/`M`/`R` are Phase 5's fold keys, which
    // landed *after* this mode was written and would each relay out — `M`
    // most visibly, since collapsing every section would take the link the
    // indicator is on off the screen entirely.
    for key in [&b"-"[..], b"-", b"-", b"+", b"z", b"M", b"R"] {
        let after = press(&pty, key);
        assert!(
            reversed_text(&after)
                .iter()
                .any(|text| text.contains("chapter")),
            "`{}` must leave the selection up",
            String::from_utf8_lossy(key)
        );
    }

    let restored = press(&pty, b"\x1b");
    assert_eq!(
        screen(&restored),
        before,
        "a `+`/`-` pressed under a link selection reached the document: it \
         came back laid out differently"
    );

    // ...and the mode still answers its own keys, which is the opposite
    // failure: a gate that swallowed navigation too.
    press(&pty, b"\t");
    let again = press(&pty, b"\t");
    assert!(
        reversed_text(&again)
            .iter()
            .any(|text| text.contains("notes")),
        "the selection must still cycle: {:?}",
        reversed_text(&again)
    );

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **DW-6.7, end to end.** `y` must put a well-formed OSC 52 on the wire
/// carrying exactly the code block's bytes.
///
/// Asserted on the terminal's own input rather than on a return value: the
/// clipboard write is not a frame, it is a sequence written between frames,
/// and "the function built a string" is a different claim from "the terminal
/// received it".
#[test]
fn test_dw_6_7_y_puts_the_code_blocks_bytes_on_the_wire_as_osc_52() {
    use base64::Engine as _;

    const FENCE: &str = "curl https://example.com/install.sh | sh\nmake test\n";

    let dir = std::env::temp_dir().join(format!("stele-yank-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    let path = dir.join("code.md");
    std::fs::write(
        &path,
        format!("# Code\n\nyank-body-marker\n\n```sh\n{FENCE}```\n"),
    )
    .expect("write fixture");

    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[path.as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"yank-body-marker", DEADLINE);
    drain_to_quiet(master);

    pty.type_bytes(b"y");
    let wire = read_until(master, b"\x1b]52;c;", DEADLINE);
    let text = String::from_utf8_lossy(&wire).into_owned();
    let body = text
        .split("\u{1b}]52;c;")
        .nth(1)
        .and_then(|rest| rest.split("\u{1b}\\").next())
        .unwrap_or_else(|| panic!("no OSC 52 sequence on the wire: {text:?}"));

    let decoded = base64::engine::general_purpose::STANDARD
        .decode(body)
        .expect("the payload must be valid base64");
    assert_eq!(
        String::from_utf8_lossy(&decoded),
        FENCE,
        "DW-6.7: the clipboard must carry exactly the code block's text — not \
         the painted (and possibly clipped) lines, and not the fence markers"
    );

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every distinct SGR sequence in `frame`. The theme's palette and the search
/// roles reach the wire only as colour escapes, and `render_row` deliberately
/// throws those away — so this is the channel they show up on.
fn sgr_palette(frame: &str) -> std::collections::BTreeSet<String> {
    let mut found = std::collections::BTreeSet::new();
    let chars: Vec<char> = frame.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\u{1b}' && chars.get(i + 1) == Some(&'[') {
            let mut j = i + 2;
            while j < chars.len() && !chars[j].is_ascii_alphabetic() {
                j += 1;
            }
            if chars.get(j) == Some(&'m') {
                found.insert(chars[i + 2..j].iter().collect::<String>());
            }
            i = j + 1;
            continue;
        }
        i += 1;
    }
    found
}

/// **The routing claim, resize-drain path.** `main.rs` runs the identical
/// chrome-then-app ordering inside the resize debounce, so a key read there
/// has to take the same route as one read idle — and Phase 2's own history
/// (a `T` read mid-drag offered only to `handle_key_event` and dropped) is
/// the reason to check rather than assume.
///
/// Which path a given keystroke lands on is inherently racy — the drain is a
/// 50 ms quiet window — so this asserts only what must hold **either way**:
/// `Tab` is not lost, and `T` does not reach the document. A test that
/// asserted "it went through the drain" would be asserting the race.
#[test]
fn test_dw_6_1_a_key_read_during_a_resize_drain_takes_the_same_route_as_an_idle_one() {
    let dir = fixture("drain");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[dir.join("index.md").as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"index-filler-00", DEADLINE);

    // Settle at the post-resize geometry first, so the before/after
    // comparison is between two frames of the same size.
    drain_to_quiet(master);
    pty.resize(30, 100);
    read_one_frame(master, DEADLINE);
    let before_frame = press(&pty, b"G");
    let before_rows = (1..=29)
        .map(|row| render_row(&before_frame, row, 100))
        .collect::<Vec<_>>()
        .join("\n");
    let before_palette = sgr_palette(&before_frame);
    assert!(
        !before_palette.is_empty(),
        "the document must paint some colour, or this test has no oracle"
    );

    // A resize with `Tab` hard on its heels: the sequence that can land a key
    // inside the drain loop. The size must genuinely change or no SIGWINCH is
    // delivered and there is no drain at all.
    drain_to_quiet(master);
    pty.resize(29, 100);
    pty.type_bytes(b"\t");
    let selected = read_one_frame(master, DEADLINE);
    let _ = selected;
    // Let the burst settle, then read a fresh frame: whichever path the key
    // took, the selection must exist by now.
    let settled = press(&pty, b"\t");
    assert!(
        !reversed_text(&settled).is_empty(),
        "`Tab` read during a resize drain must not be lost — no indicator \
         appeared at all"
    );

    // ...and a chrome key read the same way must still not reach the
    // document underneath the selection.
    drain_to_quiet(master);
    pty.resize(30, 100);
    pty.type_bytes(b"T");
    read_one_frame(master, DEADLINE);
    let after = press(&pty, b"\x1b");
    let after_rows = (1..=29)
        .map(|row| render_row(&after, row, 100))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        after_rows, before_rows,
        "a `T` read during a resize drain reached the document under a link \
         selection"
    );
    assert_eq!(
        sgr_palette(&after),
        before_palette,
        "…and it came back in the other theme"
    );

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **Search and link selection compose, through the real binary.**
///
/// Phase 4's search state outlives `Mode::Search` so `n`/`N` work from normal
/// mode, which means a reader can accept a query and then press `Tab`. Both
/// overlays are live at once and `main.rs::paint` routes every document mode
/// through one call — this is what would fail if it ever grew a second
/// wrapper: the search highlighting would vanish the moment a link was
/// selected, on a keystroke that was never about the search.
#[test]
fn test_dw_6_1_a_link_selection_over_an_accepted_search_keeps_both_highlights() {
    let dir = fixture("compose");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[dir.join("index.md").as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"index-filler-00", DEADLINE);

    // At the tail, where the links live. `tail` matches exactly once and on
    // this screen, so accepting the query does not move the viewport away
    // from the links.
    let plain = press(&pty, b"G");
    let plain_palette = sgr_palette(&plain);

    press(&pty, b"/");
    for byte in b"tail" {
        press(&pty, &[*byte]);
    }
    let searched = press(&pty, b"\r");
    assert!(
        screen(&searched).contains("index-tail-marker"),
        "the accepted query must leave the reader on the matching screen:\n{}",
        screen(&searched)
    );
    let search_palette = sgr_palette(&searched);
    let search_only: Vec<String> = search_palette.difference(&plain_palette).cloned().collect();
    assert!(
        !search_only.is_empty(),
        "the match must paint in a style the unsearched frame did not use, or \
         this test has no oracle: {search_palette:?}"
    );

    // Now select a link. Both overlays must be on the frame.
    let both = press(&pty, b"\t");
    assert!(
        reversed_text(&both)
            .iter()
            .any(|text| text.contains("chapter")),
        "the link indicator must be painted: {:?}",
        reversed_text(&both)
    );
    let both_palette = sgr_palette(&both);
    for sgr in &search_only {
        assert!(
            both_palette.contains(sgr),
            "selecting a link dropped the search highlighting — SGR {sgr:?} \
             was on the searched frame and is gone from this one"
        );
    }

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **The security review's blocker, on the real binary.** Open `index.md`,
/// follow a link to `second.md`, replace `index.md` with a FIFO, press
/// `Backspace`.
///
/// Before the fix this wedged the process: `Navigator::back` called the
/// command-line loader, which goes straight to `File::open`, and `open` on a
/// FIFO with no writer blocks forever. The event loop never returned — no key
/// read, no frame painted, `q` and Ctrl-C inert, every stack sample inside
/// `open()` — and the process had to be killed from outside.
///
/// The assertion is therefore not only "it refused" but "**it was still
/// alive to refuse**": the viewer answers on the status row, keeps rendering
/// the child document, and still quits on `q`. A regression hangs this test
/// rather than failing it, so the reap at the end is bounded by the harness's
/// own deadline.
#[test]
fn test_dw_6_4_a_parent_replaced_by_a_fifo_does_not_wedge_the_viewer_on_backspace() {
    let dir = fixture("back-fifo");
    let index = dir.join("index.md");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[index.as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    read_until(master, b"index-filler-00", DEADLINE);

    // Into the child document.
    press(&pty, b"G");
    press(&pty, b"\t");
    let opened = press(&pty, b"\r");
    assert!(
        screen(&opened).contains("second-document-marker"),
        "the link must open before the parent is sabotaged:\n{}",
        screen(&opened)
    );

    // Replace the parent with a FIFO that has no writer.
    std::fs::remove_file(&index).expect("remove the parent");
    let made = std::process::Command::new("mkfifo")
        .arg(&index)
        .status()
        .expect("run mkfifo");
    assert!(made.success(), "mkfifo failed: {made:?}");

    // `Backspace`. This is the keystroke that used to never come back.
    let refused = press(&pty, b"\x7f");
    assert!(
        status(&refused).contains("not a regular file"),
        "the viewer must refuse and say so, status was:\n{}",
        status(&refused)
    );
    assert!(
        screen(&refused).contains("second-document-marker"),
        "…and must still be rendering the document the reader is in:\n{}",
        screen(&refused)
    );

    // Still alive and still reading keys — the part a hang would fail.
    let scrolled = press(&pty, b"j");
    assert!(
        !screen(&scrolled).is_empty(),
        "the viewer must still be painting frames after the refusal"
    );

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::remove_file(&index);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The same hang through the other in-loop door: `--watch` on a path that is
/// replaced by a FIFO. The reload runs inside the event loop in raw mode, so
/// a blocking open there is exactly as unescapable.
#[test]
fn test_dw_6_4_a_watched_path_replaced_by_a_fifo_does_not_wedge_the_viewer() {
    let dir = std::env::temp_dir().join(format!("stele-watch-fifo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    let path = dir.join("doc.md");
    std::fs::write(&path, "# Watched\n\nwatch-body-marker\n").expect("write fixture");

    let pty = Pty::open();
    let mut child = spawn_viewer(
        &pty,
        &[std::ffi::OsStr::new("--watch"), path.as_os_str()],
        ChildStdin::Tty,
    );
    let master = pty.master_fd();
    read_until(master, b"watch-body-marker", DEADLINE);
    drain_to_quiet(master);

    std::fs::remove_file(&path).expect("remove the watched file");
    let made = std::process::Command::new("mkfifo")
        .arg(&path)
        .status()
        .expect("run mkfifo");
    assert!(made.success(), "mkfifo failed: {made:?}");

    // The watch tick is owed within 250 ms; the refusal must arrive on the
    // status row rather than the loop disappearing into `open()`.
    let frame = read_one_frame(master, DEADLINE);
    assert!(
        status(&frame).contains("reload failed"),
        "the reload must refuse rather than block, status was:\n{}",
        status(&frame)
    );
    assert!(
        screen(&frame).contains("watch-body-marker"),
        "…and the last good render must stay on screen (DW-2.4):\n{}",
        screen(&frame)
    );

    // Still reading keys.
    let after = press(&pty, b"j");
    assert!(!screen(&after).is_empty());

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir_all(&dir);
}

/// **The security review's fourth blocker, and the reason no gate caught it.**
///
/// stele gates images on `TERM_PROGRAM=ghostty`, and the shared pty harness
/// removes that variable — so `ImageSizer`, `GfxMediaSink` and everything
/// under them were *unreachable from the entire test suite*. Four rounds of
/// review ran with the media interface never executing. This is the test that
/// makes it reachable.
///
/// The defect: `gfx::decode` opened a document-supplied image path with no
/// `stat`, so `![x](pipe.png)` naming a writerless FIFO blocked `open(2)`
/// forever, inside the event loop, under raw mode — `q` and Ctrl-C inert,
/// process killed from outside. The route that matters is this one: following
/// a link lays out a document a stranger chose, so the same author controls
/// both the link and the image path behind it.
///
/// The fixture carries a **real PNG as well**, and the assertion that a kitty
/// graphics payload reaches the wire is what proves graphics were genuinely
/// on — without it this test could pass while testing nothing at all.
#[test]
fn test_dw_6_4_a_fifo_image_in_a_linked_document_does_not_wedge_the_viewer() {
    use common::pty::{Graphics, spawn_viewer_with};

    let dir = std::env::temp_dir().join(format!("stele-fifo-image-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create fixture dir");

    // A real image, so the graphics path demonstrably runs...
    common::fixtures::write_png(&dir, "real.png", 48, 48);
    // ...and a FIFO wearing a .png name, which is the hostile half.
    let pipe = dir.join("pipe.png");
    let made = std::process::Command::new("mkfifo")
        .arg(&pipe)
        .status()
        .expect("run mkfifo");
    assert!(made.success(), "mkfifo failed: {made:?}");

    std::fs::write(
        dir.join("index.md"),
        "# Index\n\nindex-marker\n\n[pics](pics.md)\n",
    )
    .expect("write index.md");
    std::fs::write(
        dir.join("pics.md"),
        "# Pics\n\npics-marker\n\n![real](real.png)\n\n![trap](pipe.png)\n",
    )
    .expect("write pics.md");

    let pty = Pty::open();
    let mut child = spawn_viewer_with(
        &pty,
        &[dir.join("index.md").as_os_str()],
        ChildStdin::Tty,
        Graphics::On,
    );
    let master = pty.master_fd();
    read_until(master, b"index-marker", DEADLINE);

    // Follow the link. This is the keystroke that used to never come back —
    // the layout probe alone reaches `gfx::decode` for every image in the
    // document being opened.
    press(&pty, b"\t");
    let opened = press(&pty, b"\r");
    assert!(
        screen(&opened).contains("pics-marker"),
        "the linked document must open rather than wedging the loop:\n{}",
        screen(&opened)
    );

    // Graphics really were on: a kitty APC transmission for the *valid* image
    // reached the wire. Without this the test would pass with images disabled
    // and prove nothing.
    assert!(
        contains(opened.as_bytes(), b"\x1b_G"),
        "no kitty graphics payload on the wire — graphics were not actually \
         enabled, so this test would not have exercised the image path at all"
    );

    // The hostile one degraded to its alt text instead of blocking.
    assert!(
        screen(&opened).contains("trap"),
        "the FIFO image must fall back to alt text:\n{}",
        screen(&opened)
    );

    // Still alive and still reading keys — the part a hang would fail.
    let scrolled = press(&pty, b"j");
    assert!(!screen(&scrolled).is_empty(), "the viewer must still paint");

    quit_and_reap(&pty, &mut child);
    let _ = std::fs::remove_file(&pipe);
    let _ = std::fs::remove_dir_all(&dir);
}
