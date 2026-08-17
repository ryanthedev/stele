//! Viewport state: scroll position tracking, key-driven navigation, and
//! debounced resize/relayout — all pure and independently testable, so the
//! event loop in `main.rs` stays thin glue over real crossterm I/O.

use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::{Path, PathBuf};

use ast::{BlockKind, Document, NodeId, NodeRef};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use layout::{
    Chrome, FoldState, IntrinsicSizer, LayoutConfig, LayoutTree, Line, LineItem, Outline, Run,
    Semantic, StyleId, layout, layout_with_folds,
};
use width::WidthEngine;

use crate::explore::{EditBuffer, EditPlan, EntryKind, Listing, OverlayRow, RowStyle};
use crate::painter::{self, Page, SearchOverlay, Size, item_columns};

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

/// One literal match of the active query, addressed in the *laid-out* tree
/// so the painter can restyle exactly the cells it covers.
///
/// [`range`](Self::range) is a **byte** range measured from the start of
/// [`line`](Self::line)'s text, and it may run *past* that line's length —
/// which is how a match that straddles a wrap boundary is expressed. Layout
/// breaks a paragraph mid-match without asking; the match is still one
/// match, so it stays one `Match`, anchored at the line it starts on, and
/// the painter walks forward subtracting each line's length to find the
/// piece that falls on each row (DW-4.4). For the overwhelmingly common
/// case of a match inside one line, `range` is simply that line's local
/// byte range.
///
/// Bytes rather than cell columns, even though columns are what the reader
/// sees: bytes are what safely slice a `&str`, and restyling a match means
/// splitting runs. The column follows from the byte offset through the
/// width engine, which is the only oracle for it anyway; storing columns
/// would mean converting twice and re-deriving the byte offset regardless.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// The top-level source block the match falls inside. The one part of a
    /// match's address that survives a relayout — see
    /// [`AppState::relayout`], which recomputes the rest.
    pub block: NodeId,
    /// Tree line index the match *starts* on.
    pub line: usize,
    /// Byte range within the block's laid-out text, measured from the start
    /// of `line`.
    pub range: Range<usize>,
}

/// The active search: what was typed, where it matched, and which match the
/// reader is on. Outlives [`Mode::Search`] on purpose — `n`/`N` traverse the
/// last accepted query from normal mode, exactly like vim.
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    pub query: String,
    /// Every match, in tree order (ascending `line`, then `range.start`).
    pub matches: Vec<Match>,
    /// Index into `matches`. Meaningless — and never read — when `matches`
    /// is empty; [`AppState::search_overlay`] is what enforces that.
    pub current: usize,
    /// How many additional matches of `query` exist in the fully-expanded
    /// document but are currently invisible because they fall inside a
    /// folded section — the count [`AppState::report_no_matches`] needs to
    /// tell "no matches anywhere" apart from "matches, all folded away"
    /// (Phase 5's edge case: `n` must not report a query has no matches when
    /// it does). Kept only as fresh as the last [`AppState::recompute_matches`]
    /// call, which is every relayout — including the one a fold or unfold
    /// itself triggers — so it is exactly the source of the bug it exists to
    /// prevent. Left at `0` by [`AppState::refresh_incremental`] (typing has
    /// no [`LayoutContext`] to recompute it with — see that method's doc) and
    /// by `Default`, so a fresh or mid-typing query never claims a count it
    /// cannot back.
    pub hidden_by_folds: usize,
}

impl SearchState {
    /// Whether the query is case-sensitive, by vim's smart-case rule: an
    /// all-lowercase query matches either case, and one uppercase character
    /// anywhere in it makes the whole query exact (DW-4.2).
    ///
    /// `char::is_uppercase` rather than `is_ascii_uppercase`, so the rule
    /// reads the same way for a reader typing `Ärger` as for one typing
    /// `Error`.
    fn case_sensitive(&self) -> bool {
        self.query.chars().any(char::is_uppercase)
    }

    /// The status-row text while the prompt is open: the query as typed,
    /// plus where the reader is in the results — or that there are none
    /// (DW-4.5).
    fn prompt(&self) -> String {
        if self.query.is_empty() {
            return "/".to_string();
        }
        if self.matches.is_empty() {
            return format!("/{} — no matches", self.query);
        }
        format!(
            "/{}  [{}/{}]",
            self.query,
            self.current + 1,
            self.matches.len()
        )
    }
}

/// What the viewer is showing, and therefore what a key means.
///
/// Deliberately a flat enum matched at the top of
/// [`AppState::handle_key_event`], not an overlay stack: there is exactly one
/// overlay and it is modal, so a stack would be a general mechanism with one
/// user and two states it can be in that this cannot express (two overlays
/// open, and an empty stack that is not `Normal`).
///
/// Every match on this type is exhaustive with no wildcard arm, so a new mode
/// is a compile error everywhere it must be handled rather than a silently
/// missing case — including at [`Mode::captures_all_keys`], which is the one
/// question a mode cannot answer from inside itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Reading the document. Every pre-existing binding means what it did.
    Normal,
    /// The full-screen table of contents, with `selected` indexing
    /// [`Outline::entries`].
    Toc { selected: usize },
    /// Reading a query into the status row. `origin` is the scroll position
    /// `/` was pressed at — incremental search moves the viewport as the
    /// reader types, and `Esc` has to be able to undo all of it (DW-4.1).
    /// It rides on the variant rather than on [`AppState`] so the datum
    /// cannot outlive the mode that gives it meaning.
    Search { origin: usize },
    /// Cycling the links visible in the viewport (DW-6.1), with `index`
    /// indexing [`AppState::visible_links`].
    ///
    /// The index is deliberately *not* accompanied by a cached link list.
    /// Links are recomputed from `(tree, scroll)` every time they are asked
    /// for, so neither a `--watch` reload nor a resize can leave this mode
    /// addressing a link in a tree that no longer exists — the worst either
    /// can do is index past the end, which [`AppState::reseat_link_select`]
    /// clamps on the same relayout hook [`AppState::reseat_search`] uses.
    LinkSelect { index: usize },
    /// The full-screen directory explorer (Phase 3), with `selected` indexing
    /// [`AppState::explore`]'s [`crate::explore::Listing::entries`] and
    /// `rooted` saying whether there is a real document behind it to reveal.
    ///
    /// `rooted` rides on the variant for the same reason `Search { origin }`
    /// does: the datum must not outlive the mode it gives meaning to. It is
    /// `true` exactly when this explorer is the *reason* the process is
    /// running — `stele` with no argument, or `stele <dir>` — so there is no
    /// previously-open document for `Esc`/`q` to reveal (DW-3.15). It starts
    /// `false` and stays `false` for every explorer opened with `_` from a
    /// real document, and — once set at the root of a rooted launch — is
    /// carried unchanged through every re-list `Enter`/`-` produce, however
    /// deep the reader descends, because the *reason* the process is running
    /// does not change on the way down.
    ///
    /// The listing itself cannot ride on the variant: `Mode` is `Copy`, and
    /// [`crate::explore::Listing`] owns a [`std::path::PathBuf`] per entry.
    /// It lives in [`AppState::explore`] instead, alongside this index.
    Explore { selected: usize, rooted: bool },
}

impl Mode {
    /// Whether this mode reads the keyboard *whole*: every key is its own,
    /// and no layer above it may act on a key before it does.
    ///
    /// **This exists to be asked by code that is not in this file.** The
    /// event loop in `main.rs` runs a chrome table (`+`, `-`, `T`) *before*
    /// [`AppState::handle_key_event`], because those keys need a `Painter`
    /// and a `LayoutContext` that `AppState` does not own. A key that table
    /// claims never reaches `handle_key_event`, so no guard inside this type
    /// can defend against it.
    ///
    /// Three phases found that independently, from three directions: a `T`
    /// read during a resize drain vanished (Phase 2), `+`/`-`/`T` relaid out
    /// a document the TOC overlay was not showing (Phase 3), and `/The`
    /// searched for `he` while swapping the theme (Phase 4). Phase 3 fixed
    /// its instance with a `mode() != Mode::Normal` gate in `main.rs`, which
    /// was correct and would have kept being correct until the next mode
    /// forgot to be added to it — in a file that mode does not own.
    ///
    /// Answering the question here inverts that obligation. The match is
    /// exhaustive with no wildcard arm, so `Mode::LinkSelect` (Phase 6) is a
    /// compile error at this line until its author states whether their mode
    /// owns the keyboard — the same discipline the `Semantic` style tables
    /// use, applied to the one seam a mode cannot police from inside itself.
    ///
    /// A mode answering `true` takes on the whole obligation: every key,
    /// including ones another layer would like to claim, is handled by that
    /// mode or deliberately ignored by it.
    pub fn captures_all_keys(self) -> bool {
        match self {
            Mode::Normal => false,
            // The overlay reads keys of its own (`j`/`k`, `Enter`, `Esc`) and
            // deliberately ignores the rest. `+`/`-`/`T` are inert while it
            // is up rather than relaying out or re-theming a document the
            // reader cannot see — Phase 3's rule, unchanged, now stated by
            // the mode instead of by the chrome table.
            Mode::Toc { .. } => true,
            // Every printable key is a character of the query (DW-4.1); the
            // rest edit it or end it.
            Mode::Search { .. } => true,
            // `Tab`/`Shift-Tab`/`Enter`/`Esc` are the mode's own, and the
            // rest are deliberately ignored (DW-6.1). `+`/`-`/`T` are inert
            // for a reason narrower than the TOC's: a relayout rewraps every
            // line, which is what `Mode::LinkSelect { index }` indexes into.
            // The reader would see the indicator jump to a different link on
            // a keystroke that was never about links.
            Mode::LinkSelect { .. } => true,
            // The explorer's own key table reads `j`/`k`/`Enter`/`Esc`/`-`/
            // `q` and deliberately ignores the rest — including `-`, which
            // `chrome_action` would otherwise claim as `ChromeAction::Narrow`
            // before this method is ever consulted. Answering `true` here is
            // the entire reason `-` can mean "up a directory" inside the
            // explorer at all (DW-3.6).
            Mode::Explore { .. } => true,
        }
    }
}

/// One contiguous range of runs, on one line, that paints part of a link.
///
/// A link is several runs whenever its text carries a style change or crosses
/// a wrap boundary, so a span is a *range* of item indices and a link is a
/// list of spans. The painter needs exactly this to draw the selection
/// indicator, and the mouse hit-test needs exactly this to answer "is there a
/// link under this cell".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkSpan {
    /// Absolute line index in the layout tree, not a viewport row.
    pub line: usize,
    /// Index of the first [`LineItem`] of this span on that line.
    pub first_item: usize,
    /// Index of the last, inclusive.
    pub last_item: usize,
}

/// One link the reader can currently see and select.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleLink {
    /// The raw destination as written in the document — untrusted, and not
    /// validated here. `crate::link` is the barricade; this is enumeration.
    pub target: String,
    /// The link's visible text, for the status row.
    pub text: String,
    /// Every run range this link paints into, in viewport order.
    pub spans: Vec<LinkSpan>,
}

/// Something the key/mouse tables decided to do that needs resources
/// [`AppState`] deliberately does not have: the filesystem, a child process,
/// or the terminal.
///
/// The event loop drains exactly one of these after every event via
/// [`AppState::take_action`]. It is the same "return the decision as a value"
/// shape [`ChromeAction`] uses, and for the same reason Phase 4 gives there:
/// a decision that lives in `main.rs` lives in the one file no test can
/// reach. Here it additionally keeps `std::process` and `std::fs` out of this
/// module entirely, so the whole interaction layer is drivable from a unit
/// test with no terminal, no document on disk, and no browser opening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingAction {
    /// Follow this raw, unvalidated destination (DW-6.1, DW-6.6).
    OpenLink(String),
    /// `Backspace`: return to the previous document (DW-6.2).
    Back,
    /// `y`: copy the code block in view to the clipboard (DW-6.7).
    CopyCodeBlock,
    /// `m`: turn mouse capture on or off (DW-6.6).
    SetMouseCapture(bool),
    /// `_` from [`Mode::Normal`]: open the explorer at the open document's
    /// directory (Phase 3, DW-3.1).
    ///
    /// Carries no path, unlike [`PendingAction::ListDirectory`] below —
    /// `AppState` performs no I/O and, unlike once a [`crate::explore::Listing`]
    /// is already held, has no way to *name* the open document's directory
    /// either: that lives in `Session::source`, in `main.rs`. The event loop
    /// resolves it (`session.source.base_dir()`, which already answers the
    /// current working directory for a document with no directory of its
    /// own — a stdin document included) and seats the selection on the open
    /// document's own row.
    OpenExplorer,
    /// Re-list this directory into the explorer (Phase 3, DW-3.4): `Enter`
    /// on a directory row, `Enter` on `../`, or `-`. Unlike
    /// [`PendingAction::OpenExplorer`], `AppState` already holds a
    /// [`crate::explore::Listing`] by the time this is queued, so it can
    /// name the exact target path itself — the entry's own recorded
    /// [`crate::explore::Entry::path`], never reconstructed from display
    /// text (which is lossy for a non-UTF8 name).
    ListDirectory(PathBuf),
    /// `Enter` on a document row (Phase 3, DW-3.3): open this path through
    /// [`crate::link::Navigator::open_path`], the same non-lossy path as
    /// [`PendingAction::ListDirectory`].
    OpenPath(PathBuf),
    /// `y` at the confirmation gate (Phase 4): carry out this plan.
    ///
    /// **The only way a write can happen**, and it exists for the reason
    /// every other variant here does — `AppState` performs no I/O — but with
    /// a second job on top. Because the plan is a *value* that only reaches
    /// `main.rs` through this queue, and because
    /// [`AppState::handle_confirm_key`] is the only thing that queues it,
    /// "no filesystem operation occurs until the confirmation is accepted"
    /// (DW-4.3) is a structural property: a rejected confirmation drops the
    /// plan without ever constructing this action.
    ApplyEdits(EditPlan),
}
/// What the status row says when a heading motion or the TOC has nothing to
/// work with. One constant, because a document with no headings must answer
/// the same way whichever key asked (DW-3.1, and the overlay's edge case).
const NO_HEADINGS: &str = "no headings in this document";

/// What a rejected confirmation says (Phase 4, DW-4.3). States the outcome —
/// *nothing* changed — rather than merely that the question went away, because
/// "cancelled" alone leaves a reader wondering how much of it ran.
const WRITE_CANCELLED: &str = "write cancelled: nothing on disk was changed";

/// `w` over a buffer that asks for nothing.
const NOTHING_TO_WRITE: &str = "no changes to write";

/// `Enter` or `-` with unsaved edits: leaving would throw them away.
const UNSAVED_EDITS: &str = "unsaved edits: w writes them, Esc discards them";

/// `Esc` over a dirty buffer.
const EDITS_DISCARDED: &str = "edits discarded: nothing on disk was changed";

/// `z`'s answer when the document *has* headings but the cursor is above all
/// of them (`Outline::index_at_or_before` returns `None` for that reason
/// too, not only for an empty outline — see [`AppState::toggle_fold`]).
/// Distinct from [`NO_HEADINGS`] on purpose: that message is false here —
/// the document is not headingless, there is simply no section covering the
/// preamble the reader is currently on.
const NO_SECTION_HERE: &str = "no section to fold here";

/// A key press the event loop must act on with resources [`AppState`] does
/// not own: `+`/`-` need a [`LayoutContext`] to relay out against, and `T`
/// needs the [`crate::painter::Painter`] whose decor it swaps.
///
/// **Why this is a value and not a call.** `main.rs` used to hold both the
/// decision (*is this key chrome?*) and the action (*do the thing*), and its
/// own module doc says it should hold neither — "all decision logic lives in
/// the library; this file is thin glue over real crossterm I/O and is not
/// itself unit-tested". The consequence was not theoretical: because the
/// decision lived in the one file no test can reach, nothing noticed that it
/// ran *before* [`AppState::handle_key_event`] and swallowed `T`, `+` and `-`
/// out of a search query. Two review gates passed over it.
///
/// Splitting the decision out returns `main.rs` to the glue it claims to be
/// and puts the part that can be wrong under test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeAction {
    /// `+` — widen the content column (DW-1.4).
    Widen,
    /// `-` — narrow it.
    Narrow,
    /// `T` — swap the built-in theme variant (DW-1.5).
    ToggleTheme,
    /// `z` — fold or unfold the heading at or above the cursor (DW-5.1).
    /// Needs a relayout exactly as `Widen`/`Narrow` do, which is why this
    /// lives here rather than in `handle_key_event`'s own table.
    ToggleFold,
    /// `R` — open every fold (DW-5.4).
    ExpandAllFolds,
    /// `M` — collapse every heading's section (DW-5.4).
    CollapseAllFolds,
    /// `#` — show or hide the line-number gutter.
    ///
    /// A chrome action rather than an ordinary key because the gutter takes
    /// cells the document was laid out into: turning it on rewraps the page,
    /// exactly as `-` does, and that needs the `ctx` only the event loop has.
    ToggleLineNumbers,
}

/// What the status row says when `Tab` has nothing to select, or when a
/// reload leaves an open selection with nothing under it (DW-6.1).
const NO_LINKS: &str = "no links in view";

/// What `y` says when the viewport holds no code block (DW-6.7).
const NO_CODE_BLOCK: &str = "no code block in view";

/// Lines one wheel notch scrolls (DW-6.6). Three is the conventional
/// terminal/pager step: one line is imperceptible against the effort of
/// turning the wheel, a full page overshoots what the reader was tracking.
const WHEEL_LINES: isize = 3;

/// The viewer's mutable state: the current layout tree, scroll offset, and
/// viewport size. Navigation and resize are pure state transitions, driven
/// directly from tests without a real terminal or timers.
pub struct AppState {
    tree: LayoutTree,
    scroll: usize,
    /// **The reading line**: an absolute index into [`AppState::tree`], not a
    /// viewport row.
    ///
    /// A pager has no cursor, so before anything could be highlighted one had
    /// to be invented. This is it — the line every motion actually addresses,
    /// with the viewport following it rather than the other way round. The
    /// change of model is real and worth stating: `j` used to scroll and now
    /// moves the reader, and a document taller than the screen does not budge
    /// until the reading line reaches the bottom of it.
    ///
    /// It exists whether or not it is painted (`current_line = false` in a
    /// theme hides the band and changes no motion), because a key that means
    /// different things depending on a display setting is worse than either
    /// meaning. Absolute rather than a viewport row so scrolling cannot slide
    /// it onto different text, which is the same rule the search and
    /// link-selection overlays follow.
    cursor: usize,
    size: Size,
    /// The furniture the theme asked for: padding, gutter, band, scrolloff.
    /// Held here rather than passed per-frame because it decides the *layout*
    /// width and the page height, so navigation arithmetic needs it as much as
    /// painting does.
    chrome: Chrome,
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
    search: SearchState,
    /// Which sections are collapsed, keyed by heading [`NodeId`] (Phase 5).
    /// Consulted by every [`AppState::relayout`] via `layout::layout_with_folds`.
    folds: FoldState,
    /// Set by [`AppState::toggle_fold`] when the viewport is inside the
    /// section that is about to fold, so the next [`AppState::relayout`]
    /// snaps the reader to the marker line instead of trusting the ordinary
    /// block anchor — which cannot find a block that no longer emits a line
    /// of its own once it is inside a collapsed range (DW-5.5). `None` for
    /// every other relayout path (`widen`, `narrow`, a theme swap, a reload,
    /// a resize, an unfold, `expand_all`, `collapse_all`), all of which are
    /// well served by the ordinary anchor.
    pending_fold_snap: Option<NodeId>,
    /// The one action the event loop still owes the OS, drained by
    /// [`AppState::take_action`]. At most one is outstanding: every producer
    /// is a single key or click, and the loop drains after each.
    action: Option<PendingAction>,
    /// Whether mouse reporting is on (DW-6.6). Mirrored rather than owned —
    /// the terminal holds the real state — so `m` can report which way it
    /// went without the event loop having to tell this type back.
    mouse_capture: bool,
    /// The directory listing behind [`Mode::Explore`], read by `Session` and
    /// handed over whole by [`AppState::install_listing`] (Phase 3).
    ///
    /// Holding an already-read [`crate::explore::Listing`] is not the I/O
    /// this type forbids itself — only [`crate::explore::Listing::read`] is,
    /// and nothing here calls it (see `app.rs`'s own no-I/O module note,
    /// and DW-3.4's dedicated source-text test). Every method that acts on
    /// this field — `next_selectable`, `prev_selectable`, `entries`, `dir`,
    /// `rows` — is a pure projection over data `Session` already gathered.
    /// `None` outside [`Mode::Explore`].
    explore: Option<Listing>,
    /// Where `Esc` puts the reader back when an *unrooted* explorer closes
    /// (DW-3.5) — the same `toc_return_scroll` idiom, captured on the way
    /// in and left untouched across every re-list a re-listed `Enter`/`-`
    /// produces.
    explore_return_scroll: usize,
    /// The editable buffer over [`AppState::explore`] (Phase 4), or `None`
    /// while the explorer is read-only — which it is until the reader
    /// presses `i`, `o`, `d` or `w`.
    ///
    /// **Dropped by [`AppState::install_listing`], unconditionally.** That is
    /// not tidiness: the buffer's lines carry indices into
    /// [`AppState::explore`]'s entries, so a buffer that outlived its listing
    /// would name the wrong files, and the diff would happily rename or
    /// delete them. Making the drop part of the one seam that installs a
    /// listing is what turns that into an impossibility rather than a rule.
    explore_edit: Option<EditSession>,
}

