//! HUNTER-PAINT bug-bash repros and adversarial probes over the runtime
//! shell: scroll arithmetic, resize, the mode-2026 framing, the build stamp,
//! and the sanitization barricade.
//!
//! Tests named `repro_*` pin a specific defect. The `#[ignore]`d ones are
//! OPEN — they fail today and say so; un-ignore each as its fix lands rather
//! than relaxing the assertion. Tests named `probe_*` are adversarial sweeps
//! that currently pass and exist to keep passing.

use ast::Document;
use crossterm::event::KeyCode;
use layout::{LayoutConfig, NullSizer, layout};
use std::io::{self, Write};
use stele::app::{AppState, LayoutContext};
use stele::painter::{Painter, Size};
use width::{WidthConfig, WidthEngine};

fn engine() -> WidthEngine {
    WidthEngine::new(WidthConfig::default())
}

// ---------------------------------------------------------------- repro 1
// Height-only resize (no width change, no reflow at all) throws the reader
// back to the top of whatever block they were inside.
#[test]
fn repro_height_only_resize_jumps_scroll_to_block_start() {
    // One long code block: 400 lines, one source block.
    let mut src = String::from("```\n");
    for i in 0..400 {
        src.push_str(&format!("code line {i}\n"));
    }
    src.push_str("```\n");

    let doc = Document::parse(&src);
    let config = LayoutConfig::default();
    let eng = engine();
    let tree = layout(&doc, 80, &config, &eng, &NullSizer);
    let mut state = AppState::new(
        tree,
        Size {
            width: 80,
            height: 20,
        },
    );
    let ctx = LayoutContext {
        doc: &doc,
        config: &config,
        engine: &eng,
        sizer: &NullSizer,
    };

    for _ in 0..200 {
        state.handle_key(KeyCode::Down);
    }
    let before = state.scroll();
    assert_eq!(before, 200);

    // Same width, height 20 -> 21. Nothing reflows.
    state.apply_resize_burst(
        &ctx,
        &[Size {
            width: 80,
            height: 21,
        }],
    );
    let after = state.scroll();
    println!("scroll before height-only resize = {before}, after = {after}");
    assert_eq!(
        after, before,
        "a resize that changes nothing about the layout must not move the reader"
    );
}

// ---------------------------------------------------------------- repro 2
// A write error mid-frame leaves the synchronized-update block unclosed.
struct FailAfter {
    budget: usize,
    seen: Vec<u8>,
}
impl Write for FailAfter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.budget == 0 {
            return Err(io::Error::other("device full"));
        }
        let n = buf.len().min(self.budget);
        self.seen.extend_from_slice(&buf[..n]);
        self.budget -= n;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
#[ignore = "OPEN, but NOT satisfiable from painter.rs (WAVE2-LAYOUT). Painter::frame no \
           longer `?`s past SYNC_END — it always attempts the close, which is verified by \
           painter.rs's own test_frame_closes_the_sync_update_block_after_a_recoverable_\
           write_error. This repro's FailAfter is permanently dead once its budget hits 0, \
           so it rejects the close too and `seen` can never contain SYNC_END no matter what \
           the painter does. The permanently-dead-writer case is covered instead by \
           terminal.rs's RESTORE_SEQUENCE, which now leads with \\x1b[?2026l. Un-ignore only \
           with a writer that models a recoverable error."]
fn repro_write_error_midframe_leaves_sync_update_open() {
    let doc = Document::parse("hello world\n\nsecond paragraph\n");
    let tree = layout(&doc, 40, &LayoutConfig::default(), &engine(), &NullSizer);
    let mut painter = Painter::new(engine());
    let mut out = FailAfter {
        budget: 20,
        seen: Vec::new(),
    };
    let res = painter.frame(
        &tree,
        0,
        Size {
            width: 40,
            height: 10,
        },
        &mut out,
    );
    assert!(res.is_err(), "the write must actually have failed");
    let bytes = out.seen.clone();
    println!("emitted on the wire: {:?}", String::from_utf8_lossy(&bytes));
    let has_begin = bytes.windows(8).any(|w| w == b"\x1b[?2026h".as_slice());
    let has_end = bytes.windows(8).any(|w| w == b"\x1b[?2026l".as_slice());
    println!("SYNC_BEGIN on wire: {has_begin}, SYNC_END on wire: {has_end}");
    assert!(
        !has_begin || has_end,
        "frame left the terminal in synchronized-update mode"
    );
}

