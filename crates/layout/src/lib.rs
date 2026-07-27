//! `crates/layout` — pure, deterministic layout: AST + width in, retained
//! line-box tree out.
//!
//! One entry point, [`layout`], hides everything: inline flattening and
//! greedy wrapping over grapheme clusters, prefix composition for nested
//! containers, table intrinsic measurement and css-tables-3 §3.9.3
//! distribution with the overflow ladder (wrap-in-cell → per-column floors →
//! clip with indicator), and reserved-box scaling. The function is a pure
//! map of its arguments: no I/O, no clock, no global or hidden state, no
//! hash-map iteration order — identical inputs always produce a
//! structurally identical (hash-equal) [`LayoutTree`].
//!
//! Reflow on resize is re-layout from the retained [`Document`], never from
//! source text; the signature enforces it. Media (P6) plugs in through the
//! [`IntrinsicSizer`] seam only — the default [`NullSizer`] sizes nothing,
//! so images and math degrade to alt-text/TeX-source runs and this crate is
//! complete without any media machinery.

#![forbid(unsafe_code)]

use std::collections::HashSet;
use std::ops::Range;

use ast::{Document, NodeId};
use width::WidthEngine;

mod block;
mod inline;
mod table;

/// A width × height extent in terminal cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellSize {
    pub cols: u16,
    pub rows: u16,
}

/// The range the viewport width is clamped to before layout.
///
/// The ambiguous-width policy is *not* here — it lives in
/// [`WidthEngine`]'s config (P3 seam).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutConfig {
    pub min_width: u16,
    pub max_width: u16,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        LayoutConfig {
            min_width: 24,
            max_width: 100,
        }
    }
}

/// The media seam (P6). Given a node id (an [`ast::InlineKind::Image`] or
/// [`ast::InlineKind::Math`] node), report its natural size in cells, or
/// `None` to decline — layout then falls back to the node's textual content
/// (alt text / TeX source). The `NodeId` handed here is the same identity
/// P6 uses to resolve the image path or math source via [`Document::node`],
/// and the same identity the emitted [`Reserved`] lines carry.
pub trait IntrinsicSizer {
    fn size(&self, node: NodeId, doc: &Document) -> Option<CellSize>;
}

/// The no-media default sizer: declines every node, so every image lays out
/// as its alt text and every formula as its TeX source.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullSizer;

impl IntrinsicSizer for NullSizer {
    fn size(&self, _node: NodeId, _doc: &Document) -> Option<CellSize> {
        None
    }
}

/// Alert flavor carried by [`Semantic::AlertTitle`]. Mirrors
/// [`ast::AlertKind`] (which does not derive `Hash`, so it cannot be
/// embedded here directly).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlertTone {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

/// How a heading level's text is cased when rendered.
///
/// This lives in `layout`, not in the theme, because case changes *text* and
/// therefore changes measured width (`ß`.to_uppercase() is `SS`) — it has to
/// be settled before wrapping, not painted on afterwards. `highlight`
/// re-exports it; the dependency only runs that way.
///
/// Display-only in the sense that matters: the outline and the TOC read
/// `heading_text` off the AST and keep the author's own capitalisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeadingCase {
    /// H1, H2 — the author's own casing, untouched.
    AsWritten,
    /// H3, H4 — `SUPPORTED PLATFORMS`.
    Upper,
    /// H5, H6 — `Checksum Mismatches`.
    Title,
}

/// The case transform for a heading level. Clamps like every other consumer,
/// so an out-of-range level degrades to the nearest rung.
pub fn heading_case(level: u8) -> HeadingCase {
    match level.clamp(1, 6) {
        1 | 2 => HeadingCase::AsWritten,
        3 | 4 => HeadingCase::Upper,
        _ => HeadingCase::Title,
    }
}