/// The reader's in-progress edit of a directory (Phase 4).
///
/// Three states in one struct rather than three [`Mode`] variants, because
/// `Mode` is `Copy` and every one of these fields owns heap data — and
/// because the confirmation gate is not a different *mode* to a reader, it is
/// a question the explorer is asking.
#[derive(Debug)]
struct EditSession {
    buffer: EditBuffer,
    /// The row the reader is typing into, if any.
    editing: Option<usize>,
    /// The plan waiting for a `y`.
    ///
    /// **The gate.** While this is `Some`, exactly three keys mean anything
    /// (`y`, `n`, `Esc`) and every other key — `Enter` emphatically included
    /// — is inert, so no reflexive keystroke can confirm a delete.
    pending: Option<EditPlan>,
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
            cursor: 0,
            size,
            chrome: Chrome::default(),
            mode: Mode::Normal,
            pending: None,
            toc_return_scroll: 0,
            content_width,
            file_info,
            status_message: None,
            document_changed: false,
            search: SearchState::default(),
            folds: FoldState::default(),
            pending_fold_snap: None,
            action: None,
            mouse_capture: true,
            explore: None,
            explore_return_scroll: 0,
            explore_edit: None,
        }
    }

    pub fn tree(&self) -> &LayoutTree {
        &self.tree
    }

    /// What the viewer is showing, and which key table is live — the event
    /// loop's cue for which painter entry point to call, and (for
    /// `Mode::Search`) that the status row is a query prompt rather than the
    /// ruler.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The document's headings, in document order.
    pub fn outline(&self) -> &Outline {
        self.tree.outline()
    }

    /// Which sections are currently folded (Phase 5).
    /// Test-only: fold one named section directly. The shipped API offers
    /// `toggle_fold` (which picks the heading at or above the cursor),
    /// `expand_all` and `collapse_all`; a test that needs *this* heading
    /// folded and no other would otherwise have to drive the cursor there
    /// first, which is a different thing to be testing.
    #[cfg(test)]
    fn folds_mut_for_test(&mut self) -> &mut FoldState {
        &mut self.folds
    }

    pub fn folds(&self) -> &FoldState {
        &self.folds
    }

    /// Which [`ChromeAction`] `key` requests, or `None` when the key is not
    /// chrome — and `None` for **every** key while the current mode owns the
    /// keyboard.
    ///
    /// This is the whole of the routing decision the event loop used to make
    /// for itself, moved to where the mode lives and where a test can reach
    /// it. The order of the two guards is the part that was wrong: the mode
    /// question has to be asked first, because a key claimed as chrome never
    /// reaches [`AppState::handle_key_event`] and so never meets that
    /// method's own "while the prompt is open, every key is text" guard.
    /// `/The` searched for `he` and flipped the theme on the way through.
    ///
    /// Ordinary navigation is deliberately not here: it needs nothing beyond
    /// `AppState` and belongs on `handle_key_event` with the rest of the key
    /// table. Only the keys that genuinely need a painter or a layout context
    /// have to make this trip.
    pub fn chrome_action(&self, key: KeyEvent) -> Option<ChromeAction> {
        if self.mode.captures_all_keys() {
            return None;
        }
        // A chord is not the bare key: `Ctrl-T` must keep falling through to
        // whatever `handle_control_chord` makes of it, not toggle the theme.
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return None;
        }
        match key.code {
            KeyCode::Char('+') => Some(ChromeAction::Widen),
            KeyCode::Char('-') => Some(ChromeAction::Narrow),
            KeyCode::Char('T') => Some(ChromeAction::ToggleTheme),
            KeyCode::Char('z') => Some(ChromeAction::ToggleFold),
            KeyCode::Char('R') => Some(ChromeAction::ExpandAllFolds),
            KeyCode::Char('M') => Some(ChromeAction::CollapseAllFolds),
            KeyCode::Char('#') => Some(ChromeAction::ToggleLineNumbers),
            _ => None,
        }
    }

    /// The active search — query, matches, and which one the reader is on.
    /// Read-only from outside this type: `matches` and `current` are only
    /// consistent because every mutation here keeps them so.
    pub fn search(&self) -> &SearchState {
        &self.search
    }

    /// What the painter needs to highlight this frame (DW-4.4). `current`
    /// is `None` — no distinct-styled match — exactly when there is nothing
    /// to be current *among*, which is what keeps the painter from having
    /// to re-check the index it is handed.
    pub fn search_overlay(&self) -> SearchOverlay<'_> {
        SearchOverlay {
            matches: &self.search.matches,
            current: (self.search.current < self.search.matches.len())
                .then_some(self.search.current),
        }
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
    /// once. Hence the callers: [`AppState::reload_document`], the one place
    /// a document is replaced, and [`AppState::build_edit_plan`], where
    /// arming a confirmation invalidates every account of the edit that
    /// preceded it.
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
        let message = match self.mode {
            // The query prompt owns the row for as long as it is open
            // (DW-4.1), and it does not age a transient message on its way
            // past: a `Ctrl-G` set just before `/` should still have its
            // remaining frames when the reader escapes back out.
            Mode::Search { .. } => Some(self.search.prompt()),
            // The TOC paints this same row (see `main.rs::paint`), and a
            // transient message has to keep ageing while the overlay is up or
            // a `Ctrl-G` set just before `t` would hang on screen for as long
            // as the reader browsed. Link selection is the same channel by
            // design: `announce_selection` sets an ordinary transient message
            // naming the destination, so it ages, and a reader who leaves the
            // indicator up while reading is not stuck looking at a URL.
            Mode::Normal | Mode::Toc { .. } | Mode::LinkSelect { .. } => {
                self.take_transient_message()
            }
            // The listed directory owns the row while the explorer is open
            // (DW-3.10) — never the hidden document's name and scroll
            // percentage, which `position_pct`/`file_info.name` below would
            // otherwise supply. A transient message (a barricade refusal or
            // a re-list failure, DW-3.8) still ages and takes priority over
            // it for exactly as long as its TTL runs, the same rule the TOC
            // arm above follows; once it expires this reverts to the
            // directory rather than to the ordinary ruler.
            //
            // **Except over an armed confirmation, which outranks
            // everything.** A pending plan is the only state in which a bare
            // `y` destroys files, so the question naming what it destroys
            // must be the text on the row — not merely the text that gets
            // there when nothing else happens to be showing. `w`, `n`, `w`
            // put a live `WRITE_CANCELLED` in front of a re-armed DELETE for
            // its whole TTL and `y` still ran it. [`AppState::build_edit_plan`]
            // clears the row when it arms; this makes that unforgeable
            // rather than a convention every future caller must remember.
            // The transient is still taken, so it ages underneath the
            // question and cannot pop back once the gate closes.
            Mode::Explore { .. } => {
                let transient = self.take_transient_message();
                Some(match self.pending_confirmation() {
                    Some(question) => question,
                    None => transient.unwrap_or_else(|| self.explore_status_text()),
                })
            }
        };
        StatusLine {
            position_pct: self.position_pct(),
            name: self.file_info.name.clone(),
            message,
        }
    }

    /// The question an armed plan is asking, if one is armed.
    ///
    /// The single source of truth for "is the reader at the gate", read by
    /// [`AppState::status`] to give that question precedence over any
    /// transient message.
    fn pending_confirmation(&self) -> Option<String> {
        Some(self.explore_edit.as_ref()?.pending.as_ref()?.confirmation())
    }

    /// The explorer's default status text: the listed directory. Always
    /// `Some` from [`AppState::status`]'s `Mode::Explore` arm, so
    /// `position_pct`/`name` below are never actually rendered — see
    /// [`StatusLine::render`].
    ///
    /// While an edit is in progress (Phase 4) the row carries the dirty
    /// indicator instead, and while a plan is waiting it carries the
    /// confirmation question — which is the *only* place that question
    /// appears, so a reader can never be at the gate without seeing it.
    /// [`AppState::status`] reaches the gate before this function is
    /// consulted; the arm here keeps the answer right for a caller that does
    /// not, rather than leaving a second definition of it to drift.
    fn explore_status_text(&self) -> String {
        if let Some(question) = self.pending_confirmation() {
            return question;
        }
        if let Some(session) = &self.explore_edit {
            // The indicator leads and the directory trails, which is the
            // opposite of the read-only row and is not a preference: the
            // status row is one terminal line and the painter clips it.
            // A pty test found the whole indicator — the dirty state and
            // both the keys that resolve it — clipped off the end by an
            // ordinary-length directory path. While an edit is in
            // progress, what the reader needs is "you have unsaved
            // changes, `w` writes and `Esc` discards"; the directory is
            // the part that can afford to be cut.
            return format!(
                "{}  {}",
                session.buffer.indicator(),
                self.explore_dir().unwrap_or(Path::new("")).display()
            );
        }
        match self.explore_dir() {
            Some(dir) => dir.display().to_string(),
            // Reachable only mid-startup, before the first
            // `install_listing` call — `main.rs` never paints a frame in
            // that window, but the answer has to exist regardless of
            // whether anything today reads it.
            None => String::new(),
        }
    }

    /// The transient message, aged by one frame. Split out of
    /// [`AppState::status`] so the search prompt's arm reads as "the prompt
    /// instead of the message" rather than "the prompt, and also do not run
    /// the aging code".
    fn take_transient_message(&mut self) -> Option<String> {
        let (text, ttl) = self.status_message.take()?;
        if ttl > 1 {
            self.status_message = Some((text.clone(), ttl - 1));
        }
        Some(text)
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
    ///
    /// `old_doc` is the document `self.tree` was built from *before* this
    /// call — the same `Rc<Document>` the caller is about to replace, still
    /// alive because the caller has not dropped its last reference yet.
    /// `None` when the caller has no such reference to offer (chiefly test
    /// helpers that never fold); a live fold is then dropped rather than
    /// guessed at — see [`AppState::reseat_folds`].
    pub fn reload_document(
        &mut self,
        ctx: &LayoutContext,
        file_info: FileInfo,
        old_doc: Option<&Document>,
    ) {
        self.file_info = file_info;
        // Cleared for the reason the doc above gives — but only when the row
        // is the *document's* to speak on. While the explorer is open it is
        // not: DW-3.10 gives the status row to the listing, and the message
        // sitting on it is a barricade refusal or a re-list failure that
        // describes some other file entirely. `stele --watch notes.md`, `_`,
        // `Enter` on a refused file, and the very next quarter-second tick
        // that found `notes.md` changed erased the reason the reader had
        // just asked for. The message that outlives its document is the
        // hazard here; a message that never described that document was
        // never in scope.
        if !matches!(self.mode, Mode::Explore { .. }) {
            self.clear_status();
        }
        self.document_changed = true;
        // Captured before clearing: `self.folds.collapsed` is about to be
        // emptied, and this is the only record of which ids to re-key.
        let old_collapsed = self.folds.collapsed.clone();
        let had_folds = !old_collapsed.is_empty();
        // A complete (fold-free) outline of the *old* document — see
        // `reseat_folds` for why `self.tree.outline()` cannot serve this
        // purpose when any fold was active. Built once, before anything
        // about `self` changes, and only when there is actually something to
        // re-key: an extra full layout pass is not free, so it is not paid
        // for on the (overwhelmingly common) reload with nothing folded.
        let old_outline = if had_folds {
            old_doc.map(|doc| {
                layout(doc, self.tree.width(), ctx.config, ctx.engine, ctx.sizer)
                    .outline()
                    .clone()
            })
        } else {
            None
        };
        // A `NodeId` from the old document cannot be trusted against the new
        // one at all — see `reseat_folds` — and worse than merely stale, a
        // coincidentally-equal id in the fresh parse would make `fold_range`
        // (`layout::block::Ctx`) collapse a section that was never folded.
        // Clearing first means the very first relayout below is guaranteed
        // fold-free, so `reseat_folds` has a clean, fully-expanded `Outline`
        // on the *new* side too, matching `old_outline`'s.
        if had_folds {
            self.folds.collapsed.clear();
        }
        self.relayout_preserving_anchor(ctx, *ctx.config);
        if let Some(old_outline) = old_outline {
            self.reseat_folds(&old_outline, &old_collapsed);
            if !self.folds.collapsed.is_empty() {
                // A second pass to actually apply the re-keyed folds. Costs
                // one extra `layout_with_folds` call, only when there was
                // something to re-seat, and only on the reload path — never
                // per keystroke or per frame.
                self.relayout_preserving_anchor(ctx, *ctx.config);
            }
        }
        self.reseat_toc();
    }

    /// Re-seats fold state across a `--watch` reload (DW-5.2). A `NodeId` is
    /// positional and reload reparses (see [`AppState::line_of_reloaded`]),
    /// so a fold recorded against the old document's ids would silently
    /// address whatever now happens to occupy that slot in the new one — or
    /// nothing at all.
    ///
    /// Re-keyed by content plus occurrence, the same principle
    /// [`AppState::line_of_reloaded`] uses for the scroll anchor, but over
    /// flattened heading text rather than painted lines: a folded heading's
    /// own line paints its *marker*, not its title, so hashing what is
    /// currently on screen would compare a summary against a title (or a
    /// title against a summary) and could never match.
    ///
    /// **Both `old_outline` and `self.tree.outline()` (the "new" side, read
    /// below) must be complete — every heading, none of them abbreviated by
    /// a fold.** This is the fix for a real defect: an occurrence index is
    /// only comparable between two lists that agree on what they are
    /// counting *among*. `walk_blocks` never visits a heading nested inside
    /// a folded ancestor's range, so `self.tree.outline()` *while a fold is
    /// active* silently omits it — and if `old_outline` had been taken from
    /// that abbreviated list, "the 1st visible `Notes`" on the old side and
    /// "the 1st `Notes` overall" on the new side can name two different
    /// headings, or the omitted one can vanish from `collapsed` entirely
    /// (both reproduced and now regression-tested). `reload_document` pays
    /// for `old_outline`'s completeness with an extra fold-free layout pass
    /// of the old document; `self.tree.outline()` here is already fold-free
    /// too, because `reload_document` clears every fold before the relayout
    /// that installs `self.tree`.
    fn reseat_folds(&mut self, old_outline: &Outline, old_collapsed: &HashSet<NodeId>) {
        let wanted: Vec<(u8, &str, usize)> = old_collapsed
            .iter()
            .filter_map(|&id| {
                let index = old_outline.entries.iter().position(|e| e.block == id)?;
                let entry = &old_outline.entries[index];
                let occurrence = old_outline.entries[..index]
                    .iter()
                    .filter(|e| e.level == entry.level && e.text == entry.text)
                    .count();
                Some((entry.level, entry.text.as_str(), occurrence))
            })
            .collect();
        let new_outline = self.tree.outline();
        self.folds.collapsed = wanted
            .into_iter()
            .filter_map(|(level, text, occurrence)| {
                new_outline
                    .entries
                    .iter()
                    .filter(|e| e.level == level && e.text == text)
                    .nth(occurrence)
                    .map(|e| e.block)
            })
            .collect();
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

    /// The chrome this viewport can actually afford, with the gutter measured
    /// against the document currently laid out.
    ///
    /// Everything geometric goes through here rather than through
    /// [`AppState::chrome`] directly, so a terminal too narrow for furniture
    /// is narrow in exactly one place instead of in every caller.
    pub fn fitted_chrome(&self) -> Chrome {
        let gutter = painter::gutter_width(self.chrome, self.tree.line_count());
        self.chrome.fit(self.size.width, self.size.height, gutter)
    }

    /// The page the painter should draw this state into.
    pub fn page(&self) -> Page {
        Page::new(
            self.fitted_chrome(),
            self.size,
            self.tree.line_count(),
            self.cursor,
        )
    }

    pub fn set_chrome(&mut self, chrome: Chrome) {
        self.chrome = chrome;
    }

    /// The content column a terminal `terminal_width` cells wide leaves once
    /// this state's chrome has taken its share.
    ///
    /// The **one** place a terminal width becomes a layout width. Everything
    /// downstream — `content_width`, `+`/`-`, the anchor arithmetic — works in
    /// content columns and never subtracts chrome again, which is what keeps
    /// `+` widening the measure rather than fighting the gutter for the same
    /// cells.
    pub fn content_width_in(&self, terminal_width: u16) -> u16 {
        let gutter = painter::gutter_width(self.chrome, self.tree.line_count());
        let chrome = self
            .chrome
            .fit(terminal_width, self.size.height, gutter)
            .horizontal(gutter);
        terminal_width.saturating_sub(chrome).max(1)
    }

    pub fn chrome(&self) -> Chrome {
        self.chrome
    }

    /// The reading line, as an absolute index into the tree.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    fn set_scroll(&mut self, value: usize) {
        self.scroll = value.min(self.max_scroll());
        self.reseat_cursor();
    }

    fn scroll_by(&mut self, delta: isize) {
        let current = self.scroll as isize;
        self.set_scroll((current + delta).max(0) as usize);
    }

    /// Moves the reading line by `delta` and brings the viewport with it.
    ///
    /// The ordinary motion. `j` at the bottom of the screen scrolls because
    /// the reading line ran out of viewport, not because `j` is a scroll key —
    /// which is the whole difference between this model and the one it
    /// replaced, and the reason the reader keeps their place when the document
    /// is shorter than the screen.
    fn move_cursor(&mut self, delta: isize) {
        let last = self.last_line();
        let target = (self.cursor as isize).saturating_add(delta).max(0) as usize;
        self.cursor = target.min(last);
        self.follow_cursor();
    }

    /// Moves the page by `delta` rows and carries the reading line with it.
    ///
    /// Distinct from [`AppState::move_cursor`], and the distinction is the
    /// whole difference between a line key and a page key: `j` moves the
    /// reader and lets the page follow, `PgDn` moves the page and takes the
    /// reader along. Move the reading line a page and let the viewport chase
    /// it and you get a page key that scrolls by one row — the reading line
    /// lands on the bottom row, which is already visible, so nothing needs to
    /// move. That is not what anyone pressing `PgDn` is asking for.
    ///
    /// At the end of the document the page stops and the reading line does
    /// not: `Ctrl-f` on the last screen still walks to the final line, which
    /// is the only way to reach it with a page key.
    fn move_page(&mut self, delta: isize) {
        let target = (self.cursor as isize).saturating_add(delta).max(0) as usize;
        self.scroll_by(delta);
        self.set_cursor(target);
    }

    /// Puts the reading line on `line` and brings the viewport with it.
    fn set_cursor(&mut self, line: usize) {
        self.cursor = line.min(self.last_line());
        self.follow_cursor();
    }

    /// The last addressable line, or `0` for an empty document — which is also
    /// where the reading line sits in one, since there is nowhere else.
    fn last_line(&self) -> usize {
        self.tree.line_count().saturating_sub(1)
    }

    /// Rows of document the page shows: the viewport less the theme's vertical
    /// padding. One at minimum, so a viewport swallowed entirely by padding
    /// still scrolls rather than dividing the reader's motions by zero.
    fn content_rows(&self) -> usize {
        usize::from(
            self.size
                .height
                .saturating_sub(self.fitted_chrome().vertical())
                .max(1),
        )
    }

    /// Rows kept between the reading line and the edge of the page before it
    /// scrolls, capped so it can never exceed half the page — past that the
    /// two limits cross and the reading line would have nowhere legal to be.
    fn scrolloff(&self) -> usize {
        let rows = self.content_rows();
        usize::from(self.fitted_chrome().scrolloff).min(rows.saturating_sub(1) / 2)
    }

    /// Scrolls the minimum needed to bring the reading line back into the
    /// page, honouring [`AppState::scrolloff`].
    ///
    /// Minimum, not centred: a motion that only needs one row of scroll should
    /// cost one row of scroll. Recentring on every `j` past the bottom edge is
    /// what makes a pager feel like it is fighting the reader.
    fn follow_cursor(&mut self) {
        let rows = self.content_rows();
        let off = self.scrolloff();
        let highest_top = self.cursor.saturating_sub(off);
        // The lowest top that still shows the reading line plus its margin.
        let lowest_top = self
            .cursor
            .saturating_add(off)
            .saturating_add(1)
            .saturating_sub(rows);
        let mut scroll = self.scroll;
        if scroll > highest_top {
            scroll = highest_top;
        }
        if scroll < lowest_top {
            scroll = lowest_top;
        }
        // `max_scroll` can only pull the top *up*, which can only reveal more
        // of the document below the reading line — so the clamp cannot undo
        // the work above.
        self.scroll = scroll.min(self.max_scroll());
    }

    /// Pulls the reading line back onto the page after the viewport moved
    /// without it: a `Esc` out of the TOC, a relayout's anchor, a resize.
    ///
    /// The scrolloff margin is dropped at the document's ends, where there is
    /// no context to keep: with the page at the very top the reading line may
    /// sit on line 0, and at the very bottom on the last line. Without that
    /// exception a reader who pressed `G` would land two rows short of the end
    /// and have no way to reach it.
    fn reseat_cursor(&mut self) {
        let last = self.last_line();
        let rows = self.content_rows();
        let off = self.scrolloff();
        let bottom = self.scroll.saturating_add(rows).saturating_sub(1).min(last);
        let mut lowest = if self.scroll == 0 {
            0
        } else {
            self.scroll.saturating_add(off)
        };
        let mut highest = if self.scroll >= self.max_scroll() {
            last
        } else {
            bottom.saturating_sub(off)
        };
        lowest = lowest.min(bottom);
        highest = highest.max(lowest).min(last);
        self.cursor = self.cursor.clamp(lowest, highest);
    }

    /// One page's worth of lines: the step for `PgUp`/`PgDn` and for
    /// vim's `Ctrl-f`/`Ctrl-b`.
    fn page_size(&self) -> usize {
        self.content_rows()
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
            // Scroll *and* reading line, in that order: a jump puts the block
            // at the top of the page and the reader on its first row, which is
            // the row they asked for. Setting only the scroll would leave the
            // band wherever the last motion left it, several rows into a
            // section the reader has not read yet.
            self.set_scroll(line);
            self.set_cursor(line);
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
        // Stepped from the *reading line*, not from the page's top row. The
        // two were the same thing until the reading line existed; now a reader
        // three rows down a section and pressing `]]` means "the next heading
        // after me", and answering from the top row would jump to the heading
        // they are already standing under.
        let index = if forward {
            outline.next_after(self.cursor)
        } else {
            outline.previous_before(self.cursor)
        };
        let target = index.and_then(|index| outline.line_of(index));
        let empty = outline.is_empty();
        let before = (self.scroll, self.cursor);
        if let Some(line) = target {
            self.set_scroll(line);
            self.set_cursor(line);
            if (self.scroll, self.cursor) != before {
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
        let selected = outline.index_at_or_before(self.cursor).unwrap_or(0);
        self.toc_return_scroll = self.scroll;
        self.mode = Mode::Toc { selected };
    }

    /// The rows the TOC overlay paints into a viewport `height` rows tall,
    /// scrolled so the selection is always among them (DW-3.2).
    ///
    /// Empty outside [`Mode::Toc`], and empty for a zero-row viewport — a
    /// terminal too short to render the overlay paints nothing rather than
    /// panicking on the window arithmetic.
    pub fn toc_rows(&self, height: u16) -> Vec<OverlayRow> {
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
            .map(|(offset, entry)| OverlayRow {
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
                style: if first + offset == selected {
                    RowStyle::Selected
                } else {
                    RowStyle::Ordinary
                },
            })
            .collect()
    }

    // --------------------------------------------------------------- explore (Phase 3)

    /// Installs a freshly-read [`crate::explore::Listing`] and enters or
    /// stays in [`Mode::Explore`] — the one seam every way of reaching or
    /// re-listing the explorer goes through: the initial rooted launch, `_`
    /// from [`Mode::Normal`], and every `Enter`-on-directory or `-` re-list.
    ///
    /// `rooted` is the caller's to set correctly; this method does not infer
    /// it. `main.rs` passes `true` only for the launch that opened the
    /// explorer with nothing behind it, and reads the *current* mode's
    /// `rooted` back out to pass unchanged into every re-list — see
    /// [`Mode::Explore`]'s own doc for why a re-list must never flip it.
    ///
    /// The reader's scroll position is captured for [`AppState::handle_explore_key`]'s
    /// `Esc` to restore, but **only on the way in** — a re-list while already
    /// exploring must not overwrite the position the *document* was left at
    /// with whatever [`AppState::scroll`] happens to hold mid-browse (it is
    /// meaningless there; the explorer paints over the whole viewport).
    /// Any edit in progress is dropped here (Phase 4), and this is the only
    /// place that needs to do it: a buffer's lines index the entries of the
    /// listing it was built from, so a buffer that survived a re-list would
    /// name different files than the ones the reader typed over. See
    /// [`AppState::explore_edit`].
    pub fn install_listing(&mut self, listing: Listing, selected: usize, rooted: bool) {
        if !matches!(self.mode, Mode::Explore { .. }) {
            self.explore_return_scroll = self.scroll;
        }
        self.explore = Some(listing);
        self.explore_edit = None;
        self.mode = Mode::Explore { selected, rooted };
    }

    /// The rows the explorer overlay paints into a viewport `height` rows
    /// tall (DW-3.2), windowed exactly as [`AppState::toc_rows`] windows the
    /// TOC — delegated to [`crate::explore::Listing::rows`], which already
    /// does that windowing and the dim/selected styling both, over data this
    /// type only ever holds, never reads.
    ///
    /// Empty outside [`Mode::Explore`], and empty when nothing has been
    /// installed yet — reachable only mid-startup, before the first
    /// [`AppState::install_listing`] call lands.
    ///
    /// Once an edit is in progress (Phase 4) the buffer paints instead of the
    /// listing, through [`crate::explore::EditBuffer::rows`], which windows
    /// identically — so the overlay does not jump when `i` is pressed. While
    /// editing, `selected` indexes the *buffer's* rows; the buffer holds one
    /// row per entry in the same order, so the two spaces agree until the
    /// reader adds a row.
    pub fn explore_rows(&self, height: u16) -> Vec<OverlayRow> {
        let Mode::Explore { selected, .. } = self.mode else {
            return Vec::new();
        };
        match (&self.explore_edit, &self.explore) {
            (Some(session), _) => session.buffer.rows(height, selected),
            (None, Some(listing)) => listing.rows(height, selected),
            (None, None) => Vec::new(),
        }
    }

    /// The directory the explorer is currently listing, or `None` outside
    /// [`Mode::Explore`] — read by `main.rs` before a re-list, so the
    /// directory just being *left* can be looked up by name in the
    /// directory being entered (DW-3.7's noted re-seat-by-name choice).
    ///
    /// The `None` half is an invariant, not a hope: every exit from
    /// [`Mode::Explore`] drops the listing — [`AppState::close_explore`] on
    /// `Esc`, [`AppState::open_document`] on `Enter` over a document row —
    /// and [`AppState::install_listing`] is the only thing that installs
    /// one. The assertion below is what keeps a future third exit honest;
    /// `open_document` was that third exit until this pass, and this method
    /// silently answered `Some` in `Mode::Normal` for as long as it was.
    pub fn explore_dir(&self) -> Option<&Path> {
        debug_assert!(
            self.explore.is_none() || matches!(self.mode, Mode::Explore { .. }),
            "a listing outlived Mode::Explore"
        );
        self.explore.as_ref().map(Listing::dir)
    }

    /// The explorer's key table (DW-3.6): `j`/`k` move the selection,
    /// skipping unselectable rows; `Enter` opens or descends; `-` ascends;
    /// `Esc` closes (or quits, rooted); `q`/`Ctrl-c` always quit. Every other
    /// key — including the whole chrome table, which `chrome_action` has
    /// already declined to claim because [`Mode::captures_all_keys`] says
    /// this mode owns the keyboard — is a deliberate no-op.
    ///
    /// Phase 4 puts three tiers in front of that table, checked in this
    /// order, and the order is the safety property: a confirmation gate that
    /// could be reached *past* a typed character, or a typed character that
    /// could be read as a command, is how a reflexive keystroke deletes
    /// something.
    ///
    /// 1. **A plan is waiting.** `y` applies, `n`/`Esc` cancels, everything
    ///    else is inert.
    /// 2. **A row is being typed into.** Every printable key is a character
    ///    of the name — which is why `q` cannot quit from here, and why
    ///    `Ctrl-c` is lifted above all three tiers as the one key that always
    ///    can.
    /// 3. **A buffer is open.** `j`/`k`/`i`/`o`/`d`/`w`/`Esc`/`q`.
    ///
    /// With no buffer open the explorer is exactly the Phase 3 read-only
    /// navigator, except that `i`/`o`/`d`/`w` open a buffer and then act.
    fn handle_explore_key(&mut self, key: KeyEvent, selected: usize, rooted: bool) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        // Above every tier, including the one where letters are text: a
        // reader who has typed themselves into a corner must always be able
        // to leave the process.
        if ctrl && key.code == KeyCode::Char('c') {
            return true;
        }
        if self
            .explore_edit
            .as_ref()
            .is_some_and(|session| session.pending.is_some())
        {
            self.handle_confirm_key(key);
            return false;
        }
        if let Some(row) = self
            .explore_edit
            .as_ref()
            .and_then(|session| session.editing)
        {
            self.handle_row_edit_key(key, row, ctrl);
            return false;
        }
        if self.explore_edit.is_none() {
            if ctrl || !matches!(key.code, KeyCode::Char('i' | 'o' | 'd' | 'w')) {
                return self.handle_browse_key(key, selected, rooted);
            }
            if !self.begin_edit_buffer() {
                return false;
            }
        }
        self.handle_buffer_key(key, selected, rooted)
    }

    /// The Phase 3 read-only table, unchanged except that its `Ctrl-c` arm
    /// moved up to [`AppState::handle_explore_key`]. It had to: `Ctrl-c` must
    /// also quit from inside a half-typed filename, where `c` is a character,
    /// and a copy left down here would have been a branch nothing could
    /// reach.
    fn handle_browse_key(&mut self, key: KeyEvent, selected: usize, rooted: bool) -> bool {
        match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Esc => {
                if rooted {
                    // Nothing real is behind this explorer (DW-3.15): there
                    // is no document to reveal, so the only honest answer to
                    // "leave" is to leave the process.
                    return true;
                }
                self.close_explore();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_explore_selection(selected, rooted, true)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_explore_selection(selected, rooted, false)
            }
            KeyCode::Enter => self.activate_explore_row(selected),
            KeyCode::Char('-') => self.ascend_explore(),
            _ => {}
        }
        false
    }

    // ----------------------------------------------------- editing (Phase 4)

    /// Opens an edit buffer over the current listing. Answers whether there
    /// was a listing to open one over.
    fn begin_edit_buffer(&mut self) -> bool {
        let Some(listing) = &self.explore else {
            return false;
        };
        self.explore_edit = Some(EditSession {
            buffer: EditBuffer::new(listing),
            editing: None,
            pending: None,
        });
        true
    }

    /// The confirmation gate's whole key table (DW-4.3).
    ///
    /// `Enter` is **deliberately not** a yes. It is the key a reader presses
    /// without reading, and the sentence above it names files that are about
    /// to stop existing. `y` is a key you have to mean.
    ///
    /// And only a **bare** `y` or `n`. A chord is not an answer to a
    /// yes-or-no question: `Alt-y` is a key the reader's terminal or window
    /// manager may well have sent on its own, and it must not be the thing
    /// that deletes their files. `Esc` is exempt from that rule in the safe
    /// direction — cancelling is always allowed, however it arrives.
    fn handle_confirm_key(&mut self, key: KeyEvent) {
        let bare = key.modifiers.is_empty();
        match key.code {
            KeyCode::Char('y') if bare => {
                let plan = self
                    .explore_edit
                    .as_mut()
                    .and_then(|session| session.pending.take());
                if let Some(plan) = plan {
                    self.action = Some(PendingAction::ApplyEdits(plan));
                }
            }
            KeyCode::Char('n') if bare => self.cancel_pending_write(),
            KeyCode::Esc => self.cancel_pending_write(),
            // Every other key is inert, on purpose. Falling through to the
            // buffer's table here would let `d` mark a row for deletion while
            // a delete confirmation is on screen.
            _ => {}
        }
    }

    /// Drops the plan without running any of it, and says so.
    fn cancel_pending_write(&mut self) {
        if let Some(session) = self.explore_edit.as_mut() {
            session.pending = None;
        }
        self.set_status(StatusMessage::new(WRITE_CANCELLED));
    }

    /// Typing a name into one row: the same four arms
    /// [`AppState::handle_search_key`] uses, which is the whole of the
    /// crate's line editing.
    ///
    /// **Not factored into a shared editor**, and that is this phase's answer
    /// to the plan's stated uncertainty. The search prompt's editor is
    /// `String::push` on a printable key and `String::pop` on `Backspace` —
    /// no cursor, no word motions, no kill ring. An abstraction over those
    /// two calls would be a module with no depth at all, and both callers
    /// would still own their own `Esc` semantics (search restores a scroll
    /// position; this restores a row's seeded text). There is nothing here to
    /// share yet; when one of the two grows a cursor, there will be.
    fn handle_row_edit_key(&mut self, key: KeyEvent, row: usize, ctrl: bool) {
        let Some(session) = self.explore_edit.as_mut() else {
            return;
        };
        match key.code {
            // A chord is never a character. Without this guard `Ctrl-d` would
            // type a `d` into a filename.
            KeyCode::Char(ch) if !ctrl => session.buffer.push_char(row, ch),
            KeyCode::Backspace => session.buffer.pop_char(row),
            KeyCode::Enter => session.editing = None,
            KeyCode::Esc => {
                session.buffer.revert(row);
                session.editing = None;
            }
            _ => {}
        }
    }

    /// The open-buffer table: move, edit, add, remove, write, discard.
    fn handle_buffer_key(&mut self, key: KeyEvent, selected: usize, rooted: bool) -> bool {
        match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Esc => self.discard_edits(),
            KeyCode::Down | KeyCode::Char('j') => self.move_edit_selection(selected, rooted, true),
            KeyCode::Up | KeyCode::Char('k') => self.move_edit_selection(selected, rooted, false),
            KeyCode::Char('i') => self.begin_row_edit(selected),
            KeyCode::Char('o') => self.insert_edit_row(selected, rooted),
            KeyCode::Char('d') => self.toggle_row_removal(selected),
            KeyCode::Char('w') => self.build_edit_plan(),
            // Leaving the directory would throw the edit away silently. Say
            // so instead: the reader has two keys that resolve it and neither
            // is the one they pressed.
            KeyCode::Enter | KeyCode::Char('-') => {
                self.set_status(StatusMessage::new(UNSAVED_EDITS));
            }
            _ => {}
        }
        false
    }

    /// `j`/`k` over the buffer. Every buffer row is selectable — including
    /// `../`, which cannot be edited but can be passed over, and a row marked
    /// for removal, which the reader must be able to reach again to unmark.
    fn move_edit_selection(&mut self, selected: usize, rooted: bool, forward: bool) {
        let Some(session) = &self.explore_edit else {
            return;
        };
        let last = match session.buffer.len().checked_sub(1) {
            Some(last) => last,
            None => return,
        };
        let selected = if forward {
            selected.saturating_add(1).min(last)
        } else {
            selected.saturating_sub(1)
        };
        self.mode = Mode::Explore { selected, rooted };
    }

    /// `i`: start typing into the selected row, unless it is `../`.
    fn begin_row_edit(&mut self, selected: usize) {
        let Some(session) = self.explore_edit.as_mut() else {
            return;
        };
        if session.buffer.is_editable(selected) {
            session.editing = Some(selected);
        }
    }

    /// `o`: a new empty row below the selection, already being typed into.
    fn insert_edit_row(&mut self, selected: usize, rooted: bool) {
        let Some(session) = self.explore_edit.as_mut() else {
            return;
        };
        let selected = session.buffer.insert_new(selected);
        session.editing = Some(selected);
        self.mode = Mode::Explore { selected, rooted };
    }

    /// `d`: mark the selected row for removal, or unmark it. A mark is not a
    /// delete — nothing happens until `w` and then `y`.
    fn toggle_row_removal(&mut self, selected: usize) {
        if let Some(session) = self.explore_edit.as_mut() {
            session.buffer.toggle_removed(selected);
        }
    }

    /// `w`: turn the buffer into a plan and put the confirmation up.
    ///
    /// Still no I/O — [`crate::explore::EditPlan::diff`] is pure, which is
    /// what lets this type build the prompt at all. A refusal lands on the
    /// status row naming the row that caused it; an edit that asks for
    /// nothing says so rather than putting an empty question on screen.
    fn build_edit_plan(&mut self) {
        let (Some(listing), Some(session)) = (&self.explore, &self.explore_edit) else {
            return;
        };
        match EditPlan::diff(listing, &session.buffer.lines()) {
            Ok(plan) if plan.is_empty() => self.set_status(StatusMessage::new(NOTHING_TO_WRITE)),
            Ok(plan) => {
                if let Some(session) = self.explore_edit.as_mut() {
                    session.pending = Some(plan);
                }
                // Arming the gate retires whatever the row was saying. Every
                // message that could still be up here describes the edit
                // *before* this plan — a refusal, or the `WRITE_CANCELLED`
                // from the last time this same buffer was offered — and none
                // of them is true of the question now being asked.
                // [`AppState::status`] gives the question precedence anyway;
                // this keeps a stale message from outliving the gate and
                // reappearing on the frame after it closes.
                self.clear_status();
            }
            Err(error) => self.set_status(StatusMessage::new(error.to_string())),
        }
    }

    /// `Esc` with a buffer open: throw the edit away and go back to browsing.
    ///
    /// Says so when there was something to throw away, and stays silent when
    /// there was not — a reader who pressed `i` and changed their mind should
    /// not be told they lost work.
    fn discard_edits(&mut self) {
        let dirty = self
            .explore_edit
            .as_ref()
            .is_some_and(|session| session.buffer.is_dirty());
        self.explore_edit = None;
        if dirty {
            self.set_status(StatusMessage::new(EDITS_DISCARDED));
        }
        // The buffer's row space can be longer than the listing's (`o` adds
        // rows), so the selection has to come back into range.
        self.reseat_explore();
    }

    /// `Esc` with `rooted: false` (DW-3.5): back to the document, at the
    /// scroll position the reader left it at.
    fn close_explore(&mut self) {
        self.mode = Mode::Normal;
        self.explore = None;
        self.explore_edit = None;
        self.set_scroll(self.explore_return_scroll);
    }

    /// `j`/`k`: the next or previous selectable row, or a no-op at either
    /// end — [`crate::explore::Listing::next_selectable`]/`prev_selectable`
    /// already encode "never land on an unopenable row" and "no wrap", so
    /// this only has to thread `rooted` through unchanged.
    fn move_explore_selection(&mut self, selected: usize, rooted: bool, forward: bool) {
        let Some(listing) = &self.explore else {
            return;
        };
        let target = if forward {
            listing.next_selectable(selected)
        } else {
            listing.prev_selectable(selected)
        };
        if let Some(selected) = target {
            self.mode = Mode::Explore { selected, rooted };
        }
    }

    /// `Enter` (DW-3.3/DW-3.4): a directory or `../` row re-lists through
    /// [`PendingAction::ListDirectory`]; a document row opens through
    /// [`PendingAction::OpenPath`]. Both name the entry's own recorded
    /// [`crate::explore::Entry::path`] — exact, never reconstructed from the
    /// row's display text, which is lossy for a non-UTF8 name.
    ///
    /// An [`EntryKind::Unopenable`] row is a deliberate no-op rather than an
    /// error: the three movement methods already refuse to select one, so
    /// reaching this arm at all would mean `selected` was never seated by
    /// them — defended here rather than assumed away.
    fn activate_explore_row(&mut self, selected: usize) {
        let Some(listing) = &self.explore else {
            return;
        };
        let Some(entry) = listing.entries().get(selected) else {
            return;
        };
        self.action = Some(match entry.kind {
            EntryKind::Parent | EntryKind::Directory => {
                PendingAction::ListDirectory(entry.path.clone())
            }
            EntryKind::Document => PendingAction::OpenPath(entry.path.clone()),
            EntryKind::Unopenable => return,
        });
    }

    /// `-` (DW-3.4/DW-3.6): re-lists the parent directory, or does nothing at
    /// the filesystem root, where there is no parent to ascend to — the same
    /// "no such row" no-op `Enter` on a `../` that does not exist would be.
    ///
    /// The target is [`crate::explore::Listing::parent`] — the `../` row's
    /// own recorded path — precisely so that `-` and `Enter` on `../` cannot
    /// name different directories. This used to call `dir().parent()`
    /// itself, a second answer to the same question, and both answers were
    /// wrong for a relative directory: `Path::new(".").parent()` is
    /// `Some("")`, so `stele .` followed by one `-` queued a read of the
    /// empty path and left the reader in a listing no key could leave.
    fn ascend_explore(&mut self) {
        let Some(listing) = &self.explore else {
            return;
        };
        if let Some(parent) = listing.parent() {
            self.action = Some(PendingAction::ListDirectory(parent.to_path_buf()));
        }
    }

    /// Re-clamps [`Mode::Explore`]'s `selected` after a relayout — a resize
    /// storm being the only thing that can trigger one while the explorer is
    /// open, since [`AppState::explore`] itself only ever changes through
    /// [`AppState::install_listing`], which seats `selected` fresh every
    /// time. Called unconditionally, like [`AppState::reseat_link_select`]:
    /// a clamp that finds nothing out of range is a no-op.
    ///
    /// **Only ever clamps, never drops to [`Mode::Normal`].**
    /// [`crate::explore::Listing::rows`] is total over every `(height,
    /// selected)`, including an empty listing, so there is never a listing
    /// this mode cannot honestly paint — and for a rooted launch, dropping to
    /// `Normal` would reveal the empty placeholder document, which DW-3.15
    /// forbids outright. DW-3.7's "or drop cleanly to Normal" alternative is
    /// therefore never exercised: a valid row (or the honest absence of one,
    /// on a listing with nothing selectable) always exists instead.
    fn reseat_explore(&mut self) {
        let Mode::Explore { selected, rooted } = self.mode else {
            return;
        };
        let Some(listing) = &self.explore else {
            return;
        };
        let count = listing.entries().len();
        let clamped = if count == 0 {
            0
        } else {
            selected.min(count - 1)
        };
        if clamped != selected {
            self.mode = Mode::Explore {
                selected: clamped,
                rooted,
            };
        }
    }

    /// `z` (DW-5.1): folds or unfolds the heading at or above the cursor.
    ///
    /// Zero-arg by design (see the plan's `Produces`): this only decides
    /// *which* heading and flips its membership in [`AppState::folds`],
    /// arming [`AppState::pending_fold_snap`] when folding would carry the
    /// current line out of view. The relayout that actually applies it is the
    /// caller's job (`main.rs::handle_chrome_key`), through the same
    /// [`AppState::relayout_preserving_anchor`] every other chrome mutation
    /// uses.
    pub fn toggle_fold(&mut self) {
        let outline = self.tree.outline();
        let Some(index) = outline.index_at_or_before(self.cursor) else {
            // `None` here means either there is no heading in the document
            // at all, or there is one but it is below the cursor — a
            // document with headings the reader just has not reached yet.
            // `NO_HEADINGS` would be false for the second case.
            let message = if outline.is_empty() {
                NO_HEADINGS
            } else {
                NO_SECTION_HERE
            };
            self.set_status(StatusMessage::new(message));
            return;
        };
        let target = outline.entries[index].block;
        let folding = !self.folds.is_folded(target);
        self.pending_fold_snap =
            (folding && self.section_line_range(index).contains(&self.cursor)).then_some(target);
        self.folds.toggle(target);
    }

    /// `R` (DW-5.4): opens every fold. Equivalent to relaying out with an
    /// empty [`FoldState`] — the full document, restored.
    pub fn expand_all(&mut self) {
        self.folds.collapsed.clear();
    }

    /// `M` (DW-5.4): folds every heading, top-level and nested alike.
    /// Collapsing a nested heading is not wasted work: a fold range stops at
    /// the next heading of equal or shallower level, so a top-level
    /// heading's own fold already swallows every heading nested inside it
    /// without ever visiting them — nesting the same id into
    /// [`FoldState::collapsed`] just means that if the reader later opens the
    /// outer one, the inner heading is still folded rather than snapping
    /// wide open underneath it.
    ///
    /// **Unions into `collapsed` rather than replacing it — load-bearing,
    /// not stylistic.** `self.tree.outline()` is abbreviated while any fold
    /// is already active (a heading nested inside a folded range is never
    /// visited — see [`AppState::reseat_folds`]'s doc for the same fact
    /// biting a reload). A `collapsed = outline_ids` assignment would then
    /// silently drop every id the *current* abbreviated outline cannot see,
    /// which is exactly the ids this method's own doc promises stay folded:
    /// pressing `M` a second time, after the first already hid some nested
    /// heading, replaced the whole set with only what was still visible and
    /// lost it. `extend` only ever adds, so a heading already in `collapsed`
    /// — visible in the current outline or not — is never removed by a
    /// later collapse-all.
    pub fn collapse_all(&mut self) {
        self.folds
            .collapsed
            .extend(self.tree.outline().entries.iter().map(|entry| entry.block));
    }

    /// The current (pre-fold) line range of the section the `index`-th
    /// outline entry heads: from its own line up to, but not including, the
    /// next heading of equal or shallower level — the same "runs to the next
    /// heading of equal or shallower level" rule
    /// `layout::block::Ctx::fold_range` applies during the walk, computed
    /// here against line numbers in the *already laid out* tree instead of
    /// block indices mid-walk, because this is what [`AppState::toggle_fold`]
    /// needs to decide DW-5.5 before any relayout has happened.
    fn section_line_range(&self, index: usize) -> Range<usize> {
        let outline = self.tree.outline();
        let level = outline.entries[index].level;
        let start = outline.line_of(index).unwrap_or(0);
        let end = outline.entries[index + 1..]
            .iter()
            .position(|e| e.level <= level)
            .and_then(|rel| outline.line_of(index + 1 + rel))
            .unwrap_or(self.tree.line_count());
        start..end
    }

    /// Every link the reader can currently see, in viewport order (DW-6.1).
    ///
    /// Recomputed from `(tree, scroll)` on every call rather than cached. That
    /// costs a walk of at most one viewport's worth of lines — the same walk
    /// the painter does every frame — and buys the property that no stored
    /// index can ever address a link in a document that has been replaced
    /// underneath it by a `--watch` reload.
    ///
    /// **Consecutive runs are one link.** A link whose text carries a style
    /// change (`[**bold** rest](x)`) becomes several runs sharing one `aux`,
    /// and a link that crosses a wrap boundary continues on the next line. Both
    /// are merged here, so `Tab` counts links as a reader counts them rather
    /// than counting runs. The continuation rule is "same destination, on the
    /// immediately following line"; two *different* links to the same URL on
    /// adjacent lines therefore merge into one entry, which is a cosmetic
    /// mis-count and never a mis-navigation — both halves open the same place.
    pub fn visible_links(&self) -> Vec<VisibleLink> {
        let height = usize::from(self.size.height);
        let last = self
            .scroll
            .saturating_add(height)
            .min(self.tree.line_count());
        let mut links: Vec<VisibleLink> = Vec::new();
        for line_index in self.scroll..last {
            let Some(Line::Items(items)) = self.tree.lines(line_index..line_index + 1).next()
            else {
                continue;
            };
            for (target, text, span) in link_groups(items, line_index) {
                let continues = links.last().is_some_and(|link| {
                    link.target == target
                        && link
                            .spans
                            .last()
                            .is_some_and(|last| last.line + 1 == span.line)
                });
                match links.last_mut() {
                    Some(link) if continues => {
                        // The wrap point ate a space; put one back so the
                        // status row reads as one label rather than as two
                        // words jammed together.
                        link.text.push(' ');
                        link.text.push_str(&text);
                        link.spans.push(span);
                    }
                    Some(_) | None => links.push(VisibleLink {
                        target,
                        text,
                        spans: vec![span],
                    }),
                }
            }
        }
        links
    }

    /// The link [`Mode::LinkSelect`] is currently on, or `None` in any other
    /// mode (and when the index no longer addresses a link, which a reload can
    /// cause between a keystroke and the frame that follows it).
    pub fn selected_link(&self) -> Option<VisibleLink> {
        let Mode::LinkSelect { index } = self.mode else {
            return None;
        };
        self.visible_links().into_iter().nth(index)
    }

    /// The run ranges the painter must show as selected — empty in every mode
    /// but [`Mode::LinkSelect`].
    pub fn selection_spans(&self) -> Vec<LinkSpan> {
        self.selected_link()
            .map_or_else(Vec::new, |link| link.spans)
    }

    /// `Tab` / `Shift-Tab` from [`Mode::Normal`] (DW-6.1): enter link
    /// selection on the first (or last) visible link, or report that there is
    /// none rather than entering a mode with nothing in it.
    fn enter_link_select(&mut self, forward: bool) {
        let count = self.visible_links().len();
        if count == 0 {
            self.set_status(StatusMessage::new(NO_LINKS));
            return;
        }
        self.mode = Mode::LinkSelect {
            index: if forward { 0 } else { count - 1 },
        };
        self.announce_selection();
    }

    /// `Tab` / `Shift-Tab` inside [`Mode::LinkSelect`]: the next or previous
    /// link, wrapping at both ends.
    ///
    /// Wrapping, unlike `]]`/`[[`'s clamp, because the set being cycled is
    /// what is *on the screen right now*: a reader tabbing past the last link
    /// has not asked to keep going in a direction, they have asked for the
    /// next one of a handful they can see.
    fn cycle_link(&mut self, index: usize, forward: bool) {
        let count = self.visible_links().len();
        if count == 0 {
            self.mode = Mode::Normal;
            self.set_status(StatusMessage::new(NO_LINKS));
            return;
        }
        let index = index.min(count - 1);
        let next = if forward {
            (index + 1) % count
        } else {
            (index + count - 1) % count
        };
        self.mode = Mode::LinkSelect { index: next };
        self.announce_selection();
    }

    /// Puts the selected link's destination on the status row. The reverse
    /// video indicator says *which* link is selected; this says where it goes,
    /// which is the part a reader wants before pressing `Enter` on a document
    /// they did not write.
    fn announce_selection(&mut self) {
        if let Some(link) = self.selected_link() {
            let (index, count) = match self.mode {
                Mode::LinkSelect { index } => (index + 1, self.visible_links().len()),
                // Unreachable — `selected_link` already returned `None` in
                // every other mode. Named rather than wildcarded so a new
                // mode has to be considered here too.
                Mode::Normal | Mode::Toc { .. } | Mode::Search { .. } | Mode::Explore { .. } => {
                    return;
                }
            };
            self.set_status(StatusMessage::new(format!(
                "link {index}/{count}: {}",
                link.target
            )));
        }
    }

    /// `Enter` in [`Mode::LinkSelect`] (DW-6.1): hand the selected
    /// destination to the event loop and drop back to reading.
    ///
    /// The mode is left *before* the action is queued, so however the follow
    /// turns out — a new document, a browser, or a refusal on the status row —
    /// the reader is looking at a document rather than at a selection whose
    /// index may no longer mean anything.
    fn activate_selection(&mut self) {
        let selected = self.selected_link();
        self.mode = Mode::Normal;
        match selected {
            Some(link) => self.action = Some(PendingAction::OpenLink(link.target)),
            None => self.set_status(StatusMessage::new(NO_LINKS)),
        }
    }

    /// Takes the one action the event loop still owes the OS, if any.
    pub fn take_action(&mut self) -> Option<PendingAction> {
        self.action.take()
    }

    /// Whether mouse reporting is currently on (DW-6.6).
    pub fn mouse_capture(&self) -> bool {
        self.mouse_capture
    }

    /// `m` (DW-6.6): flips mouse capture and says which way it went.
    ///
    /// The message is not decoration. Turning capture off gives the terminal
    /// its own click-drag text selection back, and turning it on takes it
    /// away — an effect with no visible sign until the reader tries to select
    /// something and it does the other thing.
    fn toggle_mouse_capture(&mut self) {
        self.mouse_capture = !self.mouse_capture;
        self.action = Some(PendingAction::SetMouseCapture(self.mouse_capture));
        self.set_status(StatusMessage::new(if self.mouse_capture {
            "mouse capture on — terminal text selection is disabled"
        } else {
            "mouse capture off — terminal text selection is back"
        }));
    }

    /// Applies one mouse report (DW-6.6). Returns whether the frame changed.
    ///
    /// `engine` is needed because a click is answered in *columns*, and a
    /// column is a measurement: the painter clips each run through the width
    /// engine, so a hit-test that summed laid-out run widths instead would
    /// disagree with the screen the moment a run was clipped or carried a
    /// double-width cluster.
    pub fn handle_mouse_event(&mut self, event: MouseEvent, engine: &WidthEngine) -> bool {
        match event.kind {
            MouseEventKind::ScrollDown => {
                self.leave_link_select();
                self.scroll_by(WHEEL_LINES);
                true
            }
            MouseEventKind::ScrollUp => {
                self.leave_link_select();
                self.scroll_by(-WHEEL_LINES);
                true
            }
            MouseEventKind::Down(MouseButton::Left) => self.click(event.column, event.row, engine),
            // Everything else — right/middle buttons, drags, releases,
            // horizontal scroll, bare motion — is not a binding. Enumerated
            // rather than wildcarded so a crossterm that grows a variant is a
            // compile error here instead of a silently ignored gesture.
            MouseEventKind::Down(MouseButton::Right | MouseButton::Middle)
            | MouseEventKind::Up(_)
            | MouseEventKind::Drag(_)
            | MouseEventKind::Moved
            | MouseEventKind::ScrollLeft
            | MouseEventKind::ScrollRight => false,
        }
    }

    /// A left click at a 0-indexed `(column, row)` of the *content* viewport:
    /// activate the link under it, or do nothing at all (DW-6.6).
    fn click(&mut self, column: u16, row: u16, engine: &WidthEngine) -> bool {
        if row >= self.size.height {
            // The reserved status row. Nothing there is clickable.
            return false;
        }
        let page = self.page();
        // Both axes come back through the page's origin, because the event
        // arrives in the terminal's coordinates and everything below thinks in
        // the document's. Skipping this was the whole of the padding bug it is
        // here to prevent: with a gutter on, every click resolved a few cells
        // to the left of the glyph the reader aimed at, so the last character
        // of each link stopped working and the first character of the next
        // word started opening it.
        if row < page.origin.y || column < page.origin.x.saturating_add(page.gutter) {
            // The margin or the gutter. Neither is clickable — a click on a
            // line number is not a click on the line.
            return false;
        }
        let row = row - page.origin.y;
        let column = column - page.origin.x - page.gutter;
        let line = self.scroll.saturating_add(usize::from(row));
        let Some(target) = self.link_at(line, column, engine) else {
            return false;
        };
        self.mode = Mode::Normal;
        self.action = Some(PendingAction::OpenLink(target));
        true
    }

    /// The destination of the link painted at `(line, column)`, if any.
    /// `column` is page-relative: [`AppState::click`] has already taken the
    /// padding and the gutter off it.
    fn link_at(&self, line: usize, column: u16, engine: &WidthEngine) -> Option<String> {
        if line >= self.tree.line_count() {
            return None;
        }
        let Some(Line::Items(items)) = self.tree.lines(line..line + 1).next() else {
            return None;
        };
        let columns = item_columns(items, engine, self.page().content);
        self.visible_links().into_iter().find_map(|link| {
            let hit = link
                .spans
                .iter()
                .filter(|span| span.line == line)
                .any(|span| {
                    (span.first_item..=span.last_item)
                        .filter_map(|item| columns.get(item))
                        .any(|&(start, end)| column >= start && column < end)
                });
            hit.then_some(link.target)
        })
    }

    /// Drops out of [`Mode::LinkSelect`] without activating anything — used by
    /// the wheel, whose scroll changes which links are visible and therefore
    /// what any stored index would mean.
    fn leave_link_select(&mut self) {
        if matches!(self.mode, Mode::LinkSelect { .. }) {
            self.mode = Mode::Normal;
        }
    }

    /// `y` (DW-6.7): the text of the code block the reader is looking at, for
    /// the event loop to put on the clipboard.
    ///
    /// "The block in view" is the first code block any viewport line belongs
    /// to. The text is the AST's own `literal`, **not** the painted lines:
    /// layout clips a long code line and marks the clip with `…`, so copying
    /// what is on screen would put a truncated command on the clipboard that
    /// looks complete.
    ///
    /// The search descends into the top-level block, so a fence inside a list
    /// item or a blockquote is found too — `line_blocks` only ever names the
    /// top-level block a line belongs to.
    pub fn code_block_in_view(&mut self, doc: &Document) -> Option<String> {
        let height = usize::from(self.size.height);
        let last = self
            .scroll
            .saturating_add(height)
            .min(self.tree.line_count());
        let found = (self.scroll..last)
            .filter_map(|line| self.tree.block_at(line))
            .filter_map(|block| doc.node(block))
            .find_map(first_code_literal);
        if found.is_none() {
            self.set_status(StatusMessage::new(NO_CODE_BLOCK));
        }
        found
    }

    /// Installs a **different** document — a link followed, or a `Backspace`
    /// back to one — at `scroll`.
    ///
    /// Not [`AppState::reload_document`]: that one re-anchors the reader by
    /// content because the new tree is a new version of the *same* document.
    /// Here it is a different document entirely, so there is no place to
    /// preserve; the caller says where to land (0 for a link followed, the
    /// remembered offset for a `Backspace`). The reader's chosen
    /// [`AppState::content_width`] is carried across, because it is a
    /// preference about the terminal rather than a fact about the document.
    pub fn open_document(&mut self, ctx: &LayoutContext, file_info: FileInfo, scroll: usize) {
        let width = self
            .content_width
            .clamp(ctx.config.min_width, ctx.config.max_width);
        // Folds are dropped **before** the layout that installs the new tree,
        // and this ordering is the whole of it (Phase 5 × DW-6.2).
        //
        // `FoldState::collapsed` is a set of `NodeId`s, and a `NodeId` is a
        // dense positional index into *one* document — `ast::Document::node`
        // says so, and `reseat_folds` exists because a reload invalidates
        // them. Following a link replaces the document outright, so every id
        // in the set is not merely stale but *silently valid*: `NodeId(7)`
        // names some block in the new document too, and `layout_with_folds`
        // would collapse whatever section that turned out to head. The reader
        // would open a document they have never folded and find a section of
        // it already collapsed, keyed to a heading in a different file.
        //
        // So fold state is **per-document and does not travel the stack**:
        // it is cleared on the way in and, because `Backspace` comes back
        // through this same method, cleared on the way back too. Preserving
        // it across the stack would mean stashing a `FoldState` per
        // `StackedDocument` and re-keying it by content the way
        // `reseat_folds` does for a reload — a real feature with real
        // machinery, not a line of this one. The cost is stated rather than
        // hidden: fold a section, follow a link, come back, and the section
        // is open again.
        self.folds = FoldState::default();
        // Armed by `toggle_fold` for the *next* relayout of the tree it was
        // computed against. That tree is being replaced right here, so the
        // snap would land on a `NodeId` in a document that never armed it.
        self.pending_fold_snap = None;
        self.tree = layout_with_folds(
            ctx.doc,
            width,
            ctx.config,
            ctx.engine,
            ctx.sizer,
            &self.folds,
        );
        self.content_width = self.tree.width();
        self.file_info = file_info;
        self.mode = Mode::Normal;
        self.pending = None;
        // The listing the reader opened this document *from* is dropped
        // here, and this is the only place that can drop it: `Enter` on a
        // document row leaves `Mode::Explore` through this method rather
        // than through `close_explore`. Left alive it was up to 256 `Entry`
        // records held for the life of the next document, and — worse than
        // the memory — it made `explore_dir` answer `Some` in `Mode::Normal`,
        // falsifying that method's own documented contract for any future
        // caller that trusted it.
        self.explore = None;
        self.explore_edit = None;
        self.clear_status();
        self.document_changed = false;
        self.set_scroll(scroll);
        self.toc_return_scroll = self.scroll;
        // The search belongs to the document it was typed against, and this
        // is a different document — so it is dropped, not carried across.
        //
        // Not a preference. `SearchState::matches` addresses text by tree
        // line index and by a byte range into that line's laid-out text
        // (Phase 4's `Match`), and `open_document` has just replaced the
        // tree wholesale. Carrying the vector over would have the painter
        // highlight whatever bytes now sit at those coordinates and `n`/`N`
        // jump to lines that mean something else — the same staleness
        // `reseat_search` and `recompute_matches` exist to prevent on the
        // reload path, arriving through a door they do not cover, because
        // following a link does not go through `relayout` at all.
        //
        // Recomputing against the new tree instead would be safe too, and is
        // rejected on behaviour rather than on safety: a reader who follows a
        // link would land on a document they have never searched, already
        // covered in highlights for a query they typed somewhere else. `n`
        // would then traverse matches they never asked for. Dropping it is
        // what a browser's find-in-page does on navigation, and it is what
        // leaves nothing addressing a tree that no longer exists.
        //
        // Assigning the whole `SearchState` rather than clearing its fields
        // is deliberate: Phase 5's fix-forward added `hidden_by_folds`, a
        // count derived from a fold-free relayout of the *previous* document,
        // and a field-by-field clear is exactly where a new field gets
        // forgotten. `Default` cannot forget one.
        self.search = SearchState::default();
    }

    /// Puts [`Mode::LinkSelect`] back on a link that exists in the tree
    /// installed now.
    ///
    /// Called from [`AppState::relayout`] beside [`AppState::reseat_search`],
    /// which is the one place every tree replacement passes through: a
    /// `--watch` reload, a resize that rewraps the lines the index was
    /// computed from, and — since Phase 5 — a fold that removes whole
    /// sections of them. A tree with no visible links dismisses the mode
    /// rather than leaving an indicator on a link that is not there.
    ///
    /// **Unconditional, unlike [`AppState::reseat_search`], and that
    /// difference is the point.** `reseat_search` guards on `reflowed`
    /// because it *overwrites* `Mode::Search { origin }` with the current
    /// scroll, so running it when nothing changed would throw away a good
    /// answer. There is nothing here to throw away: the link list is
    /// recomputed from `(tree, scroll)` on every call and never cached, so
    /// this is a pure clamp of an index against freshly computed truth — a
    /// no-op precisely when the tree did not change.
    ///
    /// Guarding on `reflowed` would also have been *wrong* after Phase 5.
    /// `reflowed` means "the width changed or the document was replaced", and
    /// a fold is neither: it relays out at the same width on the same
    /// document and removes lines. The guard would have skipped exactly the
    /// case that changes which links exist.
    fn reseat_link_select(&mut self) {
        let Mode::LinkSelect { index } = self.mode else {
            return;
        };
        let count = self.visible_links().len();
        if count == 0 {
            self.mode = Mode::Normal;
            self.set_status(StatusMessage::new(NO_LINKS));
            return;
        }
        self.mode = Mode::LinkSelect {
            index: index.min(count - 1),
        };
    }

    /// Applies one key press *with its modifiers*. Returns `true` when the
    /// key requests quit. This is the event loop's entry point.
    ///
    /// The mode decides what a key means before anything else looks at it, so
    /// the TOC's `j`/`k` cannot also scroll the document underneath it, and a
    /// `q` typed into a search query is a character rather than a quit.
    pub fn handle_key_event(&mut self, key: KeyEvent) -> bool {
        match self.mode {
            Mode::Toc { selected } => self.handle_toc_key(key, selected),
            Mode::Search { .. } => self.handle_search_key(key),
            Mode::LinkSelect { index } => self.handle_link_select_key(key, index),
            Mode::Normal => self.handle_normal_key(key),
            Mode::Explore { selected, rooted } => self.handle_explore_key(key, selected, rooted),
        }
    }

    /// The link-selection key table (DW-6.1). `index` is the mode's own field,
    /// passed in for the reason [`AppState::handle_toc_key`]'s `selected` is.
    ///
    /// The two quit keys are repeated here for the reason the TOC repeats
    /// them: a mode that swallows `q` and Ctrl-C leaves a reader with no way
    /// out they would think to try.
    fn handle_link_select_key(&mut self, key: KeyEvent, index: usize) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('c') if ctrl => return true,
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.clear_status();
            }
            // `BackTab` is what crossterm reports for Shift-Tab on the
            // terminals that send CSI Z; the `Tab`-with-SHIFT form covers the
            // ones that report the modifier instead.
            KeyCode::BackTab => self.cycle_link(index, false),
            KeyCode::Tab if shift => self.cycle_link(index, false),
            KeyCode::Tab => self.cycle_link(index, true),
            KeyCode::Enter => self.activate_selection(),
            _ => {}
        }
        false
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
                // Phase 6's Normal-mode bindings. Inside the
                // `!CONTROL` guard with `t` and the brackets, so `Ctrl-y`
                // and `Ctrl-m` (which a terminal reports as `Enter`) cannot
                // reach them by the chord table's fallthrough.
                KeyCode::Tab => {
                    self.enter_link_select(true);
                    return false;
                }
                KeyCode::BackTab => {
                    self.enter_link_select(false);
                    return false;
                }
                KeyCode::Backspace => {
                    self.action = Some(PendingAction::Back);
                    return false;
                }
                KeyCode::Char('y') => {
                    self.action = Some(PendingAction::CopyCodeBlock);
                    return false;
                }
                KeyCode::Char('m') => {
                    self.toggle_mouse_capture();
                    return false;
                }
                // `_` (Phase 3, DW-3.1): opens the explorer at the open
                // document's directory. Unbound before this phase — verified
                // against every `KeyCode::Char` arm in this file — and safe
                // from `-`'s fate (claimed by `chrome_action` before this
                // table runs) because `_` was never claimed there either.
                KeyCode::Char('_') => {
                    self.action = Some(PendingAction::OpenExplorer);
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

    /// The key table while a query is being typed (DW-4.1). Returns `true`
    /// only for the one key that still means quit.
    ///
    /// Ctrl-C survives here for the reason it survives everywhere else:
    /// raw mode clears `ISIG`, so a Ctrl-C keystroke never becomes a
    /// `SIGINT`, and a reader who opened the prompt by accident would
    /// otherwise have no chord that works. Every other Control chord is
    /// swallowed rather than falling through — `Ctrl-d` must not scroll the
    /// viewport out from under a half-typed query.
    fn handle_search_key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return key.code == KeyCode::Char('c');
        }
        match key.code {
            KeyCode::Esc => self.cancel_search(),
            KeyCode::Enter => self.accept_search(),
            KeyCode::Backspace => {
                self.search.query.pop();
                self.refresh_incremental();
            }
            KeyCode::Char(c) => {
                self.search.query.push(c);
                self.refresh_incremental();
            }
            _ => {}
        }
        false
    }

    /// `/` (DW-4.1): opens the prompt on an empty query, remembering where
    /// the reader was so `Esc` can put them back.
    fn begin_search(&mut self) {
        self.mode = Mode::Search {
            origin: self.scroll,
        };
        self.search = SearchState::default();
    }

    /// `Esc` (DW-4.1): abandons the query *and* every viewport movement
    /// incremental search made on the way to it.
    fn cancel_search(&mut self) {
        if let Mode::Search { origin } = self.mode {
            self.set_scroll(origin);
        }
        self.mode = Mode::Normal;
        self.search = SearchState::default();
    }

    /// `Enter`: keeps the query and its matches so `n`/`N` can traverse them
    /// from normal mode, and leaves the reader on the current match.
    fn accept_search(&mut self) {
        self.mode = Mode::Normal;
        if self.search.matches.is_empty() {
            self.report_no_matches();
            return;
        }
        self.reveal_current_match();
    }

    /// Re-runs the search after every edit to the query, and moves the
    /// reader to the first match at or after where they started — the
    /// "incremental" half of incremental search.
    ///
    /// A query with no matches deliberately does nothing at all to the
    /// scroll position (DW-4.5): the viewport stays where the last matching
    /// prefix left it, so typing one character too many does not throw the
    /// reader somewhere else on its way to being deleted again.
    ///
    /// **Cannot recompute [`SearchState::hidden_by_folds`].** Doing so needs
    /// a fold-free relayout of the document (see
    /// [`AppState::recompute_matches`]), which needs a [`LayoutContext`] —
    /// and this runs from [`AppState::handle_search_key`], on every
    /// keystroke of the query, which by design has no `LayoutContext`
    /// (chrome — the keys that need one — is routed and handled entirely
    /// outside `AppState`; see [`AppState::chrome_action`]'s doc). So this
    /// resets the count to `0` rather than carry over a number computed for
    /// a *different* query: a stale "3 hidden by folds" surviving into a
    /// query it was never measured against would be exactly the kind of
    /// confidently wrong status row this field exists to prevent. The count
    /// becomes accurate again on the next relayout — which folding itself
    /// always causes, and folding while a query is being typed is
    /// impossible anyway ([`Mode::Search`] captures every key).
    fn refresh_incremental(&mut self) {
        let origin = match self.mode {
            Mode::Search { origin } => origin,
            // Only reachable from `handle_search_key`, so these arms are the
            // compiler's price for the exhaustiveness that makes a new mode a
            // compile error everywhere. "Wherever the reader is" is the
            // honest answer for a caller that arrived without an origin.
            Mode::Normal | Mode::Toc { .. } | Mode::LinkSelect { .. } | Mode::Explore { .. } => {
                self.scroll
            }
        };
        self.search.matches = find_matches(&self.tree, &self.search);
        self.search.current = first_match_at_or_after(&self.search.matches, origin);
        self.search.hidden_by_folds = 0;
        if self.search.matches.is_empty() {
            return;
        }
        self.reveal_current_match();
    }

    /// `n`/`N` (DW-4.3): steps to the next or previous match, wrapping at
    /// both ends. Modular arithmetic on `usize`, with the backward step
    /// written as `+ len - 1` so it never underflows at index 0.
    fn step_match(&mut self, forward: bool) {
        let count = self.search.matches.len();
        if count == 0 {
            self.report_no_matches();
            return;
        }
        let step = if forward { 1 } else { count - 1 };
        self.search.current = (self.search.current + step) % count;
        self.reveal_current_match();
    }

    /// Scrolls the current match into view, moving as little as possible: a
    /// match already on screen does not move the viewport at all, which is
    /// what keeps `n` through a cluster of nearby matches from lurching.
    fn reveal_current_match(&mut self) {
        let Some(found) = self.search.matches.get(self.search.current) else {
            return;
        };
        let line = found.line;
        let height = self.page_size();
        if line < self.scroll {
            self.set_scroll(line);
        } else if line >= self.scroll.saturating_add(height) {
            self.set_scroll(line + 1 - height);
        }
        // The reading line goes to the match, not merely near it. `n` is a
        // motion, and a band left three rows above the match the reader is
        // now looking at would be pointing at the wrong line — worse than not
        // pointing at all. Ordered after the scroll so `follow_cursor` has
        // nothing left to do and cannot undo the minimal-movement rule above.
        self.set_cursor(line);
    }

    /// DW-4.5's status-row half. Silent on an empty query — pressing `n`
    /// before ever searching has nothing to report, and "no matches" would
    /// be a lie about a query that was never made.
    ///
    /// **The listed search-in-fold edge case's status-row half.** A visible
    /// match count of zero means two different things, and this is what
    /// tells them apart: `query` really has no match anywhere
    /// (`hidden_by_folds == 0`), or every match that exists is currently
    /// folded away (`hidden_by_folds > 0`). Reporting the first message for
    /// the second situation is not merely uninformative, it is false — the
    /// reproduction that found this was `/needle` over a document that
    /// contains it, folding the section holding it, then `n` answering "no
    /// matches: needle".
    fn report_no_matches(&mut self) {
        if self.search.query.is_empty() {
            return;
        }
        let query = self.search.query.clone();
        let message = match self.search.hidden_by_folds {
            0 => format!("no matches: {query}"),
            1 => format!("no matches: {query} (1 hidden by a fold — R to expand)"),
            hidden => format!("no matches: {query} ({hidden} hidden by folds — R to expand)"),
        };
        self.set_status(StatusMessage::new(message));
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
            KeyCode::Char('d') => self.move_page(self.half_page() as isize),
            KeyCode::Char('u') => self.move_page(-(self.half_page() as isize)),
            KeyCode::Char('f') => self.move_page(self.page_size() as isize),
            KeyCode::Char('b') => self.move_page(-(self.page_size() as isize)),
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
            // Every motion here moves the *reading line*; the viewport follows
            // it. See [`AppState::cursor`] for why the model changed.
            KeyCode::Up | KeyCode::Char('k') => self.move_cursor(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_cursor(1),
            KeyCode::PageUp => self.move_page(-(self.page_size() as isize)),
            KeyCode::PageDown => self.move_page(self.page_size() as isize),
            KeyCode::Home | KeyCode::Char('g') => self.set_cursor(0),
            KeyCode::End | KeyCode::Char('G') => self.set_cursor(self.last_line()),
            // Search (Phase 4). All three were unbound before this phase,
            // so nothing pre-existing is displaced.
            KeyCode::Char('/') => self.begin_search(),
            KeyCode::Char('n') => self.step_match(true),
            KeyCode::Char('N') => self.step_match(false),
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

        // The reading line as a *fraction of the block it is in*, captured
        // before the tree is replaced. See `place_cursor` for why the raw
        // index is not enough.
        let cursor_offset = self.cursor.saturating_sub(previous_scroll);

        self.size = new_size;
        self.tree = self.layout_fitting_the_gutter(ctx, width);
        // Resync to what layout actually used (already clamped), not the
        // raw `width` argument — the two agree today but this is the
        // authoritative value regardless. A caller-driven toggle
        // (`AppState::widen`/`narrow`) and a real terminal resize both go
        // through here, so a resize always overrides a stale toggle.
        self.content_width = self.tree.width();

        let reflowed = document_changed || !self.no_reflow_occurred(previous_width);
        let target = if !reflowed {
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
        // DW-5.5: a fold that just carried the reader's line into a collapsed
        // range overrides the ordinary anchor here — that anchor's own block
        // no longer emits a line of its own once it is inside the range, so
        // it can only ever fall back to the proportional estimate above,
        // which is not the exact marker line DW-5.5 requires. Every other
        // relayout leaves this `None` and this is a no-op.
        let target = self
            .pending_fold_snap
            .take()
            .and_then(|id| self.tree.first_line_of(id))
            .unwrap_or(target);
        self.set_scroll(target);
        self.place_cursor(cursor_offset);
        self.reseat_search(reflowed);
        self.reseat_link_select();
        self.reseat_explore();
        self.recompute_matches(ctx);
    }

    /// Lays the document out at `width`, narrowed by the chrome, sizing the
    /// gutter to the line count the result actually has.
    ///
    /// **The gutter can chase its own tail.** Its width comes from the
    /// document's line count; it narrows the content column; a narrower column
    /// rewraps the document into *more* lines; more lines can need another
    /// digit. So this lays out at most twice and keeps the **wider** of the
    /// two gutters — never the second, which could be the narrower one and
    /// would clip a number the first pass proved was needed.
    ///
    /// It terminates because the second pass is unconditional in the only
    /// direction that matters: narrowing the content can only add lines, so
    /// the digit count is monotone across the pair and one extra pass reaches
    /// a width that fits. A third pass could differ only if a single cell of
    /// content crossed a power-of-ten boundary in line count, which costs a
    /// gutter one cell wider than strictly needed — invisible, and a great
    /// deal cheaper than laying a ten-thousand-line document out until a fixed
    /// point falls out.
    fn layout_fitting_the_gutter(&self, ctx: &LayoutContext, width: u16) -> LayoutTree {
        let lay_out = |width: u16| {
            layout_with_folds(
                ctx.doc,
                width,
                ctx.config,
                ctx.engine,
                ctx.sizer,
                &self.folds,
            )
        };
        // `width` is a content column the caller already took chrome out of,
        // using the gutter the *current* tree needs. Seeded from that same
        // tree, so `budgeted` is the number the caller budgeted for and the
        // comparison below is against the right thing.
        let budgeted = painter::gutter_width(self.chrome, self.tree.line_count());
        let tree = lay_out(width);
        let needed = painter::gutter_width(self.chrome, tree.line_count());
        let Some(growth) = needed.checked_sub(budgeted).filter(|n| *n > 0) else {
            // The common case by a wide margin, and the only one that costs a
            // single layout: a gutter that did not grow needs no more room. It
            // may have *shrunk*, which leaves the page a cell wider than it
            // strictly needs to be — invisible, and not worth a second pass.
            return tree;
        };
        lay_out(width.saturating_sub(growth).max(ctx.config.min_width))
    }

    /// Puts the reading line back after a relayout, `offset` rows below the
    /// top of the page — where it was before the tree was replaced.
    ///
    /// Anchored to the *viewport* rather than to a block, unlike the scroll
    /// anchor above, and the asymmetry is deliberate. The scroll anchor answers
    /// "which text was the reader looking at", which survives a rewrap because
    /// the text does. The reading line answers "which row was the reader on",
    /// and after a rewrap that row's content is somewhere else — so the honest
    /// reconstruction is the reader's position *on the screen*, which is what
    /// they were actually looking at when the terminal changed shape under
    /// them. `reseat_cursor` then clamps it onto the page.
    fn place_cursor(&mut self, offset: usize) {
        self.cursor = self.scroll.saturating_add(offset).min(self.last_line());
        self.reseat_cursor();
    }

    /// Puts an open search prompt's `origin` back on a line that exists in
    /// the tree installed now.
    ///
    /// `Mode::Search { origin }` is where `Esc` returns the reader, captured
    /// as a raw line index when `/` was pressed — the same shape, and the
    /// same staleness, as the `toc_return_scroll` [`AppState::reseat_toc`]
    /// exists to fix. A `--watch` reload replaces the document under a live
    /// prompt and a resize rewraps it; either way the old index names
    /// different text afterwards, and `Esc` would drop the reader somewhere
    /// they never were.
    ///
    /// The answer is the same one the TOC gives, for the same reason: after
    /// the tree is replaced, the anchored current position is the best
    /// available account of where the reader is, and a defensible position
    /// beats a precisely wrong one. The cost is honest and worth stating —
    /// resize or reload mid-query and `Esc` returns you to where the reflow
    /// left you rather than to the line you pressed `/` on.
    ///
    /// Skipped entirely when nothing reflowed: `relayout` runs on every theme
    /// swap too, and there the tree is identical, so the index still means
    /// exactly what it meant and re-seating it would throw away a good answer.
    fn reseat_search(&mut self, reflowed: bool) {
        let Mode::Search { .. } = self.mode else {
            return;
        };
        if !reflowed {
            return;
        }
        self.mode = Mode::Search {
            origin: self.scroll,
        };
    }

    /// Re-runs the active search against the tree that is installed *now*.
    ///
    /// **The plan's Phase 4 assumption — "match positions survive relayout
    /// without full recomputation" — is false, and this is the correctness
    /// answer to it.** A [`Match`] is addressed by tree line index and by a
    /// byte offset into that line's *laid-out* text. A relayout at a new
    /// width rewraps paragraphs: both the line index and the split points
    /// move, so every match but its `block` would be pointing at text that
    /// is no longer there — and the painter would highlight whatever now
    /// occupies those bytes. Recomputing is cheap next to `layout()` itself
    /// (one literal scan of already-materialized text, no parse and no
    /// width measurement) and it happens on the resize path, not the
    /// per-keystroke one.
    ///
    /// `current` is clamped rather than re-derived: the reader was on the
    /// n-th match before the resize and, since matching is deterministic and
    /// the document did not change, the n-th match after it is the same
    /// text. Only a query whose match count somehow shrank needs the clamp.
    ///
    /// **Phase 5's answer to "a search match inside a folded range."** A
    /// folded section contributes no lines to the tree at all — only its one
    /// marker line does — so `find_matches` simply cannot see text that is
    /// not there, and a match inside it is silently absent from the
    /// recomputed set rather than merely skipped over. That is *skip*, not
    /// *expand*: `n`/`N` step past it as if it did not match, and it
    /// reappears the moment the section is unfolded and this runs again.
    ///
    /// The choice to skip is only defensible if the reader can tell it
    /// happened, which is what `ctx` is for here: it also recomputes
    /// [`SearchState::hidden_by_folds`], the count [`AppState::report_no_matches`]
    /// needs to say "3 hidden by folds" instead of the flatly false "no
    /// matches" a query that matched — just not anywhere currently visible —
    /// would otherwise get.
    fn recompute_matches(&mut self, ctx: &LayoutContext) {
        if self.search.query.is_empty() {
            return;
        }
        self.search.matches = find_matches(&self.tree, &self.search);
        self.search.current = self
            .search
            .current
            .min(self.search.matches.len().saturating_sub(1));
        self.search.hidden_by_folds = self.matches_hidden_by_folds(ctx);
    }

    /// How many matches of the active query exist in the document but not in
    /// `self.tree` because a fold currently hides them — `0` when nothing is
    /// folded, without paying for the extra layout pass below at all, which
    /// is the overwhelmingly common case (every relayout that is not
    /// fold-related still calls [`AppState::recompute_matches`]).
    ///
    /// Answered by laying the *same* document out fold-free at the *same*
    /// width and taking the difference in match count. This is the only way
    /// to answer it: the folded-away text is not in `self.tree` — that is
    /// the entire premise of folding — so nothing already in `AppState` can
    /// see it. The extra pass costs one more `layout()` on the relayout path
    /// (never per keystroke — see [`AppState::refresh_incremental`]'s doc for
    /// why it cannot run there), and only when a fold is actually active.
    fn matches_hidden_by_folds(&self, ctx: &LayoutContext) -> usize {
        if self.folds.collapsed.is_empty() {
            return 0;
        }
        let unfolded = layout(
            ctx.doc,
            self.content_width,
            ctx.config,
            ctx.engine,
            ctx.sizer,
        );
        let total = find_matches(&unfolded, &self.search).len();
        total.saturating_sub(self.search.matches.len())
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
            // The terminal's width, less the chrome — the resize is one of the
            // two places a terminal width becomes a layout width, and the only
            // one that happens more than once. See `content_width_in`.
            let width = self.content_width_in(last.width);
            self.relayout(ctx, width, last);
        }
    }

    /// Applies `chrome` and re-lays the document out to the width it leaves.
    ///
    /// A relayout rather than a repaint, because the gutter and the padding
    /// are cells the document does not get: turning the gutter on with `#`
    /// rewraps the page, exactly as `-` does. The scroll anchor is preserved
    /// through the same path every other chrome mutation uses, so the reader
    /// keeps their place across the rewrap.
    pub fn apply_chrome(&mut self, chrome: Chrome, ctx: &LayoutContext) {
        self.chrome = chrome;
        let width = self.content_width_in(self.size.width);
        self.content_width = width;
        let size = self.size;
        self.relayout(ctx, width, size);
    }
}

