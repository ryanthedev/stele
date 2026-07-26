//! Viewport state: scroll position tracking, key-driven navigation, and
//! debounced resize/relayout — all pure and independently testable, so the
//! event loop in `main.rs` stays thin glue over real crossterm I/O.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use ast::{Document, NodeId};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use layout::{IntrinsicSizer, LayoutConfig, LayoutTree, Line, LineItem, Outline, layout};
use width::WidthEngine;

use crate::painter::Size;

/// Mixed into [`AppState::fingerprint`] between lines so two blocks whose runs
/// concatenate to the same bytes but break differently cannot hash alike.
const LINE_BOUNDARY: u8 = 0xff;

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
    /// `(fingerprint, occurrence)` — a hash of what the block paints, and how
    /// many earlier blocks paint the same thing. Together these are what
    /// survives a re-parse; `block` does not, because a [`NodeId`] is
    /// positional (see [`AppState::line_of_reloaded`]).
    ///
    /// `None` on a resize, where nothing re-parsed and the `NodeId` is
    /// authoritative — computing it there would hash a block whose answer is
    /// never consulted.
    identity: Option<(u64, usize)>,
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

/// What the viewer is showing, and therefore what a key means.
///
/// Deliberately a flat enum matched at the top of
/// [`AppState::handle_key_event`], not an overlay stack: there is exactly one
/// overlay and it is modal, so a stack would be a general mechanism with one
/// user and two states it can be in that this cannot express (two overlays
/// open, and an empty stack that is not `Normal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Reading the document. Every pre-existing binding means what it did.
    Normal,
    /// The full-screen table of contents, with `selected` indexing
    /// [`Outline::entries`].
    Toc { selected: usize },
}

/// One row of the rendered TOC overlay: the text to paint and whether it is
/// the selected entry.
///
/// [`AppState::toc_rows`] composes these, so which entries are on screen,
/// how a level is shown, and what happens when the list is taller than the
/// terminal are all decided in pure code a test can call — the painter only
/// prints what it is handed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocRow {
    pub text: String,
    pub selected: bool,
}

/// What the status row says when a heading motion or the TOC has nothing to
/// work with. One constant, because a document with no headings must answer
/// the same way whichever key asked (DW-3.1, and the overlay's edge case).
const NO_HEADINGS: &str = "no headings in this document";

