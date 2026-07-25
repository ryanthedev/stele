//! Regression: a media box inside a **container** must be placed at the
//! column its content starts at, and the container's own prefix — the
//! blockquote gutter bar, the list marker — must survive on every row of the
//! box.
//!
//! Asserted on the exact bytes `Painter::frame` puts on the wire: the CUP that
//! immediately precedes the kitty `a=p` put, and the glyphs painted ahead of
//! it on the same row. That level is the point of this file, the same way it
//! is the point of `scroll_placement.rs`. The layout suite already asserted
//! `line.width() <= tree.width()` on every line — true of a `Line::Reserved`
//! whose width is just `cols`, and true whether that box is drawn beside its
//! gutter or straight across it. A bound cannot see a column.

mod common;

use layout::{LayoutConfig, layout};
use stele::media::GfxMediaSink;
use stele::{ImageSizer, Painter, Size};

use common::fixtures::{engine, scratch_dir, write_png};
use common::render::render_row;

const VIEWPORT: Size = Size {
    width: 60,
    height: 12,
};

/// The fixture image is 48x96 px: 2 cells wide, 2 cells tall at the sink's
/// assumed 24x48 cell geometry — small enough to sit well inside every
/// container tested here, so `cols` can never be mistaken for the container's
/// own width.
const PNG_PX: (u32, u32) = (48, 96);

fn frame_of(tag: &str, src: &str) -> String {
    let dir = scratch_dir(&format!("reserved-column-{tag}"));
    write_png(&dir, "t.png", PNG_PX.0, PNG_PX.1);
    let doc = ast::Document::parse(src);
    let config = LayoutConfig {
        min_width: 24,
        max_width: VIEWPORT.width,
    };
    let sizer = ImageSizer::new(&dir);
    let tree = layout(&doc, VIEWPORT.width, &config, &engine(), &sizer);
    let mut painter = Painter::new(engine());
    painter.register_media(Box::new(GfxMediaSink::new(doc, &dir)));
    let mut buf = Vec::new();
    painter.frame(&tree, 0, VIEWPORT, &mut buf).expect("paint");
    String::from_utf8(buf).expect("painter output is utf-8")
}

/// The `(row, column)` of the CUP immediately preceding the `a=p` put, both
/// 1-indexed exactly as they appear on the wire.
fn placement_cup(frame: &str) -> (u16, u16) {
    let put = frame
        .find("\x1b_Ga=p")
        .expect("a placement must be emitted at all");
    let cup_start = frame[..put].rfind("\x1b[").expect("a CUP before the put");
    let cup = &frame[cup_start + 2..put];
    assert!(
        cup.ends_with('H'),
        "the escape immediately before the put must be a CUP, got {cup:?}"
    );
    let mut parts = cup.trim_end_matches('H').split(';');
    let row = parts.next().unwrap().parse().unwrap();
    let col = parts.next().unwrap().parse().unwrap();
    (row, col)
}

/// Everything painted on 1-indexed `row` before the first APC on that row —
/// i.e. the container prefix, with SGR sequences stripped.
fn glyphs_before_the_box(frame: &str, row: u16) -> String {
    let home = format!("\x1b[{row};1H");
    let rest = frame
        .split(&home)
        .nth(1)
        .unwrap_or_else(|| panic!("row {row} was never homed to"));
    let mut out = String::new();
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                // A CUP away from column 1 ends the prefix; so does any APC.
                Some('_') => break,
                Some('[') => {
                    let mut seq = String::new();
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() {
                            seq.push(c);
                            break;
                        }
                        seq.push(c);
                    }
                    if seq.ends_with('H') {
                        break;
                    }
                }
                _ => break,
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// A blockquote's gutter is two cells (`│ `), so the box belongs at 0-indexed
/// column 2 — 1-indexed column 3 on the wire — with the bar still painted on
/// every one of the box's rows.
#[test]
fn test_box_in_a_blockquote_is_placed_beside_the_gutter_not_across_it() {
    let frame = frame_of("quote", "> before\n>\n> ![tall](t.png)\n>\n> after\n");
    let (row, col) = placement_cup(&frame);
    assert_eq!(
        col, 3,
        "the box must start past the two-cell gutter, not at column 1: {frame:?}"
    );
    // "> before", "> (blank)", then the box's first row.
    assert_eq!(row, 3, "the box's first row: {frame:?}");
    for box_row in row..row + 2 {
        assert_eq!(
            glyphs_before_the_box(&frame, box_row),
            "\u{2502} ",
            "row {box_row} lost the blockquote's gutter bar: {frame:?}"
        );
    }
}

