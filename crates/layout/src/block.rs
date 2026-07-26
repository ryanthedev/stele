//! The block walker: containers push prefix segments (gutter bars, list
//! markers), leaves emit wrapped/clipped lines through [`Ctx`].
//!
//! Prefix discipline: every content line = composed prefix runs + content
//! runs. The prefix is capped so at least `min(8, width)` content columns
//! always remain (the nested-indent clamp) and a composed line never
//! exceeds the layout width.

use ast::{AlertKind, Block, BlockKind, Document, ListKind};
use width::WidthEngine;

use crate::inline::{self, Piece};
use crate::{
    AlertTone, CellSize, IntrinsicSizer, Line, LineItem, Outline, OutlineEntry, Reserved,
    ReservedLine, Run, Semantic, StyleId,
};

/// Minimum content columns the prefix may never consume (clamped to the
/// layout width itself at pathological widths).
const MIN_CONTENT_COLS: u16 = 8;

/// Gutter bar for blockquotes and alerts.
const QUOTE_BAR: &str = "\u{2502} "; // "│ "

struct Seg {
    /// Emitted on the first line after the segment is pushed (the marker).
    first: String,
    /// Emitted on every later line (the continuation indent).
    rest: String,
    style: Semantic,
    used: bool,
}

pub(crate) struct Ctx<'a> {
    pub doc: &'a Document,
    pub engine: &'a WidthEngine,
    pub sizer: &'a dyn IntrinsicSizer,
    width: u16,
    lines: Vec<Line>,
    /// The top-level block each emitted line belongs to, in lockstep with
    /// `lines`. Lets the viewer re-anchor scroll to a source block on resize
    /// instead of a proportional guess (DW-5.3).
    line_blocks: Vec<Option<ast::NodeId>>,
    /// The top-level block currently being walked; tags every line emitted
    /// while inside it (including its nested descendants).
    current_block: Option<ast::NodeId>,
    /// Walk depth: 1 at the top-level `walk_blocks`, deeper inside
    /// blockquotes/lists. Only depth 1 sets `current_block`.
    depth: usize,
    prefix: Vec<Seg>,
    /// The headings emitted so far, with the line each started at. Collected
    /// here rather than by a later scan over `lines` because this is the only
    /// point at which a heading's level, its inline children, its anchoring
    /// block and its first emitted line are all in hand at once.
    outline: Outline,
}

impl<'a> Ctx<'a> {
    pub fn new(
        doc: &'a Document,
        engine: &'a WidthEngine,
        sizer: &'a dyn IntrinsicSizer,
        width: u16,
    ) -> Self {
        Ctx {
            doc,
            engine,
            sizer,
            width,
            lines: Vec::new(),
            line_blocks: Vec::new(),
            current_block: None,
            depth: 0,
            prefix: Vec::new(),
            outline: Outline::default(),
        }
    }

    /// Push one line, tagging it with the current top-level block so
    /// `lines` and `line_blocks` stay the same length.
    fn push_line(&mut self, line: Line) {
        self.lines.push(line);
        self.line_blocks.push(self.current_block);
    }

    pub fn into_parts(self) -> (Vec<Line>, Vec<Option<ast::NodeId>>, Outline) {
        (self.lines, self.line_blocks, self.outline)
    }

    /// Lay out one heading and record it in the outline.
    ///
    /// The line is captured *before* the flow and the entry pushed after, so
    /// a heading that wraps to several lines is still addressed by its first.
    /// A heading that emitted no line at all (only reachable from an empty
    /// heading at a pathological width) is not recorded: an outline entry
    /// nothing can jump to would be a dead row in the TOC.
    fn heading(&mut self, level: u8, children: &[ast::Inline], id: ast::NodeId) {
        let line = self.lines.len();
        self.flow(children, Semantic::Heading(level));
        if self.lines.len() > line {
            self.outline.push(
                OutlineEntry {
                    level,
                    text: heading_text(children),
                    block: self.current_block.unwrap_or(id),
                },
                line,
            );
        }
    }

    /// Push a container prefix; `first` shows on the next emitted line,
    /// `rest` (same display width by construction) on lines after that.
    fn push_prefix(&mut self, first: String, rest: String, style: Semantic) {
        self.prefix.push(Seg {
            first,
            rest,
            style,
            used: false,
        });
    }

    fn pop_prefix(&mut self) {
        self.prefix.pop();
    }

    /// The widest prefix layout will actually honor.
    fn prefix_budget(&self) -> u16 {
        self.width - MIN_CONTENT_COLS.min(self.width)
    }

