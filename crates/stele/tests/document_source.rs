//! Where the document comes from, proved through the real binary on a real
//! terminal: `stele -` (DW-2.1) and `--watch` (DW-2.2, DW-2.4).
//!
//! These have to be pty tests. Reading keys from `/dev/tty` while stdin is a
//! pipe is a property of the *process's controlling terminal*, not of any
//! function stele owns — no unit test can be wrong about it, because no unit
//! test can ask. Likewise "the viewer repainted without anyone pressing a
//! key": the oracle is a frame arriving on the wire during a window in which
//! nothing was typed.

#![cfg(unix)]

use std::ffi::OsStr;
use std::io::Write as _;
use std::time::{Duration, Instant};

mod common;

use common::pty::{
    ChildStdin, FRAME_END, Pty, RESTORE, contains, drain_for, drain_to_quiet, read_one_frame,
    read_until, spawn_viewer,
};
use common::render::render_row;

/// The pty the harness opens is 80x24, so the content viewport is 23 rows and
/// the status row is row 24.
const COLS: usize = 80;
const STATUS_ROW: u16 = 24;

/// Long enough for a poll-driven repaint (`WATCH_POLL_INTERVAL` is 250 ms) to
/// be unambiguous, short enough that a *missing* repaint fails the test
/// instead of hanging cargo.
const REPAINT_DEADLINE: Duration = Duration::from_secs(5);

/// Above one poll interval with room for process scheduling, but far below
/// `REPAINT_DEADLINE`: a repaint that took longer than this did not come from
/// the watch tick.
const ONE_POLL_BUDGET: Duration = Duration::from_secs(2);

fn scratch(tag: &str, contents: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("stele-watch-{tag}-{}.md", std::process::id()));
    std::fs::write(&path, contents).expect("write fixture");
    path
}

/// Ends a session and reaps it, draining the wire on the way out.
///
/// The drain is not tidiness. A pty master's buffer is small; a child whose
/// last frames nobody read blocks inside `write` and never reaches its exit,
/// so a bare `wait()` after `q` hangs the test run rather than failing it.
/// Reading until the restore sequence both unblocks the child and asserts the
/// ending was clean.
fn quit_and_reap(pty: &Pty, child: &mut std::process::Child) {
    pty.type_bytes(b"q");
    let after = read_until(pty.master_fd(), RESTORE, Duration::from_secs(10));
    assert!(
        contains(&after, RESTORE),
        "`q` must end the session with the restore sequence; wire was {:?}",
        String::from_utf8_lossy(&after)
    );
    let exit = child.wait().expect("wait");
    assert_eq!(exit.code(), Some(0), "clean quit expected: {exit:?}");
}

/// 40 one-line paragraphs, none of which wraps at 80 columns — so a line of
/// the document is a line of the screen and `render_row` can be compared
/// verbatim.
fn numbered_document(prefix: &str) -> String {
    (0..40).map(|i| format!("{prefix}-{i:03}\n\n")).collect()
}