/// The style roles layout can distinguish. Theme resolution (role → color/
/// attribute) happens later, at paint (P5) and theming (P7); layout only
/// says *what* a run is, never how it looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Semantic {
    /// Plain body text (also table body cells and padding).
    Text,
    /// Heading content; the level is 1–6.
    Heading(u8),
    Emph,
    Strong,
    Strikethrough,
    /// Inline code span content.
    CodeInline,
    /// Code block content (also its clip padding).
    CodeBlock,
    /// Link label content.
    Link,
    /// Image alt text laid out in place of an unsized image.
    ImageAlt,
    /// Raw TeX source laid out in place of an unsized formula.
    MathTex,
    /// List bullet / ordered-number marker.
    ListMarker,
    /// GFM task checkbox marker (`[x]` / `[ ]`).
    TaskMarker,
    /// Blockquote (and alert body) gutter bar.
    BlockquoteMarker,
    /// GitHub alert title line (`[!NOTE]` …).
    AlertTitle(AlertTone),
    /// Thematic break rule.
    Rule,
    /// Table cell separators and the header rule.
    TableBorder,
    /// Table header cell content.
    TableHeader,
    /// `[^label]` reference in running text.
    FootnoteRef,
    /// `[^label]` marker prefixing a footnote definition.
    FootnoteLabel,
    /// Raw HTML shown as literal text.
    Html,
    /// YAML frontmatter shown as literal text.
    FrontMatter,
    /// The `…` clip indicator (code blocks, table rung 3).
    OverflowIndicator,
    /// A stretch of text matching the active search query. Layout never
    /// emits this — the search overlay (`crates/stele/src/app.rs`) re-tags
    /// runs with it at paint time — but it lives here because it is a style
    /// *role*, and the exhaustive tables that must answer for it
    /// (`highlight::theme`, `stele::decor`) are keyed on this enum.
    SearchMatch,
    /// The one match the reader is currently on, styled distinctly from the
    /// rest so `n`/`N` traversal is visible.
    SearchCurrent,
}

/// Style identity on a run. Layout emits **only** [`StyleId::Semantic`];
/// the `Capture` range is allocated by `crates/highlight` (P7) when syntax
/// captures replace semantic roles inside code blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StyleId {
    Semantic(Semantic),
    Capture(u16),
}

/// One styled fragment of a line. `width` is the text's total cell width as
/// measured through the [`WidthEngine`] the tree was laid out with.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Run {
    pub text: String,
    pub style_id: StyleId,
    pub width: u16,
    /// Auxiliary paint-time data whose meaning is fixed by `style_id`, since
    /// a run is exactly one kind: the code-fence info string (language) for a
    /// `Semantic::CodeBlock` run, or the destination URL for a `Semantic::Link`
    /// run. `None` for every other run. This is the channel the painter needs
    /// to pass a language to `Decor::highlight` and to wrap link text in an
    /// OSC 8 hyperlink — layout is the only place that still has the AST's
    /// `info`/`dest` in hand.
    pub aux: Option<Box<str>>,
}

/// A cell region reserved for media (P6 paints into it). A box `rows` tall
/// occupies `rows` consecutive [`Line::Reserved`] lines carrying the same
/// `node_id`/`cols`/`rows` and an ascending [`row`](Self::row).
/// `cols`/`rows` are already scaled to fit the layout width.
///
/// Every field is a fact about the *box*, and that is what lets one type serve
/// both a standalone [`Line::Reserved`] row and an inline [`LineItem::Box`]
/// (where `rows == 1` and `row == 0` are true statements about the box, not a
/// filled-in convention). The row's container prefix is deliberately not here:
/// it is a property of the *line*, and it lives on [`ReservedLine`].
///
/// The box's *column* is not a field either: for a standalone box it is the
/// total width of [`ReservedLine::prefix`], and for an inline
/// [`LineItem::Box`] it depends on the items to its left — the painter knows
/// both and passes the answer as `CellRect::x`, so a `col` here could only
/// restate it (and would have to lie in the inline case).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reserved {
    pub node_id: NodeId,
    pub cols: u16,
    pub rows: u16,
    /// 0-based index of *this* line within the box (`0..rows`).
    ///
    /// Load-bearing for scrolling, not bookkeeping: a viewport shows only a
    /// contiguous slice of the box's lines, so a consumer that sees the box
    /// start at its own first `paint` call cannot tell "row 0 of the box"
    /// from "row 7, the top 7 having scrolled above the viewport" — and a
    /// media sink that guesses the former anchors the whole image at the top
    /// visible row and paints it down over the text below. Scanning back
    /// over equal `node_id` is not an option either: the lines above the
    /// viewport are simply not painted.
    pub row: u16,
}

