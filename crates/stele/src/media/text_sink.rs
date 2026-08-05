//! The sink for a terminal that cannot carry a raster.
//!
//! Its whole job is DW-6.2's second rung: paint a formula as the Unicode text
//! grid `math::render_text` produces, and fall to the literal TeX source when
//! txm declines. It never touches `gfx`, holds no residency cache and owns no
//! emitter — which is what makes "zero graphics escapes under `$TMUX`"
//! (DW-6.4) a structural fact about this type rather than an assertion someone
//! has to keep re-running. There is no code path from here to an APC byte,
//! because there is no `gfx` handle to reach it through.
//!
//! Images are not its business: under [`MediaMode::TextOnly`] the sizer
//! declines every image node, so layout never reserves a box for one and the
//! alt text takes the ordinary text path through `layout::inline`. Only
//! *display* math reaches this sink.

use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;

use ast::{Document, InlineKind, NodeId, NodeRef};
use layout::Reserved;
use width::WidthEngine;

use crate::media::MediaSink;
use crate::media::rung;
use crate::painter::{CellRect, sanitize};

pub struct TextMediaSink {
    doc: Rc<Document>,
    width_engine: WidthEngine,
    /// How many rows of each box this frame has painted so far. The box's
    /// first painted row resolves its content; every later row indexes into
    /// what that first row cached.
    row_in_frame: HashMap<NodeId, u16>,
    /// This frame's resolved lines per box.
    ///
    /// Both maps are cleared in `begin_frame`, and that is not tidiness — it
    /// is the only reliable frame boundary a sink has (see
    /// [`MediaSink::begin_frame`]). Without the reset, `row_in_frame` keeps
    /// climbing across frames: on frame two a three-row formula starts at grid
    /// row three, paints blanks, and visibly walks out of its own box on every
    /// repaint. The trait defaults `begin_frame` to a no-op, so forgetting it
    /// is silent.
    resolved_this_frame: HashMap<NodeId, Vec<String>>,
}

impl TextMediaSink {
    pub fn new(doc: Rc<Document>, width_engine: WidthEngine) -> Self {
        TextMediaSink {
            doc,
            width_engine,
            row_in_frame: HashMap::new(),
            resolved_this_frame: HashMap::new(),
        }
    }

    /// The lines this box shows: the txm grid, or the literal TeX when txm
    /// declines the formula.
    ///
    /// Every line is `sanitize`d on the way out. That is not belt-and-braces
    /// over `math::render_text`'s own stripping: that one is deliberately
    /// narrow — SGR only, "the only escape kind txm's terminal backend emits"
    /// — while `sanitize` is the painter's barricade against every display
    /// hazard, and TeX source reaching the literal rung has had nothing done
    /// to it at all.
    fn lines_for(&self, node_id: NodeId) -> Vec<String> {
        let Some(NodeRef::Inline(inline)) = self.doc.node(node_id) else {
            return vec![String::new()];
        };
        let InlineKind::Math { tex, .. } = &inline.kind else {
            // Not math. Under `TextOnly` nothing else reserves a box, so this
            // is unreachable in the viewer — but a blank row is the right
            // answer for a box this sink cannot describe, and it keeps the
            // seam total rather than panicking on a shape it did not expect.
            return vec![String::new()];
        };
        match math::render_text(tex) {
            Some(grid) => grid.rows.iter().map(|row| sanitize(row)).collect(),
            None => vec![sanitize(tex)],
        }
    }
}

impl MediaSink for TextMediaSink {
    fn begin_frame(&mut self, _out: &mut dyn Write) {
        self.row_in_frame.clear();
        self.resolved_this_frame.clear();
    }

    fn paint(&mut self, reserved: &Reserved, rect: CellRect, out: &mut dyn Write) {
        let row_index = *self.row_in_frame.get(&reserved.node_id).unwrap_or(&0);
        self.row_in_frame.insert(reserved.node_id, row_index + 1);

        if row_index == 0 {
            let lines = self.lines_for(reserved.node_id);
            let line = rung::text_rung_line(&lines, reserved, 0);
            rung::write_text_row(&self.width_engine, line, rect, out);
            self.resolved_this_frame.insert(reserved.node_id, lines);
        } else if let Some(lines) = self.resolved_this_frame.get(&reserved.node_id) {
            let line = rung::text_rung_line(lines, reserved, row_index);
            rung::write_text_row(&self.width_engine, line, rect, out);
        }
    }

    fn reload_document(&mut self, doc: Rc<Document>, _out: &mut dyn Write) {
        // A re-parse renumbers nodes, so every cached `NodeId` now names
        // something else. Swapping the document without dropping the caches is
        // how a sink starts answering with the wrong node's content.
        self.doc = doc;
        self.row_in_frame.clear();
        self.resolved_this_frame.clear();
    }
}

#[cfg(test)]
mod tests {
    use layout::{LayoutConfig, layout};
    use width::WidthConfig;

