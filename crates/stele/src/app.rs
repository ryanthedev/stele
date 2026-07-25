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

/// Static per-session facts about the open document: its display name, and
/// the raw file's byte size and line count as loaded — *before*
/// frontmatter/mermaid preprocessing, since that is what a reader means by
/// "this file" when they ask [`AppState::show_file_info`]. `line_count`
/// uses [`str::lines`], which is invariant to a trailing newline's presence
/// (a file ending `"a\nb"` and one ending `"a\nb\n"` both count 2), rather
/// than counting `\n` bytes, which would differ by one between them.
#[derive(Debug, Clone, Default)]
pub struct FileInfo {
    pub name: String,
    pub byte_size: u64,
    pub line_count: usize,
}

/// A transient status-row message with a frame-count time-to-live, set via
/// [`AppState::set_status`]. The TTL itself is [`AppState`]'s concern (it
/// owns the frame counter); this type only carries the text.
#[derive(Debug, Clone)]
pub struct StatusMessage {
    text: String,
}

impl StatusMessage {
    pub fn new(text: impl Into<String>) -> Self {
        StatusMessage { text: text.into() }
    }
}

/// How many [`AppState::status`] calls (i.e. frames) a [`StatusMessage`]
/// stays visible for before reverting to the permanent ruler. Not
/// wall-clock time — this app repaints on events, not a timer — so the
/// budget is expressed in the unit that is actually meaningful here.
const STATUS_MESSAGE_TTL_FRAMES: u32 = 100;

/// One line of terminal-facing chrome: painted by [`crate::painter::Painter`]
/// into the row it reserves. Either the permanent ruler (document name +
/// scroll position) or a transient message that temporarily replaces it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusLine {
    /// Scroll position as a percentage: 0 at the top, 100 at `max_scroll` —
    /// including when `max_scroll` is 0, since a document that fits the
    /// whole viewport has already shown the reader all of it.
    pub position_pct: u8,
    /// The open document's display name.
    pub name: String,
    /// A transient message set by [`AppState::set_status`] (e.g. `Ctrl-G`'s
    /// file info), overriding the ruler until its frame budget runs out.
    pub message: Option<String>,
}

impl StatusLine {
    /// The exact text to paint into the reserved row.
    pub(crate) fn render(&self) -> String {
        match &self.message {
            Some(text) => text.clone(),
            None if self.name.is_empty() => format!("{}%", self.position_pct),
            None => format!("{} — {}%", self.name, self.position_pct),
        }
    }
}

/// The viewer's mutable state: the current layout tree, scroll offset, and
/// viewport size. Navigation and resize are pure state transitions, driven
/// directly from tests without a real terminal or timers.
pub struct AppState {
    tree: LayoutTree,
    scroll: usize,
    size: Size,
    /// The layout width `+`/`-` adjust, independent of the terminal's own
    /// width — see [`AppState::relayout_preserving_anchor`]. Resynced to
    /// `tree.width()` at the end of every [`AppState::relayout`], so a real
    /// terminal resize always wins over a stale toggle.
    content_width: u16,
    file_info: FileInfo,
    /// `(text, frames remaining)`. `None` once the TTL has been exhausted or
    /// no message has been set.
    status_message: Option<(String, u32)>,
}

impl AppState {
    /// `WIDTH_STEP` cells per `+`/`-` press — coarse enough that a handful
    /// of presses crosses the useful range of a typical `LayoutConfig`
    /// clamp (24-100), fine enough that a single press is a visible but not
    /// jarring change.
    const WIDTH_STEP: u16 = 4;

