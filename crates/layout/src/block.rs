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
    AlertTone, CellSize, FoldState, IntrinsicSizer, Line, LineItem, Outline, OutlineEntry,
    Reserved, ReservedLine, Run, Semantic, StyleId,
};

/// Minimum content columns the prefix may never consume (clamped to the
/// layout width itself at pathological widths).
const MIN_CONTENT_COLS: u16 = 8;

/// Gutter bar for blockquotes and alerts.
const QUOTE_BAR: &str = "\u{2502} "; // "│ "

/// One unit of a heading's depth marker: LEFT HALF BLOCK. Half-width ink in
/// a full cell, so consecutive markers read as separated bars without
/// spending a cell on the gap — six of them cost six columns, where six
/// space-separated full blocks would cost eleven.
const HEADING_MARKER: &str = "\u{258c}"; // "▌"

/// The ember rule drawn under a top-level heading.
const HEADING_RULE: &str = "\u{2500}"; // "─"

/// Deepest heading level that gets an ember rule under it. Below this the
/// marker count and the case transform carry the level on their own, and a
/// rule per heading would stripe the page.
const HEADING_RULE_MAX_LEVEL: u8 = 2;

/// Deepest heading level whose line is extended to the full measure so a
/// themed background reads as a band. Only the document's title earns it.
const HEADING_WASH_MAX_LEVEL: u8 = 1;

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
    /// Which top-level headings are currently folded (Phase 5). Consulted only at
    /// `depth == 1` — see [`walk_blocks`].
    folds: &'a FoldState,
}