/// DW-2.1: the document arrives on stdin and the keyboard still works.
///
/// Two claims in one run, and they need each other. "It rendered" alone would
/// pass for a viewer that then sat deaf forever, which is exactly the failure
/// mode of reading keys from a stdin that is a pipe; "it quit" alone would
/// pass for one that never showed the document.
#[test]
fn test_dw_2_1_piped_stdin_renders_and_still_answers_keys_from_the_tty() {
    let pty = Pty::open();
    let canonical = pty.lflag();
    let document = "# piped-heading\n\nbody from the pipe\n";
    let mut child = spawn_viewer(
        &pty,
        &[OsStr::new("-")],
        ChildStdin::Pipe(document.as_bytes()),
    );

    let master = pty.master_fd();
    let startup = read_until(master, b"piped-heading", REPAINT_DEADLINE);
    assert!(
        contains(&startup, b"piped-heading"),
        "the piped document never reached the screen; wire was {:?}",
        String::from_utf8_lossy(&startup)
    );
    assert!(
        contains(&startup, b"body from the pipe"),
        "only the heading arrived, not the body"
    );
    // The status row names the source, so a reader can tell a piped document
    // from a file of the same content.
    assert!(
        contains(&startup, b"(stdin)"),
        "the status row must name the stdin source; wire was {:?}",
        String::from_utf8_lossy(&startup)
    );
    assert_eq!(
        pty.lflag() & (libc::ICANON | libc::ECHO),
        0,
        "raw mode must be on, or `q` below would be line-buffered rather than a key"
    );

    // Nothing but the ending may be on the wire when the restore is asserted.
    drain_to_quiet(master);
    pty.type_bytes(b"q");

    let after = read_until(master, RESTORE, Duration::from_secs(10));
    assert_eq!(
        after, RESTORE,
        "`q` typed at the tty must quit a stdin-sourced viewer and restore the \
         terminal exactly.\n  expected: {RESTORE:?}\n  got:      {after:?}"
    );
    let status = child.wait().expect("wait");
    assert_eq!(status.code(), Some(0), "clean quit expected: {status:?}");
    assert_eq!(pty.lflag(), canonical, "the line discipline must come back");
}

/// DW-2.2: an external write repaints the viewer, unprompted, within a poll
/// interval — and puts the reader back on the block they were reading.
///
/// The anchor claim is asserted on row 1 of the *modelled screen* rather than
/// on a scroll offset: a scroll number that survived a reload could still be
/// pointing at different text, which is the failure the anchor exists to
/// prevent.
#[test]
fn test_dw_2_2_an_external_write_repaints_within_a_poll_interval_and_keeps_the_top_block() {
    let path = scratch("reload", &numbered_document("para"));
    let pty = Pty::open();
    let mut child = spawn_viewer(
        &pty,
        &[OsStr::new("--watch"), path.as_os_str()],
        ChildStdin::Tty,
    );
    let master = pty.master_fd();

    assert!(
        contains(
            &read_until(master, b"para-000", REPAINT_DEADLINE),
            b"para-000"
        ),
        "the document never rendered"
    );

    // Scroll away from the top so "the anchor was preserved" is a claim with
    // content — at scroll 0 it would hold for a viewer that ignored it.
    // An *even* number of lines: consecutive paragraphs are separated by a
    // blank layout line, so an odd scroll offset would put a blank row at the
    // top and "the same block is still on top" would be a claim about
    // whitespace.
    //
    // `j` moves the reading line and the page only follows once the reading
    // line reaches its bottom edge, so scrolling the page by `SCROLL_LINES`
    // costs one press per content row first. That is arithmetic about this
    // 24-row pty, not about the anchor this test is here for.
    const SCROLL_LINES: usize = 12;
    const CONTENT_ROWS: usize = 23; // 24 rows, less the status line.
    let presses = CONTENT_ROWS + SCROLL_LINES - 1;
    drain_to_quiet(master);
    pty.type_bytes(&vec![b'j'; presses - 1]);
    drain_to_quiet(master);
    pty.type_bytes(b"j");
    let before = read_one_frame(master, REPAINT_DEADLINE);
    let anchored_row = render_row(&before, 1, COLS);
    let status_before = render_row(&before, STATUS_ROW, COLS);
    assert!(
        anchored_row.trim().starts_with("para-"),
        "test setup: row 1 must be a numbered paragraph, got {anchored_row:?}"
    );

    // From here nothing is typed. Any frame that arrives came from the watch
    // tick and from nothing else.
    drain_to_quiet(master);
    let mut grown = numbered_document("para");
    grown.push_str("tail-a\n\ntail-b\n\ntail-c\n\n");
    let wrote_at = Instant::now();
    std::fs::write(&path, &grown).expect("external write");

    let after = read_one_frame(master, REPAINT_DEADLINE);
    let elapsed = wrote_at.elapsed();
    assert!(
        after.contains("\u{1b}[?2026l"),
        "no repaint followed the external write within {REPAINT_DEADLINE:?}"
    );
    assert!(
        elapsed < ONE_POLL_BUDGET,
        "the repaint took {elapsed:?}, too slow to have come from the poll tick"
    );

    assert_eq!(
        render_row(&after, 1, COLS),
        anchored_row,
        "the reader must still be on the same block after the reload"
    );
    // Proof the frame is a genuine *reload* and not an incidental repaint of
    // the same tree: the document grew, so the same scroll offset is now a
    // smaller fraction of it, and the ruler says so.
    assert_ne!(
        render_row(&after, STATUS_ROW, COLS),
        status_before,
        "the status ruler must reflect the longer document"
    );

    quit_and_reap(&pty, &mut child);
    std::fs::remove_file(&path).ok();
}

