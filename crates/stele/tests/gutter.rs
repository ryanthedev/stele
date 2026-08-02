//! The page's furniture: padding, the line-number gutter, and the band under
//! the reading line.
//!
//! Every assertion here is **positional**, replayed through the shared
//! terminal model rather than read off the wire, and that is the point rather
//! than a style. The defect archetype this suite exists for is *a count cannot
//! see a position*: "the frame contains the number 7" is true of a frame that
//! painted it in the margin, over the text, or three rows from where it
//! belongs. `render_row` puts the cells where a terminal would and the
//! assertions name columns.

mod common;

use ast::Document;
use layout::{Chrome, LayoutConfig, NullSizer, Padding, layout};
use stele::decor::themed::ThemedDecor;
use stele::painter::{Page, Painter, Size, gutter_width};

use common::fixtures::engine;
use common::render::render_row;

/// A document of `n` one-line paragraphs, each naming its own index, so a row
/// assertion can say *which* line landed where.
fn numbered(n: usize) -> String {
    (0..n)
        .map(|i| format!("para-{i:03}\n\n"))
        .collect::<String>()
}

/// Paints one frame of `source` at `size` with `chrome` and the reading line
/// on `cursor`, and hands back the wire.
fn frame(source: &str, size: Size, chrome: Chrome, cursor: usize) -> String {
    let doc = Document::parse(source);
    let config = LayoutConfig::default();
    // The width the document is laid out at has to come off the same gutter
    // the page is painted with, exactly as `AppState` does it — a test that
    // laid out at the full width would be testing a state the app cannot
    // produce.
    let seed = layout(&doc, size.width, &config, &engine(), &NullSizer);
    let gutter = gutter_width(chrome, seed.line_count());
    let width = size.width.saturating_sub(chrome.horizontal(gutter));
    let tree = layout(&doc, width, &config, &engine(), &NullSizer);
    let page = Page::new(chrome, size, tree.line_count(), cursor);

    let mut painter = Painter::new(engine());
    painter.register_decor(Box::new(ThemedDecor::new(highlight::Theme::new(
        highlight::Variant::Dark,
        highlight::ColorMode::Truecolor,
    ))));
    let mut buf = Vec::new();
    painter
        .frame_page(
            &tree,
            0,
            size,
            page,
            &stele::app::StatusLine::default(),
            Default::default(),
            &mut buf,
        )
        .expect("frame");
    String::from_utf8(buf).expect("utf-8")
}

fn numbers_on() -> Chrome {
    Chrome {
        line_numbers: true,
        ..Chrome::default()
    }
}

/// The gutter's cells, in the columns the arithmetic promises: the number
/// right-aligned in its own column, then a space, then the separator, then the
/// configured gap, and only then the document.
///
/// Right-aligned matters and is checked rather than assumed: a left-aligned
/// column puts `9` and `10` under different cells, so the ones digits stop
/// forming a line and the gutter reads as ragged text instead of a ruler.
#[test]
fn test_the_gutter_right_aligns_its_number_and_the_page_starts_after_it() {
    // Tall enough to actually paint row 19, which is where the first
    // two-digit number lands — a shorter viewport would leave that assertion
    // comparing against a blank row and passing for the wrong reason.
    let size = Size {
        width: 60,
        height: 24,
    };
    let wire = frame(&numbered(20), size, numbers_on(), 0);

    // 20 paragraphs with a blank line between them is 39 rows, so two digits.
    // `gutter_width` = 2 digits + " │" + a 1-cell gap = 5.
    let row1 = render_row(&wire, 1, 60);
    assert!(
        row1.starts_with(" 1 │ para-000"),
        "row 1 must read as number, separator, gap, text — got {row1:?}"
    );
    // Row 3 is the second paragraph — paragraphs are separated by a blank
    // layout row, so the numbers count rendered rows and not paragraphs, which
    // is the whole claim `docs/theming.md` makes about them. Still one digit,
    // so it is padded into the same two-cell column.
    let row3 = render_row(&wire, 3, 60);
    assert!(
        row3.starts_with(" 3 │ para-001"),
        "row 3 must right-align into the same column — got {row3:?}"
    );
    // And a two-digit number fills the column rather than shifting the page:
    // `para-009` is on rendered row 19, and its text starts in the same column
    // `para-000`'s does even though its number is a digit longer.
    let row19 = render_row(&wire, 19, 60);
    assert!(
        row19.starts_with("19 │ para-009"),
        "a two-digit number must not move the text — got {row19:?}"
    );
}