/// Every match of `search`'s query in `tree`, in tree order.
///
/// Matching runs over the **laid-out** line text, per the phase constraint,
/// because that is the only text a match can be addressed in: a match has to
/// name a line and a column range for the painter to restyle, and source
/// text has neither. Lines are joined per top-level block so a match can
/// straddle a wrap boundary and still be found as one match (DW-4.4).
///
/// The accepted cost of matching post-layout: greedy wrapping *consumes* the
/// space it breaks at, so a paragraph wrapped between "hello" and "world"
/// joins as `helloworld`. A query for the two-word phrase does not match
/// across that break; a query for `lowo` does. Matching source text instead
/// would fix the former and make every match unaddressable, which is the
/// worse trade for a viewer whose whole job is highlighting what it found.
///
/// One `String` and one index vector are reused across blocks rather than
/// allocated per block — this runs on every keystroke.
fn find_matches(tree: &LayoutTree, search: &SearchState) -> Vec<Match> {
    if search.query.is_empty() {
        return Vec::new();
    }
    let case_sensitive = search.case_sensitive();
    let mut matches = Vec::new();
    let mut joined = String::new();
    let mut starts: Vec<(usize, usize)> = Vec::new();
    let mut line = 0;
    while line < tree.line_count() {
        // A line belonging to no block (the gaps between them) can hold no
        // addressable match — a `Match` names a block — so it is skipped
        // rather than joined into a neighbour it does not belong to.
        let Some(block) = tree.block_at(line) else {
            line += 1;
            continue;
        };
        joined.clear();
        starts.clear();
        let mut past = line;
        while past < tree.line_count() && tree.block_at(past) == Some(block) {
            starts.push((past, joined.len()));
            append_line_text(tree, past, &mut joined);
            past += 1;
        }
        collect_block_matches(
            block,
            &joined,
            &starts,
            &search.query,
            case_sensitive,
            &mut matches,
        );
        line = past;
    }
    matches
}

