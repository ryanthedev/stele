//! The build stamp: the commit sha the binary was built from, painted flush
//! with the right edge of the last viewport row — and only when the document
//! has left that corner empty.
//!
//! Asserted on the **rendered row**, not on the byte stream. The defect was
//! that the stamp landed on cells the document already owned, and the wire
//! cannot answer that: the stamp's bytes are perfectly well-formed either way,
//! and both the document run and the stamp are legitimately present in the
//! frame. Only replaying the frame the way a terminal would shows which of the
//! two ends up on the glass. (An earlier form of this test asserted that row
//! 3's document text stopped short of the stamp column — the wrong invariant
//! stated at the wrong level: the fix keeps the text and drops the stamp.)
//!
//! Split out of the former `hunter_paint.rs`, which held this subject beside
//! four unrelated ones under an agent's work-assignment label.

mod common;

use ast::Document;
use layout::{LayoutConfig, NullSizer, layout};
use stele::painter::{Painter, Size};

use common::fixtures::engine;
use common::render::render_row;

/// A 3-row viewport painted from `src`, plus the sha that would be stamped.
fn frame_3_rows(src: &str) -> (String, &'static str) {
    let doc = Document::parse(src);
    let config = LayoutConfig {
        min_width: 24,
        max_width: 100,
    };
    let tree = layout(&doc, 80, &config, &engine(), &NullSizer);
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
    (
        String::from_utf8_lossy(&buf).into_owned(),
        env!("STELE_BUILD_SHA"),
    )
}

/// A document whose text fills the last viewport row keeps that text: the
/// stamp is what yields, not the content.
#[test]
fn test_build_stamp_never_overwrites_the_last_rows_document_text() {
    let long: String = (0..60).map(|i| format!("word{i:03} ")).collect();
    // Viewport is 80x3; the wrapped paragraph fills all three rows.
    let (text, sha) = frame_3_rows(&format!("{long}\n"));
    let stamp_col = 80 - sha.chars().count() + 1;
    println!("stamp sha={sha:?} would land at 1-indexed column {stamp_col} of row 3");

    // What the document laid out for row 3, independent of what was painted —
    // the oracle comes from the layout tree, not from the frame under test.
    let doc = Document::parse(&format!("{long}\n"));
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
    assert!(
        !expected.trim().is_empty(),
        "the fixture must actually fill row 3, or this proves nothing"
    );
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

/// The complement, so the guard above is not simply "never paint the stamp": a
/// last row the document leaves room on still gets it, at the exact right
/// column.
#[test]
fn test_build_stamp_still_paints_when_the_last_row_leaves_room() {
    let (text, sha) = frame_3_rows("short\n");
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