/// The number column is sized by the whole document, not by the rows on
/// screen.
///
/// A gutter that grew a cell when the reader scrolled past line 99 would shift
/// every glyph on the page sideways mid-read — the one thing furniture must
/// never do.
#[test]
fn test_the_number_column_is_sized_by_the_document_not_by_the_visible_rows() {
    let size = Size {
        width: 60,
        height: 6,
    };
    // 120 paragraphs is 239 rows: three digits, none of them on screen at
    // scroll 0.
    let wire = frame(&numbered(120), size, numbers_on(), 0);
    let row1 = render_row(&wire, 1, 60);
    assert!(
        row1.starts_with("  1 │ para-000"),
        "the first screen must already carry the last screen's column width — \
         got {row1:?}"
    );
}

/// Left padding is dead space the page sits *right* of, and it is written as
/// spaces rather than skipped.
///
/// Skipped would leave whatever the terminal had in those cells — the previous
/// frame's first characters, after a scroll — sitting in the margin.
#[test]
fn test_padding_moves_the_page_right_and_blanks_the_margin() {
    let size = Size {
        width: 60,
        height: 8,
    };
    let chrome = Chrome {
        padding: Padding {
            left: 4,
            right: 2,
            ..Padding::default()
        },
        ..Chrome::default()
    };
    let wire = frame(&numbered(10), size, chrome, 0);
    let row1 = render_row(&wire, 1, 60);
    assert_eq!(
        &row1[..4],
        "    ",
        "the left margin must be blank cells, not skipped ones — got {row1:?}"
    );
    assert!(
        row1[4..].starts_with("para-000"),
        "the page must start at column 4 — got {row1:?}"
    );
}

/// Vertical padding pushes the document down and blanks the rows it took.
#[test]
fn test_top_padding_pushes_the_document_down_and_blanks_the_rows_above_it() {
    let size = Size {
        width: 60,
        height: 8,
    };
    let chrome = Chrome {
        padding: Padding {
            top: 2,
            ..Padding::default()
        },
        ..Chrome::default()
    };
    let wire = frame(&numbered(10), size, chrome, 0);
    assert_eq!(
        render_row(&wire, 1, 60).trim(),
        "",
        "row 1 is padding and must be blank"
    );
    assert_eq!(render_row(&wire, 2, 60).trim(), "", "row 2 is padding too");
    assert!(
        render_row(&wire, 3, 60).starts_with("para-000"),
        "the document must start on the row after the padding"
    );
}

/// Chrome is all-or-nothing: a viewport that cannot hold the page plus its
/// furniture gets the page.
///
/// Degrading a cell at a time produces a sequence of intermediate layouts
/// nobody designed — a one-cell left pad, a gutter with no gap — each of which
/// reads as a bug on the way to the narrow terminal it was heading for.
#[test]
fn test_a_viewport_too_narrow_for_furniture_gets_none_of_it() {
    let chrome = Chrome {
        line_numbers: true,
        padding: Padding {
            left: 8,
            right: 8,
            ..Padding::default()
        },
        ..Chrome::default()
    };
    // 8 + 8 padding and a 5-cell gutter leaves 9 cells of content in a 30-cell
    // terminal, well under the 24-cell floor.
    let dropped = chrome.fit(30, 20, gutter_width(chrome, 40));
    assert_eq!(
        dropped,
        Chrome::none(),
        "chrome that does not fit must be dropped whole"
    );
    let kept = chrome.fit(120, 20, gutter_width(chrome, 40));
    assert_eq!(kept, chrome, "chrome that fits must be left alone");
}