/// Appends every match of `query` in one block's joined text to `out`,
/// translating each back to a `(line, byte range from that line's start)`
/// address. Matches are non-overlapping: the scan resumes past the end of
/// each one, so `aa` finds two matches in `aaaa`, not three.
fn collect_block_matches(
    block: NodeId,
    joined: &str,
    starts: &[(usize, usize)],
    query: &str,
    case_sensitive: bool,
    out: &mut Vec<Match>,
) {
    let mut from = 0usize;
    while let Some((at, len)) = find_from(joined, query, from, case_sensitive) {
        let Some((line, line_start)) = line_containing(starts, at) else {
            return;
        };
        out.push(Match {
            block,
            line,
            range: (at - line_start)..(at + len - line_start),
        });
        from = at + len;
    }
}

/// The `(line index, offset in the join)` entry covering byte `at`: the last
/// line whose own offset does not exceed it. `None` only for an empty
/// `starts`, which cannot happen for an `at` that came from a real match —
/// returned rather than indexed so a future caller cannot turn that into a
/// panic on a painted frame.
fn line_containing(starts: &[(usize, usize)], at: usize) -> Option<(usize, usize)> {
    let index = starts
        .partition_point(|&(_, start)| start <= at)
        .saturating_sub(1);
    starts.get(index).copied()
}

/// Byte offset and byte length of the first match of `needle` in `haystack`
/// at or after `from`.
///
/// The length is measured **in the haystack**, not in the needle: a
/// case-insensitive match can consume a different number of bytes than the
/// query occupies (`İ` and `i` are one byte apart in UTF-8), and it is the
/// haystack's bytes that have to be highlighted.
fn find_from(
    haystack: &str,
    needle: &str,
    from: usize,
    case_sensitive: bool,
) -> Option<(usize, usize)> {
    for (offset, _) in haystack.get(from..)?.char_indices() {
        let at = from + offset;
        if let Some(len) = match_len_at(&haystack[at..], needle, case_sensitive) {
            return Some((at, len));
        }
    }
    None
}

/// How many bytes of `text` `needle` matches at `text`'s start, or `None`.
///
/// Compares one `char` at a time rather than lowercasing either side first.
/// `str::to_lowercase` is not length-preserving — U+0130 lowercases to two
/// chars — so folding the haystack would shift every byte offset after such
/// a character and the highlight would land on the wrong text. Per-char
/// comparison keeps haystack offsets exact by construction. The price is no
/// full Unicode case folding (`İ` will not match `i̇`), which is inside the
/// phase's literal-matching scope.
fn match_len_at(text: &str, needle: &str, case_sensitive: bool) -> Option<usize> {
    let mut consumed = 0usize;
    let mut haystack = text.chars();
    for wanted in needle.chars() {
        let found = haystack.next()?;
        if !chars_match(found, wanted, case_sensitive) {
            return None;
        }
        consumed += found.len_utf8();
    }
    Some(consumed)
}

fn chars_match(found: char, wanted: char, case_sensitive: bool) -> bool {
    found == wanted || (!case_sensitive && found.to_lowercase().eq(wanted.to_lowercase()))
}

/// Index of the first match at or after `line`, wrapping to the first match
/// in the document when every match is above it — so opening a search near
/// the tail still lands somewhere rather than nowhere.
fn first_match_at_or_after(matches: &[Match], line: usize) -> usize {
    matches
        .iter()
        .position(|found| found.line >= line)
        .unwrap_or(0)
}

/// Appends the painted text of tree line `index` to `out`: its runs
/// concatenated, in paint order.
///
/// This is the string search offsets and paint offsets are both measured in,
/// which is why it includes container prefix runs (a blockquote's gutter
/// bar, a list marker) — they occupy real bytes on the painted row, and an
/// offset that skipped them would restyle the wrong cells. A reserved media
/// box contributes nothing: it carries no text.
pub(crate) fn append_line_text(tree: &LayoutTree, index: usize, out: &mut String) {
    let Some(Line::Items(items)) = tree.lines(index..index + 1).next() else {
        return;
    };
    for item in searchable(items) {
        if let LineItem::Run(run) = item {
            out.push_str(&run.text);
        }
    }
}

/// A line's items with its trailing blank runs dropped.
///
/// Layout pads two kinds of line out to the measure so a background reads as a
/// band rather than stopping where the words do: a top-level heading (the H1
/// wash) and every line of a code block (the code slab). Those cells are
/// scenery. Letting them into the searchable text put real whitespace between
/// things a reader sees as adjacent — a search for `{}` across
///
/// ```text
/// fn foo() {
///
/// }
/// ```
///
/// stopped matching, because the blank line between the braces had become a
/// full measure of pad. Trailing blanks are dropped rather than all blanks
/// because a code block's *indentation* is content: `    println!` has to stay
/// findable by its leading spaces.
///
/// Dropping only from the end is also what keeps this compatible with the
/// painter, which walks byte offsets across every run including the pad. Every
/// offset before the pad is unchanged by definition, so a match's range still
/// lands on the same bytes.
fn searchable(items: &[LineItem]) -> &[LineItem] {
    let end = items
        .iter()
        .rposition(|item| match item {
            LineItem::Run(run) => !run.text.chars().all(char::is_whitespace),
            LineItem::Box(_) => true,
        })
        .map_or(0, |last| last + 1);
    &items[..end]
}

/// Byte length of [`append_line_text`]'s output for one line, without
/// building the string. The painter walks this to project a match's range
/// across the lines it spans.
pub(crate) fn line_text_len(tree: &LayoutTree, index: usize) -> usize {
    let Some(Line::Items(items)) = tree.lines(index..index + 1).next() else {
        return 0;
    };
    // Must agree with `append_line_text` item for item, or a match's range
    // projects onto bytes nobody searched.
    searchable(items)
        .iter()
        .filter_map(|item| match item {
            LineItem::Run(run) => Some(run.text.len()),
            LineItem::Box(_) => None,
        })
        .sum()
}

/// The destination a run paints part of, if it paints part of a link.
///
/// `Run.aux` is a channel whose meaning is fixed by `style_id` — layout's own
/// doc says so: a link destination for a `Semantic::Link` run, a code-fence
/// info string for a `Semantic::CodeBlock` one.
///
/// Keying on `Semantic::Link` alone would still be wrong, and measurably so.
/// Inside a link, layout styles inline markup by *its* role and keeps the
/// destination on the run anyway: `[**bold** text](x)` emits a
/// `Semantic::Strong` run carrying `x`. A link whose text is entirely bold
/// would then be invisible to `Tab` and unclickable, while looking exactly
/// like every other link on the screen.
///
/// So the rule is "carries an aux, and is not the one role that means
/// something else by it", written as a match rather than a `!=` so a third
/// consumer of `aux` has to be considered here.
fn link_dest(run: &Run) -> Option<&str> {
    let dest = run.aux.as_deref()?;
    match run.style_id {
        StyleId::Semantic(Semantic::CodeBlock) => None,
        StyleId::Semantic(_) | StyleId::Capture(_) => Some(dest),
    }
}

/// Whether everything strictly between items `from` and `to` is blank text.
///
/// Load-bearing for grouping: layout emits each *word* of a link as its own
/// run with a separate, aux-less space run between them, so "consecutive
/// indices" would count a three-word link as three links. A blank run bridges;
/// a media box or any run with visible glyphs does not.
fn only_blank_between(items: &[LineItem], from: usize, to: usize) -> bool {
    items
        .get(from + 1..to)
        .is_some_and(|between| between.iter().all(is_blank_run))
}

fn is_blank_run(item: &LineItem) -> bool {
    match item {
        LineItem::Run(run) => run.text.trim().is_empty(),
        LineItem::Box(_) => false,
    }
}

/// The link run-groups on one line, left to right: `(destination, text,
/// span)` per maximal group of same-destination link runs, bridged across the
/// blank runs layout puts between a link's words.
///
/// A media box, or any visible non-link text, between two link runs breaks the
/// group — they really are two links that happen to share a destination.
fn link_groups(items: &[LineItem], line: usize) -> Vec<(String, String, LinkSpan)> {
    let mut groups: Vec<(String, String, LinkSpan)> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let LineItem::Run(run) = item else {
            continue;
        };
        let Some(dest) = link_dest(run) else {
            continue;
        };
        let extends = groups.last().is_some_and(|(target, _, span)| {
            target == dest && only_blank_between(items, span.last_item, index)
        });
        match groups.last_mut() {
            Some((_, existing, span)) if extends => {
                // The bridged blanks are part of the link's text as the reader
                // reads it, so they come along rather than being elided.
                for bridged in &items[span.last_item + 1..index] {
                    if let LineItem::Run(blank) = bridged {
                        existing.push_str(&blank.text);
                    }
                }
                existing.push_str(&run.text);
                span.last_item = index;
            }
            Some(_) | None => groups.push((
                dest.to_string(),
                run.text.clone(),
                LinkSpan {
                    line,
                    first_item: index,
                    last_item: index,
                },
            )),
        }
    }
    groups
}

/// The first code fence in `node`'s subtree, in document order, as its
/// literal source text.
///
/// Iterative rather than recursive for the reason [`Document::nodes`] is: a
/// pathologically nested document must not put a viewer's stack at risk on a
/// keystroke.
fn first_code_literal(node: NodeRef<'_>) -> Option<String> {
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        if let NodeRef::Block(block) = node
            && let BlockKind::CodeBlock { literal, .. } = &block.kind
        {
            return Some(literal.clone());
        }
        let mut children: Vec<NodeRef<'_>> = node.children().collect();
        children.reverse();
        stack.extend(children);
    }
    None
}

#[cfg(test)]
mod tests {
    use ast::Document;
    use layout::NullSizer;
    use width::WidthConfig;

    use super::*;
    use layout::Padding;

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

    /// Puts the top of the page on line `top` by pressing `Down`, and leaves
    /// the reading line there too.
    ///
    /// Through the real key path rather than by reaching into `scroll`,
    /// because a back door into a private field is a setup that cannot catch a
    /// motion regression. What it hides is arithmetic, not behaviour: under
    /// the reading-line model `Down` moves the reader and the page only
    /// follows once the reader reaches its bottom edge, so reaching a given
    /// *top* takes one press per page row more than it used to. A dozen tests
    /// here want nothing more than "a reader partway down a document", and
    /// that count is not what any of them are about.
    fn scroll_to(state: &mut AppState, top: usize) {
        // One press per row of the document is the most that can ever be
        // needed, and a bound is what turns "this fixture is too short" into a
        // failed assertion rather than a hung test run.
        let cap = state.tree().line_count() + 1;
        for _ in 0..cap {
            if state.scroll() >= top {
                break;
            }
            assert!(
                !state.handle_key_event(plain(KeyCode::Down)),
                "Down must not ask to quit"
            );
        }
        assert_eq!(
            state.scroll(),
            top,
            "could not scroll to {top}: the fixture bottoms out at {}",
            state.max_scroll()
        );
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
        assert_eq!(state.cursor(), 0);

        // A line key moves the *reader*. The page does not budge until the
        // reader reaches its bottom edge — that is the reading-line model,
        // and it is what makes the band mean something.
        assert!(!state.handle_key_event(plain(KeyCode::Down)));
        assert_eq!(state.cursor(), 1);
        assert_eq!(state.scroll(), 0, "one line down is not one page scrolled");
        assert!(!state.handle_key_event(plain(KeyCode::Up)));
        assert_eq!(state.cursor(), 0);
        // Up at the top must clamp, not underflow.
        assert!(!state.handle_key_event(plain(KeyCode::Up)));
        assert_eq!(state.cursor(), 0);
        assert_eq!(state.scroll(), 0);

        // A page key moves the *page*, and carries the reader with it.
        assert!(!state.handle_key_event(plain(KeyCode::PageDown)));
        assert_eq!(state.scroll(), state.page_size());
        assert_eq!(state.cursor(), state.page_size());

        assert!(!state.handle_key_event(plain(KeyCode::Char('G'))));
        assert_eq!(state.scroll(), state.max_scroll());
        assert_eq!(
            state.cursor(),
            state.tree().line_count() - 1,
            "`G` must reach the last line, not merely the last screen"
        );
        assert!(!state.handle_key_event(plain(KeyCode::End)));
        assert_eq!(state.scroll(), state.max_scroll());

        assert!(!state.handle_key_event(plain(KeyCode::Char('g'))));
        assert_eq!(state.scroll(), 0);
        assert_eq!(state.cursor(), 0);
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
        scroll_to(&mut state, 100);
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
        scroll_to(&mut state, 40);
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

        scroll_to(&mut state, 100);
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

        scroll_to(&mut state, 200);
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

        scroll_to(&mut state, 60);
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
        assert_eq!(state.cursor(), 1);

        assert!(
            state.handle_key_event(ctrl('c')),
            "Ctrl-C must request quit"
        );
        // Quitting is all it does — it must not move the reader on the way out.
        assert_eq!(state.cursor(), 1);

        // A bare `c` is not a quit key and never was.
        assert!(!state.handle_key_event(plain(KeyCode::Char('c'))));
        assert_eq!(state.cursor(), 1);
    }

    // ---- The reading line -------------------------------------------------

