//! Viewport state: scroll position tracking, key-driven navigation, and
//! debounced resize/relayout — all pure and independently testable, so the
//! event loop in `main.rs` stays thin glue over real crossterm I/O.

use ast::Document;
use crossterm::event::KeyCode;
use layout::{IntrinsicSizer, LayoutConfig, LayoutTree, layout};
use width::WidthEngine;

use crate::painter::Size;

/// Everything [`AppState::relayout`] needs to re-derive a [`LayoutTree`]
/// from the retained document, bundled so the method stays under the
/// routine-design parameter guideline (cc-routine-and-class-design).
pub struct LayoutContext<'a> {
    pub doc: &'a Document,
    pub config: &'a LayoutConfig,
    pub engine: &'a WidthEngine,
    pub sizer: &'a dyn IntrinsicSizer,
}

/// The viewer's mutable state: the current layout tree, scroll offset, and
/// viewport size. Navigation and resize are pure state transitions, driven
/// directly from tests without a real terminal or timers.
pub struct AppState {
    tree: LayoutTree,
    scroll: usize,
    size: Size,
}

impl AppState {
    pub fn new(tree: LayoutTree, size: Size) -> Self {
        AppState {
            tree,
            scroll: 0,
            size,
        }
    }

    pub fn tree(&self) -> &LayoutTree {
        &self.tree
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// The largest scroll offset that still shows a full viewport (`0` for
    /// a document shorter than the viewport — never negative, never
    /// panics).
    pub fn max_scroll(&self) -> usize {
        self.tree
            .line_count()
            .saturating_sub(self.size.height.max(1) as usize)
    }

    fn set_scroll(&mut self, value: usize) {
        self.scroll = value.min(self.max_scroll());
    }

    fn scroll_by(&mut self, delta: isize) {
        let current = self.scroll as isize;
        self.set_scroll((current + delta).max(0) as usize);
    }

    fn page_size(&self) -> usize {
        self.size.height.max(1) as usize
    }

    /// Applies one key press. Returns `true` when the key requests quit.
    pub fn handle_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char('q') => return true,
            KeyCode::Up => self.scroll_by(-1),
            KeyCode::Down => self.scroll_by(1),
            KeyCode::PageUp => self.scroll_by(-(self.page_size() as isize)),
            KeyCode::PageDown => self.scroll_by(self.page_size() as isize),
            KeyCode::Home | KeyCode::Char('g') => self.set_scroll(0),
            KeyCode::End | KeyCode::Char('G') => self.set_scroll(self.max_scroll()),
            _ => {}
        }
        false
    }

    /// Re-lays out the retained document at `width`/`new_size`, keeping the
    /// block that was at the top of the viewport at the top afterward
    /// (DW-5.3). Reflow changes line counts and wrap points, so a proportional
    /// scroll ratio drifts; anchoring to the source block via the layout
    /// tree's `line_blocks` map holds the reader's place exactly. Falls back
    /// to a proportional estimate only when there is no block to anchor to
    /// (an empty document).
    pub fn relayout(&mut self, ctx: &LayoutContext, width: u16, new_size: Size) {
        let anchor = self.tree.block_at(self.scroll);
        let old_max = self.max_scroll();
        let ratio = if old_max == 0 {
            0.0
        } else {
            self.scroll as f64 / old_max as f64
        };

        self.tree = layout(ctx.doc, width, ctx.config, ctx.engine, ctx.sizer);
        self.size = new_size;

        let target = anchor
            .and_then(|block| self.tree.first_line_of(block))
            .unwrap_or_else(|| (ratio * self.max_scroll() as f64).round() as usize);
        self.set_scroll(target);
    }

    /// Applies a burst of resize events as the debounced event loop does:
    /// only the final size in the burst drives a real relayout, so a rapid
    /// resize storm still costs exactly one re-layout.
    pub fn apply_resize_burst(&mut self, ctx: &LayoutContext, sizes: &[Size]) {
        if let Some(&last) = sizes.last() {
            self.relayout(ctx, last.width, last);
        }
    }
}

#[cfg(test)]
mod tests {
    use ast::Document;
    use layout::NullSizer;
    use width::WidthConfig;

    use super::*;

    fn build(
        source: &str,
        width: u16,
        height: u16,
    ) -> (Document, LayoutConfig, WidthEngine, AppState) {
        let doc = Document::parse(source);
        let config = LayoutConfig::default();
        let engine = WidthEngine::new(WidthConfig::default());
        let tree = layout(&doc, width, &config, &engine, &NullSizer);
        let size = Size { width, height };
        let state = AppState::new(tree, size);
        (doc, config, engine, state)
    }

    /// 40 short, single-line paragraphs — none of them ever wraps
    /// regardless of width (each is far under the 24-cell floor), so line
    /// count and content-per-line stay identical across every width used
    /// in these tests. This lets the resize-storm tests assert *exact*
    /// scroll-anchor preservation instead of merely "didn't crash".
    fn non_reflowing_source(n: usize) -> String {
        (0..n).map(|i| format!("line {i}\n\n")).collect()
    }