    /// Columns available to content after the (possibly clamped) prefix.
    pub fn content_width(&self) -> u16 {
        let pw: u16 = self.prefix.iter().fold(0u16, |acc, s| {
            acc.saturating_add(inline::cells(self.engine, &s.rest))
        });
        self.width - pw.min(self.prefix_budget())
    }

    /// Compose the current prefix as runs (consuming each segment's `first`
    /// form), clamped to the prefix budget.
    fn prefix_runs(&mut self) -> Vec<Run> {
        let mut runs: Vec<Run> = Vec::new();
        for seg in &mut self.prefix {
            let text = if seg.used { &seg.rest } else { &seg.first };
            seg.used = true;
            let w = self.engine.display_width(text) as u16;
            inline::append_run(&mut runs, text, StyleId::Semantic(seg.style), w);
        }
        inline::clip_runs(runs, self.prefix_budget(), false, self.engine)
    }

    /// Emit one content line under the current prefix. The prefix is always
    /// plain text (gutter bars, list markers), so it composes as runs; only
    /// the content can carry an inline media box.
    pub(crate) fn emit(&mut self, content: Vec<LineItem>) {
        let mut items: Vec<LineItem> = self.prefix_runs().into_iter().map(LineItem::Run).collect();
        items.extend(content);
        self.push_line(Line::Items(items));
    }

    /// [`emit`](Self::emit) for content that is text by construction — rules,
    /// alert titles, table rows. Saves every such caller from restating the
    /// lift into [`LineItem`].
    pub(crate) fn emit_runs(&mut self, content: Vec<Run>) {
        self.emit(content.into_iter().map(LineItem::Run).collect());
    }

    /// Vertical spacing: a prefix-only line.
    fn blank_line(&mut self) {
        self.emit(Vec::new());
    }

    /// Emit a reserved media box scaled to fit the current content width,
    /// one `Line::Reserved` per row.
    ///
    /// Every row composes the container prefix exactly as [`emit`](Self::emit)
    /// does — same `prefix_runs()`, so a segment's `first` form (the list
    /// marker) is consumed on the box's first row and its `rest` form (the
    /// continuation indent) on the rest. Skipping that is not a cosmetic loss:
    /// `prefix_runs` is the only thing that consumes a prefix segment, so a
    /// list item whose *only* child is an image never emitted its marker at
    /// all and an ordered list silently began at "2.".
    fn emit_box(&mut self, node_id: ast::NodeId, size: CellSize) {
        let cw = self.content_width().max(1);
        let (cols, rows) = if size.cols <= cw {
            (size.cols.max(1), size.rows.max(1))
        } else {
            // Scale to fit, preserving aspect ratio (ceil, at least 1 row).
            let rows = (u32::from(size.rows.max(1)) * u32::from(cw)).div_ceil(u32::from(size.cols));
            (cw, rows.clamp(1, u32::from(u16::MAX)) as u16)
        };
        for row in 0..rows {
            let prefix = self.prefix_runs();
            self.push_line(Line::Reserved(ReservedLine {
                prefix,
                boxed: Reserved {
                    node_id,
                    cols,
                    rows,
                    row,
                },
            }));
        }
    }

    /// Wrap `pieces` (already flattened) under the current prefix.
    fn emit_pieces(&mut self, pieces: Vec<Piece>) {
        if pieces.is_empty() {
            // An empty block (e.g. `# ` heading) still occupies one line.
            self.emit(Vec::new());
            return;
        }
        for piece in pieces {
            match piece {
                Piece::Items(items) => self.emit(items),
                Piece::Box(node, size) => self.emit_box(node, size),
            }
        }
    }

    /// Flatten + wrap an inline sequence under `base` style and emit it.
    fn flow(&mut self, inlines: &[ast::Inline], base: Semantic) {
        let atoms = inline::flatten(inlines, base, self.doc, self.sizer, true);
        let pieces = inline::wrap(atoms, self.content_width(), self.engine);
        self.emit_pieces(pieces);
    }