// ---------------------------------------------------------------- repro 3
// The build stamp overwrites document text on the last viewport row.
//
// Asserted on the *rendered row*, not on the byte stream: the defect is that
// the stamp lands on cells the document already owns, and only replaying the
// frame the way a terminal would can see that. The original form of this test
// asserted that row 3's document text stopped short of the stamp column, which
// states the wrong invariant — the fix keeps the text and drops the stamp.
#[test]
fn repro_build_stamp_overwrites_last_row_content() {
    let long: String = (0..60).map(|i| format!("word{i:03} ")).collect();
    let doc = Document::parse(&format!("{long}\n"));
    let config = LayoutConfig {
        min_width: 24,
        max_width: 100,
    };
    let tree = layout(&doc, 80, &config, &engine(), &NullSizer);
    let mut painter = Painter::new(engine());
    let mut buf = Vec::new();
    // Viewport is 80x3; the wrapped paragraph fills all three rows.
    painter
        .frame(
            &tree,
            0,
            Size {
                width: 80,
                height: 3,
            },
            &mut buf,
        )
        .unwrap();
    let text = String::from_utf8_lossy(&buf);
    println!("---- frame bytes ----\n{text:?}\n---------------------");
    let sha = env!("STELE_BUILD_SHA");
    let stamp_col = 80 - sha.chars().count() + 1;
    println!("stamp sha={sha:?} would land at 1-indexed column {stamp_col} of row 3");

    // What the document laid out for row 3, independent of what was painted.
    let expected: String = match tree.lines(2..3).next().unwrap() {
        layout::Line::Items(items) => items
            .iter()
            .map(|i| match i {
                layout::LineItem::Run(r) => r.text.clone(),
                layout::LineItem::Box(_) => String::new(),
            })
            .collect(),
        layout::Line::Reserved(_) => String::new(),
    };
    let rendered = render_row(&text, 3, 80);
    println!("row 3 laid out : {expected:?}");
    println!("row 3 rendered : {rendered:?}");
    assert!(
        rendered.starts_with(&expected),
        "the last row's document text was clobbered\n  laid out: {expected:?}\n  rendered: {rendered:?}"
    );
    assert!(
        !rendered.contains(sha),
        "the build stamp is sitting on row 3's document text at column {stamp_col}: {rendered:?}"
    );
}

/// The complement, so the guard is not simply "never paint the stamp": a last
/// row the document leaves room on still gets it, at the exact right column.
#[test]
fn probe_build_stamp_still_paints_when_the_last_row_leaves_room() {
    let doc = Document::parse("short\n");
    let tree = layout(
        &doc,
        80,
        &LayoutConfig {
            min_width: 24,
            max_width: 100,
        },
        &engine(),
        &NullSizer,
    );
    let mut painter = Painter::new(engine());
    let mut buf = Vec::new();
    painter
        .frame(
            &tree,
            0,
            Size {
                width: 80,
                height: 3,
            },
            &mut buf,
        )
        .unwrap();
    let text = String::from_utf8_lossy(&buf);
    let sha = env!("STELE_BUILD_SHA");
    let rendered = render_row(&text, 3, 80);
    println!("row 3 rendered : {rendered:?}");
    let at = rendered
        .find(sha)
        .unwrap_or_else(|| panic!("the stamp must still paint on an empty last row: {rendered:?}"));
    assert_eq!(
        at + sha.chars().count(),
        80,
        "the stamp is flush with the right edge: {rendered:?}"
    );
}