/// One row of a standalone reserved box: the box, plus the container prefix
/// that row is painted behind.
///
/// The split is what keeps the media seam honest. `MediaSink::paint` is handed
/// the [`Reserved`] alone, so the prefix runs — which are the painter's to
/// draw, and which mean nothing to a sink that positions by `CellRect` — are
/// not in its reach at all. While they were one struct the rule was enforced
/// only by prose, and every sink test had to fabricate a `prefix` field that
/// none of them read. It also keeps an always-empty `Vec` off an inline
/// [`LineItem::Box`], whose line carries its container prefix as its own
/// leading runs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReservedLine {
    /// The container prefix painted to the left of this box row: the
    /// blockquote gutter bar, the list marker on row 0 and the continuation
    /// indent on the rows below it, a footnote label — already composed and
    /// clipped by the same `prefix_runs()` a text line goes through. Empty at
    /// the top level.
    ///
    /// It is a property of *this row of this box* that nothing downstream can
    /// reconstruct. The painter needs the runs to paint the gutter, and their
    /// total width **is** the column the box starts at — one source of truth
    /// for both, rather than a `col` that could drift from the runs beside it.
    pub prefix: Vec<Run>,
    pub boxed: Reserved,
}

impl ReservedLine {
    /// Total cell width of [`prefix`](Self::prefix) — the column this box row
    /// starts at. Saturating, matching the rest of the width arithmetic here.
    pub fn prefix_width(&self) -> u16 {
        self.prefix
            .iter()
            .fold(0u16, |acc, run| acc.saturating_add(run.width))
    }
}

/// One item on a text line: a styled text fragment, or an inline media box
/// sitting on that line's baseline.
///
/// The box case is what makes `$e^{i\pi}+1=0$` render *inside* its sentence
/// instead of interrupting it. An inline box is always exactly **one row**
/// tall — [`LayoutTree`] is a flat list where one [`Line`] is one terminal
/// row and scrolling addresses it by line index, so a taller line box would
/// break scroll math everywhere. A formula whose natural height exceeds one
/// row therefore keeps the standalone [`Line::Reserved`] path (display math,
/// which wants its own rows anyway).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LineItem {
    Run(Run),
    /// An inline media box occupying `cols` columns of this row. `rows` is
    /// always 1 and `row` always 0, so P6's `paint(reserved, rect)` seam takes
    /// the same [`Reserved`] value for inline and block media alike. The
    /// container prefix is not on the box (see [`ReservedLine`]) — this line's
    /// own leading runs carry it.
    Box(Reserved),
}

impl LineItem {
    /// Cell width this item occupies on its line.
    pub fn width(&self) -> u16 {
        match self {
            LineItem::Run(run) => run.width,
            LineItem::Box(reserved) => reserved.cols,
        }
    }
}

/// One terminal row: either a sequence of line items (styled text and any
/// inline media boxes riding the baseline) or one row of a standalone
/// reserved media box.
///
/// A standalone box gets its own variant rather than becoming a
/// [`LineItem::Box`] on an `Items` line because the two are painted by
/// different machinery, and because the inline path is one row tall by
/// construction and cannot describe a multi-row box. The row's container
/// prefix rides along inside [`ReservedLine::prefix`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Line {
    Items(Vec<LineItem>),
    Reserved(ReservedLine),
}

impl Line {
    /// Total cell width of this line. Saturating, matching the rest of the
    /// width arithmetic here: a line past `u16::MAX` is pathological, and an
    /// honest-but-capped width still drives clipping correctly, whereas a
    /// summing overflow would panic in a debug build.
    pub fn width(&self) -> u16 {
        match self {
            Line::Items(items) => items
                .iter()
                .fold(0u16, |acc, item| acc.saturating_add(item.width())),
            Line::Reserved(r) => r.prefix_width().saturating_add(r.boxed.cols),
        }
    }
}