/// The band stops at the page's right edge, not at the terminal's.
///
/// stele's content column is capped at 100 cells, so on a wide terminal a band
/// running to the right edge is a stretch of colour with nothing in it — it
/// reads as a rendering fault rather than as a page. Asserted on the wire's
/// SGR rather than through the cell model because a background is a colour, and
/// the cell model only carries glyphs.
#[test]
fn test_the_band_ends_where_the_page_ends() {
    let size = Size {
        width: 60,
        height: 6,
    };
    let chrome = Chrome {
        line_numbers: true,
        padding: Padding {
            left: 3,
            right: 3,
            ..Padding::default()
        },
        ..Chrome::default()
    };
    let wire = frame(&numbered(10), size, chrome, 0);
    // The band's own SGR closes before the erase, so the row's tail is erased
    // with no background set. An unreset band would fill the rest of the
    // terminal on any terminal with BCE, which is exactly the bug.
    let first_row_end = wire.find("\x1b[2;1H").expect("a second row");
    let first_row = &wire[..first_row_end];
    let last_reset = first_row.rfind("\x1b[0m").expect("a closing reset");
    let after_reset = &first_row[last_reset..];
    assert!(
        after_reset.contains("\x1b[K"),
        "the reading line must reset its background before erasing to the end \
         of the row, or the band runs to the terminal's edge: {first_row:?}"
    );
}

/// The reading line's number takes a different role from its neighbours'.
///
/// This is the half of the highlight that survives an image: on a row whose
/// content cells are under a kitty raster the band is invisible, and the
/// gutter is the only thing left that can say where the reader is.
#[test]
fn test_the_reading_lines_number_is_painted_in_its_own_role() {
    let size = Size {
        width: 60,
        height: 8,
    };
    let wire = frame(&numbered(10), size, numbers_on(), 4);

    // Everything between a row's CUP and its first glyph: the full SGR run
    // the gutter opened with, not just its first sequence. `write_sgr` always
    // leads with a reset, so comparing one sequence would compare two resets
    // and pass whatever the styles were.
    let sgr_before = |row: u16| {
        let cup = format!("\x1b[{row};1H");
        let at = wire.find(&cup).expect("row present");
        let rest: Vec<char> = wire[at + cup.len()..].chars().collect();
        let mut i = 0;
        while rest.get(i) == Some(&'\u{1b}') {
            i += 2; // ESC [
            while i < rest.len() && !rest[i].is_ascii_alphabetic() {
                i += 1;
            }
            i += 1; // the final byte
        }
        rest[..i].iter().collect::<String>()
    };
    let ordinary = sgr_before(1);
    let reading = sgr_before(5);
    assert_ne!(
        ordinary, reading,
        "the reading line's number must not resolve to the same style as an \
         ordinary one — on an image row it is the whole of the highlight"
    );
    assert!(
        ordinary.contains("\x1b[2m"),
        "an ordinary line number is dim so a column of digits does not compete \
         with the prose beside it — got {ordinary:?}"
    );
    assert!(
        !reading.contains("\x1b[2m"),
        "the reading line's number drops the dim rather than adding bold, \
         which would move the digits — got {reading:?}"
    );
}

/// A gutter that is switched off costs nothing: the frame is byte-identical to
/// the one a caller with no `Page` at all gets.
///
/// The property that makes this feature safe to ship on by default. Every
/// existing frame test asserts exact bytes, and the reader who never turns any
/// of this on must keep getting exactly the frame they got before it existed.
#[test]
fn test_no_furniture_paints_the_same_bytes_as_no_page_at_all() {
    let size = Size {
        width: 60,
        height: 8,
    };
    let source = numbered(10);
    let doc = Document::parse(&source);
    let config = LayoutConfig::default();
    let tree = layout(&doc, size.width, &config, &engine(), &NullSizer);

    let paint = |page: Page| {
        let mut painter = Painter::new(engine());
        let mut buf = Vec::new();
        painter
            .frame_page(
                &tree,
                0,
                size,
                page,
                &stele::app::StatusLine::default(),
                Default::default(),
                &mut buf,
            )
            .expect("frame");
        buf
    };

    let full = paint(Page::full(size));
    let none = paint(Page::new(Chrome::none(), size, tree.line_count(), 0));
    assert_eq!(
        full, none,
        "`Chrome::none()` must be indistinguishable from the pre-gutter frame"
    );
}

/// The separator is one cell wide, measured through the width engine rather
/// than assumed.
///
/// Load-bearing: `gutter_width` budgets exactly one cell for it, so a glyph the
/// terminal renders double-width would push the whole page one column right of
/// where the document was laid out, clipping its last cell on every row.
#[test]
fn test_the_gutter_separator_is_one_cell_in_the_pinned_corpus() {
    assert_eq!(
        engine().display_width("│"),
        1,
        "the gutter separator must be a single cell"
    );
}