    pub fn new(tree: LayoutTree, size: Size, file_info: FileInfo) -> Self {
        let content_width = tree.width();
        AppState {
            tree,
            scroll: 0,
            size,
            content_width,
            file_info,
            status_message: None,
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

    /// The layout width currently in effect — `tree.width()` at construction,
    /// then kept in sync by every [`AppState::relayout`] afterward.
    pub fn content_width(&self) -> u16 {
        self.content_width
    }

    /// Scroll position as a percentage (DW-1.2): 0 at the top, 100 at
    /// `max_scroll`. A document that fits the viewport (`max_scroll == 0`)
    /// reads 100 — the reader has already seen all of it — rather than 0,
    /// which would otherwise be indistinguishable from "just opened".
    fn position_pct(&self) -> u8 {
        let max = self.max_scroll();
        if max == 0 {
            100
        } else {
            ((self.scroll as u64 * 100) / max as u64) as u8
        }
    }

    /// Sets a transient status-row message, replacing the permanent ruler
    /// for [`STATUS_MESSAGE_TTL_FRAMES`] calls to [`AppState::status`].
    pub fn set_status(&mut self, message: StatusMessage) {
        self.status_message = Some((message.text, STATUS_MESSAGE_TTL_FRAMES));
    }

    /// `Ctrl-g`'s action (DW-1.3): shows the open document's name, byte
    /// size, and line count as a transient status message.
    fn show_file_info(&mut self) {
        let info = self.file_info.clone();
        self.set_status(StatusMessage::new(format!(
            "{} — {} bytes, {} lines",
            info.name, info.byte_size, info.line_count
        )));
    }

    /// The status row's current content (DW-1.1/1.2/1.3), and the entry
    /// point that ages a transient message toward its TTL by one frame.
    ///
    /// Must be called **at most once per painted frame** — the event loop is
    /// the only real caller — since every call spends one frame of the
    /// active message's remaining budget. A message set via
    /// [`AppState::set_status`] is returned on this and the next
    /// `STATUS_MESSAGE_TTL_FRAMES - 1` calls, then reverts to the permanent
    /// ruler.
    pub fn status(&mut self) -> StatusLine {
        let message = match self.status_message.take() {
            Some((text, ttl)) => {
                if ttl > 1 {
                    self.status_message = Some((text.clone(), ttl - 1));
                }
                Some(text)
            }
            None => None,
        };
        StatusLine {
            position_pct: self.position_pct(),
            name: self.file_info.name.clone(),
            message,
        }
    }

    /// `+`/`-` (DW-1.4): widens/narrows [`AppState::content_width`] by
    /// [`AppState::WIDTH_STEP`], clamped to `ctx.config`'s range, and
    /// relays out preserving the scroll anchor.
    pub fn widen(&mut self, ctx: &LayoutContext) {
        self.adjust_width(i32::from(Self::WIDTH_STEP), ctx);
    }

    /// The narrowing half of [`AppState::widen`].
    pub fn narrow(&mut self, ctx: &LayoutContext) {
        self.adjust_width(-i32::from(Self::WIDTH_STEP), ctx);
    }

    /// Applies `delta` to `content_width`, clamping *before* relaying out —
    /// not after — so repeated presses at a clamp boundary compose
    /// correctly: without the eager clamp, several `+` presses past
    /// `max_width` would each leave `content_width` growing unboundedly
    /// while the laid-out tree stays pinned, and the next `-` press would
    /// need several presses of its own before the tree visibly narrows.
    fn adjust_width(&mut self, delta: i32, ctx: &LayoutContext) {
        let requested = i32::from(self.content_width) + delta;
        let clamped = requested.clamp(
            i32::from(ctx.config.min_width),
            i32::from(ctx.config.max_width),
        );
        // Safe: `clamped` is bounded by two `u16` values.
        self.content_width = clamped as u16;
        self.relayout_preserving_anchor(ctx, *ctx.config);
    }

    /// The entry point every later phase calls after a width, theme, fold,
    /// or reload change (see the plan's Phase 1 `Produces`): relays out at
    /// [`AppState::content_width`] — clamped to `config` — while preserving
    /// the scroll anchor and leaving the viewport [`Size`] untouched.
    ///
    /// For a change that does not touch layout width at all (a theme swap),
    /// this still recomputes the tree at the *same* width, which
    /// [`AppState::relayout`]'s `no_reflow_occurred` check turns into a
    /// no-op scroll adjustment — cheap, and it keeps every chrome-mutating
    /// key on one anchor-preserving path instead of a bespoke one per key.
    pub fn relayout_preserving_anchor(&mut self, ctx: &LayoutContext, config: LayoutConfig) {
        let width = self.content_width.clamp(config.min_width, config.max_width);
        let size = self.size;
        self.relayout(ctx, width, size);
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
            // Ctrl-g (DW-1.3): file info. Before this phase, lowercase
            // Ctrl-g meant nothing to us and fell through to the unmodified
            // `'g'` binding (jump to top) — an accident of the "strictly
            // additive" fallthrough this function's own doc comment
            // describes, preserved only for pre-Ctrl-aware compatibility,
            // never a deliberate feature. Now that Ctrl-g means something,
            // it stops falling through, exactly like every other chord in
            // this match. Ctrl-G (uppercase, jump to end) is untouched.
            KeyCode::Char('g') => self.show_file_info(),
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
        // Resync to what layout actually used (already clamped), not the
        // raw `width` argument — the two agree today but this is the
        // authoritative value regardless. A caller-driven toggle
        // (`AppState::widen`/`narrow`) and a real terminal resize both go
        // through here, so a resize always overrides a stale toggle.
        self.content_width = self.tree.width();

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
        let file_info = FileInfo {
            name: "test.md".to_string(),
            byte_size: source.len() as u64,
            line_count: source.lines().count(),
        };
        let state = AppState::new(tree, size, file_info);
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
            FileInfo::default(),
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

        // Ctrl-g (lowercase) is deliberately NOT covered here as of Phase 1:
        // DW-1.3 claims that chord for file info (see
        // `test_dw_1_3_ctrl_g_shows_file_name_byte_size_and_line_count`), so
        // it now stops falling through to the unmodified `'g'` binding,
        // exactly like every other chord this function already owns.

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

    // ---- Status row: position %, Ctrl-g file info, width toggle (Phase 1) --

    #[test]
    fn test_dw_1_2_status_position_percentage_reads_0_at_top_and_100_at_max_scroll() {
        let (_doc, _config, _engine, mut state) = build(&non_reflowing_source(50), 40, 10);
        assert_eq!(state.status().position_pct, 0, "scroll 0 must read 0%");

        state.handle_key_event(plain(KeyCode::Char('G')));
        assert!(state.max_scroll() > 0, "fixture must actually scroll");
        assert_eq!(
            state.status().position_pct,
            100,
            "max_scroll must read 100%"
        );
    }

    /// Dirty case: a document that fits the viewport has `max_scroll == 0`,
    /// so "top" and "max_scroll" are literally the same position. The reader
    /// has already seen the whole document, so this reads 100, not 0.
    #[test]
    fn test_dw_1_2_position_percentage_reads_100_when_the_document_fits_the_viewport() {
        let (_doc, _config, _engine, mut state) = build("short\n", 40, 100);
        assert_eq!(state.max_scroll(), 0);
        assert_eq!(state.status().position_pct, 100);
    }

    #[test]
    fn test_dw_1_3_ctrl_g_shows_file_name_byte_size_and_line_count() {
        let source = "line one\nline two\nline three\n";
        let (_doc, _config, _engine, mut state) = build(source, 40, 10);
        assert!(!state.handle_key_event(ctrl('g')), "Ctrl-g must not quit");
        let message = state
            .status()
            .message
            .expect("Ctrl-g must set a status message");
        assert!(message.contains("test.md"), "{message:?}");
        assert!(
            message.contains(&format!("{} bytes", source.len())),
            "{message:?}"
        );
        assert!(
            message.contains(&format!("{} lines", source.lines().count())),
            "{message:?}"
        );
    }

    /// Dirty: a zero-byte file. Byte size and line count must both read 0,
    /// not panic on an empty document.
    #[test]
    fn test_dw_1_3_ctrl_g_on_a_zero_byte_file_reports_zero_bytes_and_zero_lines() {
        let (_doc, _config, _engine, mut state) = build("", 40, 10);
        state.handle_key_event(ctrl('g'));
        let message = state.status().message.unwrap();
        assert!(message.contains("0 bytes"), "{message:?}");
        assert!(message.contains("0 lines"), "{message:?}");
    }

    /// Dirty: no trailing newline. `str::lines` (not `\n`-counting) is what
    /// keeps this at 2 rather than 1.
    #[test]
    fn test_dw_1_3_ctrl_g_on_a_file_with_no_trailing_newline_counts_lines_correctly() {
        let (_doc, _config, _engine, mut state) = build("a\nb", 40, 10);
        state.handle_key_event(ctrl('g'));
        let message = state.status().message.unwrap();
        assert!(message.contains("2 lines"), "{message:?}");
    }

    #[test]
    fn test_dw_1_3_status_message_clears_after_its_frame_budget() {
        let (_doc, _config, _engine, mut state) = build("hello\n", 40, 10);
        state.handle_key_event(ctrl('g'));
        for frame in 0..STATUS_MESSAGE_TTL_FRAMES {
            assert!(
                state.status().message.is_some(),
                "message must still be visible on frame {frame}"
            );
        }
        assert!(
            state.status().message.is_none(),
            "message must be gone once its frame budget ({STATUS_MESSAGE_TTL_FRAMES} frames) is spent"
        );
    }

    #[test]
    fn test_dw_1_4_widen_preserves_the_top_visible_block() {
        let source: String = (0..60)
            .map(|i| {
                format!("Paragraph {i:02} word word word word word word word word word word.\n\n")
            })
            .collect();
        let (doc, config, engine, mut state) = build(&source, 60, 10);
        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };

        for _ in 0..30 {
            state.handle_key_event(plain(KeyCode::Down));
        }
        let anchor = state
            .tree()
            .block_at(state.scroll())
            .expect("a scrolled-to line has a block");
        let before_width = state.content_width();

        state.widen(&ctx);

        assert!(
            state.content_width() > before_width,
            "widen must actually widen"
        );
        assert_eq!(
            state.tree().block_at(state.scroll()),
            Some(anchor),
            "widening must preserve the top visible block"
        );
    }

    #[test]
    fn test_dw_1_4_narrow_preserves_the_top_visible_block() {
        let source: String = (0..60)
            .map(|i| {
                format!("Paragraph {i:02} word word word word word word word word word word.\n\n")
            })
            .collect();
        let (doc, config, engine, mut state) = build(&source, 90, 10);
        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };

        for _ in 0..30 {
            state.handle_key_event(plain(KeyCode::Down));
        }
        let anchor = state
            .tree()
            .block_at(state.scroll())
            .expect("a scrolled-to line has a block");
        let before_width = state.content_width();

        state.narrow(&ctx);

        assert!(
            state.content_width() < before_width,
            "narrow must actually narrow"
        );
        assert_eq!(
            state.tree().block_at(state.scroll()),
            Some(anchor),
            "narrowing must preserve the top visible block"
        );
    }

    /// The clamp boundary (dirty case): many `+` presses past `max_width`
    /// must not leave `content_width` growing past the ceiling — and a
    /// single subsequent `-` must move immediately, not need several
    /// presses to "unstick" from an overshoot.
    #[test]
    fn test_dw_1_4_widen_clamps_at_max_width_and_narrow_moves_immediately_from_there() {
        let (doc, config, engine, mut state) = build(&non_reflowing_source(10), 40, 10);
        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };

        for _ in 0..50 {
            state.widen(&ctx);
        }
        assert_eq!(state.content_width(), config.max_width);

        state.narrow(&ctx);
        assert_eq!(
            state.content_width(),
            config.max_width - AppState::WIDTH_STEP
        );
    }

