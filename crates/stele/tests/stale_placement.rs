//! End-to-end regression for stale media placements: after any frame, the
//! only images the terminal is drawing are the ones that frame placed, at the
//! rows that frame's boxes occupy.
//!
//! The bug this closes was reported from a real Ghostty pane: one Ctrl-d and
//! the previous screen's formulas (`E = mc²`, `e^{iπ}+1=0`, `f(x)=x²`) stayed
//! painted on top of an unrelated heading. The sink deferred eviction by a
//! frame of grace, and `stele` paints only in response to input — so the
//! frame the grace deferred to arrived only when the user next pressed a key.
//!
//! Everything here drives the real `layout` → [`Painter::frame`] → `MediaSink`
//! path over a real document and asserts against a model of the terminal's own
//! graphics state, rebuilt from the emitted bytes. Nothing counts escapes: the
//! archetype that started this bug bash was a green test asserting create /
//! delete *balance* while placements sat at the wrong rows, and a count cannot
//! see a row.

use std::collections::{BTreeMap, BTreeSet};

use layout::{LayoutTree, Line, LineItem, layout};
use stele::media::GfxMediaSink;
use stele::{ImageSizer, Painter, Size};
use width::{WidthConfig, WidthEngine};

const VIEWPORT_W: u16 = 90;
const VIEWPORT_H: u16 = 24;

/// A model of the terminal's graphics state, driven only by the bytes the
/// painter emits.
///
/// Three questions the protocol can answer and a count cannot: which images
/// does the terminal hold, which is it *drawing*, and *where*. The row comes
/// from the `CSI row;colH` that `gfx::Emitter` emits immediately before every
/// put, so it is the terminal's own answer, not the painter's intent.
#[derive(Default, Debug)]
struct TerminalGfx {
    cursor: (u16, u16),
    /// Image ids whose pixel data the terminal is holding.
    stored: BTreeSet<u32>,
    /// Image id -> the 1-based `(row, col)` its live placement sits at.
    visible: BTreeMap<u32, (u16, u16)>,
}

impl TerminalGfx {
    /// Replays one frame's bytes; returns the ids that frame placed.
    fn apply(&mut self, frame: &[u8]) -> BTreeSet<u32> {
        let text = String::from_utf8_lossy(frame).into_owned();
        let bytes = text.as_bytes();
        let mut placed = BTreeSet::new();
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] != 0x1b {
                i += 1;
                continue;
            }
            match bytes.get(i + 1) {
                // CSI: run to the final byte, and remember the cursor if it
                // was a CUP.
                Some(b'[') => {
                    let mut j = i + 2;
                    while j < bytes.len() && !(0x40..=0x7e).contains(&bytes[j]) {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b'H' {
                        let mut params = text[i + 2..j].split(';');
                        let row = params.next().unwrap_or("").parse().unwrap_or(1);
                        let col = params.next().unwrap_or("").parse().unwrap_or(1);
                        self.cursor = (row, col);
                    }
                    i = j.saturating_add(1);
                }
                // APC: kitty graphics command, terminated by ESC \.
                Some(b'_') => {
                    let end = text[i..]
                        .find("\x1b\\")
                        .map(|o| i + o)
                        .unwrap_or(bytes.len());
                    let body = &text[i + 2..end];
                    let head = body.split(';').next().unwrap_or("");
                    let id = head
                        .split(',')
                        .find_map(|kv| kv.strip_prefix("i="))
                        .and_then(|v| v.parse::<u32>().ok());
                    if let Some(id) = id {
                        if head.starts_with("Ga=t") {
                            self.stored.insert(id);
                        } else if head.starts_with("Ga=p") {
                            assert!(
                                self.stored.contains(&id),
                                "placed image {id}, whose pixel data the terminal does not \
                                 have — the put draws nothing"
                            );
                            self.visible.insert(id, self.cursor);
                            placed.insert(id);
                        } else if head.starts_with("Ga=d,d=i") {
                            self.visible.remove(&id);
                        } else if head.starts_with("Ga=d,d=I") {
                            self.visible.remove(&id);
                            self.stored.remove(&id);
                        }
                    }
                    i = end.saturating_add(2);
                }
                _ => i += 2,
            }
        }
        placed
    }

    /// The 1-based rows the terminal is currently drawing an image on.
    fn visible_rows(&self) -> BTreeSet<u16> {
        self.visible.values().map(|&(row, _)| row).collect()
    }
}

struct Harness {
    tree: LayoutTree,
    painter: Painter,
    term: TerminalGfx,
}