/// One heading in the document's [`Outline`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OutlineEntry {
    /// The heading's level, 1–6, exactly as [`Semantic::Heading`] carries it.
    pub level: u8,
    /// The heading's text, flattened from its inline children — so
    /// `## a *b* `c`` reads as `a b c` rather than as three styled fragments.
    /// Taken from the AST rather than from the emitted runs because a run's
    /// style is overridden inside emphasis and code spans, and an outline
    /// built from runs would drop exactly those words.
    pub text: String,
    /// The block to anchor a jump on: the heading's own node for a top-level
    /// heading (the ordinary case), and the enclosing top-level block for a
    /// heading nested inside a blockquote or list item — because the tree's
    /// per-line block tags name only top-level blocks (see
    /// [`LayoutTree::block_at`]), so a nested heading's own id would address
    /// no line at all.
    ///
    /// [`Outline::line_of`] is the exact answer for every heading regardless
    /// of nesting; this field is what the plan's seam promises and what
    /// callers that think in blocks (folding, in a later phase) will want.
    pub block: NodeId,
}

/// Every heading in the laid-out document, in document order — built once
/// during layout, when level, text, anchoring block and emitted line are all
/// in hand at the same moment.
///
/// The line each heading starts at is kept private rather than published on
/// [`OutlineEntry`]: it is only meaningful against *this* tree, and the tree
/// owns the outline, so the pairing cannot come apart. Callers navigate
/// through [`Outline::next_after`] / [`Outline::previous_before`] /
/// [`Outline::line_of`] and never hold a line index across a relayout.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Outline {
    /// The headings, in document order.
    pub entries: Vec<OutlineEntry>,
    /// The tree line each entry's heading starts at, in lockstep with
    /// `entries` and strictly increasing.
    lines: Vec<usize>,
}

impl Outline {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The tree line the `index`-th heading starts at.
    pub fn line_of(&self, index: usize) -> Option<usize> {
        self.lines.get(index).copied()
    }

    /// The index of the first heading strictly below `line` — `]]`'s answer.
    /// Strict, so repeating the jump keeps moving, and `None` at the last
    /// heading so the caller can clamp rather than wrap.
    pub fn next_after(&self, line: usize) -> Option<usize> {
        self.lines.iter().position(|&l| l > line)
    }

    /// The index of the last heading strictly above `line` — `[[`'s answer.
    pub fn previous_before(&self, line: usize) -> Option<usize> {
        self.lines.iter().rposition(|&l| l < line)
    }

    /// The index of the heading whose section contains `line`: the last one
    /// at or above it. This is "which heading am I under", which is what the
    /// TOC opens on — distinct from [`Outline::previous_before`], which is a
    /// *motion* and must never answer with the heading already at the top of
    /// the viewport.
    pub fn index_at_or_before(&self, line: usize) -> Option<usize> {
        self.lines.iter().rposition(|&l| l <= line)
    }

    /// The line a heading anchored on `block` starts at, for the nested case
    /// [`LayoutTree::first_line_of`] cannot answer.
    pub fn line_for_block(&self, block: NodeId) -> Option<usize> {
        let index = self.entries.iter().position(|e| e.block == block)?;
        self.line_of(index)
    }

    pub(crate) fn push(&mut self, entry: OutlineEntry, line: usize) {
        self.entries.push(entry);
        self.lines.push(line);
    }
}

/// The retained layout: a flat sequence of lines at one width. Scrolling
/// and painting (P5) address it purely by line range.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LayoutTree {
    lines: Vec<Line>,
    /// The top-level source block each line belongs to, in lockstep with
    /// `lines`. Powers block-anchored scroll on resize (DW-5.3): re-lay out at
    /// a new width, then re-anchor to the block that was at the top rather
    /// than guessing a proportional offset.
    line_blocks: Vec<Option<NodeId>>,
    outline: Outline,
    width: u16,
}