    #[test]
    fn test_dw_5_1_scroll_navigation_and_quit() {
        let (_doc, _config, _engine, mut state) = build(&non_reflowing_source(50), 40, 10);
        assert_eq!(state.scroll(), 0);

        assert!(!state.handle_key(KeyCode::Down));
        assert_eq!(state.scroll(), 1);
        assert!(!state.handle_key(KeyCode::Up));
        assert_eq!(state.scroll(), 0);
        // Up at scroll 0 must clamp, not underflow.
        assert!(!state.handle_key(KeyCode::Up));
        assert_eq!(state.scroll(), 0);

        assert!(!state.handle_key(KeyCode::PageDown));
        assert_eq!(state.scroll(), state.page_size());

        assert!(!state.handle_key(KeyCode::Char('G')));
        assert_eq!(state.scroll(), state.max_scroll());
        assert!(!state.handle_key(KeyCode::End));
        assert_eq!(state.scroll(), state.max_scroll());

        assert!(!state.handle_key(KeyCode::Char('g')));
        assert_eq!(state.scroll(), 0);
        assert!(!state.handle_key(KeyCode::Home));
        assert_eq!(state.scroll(), 0);

        assert!(state.handle_key(KeyCode::Char('q')));
    }

    #[test]
    fn test_max_scroll_is_zero_for_document_shorter_than_viewport() {
        let (_doc, _config, _engine, state) = build("short\n", 40, 100);
        assert_eq!(state.max_scroll(), 0);
        assert_eq!(state.scroll(), 0);
    }

    #[test]
    fn test_dw_5_3_resize_storm_no_crash_and_final_width_correct() {
        let (doc, config, engine, mut state) = build(&non_reflowing_source(200), 80, 20);
        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };

        // 50 simulated resize events in a burst, widths bouncing around,
        // ending at 42.
        let sizes: Vec<Size> = (0..50)
            .map(|i| Size {
                width: 30 + (i % 15) * 4,
                height: 20,
            })
            .chain(std::iter::once(Size {
                width: 42,
                height: 25,
            }))
            .collect();
        state.apply_resize_burst(&ctx, &sizes);

        // No crash (we got here) and the final layout matches a fresh
        // parse+layout at the final width exactly.
        let fresh = layout(&doc, 42, &config, &engine, &NullSizer);
        assert_eq!(state.tree(), &fresh);
        assert_eq!(
            state.size(),
            Size {
                width: 42,
                height: 25
            }
        );
        assert!(state.scroll() <= state.max_scroll());
    }

    #[test]
    fn test_dw_5_3_topmost_visible_block_preserved_across_resize_storm() {
        let (doc, config, engine, mut state) = build(&non_reflowing_source(500), 80, 10);
        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };

        // Scroll partway into the document first.
        for _ in 0..100 {
            state.handle_key(KeyCode::Down);
        }
        let topmost_before = topmost_line_text(&state);

        // A 50-event resize storm; the content never reflows (every line
        // is short), so line count is identical at every width in the
        // burst and the topmost visible line's text must be identical
        // after the storm settles.
        let sizes: Vec<Size> = (0..50)
            .map(|i| Size {
                width: 24 + (i % 20) * 3,
                height: 10,
            })
            .collect();
        state.apply_resize_burst(&ctx, &sizes);

        let topmost_after = topmost_line_text(&state);
        assert_eq!(topmost_before, topmost_after);
    }

    /// The stronger DW-5.3 case: a document whose paragraphs genuinely REFLOW
    /// at different widths, so line counts and wrap points change across the
    /// resize. Proportional scroll drifts here; block anchoring must land the
    /// reader back on the exact same source block. Uses distinctly-numbered
    /// long paragraphs so the topmost block's identity is checkable by text.
    #[test]
    fn test_dw_5_3_reflowing_document_anchors_to_the_same_block() {
        // 60 long paragraphs (~20 words each) that wrap differently at 40 vs
        // 90 cells — the exact case a proportional ratio gets wrong.
        let source: String = (0..60)
            .map(|i| {
                format!(
                    "Paragraph {i:02} begins here and continues with enough words to wrap \
                     across several lines at a narrow width but far fewer at a wide one.\n\n"
                )
            })
            .collect();
        let doc = Document::parse(&source);
        let config = LayoutConfig::default();
        let engine = WidthEngine::new(WidthConfig::default());
        let tree = layout(&doc, 90, &config, &engine, &NullSizer);
        let mut state = AppState::new(
            tree,
            Size {
                width: 90,
                height: 10,
            },
        );
        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };

        // Scroll to somewhere in the middle and note which source block sits
        // at the top of the viewport (by node identity, not by text — the top
        // line may be a wrapped continuation line, not a paragraph start).
        for _ in 0..40 {
            state.handle_key(KeyCode::Down);
        }
        let anchor = state
            .tree()
            .block_at(state.scroll())
            .expect("a scrolled-to line belongs to a block");

        // Resize narrow (heavy reflow) then back wide — the burst ends at 40,
        // a width at which every paragraph wraps to a different line count.
        let sizes: Vec<Size> = [90u16, 30, 55, 40]
            .iter()
            .map(|&w| Size {
                width: w,
                height: 10,
            })
            .collect();
        state.apply_resize_burst(&ctx, &sizes);

        // The same block is now at the top, pinned to its first line — the
        // reader's place is held exactly, which a proportional ratio cannot do
        // once wrap points change.
        assert_eq!(
            state.tree().block_at(state.scroll()),
            Some(anchor),
            "the topmost block must be preserved across a reflowing resize"
        );
        assert_eq!(
            state.scroll(),
            state.tree().first_line_of(anchor).unwrap(),
            "scroll must land on the anchored block's first line"
        );
    }

    fn topmost_line_text(state: &AppState) -> String {
        use layout::{Line, LineItem};
        match state
            .tree()
            .lines(state.scroll()..state.scroll() + 1)
            .next()
        {
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
}