/// The viewer's mutable state: the current layout tree, scroll offset, and
/// viewport size. Navigation and resize are pure state transitions, driven
/// directly from tests without a real terminal or timers.
pub struct AppState {
    tree: LayoutTree,
    scroll: usize,
    size: Size,
    mode: Mode,
    /// The first half of a two-keystroke motion (`]]` / `[[`), waiting for
    /// its twin. Not folded into [`Mode`]: `Mode` answers "what is on the
    /// screen", and a half-typed bracket is not on the screen — a reader
    /// mid-sequence is looking at exactly the document they were looking at.
    pending: Option<char>,
    /// Where `Esc` puts the reader back when the TOC is dismissed (DW-3.2).
    /// Captured on the way in, so the restore holds even if some future
    /// binding moves the viewport while the overlay is up.
    toc_return_scroll: usize,
    /// The layout width `+`/`-` adjust, independent of the terminal's own
    /// width — see [`AppState::relayout_preserving_anchor`]. Resynced to
    /// `tree.width()` at the end of every [`AppState::relayout`], so a real
    /// terminal resize always wins over a stale toggle.
    content_width: u16,
    file_info: FileInfo,
    /// `(text, frames remaining)`. `None` once the TTL has been exhausted or
    /// no message has been set.
    status_message: Option<(String, u32)>,
    /// Set by [`AppState::reload_document`] and consumed by the next
    /// [`AppState::relayout`]. See [`AppState::no_reflow_occurred`] for why a
    /// width comparison cannot answer this on its own.
    document_changed: bool,
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
            mode: Mode::Normal,
            pending: None,
            toc_return_scroll: 0,
            content_width,
            file_info,
            status_message: None,
            document_changed: false,
        }
    }

    pub fn tree(&self) -> &LayoutTree {
        &self.tree
    }

    /// What the viewer is showing — the event loop's cue for which painter
    /// entry point to call.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The document's headings, in document order.
    pub fn outline(&self) -> &Outline {
        self.tree.outline()
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

    /// Drops the transient message immediately, whatever is left of its TTL,
    /// so the permanent ruler returns on the very next frame.
    ///
    /// The TTL is a budget for a message the reader may not have finished
    /// reading; it is **not** a claim that the message is still true. Every
    /// message this type shows describes the document as it was when the
    /// message was set — `Ctrl-G`'s byte and line counts, or a failed
    /// reload's reason — so replacing the document invalidates all of them at
    /// once. Hence the caller: [`AppState::reload_document`], the one place a
    /// document is replaced.
    fn clear_status(&mut self) {
        self.status_message = None;
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

    /// A `--watch` reload (DW-2.2): `ctx.doc` is a **different** document from
    /// the one the current tree was built from. Re-anchors the reader into
    /// the new tree and refreshes what `Ctrl-G` reports.
    ///
    /// Separate from [`AppState::relayout_preserving_anchor`] because of
    /// [`AppState::no_reflow_occurred`]: a reload happens at the same layout
    /// width, so the width comparison that stands in for "the tree is
    /// unchanged" would wrongly say nothing moved and keep the raw scroll
    /// offset — correct only when the edit was entirely below the reader.
    /// This marks the tree as genuinely new so the anchor path runs, and the
    /// anchor is resolved *by content* rather than by node identity — see
    /// [`AppState::line_of_reloaded`] for why identity alone is not enough.
    /// Any transient status message is dropped here rather than left to age
    /// out. A message on the row describes the document that produced it, so
    /// once the document is replaced the message is not merely stale, it is
    /// wrong: `reload failed: … No such file or directory` sat under a
    /// correctly re-rendered document for ~100 frames after the file came
    /// back, and `Ctrl-G`'s byte and line counts would have outlived the file
    /// they measured the same way.
    ///
    /// An open TOC is re-seated here for the same reason the status message is
    /// dropped: it describes a document that no longer exists. `selected`
    /// indexes the *old* outline, and the new one may be shorter or empty, so
    /// the overlay is either clamped back into range or — when the reloaded
    /// document has no headings at all — dismissed with the same message `t`
    /// would have given. Leaving it up would show a list of headings that are
    /// not in the document behind it, and `Enter` on one of them would jump to
    /// wherever that index now happens to land.
    pub fn reload_document(&mut self, ctx: &LayoutContext, file_info: FileInfo) {
        self.file_info = file_info;
        self.clear_status();
        self.document_changed = true;
        self.relayout_preserving_anchor(ctx, *ctx.config);
        self.reseat_toc();
    }

    /// Puts an open TOC back on a heading that exists in the tree installed
    /// now. Called after a reload; a no-op in [`Mode::Normal`].
    fn reseat_toc(&mut self) {
        let Mode::Toc { selected } = self.mode else {
            // The overlay is not up, but `toc_return_scroll` is still a line
            // index into a tree that has just been replaced. Re-seating it on
            // the anchored scroll keeps the next `Esc` honest.
            self.toc_return_scroll = self.scroll;
            return;
        };
        let count = self.tree.outline().len();
        self.toc_return_scroll = self.scroll;
        if count == 0 {
            self.mode = Mode::Normal;
            self.set_status(StatusMessage::new(NO_HEADINGS));
            return;
        }
        self.mode = Mode::Toc {
            selected: selected.min(count - 1),
        };
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

    /// Puts the reader at the top of `block`, if that block is in the tree.
    ///
    /// Two lookups, because a heading is addressable two ways and only one of
    /// them is always available: `first_line_of` answers for any *top-level*
    /// block, and the outline answers for a heading nested inside a
    /// blockquote or list item, whose own node tags no line (see
    /// [`layout::OutlineEntry::block`]). A block that is neither leaves the
    /// viewport alone rather than guessing.
    pub fn jump_to_block(&mut self, block: NodeId) {
        let line = self
            .tree
            .first_line_of(block)
            .or_else(|| self.tree.outline().line_for_block(block));
        if let Some(line) = line {
            self.set_scroll(line);
        }
    }

    /// `]]` / `[[` (DW-3.1): the next or previous heading, or a status-row
    /// message when there is none to move to.
    ///
    /// Both ends are a clamp, not a wrap: `]]` at the last heading says so
    /// and stays put. Wrapping would silently teleport a reader to the top of
    /// a long document on a keystroke they meant as "keep going".
    ///
    /// A motion that leaves the viewport where it was reports too, and with
    /// the same message as no-such-heading — because from the reader's chair
    /// the two are one fact. It happens for real at the document tail: the
    /// last heading may sit below `max_scroll`, so the jump to it is honoured,
    /// clamps, and moves nothing. Silence there reads as a dropped keystroke.
    fn jump_heading(&mut self, forward: bool) {
        let outline = self.tree.outline();
        let index = if forward {
            outline.next_after(self.scroll)
        } else {
            outline.previous_before(self.scroll)
        };
        let target = index.and_then(|index| outline.line_of(index));
        let empty = outline.is_empty();
        let before = self.scroll;
        if let Some(line) = target {
            self.set_scroll(line);
            if self.scroll != before {
                return;
            }
        }
        self.set_status(StatusMessage::new(match (empty, forward) {
            (true, _) => NO_HEADINGS,
            (false, true) => "last heading",
            (false, false) => "first heading",
        }));
    }

    /// `t` (DW-3.2): opens the TOC on the heading the reader is currently
    /// under, so `t` followed immediately by `Enter` is a no-op rather than a
    /// jump to the top of the document.
    fn open_toc(&mut self) {
        let outline = self.tree.outline();
        if outline.is_empty() {
            self.set_status(StatusMessage::new(NO_HEADINGS));
            return;
        }
        let selected = outline.index_at_or_before(self.scroll).unwrap_or(0);
        self.toc_return_scroll = self.scroll;
        self.mode = Mode::Toc { selected };
    }

    /// The rows the TOC overlay paints into a viewport `height` rows tall,
    /// scrolled so the selection is always among them (DW-3.2).
    ///
    /// Empty outside [`Mode::Toc`], and empty for a zero-row viewport — a
    /// terminal too short to render the overlay paints nothing rather than
    /// panicking on the window arithmetic.
    pub fn toc_rows(&self, height: u16) -> Vec<TocRow> {
        let Mode::Toc { selected } = self.mode else {
            return Vec::new();
        };
        let entries = &self.tree.outline().entries;
        let height = usize::from(height).min(entries.len());
        if height == 0 {
            return Vec::new();
        }
        let first = selected
            .saturating_sub(height / 2)
            .min(entries.len() - height);
        entries[first..first + height]
            .iter()
            .enumerate()
            .map(|(offset, entry)| TocRow {
                // Level shown twice over, and both are load-bearing: the
                // indent makes the shape of the document scannable, the
                // `#`s name the level exactly (an indent alone cannot
                // distinguish an H3 under an H2 from an H3 under an H1).
                text: format!(
                    "{:width$}{} {}",
                    "",
                    "#".repeat(usize::from(entry.level.max(1))),
                    entry.text,
                    width = 2 * usize::from(entry.level.saturating_sub(1)),
                ),
                selected: first + offset == selected,
            })
            .collect()
    }

    /// Applies one key press *with its modifiers*. Returns `true` when the
    /// key requests quit. This is the event loop's entry point.
    ///
    /// The mode decides what a key means before anything else looks at it, so
    /// the TOC's `j`/`k` cannot also scroll the document underneath it.
    pub fn handle_key_event(&mut self, key: KeyEvent) -> bool {
        match self.mode {
            Mode::Toc { selected } => self.handle_toc_key(key, selected),
            Mode::Normal => self.handle_normal_key(key),
        }
    }

    /// The document-reading key table.
    ///
    /// Control chords are tried first and **fall through** to the unmodified
    /// table when the chord means nothing to us, so every pre-existing
    /// binding keeps behaving exactly as it did when the event loop passed
    /// `key.code` and dropped the modifiers on the floor: `Ctrl-Down` still
    /// scrolls down, `Ctrl-q` still quits, `Ctrl-G` still jumps to the end.
    ///
    /// The bracket motions are checked ahead of both, and only unmodified: a
    /// chord is never half of a two-keystroke sequence, so `Ctrl-]` cannot
    /// arm one and cannot complete one either.
    fn handle_normal_key(&mut self, key: KeyEvent) -> bool {
        let pending = self.pending.take();
        if !key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char(bracket @ (']' | '[')) => {
                    if pending == Some(bracket) {
                        self.jump_heading(bracket == ']');
                    } else {
                        self.pending = Some(bracket);
                    }
                    return false;
                }
                KeyCode::Char('t') => {
                    self.open_toc();
                    return false;
                }
                _ => {}
            }
        }
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && let Some(quit) = self.handle_control_chord(key.code)
        {
            return quit;
        }
        self.handle_key(key.code)
    }

    /// The TOC overlay's key table (DW-3.2). `selected` is the mode's own
    /// field, passed in so this function never has to re-match the mode it
    /// was dispatched on.
    ///
    /// The two quit keys are repeated here rather than delegated: an overlay
    /// that swallows `q` and Ctrl-C leaves a reader with no way out that they
    /// would think to try, and raw mode has already made Ctrl-C a keystroke
    /// rather than a signal.
    fn handle_toc_key(&mut self, key: KeyEvent, selected: usize) -> bool {
        let last = self.tree.outline().len().saturating_sub(1);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('c') if ctrl => return true,
            KeyCode::Esc | KeyCode::Char('t') => {
                self.mode = Mode::Normal;
                self.set_scroll(self.toc_return_scroll);
            }
            KeyCode::Enter => {
                let line = self.tree.outline().line_of(selected);
                if let Some(line) = line {
                    self.set_scroll(line);
                }
                self.mode = Mode::Normal;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.mode = Mode::Toc {
                    selected: selected.saturating_add(1).min(last),
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.mode = Mode::Toc {
                    selected: selected.saturating_sub(1),
                }
            }
            KeyCode::Home | KeyCode::Char('g') => self.mode = Mode::Toc { selected: 0 },
            KeyCode::End | KeyCode::Char('G') => self.mode = Mode::Toc { selected: last },
            KeyCode::PageDown => {
                self.mode = Mode::Toc {
                    selected: selected.saturating_add(self.page_size()).min(last),
                }
            }
            KeyCode::PageUp => {
                self.mode = Mode::Toc {
                    selected: selected.saturating_sub(self.page_size()),
                }
            }
            _ => {}
        }
        false
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
        // Read before `anchor()`: whether the document was replaced decides
        // what identity the anchor has to carry.
        let document_changed = self.document_changed;
        let anchor = self.anchor(document_changed);
        let old_max = self.max_scroll();
        let ratio = if old_max == 0 {
            0.0
        } else {
            self.scroll as f64 / old_max as f64
        };
        let previous_scroll = self.scroll;
        let previous_width = self.tree.width();
        // Cleared here, whatever path is taken below, so a reload's claim
        // cannot survive into the *next* relayout and force a needless
        // re-anchor there.
        self.document_changed = false;

        self.tree = layout(ctx.doc, width, ctx.config, ctx.engine, ctx.sizer);
        self.size = new_size;
        // Resync to what layout actually used (already clamped), not the
        // raw `width` argument — the two agree today but this is the
        // authoritative value regardless. A caller-driven toggle
        // (`AppState::widen`/`narrow`) and a real terminal resize both go
        // through here, so a resize always overrides a stale toggle.
        self.content_width = self.tree.width();

        let target = if !document_changed && self.no_reflow_occurred(previous_width) {
            previous_scroll
        } else {
            anchor
                .and_then(|anchor| {
                    if document_changed {
                        self.line_of_reloaded(anchor)
                    } else {
                        self.line_of(anchor)
                    }
                })
                .unwrap_or_else(|| (ratio * self.max_scroll() as f64).round() as usize)
        };
        self.set_scroll(target);
    }

    /// The reader's current position as an [`Anchor`], or `None` when the top
    /// line belongs to no block (an empty document).
    ///
    /// `for_reload` asks for the extra identity a re-parse needs — the
    /// fingerprint and the occurrence index. It is skipped on a resize, where
    /// the `NodeId` is authoritative and hashing the block would be work whose
    /// answer is never read.
    fn anchor(&self, for_reload: bool) -> Option<Anchor> {
        let block = self.tree.block_at(self.scroll)?;
        let first = self.tree.first_line_of(block)?;
        let span = self.block_span(block, first);
        let identity = for_reload.then(|| {
            let fingerprint = self.fingerprint(first, span);
            (fingerprint, self.occurrence_of(first, fingerprint))
        });
        Some(Anchor {
            block,
            offset: self.scroll.saturating_sub(first),
            span,
            identity,
        })
    }

    /// How many blocks *above* line `first` paint the same thing it does —
    /// making the anchored block "the `n`th block whose content hashes to
    /// this" rather than "the block at index `k`".
    ///
    /// This is the property that survives an edit elsewhere in the file. An
    /// absolute ordinal does not: it is an index in the **old** document, so
    /// once blocks are inserted above the reader every candidate's ordinal has
    /// shifted and choosing the one nearest the old value drags the reader
    /// backwards — measured at up to 24 lines, onto a different copy, once the
    /// insertion exceeded half the spacing between two identical blocks.
    /// Occurrence index is invariant to *unrelated* content appearing or
    /// disappearing above, which is exactly the `--watch` edit that broke it.
    fn occurrence_of(&self, first: usize, fingerprint: u64) -> usize {
        self.block_runs()
            .into_iter()
            .take_while(|&(candidate, _)| candidate < first)
            .filter(|&(candidate, span)| self.fingerprint(candidate, span) == fingerprint)
            .count()
    }

    /// A hash of what the block starting at `first` actually *paints*, over
    /// its `span` lines — the identity that survives a re-parse.
    ///
    /// Painted text, not source text, because that is what this type has:
    /// [`AppState::anchor`] runs before the new tree is installed and never
    /// holds the old [`Document`] at all. It is equivalent for the purpose, so
    /// long as the two trees were laid out at the same width — which a reload
    /// always is, since [`AppState::reload_document`] goes through
    /// [`AppState::relayout_preserving_anchor`] at the current
    /// [`AppState::content_width`]. (At *different* widths the same block
    /// wraps into different lines and this would not match, which is exactly
    /// why the fingerprint is consulted only on the document-changed path and
    /// never on a resize.)
    ///
    /// A media box contributes its cell extent rather than a `NodeId`, since
    /// the id is the very thing a re-parse renumbers. A line separator is
    /// mixed in so `["ab", "c"]` and `["a", "bc"]` cannot collide.
    fn fingerprint(&self, first: usize, span: usize) -> u64 {
        let mut hasher = DefaultHasher::new();
        for line in self.tree.lines(first..first.saturating_add(span)) {
            match line {
                Line::Items(items) => {
                    for item in items {
                        match item {
                            LineItem::Run(run) => run.text.hash(&mut hasher),
                            LineItem::Box(reserved) => {
                                (reserved.cols, reserved.rows).hash(&mut hasher);
                            }
                        }
                    }
                }
                Line::Reserved(reserved) => {
                    for run in &reserved.prefix {
                        run.text.hash(&mut hasher);
                    }
                    (reserved.boxed.cols, reserved.boxed.rows).hash(&mut hasher);
                }
            }
            LINE_BOUNDARY.hash(&mut hasher);
        }
        hasher.finish()
    }

    /// Every block in the installed tree, top to bottom, as
    /// `(first line, span)`.
    ///
    /// One walk of the line-to-block map; the blocks partition the lines, so
    /// this is linear in the document even though it looks nested.
    fn block_runs(&self) -> Vec<(usize, usize)> {
        let mut runs = Vec::new();
        let mut line = 0;
        while line < self.tree.line_count() {
            let Some(block) = self.tree.block_at(line) else {
                line += 1;
                continue;
            };
            let span = self.block_span(block, line);
            runs.push((line, span));
            line += span;
        }
        runs
    }

    /// Where `anchor` lands after the **document itself** was replaced.
    ///
    /// [`AppState::line_of`] cannot be used here, and the difference is the
    /// whole of DW-2.2's anchor claim. A [`NodeId`] is a position in the
    /// re-parsed node stream, so inserting one block at the top of the file
    /// shifts every id below it — and `first_line_of` still answers `Some`
    /// for the shifted id, so a positional lookup does not merely lose
    /// precision, it returns a **different block** and reports success.
    /// Measured before this was fixed: prepending a three-line fence above a
    /// reader parked below a 200-line fence moved them 201 lines backwards,
    /// into the fence.
    ///
    /// So the block is re-found by **content and occurrence**, never by
    /// position: collect every block that hashes like the reader's, and take
    /// the one at the same occurrence index. "The 3rd block that paints this"
    /// stays the 3rd such block however much unrelated text appeared above it.
    ///
    /// There is deliberately no `NodeId` fast path. An id that still resolves
    /// and still hashes the same looks like proof, and is not: with two
    /// identical blocks in a document, a shifted id can land on the *other*
    /// copy and match. Checking cheaply-but-sometimes-wrongly first, then
    /// carefully, is only cheaper when the cheap answer can be trusted — here
    /// it cannot, and the careful answer needs the whole-document scan anyway
    /// to know the occurrence index. One path, always right, is worth more
    /// than two paths where the first is a trap.
    ///
    /// Returns `None` when nothing in the new document paints like the
    /// reader's block — their own paragraph was edited or deleted — so
    /// [`AppState::relayout`] falls back to the proportional ratio. That
    /// fallback is now reached when it *should* be, which a positional lookup
    /// prevented by always "succeeding".
    fn line_of_reloaded(&self, anchor: Anchor) -> Option<usize> {
        let (fingerprint, occurrence) = anchor.identity?;
        let matches: Vec<(usize, usize)> = self
            .block_runs()
            .into_iter()
            .filter(|&(first, span)| self.fingerprint(first, span) == fingerprint)
            .collect();

        // Clamped rather than exact: if copies above the reader were deleted,
        // their block is still in here, just at a lower index. Landing on the
        // last remaining copy beats refusing to anchor at all.
        let (first, span) = *matches.get(occurrence).or_else(|| matches.last())?;
        Some(self.place(anchor, first, span))
    }

    /// The line the reader lands on, given the block they were anchored to now
    /// starts at `first` and occupies `span` lines. Shared by both resolution
    /// paths so "how far into the block" is decided in exactly one place.
    fn place(&self, anchor: Anchor, first: usize, span: usize) -> usize {
        let offset = if span == anchor.span {
            anchor.offset
        } else {
            (anchor.offset as f64 * span as f64 / anchor.span as f64).round() as usize
        };
        first + offset.min(span - 1)
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
    /// Used when the document is known unchanged (a resize, a width toggle, a
    /// theme swap): the `NodeId` is authoritative because nothing re-parsed,
    /// and the block's painted text is *expected* to differ after a reflow, so
    /// a content check would be wrong here rather than merely redundant. A
    /// reload goes to [`AppState::line_of_reloaded`] instead.
    fn line_of(&self, anchor: Anchor) -> Option<usize> {
        let first = self.tree.first_line_of(anchor.block)?;
        let span = self.block_span(anchor.block, first);
        Some(self.place(anchor, first, span))
    }

    /// Whether the just-installed tree is identical to the one it replaced.
    ///
    /// `layout` is pure and deterministic in `(doc, width, config, engine,
    /// sizer)` and clamps `width` into the config's range *before* laying
    /// out, storing the clamped value on the tree. The config, engine and
    /// sizer are fixed for the session, so equal clamped widths imply
    /// identical trees — which is why comparing one `u16` is a sound
    /// stand-in for comparing the whole tree, and why a resize between two
    /// widths that both clamp to the same value (say 10 and 15 against a
    /// 24-cell floor) correctly counts as "no reflow" too.
    ///
    /// **The document is the one input that stopped being fixed.** Since
    /// `--watch` (DW-2.2) it can be replaced under an unchanged width, and
    /// this function cannot see that — which is exactly what
    /// [`AppState::document_changed`] carries, checked by the caller *before*
    /// this. Callers must not use this alone.
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

    // ---- Heading navigation and the TOC overlay (Phase 3) ----------------

    /// `n` headings at cycling levels 1-3, each followed by a body
    /// paragraph, so consecutive heading lines are several lines apart and
    /// every heading names itself.
    fn heading_source(n: usize) -> String {
        (0..n)
            .map(|i| {
                let hashes = "#".repeat(1 + i % 3);
                format!("{hashes} Heading {i}\n\nbody {i}\n\n")
            })
            .collect()
    }

    /// The tree line each heading starts at — the oracle every jump assertion
    /// below is checked against, read from the outline the layout built
    /// rather than recomputed here.
    fn heading_lines(state: &AppState) -> Vec<usize> {
        (0..state.outline().len())
            .map(|i| {
                state
                    .outline()
                    .line_of(i)
                    .expect("every outline entry has a line")
            })
            .collect()
    }

    #[test]
    fn test_dw_3_1_bracket_bracket_moves_to_the_next_heading_and_back() {
        let (_doc, _config, _engine, mut state) = build(&heading_source(10), 40, 3);
        let lines = heading_lines(&state);
        assert_eq!(lines.len(), 10, "the fixture must have ten headings");
        assert_eq!(
            lines[0], 0,
            "the first heading is the document's first line"
        );
        assert!(
            *lines.last().unwrap() <= state.max_scroll(),
            "the fixture must be tall enough that no jump is clamped"
        );

        // Forward: `]]` from the top lands on the *second* heading, because
        // the reader is already looking at the first.
        for expected in &lines[1..] {
            assert!(!state.handle_key_event(plain(KeyCode::Char(']'))));
            assert!(!state.handle_key_event(plain(KeyCode::Char(']'))));
            assert_eq!(state.scroll(), *expected);
        }

        // Backward, all the way home.
        for expected in lines[..lines.len() - 1].iter().rev() {
            state.handle_key_event(plain(KeyCode::Char('[')));
            state.handle_key_event(plain(KeyCode::Char('[')));
            assert_eq!(state.scroll(), *expected);
        }
        assert_eq!(state.scroll(), 0);
    }

    #[test]
    fn test_dw_3_1_a_document_with_no_headings_reports_instead_of_moving() {
        let (_doc, _config, _engine, mut state) = build(&non_reflowing_source(50), 40, 10);
        assert!(
            state.outline().is_empty(),
            "the fixture must have no heading"
        );
        state.handle_key_event(plain(KeyCode::Char('j')));
        let before = state.scroll();

        for bracket in [']', '['] {
            state.handle_key_event(plain(KeyCode::Char(bracket)));
            assert!(!state.handle_key_event(plain(KeyCode::Char(bracket))));
            assert_eq!(state.scroll(), before, "`{bracket}{bracket}` must not move");
            let message = state
                .status()
                .message
                .unwrap_or_else(|| panic!("`{bracket}{bracket}` must report why it did nothing"));
            assert!(message.contains("no headings"), "{message:?}");
        }
    }

    /// The plan's edge case: a heading as the very first block and as the
    /// very last. Both ends clamp with a message rather than wrapping — a
    /// wrap would teleport a reader to the other end of the document on a
    /// keystroke they meant as "keep going".
    #[test]
    fn test_dw_3_1_a_heading_as_the_first_and_last_block_clamps_at_both_ends() {
        let mut source = String::from("# First\n\nbody\n\n");
        source.push_str(&"filler paragraph\n\n".repeat(20));
        source.push_str("## Last\n");
        let (_doc, _config, _engine, mut state) = build(&source, 40, 4);
        let lines = heading_lines(&state);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], 0, "the first block is a heading");

        // At the top, `[[` has nowhere above it.
        state.handle_key_event(plain(KeyCode::Char('[')));
        state.handle_key_event(plain(KeyCode::Char('[')));
        assert_eq!(state.scroll(), 0);
        assert!(state.status().message.unwrap().contains("first heading"));

        // Forward to the last heading, then past it.
        state.handle_key_event(plain(KeyCode::Char(']')));
        state.handle_key_event(plain(KeyCode::Char(']')));
        let at_last = state.scroll();
        assert_eq!(at_last, lines[1].min(state.max_scroll()));
        state.handle_key_event(plain(KeyCode::Char(']')));
        state.handle_key_event(plain(KeyCode::Char(']')));
        assert_eq!(
            state.scroll(),
            at_last,
            "`]]` past the last heading must stay"
        );
        assert!(state.status().message.unwrap().contains("last heading"));
    }

    /// Half a sequence is not a motion, and it must not swallow the key that
    /// follows it either.
    #[test]
    fn test_dw_3_1_a_lone_bracket_is_not_a_motion_and_does_not_eat_the_next_key() {
        let (_doc, _config, _engine, mut state) = build(&heading_source(6), 40, 4);

        assert!(!state.handle_key_event(plain(KeyCode::Char(']'))));
        assert_eq!(state.scroll(), 0, "a lone `]` must not move the reader");

        // `]` then `j` is one line down, not a heading jump.
        assert!(!state.handle_key_event(plain(KeyCode::Char('j'))));
        assert_eq!(state.scroll(), 1);

        // A mismatched pair (`]` then `[`) arms the second bracket rather
        // than jumping; the *next* `[` completes it.
        state.handle_key_event(plain(KeyCode::Char(']')));
        state.handle_key_event(plain(KeyCode::Char('[')));
        assert_eq!(state.scroll(), 1, "`][` is not a motion");
        state.handle_key_event(plain(KeyCode::Char('[')));
        assert_eq!(state.scroll(), 0, "the following `[` completes `[[`");
    }

    #[test]
    fn test_dw_3_2_t_opens_a_toc_listing_every_heading_with_its_level() {
        let (_doc, _config, _engine, mut state) = build(&heading_source(6), 60, 20);
        assert_eq!(state.mode(), Mode::Normal);

        assert!(!state.handle_key_event(plain(KeyCode::Char('t'))));
        assert_eq!(state.mode(), Mode::Toc { selected: 0 });

        let rows = state.toc_rows(20);
        assert_eq!(rows.len(), 6, "every heading must be listed");
        for (i, row) in rows.iter().enumerate() {
            assert!(
                row.text.contains(&format!("Heading {i}")),
                "row {i} must name its heading: {:?}",
                row.text
            );
            let level = 1 + i % 3;
            assert!(
                row.text
                    .contains(&format!("{} Heading {i}", "#".repeat(level))),
                "row {i} must show its level ({level}): {:?}",
                row.text
            );
        }
        assert!(rows[0].selected, "the reader is under the first heading");
        assert_eq!(
            rows.iter().filter(|row| row.selected).count(),
            1,
            "exactly one row is selected"
        );
    }

    #[test]
    fn test_dw_3_2_enter_jumps_to_the_selected_heading_and_leaves_the_overlay() {
        let (_doc, _config, _engine, mut state) = build(&heading_source(8), 60, 4);
        let lines = heading_lines(&state);

        state.handle_key_event(plain(KeyCode::Char('t')));
        for _ in 0..3 {
            state.handle_key_event(plain(KeyCode::Char('j')));
        }
        assert_eq!(state.mode(), Mode::Toc { selected: 3 });

        assert!(!state.handle_key_event(plain(KeyCode::Enter)));
        assert_eq!(state.mode(), Mode::Normal, "Enter closes the overlay");
        assert_eq!(state.scroll(), lines[3].min(state.max_scroll()));
        assert!(
            state.toc_rows(20).is_empty(),
            "no overlay rows outside Mode::Toc"
        );
    }

    #[test]
    fn test_dw_3_2_esc_returns_to_the_scroll_position_the_toc_was_opened_from() {
        let (_doc, _config, _engine, mut state) = build(&heading_source(8), 60, 4);
        for _ in 0..7 {
            state.handle_key_event(plain(KeyCode::Down));
        }
        let before = state.scroll();
        assert!(before > 0);

        state.handle_key_event(plain(KeyCode::Char('t')));
        // Move the selection around; none of it may move the document.
        for _ in 0..4 {
            state.handle_key_event(plain(KeyCode::Char('j')));
        }
        assert_eq!(state.scroll(), before, "browsing the TOC must not scroll");

        assert!(!state.handle_key_event(plain(KeyCode::Esc)));
        assert_eq!(state.mode(), Mode::Normal);
        assert_eq!(state.scroll(), before, "Esc restores the prior position");

        // `t` dismisses it too, from the same state.
        state.handle_key_event(plain(KeyCode::Char('t')));
        state.handle_key_event(plain(KeyCode::Char('j')));
        state.handle_key_event(plain(KeyCode::Char('t')));
        assert_eq!(state.mode(), Mode::Normal);
        assert_eq!(state.scroll(), before);
    }

    /// The plan's edge case: more headings than the screen has rows. The
    /// window must follow the selection, and the selected entry must be
    /// among the rows painted — otherwise `Enter` jumps somewhere the reader
    /// cannot see they chose.
    #[test]
    fn test_dw_3_2_a_toc_longer_than_the_screen_scrolls_to_keep_the_selection_visible() {
        let (_doc, _config, _engine, mut state) = build(&heading_source(30), 60, 5);
        state.handle_key_event(plain(KeyCode::Char('t')));

        // Walk the whole list one row at a time; at every step the selection
        // must be on screen and the window must be exactly five rows.
        for step in 0..30usize {
            let rows = state.toc_rows(5);
            assert_eq!(
                rows.len(),
                5,
                "step {step}: the window must fill the screen"
            );
            let selected: Vec<&TocRow> = rows.iter().filter(|row| row.selected).collect();
            assert_eq!(
                selected.len(),
                1,
                "step {step}: the selection must be visible"
            );
            assert!(
                selected[0].text.contains(&format!("Heading {step}")),
                "step {step}: the highlighted row must be the selected heading: {:?}",
                selected[0].text
            );
            state.handle_key_event(plain(KeyCode::Char('j')));
        }

        // `G`/`g` reach both ends, and the window follows.
        state.handle_key_event(plain(KeyCode::Char('G')));
        let rows = state.toc_rows(5);
        assert!(
            rows.last().unwrap().selected,
            "`G` selects the last heading"
        );
        assert!(rows.last().unwrap().text.contains("Heading 29"));

        state.handle_key_event(plain(KeyCode::Char('g')));
        let rows = state.toc_rows(5);
        assert!(rows[0].selected, "`g` selects the first heading");
        assert!(rows[0].text.contains("Heading 0"));
    }

    #[test]
    fn test_dw_3_2_t_reports_instead_of_opening_when_there_are_no_headings() {
        let (_doc, _config, _engine, mut state) = build(&non_reflowing_source(20), 40, 10);
        assert!(!state.handle_key_event(plain(KeyCode::Char('t'))));
        assert_eq!(
            state.mode(),
            Mode::Normal,
            "an empty TOC must not become a blank screen"
        );
        let message = state.status().message.expect("`t` must report why");
        assert!(message.contains("no headings"), "{message:?}");
        assert!(state.toc_rows(10).is_empty());
    }

    /// The plan's edge case: a terminal too short to render the overlay. Zero
    /// rows of content is a real viewport (a two-row terminal, one row of
    /// which is the status line), and it must produce no rows rather than
    /// panic on the window arithmetic.
    #[test]
    fn test_dw_3_2_a_viewport_too_short_for_the_overlay_paints_no_rows() {
        let (_doc, _config, _engine, mut state) = build(&heading_source(6), 40, 1);
        state.handle_key_event(plain(KeyCode::Char('t')));
        assert_eq!(state.mode(), Mode::Toc { selected: 0 });
        assert!(state.toc_rows(0).is_empty());
        assert_eq!(state.toc_rows(1).len(), 1, "one row still shows one entry");
        // ...and the overlay is still dismissable from a screen that shows
        // none of it.
        state.handle_key_event(plain(KeyCode::Esc));
        assert_eq!(state.mode(), Mode::Normal);
    }

    /// The TOC opens on the heading whose section the reader is in, not on
    /// the top of the document — so `t` then `Enter` leaves them where they
    /// were rather than teleporting them home.
    #[test]
    fn test_the_toc_opens_on_the_heading_the_reader_is_under() {
        let (_doc, _config, _engine, mut state) = build(&heading_source(8), 60, 4);
        let lines = heading_lines(&state);
        while state.scroll() < lines[4] + 1 {
            state.handle_key_event(plain(KeyCode::Down));
        }

        state.handle_key_event(plain(KeyCode::Char('t')));
        assert_eq!(state.mode(), Mode::Toc { selected: 4 });
    }

    /// An overlay that swallows the two keys every terminal user tries first
    /// is a trap. Both must still quit.
    #[test]
    fn test_q_and_ctrl_c_still_quit_from_inside_the_toc() {
        for key in [plain(KeyCode::Char('q')), ctrl('c')] {
            let (_doc, _config, _engine, mut state) = build(&heading_source(4), 40, 10);
            state.handle_key_event(plain(KeyCode::Char('t')));
            assert!(
                state.handle_key_event(key),
                "{key:?} must quit from the overlay"
            );
        }
    }

    /// A `--watch` reload while the TOC is open: `selected` indexes the
    /// outline of a document that no longer exists, and the new one may be
    /// shorter. Left alone, the overlay would highlight nothing the reader can
    /// see and `Enter` would resolve to no line at all.
    #[test]
    fn test_a_reload_reseats_an_open_toc_onto_the_new_documents_headings() {
        let (_doc, config, engine, mut state) = build(&heading_source(10), 60, 8);
        state.handle_key_event(plain(KeyCode::Char('t')));
        state.handle_key_event(plain(KeyCode::Char('G')));
        assert_eq!(state.mode(), Mode::Toc { selected: 9 });

        reload(&mut state, &heading_source(3), &config, &engine);

        assert_eq!(
            state.mode(),
            Mode::Toc { selected: 2 },
            "the selection must be clamped into the reloaded outline"
        );
        let rows = state.toc_rows(8);
        assert_eq!(rows.len(), 3, "the overlay must list the new document");
        assert!(
            rows[2].selected && rows[2].text.contains("Heading 2"),
            "a visible row must be highlighted: {rows:?}"
        );

        // ...and `Enter` still resolves to a real line of the new tree.
        state.handle_key_event(plain(KeyCode::Enter));
        assert_eq!(state.mode(), Mode::Normal);
        assert_eq!(
            state.scroll(),
            state
                .outline()
                .line_of(2)
                .expect("the third heading has a line")
                .min(state.max_scroll())
        );
    }

    #[test]
    fn test_a_reload_to_a_document_with_no_headings_dismisses_the_toc() {
        let (_doc, config, engine, mut state) = build(&heading_source(6), 60, 8);
        state.handle_key_event(plain(KeyCode::Char('t')));
        assert!(matches!(state.mode(), Mode::Toc { .. }));

        reload(&mut state, &non_reflowing_source(20), &config, &engine);

        assert_eq!(
            state.mode(),
            Mode::Normal,
            "an overlay with nothing to list is a blank screen the reader is \
             stuck on"
        );
        let message = state
            .status()
            .message
            .expect("the dismissal must say why it happened");
        assert!(message.contains("no headings"), "{message:?}");
        assert!(state.toc_rows(8).is_empty());
    }

    /// The outline is rebuilt with the tree, so its line indices always
    /// address the tree that is installed now. A width change that rewraps
    /// every heading is where a stale outline would show.
    #[test]
    fn test_the_outline_line_indices_follow_a_relayout() {
        let source: String = (0..12)
            .map(|i| {
                format!(
                    "## Heading {i} with enough words in its title to wrap at a narrow width\n\n\
                     body paragraph {i} carrying several words of its own\n\n"
                )
            })
            .collect();
        let (doc, config, engine, mut state) = build(&source, 90, 10);
        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };
        let wide = heading_lines(&state);

        state.apply_resize_burst(
            &ctx,
            &[Size {
                width: 30,
                height: 10,
            }],
        );
        let narrow = heading_lines(&state);
        assert_eq!(narrow.len(), wide.len(), "the heading count cannot change");
        assert_ne!(
            narrow, wide,
            "the fixture is broken: headings must move when they rewrap, or \
             this test cannot see a stale outline"
        );

        // Every recorded line must still be the first line of its heading in
        // the tree that is installed now — checked against the painted text,
        // not against another index.
        for (i, &line) in narrow.iter().enumerate() {
            assert!(
                line_text(&state, line).contains(&format!("Heading {i}")),
                "outline entry {i} points at line {line}, which reads {:?}",
                line_text(&state, line)
            );
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

    /// Lays out `source` into `state` as a `--watch` reload would, at the same
    /// width the tree already has.
    fn reload(state: &mut AppState, source: &str, config: &LayoutConfig, engine: &WidthEngine) {
        let doc = Document::parse(source);
        let ctx = LayoutContext {
            doc: &doc,
            config,
            engine,
            sizer: &NullSizer,
        };
        state.reload_document(
            &ctx,
            FileInfo {
                name: "test.md".to_string(),
                byte_size: source.len() as u64,
                line_count: source.lines().count(),
            },
        );
    }

    /// DW-2.2: a reload happens at an *unchanged layout width*, so the width
    /// comparison that stands in for "nothing moved" reports nothing moved,
    /// and the raw scroll offset is kept — which slides the reader by however
    /// many lines an edit above them added. Here a paragraph above the reader
    /// grows from one line to many; the block count is unchanged, so the
    /// anchor still names their block, and they must stay on it.
    ///
    /// This is the assertion the `document_changed` flag exists for: without
    /// it the offset is carried verbatim and the top line becomes a different
    /// paragraph.
    #[test]
    fn test_dw_2_2_a_reload_that_grows_a_block_above_the_reader_keeps_their_block() {
        let (_doc, config, engine, mut state) = build(&non_reflowing_source(40), 40, 10);
        for _ in 0..12 {
            state.handle_key_event(plain(KeyCode::Down));
        }
        let anchored = topmost_line_text(&state);
        assert_eq!(anchored, "line 6", "test setup: the reader is on line 6");
        let scroll_before = state.scroll();

        // The *first* paragraph rewraps from one line to many. Same 40
        // blocks, so every block below it keeps its identity.
        let grown = non_reflowing_source(40).replacen("line 0\n", &"word ".repeat(60), 1);
        reload(&mut state, &grown, &config, &engine);

        assert_eq!(
            topmost_line_text(&state),
            anchored,
            "the reader must still be looking at the same block after the reload"
        );
        assert!(
            state.scroll() > scroll_before,
            "their block moved down the document, so the offset must have grown: \
             {} vs {scroll_before}",
            state.scroll()
        );
    }

    /// A reload that only appends *below* the reader must not move them at
    /// all — the anchor resolves to the same line it already occupied.
    #[test]
    fn test_dw_2_2_a_reload_that_appends_below_the_reader_leaves_the_scroll_alone() {
        let (_doc, config, engine, mut state) = build(&non_reflowing_source(40), 40, 10);
        for _ in 0..12 {
            state.handle_key_event(plain(KeyCode::Down));
        }
        let anchored = topmost_line_text(&state);
        let scroll_before = state.scroll();

        let grown = format!("{}tail x\n\ntail y\n\n", non_reflowing_source(40));
        reload(&mut state, &grown, &config, &engine);

        assert_eq!(state.scroll(), scroll_before);
        assert_eq!(topmost_line_text(&state), anchored);
    }

    /// A reload that shrinks the document out from under a reader scrolled
    /// past its new end must clamp, not panic and not leave a scroll offset
    /// the painter would read past the tail.
    #[test]
    fn test_dw_2_2_a_reload_past_the_new_end_clamps_instead_of_dangling() {
        let (_doc, config, engine, mut state) = build(&non_reflowing_source(200), 40, 10);
        state.handle_key_event(plain(KeyCode::Char('G')));
        assert!(state.scroll() > 20, "test setup: the reader is deep in");

        reload(&mut state, &non_reflowing_source(3), &config, &engine);

        assert!(
            state.scroll() <= state.max_scroll(),
            "scroll {} must be clamped to max_scroll {}",
            state.scroll(),
            state.max_scroll()
        );
    }

    /// The empty-file edge: an editor that truncates before writing leaves a
    /// zero-byte document for one poll. It must lay out, clamp to 0, and
    /// still report a status line rather than panicking on an absent anchor.
    #[test]
    fn test_dw_2_4_a_reload_to_an_empty_document_survives_and_clamps_to_the_top() {
        let (_doc, config, engine, mut state) = build(&non_reflowing_source(40), 40, 10);
        state.handle_key_event(plain(KeyCode::Char('G')));

        reload(&mut state, "", &config, &engine);

        assert_eq!(state.scroll(), 0);
        assert_eq!(state.max_scroll(), 0);
        let _ = state.status();
    }

    /// A reload refreshes what `Ctrl-G` reports: the file grew, and the
    /// status message must say so rather than quoting the size it had at
    /// startup.
    #[test]
    fn test_dw_2_2_a_reload_refreshes_the_file_info_ctrl_g_reports() {
        let (_doc, config, engine, mut state) = build(&non_reflowing_source(4), 40, 10);
        let grown = non_reflowing_source(40);
        reload(&mut state, &grown, &config, &engine);

        state.handle_key_event(ctrl('g'));
        let status = state.status();
        let message = status.message.expect("Ctrl-G sets a message");
        assert!(
            message.contains(&format!("{} bytes", grown.len())),
            "status must quote the reloaded size, got {message:?}"
        );
    }

    /// The reload flag is one-shot: a width toggle immediately after a reload
    /// must go back to the cheap same-width path, not re-anchor forever.
    #[test]
    fn test_a_reload_does_not_leave_later_relayouts_permanently_re_anchoring() {
        let (_doc, config, engine, mut state) = build(&non_reflowing_source(40), 40, 10);
        for _ in 0..12 {
            state.handle_key_event(plain(KeyCode::Down));
        }
        reload(&mut state, &non_reflowing_source(40), &config, &engine);
        let scroll_after_reload = state.scroll();

        // A relayout at the same width, with the document unchanged: the
        // scroll offset must be carried verbatim.
        let doc = Document::parse(&non_reflowing_source(40));
        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };
        state.relayout_preserving_anchor(&ctx, config);
        assert_eq!(state.scroll(), scroll_after_reload);
    }

    /// The reviewer's reproduction, generalised: an author inserts a block
    /// *above* what the reader is looking at — the ordinary `--watch` edit —
    /// and the reader must still be looking at the same content afterwards.
    ///
    /// This is the case a positional `NodeId` anchor got wrong while
    /// *reporting success*: prepending shifts every id, `first_line_of` still
    /// answers `Some` for the shifted id, and the reader was silently moved
    /// into a different block. Measured before the fix, on the fence fixture
    /// below: 201 lines backwards, top line `"code 0"` instead of
    /// `"AFTER-THE-FENCE"`.
    ///
    /// Asserted on the painted text of the reader's row, not on a line number
    /// or a distance — a nearby line in the wrong block is exactly the failure
    /// being ruled out.
    #[test]
    fn test_dw_2_2_a_block_inserted_above_the_reader_still_leaves_them_on_their_own_block() {
        // Every shape the review measured drift for, plus a heading.
        let insertions = [
            ("one paragraph", "inserted paragraph\n\n".to_string()),
            ("one heading", "# Inserted Heading\n\n".to_string()),
            ("a bullet list", "- alpha\n- beta\n- gamma\n\n".to_string()),
            (
                "a small code fence",
                format!("```\n{}```\n\n", "code 0\ncode 1\ncode 2\n"),
            ),
            (
                "two hundred lines of fence",
                format!(
                    "```\n{}```\n\n",
                    (0..200).map(|i| format!("deep {i}\n")).collect::<String>()
                ),
            ),
        ];

        for (label, inserted) in insertions {
            // A long fence above the reader is what made the drift large: the
            // shifted id resolved to a 200-line block, so `place` put the
            // reader at its first line.
            let fence: String = (0..200).map(|i| format!("fence {i}\n")).collect();
            let body = format!(
                "intro paragraph\n\n```\n{fence}```\n\nAFTER-THE-FENCE\n\n{}",
                non_reflowing_source(60)
            );

            let (_doc, config, engine, mut state) = build(&body, 40, 10);
            // Park the reader exactly on the paragraph after the fence.
            let target = (0..state.tree().line_count())
                .find(|&line| {
                    let scroll = line;
                    state
                        .tree()
                        .lines(scroll..scroll + 1)
                        .any(|l| matches!(l, layout::Line::Items(items) if items.iter().any(|i| matches!(i, layout::LineItem::Run(r) if r.text.contains("AFTER-THE-FENCE")))))
                })
                .expect("fixture must contain the marker paragraph");
            for _ in 0..target {
                state.handle_key_event(plain(KeyCode::Down));
            }
            assert_eq!(
                topmost_line_text(&state).trim(),
                "AFTER-THE-FENCE",
                "{label}: test setup — the reader must start on the marker"
            );

            reload(&mut state, &format!("{inserted}{body}"), &config, &engine);

            assert_eq!(
                topmost_line_text(&state).trim(),
                "AFTER-THE-FENCE",
                "{label}: inserting above the reader must not move them off their block"
            );
        }
    }

    /// The mirror case the review also measured: a block *deleted* above the
    /// reader shifts ids the other way.
    #[test]
    fn test_dw_2_2_a_block_deleted_above_the_reader_still_leaves_them_on_their_own_block() {
        let body = format!(
            "first paragraph\n\nsecond paragraph\n\nMARKER-PARAGRAPH\n\n{}",
            non_reflowing_source(40)
        );
        let (_doc, config, engine, mut state) = build(&body, 40, 10);
        for _ in 0..4 {
            state.handle_key_event(plain(KeyCode::Down));
        }
        assert_eq!(topmost_line_text(&state).trim(), "MARKER-PARAGRAPH");

        let shrunk = body.replacen("first paragraph\n\n", "", 1);
        reload(&mut state, &shrunk, &config, &engine);

        assert_eq!(
            topmost_line_text(&state).trim(),
            "MARKER-PARAGRAPH",
            "deleting a block above the reader must not move them off their block"
        );
    }

    /// Duplicated text is ordinary in a document (a repeated `---`, an
    /// identical list item, a boilerplate line under every heading). The
    /// anchor must resolve to the copy the reader was actually on.
    ///
    /// The earlier version of this test used copies 21 blocks apart and
    /// prepended a single block, so the old nearest-ordinal tiebreak won by a
    /// margin of 20 and the test passed while the rule was wrong. It is
    /// replaced — not merely renamed — by a sweep whose spacing and insertion
    /// size actually cross the failure threshold: the ordinal rule breaks once
    /// the insertion exceeds half the spacing between copies, so `gap = 1`
    /// with 3 or 10 blocks prepended is squarely inside the broken region.
    /// Measured on the old rule, 10 of these 15 combinations moved the reader
    /// to a different copy, drifting up to 24 lines.
    #[test]
    fn test_dw_2_2_duplicated_content_re_anchors_to_the_copy_the_reader_was_on() {
        const DUPLICATE: &str = "the same boilerplate line";

        for gap in [1usize, 3, 10] {
            for inserted in [1usize, 3, 10] {
                // `gap` unique paragraphs, then an identical block, repeated.
                // The unique markers are what make "which copy" observable.
                let body: String = (0..20)
                    .flat_map(|group| {
                        (0..gap)
                            .map(move |i| format!("unique-{:03}\n\n", group * gap + i))
                            .chain(std::iter::once(format!("{DUPLICATE}\n\n")))
                    })
                    .collect();

                let (_doc, config, engine, mut state) = build(&body, 40, 10);

                // Park the reader on the 13th copy of the duplicated block.
                let copy_line = duplicate_lines(&state, DUPLICATE)[12];
                for _ in 0..copy_line {
                    state.handle_key_event(plain(KeyCode::Down));
                }
                assert_eq!(
                    topmost_line_text(&state).trim(),
                    DUPLICATE,
                    "gap={gap}: test setup — the reader must start on a duplicate"
                );
                // The unique marker just above the reader is the oracle: it
                // names *which* copy they are on, which the duplicated text
                // itself cannot.
                let marker_before = marker_above(&state);

                let grown = format!(
                    "{}{body}",
                    (0..inserted)
                        .map(|i| format!("prepended-{i}\n\n"))
                        .collect::<String>()
                );
                reload(&mut state, &grown, &config, &engine);

                assert_eq!(
                    topmost_line_text(&state).trim(),
                    DUPLICATE,
                    "gap={gap}, inserted={inserted}: reader must still be on a duplicate"
                );
                assert_eq!(
                    marker_above(&state),
                    marker_before,
                    "gap={gap}, inserted={inserted}: the reader must be on the SAME copy — \
                     the marker above them names which one"
                );
            }
        }
    }

    /// The line index of every line whose painted text is exactly `text`.
    fn duplicate_lines(state: &AppState, text: &str) -> Vec<usize> {
        (0..state.tree().line_count())
            .filter(|&line| line_text(state, line).trim() == text)
            .collect()
    }

    /// The nearest `unique-NNN` marker at or above the viewport top — the
    /// identity of the duplicated block the reader is sitting on.
    fn marker_above(state: &AppState) -> String {
        (0..=state.scroll())
            .rev()
            .map(|line| line_text(state, line))
            .find(|text| text.trim().starts_with("unique-"))
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    /// When the reader's own block is what changed, there is no block to
    /// return to — the anchor must *fail* and let the proportional fallback
    /// run, rather than confidently resolving to a neighbour.
    #[test]
    fn test_a_reload_that_rewrites_the_readers_own_block_falls_back_proportionally() {
        let body = format!("head\n\nEDIT-ME\n\n{}", non_reflowing_source(60));
        let (_doc, config, engine, mut state) = build(&body, 40, 10);
        // `head` on line 0, a blank separator on line 1, the marker on line 2.
        for _ in 0..2 {
            state.handle_key_event(plain(KeyCode::Down));
        }
        assert_eq!(topmost_line_text(&state).trim(), "EDIT-ME");
        let scroll_before = state.scroll();

        reload(
            &mut state,
            &body.replace("EDIT-ME", "COMPLETELY DIFFERENT TEXT"),
            &config,
            &engine,
        );

        // No panic, a valid position, and near where they were — the ratio
        // fallback's job. The point is that it *ran*.
        assert!(state.scroll() <= state.max_scroll());
        assert!(
            state.scroll().abs_diff(scroll_before) <= state.size().height as usize,
            "the fallback should keep the reader within a viewport of where they \
             were, got {} vs {scroll_before}",
            state.scroll()
        );
    }

    /// A message on the status row describes the document that produced it,
    /// so replacing the document must take it down — otherwise the reader is
    /// told something about a file that is no longer open.
    ///
    /// Both producers are covered here because the rule is about the
    /// mechanism, not about one message: `Ctrl-G`'s counts and a reload
    /// failure's reason are equally invalidated by a reload, and a third
    /// producer added later gets the same treatment for free.
    #[test]
    fn test_dw_2_4_a_reload_takes_down_a_status_message_about_the_old_document() {
        let (_doc, config, engine, mut state) = build(&non_reflowing_source(40), 40, 10);

        // The reload-failure shape: an external message set by the event loop.
        state.set_status(StatusMessage::new("reload failed: could not read file"));
        assert!(
            state.status().message.is_some(),
            "test setup: the message must be showing before the reload"
        );

        reload(&mut state, &non_reflowing_source(41), &config, &engine);

        assert_eq!(
            state.status().message,
            None,
            "a message about the previous document must not outlive it"
        );

        // The Ctrl-G shape: a message this type sets about its own file_info.
        state.handle_key_event(ctrl('g'));
        assert!(
            state.status().message.is_some(),
            "test setup: Ctrl-G sets one"
        );

        reload(&mut state, &non_reflowing_source(42), &config, &engine);

        assert_eq!(
            state.status().message,
            None,
            "Ctrl-G's byte and line counts describe the file that was open when it \
             was pressed, so a reload must take them down too"
        );
    }

    /// The clearing must be surgical: a reload takes down the *message*, not
    /// the status row. The permanent ruler has to come straight back.
    #[test]
    fn test_a_reload_leaves_the_permanent_ruler_intact() {
        let (_doc, config, engine, mut state) = build(&non_reflowing_source(40), 40, 10);
        state.set_status(StatusMessage::new("reload failed: something"));

        reload(&mut state, &non_reflowing_source(80), &config, &engine);

        let status = state.status();
        assert_eq!(status.message, None);
        assert_eq!(
            status.name, "test.md",
            "the ruler still names the document after a reload"
        );
        assert!(
            !status.render().contains("reload failed"),
            "the painted row must be the ruler, got {:?}",
            status.render()
        );
    }
}
