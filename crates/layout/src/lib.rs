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
/// occupies `rows` consecutive [`Line::Reserved`] lines all carrying this
/// same value — consumers find the box top by scanning back over equal
/// `node_id`. `cols`/`rows` are already scaled to fit the layout width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reserved {
    pub node_id: NodeId,
    pub cols: u16,
    pub rows: u16,
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
    /// always 1; the field is kept so P6's `paint(reserved, rect)` seam takes
    /// the same [`Reserved`] value for inline and block media alike.
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Line {
    Items(Vec<LineItem>),
    Reserved(Reserved),
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
            Line::Reserved(r) => r.cols,
        }
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
}

/// Lay out a parsed document at `width` cells.
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
    let min = config.min_width.max(1);
    let max = config.max_width.max(min);
    let width = width.clamp(min, max);
    let mut ctx = block::Ctx::new(doc, engine, sizer, width);
    block::walk_blocks(&mut ctx, doc.blocks(), true);
    let (lines, line_blocks) = ctx.into_parts();
    LayoutTree {
        lines,
        line_blocks,
        width,
    }
}