    /// The band exists whether or not it is painted, and turning the paint off
    /// changes no motion.
    ///
    /// The rule that keeps the model honest: a key that means different things
    /// depending on a display setting is worse than either meaning, so
    /// `current_line = false` hides the band and leaves `j` alone.
    #[test]
    fn test_hiding_the_band_does_not_change_a_single_motion() {
        let mut painted = build(&non_reflowing_source(50), 40, 10).3;
        let mut hidden = build(&non_reflowing_source(50), 40, 10).3;
        hidden.set_chrome(Chrome {
            current_line: false,
            ..Chrome::default()
        });

        for key in [
            KeyCode::Down,
            KeyCode::Down,
            KeyCode::PageDown,
            KeyCode::Up,
            KeyCode::Char('G'),
            KeyCode::Char('g'),
        ] {
            painted.handle_key_event(plain(key));
            hidden.handle_key_event(plain(key));
            assert_eq!(
                (painted.scroll(), painted.cursor()),
                (hidden.scroll(), hidden.cursor()),
                "{key:?} moved differently with the band switched off"
            );
        }
        assert!(painted.page().cursor.is_some(), "the band must be painted");
        assert!(
            hidden.page().cursor.is_none(),
            "the band must not be painted"
        );
    }

    /// `scrolloff` keeps rows of context below the reading line, and gives
    /// them up at the ends of the document.
    ///
    /// Without the exception a reader who pressed `G` would land two rows
    /// short of the last line with no way to reach it.
    #[test]
    fn test_scrolloff_keeps_context_but_not_at_the_documents_ends() {
        let (_doc, _config, _engine, mut state) = build(&non_reflowing_source(50), 40, 10);
        state.set_chrome(Chrome {
            scrolloff: 3,
            ..Chrome::default()
        });

        // Walking down, the page starts moving three rows early.
        for _ in 0..7 {
            state.handle_key_event(plain(KeyCode::Down));
        }
        assert_eq!(state.cursor(), 7);
        assert_eq!(
            state.scroll(),
            1,
            "with three rows of margin the page must lead the reader by one"
        );

        // But the last line is still reachable.
        state.handle_key_event(plain(KeyCode::Char('G')));
        assert_eq!(
            state.cursor(),
            state.tree().line_count() - 1,
            "scrolloff must not fence the reader off the end of the document"
        );
        state.handle_key_event(plain(KeyCode::Char('g')));
        assert_eq!(
            state.cursor(),
            0,
            "nor off the start — there is no context above line 0 to keep"
        );
    }

    /// A page key moves the page; a line key moves the reader.
    ///
    /// Moving the reading line by a page and letting the viewport chase it
    /// gives a page key that scrolls by exactly one row — the reading line
    /// lands on the bottom row, which is already visible, so nothing has to
    /// move. That is not what anyone pressing `PgDn` is asking for, and it is
    /// the bug this pins.
    #[test]
    fn test_a_page_key_moves_the_page_and_a_line_key_does_not() {
        let (_doc, _config, _engine, mut state) = build(&non_reflowing_source(50), 40, 10);

        state.handle_key_event(plain(KeyCode::PageDown));
        assert_eq!(state.scroll(), 10, "a page key moves the page");
        assert_eq!(state.cursor(), 10, "and carries the reader with it");

        let before = state.scroll();
        state.handle_key_event(plain(KeyCode::Down));
        assert_eq!(state.scroll(), before, "a line key does not move the page");
        assert_eq!(state.cursor(), 11, "it moves the reader");
    }

    /// Turning the gutter on rewraps the page, because the gutter takes cells
    /// the document was laid out into.
    ///
    /// A repaint would leave the last few characters of every line clipped
    /// against the page's new right edge.
    #[test]
    fn test_turning_the_gutter_on_narrows_the_content_column() {
        // Narrower than `LayoutConfig`'s 100-cell cap, so the content column is
        // limited by the *terminal* and the gutter has something to take a bite
        // out of. On a wide terminal the cap absorbs the gutter and the tree's
        // width does not move — true, and not what this test is about.
        let (doc, config, engine, mut state) = build(&numbered_paragraphs(60), 80, 10);
        let ctx = ctx_for(&doc, &config, &engine);
        let before = state.tree().width();

        state.apply_chrome(
            Chrome {
                line_numbers: true,
                ..Chrome::default()
            },
            &ctx,
        );
        assert!(
            state.tree().width() < before,
            "the gutter must come out of the content column, not overlap it: \
             {before} -> {}",
            state.tree().width()
        );
        let page = state.page();
        assert_eq!(
            page.gutter + page.content + page.origin.x,
            state.size().width,
            "the gutter, the page and the margin must tile the viewport exactly"
        );
    }

    /// A click resolves the glyph the reader aimed at, not the one a gutter's
    /// width to the left of it.
    ///
    /// The whole of the padding bug this guards: with a gutter on, every click
    /// landed a few cells left of the target, so the last character of each
    /// link stopped working and the first character of the next word started
    /// opening it.
    #[test]
    fn test_a_click_is_resolved_through_the_gutter_and_the_margin() {
        let source = "[alpha](https://example.com/a) tail\n";
        let (doc, config, engine, mut state) = build(source, 60, 10);
        let ctx = ctx_for(&doc, &config, &engine);
        state.apply_chrome(
            Chrome {
                line_numbers: true,
                padding: Padding {
                    left: 2,
                    ..Padding::default()
                },
                ..Chrome::default()
            },
            &ctx,
        );
        let page = state.page();
        let text_x = page.origin.x + page.gutter;

        // The link's first cell, in the terminal's own coordinates.
        assert!(
            state.handle_mouse_event(click_at(text_x, 0), &engine),
            "a click on the link's first cell must activate it"
        );
        assert!(matches!(
            state.take_action(),
            Some(PendingAction::OpenLink(url)) if url == "https://example.com/a"
        ));

        // The same cell counted from column 0 — where the click *used* to be
        // resolved — is inside the gutter, and a line number is not a link.
        assert!(
            !state.handle_mouse_event(click_at(0, 0), &engine),
            "a click on the gutter must do nothing at all"
        );
        assert!(state.take_action().is_none());
    }

    /// `j`/`k` are `Down`/`Up`: one line, clamped at 0 and at the tail.
    #[test]
    fn test_vim_j_and_k_move_one_line_and_clamp_at_both_ends() {
        let (_doc, _config, _engine, mut state) = build(&non_reflowing_source(50), 40, 10);
        let tail = state.max_scroll();
        let last = state.tree().line_count() - 1;
        assert!(tail > 3, "the fixture must be taller than the viewport");

        assert!(!state.handle_key_event(plain(KeyCode::Char('j'))));
        assert_eq!(state.cursor(), 1);
        assert!(!state.handle_key_event(plain(KeyCode::Char('j'))));
        assert_eq!(state.cursor(), 2);
        assert!(!state.handle_key_event(plain(KeyCode::Char('k'))));
        assert_eq!(state.cursor(), 1);
        assert_eq!(state.scroll(), 0, "none of that reached the page's edge");

        // At the top, `k` clamps instead of underflowing.
        state.handle_key_event(plain(KeyCode::Char('k')));
        assert_eq!(state.cursor(), 0);
        state.handle_key_event(plain(KeyCode::Char('k')));
        assert_eq!(state.cursor(), 0);

        // At the end, `j` clamps instead of running off it.
        state.handle_key_event(plain(KeyCode::Char('G')));
        assert_eq!(state.cursor(), last);
        assert_eq!(state.scroll(), tail);
        state.handle_key_event(plain(KeyCode::Char('j')));
        assert_eq!(state.cursor(), last);
        assert_eq!(
            state.scroll(),
            tail,
            "clamping must not scroll past the end"
        );
        state.handle_key_event(plain(KeyCode::Char('k')));
        assert_eq!(state.cursor(), last - 1);
        assert_eq!(
            state.scroll(),
            tail,
            "the reader left the bottom row, the page did not move"
        );
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
        assert_eq!(
            state.cursor(),
            1,
            "Ctrl-Down must still move the reader down"
        );

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

        scroll_to(&mut state, 30);
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

        scroll_to(&mut state, 30);
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
        assert_eq!(state.cursor(), 0, "a lone `]` must not move the reader");

        // `]` then `j` is one line down, not a heading jump.
        assert!(!state.handle_key_event(plain(KeyCode::Char('j'))));
        assert_eq!(state.cursor(), 1);

        // A mismatched pair (`]` then `[`) arms the second bracket rather
        // than jumping; the *next* `[` completes it.
        state.handle_key_event(plain(KeyCode::Char(']')));
        state.handle_key_event(plain(KeyCode::Char('[')));
        assert_eq!(state.cursor(), 1, "`][` is not a motion");
        state.handle_key_event(plain(KeyCode::Char('[')));
        assert_eq!(state.cursor(), 0, "the following `[` completes `[[`");
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
        assert!(
            rows[0].style == RowStyle::Selected,
            "the reader is under the first heading"
        );
        assert_eq!(
            rows.iter()
                .filter(|row| row.style == RowStyle::Selected)
                .count(),
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
        scroll_to(&mut state, 7);
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
            let selected: Vec<&OverlayRow> = rows
                .iter()
                .filter(|row| row.style == RowStyle::Selected)
                .collect();
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
            rows.last().unwrap().style == RowStyle::Selected,
            "`G` selects the last heading"
        );
        assert!(rows.last().unwrap().text.contains("Heading 29"));

        state.handle_key_event(plain(KeyCode::Char('g')));
        let rows = state.toc_rows(5);
        assert!(
            rows[0].style == RowStyle::Selected,
            "`g` selects the first heading"
        );
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
            rows[2].style == RowStyle::Selected && rows[2].text.contains("Heading 2"),
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

    // ---- Incremental search (Phase 4) -------------------------------------

    /// Drives a whole search the way a reader does — `/`, the query one
    /// keystroke at a time, `Enter` — and hands back the matches. Every
    /// search test goes through the real key table rather than calling the
    /// private helpers, so a binding that stops being reachable fails here.
    fn search_for(state: &mut AppState, query: &str) -> Vec<Match> {
        state.handle_key_event(plain(KeyCode::Char('/')));
        for c in query.chars() {
            state.handle_key_event(plain(KeyCode::Char(c)));
        }
        state.handle_key_event(plain(KeyCode::Enter));
        state.search().matches.clone()
    }

    /// 80 numbered paragraphs, each one line at any width used here — so a
    /// match's line index is stable and a search for `paragraph 60` has
    /// exactly one hit, far below the top of a 10-row viewport.
    fn numbered_paragraphs(n: usize) -> String {
        (0..n).map(|i| format!("paragraph {i:02}\n\n")).collect()
    }

    /// The text a `Match` actually addresses, sliced back out of the tree it
    /// was computed against. This is the oracle every search test asserts
    /// through: a `(line, range)` pair that merely *exists* proves nothing,
    /// but one that slices to the query proves it points at real text.
    fn matched_text(state: &AppState, found: &Match) -> String {
        let mut text = String::new();
        let mut line = found.line;
        while line < state.tree().line_count() && text.len() < found.range.len() + found.range.start
        {
            append_line_text(state.tree(), line, &mut text);
            line += 1;
        }
        text[found.range.clone()].to_string()
    }

    #[test]
    fn test_dw_4_1_slash_opens_a_prompt_typing_updates_it_and_esc_restores_the_scroll_position() {
        let (_doc, _config, _engine, mut state) = build(&numbered_paragraphs(80), 40, 10);
        assert_eq!(state.mode(), Mode::Normal);
        assert_eq!(state.scroll(), 0);

        // `/` opens the prompt, and it takes over the status row from the
        // ruler immediately — before a single character is typed.
        state.handle_key_event(plain(KeyCode::Char('/')));
        assert!(matches!(state.mode(), Mode::Search { .. }));
        assert_eq!(state.status().message.as_deref(), Some("/"));

        // Typing updates the prompt.
        for c in "paragraph 60".chars() {
            state.handle_key_event(plain(KeyCode::Char(c)));
        }
        assert_eq!(state.search().query, "paragraph 60");
        let prompt = state.status().message.expect("the prompt owns the row");
        assert!(
            prompt.starts_with("/paragraph 60"),
            "the row must show what was typed: {prompt:?}"
        );

        // The fixture's match is far below a 10-row viewport, so incremental
        // search must have moved the reader — otherwise `Esc` restoring the
        // position below would be restoring nothing and prove nothing.
        assert!(
            state.scroll() > 0,
            "typing must scroll an off-screen match into view"
        );

        state.handle_key_event(plain(KeyCode::Esc));
        assert_eq!(state.mode(), Mode::Normal);
        assert_eq!(
            state.scroll(),
            0,
            "Esc must restore the position `/` was pressed at"
        );
        assert!(state.search().query.is_empty());
        assert!(state.search().matches.is_empty());
    }

    /// `Esc` has to restore from wherever the reader started, not from the
    /// top — and it has to work after the query has been backspaced away,
    /// which is the state a reader lands in when they change their mind one
    /// character at a time.
    #[test]
    fn test_dw_4_1_backspace_shortens_the_query_and_esc_from_an_empty_query_still_restores() {
        let (_doc, _config, _engine, mut state) = build(&numbered_paragraphs(80), 40, 10);
        scroll_to(&mut state, 20);
        let origin = state.scroll();
        assert_eq!(origin, 20);

        state.handle_key_event(plain(KeyCode::Char('/')));
        for c in "paragraph 70".chars() {
            state.handle_key_event(plain(KeyCode::Char(c)));
        }
        assert_ne!(state.scroll(), origin, "the match is below the viewport");

        for _ in 0..12 {
            state.handle_key_event(plain(KeyCode::Backspace));
        }
        assert!(state.search().query.is_empty());
        assert_eq!(state.status().message.as_deref(), Some("/"));

        // One more backspace on an already-empty query must not panic.
        state.handle_key_event(plain(KeyCode::Backspace));

        state.handle_key_event(plain(KeyCode::Esc));
        assert_eq!(state.scroll(), origin);
    }

    /// While the prompt is open every printable key is text. `q` in
    /// particular: it is the quit key one keystroke earlier, and a viewer
    /// that exits when you search for "query" is worse than one with no
    /// search at all.
    #[test]
    fn test_a_command_letter_is_just_text_while_a_query_is_being_typed() {
        let (_doc, _config, _engine, mut state) = build(&numbered_paragraphs(80), 40, 10);
        state.handle_key_event(plain(KeyCode::Char('/')));
        for c in "qgGjkn".chars() {
            assert!(
                !state.handle_key_event(plain(KeyCode::Char(c))),
                "`{c}` must not act as a command while typing a query"
            );
        }
        assert_eq!(state.search().query, "qgGjkn");
        assert_eq!(state.scroll(), 0, "no motion key may have moved the reader");

        // Ctrl-C is the deliberate exception: raw mode clears ISIG, so it is
        // the only chord left that can get a stuck reader out.
        assert!(
            state.handle_key_event(ctrl('c')),
            "Ctrl-C must still quit from the prompt"
        );
        // ...and an ordinary chord is swallowed rather than falling through
        // to its normal-mode motion.
        assert!(!state.handle_key_event(ctrl('d')));
        assert_eq!(state.scroll(), 0, "Ctrl-d must not scroll under the prompt");
    }

    #[test]
    fn test_dw_4_2_lowercase_query_is_case_insensitive_and_an_uppercase_one_is_not() {
        let source = "alpha one\n\nAlpha two\n\nALPHA three\n";
        let (_doc, _config, _engine, mut state) = build(source, 40, 10);

        let insensitive = search_for(&mut state, "alpha");
        assert_eq!(
            insensitive.len(),
            3,
            "an all-lowercase query must match every case: {insensitive:?}"
        );
        // Each one addresses real text, in the case the document actually
        // uses — not the case that was typed.
        let matched: Vec<String> = insensitive
            .iter()
            .map(|found| matched_text(&state, found))
            .collect();
        assert_eq!(matched, ["alpha", "Alpha", "ALPHA"]);

        let sensitive = search_for(&mut state, "Alpha");
        assert_eq!(
            sensitive.len(),
            1,
            "one uppercase character must make the whole query exact: {sensitive:?}"
        );
        assert_eq!(matched_text(&state, &sensitive[0]), "Alpha");
    }

    /// Smart case reads `char::is_uppercase`, not `is_ascii_uppercase`, so
    /// the rule means the same thing to a reader typing `Ärger` as to one
    /// typing `Error`. An ASCII-only flag would call this query lowercase
    /// and quietly match both.
    #[test]
    fn test_dw_4_2_smart_case_reads_the_uppercase_flag_from_non_ascii_letters_too() {
        let source = "ärger hier\n\nÄrger dort\n";
        let (_doc, _config, _engine, mut state) = build(source, 40, 10);

        assert_eq!(search_for(&mut state, "ärger").len(), 2);

        let sensitive = search_for(&mut state, "Ärger");
        assert_eq!(
            sensitive.len(),
            1,
            "`Ä` is uppercase, so the query is exact: {sensitive:?}"
        );
        assert_eq!(matched_text(&state, &sensitive[0]), "Ärger");
    }

    #[test]
    fn test_dw_4_3_n_and_shift_n_cycle_forward_and_backward_and_wrap_at_both_ends() {
        // Three hits, spread far enough apart that each step is a real move.
        let mut source = String::new();
        for i in 0..60 {
            if i % 20 == 5 {
                source.push_str("a needle here\n\n");
            } else {
                source.push_str(&format!("filler {i:02}\n\n"));
            }
        }
        let (_doc, _config, _engine, mut state) = build(&source, 40, 10);
        let matches = search_for(&mut state, "needle");
        assert_eq!(matches.len(), 3, "fixture must hold exactly three matches");
        assert_eq!(state.search().current, 0);

        // Forward, then past the end and back to the first.
        for expected in [1, 2, 0] {
            state.handle_key_event(plain(KeyCode::Char('n')));
            assert_eq!(state.search().current, expected);
        }
        // Backward from the first wraps to the last, then keeps stepping down.
        for expected in [2, 1, 0] {
            state.handle_key_event(plain(KeyCode::Char('N')));
            assert_eq!(state.search().current, expected);
        }

        // Traversal is not bookkeeping: the reader is actually taken to each
        // match, and every match still addresses the query text.
        for _ in 0..3 {
            state.handle_key_event(plain(KeyCode::Char('n')));
            let found = &state.search().matches[state.search().current];
            assert_eq!(matched_text(&state, found), "needle");
            let visible = state.scroll()..state.scroll() + state.size().height as usize;
            assert!(
                visible.contains(&found.line),
                "match on line {} is outside the viewport {visible:?}",
                found.line
            );
        }
    }

    #[test]
    fn test_dw_4_5_a_query_with_no_matches_leaves_the_viewport_unmoved_and_says_so() {
        let (_doc, _config, _engine, mut state) = build(&numbered_paragraphs(80), 40, 10);
        scroll_to(&mut state, 25);
        let before = state.scroll();

        state.handle_key_event(plain(KeyCode::Char('/')));
        for c in "zzzznotinthisdocument".chars() {
            state.handle_key_event(plain(KeyCode::Char(c)));
            assert_eq!(
                state.scroll(),
                before,
                "a query with no matches must not move the viewport"
            );
        }
        assert!(state.search().matches.is_empty());
        let prompt = state.status().message.expect("the prompt owns the row");
        assert!(
            prompt.contains("no matches"),
            "the status row must report it: {prompt:?}"
        );

        // Accepting an unmatched query says so too, and still does not move.
        state.handle_key_event(plain(KeyCode::Enter));
        assert_eq!(state.scroll(), before);
        let message = state.status().message.expect("accept must set a message");
        assert!(message.contains("no matches"), "{message:?}");

        // ...and so does `n` afterward, rather than silently doing nothing.
        state.handle_key_event(plain(KeyCode::Char('n')));
        assert_eq!(state.scroll(), before);
        assert!(
            state
                .status()
                .message
                .is_some_and(|m| m.contains("no matches"))
        );
    }

    /// `n` before any search has been made must be silent, not claim the
    /// empty query found nothing.
    #[test]
    fn test_n_before_any_search_reports_nothing_rather_than_no_matches() {
        let (_doc, _config, _engine, mut state) = build(&numbered_paragraphs(20), 40, 10);
        state.handle_key_event(plain(KeyCode::Char('n')));
        assert_eq!(state.status().message, None);
        assert_eq!(state.scroll(), 0);
    }

    /// The empty query matches nothing rather than everything — pressing `/`
    /// and `Enter` must not fill the screen with zero-width highlights.
    #[test]
    fn test_an_empty_query_produces_no_matches_at_all() {
        let (_doc, _config, _engine, mut state) = build(&numbered_paragraphs(20), 40, 10);
        assert!(search_for(&mut state, "").is_empty());
        assert!(state.search_overlay().current.is_none());
    }

    /// The constraint that decides how matching is done at all: a match that
    /// straddles a wrap boundary must be *found*, as one match, with a range
    /// that runs past the end of the line it starts on. Per-line matching
    /// would miss it entirely; this is what makes the painter's two-row
    /// highlight (DW-4.4) possible.
    ///
    /// The wrap point is discovered from the laid-out tree rather than
    /// assumed, so the test cannot silently stop straddling anything if
    /// wrapping ever changes.
    #[test]
    fn test_a_match_straddling_a_wrap_boundary_is_found_as_one_match() {
        let source: String = (0..400).map(|i| format!("w{i:04} ")).collect();
        let (_doc, _config, _engine, mut state) = build(&source, 40, 10);

        // Find a real wrap: two consecutive lines of the same block.
        let block = state.tree().block_at(0).expect("one paragraph, one block");
        assert_eq!(state.tree().block_at(1), Some(block), "it must wrap");
        let mut first = String::new();
        let mut second = String::new();
        append_line_text(state.tree(), 0, &mut first);
        append_line_text(state.tree(), 1, &mut second);

        // A query built from the last word of one line and the first of the
        // next — text that exists only across the boundary.
        let tail: String = first
            .chars()
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let head: String = second.chars().take(3).collect();
        let query = format!("{tail}{head}");

        let matches = search_for(&mut state, &query);
        assert_eq!(
            matches.len(),
            1,
            "expected one straddling match: {matches:?}"
        );
        let found = &matches[0];
        assert_eq!(found.line, 0, "it is anchored on the line it starts on");
        assert!(
            found.range.end > line_text_len(state.tree(), 0),
            "the range must run past line 0's own text ({} bytes) to reach line 1: {:?}",
            line_text_len(state.tree(), 0),
            found.range
        );
        assert_eq!(matched_text(&state, found), query);
    }

    /// A multi-byte query must address whole characters: a range that landed
    /// mid-character would panic the painter's slice, and one that counted
    /// chars instead of bytes would highlight the wrong cells.
    #[test]
    fn test_a_multi_byte_query_addresses_whole_characters() {
        let source = "prefix 日本語 suffix\n\nanother 日本 line\n";
        let (_doc, _config, _engine, mut state) = build(source, 40, 10);
        let matches = search_for(&mut state, "日本");
        assert_eq!(matches.len(), 2);
        for found in &matches {
            assert_eq!(found.range.len(), "日本".len(), "six bytes, not two chars");
            assert_eq!(matched_text(&state, found), "日本");
        }
    }

    /// Matches are non-overlapping: the scan resumes past the end of each
    /// one, so `aa` finds two matches in `aaaa` rather than three.
    #[test]
    fn test_overlapping_occurrences_are_counted_once_each() {
        let (_doc, _config, _engine, mut state) = build("aaaa\n", 40, 10);
        assert_eq!(search_for(&mut state, "aa").len(), 2);
    }

    /// **The plan's Phase 4 assumption, verified false and handled.** Match
    /// positions do *not* survive a relayout: a width change rewraps the
    /// paragraph and both the line index and the byte offsets move. The
    /// contract is that `relayout` recomputes them, and the proof is that
    /// every match still slices to the query in the *new* tree — an
    /// assertion a stale match set fails, because it would be pointing at
    /// whatever now occupies those offsets.
    #[test]
    fn test_matches_are_recomputed_after_a_relayout_that_rewraps_them() {
        let mut source = String::new();
        for i in 0..40 {
            source.push_str(&format!(
                "Paragraph {i:02} carries enough words to wrap to a different number of \
                 lines at ninety cells than it does at thirty, needle included.\n\n"
            ));
        }
        let (doc, config, engine, mut state) = build(&source, 90, 10);
        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };

        let before = search_for(&mut state, "needle");
        assert_eq!(before.len(), 40);
        let lines_before = state.tree().line_count();

        state.apply_resize_burst(
            &ctx,
            &[Size {
                width: 30,
                height: 10,
            }],
        );
        assert_ne!(
            state.tree().line_count(),
            lines_before,
            "the fixture is broken: the resize must actually rewrap, or this \
             test cannot tell a recomputed match set from a stale one"
        );

        let after = &state.search().matches;
        assert_eq!(after.len(), 40, "every match must still be found");
        for found in after {
            assert_eq!(
                matched_text(&state, found),
                "needle",
                "match {found:?} addresses the wrong text in the new tree"
            );
        }
        // At least one of them genuinely moved — otherwise the recomputation
        // could be a no-op and this test would pass on a stale set.
        assert!(
            after.iter().zip(before.iter()).any(|(new, old)| new != old),
            "no match changed address across a reflowing resize"
        );
    }