impl Harness {
    fn new(doc_name: &str) -> Self {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root.join("testdocs").join(doc_name);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} must be readable: {e}", path.display()));
        let base_dir = path.parent().unwrap().to_path_buf();
        let doc = ast::Document::parse(&src);
        let engine = WidthEngine::new(WidthConfig::default());
        let config = layout::LayoutConfig {
            min_width: 24,
            max_width: VIEWPORT_W,
        };
        let tree = layout(
            &doc,
            VIEWPORT_W,
            &config,
            &engine,
            &ImageSizer::new(&base_dir),
        );
        let mut painter = Painter::new(WidthEngine::new(WidthConfig::default()));
        painter.register_media(Box::new(GfxMediaSink::new(doc, &base_dir)));
        Harness {
            tree,
            painter,
            term: TerminalGfx::default(),
        }
    }

    /// Paints one frame at `scroll` and asserts the two invariants that the
    /// stale-placement bug broke, at that scroll offset:
    ///
    /// 1. **Exactness.** The set of images the terminal is drawing when the
    ///    frame ends is exactly the set the frame placed. An id that survives
    ///    from an earlier frame is a stale image on the user's screen.
    /// 2. **Position.** Every drawn image sits on a row where this viewport
    ///    actually has the first visible row of a reserved box.
    fn frame(&mut self, scroll: usize) {
        let mut buf = Vec::new();
        self.painter
            .frame(
                &self.tree,
                scroll,
                Size {
                    width: VIEWPORT_W,
                    height: VIEWPORT_H,
                },
                &mut buf,
            )
            .expect("paint");
        let placed = self.term.apply(&buf);
        let visible: BTreeSet<u32> = self.term.visible.keys().copied().collect();
        assert_eq!(
            visible, placed,
            "at scroll {scroll} the terminal is drawing {visible:?} but this frame \
             placed only {placed:?} — the difference is painted over the new screen"
        );
        let expected = self.box_rows(scroll);
        assert!(
            self.term.visible_rows().is_subset(&expected),
            "at scroll {scroll} images sit at rows {:?}, but the only rows with the \
             top of a visible media box are {expected:?}",
            self.term.visible_rows()
        );
    }

    /// The 1-based viewport rows at which the *first visible row* of a media
    /// box lands — the only rows a placement may legitimately occupy.
    ///
    /// Two shapes count. A block box (`Line::Reserved`) is painted one row at
    /// a time but placed once, on its first visible row; when its top has
    /// scrolled above the viewport that row is row 1, which is why this
    /// tracks the previous line's node rather than trusting `Reserved::row`.
    /// An inline box (`LineItem::Box`) is always one row tall and is placed
    /// on the row it sits on.
    fn box_rows(&self, scroll: usize) -> BTreeSet<u16> {
        let mut rows = BTreeSet::new();
        let mut previous: Option<ast::NodeId> = None;
        for r in 0..VIEWPORT_H {
            let idx = scroll + r as usize;
            let mut block = None;
            match self.tree.lines(idx..idx + 1).next() {
                Some(Line::Reserved(line)) => block = Some(line.boxed.node_id),
                Some(Line::Items(items)) if items.iter().any(|i| matches!(i, LineItem::Box(_))) => {
                    rows.insert(r + 1);
                }
                _ => {}
            }
            if let Some(node) = block
                && previous != Some(node)
            {
                rows.insert(r + 1);
            }
            previous = block;
        }
        rows
    }

    /// How many reserved boxes start inside the viewport at `scroll`.
    fn box_count(&self, scroll: usize) -> usize {
        self.box_rows(scroll).len()
    }
}

/// The reported bug, swept: every single-Ctrl-d jump through the math
/// document, each treated as its own "the user pressed a key once and
/// stopped" scenario.
#[test]
fn every_ctrl_d_jump_leaves_only_images_that_frame_painted() {
    let mut h = Harness::new("02-math.md");
    let half = (VIEWPORT_H / 2) as usize;
    let mut jumps_with_media = 0;
    for from in (0..h.tree.line_count().saturating_sub(half)).step_by(half) {
        h.frame(from);
        let before: BTreeSet<u32> = h.term.visible.keys().copied().collect();
        h.frame(from + half);
        let after: BTreeSet<u32> = h.term.visible.keys().copied().collect();
        if !before.is_empty() && before != after {
            jumps_with_media += 1;
        }
    }
    assert!(
        jumps_with_media >= 10,
        "this sweep is only evidence if the document actually has media on the \
         screens being jumped between; only {jumps_with_media} jumps changed the \
         visible set"
    );
}

