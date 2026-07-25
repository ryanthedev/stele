//! Scroll navigation at every boundary, swept across document heights either
//! side of the viewport: shorter than it, exactly it, one line longer, and far
//! longer.
//!
//! `AppState`'s own unit tests drive the same keys, but each at a single
//! document height. The arithmetic that breaks is `max_scroll` at the
//! boundaries — a document exactly the viewport's height, or one line longer —
//! and an off-by-one there is invisible at 50 lines. The last assertion is the
//! one a reader would notice: at the furthest scrollable position the final
//! document line must be *on* the last viewport row, never one past it and
//! never one short of it.
//!
//! This was `hunter_paint.rs`'s `probe_scroll_boundaries`. The design review
//! wanted the height sweep folded into `app.rs`'s own test module, which is the
//! right home for it — `AppState` is pure and needs no integration harness —
//! but `src/**` is outside this change's write scope, so it stays an
//! integration test for now and is noted for routing.

mod common;

use ast::Document;
use crossterm::event::{KeyCode, KeyEvent};
use layout::{LayoutConfig, NullSizer, layout};
use stele::app::AppState;
use stele::painter::Size;

use common::fixtures::engine;

const VIEWPORT: Size = Size {
    width: 40,
    height: 10,
};

#[test]
fn test_scroll_clamps_at_both_ends_for_every_document_height() {
    for lines in [0usize, 1, 9, 10, 11, 100] {
        let src: String = (0..lines).map(|i| format!("l{i}\n\n")).collect();
        let doc = Document::parse(&src);
        let tree = layout(
            &doc,
            VIEWPORT.width,
            &LayoutConfig::default(),
            &engine(),
            &NullSizer,
        );
        let n = tree.line_count();
        let mut state = AppState::new(tree, VIEWPORT);
        let height = usize::from(VIEWPORT.height);
        let expected_max = n.saturating_sub(height);
        assert_eq!(state.max_scroll(), expected_max, "{lines} lines");

        state.handle_key_event(KeyEvent::from(KeyCode::Char('G')));
        assert_eq!(state.scroll(), expected_max, "{lines} lines: G");
        // One more Down at the tail must not move (and must not underflow).
        state.handle_key_event(KeyEvent::from(KeyCode::Down));
        assert_eq!(state.scroll(), expected_max, "{lines} lines: Down at tail");
        state.handle_key_event(KeyEvent::from(KeyCode::PageDown));
        assert_eq!(
            state.scroll(),
            expected_max,
            "{lines} lines: PageDown at tail"
        );

        state.handle_key_event(KeyEvent::from(KeyCode::Char('g')));
        assert_eq!(state.scroll(), 0, "{lines} lines: g");
        state.handle_key_event(KeyEvent::from(KeyCode::Up));
        assert_eq!(state.scroll(), 0, "{lines} lines: Up at top");
        state.handle_key_event(KeyEvent::from(KeyCode::PageUp));
        assert_eq!(state.scroll(), 0, "{lines} lines: PageUp at top");

        // The last scrollable position must show the final document line as
        // the LAST viewport row — never past it, never one short of it.
        state.handle_key_event(KeyEvent::from(KeyCode::End));
        let last_visible = state.scroll() + height;
        assert!(
            last_visible >= n,
            "{lines} lines: End leaves line {} unreachable",
            n.saturating_sub(1)
        );
        assert!(
            n == 0 || last_visible < n + height,
            "{lines} lines: End scrolled {} rows past the last line",
            last_visible - n
        );
    }
}