// ---------------------------------------------------------------- repro 4
// An inline media box that falls back to TEXT paints that text one whole
// box-width to the right of the box: `write_text_row` writes at the current
// cursor, which `paint_inline_box` has already advanced past the box.
// FIXED by WAVE2-SINK: `media/sink.rs::write_text_row` now emits its own CUP
// to `rect.x`/`rect.y` instead of writing at the caller's cursor, so the sink
// honours the `CellRect` it is handed. Un-ignored; the one assertion that
// pinned the *buggy* byte shape is updated to pin the fixed one, and the
// rendered-row assertion below is unchanged and now passes.
#[test]
fn repro_inline_media_text_fallback_paints_at_the_wrong_column() {
    use stele::ImageSizer;
    use stele::media::GfxMediaSink;

    let src = "ab $x+y$ cd\n";
    let doc = Document::parse(src);
    let sizer = ImageSizer::new(".");
    let tree = layout(&doc, 40, &LayoutConfig::default(), &engine(), &sizer);

    // Confirm the math really is an inline box on the text line.
    let line = tree.lines(0..1).next().unwrap();
    println!("line 0 = {line:?}");

    let mut sink = GfxMediaSink::new(doc.clone(), ".");
    sink.force_ratex_failure_for_test(true);
    sink.force_txm_failure_for_test(true);
    let mut painter = Painter::new(engine());
    painter.register_media(Box::new(sink));
    let mut buf = Vec::new();
    painter
        .frame(
            &tree,
            0,
            Size {
                width: 40,
                height: 1,
            },
            &mut buf,
        )
        .unwrap();
    let text = String::from_utf8_lossy(&buf);
    println!("frame bytes: {text:?}");

    // The literal TeX must land inside the box's own cells, i.e. immediately
    // after "ab " (0-indexed column 3 -> the 4 blanking spaces ARE the box).
    // What used to happen: the blanking spaces are written first, so the
    // cursor was at column 7 when the sink wrote, the text landed at columns
    // 8-10, and the painter then homed back to column 8 and painted " cd"
    // straight over it — the byte shape was "ab \x1b[0m    x+y", with no CUP
    // between the blanking run and the text, and the fallback was invisible.
    // The sink now re-homes to the box origin itself.
    assert!(
        text.contains("ab \u{1b}[0m    \u{1b}[1;4Hx+y"),
        "expected a box-origin CUP before the fallback text; got {text:?}"
    );
    let visible = render_row(&text, 1, 40);
    println!("row 1 as the terminal would show it: {visible:?}");
    assert!(
        visible.contains("x+y"),
        "the inline math fallback text is nowhere on the rendered row: {visible:?}"
    );
}

/// Proof the PROPOSED one-line fix works: re-home the cursor to the box
/// origin after the blanking spaces and before handing off to the sink.
/// Splices that CUP into the exact bytes the painter emitted above and
/// replays them through the same terminal model.
#[test]
fn proposed_fix_for_inline_media_fallback_column_renders_correctly() {
    let buggy = "\u{1b}[?2026h\u{1b}[1;1H\u{1b}[0mab \u{1b}[0m    x+y\u{1b}[1;8H\u{1b}[0m cd\u{1b}[0m\u{1b}[K\u{1b}[?2026l";
    assert_eq!(
        render_row(buggy, 1, 20),
        "ab      cd          ",
        "today: the box is four blank cells and the fallback is gone"
    );
    // `paint_inline_box` blanks cols 4-7 (1-indexed) and leaves the cursor at
    // col 8; adding `ESC[1;4H` there puts the sink's text inside the box.
    let fixed = "\u{1b}[?2026h\u{1b}[1;1H\u{1b}[0mab \u{1b}[0m    \u{1b}[1;4Hx+y\u{1b}[1;8H\u{1b}[0m cd\u{1b}[0m\u{1b}[K\u{1b}[?2026l";
    assert_eq!(
        render_row(fixed, 1, 20),
        "ab x+y  cd          ",
        "with the fix: the fallback renders in the box's own cells"
    );
}