/// Two levels of list marker: `• ` inside `• `, so the box starts at
/// 0-indexed column 4 — 1-indexed 5 — and the *marker itself* is painted on
/// the box's first row rather than being dropped with the box's line.
#[test]
fn test_box_in_a_nested_list_is_placed_past_both_markers_and_keeps_them() {
    let frame = frame_of("nested", "- outer\n  - ![tall](t.png)\n");
    let (row, col) = placement_cup(&frame);
    assert_eq!(
        col, 5,
        "the box must start past `• ` inside `• `: {frame:?}"
    );
    assert_eq!(
        row, 2,
        "the box's first row is the nested item's: {frame:?}"
    );
    assert_eq!(
        glyphs_before_the_box(&frame, row),
        "  \u{2022} ",
        "the nested item's bullet is painted on the box's first row: {frame:?}"
    );
    assert_eq!(
        glyphs_before_the_box(&frame, row + 1),
        "    ",
        "later rows carry the continuation indent, not another bullet: {frame:?}"
    );
}

/// The complement, so nothing above is an artifact of always shifting right:
/// a top-level box still starts at column 1 and paints no prefix at all.
#[test]
fn test_top_level_box_still_starts_at_column_one() {
    let frame = frame_of("toplevel", "before\n\n![tall](t.png)\n\nafter\n");
    let (row, col) = placement_cup(&frame);
    assert_eq!(col, 1, "no container, no offset: {frame:?}");
    assert_eq!(
        glyphs_before_the_box(&frame, row),
        "",
        "nothing is painted ahead of a top-level box: {frame:?}"
    );
}

/// The two halves of the rect seam, composed. WAVE2-LAYOUT's half makes
/// `rect.x` correct for a box inside a container; WAVE2-SINK's half
/// (`write_text_row`'s box-origin CUP) makes the sink act on it. This asserts
/// what a reader actually sees when both are in: a formula that degrades to its
/// literal TeX inside a blockquote renders *beside* the gutter bar.
///
/// Precisely what each half buys, because it is not symmetric and the
/// difference is worth recording rather than glossing: without the layout half
/// the rect says column 0 and the box paints *across* the gutter bar —
/// observably wrong. Without the sink's half the fallback still lands in the
/// right cells here, but only because the prefix run happens to leave the
/// cursor exactly at the box column. That is luck, and it is why the CUP is
/// asserted rather than only the rendering: the same luck does not hold for an
/// inline box (whose blanking run leaves the cursor a box-width right — see
/// `media_fallback_position.rs`), and nothing in the type of `CellRect` says
/// the caller must park the cursor for the sink.
///
/// Three oracles, deliberately: the CUP on the wire, the glyphs the painter put
/// ahead of it, and the row a terminal would actually show — the last one is
/// what catches a gutter bar that is painted and then overwritten.
#[test]
fn test_text_fallback_inside_a_blockquote_lands_past_the_gutter() {
    let doc = ast::Document::parse("> before\n>\n> $$x+y$$\n");
    let config = LayoutConfig {
        min_width: 24,
        max_width: VIEWPORT.width,
    };
    let sizer = ImageSizer::new(".");
    let tree = layout(&doc, VIEWPORT.width, &config, &engine(), &sizer);
    let mut sink = GfxMediaSink::new(doc, ".");
    sink.force_ratex_failure_for_test(true);
    sink.force_txm_failure_for_test(true);
    let mut painter = Painter::new(engine());
    painter.register_media(Box::new(sink));
    let mut buf = Vec::new();
    painter.frame(&tree, 0, VIEWPORT, &mut buf).expect("paint");
    let frame = String::from_utf8(buf).expect("utf-8");
    println!("frame: {frame:?}");
    assert!(
        frame.contains("\x1b[3;3Hx+y"),
        "the literal-TeX rung must be homed to the box's own column (3): {frame:?}"
    );
    assert_eq!(
        glyphs_before_the_box(&frame, 3),
        "\u{2502} ",
        "and the gutter bar is still there: {frame:?}"
    );

    // What the reader sees, and stated relative to the quoted paragraph rather
    // than to the literal column 3: the fallback starts at the same cell column
    // the blockquote's own text does. Cell columns, not byte offsets — the
    // gutter glyph is a 3-byte `│`.
    let rows: Vec<String> = (1..=VIEWPORT.height)
        .map(|r| render_row(&frame, r, 12))
        .collect();
    let cell_col = |row: &str, needle: &str| {
        row.find(needle)
            .map(|byte_idx| row[..byte_idx].chars().count())
    };
    let quoted = rows
        .iter()
        .find(|r| r.contains("before"))
        .unwrap_or_else(|| panic!("the quoted paragraph vanished: {rows:?}"));
    let text_col = cell_col(quoted, "before").unwrap();
    assert!(
        text_col > 0,
        "this assertion needs a real gutter prefix to mean anything: {quoted:?}"
    );
    assert_eq!(
        cell_col(&rows[2], "x+y"),
        Some(text_col),
        "the fallback must render at the blockquote's content column ({text_col}), \
         not at column 0 across the gutter: {rows:?}"
    );
}
