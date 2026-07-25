//! WAVE2-SINK: end-to-end position assertions for the media sink's text
//! fallback rungs and its live-placement cap.
//!
//! Everything here goes through the real `Painter::frame` -> `GfxMediaSink`
//! path and asserts on the *bytes on the wire* plus what a terminal would
//! actually show for them. A count could not have caught any of these: the
//! sink's own unit tests already asserted that `write_text_row` produced the
//! right characters, and the painter's already asserted that the sink was
//! called with the right rect — nobody composed the two and asked what the
//! row looks like.

use ast::Document;
use layout::{LayoutConfig, layout};
use stele::ImageSizer;
use stele::media::GfxMediaSink;
use stele::painter::{Painter, Size};
use width::{WidthConfig, WidthEngine};

fn engine() -> WidthEngine {
    WidthEngine::new(WidthConfig::default())
}

/// Paints `src` at `width` x `height` with both math rungs above the literal
/// one forced to fail, so the *text* ladder is what reaches the wire.
fn frame_with_failed_math(src: &str, width: u16, height: u16, txm_fails: bool) -> String {
    let doc = Document::parse(src);
    let sizer = ImageSizer::new(".");
    let tree = layout(&doc, width, &LayoutConfig::default(), &engine(), &sizer);
    let mut sink = GfxMediaSink::new(doc, ".");
    sink.force_ratex_failure_for_test(true);
    sink.force_txm_failure_for_test(txm_fails);
    let mut painter = Painter::new(engine());
    painter.register_media(Box::new(sink));
    let mut buf = Vec::new();
    painter
        .frame(&tree, 0, Size { width, height }, &mut buf)
        .unwrap();
    String::from_utf8_lossy(&buf).into_owned()
}