    /// Emit literal text line by line: clipped with an indicator, never
    /// wrapped (code blocks, HTML blocks, frontmatter). Tabs expand to
    /// four spaces (the width engine measures cells, not control bytes).
    ///
    /// `aux` carries the code fence's info string (language) onto every
    /// emitted line's run, so the painter can hand it to `Decor::highlight`;
    /// HTML and frontmatter blocks pass `None`.
    fn literal_block(&mut self, literal: &str, style: Semantic, aux: Option<Box<str>>) {
        let cw = self.content_width();
        let mut src_lines: Vec<&str> = literal.split('\n').collect();
        if src_lines.last() == Some(&"") {
            src_lines.pop(); // the conventional trailing newline
        }
        for src in src_lines {
            let text = if src.contains('\t') {
                src.replace('\t', "    ")
            } else {
                src.to_string()
            };
            let w = inline::cells(self.engine, &text);
            let runs = vec![Run {
                text,
                style_id: StyleId::Semantic(style),
                width: w,
                aux: aux.clone(),
            }];
            let runs = inline::clip_runs(runs, cw, true, self.engine);
            self.emit_runs(runs);
        }
    }
}

/// Walk sibling blocks; with `separate`, one blank line goes between them.
///
/// At the top level (`depth == 1`) each block sets `current_block`, so every
/// line it emits — including lines from nested blockquotes/lists — is tagged
/// with that top-level block for scroll anchoring. Nested calls run deeper
/// and leave the tag alone.
pub(crate) fn walk_blocks(ctx: &mut Ctx<'_>, blocks: &[Block], separate: bool) {
    ctx.depth += 1;
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 && separate {
            ctx.blank_line();
        }
        if ctx.depth == 1 {
            ctx.current_block = Some(block.id);
        }
        walk_block(ctx, block);
    }
    ctx.depth -= 1;
}

fn walk_block(ctx: &mut Ctx<'_>, block: &Block) {
    match &block.kind {
        BlockKind::Paragraph { children } => ctx.flow(children, Semantic::Text),
        BlockKind::Heading { level, children } => ctx.heading(*level, children, block.id),
        BlockKind::ThematicBreak => {
            // `─` is East Asian *Ambiguous*: under
            // `WidthConfig::ambiguous_wide` it is 2 cells, and
            // `dash.repeat(cw / dw)` then under-fills every odd content width
            // by a cell. Fill to an exact cell count instead, exactly as the
            // table header rule does.
            let cw = ctx.content_width();
            let dash = "\u{2500}"; // ─
            let dw = ctx.engine.cluster_width(dash).max(1);
            let mut runs: Vec<Run> = Vec::new();
            crate::table::push_rule_cells(
                &mut runs,
                dash,
                dw,
                cw,
                StyleId::Semantic(Semantic::Rule),
            );
            ctx.emit_runs(runs);
        }
        BlockKind::BlockQuote { children } => {
            ctx.push_prefix(
                QUOTE_BAR.into(),
                QUOTE_BAR.into(),
                Semantic::BlockquoteMarker,
            );
            walk_container(ctx, children, true);
            ctx.pop_prefix();
        }
        BlockKind::Alert { kind, children } => {
            ctx.push_prefix(
                QUOTE_BAR.into(),
                QUOTE_BAR.into(),
                Semantic::BlockquoteMarker,
            );
            let (label, tone) = alert_label(*kind);
            let w = ctx.engine.display_width(label) as u16;
            ctx.emit_runs(vec![Run {
                text: label.to_string(),
                style_id: StyleId::Semantic(Semantic::AlertTitle(tone)),
                width: w,
                aux: None,
            }]);
            if !children.is_empty() {
                ctx.blank_line();
                walk_blocks(ctx, children, true);
            }
            ctx.pop_prefix();
        }
        BlockKind::List { kind, tight, items } => {
            for (i, item) in items.iter().enumerate() {
                if i > 0 && !tight {
                    ctx.blank_line();
                }
                walk_list_item(ctx, kind, i, item, *tight);
            }
        }
        // A ListItem outside a List cannot come out of the parser; lay its
        // children out rather than silently dropping content if it ever does.
        BlockKind::ListItem { children, .. } => walk_blocks(ctx, children, true),
        BlockKind::CodeBlock { literal, info, .. } => {
            let lang = info
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                // The info string is "lang extra words"; only the first token
                // is the language tag (CommonMark 0.31.2 §4.5).
                .and_then(|s| s.split_whitespace().next())
                .map(|s| s.to_owned().into_boxed_str());
            ctx.literal_block(literal, Semantic::CodeBlock, lang);
        }
        BlockKind::HtmlBlock { literal } => ctx.literal_block(literal, Semantic::Html, None),
        BlockKind::FrontMatter { literal } => {
            ctx.literal_block(literal, Semantic::FrontMatter, None)
        }
        BlockKind::Table { alignments, rows } => crate::table::layout_table(ctx, alignments, rows),
        // Rows/cells only occur under Table and are handled there.
        BlockKind::TableRow { .. } | BlockKind::TableCell { .. } => {}
        BlockKind::FootnoteDefinition { label, children } => {
            let marker = format!("[^{label}] ");
            let mw = ctx.engine.display_width(&marker);
            ctx.push_prefix(marker, " ".repeat(mw), Semantic::FootnoteLabel);
            walk_container(ctx, children, true);
            ctx.pop_prefix();
        }
    }
}

