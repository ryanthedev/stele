//! DW-3.3: the TOC overlay's effect on media placements, asserted end to end
//! through the real `layout` → `AppState` → `Painter` → `GfxMediaSink` path.
//!
//! The overlay is a full-screen repaint that paints no media at all, which is
//! exactly the shape of the bug the sink's frame boundary exists to prevent:
//! before that boundary existed, a frame containing no media never swept
//! anything, so the previous screen's images stayed painted over the new one.
//! An overlay is that case on purpose and on demand, so it gets its own
//! end-to-end check rather than trusting the unit tests' hand-driven frames.
//!
//! Asserted against a model of the terminal's own graphics state rebuilt from
//! the emitted bytes, never against a count of escapes — "one delete per image
//! that scrolled away" was true while the image was still on the screen.

mod common;

use ast::Document;
use layout::{Line, LineItem, layout};
use stele::app::{AppState, FileInfo, Mode};
use stele::media::GfxMediaSink;
use stele::{ImageSizer, Painter, Size};
use width::{WidthConfig, WidthEngine};

use common::fixtures::{scratch_dir, write_png};
use common::termgfx::TerminalGfx;

const VIEWPORT_W: u16 = 80;
const VIEWPORT_H: u16 = 20;

/// A document with headings (so the TOC has something to list) and images
/// (so there is something placed to take down).
struct Harness {
    state: AppState,
    painter: Painter,
    term: TerminalGfx,
}

impl Harness {
    fn new(tag: &str) -> Self {
        let dir = scratch_dir(tag);
        let mut source = String::new();
        for i in 0..6 {
            write_png(&dir, &format!("toc{i}.png"), 64, 64);
            source.push_str(&format!(
                "## Section {i}\n\nbody {i}\n\n![alt{i}](./toc{i}.png)\n\n"
            ));
        }
        let doc = Document::parse(&source);
        let engine = WidthEngine::new(WidthConfig::default());
        let config = layout::LayoutConfig {
            min_width: 24,
            max_width: VIEWPORT_W,
        };
        let sizer = ImageSizer::new(&dir);
        let tree = layout(&doc, VIEWPORT_W, &config, &engine, &sizer);
        let size = Size {
            width: VIEWPORT_W,
            height: VIEWPORT_H,
        };
        let mut painter = Painter::new(WidthEngine::new(WidthConfig::default()));
        painter.register_media(Box::new(GfxMediaSink::new(doc, &dir)));
        Harness {
            state: AppState::new(tree, size, FileInfo::default()),
            painter,
            term: TerminalGfx::default(),
        }
    }

    /// Paints one frame of whichever kind the current mode calls for — the
    /// same choice `main.rs`'s event loop makes — and returns the ids that
    /// frame placed.
    fn frame(&mut self) -> std::collections::BTreeSet<u32> {
        let mut buf = Vec::new();
        let status = self.state.status();
        let size = self.state.size();
        match self.state.mode() {
            // Mirrors `main.rs::paint`, which routes both document-showing
            // modes through the search-aware entry point. With no search
            // active the overlay is empty and this is byte-identical to the
            // `frame_with_status` this used to call.
            Mode::Normal | Mode::Search { .. } => self
                .painter
                .frame_with_search(
                    self.state.tree(),
                    self.state.scroll(),
                    size,
                    &status,
                    self.state.search_overlay(),
                    &mut buf,
                )
                .expect("paint"),
            Mode::Toc { .. } => {
                let rows = self.state.toc_rows(size.height);
                self.painter
                    .frame_overlay(&rows, size, &status, &mut buf)
                    .expect("paint")
            }
        }
        self.term.apply(&buf)
    }

    fn key(&mut self, code: crossterm::event::KeyCode) {
        let quit = self
            .state
            .handle_key_event(crossterm::event::KeyEvent::from(code));
        assert!(!quit, "{code:?} must not quit");
    }

    /// How many media boxes have their first visible row inside the viewport
    /// at the current scroll — the number of placements an honest frame emits.
    fn visible_boxes(&self) -> usize {
        let scroll = self.state.scroll();
        let tree = self.state.tree();
        let mut previous = None;
        let mut count = 0;
        for row in 0..VIEWPORT_H {
            let idx = scroll + usize::from(row);
            let Some(line) = tree.lines(idx..idx + 1).next() else {
                break;
            };
            match line {
                Line::Reserved(reserved) => {
                    if previous != Some(reserved.boxed.node_id) {
                        count += 1;
                    }
                    previous = Some(reserved.boxed.node_id);
                }
                Line::Items(items) => {
                    previous = None;
                    count += items
                        .iter()
                        .filter(|item| matches!(item, LineItem::Box(_)))
                        .count();
                }
            }
        }
        count
    }
}

