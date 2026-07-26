//! DW-5.3: folding a range that contains a placed image must remove that
//! placement, and unfolding must restore it.
//!
//! Asserted on the exact bytes `Painter::frame` puts on the wire — the kitty
//! `a=p` placement command — the same level `reserved_column.rs` and
//! `scroll_placement.rs` use for the same reason: `media/sink.rs`'s own
//! module doc states the invariant this leans on ("a box that is not painted
//! in a frame is not on the screen after that frame"), so folding needs no
//! sink-side change at all — it only has to make the box stop being *laid
//! out* while its section is collapsed, which is `layout::layout_with_folds`'s
//! job. This file is the proof that the two sides actually meet.

mod common;

use ast::Document;
use layout::{FoldState, LayoutConfig, layout_with_folds};
use stele::media::GfxMediaSink;
use stele::{ImageSizer, Painter, Size};

use common::fixtures::{engine, scratch_dir, write_png};

const VIEWPORT: Size = Size {
    width: 60,
    height: 12,
};

/// 48x96 px: comfortably inside the viewport at the sink's assumed 24x48
/// cell geometry, so its placement is never itself the reason a byte is
/// missing.
const PNG_PX: (u32, u32) = (48, 96);

fn frame_with(doc: &Document, dir: &std::path::Path, folds: &FoldState) -> String {
    let config = LayoutConfig {
        min_width: 24,
        max_width: VIEWPORT.width,
    };
    let sizer = ImageSizer::new(dir);
    let tree = layout_with_folds(doc, VIEWPORT.width, &config, &engine(), &sizer, folds);
    let mut painter = Painter::new(engine());
    painter.register_media(Box::new(GfxMediaSink::new(doc.clone(), dir)));
    let mut buf = Vec::new();
    painter.frame(&tree, 0, VIEWPORT, &mut buf).expect("paint");
    String::from_utf8(buf).expect("painter output is utf-8")
}

#[test]
fn test_dw_5_3_folding_a_section_with_an_image_removes_its_placement_and_unfolding_restores_it() {
    let dir = scratch_dir("fold-placement");
    write_png(&dir, "t.png", PNG_PX.0, PNG_PX.1);
    let doc = Document::parse("# One\n\n![alt](t.png)\n\n# Two\n\nBeta.\n");
    let one = doc.blocks()[0].id;

    let open = frame_with(&doc, &dir, &FoldState::default());
    assert!(
        open.contains("\x1b_Ga=p"),
        "test setup: the image must be placed before anything is folded:\n{open:?}"
    );

    let mut folds = FoldState::default();
    folds.toggle(one);
    let folded = frame_with(&doc, &dir, &folds);
    assert!(
        !folded.contains("\x1b_Ga=p"),
        "folding the section holding the image must remove its placement:\n{folded:?}"
    );
    assert!(
        folded.contains("hidden"),
        "the fold marker must still be on screen:\n{folded:?}"
    );

    folds.toggle(one);
    let restored = frame_with(&doc, &dir, &folds);
    assert!(
        restored.contains("\x1b_Ga=p"),
        "unfolding must restore the placement:\n{restored:?}"
    );
    assert_eq!(
        restored, open,
        "unfolding must repaint byte-for-byte identically to never having folded"
    );
}