/// The reverse direction. Scrolling *up* is the case that broke an earlier
/// revision of this sink outright (every box's `y` increases, so frames were
/// indistinguishable from one another), so it gets its own sweep rather than
/// being assumed symmetric with scrolling down.
#[test]
fn every_ctrl_u_jump_leaves_only_images_that_frame_painted() {
    let mut h = Harness::new("02-math.md");
    let half = (VIEWPORT_H / 2) as usize;
    let top = h.tree.line_count().saturating_sub(half);
    let mut offsets: Vec<usize> = (0..top).step_by(half).collect();
    offsets.reverse();
    let mut jumps_with_media = 0;
    for pair in offsets.windows(2) {
        h.frame(pair[0]);
        let before: BTreeSet<u32> = h.term.visible.keys().copied().collect();
        h.frame(pair[1]);
        let after: BTreeSet<u32> = h.term.visible.keys().copied().collect();
        if !before.is_empty() && before != after {
            jumps_with_media += 1;
        }
    }
    assert!(
        jumps_with_media >= 10,
        "the up sweep saw only {jumps_with_media} jumps that changed the visible set"
    );
}

/// The screenshot case, exactly: a screen full of formulas, then one Ctrl-d
/// onto a screen with no media at all. Nothing may remain on the glass.
#[test]
fn a_jump_onto_a_screen_with_no_media_clears_every_image() {
    let mut h = Harness::new("02-math.md");
    let half = (VIEWPORT_H / 2) as usize;
    // A pair of adjacent screens, one full of boxes and the next with none —
    // found rather than hard-coded, so it survives an edit to the fixture.
    let pair = (0..h.tree.line_count().saturating_sub(half))
        .step_by(half)
        .find(|&from| h.box_count(from) > 0 && h.box_count(from + half) == 0)
        .expect("02-math.md must contain a media screen followed by a media-free one");

    h.frame(pair);
    assert!(
        !h.term.visible.is_empty(),
        "the screen before the jump must actually be showing images"
    );
    let before: BTreeSet<u32> = h.term.visible.keys().copied().collect();

    h.frame(pair + half);
    assert!(
        h.term.visible.is_empty(),
        "after jumping from a screen showing {before:?} onto one with no media, \
         the terminal is still drawing {:?} over the new text",
        h.term.visible
    );
    // Their pixel data may legitimately still be resident — that is a
    // different lifetime, and the whole point of ending visibility with
    // `a=d,d=i` rather than `a=d,d=I`.
    assert!(
        !h.term.stored.is_empty(),
        "ending visibility must not have thrown away the rasters"
    );
}

/// Position, stated as the thing a user would notice: an image that survives
/// a jump has *moved with its text*.
///
/// Every frame's rows are already checked against the document in
/// [`Harness::frame`] (an image may only sit where the current viewport
/// starts a box). This adds the cross-frame half: for a box on screen before
/// and after a Ctrl-d, the placement must be exactly `half` rows higher — or
/// pinned to row 1, which is where a box whose top has scrolled above the
/// viewport is placed.
///
/// It cannot be written as "the drawn rows equal the reserved rows": some
/// boxes legitimately do not place at all. `02-math.md` contains a formula
/// using physics-package derivative notation that RaTeX's parser rejects on
/// purpose, and it degrades to the txm rung — a reserved row with no image on
/// it, correctly.
#[test]
fn an_image_that_survives_a_jump_moves_up_with_its_text() {
    let mut h = Harness::new("02-math.md");
    let half = (VIEWPORT_H / 2) as usize;
    let mut survivors = 0;
    for from in (0..h.tree.line_count().saturating_sub(half)).step_by(half) {
        h.frame(from);
        let before = h.term.visible.clone();
        h.frame(from + half);
        for (id, &(row, col)) in &h.term.visible {
            let Some(&(was_row, was_col)) = before.get(id) else {
                continue;
            };
            survivors += 1;
            let moved = was_row.saturating_sub(half as u16);
            assert!(
                row == moved || row == 1,
                "image {id} was drawn at row {was_row} before the jump to scroll {}; \
                 after it, it is at row {row} — expected {moved} (or row 1 if its top \
                 scrolled off)",
                from + half
            );
            assert_eq!(
                col, was_col,
                "image {id} changed column across a vertical scroll"
            );
        }
    }
    assert!(
        survivors >= 10,
        "only {survivors} images survived a jump, which is too few to be evidence"
    );
}