/// Replays a frame's bytes onto a one-row cell grid the way a terminal
/// would: honor CUP, honor EL(0), otherwise write glyphs left to right.
fn render_row(frame: &str, row: u16, width: usize) -> String {
    let mut grid = vec![' '; width];
    let mut cur_row: u16 = 1;
    let mut col: usize = 0;
    let bytes: Vec<char> = frame.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '\u{1b}' {
            let mut j = i + 2; // skip ESC [
            while j < bytes.len() && !bytes[j].is_ascii_alphabetic() {
                j += 1;
            }
            let params: String = bytes[i + 2..j].iter().collect();
            match bytes.get(j) {
                Some('H') => {
                    let mut it = params.split(';');
                    cur_row = it.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                    col = it
                        .next()
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(1)
                        .saturating_sub(1);
                }
                Some('K') if cur_row == row => {
                    for c in grid.iter_mut().skip(col) {
                        *c = ' ';
                    }
                }
                _ => {}
            }
            i = j + 1;
            continue;
        }
        if cur_row == row && col < width {
            grid[col] = bytes[i];
        }
        col += 1;
        i += 1;
    }
    grid.into_iter().collect()
}

// ---------------------------------------------------------------- probe 5
// End-to-end sanitization barricade: a hostile document that plants raw ESC,
// CSI, OSC, APC, BEL and C1 bytes in every document context, painted at
// several widths and scroll offsets, scanned byte-by-byte.
fn hostile_document() -> String {
    let esc = "\u{1b}";
    let bel = "\u{7}";
    let csi8 = "\u{9b}"; // C1 CSI
    let osc8 = "\u{9d}"; // C1 OSC
    let apc8 = "\u{9f}"; // C1 APC
    let st8 = "\u{9c}"; // C1 ST
    format!(
        "---\ntitle: {esc}[?1049h evil frontmatter\n---\n\n\
         # Heading {esc}[31mred{esc}[0m {csi8}2J\n\n\
         Paragraph with {esc}]0;retitled{bel} and {apc8}Gf=100;AAAA{st8} and {osc8}8;;file:///etc/passwd{st8}.\n\n\
         *emph {esc}[5m* and `code span {esc}[?2026l` and **strong {csi8}6n**.\n\n\
         [link text {esc}[1m](http://example.com/{esc}]8;;javascript:alert(1){bel})\n\n\
         [bad scheme](javascript:alert{esc}(1))\n\n\
         ![alt text {esc}_Gf=100{esc}\\\\ that is quite long and wraps around the box](./missing-{esc}[2J.png)\n\n\
         > blockquote {esc}[?25l hidden cursor\n\n\
         - list item {esc}[2J\n- second {csi8}0m\n\n\
         | col {esc}[7m | b {csi8}K |\n|---|---|\n| cell {apc8}Gq=1{st8} | {esc}[38;2;0;0;0m |\n\n\
         ```rust\nfn main() {{ /* {esc}[?1049h {csi8}2J */ let x = \"{esc}]0;t{bel}\"; }}\n```\n\n\
         <div onclick=\"{esc}[?2026h\">literal html {csi8}J</div>\n\n\
         Inline html <span>{esc}[31m</span> tail.\n\n\
         Math: $x^{{{esc}[2J}} + \\evil{esc}[?1049h$ and display:\n\n\
         $$\n{esc}[?1049h \\frac{{{csi8}2J}}{{b}}\n$$\n\n\
         ~~~\n{esc}[?1049h plain fence {apc8}Gz{st8}\n~~~\n\n\
         Autolink <http://example.com/{esc}[2J> done.\n"
    )
}