impl<'a> Ctx<'a> {
    pub fn new(
        doc: &'a Document,
        engine: &'a WidthEngine,
        sizer: &'a dyn IntrinsicSizer,
        width: u16,
        folds: &'a FoldState,
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
            folds,
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
        let pushed = self.push_heading_marker(level);
        self.flow(children, Semantic::Heading(level));
        for _ in 0..pushed {
            self.pop_prefix();
        }
        if self.lines.len() > line && level <= HEADING_WASH_MAX_LEVEL {
            self.wash_heading_lines(line, level);
        }
        if self.lines.len() > line && level <= HEADING_RULE_MAX_LEVEL {
            self.emit_heading_rule(level);
        }
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

    /// If the top-level block at `index` is a folded heading, the exclusive
    /// end index of the section it collapses: every following top-level
    /// block up to, but not including, the next heading whose level is `<=`
    /// this one's — "a section runs to the next heading of equal or
    /// shallower level" (Phase 5's approach note). `blocks.len()` when no
    /// such heading follows (folding the last heading in the document).
    /// `None` when the block is not a heading, or is a heading that is not
    /// folded.
    fn fold_range(&self, blocks: &[Block], index: usize) -> Option<usize> {
        let BlockKind::Heading { level, .. } = &blocks[index].kind else {
            return None;
        };
        if !self.folds.is_folded(blocks[index].id) {
            return None;
        }
        let end = blocks[index + 1..]
            .iter()
            .position(|b| matches!(&b.kind, BlockKind::Heading { level: lvl, .. } if lvl <= level))
            .map_or(blocks.len(), |rel| index + 1 + rel);
        Some(end)
    }

    /// Collapses the folded heading at `blocks[index]` and its section
    /// (`blocks[index + 1..end]`) to exactly one line (DW-5.1): a marker
    /// carrying the heading's own flattened text and how many lines are
    /// hidden right now. "Right now" recursively respects a fold nested
    /// inside the section (via [`Ctx::count_lines`]), so unfolding this
    /// heading alone reveals exactly what is really underneath — which may
    /// be another marker, not the fully expanded section.
    ///
    /// The outline still gets an entry, exactly as an open heading would:
    /// `]]`/`[[` and the TOC keep working on a folded heading, landing on the
    /// marker line instead of the heading's own first line.
    fn emit_fold_marker(&mut self, blocks: &[Block], index: usize, end: usize) {
        let BlockKind::Heading { level, children } = &blocks[index].kind else {
            unreachable!("fold_range only returns Some for a Heading block");
        };
        let (level, title) = (*level, heading_text(children));
        let hidden = self.count_lines(&blocks[index..end]).saturating_sub(1);
        // Pushed before the width is measured, so the marker is paid for out
        // of the fold marker's own budget rather than overrunning the
        // measure — a folded H6 spends six columns on blocks like any other.
        //
        // ...unless paying for it would cost the hidden count. DW-5.1 says
        // the count is shown whole or the marker line is lying about how much
        // it hides, and at a narrow width the depth blocks are exactly what
        // pushes `(15 hidden lines)` over the edge. Depth is decoration here
        // (the fold marker's own glyph already says "heading"); the count is
        // information. So the blocks yield, matching what `prefix_budget`
        // already does for container prefixes.
        let mut pushed = self.push_heading_marker(level);
        if self.content_width() < fold_marker_min_width(self.engine, hidden) {
            for _ in 0..pushed {
                self.pop_prefix();
            }
            pushed = 0;
        }
        let text = fold_marker_text(self.engine, &title, hidden, self.content_width());
        let width = inline::cells(self.engine, &text);
        let run = Run {
            text,
            style_id: StyleId::Semantic(Semantic::Heading(level)),
            width,
            aux: None,
        };
        let runs = inline::clip_runs(vec![run], self.content_width(), true, self.engine);
        let line = self.lines.len();
        self.emit_runs(runs);
        for _ in 0..pushed {
            self.pop_prefix();
        }
        self.outline.push(
            OutlineEntry {
                level,
                text: title,
                block: blocks[index].id,
            },
            line,
        );
    }

    /// How many lines `blocks` would occupy if *its own* heading (always
    /// `blocks[0]`, the one [`Ctx::emit_fold_marker`] is measuring) were
    /// unfolded, while every *other* fold — including one nested inside this
    /// section — stays exactly as it is. A dry run through the same walk on
    /// a scratch [`Ctx`], discarded once its line count is read.
    ///
    /// Excluding just `blocks[0]` from the scratch fold set, rather than
    /// reusing `self.folds` verbatim, is load-bearing and not an
    /// optimization: `blocks[0]` is folded (that is why this is being
    /// called at all), so a scratch walk that still considered it folded
    /// would immediately re-collapse the identical slice it was just handed
    /// and call itself again — the same one-line answer forever, which is a
    /// stack overflow, not a wrong number.
    fn count_lines(&self, blocks: &[Block]) -> usize {
        let mut open = self.folds.clone();
        open.collapsed.remove(&blocks[0].id);
        let mut scratch = Ctx::new(self.doc, self.engine, self.sizer, self.width, &open);
        walk_blocks(&mut scratch, blocks, true);
        scratch.lines.len()
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

    /// Extend a top-level heading's own line(s) to the full measure with
    /// blank cells carrying that heading's style, so the theme's background
    /// for the level reads as a band across the page rather than stopping
    /// where the words do.
    ///
    /// Padding here rather than at paint time is what keeps the wash honest
    /// about width: layout is the only place that knows the measure *and*
    /// the container prefix already consumed part of it. The pad is styled
    /// `Heading(level)` like the text it follows, so a variant with no
    /// background simply paints nothing and this costs a few space runs.
    ///
    /// Reserved (media) lines are left alone — a heading cannot produce one,
    /// and padding a media row would fight the box's own geometry.
    fn wash_heading_lines(&mut self, from: usize, level: u8) {
        let measure = self.width;
        for idx in from..self.lines.len() {
            let Some(Line::Items(items)) = self.lines.get_mut(idx) else {
                continue;
            };
            let used: u16 = items
                .iter()
                .fold(0u16, |acc, it| acc.saturating_add(it.width()));
            let Some(pad) = measure.checked_sub(used).filter(|p| *p > 0) else {
                continue;
            };
            items.push(LineItem::Run(Run {
                text: " ".repeat(usize::from(pad)),
                style_id: StyleId::Semantic(Semantic::Heading(level)),
                width: pad,
                aux: None,
            }));
        }
    }

    /// Emit the rule that sits under a top-level heading — an ember: bright
    /// at the margin, dimming across the measure.
    ///
    /// The gradient is banded across `Heading(level..=6)` rather than given
    /// its own colours. That is deliberate, and it is the same trick the
    /// depth marker uses: the theme's ramp already holds six rungs of one
    /// hue, so indexing it by *position along the rule* buys a fade for no
    /// new palette roles, and ties the rule's bright end to the rung its own
    /// heading is painted in. H1 gets six bands, H2 five, so the rule is
    /// visibly shorter-lived under the shallower heading.
    ///
    /// Emitted after the marker prefix is popped, so it spans the full
    /// measure rather than starting under the heading's text.
    ///
    /// This line does not exist in the document. It is counted like any other
    /// emitted line because `count_lines` re-runs this same walk, so fold
    /// counts, scroll anchoring and the outline all stay consistent without
    /// knowing the rule is special.
    fn emit_heading_rule(&mut self, level: u8) {
        let width = self.content_width();
        if width == 0 {
            return;
        }
        let rungs: Vec<u8> = (level.clamp(1, 6)..=6).collect();
        let bands = u16::try_from(rungs.len()).unwrap_or(1);
        let mut runs: Vec<Run> = Vec::with_capacity(rungs.len());
        let mut filled = 0u16;
        for (i, &rung) in rungs.iter().enumerate() {
            // Cumulative division rather than `width / bands` per band, so
            // rounding loss cannot leave the rule short of the measure.
            let upto = u16::try_from(
                (u32::from(i as u16 + 1) * u32::from(width)) / u32::from(bands),
            )
            .unwrap_or(width);
            let cells = upto.saturating_sub(filled);
            filled = upto;
            if cells == 0 {
                continue;
            }
            let text = HEADING_RULE.repeat(usize::from(cells));
            let measured = inline::cells(self.engine, &text);
            runs.push(Run {
                text,
                style_id: StyleId::Semantic(Semantic::HeadingRung(rung)),
                width: measured,
                aux: None,
            });
        }
        let runs = inline::clip_runs(runs, width, false, self.engine);
        self.emit_runs(runs);
    }

    /// Push the heading depth marker: one half-block per level, then a space.
    /// Returns how many segments were pushed, for the matching pops.
    ///
    /// The count is the signal — a reader counts blocks rather than learning
    /// what a colour means — so this is *content*, not styling, and it has to
    /// be a prefix rather than a decoration bolted on at paint time. Being a
    /// prefix buys three things for free: `content_width` shrinks by the
    /// marker so a heading still cannot overrun the measure, `rest` indents
    /// wrapped heading lines to hang under the text instead of under the
    /// blocks, and `prefix_budget`'s clamp drops the marker rather than
    /// squeezing the text to nothing at a pathological width.
    ///
    /// Each block is its own segment styled `Heading(1..=level)` — not all
    /// `Heading(level)`. The theme's ramp has exactly one rung per level, so
    /// indexing it by *position in the run* makes every marker start on the
    /// brightest rung and fade toward the page as it lengthens, which is the
    /// run fade, at the cost of no new palette roles.
    ///
    /// This never reaches `heading_text`, so the outline, the TOC and fold
    /// markers keep the author's text with no blocks in it.
    fn push_heading_marker(&mut self, level: u8) -> usize {
        let level = level.clamp(1, 6);
        // A washed heading's markers sit *inside* its band, so they take the
        // full `Heading` style — background included — or the band opens with
        // a hole where the blocks are. Every other level's markers want the
        // rung's color alone; `Heading(1)` would hand them H1's background.
        let washed = level <= HEADING_WASH_MAX_LEVEL;
        for rung in 1..=level {
            let semantic = if washed {
                Semantic::Heading(rung)
            } else {
                Semantic::HeadingRung(rung)
            };
            self.push_prefix(HEADING_MARKER.to_string(), " ".to_string(), semantic);
        }
        self.push_prefix(" ".to_string(), " ".to_string(), Semantic::Heading(level));
        usize::from(level) + 1
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
    ///
    /// Headings pass through [`inline::recase`] between the two, so their
    /// level's case transform is applied to the text that then gets measured
    /// and wrapped — not bolted on afterwards, where a width-changing
    /// uppercase could overrun the measure.
    fn flow(&mut self, inlines: &[ast::Inline], base: Semantic) {
        let mut atoms = inline::flatten(inlines, base, self.doc, self.sizer, true);
        if let Semantic::Heading(level) = base {
            inline::recase(&mut atoms, crate::heading_case(level));
        }
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
///
/// Folding (Phase 5) is consulted only at `depth == 1`, for the same reason
/// `current_block` is: a heading nested inside a blockquote or list item
/// tags no line of its own (see [`crate::OutlineEntry::block`]), so it is not
/// itself foldable, and a fold range is meaningful only among *this*
/// document's top-level blocks. An index-based loop, rather than `enumerate`,
/// is what lets a folded heading's whole section be skipped in one step
/// instead of visited and then filtered.
pub(crate) fn walk_blocks(ctx: &mut Ctx<'_>, blocks: &[Block], separate: bool) {
    ctx.depth += 1;
    let mut index = 0;
    while index < blocks.len() {
        if index > 0 && separate {
            ctx.blank_line();
        }
        if ctx.depth == 1 {
            ctx.current_block = Some(blocks[index].id);
            if let Some(end) = ctx.fold_range(blocks, index) {
                ctx.emit_fold_marker(blocks, index, end);
                index = end;
                continue;
            }
        }
        walk_block(ctx, &blocks[index]);
        index += 1;
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

/// The exact text of a folded section's one visible line (DW-5.1): a marker
/// glyph, the heading's own flattened title, and how many lines it is
/// currently hiding. The heading's *level* is not encoded here — it already
/// reaches the painter as the run's `Semantic::Heading(level)` style, which
/// is where a level belongs; restating it in the text would be a second,
/// driftable source of the same fact.
///
/// **Budgeted so the count survives being narrow, not the title.** Building
/// `"{glyph} {title} ({count})"` and clipping the assembled string
/// right-to-left (the naive approach, and the one this replaced) sacrifices
/// whichever text is last — which for that order is always the count, the
/// one piece of information a fold marker adds over the heading's own text.
/// Measured against a real heading title, that lost the count at three of
/// four ordinary layout widths (20/40/60 of `LayoutConfig::default()`'s
/// 24-100 range), and at width 60 it truncated to the actively misleading
/// `(6 h…` — a count that *looks* rendered with its unit eaten. So the
/// suffix (glyph + count) is measured first and the title gets whatever
/// width is left over, itself clipped with the usual `…` indicator via
/// [`clipped_text`] — never the assembled whole.
/// The narrowest measure at which [`fold_marker_text`] can still show the
/// glyph and the whole hidden count, with nothing left over for the title.
/// Below this, something has to give, and it must not be the count.
fn fold_marker_min_width(engine: &WidthEngine, hidden: usize) -> u16 {
    let noun = if hidden == 1 { "line" } else { "lines" };
    inline::cells(engine, FOLD_GLYPH)
        .saturating_add(inline::cells(engine, &format!(" ({hidden} hidden {noun})")))
}

/// Leading glyph on a folded heading's marker line.
const FOLD_GLYPH: &str = "\u{25b8} "; // "▸ "

fn fold_marker_text(engine: &WidthEngine, title: &str, hidden: usize, width: u16) -> String {
    const GLYPH: &str = FOLD_GLYPH;
    let noun = if hidden == 1 { "line" } else { "lines" };
    let suffix = format!(" ({hidden} hidden {noun})");
    let glyph_w = inline::cells(engine, GLYPH);
    let suffix_w = inline::cells(engine, &suffix);
    // Room left for the title once the glyph and the count are paid for.
    // Saturates to 0 at a width so narrow that even "glyph + count" alone
    // does not fit; `emit_fold_marker`'s own `clip_runs` pass over the
    // assembled result is the safety net for that pathological case
    // (DW-5.6 still holds either way — it is only the count's survival that
    // this function exists to protect).
    let title_budget = width.saturating_sub(glyph_w).saturating_sub(suffix_w);
    let clipped_title = clipped_text(engine, title, title_budget);
    format!("{GLYPH}{clipped_title}{suffix}")
}

/// `text` clipped to `width` cells with the usual `…` overflow indicator
/// when it does not fit whole — reusing [`inline::clip_runs`] rather than a
/// second truncation loop, so grapheme-boundary correctness stays in one
/// place. `width == 0` returns empty without asking `clip_runs` to do
/// nothing the hard way.
fn clipped_text(engine: &WidthEngine, text: &str, width: u16) -> String {
    if width == 0 {
        return String::new();
    }
    let run = Run {
        text: text.to_string(),
        style_id: StyleId::Semantic(Semantic::Text),
        width: inline::cells(engine, text),
        aux: None,
    };
    inline::clip_runs(vec![run], width, true, engine)
        .into_iter()
        .map(|r| r.text)
        .collect()
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