    /// The other clamp boundary, symmetric.
    #[test]
    fn test_dw_1_4_narrow_clamps_at_min_width_and_widen_moves_immediately_from_there() {
        let (doc, config, engine, mut state) = build(&non_reflowing_source(10), 40, 10);
        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };

        for _ in 0..50 {
            state.narrow(&ctx);
        }
        assert_eq!(state.content_width(), config.min_width);

        state.widen(&ctx);
        assert_eq!(
            state.content_width(),
            config.min_width + AppState::WIDTH_STEP
        );
    }

    /// Edge case named in the plan: a theme toggle relays out at an
    /// *unchanged* width (`relayout_preserving_anchor` called with the same
    /// `content_width` — exactly what `T` does, since nothing about theme
    /// touches layout). A reserved image box's `NodeId` must come out
    /// identical, since that identity is the only thing the media sink (out
    /// of this phase's file scope) uses to decide whether a placement can be
    /// reused rather than re-transmitted.
    #[test]
    fn test_theme_toggle_style_relayout_preserves_reserved_box_node_identity() {
        use layout::{CellSize, IntrinsicSizer, Line};

        struct AlwaysSizes;
        impl IntrinsicSizer for AlwaysSizes {
            fn size(&self, _node: NodeId, _doc: &Document) -> Option<CellSize> {
                // Two rows: a one-row box rides a text line as a
                // `LineItem::Box`, so only a taller box takes the standalone
                // `Line::Reserved` path this test needs (see
                // `painter.rs`'s own tests for the same distinction).
                Some(CellSize { cols: 10, rows: 2 })
            }
        }

        fn reserved_node_id(tree: &LayoutTree) -> Option<NodeId> {
            (0..tree.line_count()).find_map(|i| match tree.lines(i..i + 1).next() {
                Some(Line::Reserved(line)) => Some(line.boxed.node_id),
                _ => None,
            })
        }

        let doc = Document::parse("before\n\n![alt](pic.png)\n\nafter\n");
        let config = LayoutConfig::default();
        let engine = WidthEngine::new(WidthConfig::default());
        let tree = layout(&doc, 40, &config, &engine, &AlwaysSizes);
        let before_id = reserved_node_id(&tree).expect("fixture reserves an image box");

        let mut state = AppState::new(
            tree,
            Size {
                width: 40,
                height: 10,
            },
            FileInfo::default(),
        );
        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &AlwaysSizes,
        };

        state.relayout_preserving_anchor(&ctx, config);

        let after_id = reserved_node_id(state.tree()).expect("the box must still be reserved");
        assert_eq!(
            before_id, after_id,
            "a same-width relayout must not change the reserved box's node identity"
        );
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