/// Every escape on the wire must belong to the painter's own fixed
/// vocabulary, a CUP, an SGR, or a well-formed OSC 8 hyperlink.
///
/// Scans by *code point*, not by byte: the wire is UTF-8 and a terminal
/// decodes it before dispatching controls, so U+009B (encoded C2 9B) is a
/// live 8-bit CSI even though neither byte is 0x9B on its own.
fn assert_only_painter_escapes(bytes: &[u8]) {
    let text = String::from_utf8(bytes.to_vec()).expect("painter emits valid UTF-8");
    let c: Vec<char> = text.chars().collect();
    let fixed: &[&str] = &["\u{1b}[?2026h", "\u{1b}[?2026l", "\u{1b}[K", "\u{1b}[0m"];
    let at = |i: usize| -> String { c[i..(i + 20).min(c.len())].iter().collect() };
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        let cp = ch as u32;
        if matches!(cp, 0x00..=0x1F | 0x7F | 0x80..=0x9F) && ch != '\u{1b}' {
            panic!("raw control code point {cp:#x} at char {i}: {:?}", at(i));
        }
        if ch != '\u{1b}' {
            i += 1;
            continue;
        }
        let rest: String = c[i..].iter().collect();
        if let Some(seq) = fixed.iter().find(|s| rest.starts_with(**s)) {
            i += seq.chars().count();
            continue;
        }
        if rest.starts_with("\u{1b}]8;;") {
            let body: String = rest.chars().skip(5).collect();
            let end = body
                .find("\u{1b}\\")
                .unwrap_or_else(|| panic!("unterminated OSC 8 at {i}: {:?}", at(i)));
            for u in body[..end].chars() {
                assert!(
                    !matches!(u as u32, 0x00..=0x1F | 0x7F | 0x80..=0x9F),
                    "control code point {:#x} inside an OSC 8 URI at {i}",
                    u as u32
                );
            }
            i += 5 + body[..end].chars().count() + 2;
            continue;
        }
        assert_eq!(c.get(i + 1), Some(&'['), "ESC at {i} not CSI: {:?}", at(i));
        let mut j = i + 2;
        while j < c.len() && (c[j].is_ascii_digit() || c[j] == ';') {
            j += 1;
        }
        assert!(j < c.len(), "unterminated CSI at {i}");
        assert!(
            matches!(c[j], 'H' | 'm'),
            "foreign CSI terminator {:?} at {i}: {:?}",
            c[j],
            at(i)
        );
        i = j + 1;
    }
}

#[test]
fn probe_hostile_document_emits_no_foreign_escapes_at_any_width_or_scroll() {
    use stele::decor::themed::ThemedDecor;

    let src = stele::decor::frontmatter::apply(&hostile_document(), true).into_owned();
    let src = stele::decor::mermaid::preprocess(&src).into_owned();
    let doc = Document::parse(&src);
    for width in [1u16, 2, 10, 40, 80, 200] {
        let config = LayoutConfig {
            min_width: 24,
            max_width: 100,
        };
        let tree = layout(&doc, width, &config, &engine(), &NullSizer);
        for scroll in [0usize, 1, 5, tree.line_count().saturating_sub(1), 10_000] {
            let mut painter = Painter::new(engine());
            painter.register_decor(Box::new(ThemedDecor::detect(None)));
            let mut buf = Vec::new();
            painter
                .frame(&tree, scroll, Size { width, height: 24 }, &mut buf)
                .unwrap();
            assert_only_painter_escapes(&buf);
        }
    }
}

// ---------------------------------------------------------------- probe 6
// Same hostile document, but through the REAL media path: an enabled
// ImageSizer reserves boxes, and the GfxMediaSink writes alt text / txm grid
// rows / literal TeX source directly to the wire, bypassing `Painter`'s own
// sanitize. Both fallback rungs are forced so no kitty APC is emitted and
// every byte on the wire is text the sink produced itself.
#[test]
fn probe_hostile_document_through_the_media_sink_emits_no_foreign_escapes() {
    use stele::ImageSizer;
    use stele::decor::themed::ThemedDecor;
    use stele::media::GfxMediaSink;

    let src = stele::decor::mermaid::preprocess(&hostile_document()).into_owned();
    let doc = Document::parse(&src);
    let sizer = ImageSizer::new(".");
    for width in [10u16, 40, 80] {
        let config = LayoutConfig {
            min_width: 24,
            max_width: 100,
        };
        let tree = layout(&doc, width, &config, &engine(), &sizer);
        for scroll in [0usize, 3, 9, tree.line_count().saturating_sub(2)] {
            let mut sink = GfxMediaSink::new(doc.clone(), ".");
            sink.force_ratex_failure_for_test(true);
            sink.force_txm_failure_for_test(true);
            let mut painter = Painter::new(engine());
            painter.register_media(Box::new(sink));
            painter.register_decor(Box::new(ThemedDecor::detect(None)));
            let mut buf = Vec::new();
            painter
                .frame(&tree, scroll, Size { width, height: 24 }, &mut buf)
                .unwrap();
            assert_only_painter_escapes(&buf);
        }
    }
}