/// DW-2.4: a file that disappears under `--watch` is a status-row message,
/// not an exit.
///
/// Three things must all hold, and each one alone would let a real failure
/// through: the error is reported, the last good render is still on screen,
/// and the process is still running.
#[test]
fn test_dw_2_4_a_deleted_file_reports_on_the_status_row_and_keeps_the_last_frame() {
    let path = scratch("deleted", "# still-here\n\nbody that must survive\n");
    let pty = Pty::open();
    let mut child = spawn_viewer(
        &pty,
        &[OsStr::new("--watch"), path.as_os_str()],
        ChildStdin::Tty,
    );
    let master = pty.master_fd();

    assert!(
        contains(
            &read_until(master, b"still-here", REPAINT_DEADLINE),
            b"still-here"
        ),
        "the document never rendered"
    );

    drain_to_quiet(master);
    std::fs::remove_file(&path).expect("delete the watched file");

    let after = read_one_frame(master, REPAINT_DEADLINE);
    let status = render_row(&after, STATUS_ROW, COLS);
    assert!(
        status.contains("reload failed"),
        "the status row must report the failure, got {status:?}"
    );
    assert!(
        status.contains("could not read file"),
        "and must say what actually went wrong, got {status:?}"
    );
    // `▌` is the heading depth marker: one per level ahead of the text.
    assert_eq!(
        render_row(&after, 1, COLS)
            .trim()
            .trim_start_matches(['\u{258c}', ' ']),
        "still-here",
        "the last good render must still be on screen"
    );

    assert!(
        child.try_wait().expect("try_wait").is_none(),
        "a deleted file must not end the session"
    );

    // And the viewer is still a viewer: it answers keys after the failure.
    quit_and_reap(&pty, &mut child);
}

/// The file coming *back* must be picked up too — a watcher that gives up
/// after one failure is only half a watcher.
#[test]
fn test_a_watched_file_that_reappears_is_reloaded() {
    let path = scratch("restored", "# first-body\n");
    let pty = Pty::open();
    let mut child = spawn_viewer(
        &pty,
        &[OsStr::new("--watch"), path.as_os_str()],
        ChildStdin::Tty,
    );
    let master = pty.master_fd();

    // A whole startup frame, so the status row is in it: the ruler as it looks
    // when nothing has gone wrong, which is what the row must return to.
    let startup = read_one_frame(master, REPAINT_DEADLINE);
    assert!(
        startup.contains("first-body"),
        "the document never rendered; frame was {startup:?}"
    );
    let ruler = render_row(&startup, STATUS_ROW, COLS);
    assert!(
        !ruler.trim().is_empty(),
        "test setup: the ruler must be on row {STATUS_ROW}"
    );

    drain_to_quiet(master);
    std::fs::remove_file(&path).expect("delete");
    assert!(
        read_one_frame(master, REPAINT_DEADLINE).contains("reload failed"),
        "the deletion must be reported before the file comes back"
    );

    drain_to_quiet(master);
    std::fs::File::create(&path)
        .and_then(|mut f| f.write_all(b"# second-body\n"))
        .expect("restore the file");

    // Whole frames, until the reloaded content shows up. `read_until` on the
    // body text alone would stop before row 24 was painted, and the status row
    // is half of what this test now checks.
    let mut frame = String::new();
    let deadline = Instant::now() + REPAINT_DEADLINE;
    while Instant::now() < deadline {
        frame = read_one_frame(master, REPAINT_DEADLINE);
        if frame.contains("second-body") {
            break;
        }
    }
    assert!(
        frame.contains("second-body"),
        "the restored file must be reloaded; last frame was {frame:?}"
    );

    // The content coming back is only half of "recovered". Asserting on the
    // wire alone missed that the failure message stayed on the status row
    // underneath the correctly re-rendered document, for ~100 frames — the
    // reader was told the reload had failed while looking at its result.
    let status = render_row(&frame, STATUS_ROW, COLS);
    assert!(
        !status.contains("reload failed"),
        "a reload that succeeded must clear the earlier failure from the status \
         row, got {status:?}"
    );
    // Compared against the startup ruler rather than parsed: the row is
    // padded and clipped to the terminal width, so a document whose path is
    // long enough to fill it has no visible `%` to look for. Equality says
    // "back to the ordinary ruler" without depending on which fields survived
    // the clip.
    assert_eq!(
        status, ruler,
        "the permanent ruler must be back on the status row after a recovery"
    );

    quit_and_reap(&pty, &mut child);
    std::fs::remove_file(&path).ok();
}