    /// The width toggle goes through the same `relayout`, so it gets the
    /// same guarantee — this is the path a reader actually hits, pressing
    /// `-` while a search is active.
    #[test]
    fn test_narrowing_while_a_search_is_active_keeps_every_match_addressing_its_text() {
        let source: String = (0..30)
            .map(|i| {
                format!(
                    "Line {i:02} with a target word buried in a sentence long enough to \
                     rewrap when the viewport narrows.\n\n"
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

        assert_eq!(search_for(&mut state, "target").len(), 30);
        for _ in 0..6 {
            state.narrow(&ctx);
        }
        assert_eq!(state.search().matches.len(), 30);
        for found in &state.search().matches {
            assert_eq!(matched_text(&state, found), "target");
        }
    }

    /// The overlay is the painter's whole view of the search, so its
    /// `current` must never name an index that is not there — the painter
    /// would otherwise have to re-check what this method already knows.
    #[test]
    fn test_the_search_overlay_never_reports_a_current_index_it_cannot_back() {
        let (_doc, _config, _engine, mut state) = build(&numbered_paragraphs(40), 40, 10);
        assert!(state.search_overlay().current.is_none());
        assert!(state.search_overlay().matches.is_empty());

        search_for(&mut state, "paragraph 07");
        let overlay = state.search_overlay();
        assert_eq!(overlay.matches.len(), 1);
        assert_eq!(overlay.current, Some(0));

        // Cancelling clears both halves together.
        state.handle_key_event(plain(KeyCode::Char('/')));
        state.handle_key_event(plain(KeyCode::Esc));
        assert!(state.search_overlay().current.is_none());
    }

    // ---- The routing seam: chrome vs. the query ---------------------------

    /// **The regression this exists for.** `main.rs` runs its chrome table
    /// before `AppState::handle_key_event`, so a key the table claims never
    /// reaches the guard that would have protected it. `T`, `+` and `-` were
    /// claimed unconditionally: `/The` searched for `he` and swapped the
    /// theme on the way through.
    ///
    /// Asserted over **every printable ASCII character**, not over the three
    /// that were wrong. The three are today's chrome bindings; the property
    /// is that a mode owning the keyboard owns all of it, and a fourth
    /// binding added tomorrow has to obey it too. That is only checkable by
    /// asking about every character.
    #[test]
    fn test_dw_4_1_no_printable_key_is_claimed_as_chrome_while_a_query_is_open() {
        let (_doc, _config, _engine, mut state) = build(&numbered_paragraphs(40), 40, 10);
        state.handle_key_event(plain(KeyCode::Char('/')));
        assert!(matches!(state.mode(), Mode::Search { .. }));

        for c in ' '..='~' {
            assert_eq!(
                state.chrome_action(plain(KeyCode::Char(c))),
                None,
                "`{c}` was claimed as chrome while a query was open — it would \
                 never reach the query"
            );
        }
    }

    /// The same property stated as the reader experiences it: type the chrome
    /// keys into a query and every one of them lands in the query.
    ///
    /// Drives the real routing order — `chrome_action` first, then
    /// `handle_key_event`, exactly as `run_session` does — because the defect
    /// was in that order and a test that skipped straight to
    /// `handle_key_event` could not see it.
    #[test]
    fn test_dw_4_1_the_chrome_keys_are_ordinary_characters_inside_a_query() {
        let (_doc, _config, _engine, mut state) = build("a line with T+-z in it\n", 40, 10);

        // The event loop's routing, reproduced: chrome gets first refusal,
        // and only a key it declines goes to the key table.
        fn route(state: &mut AppState, key: KeyEvent) -> Option<ChromeAction> {
            match state.chrome_action(key) {
                Some(action) => Some(action),
                None => {
                    state.handle_key_event(key);
                    None
                }
            }
        }

        route(&mut state, plain(KeyCode::Char('/')));
        let typed = "T+-z";
        for c in typed.chars() {
            let claimed = route(&mut state, plain(KeyCode::Char(c)));
            assert_eq!(claimed, None, "`{c}` was routed to chrome, not the query");
        }

        assert_eq!(
            state.search().query,
            typed,
            "every typed character must reach the query"
        );
        assert_eq!(
            state.search().matches.len(),
            1,
            "and the query must actually be the one the reader typed"
        );
    }

    /// The guard must scope chrome to the mode, not disable it. A fix that
    /// simply stopped routing `+`/`-`/`T` would satisfy every assertion above
    /// and silently delete DW-1.4 and DW-1.5.
    #[test]
    fn test_the_chrome_keys_still_route_to_chrome_in_normal_mode() {
        let (_doc, _config, _engine, state) = build(&numbered_paragraphs(40), 40, 10);
        assert_eq!(state.mode(), Mode::Normal);

        assert_eq!(
            state.chrome_action(plain(KeyCode::Char('+'))),
            Some(ChromeAction::Widen)
        );
        assert_eq!(
            state.chrome_action(plain(KeyCode::Char('-'))),
            Some(ChromeAction::Narrow)
        );
        assert_eq!(
            state.chrome_action(plain(KeyCode::Char('T'))),
            Some(ChromeAction::ToggleTheme)
        );
        // Chrome rather than an ordinary key because the gutter takes cells
        // the document was laid out into: `#` rewraps the page, so it needs
        // the `ctx` only the event loop has.
        assert_eq!(
            state.chrome_action(plain(KeyCode::Char('#'))),
            Some(ChromeAction::ToggleLineNumbers)
        );

        // A chord is not the bare key: Ctrl-T must fall through to the chord
        // table rather than toggling the theme.
        for c in ['+', '-', 'T', '#'] {
            assert_eq!(
                state.chrome_action(ctrl(c)),
                None,
                "Ctrl-{c} must not be treated as chrome"
            );
        }
        // ...and an ordinary letter is not chrome either.
        assert_eq!(state.chrome_action(plain(KeyCode::Char('j'))), None);
    }

    /// `Mode::captures_all_keys` is the question every future mode has to
    /// answer, so pin what the two current answers are — and that the
    /// `Search` answer is what makes the property above hold.
    #[test]
    fn test_only_a_mode_that_owns_the_keyboard_captures_every_key() {
        assert!(!Mode::Normal.captures_all_keys());
        assert!(Mode::Search { origin: 0 }.captures_all_keys());
        // The origin is irrelevant to the answer — it is the *mode* that
        // owns the keyboard, not a particular scroll position.
        assert!(Mode::Search { origin: 99 }.captures_all_keys());
        // Phase 6's answer, for the same reason with a narrower cause: a
        // relayout rewraps the lines `index` was computed from, so `+`/`-`/`T`
        // would move the indicator to a different link.
        assert!(Mode::LinkSelect { index: 0 }.captures_all_keys());
        assert!(Mode::LinkSelect { index: 7 }.captures_all_keys());
        assert!(Mode::Toc { selected: 0 }.captures_all_keys());
        // Phase 3: `rooted` is likewise irrelevant to the answer.
        assert!(
            Mode::Explore {
                selected: 0,
                rooted: false,
            }
            .captures_all_keys()
        );
        assert!(
            Mode::Explore {
                selected: 3,
                rooted: true,
            }
            .captures_all_keys()
        );
    }

    // ---- Search meets the modes and features that landed beside it -------

    /// The TOC is the other mode that owns the keyboard, and it must not lose
    /// the chrome keys either. Phase 3 got this right with its own
    /// `mode() != Mode::Normal` gate in `main.rs`; this asserts the property
    /// survived being restated as `Mode::captures_all_keys`, which is what
    /// replaced that gate.
    #[test]
    fn test_the_toc_overlay_does_not_lose_the_chrome_keys_either() {
        let source: String = (0..8)
            .map(|i| format!("# Heading {i}\n\nbody\n\n"))
            .collect();
        let (_doc, _config, _engine, mut state) = build(&source, 40, 10);
        state.handle_key_event(plain(KeyCode::Char('t')));
        assert!(
            matches!(state.mode(), Mode::Toc { .. }),
            "the TOC must open"
        );

        for c in ' '..='~' {
            assert_eq!(
                state.chrome_action(plain(KeyCode::Char(c))),
                None,
                "`{c}` was claimed as chrome while the TOC was up — it would \
                 relay out or re-theme a document the overlay is not showing"
            );
        }
    }

    /// The property stated once over every mode there is, so a mode added
    /// later cannot satisfy `captures_all_keys` and still leak keys to
    /// chrome. The `match` in `captures_all_keys` is wildcard-free, so a new
    /// variant is a compile error there; this is what makes the answer it
    /// gives actually load-bearing.
    #[test]
    fn test_a_mode_that_captures_the_keyboard_is_refused_by_the_chrome_table() {
        let (_doc, _config, _engine, mut state) = build(&numbered_paragraphs(20), 40, 10);
        // **Derived, not listed.** This array used to be `['+', '-', 'T']`,
        // written out by hand — and Phase 5 added `z`, `R` and `M` to the
        // chrome table without it growing, so the three newest keys went
        // untested here. A wildcard-free `match` makes a new *mode* a compile
        // error; nothing makes a new *key* one. Asking `Mode::Normal` what it
        // accepts closes that: whatever chrome is, every capturing mode must
        // refuse exactly it.
        state.mode = Mode::Normal;
        let chrome_keys: Vec<char> = (' '..='~')
            .filter(|&c| state.chrome_action(plain(KeyCode::Char(c))).is_some())
            .collect();
        assert!(
            chrome_keys.len() >= 6,
            "the chrome table should hold at least +/-/T/z/R/M, found {chrome_keys:?}"
        );
        for mode in [
            Mode::Normal,
            Mode::Toc { selected: 0 },
            Mode::Search { origin: 0 },
            Mode::LinkSelect { index: 0 },
            Mode::Explore {
                selected: 0,
                rooted: false,
            },
            Mode::Explore {
                selected: 0,
                rooted: true,
            },
        ] {
            state.mode = mode;
            let claimed: Vec<char> = chrome_keys
                .iter()
                .copied()
                .filter(|&c| state.chrome_action(plain(KeyCode::Char(c))).is_some())
                .collect();
            if mode.captures_all_keys() {
                assert!(
                    claimed.is_empty(),
                    "{mode:?} captures the keyboard but leaked {claimed:?}"
                );
            } else {
                assert_eq!(
                    claimed, chrome_keys,
                    "{mode:?} does not capture, so every chrome key must still work"
                );
            }
        }
    }

    /// A `--watch` reload under a live query. `SearchState` addresses the
    /// *laid-out* tree, and a reload replaces it wholesale — so every match
    /// must be recomputed against the document that now exists, not carried
    /// across pointing at bytes that moved or vanished.
    #[test]
    fn test_a_reload_while_a_search_is_open_leaves_every_match_addressing_the_new_document() {
        let before: String = (0..12)
            .map(|i| format!("paragraph {i:02} mentions target here\n\n"))
            .collect();
        let (_doc, config, engine, mut state) = build(&before, 40, 10);
        assert_eq!(search_for(&mut state, "target").len(), 12);

        // The reloaded document has fewer matches and different surrounding
        // text, so a stale match set cannot survive by coincidence.
        let after: String = (0..4)
            .map(|i| format!("rewritten section {i:02} still mentions target\n\n"))
            .collect();
        reload(&mut state, &after, &config, &engine);

        assert_eq!(
            state.search().matches.len(),
            4,
            "matches must be recomputed against the reloaded document"
        );
        for found in &state.search().matches {
            assert_eq!(
                matched_text(&state, found),
                "target",
                "match {found:?} addresses the wrong text after a reload"
            );
        }
        assert!(
            state.search().current < state.search().matches.len(),
            "the current index must stay inside the new match set"
        );
    }

    /// The other half of the same staleness: `Mode::Search { origin }` is a
    /// raw line index into the tree `/` was pressed on, and `Esc` returns the
    /// reader to it. A reload replaces that tree, so the index must be
    /// re-seated or `Esc` drops the reader somewhere they never were —
    /// exactly what `reseat_toc` fixes for the TOC's own return scroll.
    #[test]
    fn test_a_reload_while_a_search_is_open_reseats_the_escape_origin() {
        let before: String = (0..60)
            .map(|i| format!("line {i:02} of the original\n\n"))
            .collect();
        let (_doc, config, engine, mut state) = build(&before, 40, 10);
        scroll_to(&mut state, 30);
        state.handle_key_event(plain(KeyCode::Char('/')));
        let Mode::Search { origin } = state.mode() else {
            panic!("the prompt must be open");
        };
        assert_eq!(origin, 30);

        // A much shorter document: line 30 may not exist at all afterwards.
        reload(
            &mut state,
            "only three\n\nshort\n\nparagraphs\n",
            &config,
            &engine,
        );

        let Mode::Search { origin } = state.mode() else {
            panic!("the prompt must still be open across a reload");
        };
        assert!(
            origin <= state.max_scroll(),
            "the escape origin ({origin}) points past the reloaded document's \
             last scrollable line ({})",
            state.max_scroll()
        );
        // And Esc actually lands there rather than clamping from nowhere.
        state.handle_key_event(plain(KeyCode::Esc));
        assert_eq!(state.mode(), Mode::Normal);
        assert_eq!(state.scroll(), origin);
    }

    /// A theme swap relays out at an unchanged width, so nothing reflows and
    /// the origin still means what it meant. Re-seating it there would throw
    /// away a good answer for no reason.
    #[test]
    fn test_a_relayout_that_reflows_nothing_leaves_the_escape_origin_alone() {
        let (doc, config, engine, mut state) = build(&numbered_paragraphs(60), 40, 10);
        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };
        scroll_to(&mut state, 20);
        state.handle_key_event(plain(KeyCode::Char('/')));

        // What `T` does: same width, same tree.
        state.relayout_preserving_anchor(&ctx, config);

        assert_eq!(
            state.mode(),
            Mode::Search { origin: 20 },
            "a no-reflow relayout must not disturb the escape origin"
        );
    }

    /// A search prompt opened over a live `Ctrl-G` message must not spend
    /// that message's frames while it is invisible — the reader should get
    /// the rest of it back when they escape out.
    #[test]
    fn test_the_prompt_does_not_age_the_transient_message_it_is_covering() {
        let (_doc, _config, _engine, mut state) = build("hello\n", 40, 10);
        state.handle_key_event(ctrl('g'));
        state.handle_key_event(plain(KeyCode::Char('/')));
        for _ in 0..STATUS_MESSAGE_TTL_FRAMES * 2 {
            let message = state.status().message.expect("the prompt is showing");
            assert!(message.starts_with('/'), "{message:?}");
        }
        state.handle_key_event(plain(KeyCode::Esc));
        assert!(
            state.status().message.is_some_and(|m| m.contains("bytes")),
            "the file-info message must still have frames left"
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

    /// Lays out `source` into `state` as a `--watch` reload would, at the same
    /// width the tree already has.
    /// The pre-Phase-5 reload helper every earlier test uses, none of which
    /// fold anything — `old_doc: None` is correct for all of them (see
    /// `AppState::reload_document`'s doc). `reload_with_old_doc` below is the
    /// fold-aware sibling.
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
            None,
        );
    }

    /// Like [`reload`], but threading `old_doc` through so a fold recorded
    /// against it can actually be re-keyed (DW-5.2).
    fn reload_with_old_doc(
        state: &mut AppState,
        old_doc: &Document,
        source: &str,
        config: &LayoutConfig,
        engine: &WidthEngine,
    ) {
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
            Some(old_doc),
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
        scroll_to(&mut state, 12);
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
        scroll_to(&mut state, 12);
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
        scroll_to(&mut state, 12);
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
            scroll_to(&mut state, target);
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
        scroll_to(&mut state, 4);
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
                scroll_to(&mut state, copy_line);
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
        scroll_to(&mut state, 2);
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

    // ---- Section folding (Phase 5) ----------------------------------------

    /// `n` top-level headings, each with one distinctive body paragraph —
    /// something to fold, and something folding must make disappear. Distinct
    /// from the shared `heading_source` (Phase 3): every heading here is
    /// level 1, so "next heading of equal or shallower level" never nests
    /// one section inside another, which is what keeps these fold tests'
    /// expectations simple and explicit.
    fn fold_fixture(n: usize) -> String {
        (0..n)
            .map(|i| format!("# Heading {i:02}\n\nbody-{i:02} text.\n\n"))
            .collect()
    }

    fn ctx_for<'a>(
        doc: &'a Document,
        config: &'a LayoutConfig,
        engine: &'a WidthEngine,
    ) -> LayoutContext<'a> {
        LayoutContext {
            doc,
            config,
            engine,
            sizer: &NullSizer,
        }
    }

    /// Mirrors `main.rs::handle_chrome_key`'s `ChromeAction::ToggleFold` arm
    /// exactly: decide (`AppState::toggle_fold`), then relay out. That `z`
    /// actually reaches this path, gated by `Mode::captures_all_keys` like
    /// every other chrome key, is proven separately by driving the compiled
    /// binary in `tests/fold_key_routing.rs`; this level exercises the state
    /// transition `toggle_fold` + a relayout produces.
    fn toggle_fold(state: &mut AppState, ctx: &LayoutContext) {
        state.toggle_fold();
        state.relayout_preserving_anchor(ctx, *ctx.config);
    }

    #[test]
    fn test_dw_5_1_toggle_fold_collapses_and_restores_exactly() {
        let (doc, config, engine, mut state) = build(&fold_fixture(3), 40, 20);
        let ctx = ctx_for(&doc, &config, &engine);
        let before = state.tree().clone();

        toggle_fold(&mut state, &ctx); // cursor starts at line 0: "Heading 00"
        assert_ne!(state.tree(), &before, "folding must change the tree");
        let marker = line_text(&state, 0);
        assert!(
            !marker.contains("body-00") && marker.contains("hidden"),
            "the section's body must be gone and the marker must report a hidden \
             count: {marker:?}"
        );

        toggle_fold(&mut state, &ctx); // re-toggle the same heading
        assert_eq!(
            state.tree(),
            &before,
            "toggling the same heading twice must restore the tree exactly"
        );
    }

    #[test]
    fn test_toggle_fold_on_a_document_with_no_headings_reports_status_and_changes_nothing() {
        let (doc, config, engine, mut state) = build(&non_reflowing_source(5), 40, 10);
        let ctx = ctx_for(&doc, &config, &engine);
        let before = state.tree().clone();

        toggle_fold(&mut state, &ctx);

        assert_eq!(state.tree(), &before, "no headings means nothing to fold");
        assert_eq!(
            state.status().message.as_deref(),
            Some(NO_HEADINGS),
            "the reader must be told there is nothing to fold"
        );
    }

    #[test]
    fn test_dw_5_2_fold_survives_a_width_change() {
        // A viewport shorter than the fixture: `jump_to_block`'s `set_scroll`
        // clamps to `max_scroll`, which is 0 (a no-op) for a document that
        // already fits the whole screen.
        let (doc, config, engine, mut state) = build(&fold_fixture(3), 40, 5);
        let ctx = ctx_for(&doc, &config, &engine);
        let target = state.outline().entries[1].block;
        state.jump_to_block(target);
        toggle_fold(&mut state, &ctx);
        assert!(state.folds().is_folded(target));

        state.widen(&ctx);

        assert!(
            state.folds().is_folded(target),
            "widen must not disturb fold state — it is keyed by node, not line"
        );
        let index = state
            .outline()
            .entries
            .iter()
            .position(|e| e.block == target)
            .expect("the folded heading is still in the outline");
        let line = state.outline().line_of(index).unwrap();
        assert!(
            line_text(&state, line).contains("hidden"),
            "the section must still render as a marker after the width change"
        );
    }

    #[test]
    fn test_dw_5_2_fold_survives_a_watch_reload_by_content_identity() {
        let (doc, config, engine, mut state) = build(&fold_fixture(3), 40, 5);
        let ctx = ctx_for(&doc, &config, &engine);
        let target = state.outline().entries[1].block; // "Heading 01"
        state.jump_to_block(target);
        toggle_fold(&mut state, &ctx);
        assert!(state.folds().is_folded(target));

        // A reload that inserts content *before* the folded heading: ids are
        // assigned pre-order at parse time, so every node from "Heading 01"
        // onward gets a different `NodeId` in the reparsed document. This
        // only passes if the fold is re-keyed by content, not carried across
        // as the same (now wrong) id.
        let edited = format!("# Intro\n\nintro text.\n\n{}", fold_fixture(3));
        reload_with_old_doc(&mut state, &doc, &edited, &config, &engine);

        assert_eq!(
            state.folds().collapsed.len(),
            1,
            "exactly the one fold must survive the reload, re-keyed onto the new document"
        );
        let new_index = state
            .outline()
            .entries
            .iter()
            .position(|e| e.text == "Heading 01")
            .expect("\"Heading 01\" must still be in the reloaded outline");
        let new_id = state.outline().entries[new_index].block;
        assert_ne!(
            new_id, target,
            "test setup: the reload must actually have renumbered nodes, or this proves \
             nothing about content-addressing"
        );
        assert!(
            state.folds().is_folded(new_id),
            "the fold must have followed \"Heading 01\" onto its new NodeId"
        );
        let line = state.outline().line_of(new_index).unwrap();
        assert!(line_text(&state, line).contains("hidden"));

        let untouched = state
            .outline()
            .entries
            .iter()
            .find(|e| e.text == "Heading 00")
            .expect("Heading 00 survives the reload too");
        assert!(
            !state.folds().is_folded(untouched.block),
            "only the heading that was folded before the reload may be folded after it"
        );
    }

    /// Regression for the review's trace 5.2-a: a heading nested inside a
    /// folded ancestor's range has no entry of its own in the *abbreviated*
    /// outline, so re-keying folds against that outline (rather than a
    /// complete, fold-free one) silently dropped it across a reload.
    #[test]
    fn test_dw_5_2_a_nested_fold_survives_a_watch_reload() {
        let source = "# A\n\nx body.\n\n## B\n\ny body.\n\n# C\n\nz body.\n";
        let (doc, config, engine, mut state) = build(source, 40, 5);
        let ctx = ctx_for(&doc, &config, &engine);

        state.collapse_all();
        state.relayout_preserving_anchor(&ctx, config);
        assert_eq!(
            state.folds().collapsed.len(),
            3,
            "test setup: A, B (nested under A), and C must all be folded"
        );

        reload_with_old_doc(&mut state, &doc, source, &config, &engine);

        assert_eq!(
            state.folds().collapsed.len(),
            3,
            "every fold, including B's — nested inside A's collapsed range and absent \
             from the abbreviated outline — must survive a byte-identical reload"
        );

        // Unfold A: B must reappear already folded, not wide open underneath it.
        let a = state
            .outline()
            .entries
            .iter()
            .find(|e| e.text == "A")
            .unwrap()
            .block;
        state.jump_to_block(a);
        toggle_fold(&mut state, &ctx);
        let all: String = (0..state.tree().line_count())
            .map(|i| line_text(&state, i))
            .collect();
        assert!(
            all.contains('B') && all.contains("hidden"),
            "B must come back already folded: {all:?}"
        );
        assert!(
            !all.contains("y body"),
            "B's own body must still be hidden: {all:?}"
        );
    }

    /// Regression for the review's trace 5.2-b: with two `## Notes` headings
    /// sharing a title (one nested under a folded `A`, one under an open
    /// `B`), re-keying the fold on the *visible* `Notes` against an
    /// abbreviated old outline computed its occurrence among only the
    /// visible copies (0), then applied that index to the new, fully
    /// expanded outline — landing on `A`'s `Notes` instead of `B`'s.
    #[test]
    fn test_dw_5_2_a_fold_on_a_duplicate_titled_heading_reseats_onto_the_same_heading() {
        let source = "# A\n\naaa.\n\n## Notes\n\nfirst notes.\n\n\
                       # B\n\nbbb.\n\n## Notes\n\nsecond notes.\n";
        // Height 1: `jump_to_block` moves the reader via `set_scroll`, which
        // clamps to `max_scroll` — a viewport any taller relative to this
        // small fixture silently caps the jump before it reaches the second
        // "Notes" heading, folding whatever line the clamp landed on instead.
        let (doc, config, engine, mut state) = build(source, 40, 1);
        let ctx = ctx_for(&doc, &config, &engine);

        let a = state.outline().entries[0].block;
        state.jump_to_block(a);
        toggle_fold(&mut state, &ctx); // folds A; A's own "Notes" vanishes from the outline

        let second_notes = state
            .outline()
            .entries
            .iter()
            .find(|e| e.text == "Notes")
            .expect("test setup: B's Notes is the only one left visible")
            .block;
        state.jump_to_block(second_notes);
        toggle_fold(&mut state, &ctx); // folds the *visible* (B's) Notes

        assert_eq!(
            state.folds().collapsed.len(),
            2,
            "test setup: two folds active"
        );

        reload_with_old_doc(&mut state, &doc, source, &config, &engine);

        assert_eq!(
            state.folds().collapsed.len(),
            2,
            "both folds must survive the reload"
        );
        assert!(
            state.folds().is_folded(a),
            "\"A\" must still be folded after the reload"
        );

        let all: Vec<String> = (0..state.tree().line_count())
            .map(|i| line_text(&state, i))
            .collect();
        // A heading line now carries a depth marker (`▌` per level) ahead of
        // the text, and an H1 is padded to the full measure so its wash
        // background reads as a band — so `# B` renders as `▌ B` plus trailing
        // blanks.
        let b_line = all
            .iter()
            .position(|t| t.trim_start_matches(['\u{258c}', ' ']).trim_end() == "B")
            .expect("B must be open and its own heading line visible");
        let notes_after_b = all[b_line..]
            .iter()
            .find(|t| t.contains("Notes"))
            .expect("B's own Notes heading must still be on screen");
        assert!(
            notes_after_b.contains("hidden"),
            "B's own Notes must still be folded — not re-seated onto A's — after \
             the reload: {all:?}"
        );
        assert!(
            !all.iter().any(|t| t.contains("second notes")),
            "B's Notes body must stay hidden: {all:?}"
        );
    }

    #[test]
    fn test_dw_5_3_folding_removes_the_folded_headings_reserved_lines_unfolding_restores_them() {
        struct AlwaysSizes;
        impl IntrinsicSizer for AlwaysSizes {
            fn size(&self, _node: NodeId, _doc: &Document) -> Option<layout::CellSize> {
                Some(layout::CellSize { cols: 10, rows: 2 })
            }
        }

        fn has_reserved(state: &AppState) -> bool {
            (0..state.tree().line_count())
                .any(|i| matches!(state.tree().lines(i..i + 1).next(), Some(Line::Reserved(_))))
        }

        let source = "# One\n\n![alt](pic.png)\n\n# Two\n\nBeta.\n";
        let doc = Document::parse(source);
        let config = LayoutConfig::default();
        let engine = WidthEngine::new(WidthConfig::default());
        let tree = layout(&doc, 40, &config, &engine, &AlwaysSizes);
        assert!(
            (0..tree.line_count())
                .any(|i| matches!(tree.lines(i..i + 1).next(), Some(Line::Reserved(_)))),
            "test setup: the fixture must reserve a media box before it is folded"
        );
        let mut state = AppState::new(
            tree,
            Size {
                width: 40,
                height: 20,
            },
            FileInfo::default(),
        );
        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &AlwaysSizes,
        };

        let target = state.outline().entries[0].block;
        state.jump_to_block(target);
        toggle_fold(&mut state, &ctx);
        assert!(
            !has_reserved(&state),
            "folding the section holding the image must drop its reserved lines — the \
             painter and media sink place only what a frame's tree actually contains \
             (DW-5.3; see `media/sink.rs`'s module doc)"
        );

        toggle_fold(&mut state, &ctx);
        assert!(
            has_reserved(&state),
            "unfolding must bring the image's reserved lines back"
        );
    }

    #[test]
    fn test_dw_5_4_collapse_all_and_expand_all() {
        let (doc, config, engine, mut state) = build(&fold_fixture(4), 40, 30);
        let ctx = ctx_for(&doc, &config, &engine);
        let before = state.tree().clone();

        state.collapse_all();
        state.relayout_preserving_anchor(&ctx, config);

        let marker_lines = (0..state.tree().line_count())
            .filter(|&i| line_text(&state, i).contains('\u{25b8}'))
            .count();
        assert_eq!(
            marker_lines, 4,
            "every heading here is top-level, so collapse-all must leave exactly one \
             marker line per heading"
        );
        let all_text: String = (0..state.tree().line_count())
            .map(|i| line_text(&state, i))
            .collect();
        for i in 0..4 {
            assert!(
                !all_text.contains(&format!("body-{i:02}")),
                "every body paragraph must be hidden once every heading is collapsed"
            );
        }

        state.expand_all();
        state.relayout_preserving_anchor(&ctx, config);
        assert_eq!(
            state.tree(),
            &before,
            "expand-all must restore the full document exactly"
        );
    }

    #[test]
    fn test_dw_5_5_folding_while_scrolled_inside_the_range_snaps_to_the_marker() {
        let (doc, config, engine, mut state) = build(&fold_fixture(3), 40, 5);
        let ctx = ctx_for(&doc, &config, &engine);
        let target = state.outline().entries[1].block; // "Heading 01"
        let heading_line = state.outline().line_of(1).unwrap();
        // Past the heading's own line, inside its body — the case the
        // ordinary block anchor cannot resolve once the section collapses,
        // because that line's own block stops emitting a line at all.
        state.jump_to_block(target);
        state.handle_key_event(plain(KeyCode::Down));
        assert!(
            state.cursor() > heading_line,
            "test setup: the reader must be inside the section, past its heading line"
        );

        toggle_fold(&mut state, &ctx);

        let new_index = state
            .outline()
            .entries
            .iter()
            .position(|e| e.block == target)
            .unwrap();
        let marker_line = state.outline().line_of(new_index).unwrap();
        assert_eq!(
            state.scroll(),
            marker_line,
            "folding a range the reader was scrolled inside must leave the viewport \
             exactly at the fold marker"
        );
        assert!(
            state.scroll() <= state.max_scroll(),
            "the snap must never land past the end"
        );
    }

    #[test]
    fn test_dw_5_5_folding_while_the_cursor_sits_on_the_headings_own_line_also_lands_on_the_marker()
    {
        let (doc, config, engine, mut state) = build(&fold_fixture(3), 40, 5);
        let ctx = ctx_for(&doc, &config, &engine);
        let target = state.outline().entries[1].block;
        state.jump_to_block(target); // cursor on the heading's own first line

        toggle_fold(&mut state, &ctx);

        let new_index = state
            .outline()
            .entries
            .iter()
            .position(|e| e.block == target)
            .unwrap();
        let marker_line = state.outline().line_of(new_index).unwrap();
        assert_eq!(state.scroll(), marker_line);
    }

    #[test]
    fn test_dw_5_6_no_line_exceeds_width_after_a_fold_driven_relayout() {
        let long = "A heading title long enough on its own that folding it still cannot overflow";
        let source = format!(
            "# {long}\n\nSome body text that is also long enough to wrap on its own at a \
             narrow width.\n\n# Two\n\nBeta.\n"
        );
        for width in [20u16, 24, 40] {
            let (doc, config, engine, mut state) = build(&source, width, 20);
            let ctx = ctx_for(&doc, &config, &engine);
            let target = state.outline().entries[0].block;
            state.jump_to_block(target);
            toggle_fold(&mut state, &ctx);

            for i in 0..state.tree().line_count() {
                let text = line_text(&state, i);
                let measured = engine.display_width(&text);
                assert!(
                    measured <= state.content_width() as usize,
                    "at width {width}: line {text:?} measured {measured} cells"
                );
            }
        }
    }

    #[test]
    fn test_folding_a_range_containing_a_search_match_drops_it_and_unfolding_restores_it() {
        let source = "# One\n\nneedle here.\n\n# Two\n\nneedle there too.\n";
        let (doc, config, engine, mut state) = build(source, 40, 20);
        let ctx = ctx_for(&doc, &config, &engine);
        let matches = search_for(&mut state, "needle");
        assert_eq!(matches.len(), 2, "test setup: two matches expected");

        let one = state.outline().entries[0].block;
        state.jump_to_block(one);
        toggle_fold(&mut state, &ctx);

        assert_eq!(
            state.search().matches.len(),
            1,
            "folding a range containing a match must drop it from the active set — the \
             phase's chosen answer to \"n must expand it or skip it\" (skip)"
        );
        let remaining = &state.search().matches[0];
        assert!(
            line_text(&state, remaining.line).contains("needle"),
            "the surviving match must still address real, visible text"
        );

        toggle_fold(&mut state, &ctx); // unfold
        assert_eq!(
            state.search().matches.len(),
            2,
            "unfolding must restore the match that was inside the folded range"
        );
    }

    /// The listed edge case's status-row half, reproducing the review's exact
    /// trace: a single match, folded away, then `n`. Before this fix the
    /// status row answered `"no matches: needle"` — false, since the
    /// document does contain it, just not currently visible.
    #[test]
    fn test_n_after_folding_the_only_match_reports_it_as_hidden_not_absent() {
        let source = "# One\n\nneedle here.\n\n# Two\n\nplain text.\n";
        let (doc, config, engine, mut state) = build(source, 40, 20);
        let ctx = ctx_for(&doc, &config, &engine);
        let matches = search_for(&mut state, "needle");
        assert_eq!(matches.len(), 1, "test setup: exactly one match");

        let one = state.outline().entries[0].block;
        state.jump_to_block(one);
        toggle_fold(&mut state, &ctx);
        assert_eq!(
            state.search().matches.len(),
            0,
            "test setup: the fold hid the only match"
        );
        assert_eq!(
            state.search().hidden_by_folds,
            1,
            "the count of matches hidden by folds must be tracked, not just the visible \
             drop to zero"
        );

        state.handle_key_event(plain(KeyCode::Char('n')));
        let message = state.status().message;
        assert_eq!(
            message.as_deref(),
            Some("no matches: needle (1 hidden by a fold — R to expand)"),
            "the reader must be told the match is hidden, not that the document has \
             none — got {message:?}"
        );
    }

    /// The other half of the same claim: a query that genuinely matches
    /// nothing, anywhere, must not start claiming folds are hiding something
    /// that was never there.
    #[test]
    fn test_n_with_genuinely_no_matches_does_not_claim_any_are_hidden_by_folds() {
        let source = "# One\n\nplain text.\n\n# Two\n\nmore text.\n";
        let (doc, config, engine, mut state) = build(source, 40, 20);
        let ctx = ctx_for(&doc, &config, &engine);
        search_for(&mut state, "needle");

        let one = state.outline().entries[0].block;
        state.jump_to_block(one);
        toggle_fold(&mut state, &ctx);

        state.handle_key_event(plain(KeyCode::Char('n')));
        let message = state.status().message;
        assert_eq!(
            message.as_deref(),
            Some("no matches: needle"),
            "a query with genuinely no matches anywhere must not claim any are hidden \
             by folds — got {message:?}"
        );
    }