/// Replays a frame's bytes onto a cell grid the way a terminal would: honour
/// CUP, honour EL(0), otherwise write glyphs left to right. Deliberately the
/// same model HUNTER-PAINT used to prove the fix before it was written.
fn render_row(frame: &str, row: u16, width: usize) -> String {
    let mut grid = vec![' '; width];
    let mut cur_row: u16 = 1;
    let mut col: usize = 0;
    let chars: Vec<char> = frame.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\u{1b}' {
            // Skip an APC (kitty graphics) payload wholesale: it is not text.
            if chars.get(i + 1) == Some(&'_') {
                while i < chars.len() && chars[i] != '\u{9c}' {
                    if chars[i] == '\u{1b}' && chars.get(i + 1) == Some(&'\\') && i > 0 {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            let mut j = i + 2; // skip ESC [
            while j < chars.len() && !chars[j].is_ascii_alphabetic() {
                j += 1;
            }
            let params: String = chars[i + 2..j].iter().collect();
            match chars.get(j) {
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
            grid[col] = chars[i];
        }
        col += 1;
        i += 1;
    }
    grid.into_iter().collect()
}

/// An INLINE box's text fallback. `paint_inline_box` blanks the box by
/// *writing* `cols` spaces, so the cursor sits a whole box-width to the
/// right of the box when the sink takes over. Before the sink emitted its
/// own CUP, "x+y" landed at columns 8-10 and the painter then homed back to
/// column 8 and painted " cd" straight over it — the fallback rendered
/// nothing at all.
#[test]
fn inline_box_text_fallback_is_cup_ed_to_the_box_origin() {
    let frame = frame_with_failed_math("ab $x+y$ cd\n", 40, 1, true);

    // The exact bytes: SGR reset, four blanking spaces for the 4-cell box,
    // then a CUP back to the box's own origin (row 1, column 4) before the
    // literal TeX. Column 4 is 1-based for the 0-based cell x=3, which is
    // where "ab " ends.
    assert!(
        frame.contains("\u{1b}[0mab \u{1b}[0m    \u{1b}[1;4Hx+y"),
        "expected a box-origin CUP between the blanking run and the \
         fallback text; got {frame:?}"
    );
    // ...and what the terminal shows for them.
    assert_eq!(
        &render_row(&frame, 1, 12),
        "ab x+y  cd  ",
        "the fallback must render inside the box's own cells"
    );
}

/// An OWN-ROW (block) box's text fallback. This path only ever looked
/// correct by accident — the frame loop happens to leave the cursor at
/// column 1 — so it is asserted explicitly now rather than left to luck.
#[test]
fn own_row_box_text_fallback_is_cup_ed_to_the_box_origin() {
    let frame = frame_with_failed_math("before\n\n$$x+y$$\n\nafter\n", 40, 8, true);

    let box_row = (1..=8u16)
        .find(|&r| render_row(&frame, r, 40).trim() == "x+y")
        .unwrap_or_else(|| panic!("the display-math fallback is on no row at all: {frame:?}"));
    assert!(
        frame.contains(&format!("\u{1b}[{box_row};1Hx+y")),
        "the own-row fallback must be preceded by a CUP to its own rect \
         origin (row {box_row}, column 1); got {frame:?}"
    );
    assert_eq!(
        render_row(&frame, 1, 10),
        "before    ",
        "the text above the box is untouched"
    );
    assert_eq!(&render_row(&frame, box_row, 10), "x+y       ");
}

/// The two halves of the rect seam, composed. WAVE2-LAYOUT made
/// `paint_reserved` hand the sink the column the container prefix ends at;
/// WAVE2-SINK made the sink honour whatever column it is handed.
///
/// Precisely what each half buys, because it is not symmetric and the
/// difference is worth recording rather than glossing: without WAVE2-LAYOUT's
/// half the rect says column 0 and the box paints *across* the gutter bar —
/// observably wrong. Without this crate's half the fallback still lands in the
/// right cells here, but only because the prefix run happens to leave the
/// cursor exactly at the box column. That is luck, and the assertion below is
/// on the CUP for that reason: the same luck does not hold for an inline box
/// (the blanking run leaves the cursor a box-width right), and nothing in the
/// type of `CellRect` says the caller must park the cursor for the sink.
#[test]
fn a_blockquoted_box_paints_its_fallback_beside_the_gutter_not_across_it() {
    let frame = frame_with_failed_math("> quoted\n>\n> $$x+y$$\n", 40, 8, true);
    let rows: Vec<String> = (1..=8u16).map(|r| render_row(&frame, r, 12)).collect();
    let box_row = rows
        .iter()
        .position(|r| r.contains("x+y"))
        .unwrap_or_else(|| panic!("the fallback is on no row at all: {rows:?}"));

    // Whatever the gutter glyph is, the box's text starts after it and at the
    // same column the quoted paragraph's own text does.
    // Cell columns, not byte offsets: the gutter glyph is a 3-byte `│`.
    let cell_col = |row: &str, needle: &str| {
        row.find(needle)
            .map(|byte_idx| row[..byte_idx].chars().count())
    };
    let quoted = rows
        .iter()
        .find(|r| r.contains("quoted"))
        .unwrap_or_else(|| panic!("the quoted paragraph vanished: {rows:?}"));
    let text_col = cell_col(quoted, "quoted").unwrap();
    assert!(
        text_col > 0,
        "this test needs a real gutter prefix to be meaningful: {quoted:?}"
    );
    assert_eq!(
        cell_col(&rows[box_row], "x+y"),
        Some(text_col),
        "the fallback must start at the blockquote's content column ({text_col}), \
         not at column 0 across the gutter: {rows:?}"
    );
    assert!(
        frame.contains(&format!("\u{1b}[{};{}Hx+y", box_row + 1, text_col + 1)),
        "and it must be a CUP to that column that puts it there: {frame:?}"
    );
}

/// The txm Unicode-grid rung, the middle of the three text rungs: it is
/// multi-row, so every one of its rows has to re-home to the box column.
/// Row *k* of the box must show row *k* of the grid, starting at the box's
/// own column — not staggered after wherever the previous row ended.
#[test]
fn txm_grid_rung_renders_every_row_inside_the_box() {
    const TEX: &str = r"\sum_{i=0}^{n} x_i";
    let grid = math::render_text(TEX).expect("txm must render a sum");
    assert!(grid.rows.len() >= 3, "this test needs a tall grid");

    let frame = frame_with_failed_math(&format!("before\n\n$${TEX}$$\n\nafter\n"), 40, 12, false);
    // Rows strictly between the two paragraphs — this skips the build stamp,
    // which the painter writes on the final viewport row.
    let rows: Vec<String> = (1..=12u16).map(|r| render_row(&frame, r, 40)).collect();
    let top = rows.iter().position(|r| r.trim() == "before").unwrap() + 1;
    let bottom = rows.iter().position(|r| r.trim() == "after").unwrap();
    let painted: Vec<&str> = rows[top..bottom]
        .iter()
        .map(|r| r.trim_end())
        .skip_while(|r| r.is_empty())
        .take_while(|r| !r.is_empty())
        .collect();
    assert!(
        painted.len() >= 2,
        "the txm grid must render more than one row; rows = {rows:?}"
    );
    for (k, shown) in painted.iter().enumerate() {
        let expected = grid.rows[k].trim_end();
        assert!(
            expected.starts_with(shown),
            "box row {k} must show grid row {k} from its own column 0 \
             (clipped to the box width): expected a prefix of {expected:?}, \
             got {shown:?}; whole frame rows = {rows:?}"
        );
    }
}

/// An image that reserves a box but will not decode falls to alt text,
/// which must land inside the box rather than a box-width to its right.
///
/// The sizer probes headers only and the sink does the full decode, so the
/// two can legitimately disagree: here the sizer's limits accept the file
/// and the sink's reject it, which is exactly what a real over-budget image
/// does. A file that fails the *header* probe never reserves a box at all
/// and so never reaches the sink — that path would make this test vacuous.
#[test]
fn inline_image_alt_text_lands_in_the_box() {
    let img_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdocs/img");
    let doc = Document::parse("hi ![ALT](small.png) there\n");
    let sizer = ImageSizer::new(&img_dir);
    let tree = layout(&doc, 40, &LayoutConfig::default(), &engine(), &sizer);
    assert!(
        (0..tree.line_count()).any(|i| matches!(
            tree.lines(i..i + 1).next(),
            Some(layout::Line::Items(items))
                if items.iter().any(|it| matches!(it, layout::LineItem::Box(_)))
        )),
        "the image must reserve a real inline box, or this test proves nothing"
    );
    let mut painter = Painter::new(engine());
    painter.register_media(Box::new(GfxMediaSink::new(doc, &img_dir).with_limits(
        gfx::Limits {
            max_dim: 1,
            max_alloc: 1,
        },
    )));
    let mut buf = Vec::new();
    painter
        .frame(
            &tree,
            0,
            Size {
                width: 40,
                height: 2,
            },
            &mut buf,
        )
        .unwrap();
    let frame = String::from_utf8_lossy(&buf);
    // small.png is 40x24 px = a 2x1-cell box at the assumed 24x48 px cell,
    // so "ALT" is clipped to the two cells the box actually reserved. That
    // clip is the point: the alt text lands *in* the box, between its
    // neighbours, rather than two cells to the right of it (where the run
    // " there" would then paint straight over it).
    assert_eq!(
        render_row(&frame, 1, 40).trim_end(),
        "hi AL there",
        "alt text must render in the box between its neighbours"
    );
}