/// A watched file truncated to nothing — what an editor that writes in two
/// steps leaves behind for a moment — must repaint an empty document, not
/// crash and not leave the previous text on screen forever.
#[test]
fn test_a_watched_file_truncated_to_empty_repaints_instead_of_crashing() {
    let path = scratch("truncated", &numbered_document("gone"));
    let pty = Pty::open();
    let mut child = spawn_viewer(
        &pty,
        &[OsStr::new("--watch"), path.as_os_str()],
        ChildStdin::Tty,
    );
    let master = pty.master_fd();
    assert!(contains(
        &read_until(master, b"gone-000", REPAINT_DEADLINE),
        b"gone-000"
    ));

    drain_to_quiet(master);
    std::fs::write(&path, "").expect("truncate");

    let after = read_one_frame(master, REPAINT_DEADLINE);
    assert!(
        render_row(&after, 1, COLS).trim().is_empty(),
        "an empty document must paint an empty first row, got {:?}",
        render_row(&after, 1, COLS)
    );
    assert!(
        child.try_wait().expect("try_wait").is_none(),
        "truncation must not end the session"
    );

    quit_and_reap(&pty, &mut child);
    std::fs::remove_file(&path).ok();
}

/// DW-2.2's bound has to hold **while the reader is using the viewer**, which
/// is the case every other watch test here misses: they all poll an idle
/// viewer, and an idle viewer is precisely the condition under which the bug
/// this test exists for is invisible.
///
/// The reload check used to hang off `event::poll`'s timeout branch, so any
/// pending event skipped it. An autorepeating key produces a pending event on
/// every iteration, so a held `j` starved the reload indefinitely — measured at
/// ~14 keys/s (macOS default autorepeat), the external write went unseen for
/// 8 s and counting.
///
/// Keys alternate `j`/`k` so the reader's net scroll stays at the top and the
/// rewritten first block is on screen; the marker is what proves the *new*
/// document was rendered rather than the old one repainted.
#[test]
fn test_dw_2_2_a_reload_lands_while_keys_are_arriving_continuously() {
    const MARKER: &str = "RELOAD-WHILE-TYPING";

    let path = scratch("starvation", &numbered_document("busy"));
    let pty = Pty::open();
    let mut child = spawn_viewer(
        &pty,
        &[OsStr::new("--watch"), path.as_os_str()],
        ChildStdin::Tty,
    );
    let master = pty.master_fd();
    assert!(
        contains(
            &read_until(master, b"busy-000", REPAINT_DEADLINE),
            b"busy-000"
        ),
        "the document never rendered"
    );
    drain_to_quiet(master);

    // Feed keys for a beat *before* the write, so the loop is already saturated
    // when the file changes rather than catching it during a lull.
    let mut down = true;
    for _ in 0..25 {
        pty.type_bytes(if down { b"j" } else { b"k" });
        down = !down;
        drain_for(master, Duration::from_millis(20));
    }

    let changed = numbered_document("busy").replacen("busy-000", MARKER, 1);
    std::fs::write(&path, &changed).expect("external write");
    let wrote_at = Instant::now();

    // Keep typing at roughly autorepeat speed and watch for the marker. The
    // input never stops, so nothing here can be explained by an idle moment.
    let mut wire = Vec::new();
    while wrote_at.elapsed() < REPAINT_DEADLINE {
        pty.type_bytes(if down { b"j" } else { b"k" });
        down = !down;
        wire.extend(drain_for(master, Duration::from_millis(20)));
        if contains(&wire, MARKER.as_bytes()) {
            break;
        }
    }
    let seen_after = wrote_at.elapsed();

    assert!(
        contains(&wire, MARKER.as_bytes()),
        "the reload never landed while keys were arriving — {seen_after:?} elapsed \
         with continuous input. This is the starvation case: a held key must not \
         be able to defer the watch tick."
    );
    assert!(
        seen_after < ONE_POLL_BUDGET,
        "the reload took {seen_after:?} under continuous input, past the \
         {ONE_POLL_BUDGET:?} budget"
    );

    quit_and_reap(&pty, &mut child);
    std::fs::remove_file(&path).ok();
}

