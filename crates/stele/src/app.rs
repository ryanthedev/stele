//! Viewport state: scroll position tracking, key-driven navigation, and
//! debounced resize/relayout — all pure and independently testable, so the
//! event loop in `main.rs` stays thin glue over real crossterm I/O.

use ast::{Document, NodeId};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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

/// Where the reader is, expressed so it survives a relayout: the top-level
/// source block at the top of the viewport, plus *how far into that block*
/// the viewport top sits.
///
/// The offset is the part block identity alone cannot carry. A reader on line
/// 200 of a 400-line code fence and a reader on its first line have the same
/// block; re-anchoring on the block alone puts both on the fence's line 0.
#[derive(Debug, Clone, Copy)]
struct Anchor {
    block: NodeId,
    /// Lines of `block` scrolled off above the viewport top.
    offset: usize,
    /// Lines `block` occupied in the tree `offset` was measured in. Only
    /// meaningful paired with `offset`: together they are a *fraction* of the
    /// block, and a fraction is what stays comparable when reflow changes how
    /// many lines the same source text takes.
    span: usize,
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

    /// One viewport's worth of lines: the step for `PgUp`/`PgDn` and for
    /// vim's `Ctrl-f`/`Ctrl-b`.
    fn page_size(&self) -> usize {
        self.size.height.max(1) as usize
    }

    /// Half a viewport: the step for vim's `Ctrl-d`/`Ctrl-u`. Floored at one
    /// line so a one- or two-row viewport still moves rather than silently
    /// swallowing the key.
    fn half_page(&self) -> usize {
        (self.page_size() / 2).max(1)
    }

    /// Applies one key press *with its modifiers*. Returns `true` when the
    /// key requests quit. This is the event loop's entry point.
    ///
    /// Control chords are tried first and **fall through** to the unmodified
    /// table when the chord means nothing to us, so every pre-existing
    /// binding keeps behaving exactly as it did when the event loop passed
    /// `key.code` and dropped the modifiers on the floor: `Ctrl-Down` still
    /// scrolls down, `Ctrl-q` still quits, `Ctrl-G` still jumps to the end.
    pub fn handle_key_event(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && let Some(quit) = self.handle_control_chord(key.code)
        {
            return quit;
        }
        self.handle_key(key.code)
    }

    /// The Control-chord bindings. `Some(quit)` when the chord is one of
    /// ours, `None` when it means nothing — which is what lets
    /// [`AppState::handle_key_event`] fall through to the unmodified binding
    /// for the same key code.
    fn handle_control_chord(&mut self, code: KeyCode) -> Option<bool> {
        match code {
            // Ctrl-C quits, exactly like `q`. Raw mode clears `ISIG`, so a
            // Ctrl-C keystroke never becomes a `SIGINT` — if key handling
            // ignores it, nothing else in the process can see it and the
            // viewer looks wedged to anyone who did not read the help. The
            // `SIGINT` handler in `terminal::signals` covers the *other*
            // path, an externally delivered `kill -INT`; both end in the
            // same terminal restore.
            KeyCode::Char('c') => return Some(true),
            KeyCode::Char('d') => self.scroll_by(self.half_page() as isize),
            KeyCode::Char('u') => self.scroll_by(-(self.half_page() as isize)),
            KeyCode::Char('f') => self.scroll_by(self.page_size() as isize),
            KeyCode::Char('b') => self.scroll_by(-(self.page_size() as isize)),
            _ => return None,
        }
        Some(false)
    }