    /// Unfolding must not just restore the match — it must clear the hidden
    /// count too, or a stale "1 hidden" could linger after there is nothing
    /// left hidden.
    #[test]
    fn test_unfolding_the_only_match_clears_the_hidden_by_folds_count() {
        let source = "# One\n\nneedle here.\n\n# Two\n\nplain text.\n";
        let (doc, config, engine, mut state) = build(source, 40, 20);
        let ctx = ctx_for(&doc, &config, &engine);
        search_for(&mut state, "needle");
        let one = state.outline().entries[0].block;
        state.jump_to_block(one);
        toggle_fold(&mut state, &ctx);
        assert_eq!(state.search().hidden_by_folds, 1);

        toggle_fold(&mut state, &ctx); // unfold
        assert_eq!(state.search().matches.len(), 1);
        assert_eq!(
            state.search().hidden_by_folds,
            0,
            "once the match is visible again, nothing should still be reported hidden"
        );
    }

    /// Mid-severity note: `collapse_all` must union into the collapsed set,
    /// not replace it — replacing reads the *current*, possibly-abbreviated
    /// outline (a heading nested inside an already-folded range has no entry
    /// in it), so a second `M` silently dropped every such fold. Same root
    /// cause as DW-5.2's reload defect, caught here at the collapse-all path
    /// instead.
    #[test]
    fn test_collapse_all_pressed_twice_does_not_drop_a_fold_nested_inside_the_first_pass() {
        let source = "# A\n\nx body.\n\n## B\n\ny body.\n\n# C\n\nz body.\n";
        let (doc, config, engine, mut state) = build(source, 40, 5);
        let ctx = ctx_for(&doc, &config, &engine);

        state.collapse_all();
        state.relayout_preserving_anchor(&ctx, config);
        assert_eq!(
            state.folds().collapsed.len(),
            3,
            "test setup: A, B, and C folded"
        );

        // The current outline is now abbreviated (B has no entry, nested
        // inside A's collapsed range) — exactly the condition that used to
        // make a second collapse-all lose it.
        state.collapse_all();
        state.relayout_preserving_anchor(&ctx, config);
        assert_eq!(
            state.folds().collapsed.len(),
            3,
            "a second collapse-all must not drop a fold the current outline cannot see"
        );

        let a = state
            .outline()
            .entries
            .iter()
            .find(|e| e.text == "A")
            .unwrap()
            .block;
        state.jump_to_block(a);
        toggle_fold(&mut state, &ctx); // unfold A
        let all: String = (0..state.tree().line_count())
            .map(|i| line_text(&state, i))
            .collect();
        assert!(
            all.contains('B') && all.contains("hidden") && !all.contains("y body"),
            "B must still be folded underneath the now-open A: {all:?}"
        );
    }

    /// Mid-severity note: `z` above the first heading of a document that
    /// *has* headings must not claim it has none — `NO_HEADINGS` is false
    /// there, and the reader has no way to tell "no headings exist" apart
    /// from "none of them are above you yet".
    #[test]
    fn test_z_above_the_first_heading_reports_no_section_here_not_no_headings() {
        let source = format!("preamble text.\n\n{}", fold_fixture(2));
        let (doc, config, engine, mut state) = build(&source, 40, 20);
        let ctx = ctx_for(&doc, &config, &engine);
        assert_eq!(state.scroll(), 0);
        assert!(
            state.outline().index_at_or_before(0).is_none(),
            "test setup: the cursor must start above every heading"
        );

        toggle_fold(&mut state, &ctx);

        assert_eq!(
            state.status().message.as_deref(),
            Some(NO_SECTION_HERE),
            "a document that has headings must not claim it has none"
        );
    }
    // ------------------------------------------------------------- Phase 6

    /// A document whose viewport holds three links with distinct destinations,
    /// plus prose around them so a hit-test has non-link cells to miss on.
    fn linked_source() -> String {
        String::from(
            "Intro prose with [alpha](alpha.md) inside it.\n\n\
             A second paragraph pointing at [beta](https://example.com/beta) here.\n\n\
             And a third naming [gamma](notes/gamma.txt) at the end.\n\n\
             Trailing prose with no destination at all.\n",
        )
    }

    fn targets(state: &AppState) -> Vec<String> {
        state
            .visible_links()
            .into_iter()
            .map(|link| link.target)
            .collect()
    }

    fn selected_index(state: &AppState) -> Option<usize> {
        match state.mode() {
            Mode::LinkSelect { index } => Some(index),
            Mode::Normal | Mode::Toc { .. } | Mode::Search { .. } | Mode::Explore { .. } => None,
        }
    }