// ---------------------------------------------------------------- probe 7
// Degenerate viewports and adversarial scroll offsets: never panic, always a
// balanced 2026 pair, never more rows addressed than the viewport has.
#[test]
fn probe_degenerate_viewports_and_scrolls() {
    let src: String = (0..40)
        .map(|i| format!("para {i} with some words\n\n"))
        .collect();
    let doc = Document::parse(&src);
    let tree = layout(&doc, 80, &LayoutConfig::default(), &engine(), &NullSizer);
    let n = tree.line_count();
    for &(w, h) in &[
        (0u16, 0u16),
        (0, 24),
        (24, 0),
        (1, 1),
        (1, 40),
        (40, 1),
        (u16::MAX, 1),
        (2, 2),
    ] {
        for &scroll in &[0usize, 1, n - 1, n, n + 1, usize::MAX] {
            let mut painter = Painter::new(engine());
            let mut buf = Vec::new();
            painter
                .frame(
                    &tree,
                    scroll,
                    Size {
                        width: w,
                        height: h,
                    },
                    &mut buf,
                )
                .unwrap();
            let s = String::from_utf8_lossy(&buf);
            assert_eq!(
                s.matches("\x1b[?2026h").count(),
                s.matches("\x1b[?2026l").count(),
                "unbalanced sync pair at {w}x{h} scroll {scroll}"
            );
            let rows = s.matches(";1H").count();
            assert!(
                rows <= h as usize + 1,
                "{rows} row homes for a {h}-row viewport at {w}x{h}"
            );
        }
    }
}

// ---------------------------------------------------------------- probe 8
// Scroll navigation at every boundary, including a document exactly the
// height of the viewport and one shorter than it.
#[test]
fn probe_scroll_boundaries() {
    for lines in [0usize, 1, 9, 10, 11, 100] {
        let src: String = (0..lines).map(|i| format!("l{i}\n\n")).collect();
        let doc = Document::parse(&src);
        let config = LayoutConfig::default();
        let eng = engine();
        let tree = layout(&doc, 40, &config, &eng, &NullSizer);
        let n = tree.line_count();
        let mut state = AppState::new(
            tree,
            Size {
                width: 40,
                height: 10,
            },
        );
        let expected_max = n.saturating_sub(10);
        assert_eq!(state.max_scroll(), expected_max, "{lines} lines");

        state.handle_key(KeyCode::Char('G'));
        assert_eq!(state.scroll(), expected_max);
        // One more Down at the tail must not move (and must not underflow).
        state.handle_key(KeyCode::Down);
        assert_eq!(state.scroll(), expected_max);
        state.handle_key(KeyCode::PageDown);
        assert_eq!(state.scroll(), expected_max);

        state.handle_key(KeyCode::Char('g'));
        assert_eq!(state.scroll(), 0);
        state.handle_key(KeyCode::Up);
        assert_eq!(state.scroll(), 0);
        state.handle_key(KeyCode::PageUp);
        assert_eq!(state.scroll(), 0);

        // The last scrollable position must show the final document line as
        // the LAST viewport row — never past it, never one short of it.
        state.handle_key(KeyCode::End);
        let last_visible = state.scroll() + 10;
        assert!(
            last_visible >= n,
            "{lines} lines: End leaves line {} unreachable",
            n - 1
        );
    }
}