/// Lay out a container's children under the prefix its caller has just pushed.
///
/// An empty container still occupies one row. [`Ctx::prefix_runs`] is the only
/// thing that consumes a prefix segment, so `walk_blocks` over an empty slice
/// emits nothing at all and `pop_prefix` then drops the unused marker — the
/// container disappears from the document, taking its gutter bar, its list
/// number or its footnote label with it. A reader then has no way to tell an
/// empty container from a missing one: `1.` / `2.` / `3.` with an empty second
/// item rendered as "1." then "3.", and a bare `>` rendered as nothing at all.
/// Mirrors [`Ctx::emit_pieces`]'s "an empty block (e.g. `# `) still occupies
/// one line".
fn walk_container(ctx: &mut Ctx<'_>, children: &[Block], separate: bool) {
    if children.is_empty() {
        ctx.blank_line();
    }
    walk_blocks(ctx, children, separate);
}

fn walk_list_item(ctx: &mut Ctx<'_>, kind: &ListKind, index: usize, item: &Block, tight: bool) {
    let BlockKind::ListItem { checked, children } = &item.kind else {
        // The parser guarantees list children are items; degrade gracefully.
        walk_block(ctx, item);
        return;
    };
    let (marker, style) = match checked {
        Some(true) => ("[x] ".to_string(), Semantic::TaskMarker),
        Some(false) => ("[ ] ".to_string(), Semantic::TaskMarker),
        None => match kind {
            ListKind::Bullet(_) => ("\u{2022} ".to_string(), Semantic::ListMarker), // "• "
            ListKind::Ordered { start, delim } => (
                format!("{}{delim} ", start.saturating_add(index as u64)),
                Semantic::ListMarker,
            ),
        },
    };
    let mw = ctx.engine.display_width(&marker);
    ctx.push_prefix(marker, " ".repeat(mw), style);
    // CommonMark tightness: blocks inside a tight item (e.g. a nested list
    // right under its paragraph) sit flush; loose items keep blank rows.
    walk_container(ctx, children, !tight);
    ctx.pop_prefix();
}

fn alert_label(kind: AlertKind) -> (&'static str, AlertTone) {
    match kind {
        AlertKind::Note => ("[!NOTE]", AlertTone::Note),
        AlertKind::Tip => ("[!TIP]", AlertTone::Tip),
        AlertKind::Important => ("[!IMPORTANT]", AlertTone::Important),
        AlertKind::Warning => ("[!WARNING]", AlertTone::Warning),
        AlertKind::Caution => ("[!CAUTION]", AlertTone::Caution),
    }
}

/// A heading's text for the outline: its inline children flattened to plain
/// text, with runs of whitespace collapsed so a soft-wrapped source heading
/// reads as one line in the TOC.
///
/// Deliberately taken from the AST rather than from the runs the flow just
/// emitted. A run's style is *overridden* inside emphasis, code spans and
/// links (`Semantic::Emph`, `CodeInline`, `Link`), so collecting only the
/// `Semantic::Heading` runs would silently drop those words, while collecting
/// every run on the line would pick up a blockquote's gutter bar and a list
/// item's marker for a nested heading.
///
/// Iterative rather than recursive, matching `ast`'s own plain-text
/// rendering: heading inlines are attacker-influenced input in a viewer that
/// opens arbitrary markdown, and a deeply nested `***...***` must cost stack
/// depth nothing.
fn heading_text(children: &[ast::Inline]) -> String {
    use ast::InlineKind;

    let mut out = String::new();
    let mut work: Vec<&ast::Inline> = children.iter().rev().collect();
    while let Some(inline) = work.pop() {
        match &inline.kind {
            InlineKind::Text(t) | InlineKind::Code(t) => out.push_str(t),
            InlineKind::Math { tex, .. } => out.push_str(tex),
            InlineKind::FootnoteReference { label } => out.push_str(label),
            InlineKind::SoftBreak | InlineKind::HardBreak => out.push(' '),
            InlineKind::HtmlInline(_) => {}
            InlineKind::Emph(c)
            | InlineKind::Strong(c)
            | InlineKind::Strikethrough(c)
            | InlineKind::Link { children: c, .. }
            | InlineKind::Image { children: c, .. } => work.extend(c.iter().rev()),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}
