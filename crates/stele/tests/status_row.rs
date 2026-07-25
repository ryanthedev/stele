//! DW-1.1: the status row Phase 1 reserves. Reproduces `main.rs`'s own
//! contract — `AppState`'s content [`Size`] is the real terminal rows minus
//! one, and `Painter::frame_with_status` paints exactly one more row after
//! it — from outside both modules, the way a regression would actually
//! surface: a document tall enough to fill every row would, without the
//! reservation, leave its last line sitting exactly where the status row now
//! goes.

mod common;

use ast::Document;
use layout::{LayoutConfig, NullSizer, layout};
use stele::app::{AppState, FileInfo};
use stele::painter::{Painter, Size};

use common::fixtures::engine;
use common::render::render_row;

/// A single fenced code block of `n` distinctly-numbered lines. Unlike
/// separate paragraphs (which the parser spaces with a blank layout line
/// between each), a fence preserves one *tree* line per *source* line with
/// no spacer, so `tree`'s 0-indexed line `i` is always exactly `"line {i}"`.
fn numbered_lines(n: usize) -> String {
    let mut src = String::from("```\n");
    for i in 0..n {
        src.push_str(&format!("line {i}\n"));
    }
    src.push_str("```\n");
    src
}

/// The painted text of tree line `index` — the same extraction
/// `app.rs`'s own tests use as their oracle for "what would this row show".
fn line_text(tree: &layout::LayoutTree, index: usize) -> String {
    use layout::{Line, LineItem};
    match tree.lines(index..index + 1).next() {
        Some(Line::Items(items)) => items
            .iter()
            .filter_map(|item| match item {
                LineItem::Run(run) => Some(run.text.as_str()),
                LineItem::Box(_) => None,
            })
            .collect(),
        _ => String::new(),
    }
}

#[test]
fn test_dw_1_1_content_height_is_rows_minus_one_and_the_status_row_carries_no_content() {
    let doc = Document::parse(&numbered_lines(30));
    let config = LayoutConfig::default();
    // Mirrors `main.rs`: `rows` is the real terminal height; `AppState`'s
    // own `Size` is `rows - 1`.
    let rows: u16 = 10;
    let content_size = Size {
        width: 40,
        height: rows - 1,
    };
    let tree = layout(&doc, content_size.width, &config, &engine(), &NullSizer);
    assert!(
        tree.line_count() >= usize::from(rows),
        "fixture must have more lines than the viewport, or this proves nothing"
    );
    // Ground truth for what the reservation must keep off the wire: the
    // text tree line 9 (0-indexed — the tenth line) carries, which a bug
    // reverting content height to the full `rows` would paint on row 10.
    let line_9_text = line_text(&tree, 9);
    assert!(
        line_9_text.contains("line 9"),
        "fixture line 9 must be distinguishable: {line_9_text:?}"
    );

    let file_info = FileInfo {
        name: "doc.md".to_string(),
        byte_size: 0,
        line_count: 0,
    };
    let mut state = AppState::new(tree, content_size, file_info);

    let mut painter = Painter::new(engine());
    let mut buf = Vec::new();
    let status = state.status();
    painter
        .frame_with_status(
            state.tree(),
            state.scroll(),
            state.size(),
            &status,
            &mut buf,
        )
        .unwrap();
    let text = String::from_utf8_lossy(&buf).into_owned();

    // Content occupies rows 1..=9 (nine rows: `content_size.height`), so the
    // ninth row shows tree line 8.
    assert!(
        render_row(&text, 9, 40).contains("line 8"),
        "the last content row must still show the document: {:?}",
        render_row(&text, 9, 40)
    );

    // Row 10 is the status row. If content height had not been reduced (the
    // bug this test exists to catch), tree line 9 would have landed here
    // instead.
    let row_10 = render_row(&text, 10, 40);
    assert!(
        !row_10.contains("line 9"),
        "document content reached the reserved status row: {row_10:?}"
    );
    assert!(
        row_10.contains("doc.md") && row_10.contains('%'),
        "the status row must carry the ruler (name + position%): {row_10:?}"
    );
}

/// One-row terminal: content height is 0 (nothing to show), and the single
/// available row is the status row. Must degrade, not panic.
#[test]
fn test_dw_1_1_status_row_degrades_gracefully_on_a_one_row_terminal() {
    let doc = Document::parse(&numbered_lines(5));
    let config = LayoutConfig::default();
    let content_size = Size {
        width: 40,
        height: 0,
    };
    let tree = layout(&doc, content_size.width, &config, &engine(), &NullSizer);
    let mut state = AppState::new(tree, content_size, FileInfo::default());

    let mut painter = Painter::new(engine());
    let mut buf = Vec::new();
    let status = state.status();
    painter
        .frame_with_status(
            state.tree(),
            state.scroll(),
            state.size(),
            &status,
            &mut buf,
        )
        .unwrap();
    let text = String::from_utf8_lossy(&buf);
    assert!(
        text.contains("0%"),
        "the one available row must carry the status ruler: {text:?}"
    );
}

/// Two-row terminal: one content row, one status row.
#[test]
fn test_dw_1_1_status_row_degrades_gracefully_on_a_two_row_terminal() {
    let doc = Document::parse(&numbered_lines(5));
    let config = LayoutConfig::default();
    let content_size = Size {
        width: 40,
        height: 1,
    };
    let tree = layout(&doc, content_size.width, &config, &engine(), &NullSizer);
    let mut state = AppState::new(tree, content_size, FileInfo::default());

    let mut painter = Painter::new(engine());
    let mut buf = Vec::new();
    let status = state.status();
    painter
        .frame_with_status(
            state.tree(),
            state.scroll(),
            state.size(),
            &status,
            &mut buf,
        )
        .unwrap();
    let text = String::from_utf8_lossy(&buf).into_owned();

    assert!(
        render_row(&text, 1, 40).contains("line 0"),
        "the sole content row must show the document: {:?}",
        render_row(&text, 1, 40)
    );
    assert!(
        render_row(&text, 2, 40).contains('%'),
        "row 2 must be the status row: {:?}",
        render_row(&text, 2, 40)
    );
}