    #[test]
    fn test_dw_6_1_tab_and_shift_tab_cycle_the_links_in_the_viewport_and_wrap() {
        let (_doc, _config, _engine, mut state) = build(&linked_source(), 80, 20);
        assert_eq!(
            targets(&state),
            vec!["alpha.md", "https://example.com/beta", "notes/gamma.txt"],
            "the fixture must offer three distinct links, in document order"
        );

        assert!(!state.handle_key_event(plain(KeyCode::Tab)));
        assert_eq!(selected_index(&state), Some(0), "Tab enters on the first");
        state.handle_key_event(plain(KeyCode::Tab));
        assert_eq!(selected_index(&state), Some(1));
        state.handle_key_event(plain(KeyCode::Tab));
        assert_eq!(selected_index(&state), Some(2));
        state.handle_key_event(plain(KeyCode::Tab));
        assert_eq!(selected_index(&state), Some(0), "and wraps at the end");

        state.handle_key_event(plain(KeyCode::BackTab));
        assert_eq!(selected_index(&state), Some(2), "Shift-Tab wraps backward");
        state.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT));
        assert_eq!(
            selected_index(&state),
            Some(1),
            "a terminal that reports Shift-Tab as Tab+SHIFT must cycle backward too"
        );
    }

    #[test]
    fn test_dw_6_1_the_status_row_names_the_destination_of_the_selected_link() {
        let (_doc, _config, _engine, mut state) = build(&linked_source(), 80, 20);
        state.handle_key_event(plain(KeyCode::Tab));
        let status = state.status();
        assert_eq!(
            status.message.as_deref(),
            Some("link 1/3: alpha.md"),
            "the reader must be told where Enter would take them"
        );
        state.handle_key_event(plain(KeyCode::Tab));
        assert_eq!(
            state.status().message.as_deref(),
            Some("link 2/3: https://example.com/beta")
        );
    }

    #[test]
    fn test_dw_6_1_only_links_inside_the_viewport_are_offered() {
        // Two rows of viewport: the first link is on screen and the later ones
        // are not, so `Tab` must have exactly one thing to select.
        let (_doc, _config, _engine, mut state) = build(&linked_source(), 80, 2);
        assert_eq!(targets(&state), vec!["alpha.md"]);
        state.handle_key_event(plain(KeyCode::Tab));
        state.handle_key_event(plain(KeyCode::Tab));
        assert_eq!(
            selected_index(&state),
            Some(0),
            "cycling one link stays on it rather than indexing off the end"
        );

        // Scroll past it and the offer changes with the screen.
        state.set_scroll(state.max_scroll());
        assert!(
            !targets(&state).contains(&"alpha.md".to_string()),
            "a link scrolled off the top must not stay selectable: {:?}",
            targets(&state)
        );
    }

    #[test]
    fn test_dw_6_1_tab_with_no_links_in_view_reports_instead_of_entering_the_mode() {
        let (_doc, _config, _engine, mut state) = build(&non_reflowing_source(20), 40, 10);
        assert!(!state.handle_key_event(plain(KeyCode::Tab)));
        assert_eq!(state.mode(), Mode::Normal, "no mode with nothing in it");
        assert_eq!(state.status().message.as_deref(), Some("no links in view"));
    }

    #[test]
    fn test_dw_6_1_enter_activates_the_selected_link_and_leaves_the_mode() {
        let (_doc, _config, _engine, mut state) = build(&linked_source(), 80, 20);
        state.handle_key_event(plain(KeyCode::Tab));
        state.handle_key_event(plain(KeyCode::Tab));
        assert!(!state.handle_key_event(plain(KeyCode::Enter)));
        assert_eq!(
            state.take_action(),
            Some(PendingAction::OpenLink(
                "https://example.com/beta".to_string()
            )),
            "Enter must queue the *selected* destination, not the first one"
        );
        assert_eq!(
            state.mode(),
            Mode::Normal,
            "activation returns the reader to the document"
        );
        assert_eq!(
            state.take_action(),
            None,
            "and the action drains exactly once"
        );
    }

    #[test]
    fn test_dw_6_1_esc_leaves_link_selection_without_activating_anything() {
        let (_doc, _config, _engine, mut state) = build(&linked_source(), 80, 20);
        state.handle_key_event(plain(KeyCode::Tab));
        state.handle_key_event(plain(KeyCode::Esc));
        assert_eq!(state.mode(), Mode::Normal);
        assert_eq!(state.take_action(), None);
    }

    #[test]
    fn test_q_and_ctrl_c_still_quit_from_inside_link_selection() {
        for key in [plain(KeyCode::Char('q')), ctrl('c')] {
            let (_doc, _config, _engine, mut state) = build(&linked_source(), 80, 20);
            state.handle_key_event(plain(KeyCode::Tab));
            assert!(
                state.handle_key_event(key),
                "a mode that swallows the quit keys strands the reader"
            );
        }
    }

    #[test]
    fn test_a_link_whose_text_spans_several_runs_counts_once() {
        // The emphasis splits the link into two runs sharing one destination;
        // a reader sees one link and `Tab` must agree with them.
        let (_doc, _config, _engine, state) =
            build("Prose [**bold** and plain](one.md) more prose.\n", 80, 10);
        let links = state.visible_links();
        assert_eq!(links.len(), 1, "one link, several runs: {links:?}");
        assert_eq!(links[0].target, "one.md");
        assert!(
            links[0].spans.len() == 1 && links[0].spans[0].first_item < links[0].spans[0].last_item,
            "the span must cover more than one run: {:?}",
            links[0].spans
        );
        assert!(
            links[0].text.contains("bold") && links[0].text.contains("plain"),
            "the merged text must be the whole link: {:?}",
            links[0].text
        );
    }

    #[test]
    fn test_a_link_wrapped_across_two_lines_counts_once_and_carries_both_spans() {
        // 24 cells forces the long link text to wrap mid-link.
        let (_doc, _config, _engine, state) = build(
            "[a very long link label that must wrap somewhere](wrapped.md)\n",
            24,
            10,
        );
        let links = state.visible_links();
        assert_eq!(
            links.len(),
            1,
            "a wrapped link is still one link: {links:?}"
        );
        assert!(
            links[0].spans.len() >= 2,
            "it must carry a span per line it paints on: {:?}",
            links[0].spans
        );
        let lines: Vec<usize> = links[0].spans.iter().map(|span| span.line).collect();
        assert!(
            lines.windows(2).all(|pair| pair[0] + 1 == pair[1]),
            "the spans must be consecutive lines: {lines:?}"
        );
    }

    #[test]
    fn test_two_different_links_on_one_line_stay_two_links() {
        let (_doc, _config, _engine, state) =
            build("See [one](one.md) and [two](two.md).\n", 80, 10);
        assert_eq!(targets(&state), vec!["one.md", "two.md"]);
    }

    // ---------------------------------------------------------------- DW-6.6

    fn wheel(kind: MouseEventKind) -> MouseEvent {
        MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn click_at(column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn test_dw_6_6_a_wheel_scroll_moves_the_viewport_and_clamps_at_both_ends() {
        let engine = WidthEngine::new(WidthConfig::default());
        let (_doc, _config, _engine, mut state) = build(&non_reflowing_source(200), 40, 10);

        assert!(state.handle_mouse_event(wheel(MouseEventKind::ScrollDown), &engine));
        assert_eq!(state.scroll(), 3, "one notch is three lines");
        assert!(state.handle_mouse_event(wheel(MouseEventKind::ScrollUp), &engine));
        assert_eq!(state.scroll(), 0);
        // Up at the top clamps rather than underflowing.
        state.handle_mouse_event(wheel(MouseEventKind::ScrollUp), &engine);
        assert_eq!(state.scroll(), 0);

        for _ in 0..500 {
            state.handle_mouse_event(wheel(MouseEventKind::ScrollDown), &engine);
        }
        assert_eq!(state.scroll(), state.max_scroll(), "and clamps at the tail");
    }

    #[test]
    fn test_dw_6_6_a_click_on_a_link_cell_activates_that_link() {
        let engine = WidthEngine::new(WidthConfig::default());
        let (_doc, _config, _engine, mut state) = build(&linked_source(), 80, 20);

        // Find where the first link actually paints, through the same
        // measurement the painter uses, rather than guessing a column.
        let link = state.visible_links().into_iter().next().expect("a link");
        let span = link.spans[0];
        let Some(Line::Items(items)) = state.tree().lines(span.line..span.line + 1).next() else {
            panic!("the link's line must be a text line");
        };
        let columns = item_columns(items, &engine, 80);
        let (start, end) = columns[span.first_item];
        assert!(start < end, "the link must occupy real cells");

        assert!(state.handle_mouse_event(click_at(start, span.line as u16), &engine));
        assert_eq!(
            state.take_action(),
            Some(PendingAction::OpenLink("alpha.md".to_string()))
        );

        // ...and one cell short of it is not the link.
        if start > 0 {
            let (_d, _c, _e, mut fresh) = build(&linked_source(), 80, 20);
            assert!(!fresh.handle_mouse_event(click_at(start - 1, span.line as u16), &engine));
            assert_eq!(fresh.take_action(), None);
        }
    }

    #[test]
    fn test_dw_6_6_a_click_on_a_cell_with_no_link_changes_nothing() {
        let engine = WidthEngine::new(WidthConfig::default());
        let (_doc, _config, _engine, mut state) = build(&linked_source(), 80, 20);
        let before = state.scroll();
        // Column 0 of row 0 is the first word of the prose, and the far right
        // of the row is past every glyph on it.
        for column in [0u16, 79] {
            assert!(
                !state.handle_mouse_event(click_at(column, 0), &engine),
                "a click at column {column} must not earn a frame"
            );
            assert_eq!(state.take_action(), None);
        }
        assert_eq!(state.scroll(), before);
        assert_eq!(state.mode(), Mode::Normal);
    }

    #[test]
    fn test_dw_6_6_a_click_past_the_end_of_the_document_changes_nothing() {
        let engine = WidthEngine::new(WidthConfig::default());
        let (_doc, _config, _engine, mut state) = build("# tiny\n", 40, 20);
        assert!(!state.handle_mouse_event(click_at(0, 19), &engine));
        // The status row is one below the content viewport and is not clickable.
        assert!(!state.handle_mouse_event(click_at(0, 20), &engine));
        assert_eq!(state.take_action(), None);
    }

    #[test]
    fn test_dw_6_6_m_toggles_mouse_capture_and_reports_which_way() {
        let (_doc, _config, _engine, mut state) = build("# doc\n", 40, 10);
        assert!(state.mouse_capture(), "capture starts on");

        assert!(!state.handle_key_event(plain(KeyCode::Char('m'))));
        assert!(!state.mouse_capture());
        assert_eq!(
            state.take_action(),
            Some(PendingAction::SetMouseCapture(false))
        );
        assert!(
            state.status().message.is_some_and(|m| m.contains("off")),
            "the effect is invisible until the reader tries to drag, so it must be said"
        );

        state.handle_key_event(plain(KeyCode::Char('m')));
        assert!(state.mouse_capture());
        assert_eq!(
            state.take_action(),
            Some(PendingAction::SetMouseCapture(true))
        );
    }

    #[test]
    fn test_a_wheel_scroll_leaves_link_selection_rather_than_keeping_a_stale_index() {
        let engine = WidthEngine::new(WidthConfig::default());
        let (_doc, _config, _engine, mut state) = build(&linked_source(), 80, 4);
        state.handle_key_event(plain(KeyCode::Tab));
        assert!(matches!(state.mode(), Mode::LinkSelect { .. }));
        state.handle_mouse_event(wheel(MouseEventKind::ScrollDown), &engine);
        assert_eq!(
            state.mode(),
            Mode::Normal,
            "scrolling changes which links are visible, so the index must not survive"
        );
    }

    #[test]
    fn test_a_gesture_with_no_binding_neither_scrolls_nor_activates() {
        let engine = WidthEngine::new(WidthConfig::default());
        let (_doc, _config, _engine, mut state) = build(&linked_source(), 80, 20);
        for kind in [
            MouseEventKind::Moved,
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::Down(MouseButton::Right),
            MouseEventKind::Down(MouseButton::Middle),
            MouseEventKind::ScrollLeft,
            MouseEventKind::ScrollRight,
        ] {
            assert!(
                !state.handle_mouse_event(wheel(kind), &engine),
                "{kind:?} must be inert"
            );
            assert_eq!(state.take_action(), None);
            assert_eq!(state.scroll(), 0);
        }
    }

    // ---------------------------------------------------------------- DW-6.7

    #[test]
    fn test_dw_6_7_y_yanks_the_code_block_the_reader_is_looking_at() {
        let source = "# Title\n\n```sh\ncurl https://example.com | sh\necho done\n```\n";
        let (doc, _config, _engine, mut state) = build(source, 60, 20);

        assert!(!state.handle_key_event(plain(KeyCode::Char('y'))));
        assert_eq!(state.take_action(), Some(PendingAction::CopyCodeBlock));
        assert_eq!(
            state.code_block_in_view(&doc).as_deref(),
            Some("curl https://example.com | sh\necho done\n"),
            "the AST literal, not the painted (and possibly clipped) lines"
        );
    }

    #[test]
    fn test_dw_6_7_the_yanked_text_is_the_source_not_the_clipped_paint() {
        // A code line far wider than the viewport: layout clips it and marks
        // the clip, so copying the screen would put a truncated command on the
        // clipboard that looks complete.
        let long = "echo ".to_string() + &"x".repeat(300);
        let source = format!("```sh\n{long}\n```\n");
        let (doc, _config, _engine, mut state) = build(&source, 40, 20);

        let yanked = state.code_block_in_view(&doc).expect("a code block");
        assert_eq!(yanked, format!("{long}\n"));
        assert!(
            !yanked.contains('\u{2026}'),
            "the clip indicator must never reach the clipboard: {yanked:?}"
        );
    }

    #[test]
    fn test_dw_6_7_y_finds_a_fence_nested_in_a_list_item() {
        // `line_blocks` only ever names the *top-level* block, so a fence
        // inside a list needs the subtree walk.
        let source = "- step one\n\n  ```sh\n  make test\n  ```\n";
        let (doc, _config, _engine, mut state) = build(source, 60, 20);
        assert_eq!(
            state.code_block_in_view(&doc).as_deref(),
            Some("make test\n")
        );
    }

    #[test]
    fn test_dw_6_7_y_with_no_code_block_in_view_reports_instead() {
        let (doc, _config, _engine, mut state) = build("# Just prose\n\nand more.\n", 60, 20);
        assert_eq!(state.code_block_in_view(&doc), None);
        assert_eq!(
            state.status().message.as_deref(),
            Some("no code block in view")
        );
    }

    #[test]
    fn test_dw_6_7_a_code_block_scrolled_out_of_view_is_not_the_one_yanked() {
        let source = format!(
            "```sh\nfirst\n```\n\n{}\n```sh\nsecond\n```\n",
            non_reflowing_source(30)
        );
        let (doc, _config, _engine, mut state) = build(&source, 60, 6);
        assert_eq!(state.code_block_in_view(&doc).as_deref(), Some("first\n"));
        state.set_scroll(state.max_scroll());
        assert_eq!(
            state.code_block_in_view(&doc).as_deref(),
            Some("second\n"),
            "\"in view\" must follow the viewport, not the document"
        );
    }

    // ------------------------------------------------- reload / resize seams

    #[test]
    fn test_a_reload_that_removes_the_selected_link_dismisses_link_selection() {
        let (_doc, config, engine, mut state) = build(&linked_source(), 80, 20);
        state.handle_key_event(plain(KeyCode::Tab));
        state.handle_key_event(plain(KeyCode::Tab));
        assert_eq!(selected_index(&state), Some(1));

        reload(
            &mut state,
            "# no links at all\n\njust prose\n",
            &config,
            &engine,
        );
        assert_eq!(
            state.mode(),
            Mode::Normal,
            "a selection addressing a document that no longer exists must be dropped"
        );
        assert_eq!(state.status().message.as_deref(), Some("no links in view"));
    }

    #[test]
    fn test_a_reload_to_fewer_links_clamps_the_selection_into_range() {
        let (_doc, config, engine, mut state) = build(&linked_source(), 80, 20);
        for _ in 0..3 {
            state.handle_key_event(plain(KeyCode::Tab));
        }
        assert_eq!(selected_index(&state), Some(2));

        reload(&mut state, "Only [one](one.md) left.\n", &config, &engine);
        assert_eq!(
            selected_index(&state),
            Some(0),
            "the index must be clamped, not left pointing past the end"
        );
        assert_eq!(
            state.selected_link().map(|link| link.target),
            Some("one.md".to_string()),
            "and it must resolve against the new document"
        );
    }

    #[test]
    fn test_a_resize_reseats_an_open_link_selection() {
        let (doc, config, engine, mut state) = build(&linked_source(), 80, 20);
        for _ in 0..3 {
            state.handle_key_event(plain(KeyCode::Tab));
        }
        assert_eq!(selected_index(&state), Some(2));

        let ctx = LayoutContext {
            doc: &doc,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };
        // Narrow and short: reflow moves every wrap point and the viewport now
        // holds fewer links than the index names.
        state.apply_resize_burst(
            &ctx,
            &[Size {
                width: 30,
                height: 3,
            }],
        );
        match state.mode() {
            Mode::LinkSelect { index } => assert!(
                index < state.visible_links().len(),
                "the reseated index must address a link that exists"
            ),
            Mode::Normal => assert!(
                state.visible_links().is_empty(),
                "dropping to Normal is only right when nothing is selectable"
            ),
            Mode::Toc { .. } | Mode::Search { .. } | Mode::Explore { .. } => {
                panic!("a resize cannot open an overlay, a query prompt, or the explorer")
            }
        }
    }

    #[test]
    fn test_open_document_lands_at_the_requested_scroll_and_clears_the_mode() {
        let (_doc, config, engine, mut state) = build(&linked_source(), 80, 20);
        state.handle_key_event(plain(KeyCode::Tab));

        let next = Document::parse(&non_reflowing_source(100));
        let ctx = LayoutContext {
            doc: &next,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };
        state.open_document(
            &ctx,
            FileInfo {
                name: "next.md".to_string(),
                byte_size: 1,
                line_count: 1,
            },
            17,
        );
        assert_eq!(state.scroll(), 17, "DW-6.2: the caller says where to land");
        assert_eq!(state.mode(), Mode::Normal);
        assert_eq!(state.status().name, "next.md");
    }

    #[test]
    fn test_open_document_clamps_a_scroll_past_the_new_documents_end() {
        let (_doc, config, engine, mut state) = build(&non_reflowing_source(200), 80, 20);
        let next = Document::parse("# tiny\n");
        let ctx = LayoutContext {
            doc: &next,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };
        state.open_document(&ctx, FileInfo::default(), 5_000);
        assert_eq!(state.scroll(), state.max_scroll());
    }

    #[test]
    fn test_backspace_and_y_queue_exactly_one_action_each() {
        let (_doc, _config, _engine, mut state) = build("# doc\n", 40, 10);
        assert!(!state.handle_key_event(plain(KeyCode::Backspace)));
        assert_eq!(state.take_action(), Some(PendingAction::Back));
        assert_eq!(state.take_action(), None);
    }

    /// The chord table's documented fallthrough must not turn `Ctrl-y` into a
    /// clipboard write or `Ctrl-m` into a capture toggle: both new bindings sit
    /// inside the `!CONTROL` guard, exactly like `t`.
    #[test]
    fn test_the_new_bindings_are_not_reachable_through_a_control_chord() {
        let (_doc, _config, _engine, mut state) = build("# doc\n", 40, 10);
        let capture_before = state.mouse_capture();
        for key in [ctrl('y'), ctrl('m')] {
            state.handle_key_event(key);
            assert_eq!(state.take_action(), None, "{key:?} must queue nothing");
        }
        assert_eq!(state.mouse_capture(), capture_before);
    }

    /// The `aux` channel carries a code fence's *language* as well as a link
    /// destination, and both are strings. A viewport showing a fence must
    /// therefore offer no links at all — otherwise `Tab` would select `rust`
    /// and `Enter` would go looking for a file called `rust`.
    #[test]
    fn test_a_code_fences_language_is_never_mistaken_for_a_link_destination() {
        let (_doc, _config, _engine, state) = build("```rust\nfn main() {}\n```\n", 60, 20);
        assert!(
            state.visible_links().is_empty(),
            "a fence's info string is not a link: {:?}",
            state.visible_links()
        );
    }

    /// The other half of the same rule: a link whose text is entirely bold is
    /// styled `Semantic::Strong` by layout, and must still be selectable.
    #[test]
    fn test_a_link_whose_text_is_entirely_bold_is_still_selectable() {
        let (_doc, _config, _engine, mut state) =
            build("Prose [**all bold**](bold.md) prose.\n", 80, 10);
        assert_eq!(targets(&state), vec!["bold.md"]);
        state.handle_key_event(plain(KeyCode::Tab));
        state.handle_key_event(plain(KeyCode::Enter));
        assert_eq!(
            state.take_action(),
            Some(PendingAction::OpenLink("bold.md".to_string()))
        );
    }

    // ------------------------- link selection meets incremental search ----

    /// Search state outlives `Mode::Search` on purpose — `n`/`N` traverse the
    /// matches from normal mode — so a reader can accept a query and *then*
    /// press `Tab`. Both overlays are then live at once, and the frame has to
    /// carry both: a per-mode paint wrapper would drop whichever one the mode
    /// match did not name.
    #[test]
    fn test_a_link_selection_opened_over_an_accepted_search_keeps_both_overlays() {
        let (_doc, _config, _engine, mut state) = build(&linked_source(), 80, 20);

        // `/paragraph` + Enter: matches survive into normal mode.
        state.handle_key_event(plain(KeyCode::Char('/')));
        for c in "paragraph".chars() {
            state.handle_key_event(plain(KeyCode::Char(c)));
        }
        state.handle_key_event(plain(KeyCode::Enter));
        assert_eq!(state.mode(), Mode::Normal);
        let matched = state.search_overlay().matches.len();
        assert!(
            matched > 0,
            "the fixture must match, or this proves nothing"
        );

        // Now select a link. The matches must still be there to paint.
        state.handle_key_event(plain(KeyCode::Tab));
        assert!(matches!(state.mode(), Mode::LinkSelect { .. }));
        assert_eq!(
            state.search_overlay().matches.len(),
            matched,
            "entering link selection must not disturb the accepted search"
        );
        assert!(
            !state.selection_spans().is_empty(),
            "…and the selection must have something to paint"
        );
    }

    /// The mirror: `Esc` out of link selection leaves the search exactly as
    /// it was, so `n` still works.
    #[test]
    fn test_leaving_link_selection_leaves_an_accepted_search_intact() {
        let (_doc, _config, _engine, mut state) = build(&linked_source(), 80, 20);
        state.handle_key_event(plain(KeyCode::Char('/')));
        for c in "paragraph".chars() {
            state.handle_key_event(plain(KeyCode::Char(c)));
        }
        state.handle_key_event(plain(KeyCode::Enter));
        let before = state.search().matches.len();

        state.handle_key_event(plain(KeyCode::Tab));
        state.handle_key_event(plain(KeyCode::Esc));
        assert_eq!(state.mode(), Mode::Normal);
        assert_eq!(state.search().matches.len(), before);
        assert_eq!(state.search().query, "paragraph");
    }

    /// **The interaction nobody had run.** `SearchState::matches` addresses
    /// text by tree line index and byte range; following a link replaces the
    /// tree without going through `relayout`, so neither `reseat_search` nor
    /// `recompute_matches` fires. Carrying the vector across would leave the
    /// painter highlighting whatever bytes now sit at those coordinates.
    #[test]
    fn test_opening_a_document_drops_the_search_that_addressed_the_old_one() {
        let (_doc, config, engine, mut state) = build(&linked_source(), 80, 20);
        state.handle_key_event(plain(KeyCode::Char('/')));
        for c in "paragraph".chars() {
            state.handle_key_event(plain(KeyCode::Char(c)));
        }
        state.handle_key_event(plain(KeyCode::Enter));
        assert!(!state.search().matches.is_empty());

        // A different document, with different text at every line index.
        let next = Document::parse("# Next\n\nnothing here matches that word.\n");
        let ctx = LayoutContext {
            doc: &next,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };
        state.open_document(&ctx, FileInfo::default(), 0);

        assert!(
            state.search().matches.is_empty(),
            "a match addressing the previous tree must not survive the swap"
        );
        assert!(state.search().query.is_empty());
        assert!(
            state.search_overlay().current.is_none(),
            "and nothing may be painted as the current match"
        );
    }

    /// Phase 5's fix-forward added `SearchState::hidden_by_folds`, a count
    /// derived from a fold-free relayout of the document that is about to be
    /// replaced. It is exactly as document-bound as `matches` is, so the drop
    /// on a document swap has to take it too — a leftover count would have
    /// `n` reporting "N matches are folded away" about folds in a file the
    /// reader has left.
    #[test]
    fn test_opening_a_document_also_drops_the_folded_match_count() {
        let source = "# One\n\nneedle here.\n\n# Two\n\nplain text.\n";
        let (doc, config, engine, mut state) = build(source, 40, 20);
        let ctx = ctx_for(&doc, &config, &engine);
        search_for(&mut state, "needle");
        let one = state.outline().entries[0].block;
        state.jump_to_block(one);
        toggle_fold(&mut state, &ctx);
        assert_eq!(
            state.search().hidden_by_folds,
            1,
            "the fixture must really have a match hidden behind a fold"
        );

        let next = Document::parse("# Next\n\nnothing here matches that word.\n");
        let next_ctx = ctx_for(&next, &config, &engine);
        state.open_document(&next_ctx, FileInfo::default(), 0);

        assert_eq!(
            state.search().hidden_by_folds,
            0,
            "a count about the previous document's folds must not survive the swap"
        );
        assert!(state.search().matches.is_empty());
        assert!(state.search().query.is_empty());
    }

    /// The same drop on the way *back*, and the DW-6.2 promise beside it: the
    /// caller's scroll offset is honoured whatever the search was doing.
    #[test]
    fn test_dw_6_2_going_back_restores_the_scroll_and_leaves_no_stale_search() {
        let (_doc, config, engine, mut state) = build(&non_reflowing_source(200), 80, 10);
        state.handle_key_event(plain(KeyCode::Char('/')));
        for c in "line 1".chars() {
            state.handle_key_event(plain(KeyCode::Char(c)));
        }
        state.handle_key_event(plain(KeyCode::Enter));
        assert!(!state.search().matches.is_empty());

        // `Backspace`'s install: the same document, at the remembered offset.
        let same = Document::parse(&non_reflowing_source(200));
        let ctx = LayoutContext {
            doc: &same,
            config: &config,
            engine: &engine,
            sizer: &NullSizer,
        };
        state.open_document(&ctx, FileInfo::default(), 42);

        assert_eq!(state.scroll(), 42, "DW-6.2: the previous scroll position");
        assert_eq!(state.mode(), Mode::Normal);
        assert!(
            state.search().matches.is_empty(),
            "even returning to a document that would match again, the vector \
             that addressed the *other* tree must not be reused"
        );
    }

    /// A `/` prompt open when a link is clicked: the click is a pointing
    /// gesture, not a key, so it is not governed by `captures_all_keys`. What
    /// must hold is that the state it leaves behind is coherent — normal
    /// mode, one queued action, and no half-typed query still owning the row.
    #[test]
    fn test_a_click_while_a_query_prompt_is_open_leaves_coherent_state() {
        let engine = WidthEngine::new(WidthConfig::default());
        let (_doc, _config, _engine, mut state) = build(&linked_source(), 80, 20);
        state.handle_key_event(plain(KeyCode::Char('/')));
        state.handle_key_event(plain(KeyCode::Char('a')));
        assert!(matches!(state.mode(), Mode::Search { .. }));

        let link = state.visible_links().into_iter().next().expect("a link");
        let span = link.spans[0];
        let Some(Line::Items(items)) = state.tree().lines(span.line..span.line + 1).next() else {
            panic!("text line");
        };
        let (start, _) = item_columns(items, &engine, 80)[span.first_item];

        assert!(state.handle_mouse_event(click_at(start, span.line as u16), &engine));
        assert_eq!(
            state.take_action(),
            Some(PendingAction::OpenLink("alpha.md".to_string()))
        );
        assert_eq!(
            state.mode(),
            Mode::Normal,
            "the prompt must not still own the status row after a click that \
             is about to replace the document"
        );
    }

    // ----------------------------- link selection meets section folding ---

    /// A fold is a relayout at the **same width** on the **same document**,
    /// so Phase 4's `reflowed` flag is false for it. `reseat_link_select` is
    /// therefore unconditional — guarding it on `reflowed` would have skipped
    /// exactly the case that changes which links exist.
    ///
    /// Folding cannot actually be reached from `Mode::LinkSelect` (`z` is
    /// chrome, and `captures_all_keys` refuses it — pinned separately). This
    /// drives the state transition directly anyway: the reseat must hold on
    /// its own, not only because a key table happens to forbid the path.
    #[test]
    fn test_a_fold_that_swallows_the_selected_link_leaves_the_mode_coherent() {
        let source = "# One\n\nSee [alpha](alpha.md) here.\n\n# Two\n\nSee [beta](beta.md) here.\n";
        let (doc, config, engine, mut state) = build(source, 60, 20);
        let ctx = ctx_for(&doc, &config, &engine);
        assert_eq!(targets(&state), vec!["alpha.md", "beta.md"]);

        // Select the second link, then collapse everything under it.
        state.handle_key_event(plain(KeyCode::Tab));
        state.handle_key_event(plain(KeyCode::Tab));
        assert_eq!(
            state.selected_link().map(|link| link.target),
            Some("beta.md".to_string())
        );

        state.collapse_all();
        state.relayout_preserving_anchor(&ctx, *ctx.config);

        assert!(
            state.visible_links().is_empty(),
            "collapsing every section must take both links off the screen: {:?}",
            state.visible_links()
        );
        assert_eq!(
            state.mode(),
            Mode::Normal,
            "a selection with nothing left to select must be dismissed, not \
             left indexing a link that is no longer painted"
        );
        assert!(state.selection_spans().is_empty());
        assert_eq!(state.status().message.as_deref(), Some("no links in view"));
    }

    /// The clamp half: a fold that removes *some* links must leave the index
    /// addressing one that still exists rather than pointing past the end.
    #[test]
    fn test_a_fold_that_removes_some_links_clamps_the_selection_into_range() {
        let source = "# One\n\nSee [alpha](alpha.md) here.\n\n# Two\n\nSee [beta](beta.md) here.\n";
        let (doc, config, engine, mut state) = build(source, 60, 20);
        let ctx = ctx_for(&doc, &config, &engine);

        state.handle_key_event(plain(KeyCode::Tab));
        state.handle_key_event(plain(KeyCode::Tab));
        assert_eq!(selected_index(&state), Some(1));

        // Fold only the second section, so exactly one link survives.
        let second = state.tree().outline().entries[1].block;
        state.folds_mut_for_test().collapsed.insert(second);
        state.relayout_preserving_anchor(&ctx, *ctx.config);
        assert_eq!(targets(&state), vec!["alpha.md"]);

        assert_eq!(
            selected_index(&state),
            Some(0),
            "the index must be clamped onto a link that still exists"
        );
        assert_eq!(
            state.selected_link().map(|link| link.target),
            Some("alpha.md".to_string())
        );
        assert!(
            !state.selection_spans().is_empty(),
            "…and the indicator must have somewhere real to paint"
        );
    }

    /// The other direction: unfolding brings links back and the selection is
    /// still usable.
    #[test]
    fn test_unfolding_restores_the_links_a_fold_took_away() {
        let source = "# One\n\nSee [alpha](alpha.md) here.\n\n# Two\n\nSee [beta](beta.md) here.\n";
        let (doc, config, engine, mut state) = build(source, 60, 20);
        let ctx = ctx_for(&doc, &config, &engine);

        state.collapse_all();
        state.relayout_preserving_anchor(&ctx, *ctx.config);
        assert!(state.visible_links().is_empty());

        state.expand_all();
        state.relayout_preserving_anchor(&ctx, *ctx.config);
        assert_eq!(targets(&state), vec!["alpha.md", "beta.md"]);
        state.handle_key_event(plain(KeyCode::Tab));
        state.handle_key_event(plain(KeyCode::Enter));
        assert_eq!(
            state.take_action(),
            Some(PendingAction::OpenLink("alpha.md".to_string()))
        );
    }

    /// Phase 5's three chrome keys arrived after `Mode::LinkSelect` was
    /// written. They must be inert while a link is selected for the same
    /// reason `+`/`-`/`T` are — a relayout rewraps the lines the index was
    /// computed from — and the mode's own keys must not be swallowed on the
    /// way.
    #[test]
    fn test_dw_6_1_the_fold_keys_are_inert_while_a_link_is_selected() {
        let (_doc, _config, _engine, mut state) = build(&linked_source(), 80, 20);
        state.handle_key_event(plain(KeyCode::Tab));
        assert!(matches!(state.mode(), Mode::LinkSelect { .. }));

        for key in ['z', 'R', 'M'] {
            assert_eq!(
                state.chrome_action(plain(KeyCode::Char(key))),
                None,
                "`{key}` must not reach the fold path while a link is selected"
            );
        }
        // ...and the mode still answers its own keys, which is the opposite
        // failure: a gate that swallowed navigation too.
        state.handle_key_event(plain(KeyCode::Tab));
        assert_eq!(selected_index(&state), Some(1));
        state.handle_key_event(plain(KeyCode::Enter));
        assert_eq!(
            state.take_action(),
            Some(PendingAction::OpenLink(
                "https://example.com/beta".to_string()
            ))
        );
    }

    /// **Fold state is per-document and does not travel the document stack.**
    ///
    /// `FoldState::collapsed` holds `NodeId`s, which are dense positional
    /// indices into *one* document — so a fold carried into a linked document
    /// would not be stale, it would be silently *valid* and collapse whatever
    /// section now sits at that index. `open_document` clears the set before
    /// laying out, which is the only ordering that works.
    #[test]
    fn test_dw_6_2_folds_do_not_leak_across_the_document_stack() {
        let source = "# One\n\nbody one.\n\n# Two\n\nbody two.\n";
        let (doc, config, engine, mut state) = build(source, 60, 20);
        let ctx = ctx_for(&doc, &config, &engine);
        state.collapse_all();
        state.relayout_preserving_anchor(&ctx, *ctx.config);
        assert!(
            !state.folds().collapsed.is_empty(),
            "the fixture must really be folded before the hop"
        );
        let folded_lines = state.tree().line_count();

        // Follow a link into a *different* document with headings of its own.
        let next = Document::parse("# Alpha\n\nalpha body.\n\n# Beta\n\nbeta body.\n");
        let next_ctx = ctx_for(&next, &config, &engine);
        state.open_document(&next_ctx, FileInfo::default(), 0);

        assert!(
            state.folds().collapsed.is_empty(),
            "an id from the previous document must not survive into this one"
        );
        let unfolded = layout(&next, 60, &config, &engine, &NullSizer);
        assert_eq!(
            state.tree().line_count(),
            unfolded.line_count(),
            "the linked document must open fully expanded, not with whichever \
             section the old ids happened to name"
        );
        assert!(
            state.tree().line_count() > folded_lines,
            "…and that is genuinely more content than the folded original"
        );

        // And back: `Backspace` returns through the same method, so the
        // original document also comes back expanded. Stated, not hidden.
        state.open_document(&ctx, FileInfo::default(), 0);
        assert!(state.folds().collapsed.is_empty());
        assert_eq!(
            state.tree().line_count(),
            layout(&doc, 60, &config, &engine, &NullSizer).line_count(),
            "fold state does not travel the stack in either direction"
        );
    }

    /// The same seam once more with the arming flag: `pending_fold_snap`
    /// holds a `NodeId` for the *next* relayout of the tree it was computed
    /// against. A document swap replaces that tree, so the flag must not
    /// survive to snap the new document to an unrelated block.
    #[test]
    fn test_a_pending_fold_snap_does_not_survive_a_document_swap() {
        let source = "# One\n\nbody one.\n\n# Two\n\nbody two.\n";
        let (_doc, config, engine, mut state) = build(source, 60, 3);
        // Scroll inside the first section, then arm the snap by folding it.
        // No relayout here on purpose: the flag must still be *pending* when
        // the document is swapped, which is the state under test.
        state.set_scroll(1);
        state.toggle_fold();
        assert!(
            state.pending_fold_snap.is_some(),
            "folding the section the reader is inside must arm the snap"
        );

        let next = Document::parse(&non_reflowing_source(60));
        let next_ctx = ctx_for(&next, &config, &engine);
        state.open_document(&next_ctx, FileInfo::default(), 20);

        assert!(state.pending_fold_snap.is_none());
        assert_eq!(
            state.scroll(),
            20,
            "the caller's scroll must stand — a stale snap would have moved it"
        );
    }

    /// Chrome runs *before* the normal key table, so a chrome key that
    /// collided with one of Phase 6's bindings would shadow it silently — no
    /// compile error, no failing mode test, just a key that stopped working.
    /// Phase 5 added `z`/`R`/`M` after these bindings existed; `M` and `m`
    /// differ only in case.
    #[test]
    fn test_no_chrome_key_shadows_a_phase_6_normal_mode_binding() {
        let (_doc, _config, _engine, state) = build(&linked_source(), 80, 20);
        for code in [
            KeyCode::Tab,
            KeyCode::BackTab,
            KeyCode::Backspace,
            KeyCode::Enter,
            KeyCode::Char('y'),
            KeyCode::Char('m'),
        ] {
            assert_eq!(
                state.chrome_action(plain(code)),
                None,
                "{code:?} is claimed by the chrome table, which runs first — \
                 the Phase 6 binding for it would never fire"
            );
        }
    }

    #[test]
    fn test_captures_all_keys_answers_for_every_mode() {
        assert!(!Mode::Normal.captures_all_keys());
        assert!(Mode::Toc { selected: 0 }.captures_all_keys());
        assert!(Mode::LinkSelect { index: 0 }.captures_all_keys());
        assert!(
            Mode::Explore {
                selected: 0,
                rooted: false,
            }
            .captures_all_keys()
        );
    }

    // ------------------------------------------------------- explore (Phase 3)

    mod explore_tests {
        use std::path::PathBuf;

        use crate::explore::{Entry, EntryKind, Listing};

        use super::*;

        /// A directory fixture with one of every kind `explore::Listing`
        /// classifies, so a test can select any of them by index without
        /// touching the filesystem — `Listing::from_entries` is the pure
        /// seam `explore.rs`'s own doc says exists for exactly this.
        ///
        /// Order: `../`, `alpha/` (directory), `blocked` (unopenable),
        /// `notes.md` (document), `zeta.md` (document). Names are chosen to
        /// sort in this order, matching `Listing::read`'s own ordering, so a
        /// test reasoning about "the next selectable row" is reasoning about
        /// the fixture as written rather than about a sort it has to
        /// remember.
        fn mixed_listing(dir: &Path) -> Listing {
            let entries = vec![
                Entry {
                    name: "..".into(),
                    path: dir.parent().unwrap_or(dir).to_path_buf(),
                    kind: EntryKind::Parent,
                },
                Entry {
                    name: "alpha".into(),
                    path: dir.join("alpha"),
                    kind: EntryKind::Directory,
                },
                Entry {
                    name: "blocked".into(),
                    path: dir.join("blocked"),
                    kind: EntryKind::Unopenable,
                },
                Entry {
                    name: "notes.md".into(),
                    path: dir.join("notes.md"),
                    kind: EntryKind::Document,
                },
                Entry {
                    name: "zeta.md".into(),
                    path: dir.join("zeta.md"),
                    kind: EntryKind::Document,
                },
            ];
            Listing::from_entries(dir.to_path_buf(), entries, false)
        }

        /// A viewport shorter than the fixture document (30 short paragraphs
        /// against 5 rows), so a handful of `j` presses reliably scrolls —
        /// `test_dw_3_5_esc_unrooted_restores_the_readers_scroll_position`
        /// needs a real, non-zero scroll to prove `Esc` restores.
        fn explore_state() -> (PathBuf, AppState) {
            let (_doc, _config, _engine, state) = build(&numbered_paragraphs(30), 80, 5);
            (PathBuf::from("/fixture/dir"), state)
        }

        /// DW-3.1 (unit half): `_` from `Mode::Normal` queues
        /// `PendingAction::OpenExplorer` — the path-free marker `main.rs`
        /// resolves against `Session::source`, since `AppState` cannot name
        /// the open document's directory itself. The pty half
        /// (`explorer_keys.rs`) proves the seating `main.rs` then does with
        /// it; this proves the key is bound to the right action at all.
        #[test]
        fn test_dw_3_1_underscore_in_normal_mode_queues_open_explorer() {
            let (_doc, _config, _engine, mut state) = build(&numbered_paragraphs(5), 80, 10);
            assert_eq!(state.mode(), Mode::Normal);
            let quit = state.handle_key_event(plain(KeyCode::Char('_')));
            assert!(!quit);
            assert_eq!(state.take_action(), Some(PendingAction::OpenExplorer));
            // The mode must not change until the round trip comes back with
            // a listing — there is nothing yet to show.
            assert_eq!(state.mode(), Mode::Normal);
        }

        /// DW-3.4 (unit half): `install_listing` is the one seam that enters
        /// or re-enters `Mode::Explore`, and the rows it exposes come from
        /// the listing it was handed — never from a fresh read.
        #[test]
        fn test_dw_3_4_install_listing_enters_explore_mode_with_the_given_selection() {
            let (dir, mut state) = explore_state();
            state.install_listing(mixed_listing(&dir), 3, false);
            assert_eq!(
                state.mode(),
                Mode::Explore {
                    selected: 3,
                    rooted: false
                }
            );
            let rows = state.explore_rows(20);
            assert_eq!(rows.len(), 5, "every entry in the fixture must paint");
            assert_eq!(rows[3].style, RowStyle::Selected);
            assert_eq!(rows[2].style, RowStyle::Dimmed, "the unopenable row dims");
        }

        /// DW-3.4: nothing in `app.rs` may *call* the read function this
        /// whole module holds itself to never calling — the one function in
        /// the crate that touches a directory. A test that only calls
        /// `install_listing`/`explore_rows` and gets the right rows would
        /// pass just as well against a version that read the directory
        /// itself; this instead inspects the *source text* of the module
        /// under the no-I/O invariant, so an implementation that
        /// reintroduces the call fails here even if its behavior looks
        /// correct.
        ///
        /// Matches on the call shape — the type, `::`, the method name, and
        /// an opening parenthesis, joined at runtime rather than written as
        /// one literal — so this comment can describe the check without the
        /// check tripping over its own description, and this file's own
        /// source cannot contain the literal call pattern by accident
        /// either.
        #[test]
        fn test_dw_3_4_app_rs_never_calls_listing_read() {
            let call_pattern = ["Listing", "::", "read", "("].concat();
            let source = std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app.rs"),
            )
            .expect("app.rs readable");
            assert!(
                !source.contains(&call_pattern),
                "app.rs must never call {call_pattern}...) — AppState performs no I/O"
            );
        }

        /// DW-3.6 (unit half): every chrome key is inert while exploring,
        /// via the ordinary `handle_key_event` dispatch — the pty test in
        /// `explorer_keys.rs` covers the resize-drain dispatch path this one
        /// cannot reach.
        #[test]
        fn test_dw_3_6_chrome_keys_change_nothing_while_exploring() {
            let (dir, mut state) = explore_state();
            state.install_listing(mixed_listing(&dir), 3, false);
            for c in ['+', 'T', 'z', 'R', 'M', '#'] {
                let before = state.mode();
                assert_eq!(
                    state.chrome_action(plain(KeyCode::Char(c))),
                    None,
                    "`{c}` must not be claimed as chrome while exploring"
                );
                state.handle_key_event(plain(KeyCode::Char(c)));
                assert_eq!(
                    state.mode(),
                    before,
                    "`{c}` must change nothing while exploring"
                );
            }
        }

        /// DW-3.6: `-` ascends instead of narrowing — asserted by the queued
        /// action, which is what `-` would never produce if it had been
        /// claimed as `ChromeAction::Narrow` upstream.
        #[test]
        fn test_dw_3_6_dash_ascends_rather_than_narrowing() {
            let (dir, mut state) = explore_state();
            state.install_listing(mixed_listing(&dir), 3, false);
            state.handle_key_event(plain(KeyCode::Char('-')));
            assert_eq!(
                state.take_action(),
                Some(PendingAction::ListDirectory(
                    dir.parent().unwrap().to_path_buf()
                ))
            );
        }

        /// `-` at a listing with no parent row (the fixture the fs root
        /// would produce) is a no-op, not a queued action to nowhere.
        #[test]
        fn test_dash_at_a_listing_with_no_parent_is_a_no_op() {
            let (_dir, mut state) = explore_state();
            let root = PathBuf::from("/");
            let entries = vec![Entry {
                name: "etc".into(),
                path: root.join("etc"),
                kind: EntryKind::Directory,
            }];
            state.install_listing(Listing::from_entries(root, entries, false), 0, false);
            state.handle_key_event(plain(KeyCode::Char('-')));
            assert_eq!(state.take_action(), None);
        }

        /// `j`/`k` skip the unopenable row in both directions and never
        /// land on it.
        #[test]
        fn test_j_and_k_skip_the_unopenable_row_in_both_directions() {
            let (dir, mut state) = explore_state();
            state.install_listing(mixed_listing(&dir), 1, false);
            state.handle_key_event(plain(KeyCode::Char('j')));
            assert_eq!(
                state.mode(),
                Mode::Explore {
                    selected: 3,
                    rooted: false
                },
                "j from `alpha/` must skip `blocked` and land on `notes.md`"
            );
            state.handle_key_event(plain(KeyCode::Char('k')));
            assert_eq!(
                state.mode(),
                Mode::Explore {
                    selected: 1,
                    rooted: false
                },
                "k back must skip `blocked` again"
            );
        }

        /// `Enter` on a directory row queues a re-list at that entry's own
        /// path (DW-3.4); `Enter` on `../` does the same for the parent row.
        #[test]
        fn test_dw_3_4_enter_on_a_directory_row_queues_list_directory() {
            let (dir, mut state) = explore_state();
            state.install_listing(mixed_listing(&dir), 1, false);
            state.handle_key_event(plain(KeyCode::Enter));
            assert_eq!(
                state.take_action(),
                Some(PendingAction::ListDirectory(dir.join("alpha")))
            );

            state.install_listing(mixed_listing(&dir), 0, false);
            state.handle_key_event(plain(KeyCode::Enter));
            assert_eq!(
                state.take_action(),
                Some(PendingAction::ListDirectory(dir.parent().unwrap().into()))
            );
        }

        /// DW-3.3: `Enter` on a document row queues `OpenPath` at that
        /// entry's own path, not a reconstruction from its display text.
        #[test]
        fn test_dw_3_3_enter_on_a_document_row_queues_open_path() {
            let (dir, mut state) = explore_state();
            state.install_listing(mixed_listing(&dir), 3, false);
            state.handle_key_event(plain(KeyCode::Enter));
            assert_eq!(
                state.take_action(),
                Some(PendingAction::OpenPath(dir.join("notes.md")))
            );
        }

        /// `Enter` on the unopenable row is a no-op — reachable only if
        /// `selected` were seated off the movement methods, which this
        /// still defends against rather than assuming away.
        #[test]
        fn test_enter_on_the_unopenable_row_queues_nothing() {
            let (dir, mut state) = explore_state();
            state.install_listing(mixed_listing(&dir), 2, false);
            state.handle_key_event(plain(KeyCode::Enter));
            assert_eq!(state.take_action(), None);
        }

        /// DW-3.5: `Esc` unrooted restores the reader's scroll and closes
        /// the explorer.
        #[test]
        fn test_dw_3_5_esc_unrooted_restores_the_readers_scroll_position() {
            let (dir, mut state) = explore_state();
            // Reach a real, non-zero scroll the ordinary way, before opening
            // the explorer over it.
            for _ in 0..5 {
                state.handle_key_event(plain(KeyCode::Char('j')));
            }
            let scroll_before = state.scroll();
            assert!(scroll_before > 0, "the fixture must actually scroll");

            state.install_listing(mixed_listing(&dir), 0, false);
            let quit = state.handle_key_event(plain(KeyCode::Esc));
            assert!(!quit, "an unrooted Esc must not quit");
            assert_eq!(state.mode(), Mode::Normal);
            assert_eq!(state.scroll(), scroll_before);
        }

        /// DW-3.15: `Esc` and `q` both quit a rooted explorer rather than
        /// falling back to `Mode::Normal`, where the empty placeholder
        /// document would show.
        #[test]
        fn test_dw_3_15_esc_and_q_both_quit_a_rooted_explorer() {
            for key in [KeyCode::Esc, KeyCode::Char('q')] {
                let (dir, mut state) = explore_state();
                state.install_listing(mixed_listing(&dir), 0, true);
                let quit = state.handle_key_event(plain(key));
                assert!(quit, "{key:?} must quit a rooted explorer");
            }
        }

        /// Rootedness survives a re-list untouched — `install_listing` is
        /// given exactly what the caller passes, and `main.rs`'s
        /// `PendingAction::ListDirectory` handler is what is responsible for
        /// reading it back out of the current mode before re-listing.
        #[test]
        fn test_rooted_survives_reinstalling_a_listing() {
            let (dir, mut state) = explore_state();
            state.install_listing(mixed_listing(&dir), 0, true);
            state.install_listing(mixed_listing(&dir), 1, true);
            assert_eq!(
                state.mode(),
                Mode::Explore {
                    selected: 1,
                    rooted: true
                }
            );
        }

        /// DW-3.7 (unit half): a relayout re-clamps `selected` against a
        /// listing that has shrunk since it was installed, rather than
        /// leaving it addressing a row that no longer exists. Driven through
        /// `relayout_preserving_anchor`, the same entry point every chrome
        /// mutation (and, in `main.rs`, every resize) uses.
        #[test]
        fn test_dw_3_7_reseat_explore_reclamps_selection_after_the_listing_shrinks() {
            let dir = PathBuf::from("/fixture/dir");
            let (doc, config, engine, mut state) = build(&numbered_paragraphs(10), 80, 20);
            state.install_listing(mixed_listing(&dir), 4, false);
            assert_eq!(
                state.mode(),
                Mode::Explore {
                    selected: 4,
                    rooted: false
                }
            );

            // A shrunk listing installed directly (as a re-list would), but
            // *without* going through the seating logic `main.rs` applies —
            // exactly the "stale index into a shorter listing" shape DW-3.7
            // names, forced here so `reseat_explore` is the thing proven to
            // fix it rather than `install_listing`'s own clamp (it has
            // none — the caller is trusted to pass a valid index, and this
            // test is what proves the *next* relayout no longer trusts it
            // blindly).
            let shorter = Listing::from_entries(
                dir.clone(),
                vec![Entry {
                    name: "only.md".into(),
                    path: dir.join("only.md"),
                    kind: EntryKind::Document,
                }],
                false,
            );
            state.install_listing(shorter, 4, false);
            let ctx = LayoutContext {
                doc: &doc,
                config: &config,
                engine: &engine,
                sizer: &NullSizer,
            };
            state.relayout_preserving_anchor(&ctx, config);
            assert_eq!(
                state.mode(),
                Mode::Explore {
                    selected: 0,
                    rooted: false
                },
                "the only row left is index 0 — selected must land there, not stay at 4"
            );
        }

        /// `-` ascends to the `../` row's **recorded** path, not to whatever
        /// `dir().parent()` would compute a second time.
        ///
        /// The listing here is the shape `stele .` produces: a directory of
        /// `.` whose parent row points at `..`. `Path::new(".").parent()` is
        /// `Some("")`, so the old implementation queued a read of the empty
        /// path and dropped the reader into a listing with no entries, no
        /// parent row and no working key. This asserts on the exact path
        /// queued, which is the only assertion that can tell the two apart.
        #[test]
        fn test_dash_ascends_to_the_parent_rows_own_path_not_a_recomputed_one() {
            let (_dir, mut state) = explore_state();
            let entries = vec![
                Entry {
                    name: "..".into(),
                    path: PathBuf::from(".."),
                    kind: EntryKind::Parent,
                },
                Entry {
                    name: "notes.md".into(),
                    path: PathBuf::from("./notes.md"),
                    kind: EntryKind::Document,
                },
            ];
            let listing = Listing::from_entries(PathBuf::from("."), entries, false);
            state.install_listing(listing, 1, false);
            state.handle_key_event(plain(KeyCode::Char('-')));
            assert_eq!(
                state.take_action(),
                Some(PendingAction::ListDirectory(PathBuf::from(".."))),
                "`-` must follow the ../ row, not `Path::new(\".\").parent()`"
            );
        }

        /// `Enter` on a document row leaves `Mode::Explore` through
        /// `open_document`, which must drop the listing with it — the exit
        /// that used to leak one.
        #[test]
        fn test_opening_a_document_drops_the_listing_behind_it() {
            let source = numbered_paragraphs(30);
            let (doc, config, engine, mut state) = build(&source, 80, 5);
            let dir = PathBuf::from("/fixture/dir");
            state.install_listing(mixed_listing(&dir), 3, false);
            assert!(state.explore_dir().is_some(), "test setup: a listing is up");

            let ctx = ctx_for(&doc, &config, &engine);
            state.open_document(
                &ctx,
                FileInfo {
                    name: "opened.md".to_string(),
                    byte_size: source.len() as u64,
                    line_count: source.lines().count(),
                },
                0,
            );

            assert_eq!(state.mode(), Mode::Normal);
            assert_eq!(
                state.explore_dir(),
                None,
                "`explore_dir` documents itself as `None` outside Mode::Explore, \
                 and up to 256 `Entry` records were staying resident besides"
            );
            assert!(
                state.explore_rows(10).is_empty(),
                "and nothing is left for a stale listing to paint"
            );
        }

        /// A `--watch` reload must not wipe a message the *explorer* put on
        /// the status row.
        ///
        /// `reload_document` clears the message because a message describes
        /// the document that produced it, and a reloaded document is a
        /// different one. An explorer refusal describes a different file
        /// entirely, and DW-3.10 gives the row to the explorer while it is
        /// open — so `stele --watch notes.md`, `_`, `Enter` on a refused
        /// file, and the next quarter-second tick erased the reason.
        #[test]
        fn test_a_watch_reload_leaves_an_explorer_message_standing() {
            let (_doc, config, engine, mut state) = build(&numbered_paragraphs(30), 80, 5);
            let dir = PathBuf::from("/fixture/dir");
            state.install_listing(mixed_listing(&dir), 3, false);
            state.set_status(StatusMessage::new("cannot open: not valid UTF-8"));

            reload(&mut state, &numbered_paragraphs(31), &config, &engine);

            assert_eq!(
                state.status().message.as_deref(),
                Some("cannot open: not valid UTF-8"),
                "the refusal describes the file the reader just tried to open, \
                 not the document that reloaded underneath the overlay"
            );
        }

        /// The other direction, so the fix above cannot be "never clear
        /// anything": with the explorer closed, a reload still takes the
        /// message down, and the explorer's own status text comes back once
        /// the message has aged out.
        #[test]
        fn test_a_watch_reload_still_clears_a_document_message_in_normal_mode() {
            let (_doc, config, engine, mut state) = build(&numbered_paragraphs(30), 80, 5);
            state.set_status(StatusMessage::new("reload failed: something"));
            reload(&mut state, &numbered_paragraphs(31), &config, &engine);
            assert_eq!(state.status().message, None);
        }
    }
}
