//! The text rungs of the media degradation ladder, shared by every sink.
//!
//! DW-6.2 gives media three rungs: a raster, a Unicode text grid, and the
//! literal source (alt text or TeX). The bottom two are *text painted into a
//! reserved box*, and painting text into a box is fiddly in ways that have
//! already been paid for once — absolute positioning from `rect` rather than
//! the cursor, a cell-measured clip because txm and CJK both emit
//! double-width glyphs, an unconditional cursor move even for an empty row,
//! and a per-rung rule for which line of a multi-line grid a given box row
//! shows.
//!
//! That work lived inside [`crate::media::GfxMediaSink`] while it was the only
//! sink. It moved here when [`crate::media::TextMediaSink`] arrived, because
//! the alternative was a second copy — and a second copy of exactly this code
//! is the drift the clip guard already exists to prevent (see
//! [`write_text_row`]'s note about the copy that had lost its saturating
//! width).

use std::io::Write;

use layout::Reserved;
use width::WidthEngine;

use crate::painter::CellRect;

/// Paints one row of a text rung at `rect`, clipped to the box's width.
///
/// Positioned absolutely from `rect` rather than written at the cursor: the
/// caller's cursor is not on the box (an inline box's blanking spaces leave it
/// a whole box width to the right), and [`gfx::Emitter`] positions its
/// placements absolutely for the same reason. Positioning here rather than at
/// the call sites also makes the seam honour whatever `rect.x` it is handed,
/// so a box indented by a blockquote or list gutter needs no second fix on
/// this side.
///
/// The cell-measured clip is load-bearing: CJK alt text and txm's own
/// double-width glyphs occupy up to two cells per character, so a char-count
/// budget lets the row overflow its reserved box. On the bottom viewport row
/// that overflow wraps, which scrolls the alternate screen and corrupts the
/// frame. It is [`crate::painter::clip_to_width`]'s budget loop, called rather
/// than re-implemented: a second copy of the cell-budget arithmetic is the
/// drift risk this guard exists to prevent, and the copy that used to live
/// inside the sink had already lost the saturating width the original returns.
///
/// `line` is `Option` because a box may be taller than its rung has lines —
/// a top-truncated grid, or a single-line rung on a multi-row box.
pub(crate) fn write_text_row(
    engine: &WidthEngine,
    line: Option<&String>,
    rect: CellRect,
    out: &mut dyn Write,
) {
    let text = line.map(String::as_str).unwrap_or("");
    let (clipped, _) = crate::painter::clip_to_width(text, engine, rect.width.max(1));
    // Emitted unconditionally, even for an empty row: the frame's per-line
    // `CLEAR_TO_EOL` fires from wherever this leaves the cursor, so a row that
    // writes no glyphs still has to leave it at the box's own origin rather
    // than a box-width past it.
    let _ = write!(
        out,
        "\x1b[{};{}H",
        rect.y.saturating_add(1),
        rect.x.saturating_add(1)
    );
    let _ = out.write_all(clipped.as_bytes());
}

/// Which line of a text rung box row `nth_paint` should show.
///
/// The rule differs per rung, and the blanket version is wrong in one
/// direction for each:
///
/// - A **multi-line** rung — a txm math grid — is a picture made of glyphs.
///   Its rows are in a fixed vertical relationship with each other, so box row
///   *k* must show grid row *k*: a top-truncated box has to show the rows it
///   scrolled *to*, not restart the formula partway down the screen. That is
///   `reserved.row`, the box's own coordinate.
/// - A **single-line** rung — image alt text, the literal-TeX source — is not
///   a picture. It has one line and one chance to be seen, and it is what a
///   user gets precisely when something has already failed. Indexing it by box
///   row would make it vanish outright the moment the box is top-truncated,
///   which is worse than showing it one row low, so it stays anchored to the
///   box's first visible row.
pub(crate) fn text_rung_line<'a>(
    lines: &'a [String],
    reserved: &Reserved,
    nth_paint: u16,
) -> Option<&'a String> {
    let index = if lines.len() > 1 {
        reserved.row
    } else {
        nth_paint
    };
    lines.get(usize::from(index))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Reserved` at box row `row`. The node id is a real one from a real
    /// parse — `NodeId`'s field is private, and borrowing one is cheaper than
    /// widening that API for a test.
    fn at_box_row(row: u16) -> Reserved {
        let doc = ast::Document::parse("x\n");
        Reserved {
            node_id: doc
                .nodes()
                .next()
                .expect("a parsed document has a node")
                .id(),
            cols: 10,
            rows: 4,
            row,
        }
    }

    /// The two rungs index differently, and this pins which is which.
    ///
    /// Both branches are one comparison apart — `lines.len() > 1` — and
    /// getting it wrong is invisible in most frames: a box whose top has not
    /// scrolled off has `reserved.row == nth_paint`, so the two rules agree
    /// and any test that never scrolls passes either way. These cases are
    /// deliberately set where they disagree.
    #[test]
    fn test_a_multi_row_grid_indexes_by_box_row_so_a_scrolled_formula_keeps_its_shape() {
        let grid: Vec<String> = ["num", "---", "den"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // The box's top two rows have scrolled above the viewport: this is the
        // first row painted this frame (`nth_paint` 0) but the third row of
        // the formula. It must show the denominator, not the numerator.
        assert_eq!(
            text_rung_line(&grid, &at_box_row(2), 0),
            Some(&"den".to_string()),
            "a top-truncated grid must show the rows it scrolled to"
        );
    }

    /// A single-line rung is not a picture, so it anchors to the box's first
    /// *visible* row instead.
    ///
    /// Indexing it by box row would make it vanish outright the moment the box
    /// is top-truncated — and it is what a reader gets precisely when
    /// something has already failed, so vanishing is the worst available
    /// answer. One row low is the better one.
    #[test]
    fn test_a_single_line_rung_stays_visible_when_the_box_is_top_truncated() {
        let alt = vec!["a picture of a badger".to_string()];
        assert_eq!(
            text_rung_line(&alt, &at_box_row(2), 0),
            Some(&"a picture of a badger".to_string()),
            "the one line a failed box has must not be indexed away"
        );
        // ...and it is not painted again further down the same box.
        assert_eq!(text_rung_line(&alt, &at_box_row(2), 1), None);
    }

    /// A box taller than its rung has lines paints blanks below it rather
    /// than wrapping back to the top.
    #[test]
    fn test_a_row_past_the_end_of_a_grid_has_no_line_to_paint() {
        let grid: Vec<String> = ["one", "two"].iter().map(|s| s.to_string()).collect();
        assert_eq!(text_rung_line(&grid, &at_box_row(5), 5), None);
    }
}