    use super::*;
    use crate::media::{ImageSizer, MediaMode};
    use crate::painter::{Painter, Size};

    fn engine() -> WidthEngine {
        WidthEngine::new(WidthConfig::default())
    }

    /// Paints `frames` consecutive frames of a display formula and returns
    /// what reached the wire each time.
    fn frames_of(source: &str, count: usize) -> Vec<String> {
        let doc = Rc::new(Document::parse(source));
        let sizer = ImageSizer::in_mode(".", MediaMode::TextOnly);
        let tree = layout(&doc, 80, &LayoutConfig::default(), &engine(), &sizer);
        let mut painter = Painter::new(engine());
        painter.register_media(Box::new(TextMediaSink::new(Rc::clone(&doc), engine())));
        (0..count)
            .map(|_| {
                let mut buf = Vec::new();
                painter
                    .frame(
                        &tree,
                        0,
                        Size {
                            width: 80,
                            height: 12,
                        },
                        &mut buf,
                    )
                    .expect("a frame paints");
                String::from_utf8_lossy(&buf).into_owned()
            })
            .collect()
    }

    /// The box's rows carry the grid's rows, in order, one per row.
    ///
    /// The oracle is `math::render_text`'s own output, asked for independently
    /// of the sink: whatever txm draws for this fraction, row *k* of the box
    /// must be row *k* of that. A weaker version of this test — "the frame
    /// contains some alphanumerics" — passed against a sink whose `paint` had
    /// been replaced with an empty body, because the surrounding prose supplied
    /// the alphanumerics. Mutation testing found that; this is the repair.
    #[test]
    fn test_each_row_of_the_box_paints_its_own_row_of_the_grid() {
        let grid = math::render_text(r"\frac{a}{b}").expect("txm renders a fraction");
        assert!(
            grid.rows.len() >= 3,
            "test setup: a fraction must be several rows, got {:?}",
            grid.rows
        );
        let painted = &frames_of("$$\\frac{a}{b}$$\n", 1)[0];
        for row in &grid.rows {
            let trimmed = row.trim_end();
            if trimmed.is_empty() {
                continue;
            }
            assert!(
                painted.contains(trimmed),
                "grid row {trimmed:?} never reached the wire; frame was {painted:?}"
            );
        }
    }

    /// A repaint must show the reader the same formula it showed a moment ago.
    ///
    /// This is the defect `begin_frame` exists for. Without the reset,
    /// `row_in_frame` keeps climbing across frames, so on frame two the box's
    /// first row is no longer row zero: it takes the "later row" branch and
    /// reads a cache that a `reload_document` may since have emptied.
    /// `MediaSink::begin_frame` defaults to a no-op, so a sink that forgets it
    /// fails silently and only on the *second* frame — the frame no
    /// single-shot test paints.
    ///
    /// The state assertion below is the one that can actually fail. Comparing
    /// frame two to frame one cannot: a multi-row grid indexes by
    /// [`Reserved::row`], which is the box's own coordinate and does not
    /// depend on the counter, so the pixels hold still even while the counter
    /// runs away. What runs away with it is the memory: two maps that are
    /// never emptied, one entry per formula per frame, for as long as the
    /// session lasts.
    #[test]
    fn test_a_frame_boundary_empties_the_per_frame_state() {
        let painted = frames_of("$$\\frac{a}{b}$$\n", 3);
        assert_eq!(
            painted[1], painted[0],
            "the second frame drifted from the first"
        );
        assert_eq!(
            painted[2], painted[0],
            "the third frame drifted from the first"
        );

        let doc = Rc::new(Document::parse("$$\\frac{a}{b}$$\n"));
        let mut sink = TextMediaSink::new(Rc::clone(&doc), engine());
        let node = math_node(&doc);
        let rect = CellRect {
            x: 0,
            y: 0,
            width: 40,
            height: 3,
        };
        let mut out = Vec::new();
        sink.paint(
            &Reserved {
                node_id: node,
                cols: 10,
                rows: 3,
                row: 0,
            },
            rect,
            &mut out,
        );
        assert_eq!(
            sink.row_in_frame.len(),
            1,
            "test setup: painting must record per-frame state to begin with"
        );
        assert_eq!(sink.resolved_this_frame.len(), 1);

        sink.begin_frame(&mut out);
        assert!(
            sink.row_in_frame.is_empty() && sink.resolved_this_frame.is_empty(),
            "a frame boundary must empty both maps, or they grow for the life of the session"
        );
    }