    /// The unmodified half of the key table.
    ///
    /// Deliberately **private**: a `KeyCode` on its own cannot express a
    /// chord — `Ctrl-C` and a bare `c` are the same value here — so any caller
    /// reaching for this name by reflex would silently drop Ctrl-C and the
    /// vim `Ctrl-d`/`u`/`f`/`b` motions. [`AppState::handle_key_event`] is the
    /// only way in, for the event loop and for tests alike, so there is
    /// exactly one path a key can take through this type.
    fn handle_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char('q') => return true,
            KeyCode::Up | KeyCode::Char('k') => self.scroll_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_by(1),
            KeyCode::PageUp => self.scroll_by(-(self.page_size() as isize)),
            KeyCode::PageDown => self.scroll_by(self.page_size() as isize),
            KeyCode::Home | KeyCode::Char('g') => self.set_scroll(0),
            KeyCode::End | KeyCode::Char('G') => self.set_scroll(self.max_scroll()),
            _ => {}
        }
        false
    }

    /// Re-lays out the retained document at `width`/`new_size`, keeping the
    /// reader where they were (DW-5.3). Reflow changes line counts and wrap
    /// points, so a whole-document proportional scroll ratio drifts; anchoring
    /// to the source block via the layout tree's `line_blocks` map, *and* to
    /// the reader's offset inside that block, holds their place. Falls back to
    /// a proportional estimate only when there is no block to anchor to (an
    /// empty document, or a block that emitted no lines at the new width).
    ///
    /// Three cases, in the order they are decided:
    ///
    /// 1. Nothing reflowed — the exact scroll line is kept, untouched. See
    ///    `no_reflow_occurred` for how that is decided.
    /// 2. Something reflowed but not this block: a code fence is clipped, not
    ///    wrapped, so its line count is width-independent, and the same holds
    ///    for any block whose wrap points did not move. `span` is unchanged,
    ///    so the reader's offset into the block is carried across *exactly* —
    ///    a reader 200 lines into a 400-line fence stays on line 200 of it.
    /// 3. This block itself rewrapped — a long paragraph, a table, a mermaid
    ///    grid. The offset is rescaled by the block's new line count, because
    ///    the old line index no longer names the same text but the *fraction*
    ///    of the way through the block still does.
    pub fn relayout(&mut self, ctx: &LayoutContext, width: u16, new_size: Size) {
        let anchor = self.anchor();
        let old_max = self.max_scroll();
        let ratio = if old_max == 0 {
            0.0
        } else {
            self.scroll as f64 / old_max as f64
        };
        let previous_scroll = self.scroll;
        let previous_width = self.tree.width();

        self.tree = layout(ctx.doc, width, ctx.config, ctx.engine, ctx.sizer);
        self.size = new_size;

        let target = if self.no_reflow_occurred(previous_width) {
            previous_scroll
        } else {
            anchor
                .and_then(|anchor| self.line_of(anchor))
                .unwrap_or_else(|| (ratio * self.max_scroll() as f64).round() as usize)
        };
        self.set_scroll(target);
    }

    /// The reader's current position as an [`Anchor`], or `None` when the top
    /// line belongs to no block (an empty document).
    fn anchor(&self) -> Option<Anchor> {
        let block = self.tree.block_at(self.scroll)?;
        let first = self.tree.first_line_of(block)?;
        Some(Anchor {
            block,
            offset: self.scroll.saturating_sub(first),
            span: self.block_span(block, first),
        })
    }

    /// How many consecutive lines from `first` onward belong to `block`.
    ///
    /// A top-level block's lines are contiguous in `line_blocks`: `walk_blocks`
    /// sets `current_block` once per block and every line emitted until the
    /// next block is tagged with it, so counting forward from the block's first
    /// line measures the whole block. Never zero — `first` is one of the
    /// block's lines by construction, which is what makes `span - 1` below a
    /// safe clamp.
    fn block_span(&self, block: NodeId, first: usize) -> usize {
        let mut span = 0;
        while self.tree.block_at(first + span) == Some(block) {
            span += 1;
        }
        span.max(1)
    }

    /// Where `anchor` lands in the tree that is installed *now*: the block's
    /// new first line, plus the same distance into it.
    ///
    /// The distance is carried verbatim when the block still occupies the same
    /// number of lines, and rescaled by the new line count when it does not.
    /// Rescaling is not a heuristic for a uniformly-wrapped block: text that
    /// sat 2/3 of the way through 300 wrapped lines sits 2/3 of the way through
    /// the 400 the same words take at a narrower width.
    fn line_of(&self, anchor: Anchor) -> Option<usize> {
        let first = self.tree.first_line_of(anchor.block)?;
        let span = self.block_span(anchor.block, first);
        let offset = if span == anchor.span {
            anchor.offset
        } else {
            (anchor.offset as f64 * span as f64 / anchor.span as f64).round() as usize
        };
        Some(first + offset.min(span - 1))
    }

    /// Whether the just-installed tree is identical to the one it replaced.
    ///
    /// `layout` is pure and deterministic in `(doc, width, config, engine,
    /// sizer)` and clamps `width` into the config's range *before* laying
    /// out, storing the clamped value on the tree. The document, config,
    /// engine and sizer are fixed for the session, so equal clamped widths
    /// imply identical trees — which is why comparing one `u16` is a sound
    /// stand-in for comparing the whole tree, and why a resize between two
    /// widths that both clamp to the same value (say 10 and 15 against a
    /// 24-cell floor) correctly counts as "no reflow" too.
    fn no_reflow_occurred(&self, previous_width: u16) -> bool {
        self.tree.width() == previous_width
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

    /// A Control chord, as the terminal delivers it in raw mode: raw mode
    /// clears `ISIG`, so Ctrl-C reaches us as `Char('c')` + `CONTROL` and
    /// never as a signal.
    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    /// The same key code with no modifiers held.
    fn plain(code: KeyCode) -> KeyEvent {
        KeyEvent::from(code)
    }

    /// A document laid out to *exactly* the viewport height: `max_scroll` is
    /// 0, so every downward motion must be a no-op even though there is a
    /// full screen of content.
    fn exactly_one_viewport(n: usize) -> AppState {
        let source = non_reflowing_source(n);
        let (_doc, _config, _engine, probe) = build(&source, 40, 1);
        let lines = probe.tree().line_count();
        let (_doc, _config, _engine, state) = build(
            &source,
            40,
            u16::try_from(lines).expect("test doc fits a u16"),
        );
        assert_eq!(state.max_scroll(), 0, "doc height must equal the viewport");
        state
    }

    #[test]
    fn test_dw_5_1_scroll_navigation_and_quit() {
        let (_doc, _config, _engine, mut state) = build(&non_reflowing_source(50), 40, 10);
        assert_eq!(state.scroll(), 0);

        assert!(!state.handle_key_event(plain(KeyCode::Down)));
        assert_eq!(state.scroll(), 1);
        assert!(!state.handle_key_event(plain(KeyCode::Up)));
        assert_eq!(state.scroll(), 0);
        // Up at scroll 0 must clamp, not underflow.
        assert!(!state.handle_key_event(plain(KeyCode::Up)));
        assert_eq!(state.scroll(), 0);

        assert!(!state.handle_key_event(plain(KeyCode::PageDown)));
        assert_eq!(state.scroll(), state.page_size());

        assert!(!state.handle_key_event(plain(KeyCode::Char('G'))));
        assert_eq!(state.scroll(), state.max_scroll());
        assert!(!state.handle_key_event(plain(KeyCode::End)));
        assert_eq!(state.scroll(), state.max_scroll());

        assert!(!state.handle_key_event(plain(KeyCode::Char('g'))));
        assert_eq!(state.scroll(), 0);
        assert!(!state.handle_key_event(plain(KeyCode::Home)));
        assert_eq!(state.scroll(), 0);

        assert!(state.handle_key_event(plain(KeyCode::Char('q'))));
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
            state.handle_key_event(plain(KeyCode::Down));
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
            state.handle_key_event(plain(KeyCode::Down));
        }
        let anchor = state
            .tree()
            .block_at(state.scroll())
            .expect("a scrolled-to line belongs to a block");
        let words_before = words_scrolled_past(&state);

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

        // The same block is now at the top, which a proportional whole-document
        // ratio cannot do once wrap points change.
        assert_eq!(
            state.tree().block_at(state.scroll()),
            Some(anchor),
            "the topmost block must be preserved across a reflowing resize"
        );
        // ...and the reader is still at the same place *within* it. This
        // assertion used to read `scroll() == first_line_of(anchor)`, i.e. it
        // required the reader be thrown to the block's first line — the exact
        // behaviour wave-4 §2.3 flagged, asserted as if it were the goal.
        let slack = topmost_line_text(&state).split_whitespace().count().max(1);
        let words_after = words_scrolled_past(&state);
        assert!(
            words_after.abs_diff(words_before) <= slack,
            "the reader had {words_before} words of this block above them and now has \
             {words_after}, more than one wrapped line ({slack} words) away"
        );
    }

    /// DW-5.3's real target, and the case block-granular anchoring loses: a
    /// *reflowing* resize while the reader is deep inside one long block.
    ///
    /// A fenced code block is clipped, never wrapped (`Ctx::literal_block`), so
    /// its 400 lines are 400 lines at every width — the reader's line inside it
    /// means the same thing before and after, and the only correct answer is
    /// the line they were on. Anchoring on the block alone answers "the fence's
    /// first line" and moves them 200 lines.
    ///
    /// Asserted from the *text at the top of the viewport*, not from an index:
    /// a line number that happens to match proves nothing when the whole tree
    /// moved.
    #[test]
    fn test_dw_5_3_reflowing_resize_keeps_the_readers_line_inside_a_clipped_block() {
        // The intro paragraph rewraps between 80 and 30 cells, so the fence's
        // own first line MOVES across the resize. Without it, block-granular
        // anchoring and offset anchoring could return the same number by
        // accident and this test could not tell them apart.
        let mut source = String::from(
            "Intro paragraph carrying enough words to occupy a different number of \
             wrapped lines at eighty cells than it does at thirty.\n\n```\n",
        );
        for i in 0..400 {
            source.push_str(&format!("code line {i:03}\n"));
        }
        source.push_str("```\n");

        let (doc, config, engine, mut state) = build(&source, 80, 20);
        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };

        // Park the reader on a line that names itself.
        let target = (0..state.tree().line_count())
            .find(|&i| line_text(&state, i).contains("code line 200"))
            .expect("the fence line is in the tree");
        while state.scroll() < target {
            state.handle_key_event(plain(KeyCode::Down));
        }
        assert_eq!(state.scroll(), target, "the target line must be reachable");
        let fence = state.tree().block_at(target).expect("the fence is a block");
        let fence_first_before = state.tree().first_line_of(fence).unwrap();

        state.apply_resize_burst(
            &ctx,
            &[Size {
                width: 30,
                height: 20,
            }],
        );

        assert_ne!(
            state.tree().first_line_of(fence).unwrap(),
            fence_first_before,
            "the fixture is broken: the fence must start at a different line \
             after the resize, or this test cannot distinguish block anchoring \
             from offset anchoring"
        );
        assert_eq!(
            line_text(&state, state.scroll()).trim(),
            "code line 200",
            "the reader must still be looking at the line they were on"
        );
    }

    /// The same guarantee for a block that genuinely REWRAPS: one enormous
    /// paragraph. Its line count changes with width, so the reader's old line
    /// index no longer names the same text — but the words above them do not
    /// change, and that is what has to be preserved.
    #[test]
    fn test_dw_5_3_reflowing_resize_keeps_the_readers_place_inside_a_rewrapping_block() {
        // One paragraph, 3000 distinctly-numbered words: a single source block
        // that wraps to ~170 lines at 90 cells and ~500 at 30.
        let source: String = (0..3000).map(|i| format!("w{i:04} ")).collect();
        let (doc, config, engine, mut state) = build(&source, 90, 20);
        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };
        assert_eq!(
            state.tree().block_at(0),
            state.tree().block_at(100),
            "the fixture must be one block"
        );

        for _ in 0..100 {
            state.handle_key_event(plain(KeyCode::Down));
        }
        let words_before = words_scrolled_past(&state);
        assert!(
            words_before > 1000,
            "the reader must be deep inside the block, not near its start"
        );

        state.apply_resize_burst(
            &ctx,
            &[Size {
                width: 30,
                height: 20,
            }],
        );

        // One wrapped line's worth of words at the NEW width is the whole
        // budget: rescaling by the block's new line count is exact for
        // uniformly-wrapped text up to the rounding of one line. Block-granular
        // anchoring scores 0 words here against ~1800.
        let slack = topmost_line_text(&state).split_whitespace().count().max(1);
        let words_after = words_scrolled_past(&state);
        assert!(
            words_after.abs_diff(words_before) <= slack,
            "the reader had {words_before} words above them and now has {words_after}, \
             more than one wrapped line ({slack} words) away"
        );
    }

    /// Regression: a resize that reflows nothing must not move the reader at
    /// all. Block anchoring alone pins to the block's FIRST line, so a reader
    /// 200 lines deep into a single 400-line code fence was thrown back to
    /// line 0 by nothing more than dragging the window one row taller. The
    /// existing DW-5.3 tests could not see it: they use one-line paragraphs,
    /// where the block's first line IS the reader's line.
    #[test]
    fn test_height_only_resize_does_not_move_the_reader_inside_a_long_block() {
        let mut source = String::from("```\n");
        for i in 0..400 {
            source.push_str(&format!("code line {i}\n"));
        }
        source.push_str("```\n");

        let (doc, config, engine, mut state) = build(&source, 80, 20);
        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };
        // The whole fence is one source block, so every visible line anchors
        // to the same block — the case block-granularity anchoring loses.
        assert_eq!(state.tree().block_at(0), state.tree().block_at(200));

        for _ in 0..200 {
            state.handle_key_event(plain(KeyCode::Down));
        }
        assert_eq!(state.scroll(), 200);

        state.apply_resize_burst(
            &ctx,
            &[Size {
                width: 80,
                height: 21,
            }],
        );
        assert_eq!(
            state.scroll(),
            200,
            "a resize at the same width reflows nothing and must not scroll"
        );
    }

    /// The same guarantee across a burst of *width* changes that all clamp to
    /// the same laid-out width: 10 and 15 both clamp up to the 24-cell floor,
    /// so no line ever rewraps and the reader must not move either.
    #[test]
    fn test_width_changes_that_clamp_to_the_same_layout_width_do_not_scroll() {
        let mut source = String::from("```\n");
        for i in 0..200 {
            source.push_str(&format!("c{i}\n"));
        }
        source.push_str("```\n");

        let (doc, config, engine, mut state) = build(&source, 10, 8);
        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };
        assert_eq!(
            state.tree().width(),
            24,
            "clamped up to the min-width floor"
        );

        for _ in 0..60 {
            state.handle_key_event(plain(KeyCode::Down));
        }
        let before = state.scroll();
        assert_eq!(before, 60);

        state.apply_resize_burst(
            &ctx,
            &[
                Size {
                    width: 12,
                    height: 8,
                },
                Size {
                    width: 15,
                    height: 8,
                },
            ],
        );
        assert_eq!(state.tree().width(), 24);
        assert_eq!(state.scroll(), before);
    }

    // ---- Ctrl-C, and the vim motions (all strictly additive) -------------

    /// Ctrl-C quits, exactly like `q`. Raw mode clears `ISIG`, so this key
    /// never becomes a `SIGINT`: if `handle_key_event` does not see the
    /// modifier, nothing downstream can, and the viewer cannot be quit with
    /// the one chord every terminal user tries first.
    #[test]
    fn test_ctrl_c_quits_exactly_like_q() {
        let (_doc, _config, _engine, mut state) = build(&non_reflowing_source(50), 40, 10);
        state.handle_key_event(plain(KeyCode::Down));
        assert_eq!(state.scroll(), 1);

        assert!(
            state.handle_key_event(ctrl('c')),
            "Ctrl-C must request quit"
        );
        // Quitting is all it does — it must not move the reader on the way out.
        assert_eq!(state.scroll(), 1);

        // A bare `c` is not a quit key and never was.
        assert!(!state.handle_key_event(plain(KeyCode::Char('c'))));
        assert_eq!(state.scroll(), 1);
    }

    /// `j`/`k` are `Down`/`Up`: one line, clamped at 0 and at the tail.
    #[test]
    fn test_vim_j_and_k_move_one_line_and_clamp_at_both_ends() {
        let (_doc, _config, _engine, mut state) = build(&non_reflowing_source(50), 40, 10);
        let tail = state.max_scroll();
        assert!(tail > 3, "the fixture must be taller than the viewport");

        assert!(!state.handle_key_event(plain(KeyCode::Char('j'))));
        assert_eq!(state.scroll(), 1);
        assert!(!state.handle_key_event(plain(KeyCode::Char('j'))));
        assert_eq!(state.scroll(), 2);
        assert!(!state.handle_key_event(plain(KeyCode::Char('k'))));
        assert_eq!(state.scroll(), 1);

        // At scroll 0, `k` clamps instead of underflowing.
        state.handle_key_event(plain(KeyCode::Char('k')));
        assert_eq!(state.scroll(), 0);
        state.handle_key_event(plain(KeyCode::Char('k')));
        assert_eq!(state.scroll(), 0);

        // At the tail, `j` clamps instead of running off the end.
        state.handle_key_event(plain(KeyCode::Char('G')));
        assert_eq!(state.scroll(), tail);
        state.handle_key_event(plain(KeyCode::Char('j')));
        assert_eq!(state.scroll(), tail);
        state.handle_key_event(plain(KeyCode::Char('k')));
        assert_eq!(state.scroll(), tail - 1);
    }

    /// `Ctrl-d`/`Ctrl-u` move half a viewport — five lines on a ten-row
    /// screen — and clamp at both ends.
    #[test]
    fn test_vim_ctrl_d_and_ctrl_u_move_half_a_viewport_and_clamp() {
        let (_doc, _config, _engine, mut state) = build(&non_reflowing_source(50), 40, 10);
        let tail = state.max_scroll();
        assert!(tail > 10, "the fixture must be several pages tall");

        assert!(!state.handle_key_event(ctrl('d')));
        assert_eq!(state.scroll(), 5);
        assert!(!state.handle_key_event(ctrl('d')));
        assert_eq!(state.scroll(), 10);
        assert!(!state.handle_key_event(ctrl('u')));
        assert_eq!(state.scroll(), 5);

        // At scroll 0, Ctrl-u clamps.
        state.handle_key_event(ctrl('u'));
        assert_eq!(state.scroll(), 0);
        state.handle_key_event(ctrl('u'));
        assert_eq!(state.scroll(), 0);

        // At the tail, Ctrl-d clamps; Ctrl-u still steps back exactly half.
        state.handle_key_event(plain(KeyCode::End));
        assert_eq!(state.scroll(), tail);
        state.handle_key_event(ctrl('d'));
        assert_eq!(state.scroll(), tail);
        state.handle_key_event(ctrl('u'));
        assert_eq!(state.scroll(), tail - 5);
    }

    /// `Ctrl-f`/`Ctrl-b` move a full viewport — the same step `PgDn`/`PgUp`
    /// already used — and clamp at both ends.
    #[test]
    fn test_vim_ctrl_f_and_ctrl_b_move_a_full_viewport_and_clamp() {
        let (_doc, _config, _engine, mut state) = build(&non_reflowing_source(50), 40, 10);
        let tail = state.max_scroll();
        assert!(tail > 20, "the fixture must be several pages tall");

        assert!(!state.handle_key_event(ctrl('f')));
        assert_eq!(state.scroll(), 10);
        assert!(!state.handle_key_event(ctrl('f')));
        assert_eq!(state.scroll(), 20);
        assert!(!state.handle_key_event(ctrl('b')));
        assert_eq!(state.scroll(), 10);

        // Ctrl-f lands where PgDn lands, from the same start.
        state.handle_key_event(plain(KeyCode::Char('g')));
        state.handle_key_event(plain(KeyCode::PageDown));
        let paged = state.scroll();
        state.handle_key_event(plain(KeyCode::Char('g')));
        state.handle_key_event(ctrl('f'));
        assert_eq!(state.scroll(), paged);

        // At scroll 0, Ctrl-b clamps.
        state.handle_key_event(plain(KeyCode::Home));
        state.handle_key_event(ctrl('b'));
        assert_eq!(state.scroll(), 0);

        // At the tail, Ctrl-f clamps; Ctrl-b steps back exactly one page.
        state.handle_key_event(plain(KeyCode::Char('G')));
        assert_eq!(state.scroll(), tail);
        state.handle_key_event(ctrl('f'));
        assert_eq!(state.scroll(), tail);
        state.handle_key_event(ctrl('b'));
        assert_eq!(state.scroll(), tail - 10);
    }

    /// A document shorter than the viewport has nowhere to scroll: every new
    /// motion must leave the reader at line 0 rather than clamping to some
    /// negative-turned-huge offset.
    #[test]
    fn test_new_motions_are_no_ops_on_a_document_shorter_than_the_viewport() {
        let (_doc, _config, _engine, mut state) = build("short\n", 40, 100);
        assert_eq!(state.max_scroll(), 0);
        for key in [ctrl('d'), ctrl('u'), ctrl('f'), ctrl('b')] {
            assert!(!state.handle_key_event(key));
            assert_eq!(state.scroll(), 0, "{key:?} moved a one-screen document");
        }
        for key in [KeyCode::Char('j'), KeyCode::Char('k')] {
            assert!(!state.handle_key_event(plain(key)));
            assert_eq!(state.scroll(), 0, "{key:?} moved a one-screen document");
        }
    }

    /// The exactly-one-viewport boundary: a full screen of content with
    /// nothing below it. `max_scroll` is 0, so the same must hold.
    #[test]
    fn test_new_motions_are_no_ops_on_a_document_exactly_one_viewport_tall() {
        let mut state = exactly_one_viewport(6);
        for key in [ctrl('d'), ctrl('u'), ctrl('f'), ctrl('b')] {
            assert!(!state.handle_key_event(key));
            assert_eq!(state.scroll(), 0, "{key:?} scrolled past the document tail");
        }
        for key in [KeyCode::Char('j'), KeyCode::Char('k')] {
            assert!(!state.handle_key_event(plain(key)));
            assert_eq!(state.scroll(), 0, "{key:?} scrolled past the document tail");
        }
    }

    /// A one-row viewport would make `height / 2` zero. Ctrl-d/Ctrl-u must
    /// still move a line rather than silently swallowing the key.
    #[test]
    fn test_half_page_moves_at_least_one_line_on_a_one_row_viewport() {
        let (_doc, _config, _engine, mut state) = build(&non_reflowing_source(20), 40, 1);
        assert_eq!(state.page_size(), 1);
        assert!(state.max_scroll() > 2);

        state.handle_key_event(ctrl('d'));
        assert_eq!(state.scroll(), 1);
        state.handle_key_event(ctrl('d'));
        assert_eq!(state.scroll(), 2);
        state.handle_key_event(ctrl('u'));
        assert_eq!(state.scroll(), 1);
    }

    /// Strictly additive: holding Control over a key that is *not* one of the
    /// new chords must still do what that key did before, because the event
    /// loop used to pass `key.code` and drop the modifier entirely.
    #[test]
    fn test_control_falls_through_to_the_pre_existing_binding() {
        let (_doc, _config, _engine, mut state) = build(&non_reflowing_source(50), 40, 10);
        let tail = state.max_scroll();

        assert!(!state.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL)));
        assert_eq!(state.scroll(), 1, "Ctrl-Down must still scroll down");

        assert!(!state.handle_key_event(ctrl('G')));
        assert_eq!(state.scroll(), tail, "Ctrl-G must still jump to the end");

        assert!(!state.handle_key_event(ctrl('g')));
        assert_eq!(state.scroll(), 0, "Ctrl-g must still jump to the top");

        assert!(state.handle_key_event(ctrl('q')), "Ctrl-q must still quit");
    }

    /// The unmodified letters behind the new chords stay unbound: `d`, `u`,
    /// `f`, `b` must not become motions of their own, and `g`/`G`/`q` must
    /// keep the meaning they already had.
    #[test]
    fn test_unmodified_chord_letters_remain_unbound() {
        let (_doc, _config, _engine, mut state) = build(&non_reflowing_source(50), 40, 10);
        state.handle_key_event(plain(KeyCode::PageDown));
        let start = state.scroll();
        assert_eq!(start, 10);

        for c in ['d', 'u', 'f', 'b'] {
            assert!(!state.handle_key_event(plain(KeyCode::Char(c))));
            assert_eq!(state.scroll(), start, "bare `{c}` must not be a motion");
        }
    }

    fn topmost_line_text(state: &AppState) -> String {
        line_text(state, state.scroll())
    }

    /// The painted text of one tree line: its runs concatenated. A reserved
    /// media box contributes nothing — it carries no text.
    fn line_text(state: &AppState, index: usize) -> String {
        use layout::{Line, LineItem};
        match state.tree().lines(index..index + 1).next() {
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

    /// How many words of the topmost visible block have scrolled off above the
    /// viewport top.
    ///
    /// This is the reader's position in the document's *content*, and it is
    /// what a reflowing resize has to preserve. A line index cannot express it:
    /// rewrapping changes how many lines the same words occupy, so line 100
    /// before a resize and line 100 after it are different text. A word count
    /// is invariant under rewrapping, so before and after are comparable.
    fn words_scrolled_past(state: &AppState) -> usize {
        let Some(block) = state.tree().block_at(state.scroll()) else {
            return 0;
        };
        let first = state
            .tree()
            .first_line_of(block)
            .expect("a block found by block_at has a first line");
        (first..state.scroll())
            .map(|i| line_text(state, i).split_whitespace().count())
            .sum()
    }
}