/// The same latency bound as the test above, under a **resize** stream rather
/// than a key stream — the other way an event source can stay permanently
/// non-empty.
///
/// This one gets its own test because it failed for a different reason and in
/// a different place. Keys are drained by the outer loop, whose wait
/// `until_next_tick` already caps. Resizes are drained by the *inner* debounce
/// loop, which re-armed a fixed 50 ms quiet period on every arriving event and
/// sat outside that cap entirely — so a resize arriving faster than the
/// debounce held the inner loop open and the tick was never reached. Measured
/// before the fix: no reload at all at a 10 ms or a 40 ms resize interval, and
/// zero bytes painted for six seconds; at 70 ms, past the debounce, everything
/// worked. The threshold was exactly `RESIZE_DEBOUNCE`, which is why this test
/// resizes every ~20 ms.
///
/// The resizes are real: `TIOCSWINSZ` on the pty master makes the kernel
/// deliver `SIGWINCH` to the viewer, so this drives signal → crossterm →
/// `Event::Resize` rather than a fake event.
#[test]
fn test_dw_2_2_a_reload_lands_during_a_sustained_resize_storm() {
    const MARKER: &str = "RELOAD-DURING-RESIZE";
    // Both wide enough that the marker never wraps, so "the marker is on the
    // wire" cannot be defeated by a narrow relayout.
    const WIDE: u16 = 80;
    const NARROW: u16 = 70;

    let path = scratch("resize-storm", &numbered_document("storm"));
    let pty = Pty::open();
    let mut child = spawn_viewer(
        &pty,
        &[OsStr::new("--watch"), path.as_os_str()],
        ChildStdin::Tty,
    );
    let master = pty.master_fd();
    assert!(
        contains(
            &read_until(master, b"storm-000", REPAINT_DEADLINE),
            b"storm-000"
        ),
        "the document never rendered"
    );
    drain_to_quiet(master);

    // Saturate the resize path *before* the write, so the file changes with
    // the debounce loop already held open rather than during a lull.
    let mut wide = false;
    for _ in 0..10 {
        pty.resize(24, if wide { WIDE } else { NARROW });
        wide = !wide;
        drain_for(master, Duration::from_millis(20));
    }

    let changed = numbered_document("storm").replacen("storm-000", MARKER, 1);
    std::fs::write(&path, &changed).expect("external write");
    let wrote_at = Instant::now();

    // Keep resizing faster than RESIZE_DEBOUNCE for the whole wait, so nothing
    // observed here can be explained by the storm having stopped.
    let mut wire = Vec::new();
    while wrote_at.elapsed() < REPAINT_DEADLINE {
        pty.resize(24, if wide { WIDE } else { NARROW });
        wide = !wide;
        wire.extend(drain_for(master, Duration::from_millis(20)));
        if contains(&wire, MARKER.as_bytes()) {
            break;
        }
    }
    let seen_after = wrote_at.elapsed();

    assert!(
        contains(&wire, MARKER.as_bytes()),
        "the reload never landed during a sustained resize storm — {seen_after:?} \
         elapsed. A resize stream faster than the debounce must not be able to \
         defer the watch tick."
    );
    assert!(
        seen_after < ONE_POLL_BUDGET,
        "the reload took {seen_after:?} under a sustained resize storm, past the \
         {ONE_POLL_BUDGET:?} budget"
    );

    quit_and_reap(&pty, &mut child);
    std::fs::remove_file(&path).ok();
}