/// DW-3.3. Three frames, and after each of them the images the terminal is
/// drawing must be exactly the images that frame placed:
///
/// 1. a document frame with media on screen — placements go up;
/// 2. the overlay frame — every one of them must come down, and the overlay
///    must place nothing of its own;
/// 3. the frame after `Esc` — the placements come back, and only the ones
///    this frame actually paints.
#[test]
fn test_dw_3_3_the_toc_frame_takes_every_placement_down_and_the_returning_frame_puts_back_exactly_what_it_paints()
 {
    let mut h = Harness::new("toc-placements");

    // Scroll to a position that genuinely has media on screen.
    for _ in 0..4 {
        h.key(crossterm::event::KeyCode::Down);
    }
    let placed = h.frame();
    let expected = h.visible_boxes();
    assert!(
        expected > 0,
        "the fixture must have media on screen, or this test cannot fail"
    );
    let visible: std::collections::BTreeSet<u32> = h.term.visible.keys().copied().collect();
    assert_eq!(visible, placed);
    assert_eq!(visible.len(), expected);

    // The overlay frame: no media painted, so nothing may be left drawn.
    h.key(crossterm::event::KeyCode::Char('t'));
    assert!(matches!(h.state.mode(), Mode::Toc { .. }));
    let placed = h.frame();
    assert!(
        placed.is_empty(),
        "the overlay paints no media, so it must place none: {placed:?}"
    );
    assert!(
        h.term.visible.is_empty(),
        "no image may survive onto the overlay: {:?}",
        h.term.visible
    );
    assert!(
        !h.term.stored.is_empty(),
        "the rasters must survive the overlay — dropping them would make \
         dismissing the TOC cost a full re-transmit"
    );

    // Back to the document: exactly the boxes this frame paints, no more.
    h.key(crossterm::event::KeyCode::Esc);
    assert_eq!(h.state.mode(), Mode::Normal);
    let placed = h.frame();
    let visible: std::collections::BTreeSet<u32> = h.term.visible.keys().copied().collect();
    assert_eq!(
        visible, placed,
        "the returning frame must leave exactly what it placed"
    );
    assert_eq!(visible.len(), h.visible_boxes());
}

/// Dismissing the TOC costs a re-place, not a re-transmit — the residency
/// half of the same story, and what makes the overlay cheap to open on a
/// document full of large images.
#[test]
fn test_dismissing_the_toc_re_places_the_same_rasters_instead_of_re_transmitting() {
    let mut h = Harness::new("toc-residency");
    for _ in 0..4 {
        h.key(crossterm::event::KeyCode::Down);
    }
    h.frame();
    let stored_before = h.term.stored.clone();
    assert!(!stored_before.is_empty());

    h.key(crossterm::event::KeyCode::Char('t'));
    h.frame();
    h.key(crossterm::event::KeyCode::Esc);
    let placed = h.frame();

    assert_eq!(
        h.term.stored, stored_before,
        "no raster may be freed or added by a round trip through the overlay"
    );
    assert!(
        placed.iter().all(|id| stored_before.contains(id)),
        "every image the returning frame placed must be one the terminal was \
         already holding: placed {placed:?}, held {stored_before:?}"
    );
}

/// Jumping from the TOC lands the reader on the chosen heading, and the frame
/// that draws it places exactly its own media — the two halves of this phase
/// meeting.
#[test]
fn test_a_toc_jump_repaints_at_the_new_position_with_only_that_screens_media() {
    let mut h = Harness::new("toc-jump");
    h.frame();
    h.key(crossterm::event::KeyCode::Char('t'));
    h.frame();
    for _ in 0..3 {
        h.key(crossterm::event::KeyCode::Char('j'));
    }
    h.key(crossterm::event::KeyCode::Enter);
    assert_eq!(h.state.mode(), Mode::Normal);

    let expected_line = h
        .state
        .outline()
        .line_of(3)
        .expect("the fourth heading has a line");
    assert_eq!(h.state.scroll(), expected_line.min(h.state.max_scroll()));

    let placed = h.frame();
    let visible: std::collections::BTreeSet<u32> = h.term.visible.keys().copied().collect();
    assert_eq!(visible, placed);
    assert_eq!(visible.len(), h.visible_boxes());
}