    /// The counter advances by one per painted row, and only the box's first
    /// row of a frame re-resolves the formula.
    ///
    /// Pinned directly because the arithmetic is invisible in the pixels: a
    /// counter that multiplied instead of incrementing, or a first-row test
    /// inverted from `==` to `!=`, both still paint a plausible-looking frame.
    #[test]
    fn test_only_the_first_painted_row_of_a_box_resolves_its_formula() {
        let doc = Rc::new(Document::parse("$$\\frac{a}{b}$$\n"));
        let mut sink = TextMediaSink::new(Rc::clone(&doc), engine());
        let node = math_node(&doc);
        let rect = CellRect {
            x: 0,
            y: 0,
            width: 40,
            height: 3,
        };
        let mut out = Vec::new();

        for row in 0..3u16 {
            sink.paint(
                &Reserved {
                    node_id: node,
                    cols: 10,
                    rows: 3,
                    row,
                },
                rect,
                &mut out,
            );
            assert_eq!(
                sink.row_in_frame.get(&node).copied(),
                Some(row + 1),
                "the counter must advance by exactly one per painted row"
            );
        }

        // Every row painted something: three absolute cursor moves, one per
        // row. An inverted first-row test would resolve nothing on row zero
        // and then find no cache on rows one and two.
        assert_eq!(
            String::from_utf8_lossy(&out).matches("\x1b[").count(),
            3,
            "each of the box's three rows must be positioned and painted"
        );
    }

    /// `--watch` reloaded the file, so every `NodeId` this sink is holding now
    /// names a node in a document that no longer exists.
    ///
    /// A re-parse renumbers nodes. Swapping the document without dropping the
    /// caches is how a sink starts answering with the wrong node's content —
    /// the reader edits one formula and a different one changes on screen. The
    /// oracle is the new document's own text: after the reload the sink must
    /// describe what is in the file now.
    #[test]
    fn test_a_reload_swaps_the_document_and_drops_what_the_old_one_cached() {
        let before = Rc::new(Document::parse("$$\\frac{a}{b}$$\n"));
        let mut sink = TextMediaSink::new(Rc::clone(&before), engine());
        let node = math_node(&before);
        let rect = CellRect {
            x: 0,
            y: 0,
            width: 40,
            height: 3,
        };
        let mut out = Vec::new();
        sink.paint(
            &Reserved {
                node_id: node,
                cols: 10,
                rows: 3,
                row: 0,
            },
            rect,
            &mut out,
        );
        assert!(
            !sink.resolved_this_frame.is_empty(),
            "test setup: the old document's formula must be cached first"
        );

        // A different formula entirely, so "the sink answered from the new
        // document" is a claim with content.
        let after = Rc::new(Document::parse("$$\\frac{p}{q}$$\n"));
        sink.reload_document(Rc::clone(&after), &mut out);
        assert!(
            sink.row_in_frame.is_empty() && sink.resolved_this_frame.is_empty(),
            "a reload must drop every per-node cache, or stale NodeIds answer"
        );

        let expected = math::render_text(r"\frac{p}{q}")
            .expect("txm renders the new fraction")
            .rows
            .iter()
            .map(|row| sanitize(row))
            .collect::<Vec<_>>();
        assert_eq!(
            sink.lines_for(math_node(&after)),
            expected,
            "after a reload the sink must describe the document it was handed"
        );
    }

    fn math_node(doc: &Document) -> ast::NodeId {
        doc.nodes()
            .find(|n| matches!(n, ast::NodeRef::Inline(i) if matches!(i.kind, InlineKind::Math { .. })))
            .expect("the document has a math node")
            .id()
    }

    /// DW-6.4, made structural rather than asserted.
    ///
    /// This sink holds no `gfx` handle, no emitter and no residency cache, so
    /// there is no code path from here to an APC byte. The assertion is still
    /// worth writing — it is what would catch someone reaching for `gfx` in
    /// here later — but the reason to trust it is the type, not the test.
    #[test]
    fn test_a_text_only_frame_carries_no_kitty_graphics_escapes() {
        let painted = frames_of("$$x^2 + y^2 = z^2$$\n", 1);
        assert!(
            !painted[0].contains("\x1b_G"),
            "a text-only sink must never emit a kitty escape, got {:?}",
            painted[0]
        );
    }

    /// txm cannot render everything, and the rung below it is the TeX source.
    ///
    /// `\begin{tikzpicture}` is the formula this workspace already relies on
    /// txm rejecting (`math`'s own `test_dw_6_2_txm_failure_falls_to_literal_rung`
    /// uses it), so the oracle is a documented rejection rather than a guess
    /// about what txm finds hard.
    #[test]
    fn test_a_formula_txm_rejects_falls_through_to_its_literal_source() {
        let tex = r"\begin{tikzpicture}\draw (0,0);\end{tikzpicture}";
        assert!(
            math::render_text(tex).is_none(),
            "test setup: txm must reject this formula"
        );
        let doc = Rc::new(Document::parse(&format!("$${tex}$$\n")));
        let sink = TextMediaSink::new(Rc::clone(&doc), engine());
        let node = doc
            .nodes()
            .find(|n| matches!(n, ast::NodeRef::Inline(i) if matches!(i.kind, InlineKind::Math { .. })))
            .expect("the document has a math node")
            .id();
        assert_eq!(
            sink.lines_for(node),
            vec![sanitize(tex)],
            "the literal rung is the sanitized TeX source"
        );
    }
}