/// A sustained resize storm must keep *painting*, independently of `--watch`.
///
/// This is the same defect seen from the user's side rather than the reload's:
/// with an unbounded debounce, a viewer whose window is being dragged emits
/// nothing at all until the drag stops, because the quiet period the loop
/// waits for never arrives. Measured before the fix: zero bytes over six
/// seconds at a 10 ms resize interval. No `--watch` here — the freeze was
/// never about watching, and this asserts the burst ceiling on its own terms.
#[test]
fn test_a_sustained_resize_storm_keeps_repainting_rather_than_freezing() {
    let path = scratch("resize-freeze", &numbered_document("drag"));
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[path.as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    assert!(
        contains(
            &read_until(master, b"drag-000", REPAINT_DEADLINE),
            b"drag-000"
        ),
        "the document never rendered"
    );
    drain_to_quiet(master);

    // Two seconds of dragging at ~20 ms, well under the 50 ms debounce.
    let started = Instant::now();
    let mut painted = Vec::new();
    let mut wide = false;
    let mut resizes = 0usize;
    while started.elapsed() < Duration::from_secs(2) {
        pty.resize(24, if wide { 80 } else { 70 });
        wide = !wide;
        resizes += 1;
        painted.extend(drain_for(master, Duration::from_millis(20)));
    }

    assert!(
        !painted.is_empty(),
        "the viewer painted nothing at all across {:?} of continuous resizing — \
         a window drag must not freeze the screen until the drag stops",
        started.elapsed()
    );
    // Frames end with the synchronized-update terminator, so this counts whole
    // frames rather than incidental bytes.
    let frames = painted
        .windows(FRAME_END.len())
        .filter(|w| *w == FRAME_END)
        .count();
    assert!(
        frames >= 2,
        "expected repeated repaints during the drag, saw {frames} frame(s) \
         across {resizes} resizes"
    );
    // The other half of the same claim, and the reason the burst ceiling is a
    // ceiling rather than its removal: coalescing must survive. One relayout
    // per resize event is the behaviour the debounce exists to prevent, so the
    // frame count has to stay far below the event count. Measured at 86
    // resizes to 9 frames (~200 ms of coalescing per frame, as designed).
    assert!(
        frames * 3 <= resizes,
        "resize coalescing regressed: {frames} frame(s) for {resizes} resizes — \
         the debounce must still fold a burst into one relayout"
    );

    quit_and_reap(&pty, &mut child);
    std::fs::remove_file(&path).ok();
}

/// Every truecolor foreground on the wire, as a set — the palette the viewer
/// is painting with. A theme swap changes the palette, so comparing two of
/// these answers "did `T` take effect" from the painted bytes rather than from
/// internal state a test cannot see.
fn truecolor_palette(frame: &str) -> std::collections::BTreeSet<String> {
    let mut found = std::collections::BTreeSet::new();
    let mut rest = frame;
    while let Some(at) = rest.find("38;2;") {
        rest = &rest[at + "38;2;".len()..];
        if let Some(end) = rest.find('m') {
            found.insert(rest[..end].to_string());
        }
    }
    found
}

/// A chrome key pressed *during* a resize burst must still act.
///
/// The debounce drain reads keys off the queue itself, and it used to offer
/// them only to `AppState::handle_key_event` — which does not know `T`. So a
/// theme toggle pressed while dragging a window edge was read, ignored, and
/// dropped: the key was gone and nothing happened. `q` and the scroll keys
/// were fine, which is why this hid.
///
/// (`+`/`-` are a different story and are deliberately *not* asserted here:
/// `apply_resize_burst` resyncs the content width to the terminal's at the end
/// of the burst, so a width toggle mid-resize is overridden by the resize —
/// the documented "a real terminal resize always wins over a stale toggle"
/// rule, not a swallowed key.)
#[test]
fn test_a_theme_toggle_pressed_during_a_resize_burst_is_not_swallowed() {
    let path = scratch("theme-in-storm", "# Heading One\n\nbody paragraph\n");
    let pty = Pty::open();
    let mut child = spawn_viewer(&pty, &[path.as_os_str()], ChildStdin::Tty);
    let master = pty.master_fd();
    assert!(
        contains(
            &read_until(master, b"Heading One", REPAINT_DEADLINE),
            b"Heading One"
        ),
        "the document never rendered"
    );

    // Both palettes are read at the same width, so the only thing that can
    // differ between them is the theme.
    drain_to_quiet(master);
    pty.resize(24, 79);
    let before = truecolor_palette(&read_one_frame(master, REPAINT_DEADLINE));
    assert!(
        !before.is_empty(),
        "the viewer must be painting truecolor for this test to mean anything"
    );

    // A dense storm with `T` pressed inside it: resizes keep arriving every
    // ~15 ms, far under RESIZE_DEBOUNCE, so the burst drain is the code path
    // holding the queue when the key lands.
    drain_to_quiet(master);
    let mut wide = false;
    for i in 0..24 {
        pty.resize(24, if wide { 80 } else { 70 });
        wide = !wide;
        if i == 8 {
            pty.type_bytes(b"T");
        }
        drain_for(master, Duration::from_millis(15));
    }

    drain_to_quiet(master);
    pty.resize(24, 79);
    let after = truecolor_palette(&read_one_frame(master, REPAINT_DEADLINE));

    assert_ne!(
        before, after,
        "`T` pressed during a resize burst must still swap the theme — the \
         drain must act on chrome keys, not just consume them"
    );

    quit_and_reap(&pty, &mut child);
    std::fs::remove_file(&path).ok();
}

/// `--watch` changes the event loop from a blocking `read` to a polled one.
/// The restore contract DW-5.1 pinned must still hold on that path: the
/// timeout must not swallow the keystroke, and the ending must still put
/// exactly the restore sequence on the wire and nothing after it.
#[test]
fn test_the_polled_watch_loop_still_restores_the_terminal_exactly_on_quit() {
    let path = scratch("watch-restore", "# hi\n\nbody text\n");
    let pty = Pty::open();
    let canonical = pty.lflag();
    let mut child = spawn_viewer(
        &pty,
        &[OsStr::new("--watch"), path.as_os_str()],
        ChildStdin::Tty,
    );
    let master = pty.master_fd();
    assert!(contains(
        &read_until(master, b"body text", REPAINT_DEADLINE),
        b"body text"
    ));

    // Wait out more than one poll interval before quitting, so the keystroke
    // lands on the polled path rather than racing the startup frame.
    std::thread::sleep(Duration::from_millis(400));
    drain_to_quiet(master);
    pty.type_bytes(b"q");

    let after = read_until(master, RESTORE, Duration::from_secs(10));
    assert_eq!(
        after, RESTORE,
        "quitting a polled session must emit exactly the restore sequence.\n  \
         expected: {RESTORE:?}\n  got:      {after:?}"
    );
    let exit = child.wait().expect("wait");
    assert_eq!(exit.code(), Some(0), "clean quit expected: {exit:?}");
    assert_eq!(pty.lflag(), canonical);
    std::fs::remove_file(&path).ok();
}