impl LayoutTree {
    /// The lines in `range`, clamped to bounds (never panics — P5's scroll
    /// math may overshoot at the document tail).
    pub fn lines(&self, range: Range<usize>) -> impl Iterator<Item = &Line> {
        let end = range.end.min(self.lines.len());
        let start = range.start.min(end);
        self.lines[start..end].iter()
    }

    /// Total number of lines.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// The width this tree was laid out at, after clamping to the
    /// [`LayoutConfig`] range. No line exceeds it.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// The top-level source block the line at `index` belongs to, or `None`
    /// for an out-of-range index or a line with no owning block.
    pub fn block_at(&self, index: usize) -> Option<NodeId> {
        self.line_blocks.get(index).copied().flatten()
    }

    /// The first line index owned by `block`, or `None` if the block emitted
    /// no lines in this tree. The scroll anchor for resize: relayout, then
    /// scroll so the previously-topmost block is at the top again.
    pub fn first_line_of(&self, block: NodeId) -> Option<usize> {
        self.line_blocks.iter().position(|b| *b == Some(block))
    }

    /// Every heading in this tree, in document order — the outline heading
    /// navigation and the TOC overlay are built on. Rebuilt with the tree, so
    /// its line indices are always the ones this tree uses.
    pub fn outline(&self) -> &Outline {
        &self.outline
    }
}

/// Which sections are folded, keyed by the heading [`NodeId`] whose section
/// is collapsed (Phase 5) — not by a line index, which would not survive a width
/// change (folding changes which lines exist at all) or a `--watch` reload
/// (the whole tree, and every id in it, is replaced; see `AppState`'s own
/// reload-time re-keying, which is the layer that owns identity across a
/// re-parse — `FoldState` itself has no opinion on it beyond the ids it is
/// handed).
///
/// Consulted by [`layout_with_folds`] during the walk, at the top level
/// only: a heading nested inside a blockquote or list item is not itself
/// foldable (out of Phase 5's scope), so an id that never names a top-level
/// heading is simply never matched.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FoldState {
    pub collapsed: HashSet<NodeId>,
}

impl FoldState {
    pub fn is_folded(&self, id: NodeId) -> bool {
        self.collapsed.contains(&id)
    }

    /// Folds `id` if it is open, opens it if it is folded.
    pub fn toggle(&mut self, id: NodeId) {
        if !self.collapsed.remove(&id) {
            self.collapsed.insert(id);
        }
    }
}

/// Lay out a parsed document at `width` cells, with nothing folded.
///
/// Pure and deterministic: identical `(doc, width, config, engine, sizer)`
/// inputs yield structurally identical trees. `width` is clamped to
/// `config`'s range first. An empty document yields a valid empty tree.
pub fn layout(
    doc: &Document,
    width: u16,
    config: &LayoutConfig,
    engine: &WidthEngine,
    sizer: &dyn IntrinsicSizer,
) -> LayoutTree {
    layout_with_folds(doc, width, config, engine, sizer, &FoldState::default())
}

/// Like [`layout`], but consulting `folds` during the walk (Phase 5): a top-level
/// heading whose id is in [`FoldState::collapsed`] collapses its section —
/// itself and every block up to the next heading of equal or shallower level
/// — to exactly one marked line (DW-5.1, DW-5.6). Still pure and
/// deterministic in all six arguments; `layout` is the zero-folds special
/// case of this function, not the other way around.
pub fn layout_with_folds(
    doc: &Document,
    width: u16,
    config: &LayoutConfig,
    engine: &WidthEngine,
    sizer: &dyn IntrinsicSizer,
    folds: &FoldState,
) -> LayoutTree {
    let min = config.min_width.max(1);
    let max = config.max_width.max(min);
    let width = width.clamp(min, max);
    let mut ctx = block::Ctx::new(doc, engine, sizer, width, folds);
    block::walk_blocks(&mut ctx, doc.blocks(), true);
    let (lines, line_blocks, outline) = ctx.into_parts();
    LayoutTree {
        lines,
        line_blocks,
        outline,
        width,
    }
}
