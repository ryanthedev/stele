//! Phase 1: block structure.
//!
//! Pseudocode (the spec's appendix strategy, mirroring cmark's blocks.c):
//!
//! ```text
//! for each line:
//!     match the continuation prefix of every open block, outermost first
//!         (blockquote '>', list-item indent, code-fence close, html end,
//!          paragraph non-blank, footnote indent, table row,
//!          grid-table extent)
//!     unless the deepest matched leaf consumes text lines outright,
//!         repeatedly try to open new blocks at the current position
//!         (blockquote, ATX, fence, html, setext, thematic break,
//!          footnote definition, list item, grid table, indented code,
//!          GFM table)
//!         closing unmatched blocks the moment the first new one opens
//!     add remaining text to the tip:
//!         lazy paragraph continuation if nothing matched or opened,
//!         otherwise close unmatched blocks and append to the open leaf
//! at EOF, close everything
//! ```
//!
//! Columns are tracked with 4-column tab stops, including partial tab
//! consumption; every content push records its source byte range so inline
//! spans survive into the original text.

use super::scan;
use super::{Leaf, MAX_CONTAINER_DEPTH, ParseOptions, PreBlock, PreKind, PreRow, RefMap, grid};
use crate::ast::{AlertKind, Alignment, ListKind, Span};

const TAB_STOP: usize = 4;

/// List-item geometry: marker indent and content padding, in columns.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ListData {
    kind: ListKind,
    marker_offset: usize,
    padding: usize,
}

fn lists_match(a: ListKind, b: ListKind) -> bool {
    match (a, b) {
        (ListKind::Bullet(x), ListKind::Bullet(y)) => x == y,
        (ListKind::Ordered { delim: x, .. }, ListKind::Ordered { delim: y, .. }) => x == y,
        _ => false,
    }
}

#[derive(Debug)]
enum OpenKind {
    Document,
    BlockQuote,
    List(ListData),
    Item(ListData),
    Paragraph,
    /// ATX or setext heading (setext = converted paragraph).
    Heading {
        level: u8,
    },
    ThematicBreak,
    IndentedCode,
    FencedCode {
        ch: u8,
        len: usize,
        indent: usize,
        info: Option<String>,
        closing_seen: bool,
    },
    HtmlBlock {
        html_type: u8,
    },
    Table {
        alignments: Vec<Alignment>,
        rows: Vec<PreRow>,
    },
    /// A Pandoc grid table, already parsed in full by [`grid::scan`]. The
    /// block only stays open so the lines it covers are consumed rather
    /// than re-offered to the other openers; `end` is the byte just past
    /// its closing rule line.
    GridTable {
        end: usize,
        alignments: Vec<Alignment>,
        rows: Vec<PreRow>,
    },
    FootnoteDef {
        label: String,
    },
}

struct Open {
    kind: OpenKind,
    span_start: usize,
    span_end: usize,
    children: Vec<PreBlock>,
    leaf: Leaf,
    /// For list tightness: did the last line inside this block end blank?
    last_line_blank: bool,
    start_line: usize,
}

impl Open {
    fn new(kind: OpenKind, span_start: usize, line_no: usize) -> Open {
        Open {
            kind,
            span_start,
            span_end: span_start,
            children: Vec::new(),
            leaf: Leaf::default(),
            last_line_blank: false,
            start_line: line_no,
        }
    }
}

pub(crate) fn parse_blocks(
    src: &str,
    opts: &ParseOptions,
) -> (Vec<PreBlock>, RefMap, std::collections::HashSet<String>) {
    let mut p = BlockParser::new(src, opts);
    p.run();
    let doc = p.open.remove(0);
    (doc.children, p.refmap, p.footnote_labels)
}

struct BlockParser<'s> {
    src: &'s str,
    opts: &'s ParseOptions,
    open: Vec<Open>,
    refmap: RefMap,
    footnote_labels: std::collections::HashSet<String>,
    line_no: usize,
    // Per-line state (byte offsets are within `line`; columns are virtual).
    line_start: usize,
    line: &'s str,
    offset: usize,
    column: usize,
    first_nonspace: usize,
    first_nonspace_col: usize,
    indent: usize,
    blank: bool,
    partially_consumed_tab: bool,
}

impl<'s> BlockParser<'s> {
    fn new(src: &'s str, opts: &'s ParseOptions) -> BlockParser<'s> {
        BlockParser {
            src,
            opts,
            open: vec![Open::new(OpenKind::Document, 0, 0)],
            refmap: RefMap::new(),
            footnote_labels: std::collections::HashSet::new(),
            line_no: 0,
            line_start: 0,
            line: "",
            offset: 0,
            column: 0,
            first_nonspace: 0,
            first_nonspace_col: 0,
            indent: 0,
            blank: true,
            partially_consumed_tab: false,
        }
    }

    fn run(&mut self) {
        let mut pos = self.scan_frontmatter();
        let bytes = self.src.as_bytes();
        while pos < bytes.len() {
            let line_end = bytes[pos..]
                .iter()
                .position(|&c| c == b'\n' || c == b'\r')
                .map(|i| pos + i)
                .unwrap_or(bytes.len());
            let term = if bytes.get(line_end) == Some(&b'\r') {
                if bytes.get(line_end + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                }
            } else if bytes.get(line_end) == Some(&b'\n') {
                1
            } else {
                0
            };
            self.process_line(pos, line_end);
            pos = line_end + term;
        }
        // EOF: close everything down to the document.
        while self.open.len() > 1 {
            self.finalize_tip();
        }
    }

    /// YAML frontmatter: only at byte 0, `---` alone on the first line,
    /// closed by `---` or `...` alone on a line. If unclosed, it is not
    /// frontmatter at all.
    fn scan_frontmatter(&mut self) -> usize {
        let src = self.src;
        if !self.opts.frontmatter || !(src.starts_with("---\n") || src.starts_with("---\r")) {
            return 0;
        }
        let mut pos = 3 + if src.as_bytes()[3] == b'\r' && src.as_bytes().get(4) == Some(&b'\n') {
            2
        } else {
            1
        };
        let body_start = pos;
        while pos < src.len() {
            let line_end = src[pos..]
                .find(['\n', '\r'])
                .map(|i| pos + i)
                .unwrap_or(src.len());
            let line = &src[pos..line_end];
            let term = if src.as_bytes().get(line_end) == Some(&b'\r') {
                if src.as_bytes().get(line_end + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                }
            } else if line_end < src.len() {
                1
            } else {
                0
            };
            if line.trim_end() == "---" || line.trim_end() == "..." {
                let literal = src[body_start..pos].to_owned();
                let end = line_end + term;
                self.open[0].children.push(PreBlock {
                    span: Span::new(0, line_end),
                    kind: PreKind::FrontMatter { literal },
                    last_line_blank: false,
                });
                return end;
            }
            pos = line_end + term;
        }
        0
    }

    // --- column/offset machinery (ports of cmark's S_advance_offset etc.) ---

    fn peek(&self, offset: usize) -> Option<u8> {
        self.line.as_bytes().get(offset).copied()
    }

    fn advance_offset(&mut self, mut count: usize, columns: bool) {
        let bytes = self.line.as_bytes();
        while count > 0 {
            let Some(&c) = bytes.get(self.offset) else {
                break;
            };
            if c == b'\t' {
                let chars_to_tab = TAB_STOP - (self.column % TAB_STOP);
                if columns {
                    self.partially_consumed_tab = chars_to_tab > count;
                    let advance = chars_to_tab.min(count);
                    self.column += advance;
                    if !self.partially_consumed_tab {
                        self.offset += 1;
                    }
                    count -= advance;
                } else {
                    self.partially_consumed_tab = false;
                    self.column += chars_to_tab;
                    self.offset += 1;
                    count -= 1;
                }
            } else {
                self.partially_consumed_tab = false;
                self.offset += 1;
                self.column += 1;
                count -= 1;
            }
        }
    }

    fn find_first_nonspace(&mut self) {
        let bytes = self.line.as_bytes();
        let mut i = self.offset;
        let mut col = self.column;
        loop {
            match bytes.get(i) {
                Some(b' ') => {
                    i += 1;
                    col += 1;
                }
                Some(b'\t') => {
                    col += TAB_STOP - (col % TAB_STOP);
                    i += 1;
                }
                _ => break,
            }
        }
        self.first_nonspace = i;
        self.first_nonspace_col = col;
        self.blank = i >= bytes.len();
        self.indent = self.first_nonspace_col - self.column;
    }

    fn rest_is_blank_after(&self, offset: usize) -> bool {
        self.line.as_bytes()[offset..]
            .iter()
            .all(|&c| c == b' ' || c == b'\t')
    }

    /// Append the remainder of the current line (from `offset`) to the open
    /// leaf, materializing a partially consumed tab as spaces.
    fn add_line(&mut self) {
        let tip = self.open.len() - 1;
        let line_off = self.line_start;
        if self.partially_consumed_tab {
            let chars_to_tab = TAB_STOP - (self.column % TAB_STOP);
            let tab_pos = line_off + self.offset;
            let spaces = " ".repeat(chars_to_tab);
            self.open[tip].leaf.push(&spaces, tab_pos, 1);
            self.offset += 1;
            self.partially_consumed_tab = false;
        }
        let start = self.offset;
        let text = &self.line[start..];
        self.open[tip].leaf.push(text, line_off + start, text.len());
        // Terminator normalized to '\n' (mapped to the terminator bytes).
        let term_pos = line_off + self.line.len();
        let term_len = usize::from(self.src.as_bytes().get(term_pos).is_some());
        self.open[tip].leaf.push("\n", term_pos, term_len);
    }

    // --- main line loop ---

    fn process_line(&mut self, start: usize, end: usize) {
        self.line_no += 1;
        self.line_start = start;
        self.line = &self.src[start..end];
        self.offset = 0;
        self.column = 0;
        self.partially_consumed_tab = false;

        let (last_matched, mut should_add_line) = self.check_open_blocks();
        let tip_before = self.open.len() - 1;
        if should_add_line {
            let opened = self.open_new_blocks(last_matched);
            self.add_text(last_matched, tip_before, opened);
        } else {
            // A closing code fence consumed the line: finalize that block.
            self.update_span_ends(self.open.len() - 1);
            while self.open.len() - 1 > last_matched {
                self.finalize_tip();
            }
            self.finalize_tip();
            should_add_line = false;
        }
        let _ = should_add_line;
    }

    /// Match continuation prefixes of open blocks, outermost first.
    /// Returns (index of last matched open block, whether the line still
    /// needs text/starts processing — false when a closing fence ate it).
    fn check_open_blocks(&mut self) -> (usize, bool) {
        let mut i = 1;
        while i < self.open.len() {
            self.find_first_nonspace();
            let matched = match &self.open[i].kind {
                OpenKind::Document => true,
                OpenKind::BlockQuote => {
                    if self.indent <= 3 && self.peek(self.first_nonspace) == Some(b'>') {
                        self.advance_offset(self.first_nonspace + 1 - self.offset, false);
                        if matches!(self.peek(self.offset), Some(b' ') | Some(b'\t')) {
                            self.advance_offset(1, true);
                        }
                        true
                    } else {
                        false
                    }
                }
                OpenKind::List(_) => true,
                OpenKind::Item(data) => {
                    let data = *data;
                    let has_content = !self.open[i].children.is_empty() || i + 1 < self.open.len();
                    if self.indent >= data.marker_offset + data.padding {
                        self.advance_offset(data.marker_offset + data.padding, true);
                        true
                    } else if self.blank && has_content {
                        self.advance_offset(self.first_nonspace - self.offset, false);
                        true
                    } else {
                        false
                    }
                }
                OpenKind::FootnoteDef { .. } => {
                    if self.indent >= 4 {
                        self.advance_offset(4, true);
                        true
                    } else if self.blank {
                        self.advance_offset(self.first_nonspace - self.offset, false);
                        true
                    } else {
                        false
                    }
                }
                OpenKind::IndentedCode => {
                    if self.indent >= 4 {
                        self.advance_offset(4, true);
                        true
                    } else if self.blank {
                        self.advance_offset(self.first_nonspace - self.offset, false);
                        true
                    } else {
                        false
                    }
                }
                OpenKind::FencedCode {
                    ch, len, indent, ..
                } => {
                    let (ch, len, fence_indent) = (*ch, *len, *indent);
                    // Closing fence?
                    if self.indent <= 3 && self.peek(self.first_nonspace) == Some(ch) {
                        let bytes = self.line.as_bytes();
                        let mut j = self.first_nonspace;
                        while bytes.get(j) == Some(&ch) {
                            j += 1;
                        }
                        if j - self.first_nonspace >= len && self.rest_is_blank_after(j) {
                            // Line consumed; block ends here.
                            if let OpenKind::FencedCode { closing_seen, .. } =
                                &mut self.open[i].kind
                            {
                                *closing_seen = true;
                            }
                            self.offset = self.line.len();
                            return (i, false);
                        }
                    }
                    // Skip up to fence_indent columns of spaces.
                    let mut remaining = fence_indent;
                    while remaining > 0
                        && matches!(self.peek(self.offset), Some(b' ') | Some(b'\t'))
                    {
                        self.advance_offset(1, true);
                        remaining -= 1;
                    }
                    true
                }
                OpenKind::HtmlBlock { html_type, .. } => match html_type {
                    6 | 7 => !self.blank,
                    _ => true,
                },
                OpenKind::Paragraph => !self.blank,
                OpenKind::Table { .. } => !self.blank && !self.breaks_table(),
                // The grid table's extent was fixed when it was opened, so
                // continuation is pure arithmetic: no line inside it can
                // start or break anything, because every one of them was
                // already validated as a rule or a cell row.
                OpenKind::GridTable { end, .. } => self.line_start < *end,
                OpenKind::Heading { .. } | OpenKind::ThematicBreak => false,
            };
            if !matched {
                return (i - 1, true);
            }
            i += 1;
        }
        (self.open.len() - 1, true)
    }

    /// Would the current line (at first_nonspace) start a block that breaks
    /// an open GFM table?
    fn breaks_table(&self) -> bool {
        if self.indent > 3 {
            return false;
        }
        let rest = &self.line[self.first_nonspace..];
        let b = rest.as_bytes();
        match b.first() {
            Some(b'>') => true,
            Some(b'#') => {
                let n = b.iter().take_while(|&&c| c == b'#').count();
                n <= 6 && matches!(b.get(n), None | Some(b' ') | Some(b'\t'))
            }
            Some(b'`') | Some(b'~') => {
                let c = b[0];
                b.iter().take_while(|&&x| x == c).count() >= 3
            }
            Some(b'-') | Some(b'_') | Some(b'*') => scan::is_thematic_break(rest),
            _ => false,
        }
    }

    fn container_depth(&self) -> usize {
        self.open.len()
    }

    /// Try to open new blocks at the current position (port of cmark's
    /// open_new_blocks). Closes unmatched blocks the moment the first new
    /// block opens. Returns whether any block was opened.
    fn open_new_blocks(&mut self, last_matched: usize) -> bool {
        let mut container = last_matched;
        let mut opened = false;
        // cmark: maybe_lazy = the tip (last text receiver) is a paragraph.
        let mut maybe_lazy = matches!(self.open.last().map(|o| &o.kind), Some(OpenKind::Paragraph));

        loop {
            match self.open[container].kind {
                OpenKind::FencedCode { .. }
                | OpenKind::IndentedCode
                | OpenKind::HtmlBlock { .. }
                | OpenKind::Table { .. }
                | OpenKind::GridTable { .. } => break,
                _ => {}
            }
            let is_paragraph = matches!(self.open[container].kind, OpenKind::Paragraph);
            let is_list = matches!(self.open[container].kind, OpenKind::List(_));
            self.find_first_nonspace();
            let indented = self.indent >= 4;
            let fns = self.first_nonspace;
            let rest = &self.line[fns..];

            if !indented && self.peek(fns) == Some(b'>') {
                if self.container_depth() >= MAX_CONTAINER_DEPTH {
                    break;
                }
                let start = self.line_start + fns;
                self.advance_offset(fns + 1 - self.offset, false);
                if matches!(self.peek(self.offset), Some(b' ') | Some(b'\t')) {
                    self.advance_offset(1, true);
                }
                self.close_down_to(container, &mut opened);
                container = self.open_child(container, OpenKind::BlockQuote, start);
                opened = true;
            } else if !indented && let Some((level, content_off)) = scan_atx_heading(rest) {
                let start = self.line_start + fns;
                self.close_down_to(container, &mut opened);
                self.advance_offset(fns + content_off - self.offset, false);
                self.open_child(container, OpenKind::Heading { level }, start);
                opened = true;
                break; // leaf: accepts lines
            } else if !indented && let Some((ch, len, info)) = scan_open_fence(rest) {
                let start = self.line_start + fns;
                let fence_indent = self.indent;
                self.close_down_to(container, &mut opened);
                self.open_child(
                    container,
                    OpenKind::FencedCode {
                        ch,
                        len,
                        indent: fence_indent,
                        info,
                        closing_seen: false,
                    },
                    start,
                );
                opened = true;
                self.offset = self.line.len(); // rest of the line is the info string
                break;
            } else if !indented
                && let Some(t) = scan_html_block_start(rest, !is_paragraph && !maybe_lazy)
            {
                let start = self.line_start + fns;
                self.close_down_to(container, &mut opened);
                self.open_child(container, OpenKind::HtmlBlock { html_type: t }, start);
                opened = true;
                break;
            } else if !indented
                && is_paragraph
                && let Some(level) = scan_setext_underline(rest)
            {
                // Strip leading reference definitions first; if nothing
                // remains the underline is ordinary paragraph content.
                self.extract_refdefs(container);
                if !self.open[container].leaf.content.trim().is_empty() {
                    self.open[container].kind = OpenKind::Heading { level };
                    self.offset = self.line.len();
                    opened = true;
                }
                break; // paragraph accepts lines either way
            } else if !indented && scan::is_thematic_break(rest) {
                let start = self.line_start + fns;
                self.close_down_to(container, &mut opened);
                self.open_child(container, OpenKind::ThematicBreak, start);
                opened = true;
                self.offset = self.line.len();
                break;
            } else if !indented
                && self.opts.footnotes
                && let Some((label, content_off)) = scan_footnote_def(rest)
            {
                if self.container_depth() >= MAX_CONTAINER_DEPTH {
                    break;
                }
                let start = self.line_start + fns;
                self.close_down_to(container, &mut opened);
                self.advance_offset(fns + content_off - self.offset, false);
                container = self.open_child(container, OpenKind::FootnoteDef { label }, start);
                opened = true;
            } else if self.indent < 4
                && let Some((marker_len, kind)) = parse_list_marker(rest, is_paragraph)
            {
                if self.container_depth() + 2 >= MAX_CONTAINER_DEPTH {
                    break;
                }
                let start = self.line_start + fns;
                let marker_offset = self.indent;
                self.advance_offset(fns + marker_len - self.offset, false);
                let save_offset = self.offset;
                let save_column = self.column;
                let save_partial = self.partially_consumed_tab;
                while self.column - save_column <= 5
                    && matches!(self.peek(self.offset), Some(b' ') | Some(b'\t'))
                {
                    self.advance_offset(1, true);
                }
                let i = self.column - save_column;
                let padding;
                if !(1..5).contains(&i) || self.offset >= self.line.len() {
                    padding = marker_len + 1;
                    self.offset = save_offset;
                    self.column = save_column;
                    self.partially_consumed_tab = save_partial;
                    if i > 0 {
                        self.advance_offset(1, true);
                    }
                } else {
                    padding = marker_len + i;
                }
                let data = ListData {
                    kind,
                    marker_offset,
                    padding,
                };
                self.close_down_to(container, &mut opened);
                let list_matches = is_list
                    && matches!(
                        &self.open[container].kind,
                        OpenKind::List(existing) if lists_match(existing.kind, data.kind)
                    );
                if !list_matches {
                    container = self.open_child(container, OpenKind::List(data), start);
                }
                container = self.open_child(container, OpenKind::Item(data), start);
                opened = true;
            } else if !indented
                && !maybe_lazy
                && !is_paragraph
                && self.opts.grid_tables
                && self.peek(fns) == Some(b'+')
                && let Some(table) = grid::scan(self.src, self.line_start)
            {
                // Pandoc grid table. Three guards earn their keep here:
                //
                // `!indented` — without it this arm would sit in front of
                // the indented-code arm below and steal a `+---+` that a
                // four-space indent had already handed to a code block.
                //
                // `!maybe_lazy` and `!is_paragraph` — a rule line directly
                // under paragraph text stays paragraph text (lazily
                // continued or not). Pandoc wants a blank line before a
                // table, and refusing to interrupt keeps prose that merely
                // happens to contain `+--+` out of the table path.
                //
                // `grid::scan` is handed the whole *physical* line, not the
                // position the container walk reached, because it has to
                // measure the table's own indent and hold every later line
                // to it. That is what lets a table sit under three spaces
                // of indent, or inside a list item whose content prefix is
                // plain spaces; it is also why a table inside a blockquote
                // does not parse — `> ` is not an indent the scan can strip.
                //
                // The scan has already validated and parsed the whole
                // table, so there is nothing left to do per line: open the
                // block, swallow this line, and let `check_open_blocks`
                // count the rest off against `end`.
                let start = self.line_start + fns;
                self.close_down_to(container, &mut opened);
                self.open_child(
                    container,
                    OpenKind::GridTable {
                        end: table.end,
                        alignments: table.alignments,
                        rows: table.rows,
                    },
                    start,
                );
                opened = true;
                self.offset = self.line.len();
                break;
            } else if indented && !maybe_lazy && !self.blank {
                self.close_down_to(container, &mut opened);
                self.advance_offset(TAB_STOP, true);
                let start = self.line_start + self.offset;
                self.open_child(container, OpenKind::IndentedCode, start);
                opened = true;
                break;
            } else if !indented
                && self.opts.tables
                && is_paragraph
                && rest.contains('|')
                && let Some(alignments) = self.scan_table_delimiter(container, rest)
            {
                // GFM table: the open one-line paragraph is the header row.
                self.begin_table(container, alignments);
                opened = true;
                self.offset = self.line.len();
                break;
            } else {
                break;
            }
            if matches!(
                self.open[container].kind,
                OpenKind::Heading { .. } | OpenKind::ThematicBreak
            ) {
                break;
            }
            maybe_lazy = false;
        }
        opened
    }

    /// Add the line's remaining text to the right leaf (port of cmark's
    /// add_text_to_container, including the tight/loose blank-line flags).
    fn add_text(&mut self, last_matched: usize, _tip_before: usize, opened: bool) {
        self.find_first_nonspace();

        // container = deepest matched-or-opened block. When nothing opened,
        // unmatched deeper blocks are still on the stack above it.
        let container = if opened {
            self.open.len() - 1
        } else {
            last_matched
        };

        // Blank-line flag on the container's last child (open or closed).
        if self.blank {
            if container + 1 < self.open.len() {
                self.open[container + 1].last_line_blank = true;
            } else if let Some(last) = self.open[container].children.last_mut() {
                last.last_line_blank = true;
            }
        }
        let container_has_children =
            !self.open[container].children.is_empty() || container + 1 < self.open.len();
        let last_line_blank = self.blank
            && !matches!(
                self.open[container].kind,
                OpenKind::BlockQuote
                    | OpenKind::Heading { .. }
                    | OpenKind::ThematicBreak
                    | OpenKind::FencedCode { .. }
            )
            && !(matches!(self.open[container].kind, OpenKind::Item(_))
                && !container_has_children
                && self.open[container].start_line == self.line_no);
        self.open[container].last_line_blank = last_line_blank;
        for o in self.open.iter_mut().take(container) {
            o.last_line_blank = false;
        }

        // Lazy continuation: nothing opened, deeper blocks unmatched, the
        // tip is a paragraph, and the line is non-blank.
        let tip = self.open.len() - 1;
        if !opened
            && tip != last_matched
            && !self.blank
            && matches!(self.open[tip].kind, OpenKind::Paragraph)
        {
            self.add_line();
            self.update_span_ends(tip);
            return;
        }

        // Not lazy: close unmatched blocks (no-op when a block opened —
        // close_down_to already ran).
        if !opened {
            while self.open.len() - 1 > last_matched {
                self.finalize_tip();
            }
        }
        let tip = self.open.len() - 1;

        match &self.open[tip].kind {
            OpenKind::FencedCode { .. } if self.open[tip].start_line == self.line_no => {
                // The opening fence line itself: info only, no content.
                self.update_span_ends(tip);
            }
            OpenKind::IndentedCode | OpenKind::FencedCode { .. } => {
                self.add_line();
                self.update_span_ends(tip);
            }
            OpenKind::HtmlBlock { html_type, .. } => {
                let t = *html_type;
                let content = &self.line[self.first_nonspace..];
                let finished = match t {
                    1 => {
                        let low = content.to_ascii_lowercase();
                        ["</pre>", "</script>", "</style>", "</textarea>"]
                            .iter()
                            .any(|e| low.contains(e))
                    }
                    2 => content.contains("-->"),
                    3 => content.contains("?>"),
                    4 => content.contains('>'),
                    5 => content.contains("]]>"),
                    _ => false,
                };
                self.add_line();
                self.update_span_ends(tip);
                if finished {
                    self.finalize_tip();
                }
            }
            OpenKind::Table { .. } => {
                if !self.blank {
                    self.add_table_row();
                }
                self.update_span_ends(tip);
            }
            // Grid-table lines carry no work: the rows were built during
            // the lookahead that opened the block. All the line does is
            // extend the span so the table covers its closing rule.
            OpenKind::GridTable { .. } => {
                self.update_span_ends(tip);
            }
            _ if self.blank => {
                self.update_span_ends(if container + 1 < self.open.len() {
                    container
                } else {
                    tip
                });
            }
            OpenKind::Heading { .. } => {
                self.advance_offset(self.first_nonspace - self.offset, false);
                self.add_line();
                self.update_span_ends(tip);
                self.finalize_tip(); // ATX headings are single-line
            }
            OpenKind::Paragraph => {
                self.advance_offset(self.first_nonspace - self.offset, false);
                self.add_line();
                self.update_span_ends(tip);
            }
            _ => {
                // Container tip: open a paragraph for the line.
                let start = self.line_start + self.first_nonspace;
                self.advance_offset(self.first_nonspace - self.offset, false);
                self.open_child(tip, OpenKind::Paragraph, start);
                self.add_line();
                self.update_span_ends(self.open.len() - 1);
            }
        }
    }

    fn update_span_ends(&mut self, upto: usize) {
        let end = self.line_start + self.line.len();
        for o in self.open.iter_mut().take(upto + 1) {
            o.span_end = end;
        }
    }

    /// Open a child block under `parent` (an index into `open`), closing
    /// anything the parent cannot contain. Returns the new tip index.
    fn open_child(&mut self, parent: usize, kind: OpenKind, span_start: usize) -> usize {
        // Close everything deeper than parent.
        while self.open.len() - 1 > parent {
            self.finalize_tip();
        }
        // Walk up while the tip can't contain the new kind.
        loop {
            let tip_kind = &self.open[self.open.len() - 1].kind;
            let ok = match (tip_kind, &kind) {
                (OpenKind::List(_), OpenKind::Item(_)) => true,
                (OpenKind::List(_), _) => false,
                (_, OpenKind::Item(_)) => false,
                (
                    OpenKind::Document
                    | OpenKind::BlockQuote
                    | OpenKind::Item(_)
                    | OpenKind::FootnoteDef { .. },
                    _,
                ) => true,
                _ => false,
            };
            if ok {
                break;
            }
            self.finalize_tip();
        }
        self.open.push(Open::new(kind, span_start, self.line_no));
        self.open.len() - 1
    }

    fn close_down_to(&mut self, container: usize, opened: &mut bool) {
        // Before the first new block opens, unmatched deeper blocks close.
        if !*opened {
            while self.open.len() - 1 > container {
                self.finalize_tip();
            }
        }
    }

    /// Finalize the tip block and attach it to its parent.
    fn finalize_tip(&mut self) {
        debug_assert!(self.open.len() > 1);
        let mut o = match self.open.pop() {
            Some(o) => o,
            None => return,
        };
        let span = Span::new(o.span_start, o.span_end);
        let pre = match o.kind {
            OpenKind::Document => None,
            OpenKind::Paragraph => {
                let consumed = extract_refdefs_into(&mut o.leaf, &mut self.refmap);
                let _ = consumed;
                o.leaf.trim_end();
                if o.leaf.content.is_empty() {
                    None
                } else {
                    let start = o.leaf.map(0, false);
                    Some(PreBlock {
                        span: Span::new(start.max(span.start).min(span.end), span.end),
                        kind: PreKind::Paragraph(o.leaf),
                        last_line_blank: false,
                    })
                }
            }
            OpenKind::Heading { level } => {
                strip_heading_suffix(&mut o.leaf);
                Some(PreBlock {
                    span,
                    kind: PreKind::Heading {
                        level,
                        leaf: o.leaf,
                    },
                    last_line_blank: false,
                })
            }
            OpenKind::ThematicBreak => Some(PreBlock {
                span,
                kind: PreKind::ThematicBreak,
                last_line_blank: false,
            }),
            OpenKind::BlockQuote => Some(finalize_blockquote(span, o.children, self.opts.alerts)),
            OpenKind::List(data) => {
                let tight = compute_tight(&o.children);
                Some(PreBlock {
                    span,
                    kind: PreKind::List {
                        kind: data.kind,
                        tight,
                        items: o.children,
                    },
                    last_line_blank: false,
                })
            }
            OpenKind::Item(_) => {
                let checked = if self.opts.task_lists {
                    extract_task_marker(&mut o.children)
                } else {
                    None
                };
                Some(PreBlock {
                    span,
                    kind: PreKind::Item {
                        checked,
                        children: o.children,
                    },
                    last_line_blank: false,
                })
            }
            OpenKind::IndentedCode => {
                // Drop trailing blank *lines* (spaces on content lines stay).
                let mut literal = o.leaf.content;
                while let Some(body) = literal.strip_suffix('\n') {
                    let line_start = body.rfind('\n').map(|i| i + 1).unwrap_or(0);
                    if body[line_start..].trim_matches([' ', '\t']).is_empty() {
                        literal.truncate(line_start);
                    } else {
                        break;
                    }
                }
                Some(PreBlock {
                    span,
                    kind: PreKind::CodeBlock {
                        info: None,
                        literal,
                        fenced: false,
                    },
                    last_line_blank: false,
                })
            }
            OpenKind::FencedCode { info, .. } => Some(PreBlock {
                span,
                kind: PreKind::CodeBlock {
                    info,
                    literal: o.leaf.content,
                    fenced: true,
                },
                last_line_blank: false,
            }),
            OpenKind::HtmlBlock { .. } => {
                let mut literal = o.leaf.content;
                if literal.ends_with('\n') {
                    literal.pop();
                }
                Some(PreBlock {
                    span,
                    kind: PreKind::HtmlBlock { literal },
                    last_line_blank: false,
                })
            }
            OpenKind::Table { alignments, rows } => Some(PreBlock {
                span,
                kind: PreKind::Table { alignments, rows },
                last_line_blank: false,
            }),
            // Grid tables lower into the same PreKind as GFM tables: one
            // shape reaches layout, HTML and decor, whichever syntax wrote
            // it.
            OpenKind::GridTable {
                alignments, rows, ..
            } => Some(PreBlock {
                span,
                kind: PreKind::Table { alignments, rows },
                last_line_blank: false,
            }),
            OpenKind::FootnoteDef { label } => {
                self.footnote_labels.insert(label.clone());
                Some(PreBlock {
                    span,
                    kind: PreKind::FootnoteDef {
                        label,
                        children: o.children,
                    },
                    last_line_blank: false,
                })
            }
        };
        if let Some(mut pre) = pre {
            pre.last_line_blank = o.last_line_blank;
            let parent = self.open.len() - 1;
            let parent_open = &mut self.open[parent];
            if parent_open.span_end < span.end {
                parent_open.span_end = span.end;
            }
            parent_open.children.push(pre);
        }
    }

    /// Strip reference definitions from an *open* paragraph (setext check).
    fn extract_refdefs(&mut self, idx: usize) {
        extract_refdefs_into(&mut self.open[idx].leaf, &mut self.refmap);
    }

    // --- GFM tables ---

    /// If `rest` is a valid delimiter row matching the open paragraph's
    /// single header line, return the alignments.
    fn scan_table_delimiter(&self, para: usize, rest: &str) -> Option<Vec<Alignment>> {
        let leaf = &self.open[para].leaf;
        let content = leaf.content.strip_suffix('\n').unwrap_or(&leaf.content);
        if content.contains('\n') || content.is_empty() {
            return None; // header must be exactly one line
        }
        let delim = rest.trim_end();
        if !delim.contains('-') && !delim.contains(':') {
            return None;
        }
        let mut alignments = Vec::new();
        for cell in split_row(delim)? {
            let c = cell.trim_matches([' ', '\t']);
            let (left, rest0) = match c.strip_prefix(':') {
                Some(r) => (true, r),
                None => (false, c),
            };
            let (right, dashes) = match rest0.strip_suffix(':') {
                Some(r) => (true, r),
                None => (false, rest0),
            };
            if dashes.is_empty() || !dashes.bytes().all(|b| b == b'-') {
                return None;
            }
            alignments.push(match (left, right) {
                (true, true) => Alignment::Center,
                (true, false) => Alignment::Left,
                (false, true) => Alignment::Right,
                (false, false) => Alignment::None,
            });
        }
        let header_cells = split_row(content)?;
        if header_cells.len() != alignments.len() {
            return None;
        }
        Some(alignments)
    }

    /// Convert the open paragraph (header line) into a table.
    fn begin_table(&mut self, para: usize, alignments: Vec<Alignment>) {
        let o = &mut self.open[para];
        let leaf = std::mem::take(&mut o.leaf);
        let span_start = o.span_start;
        o.kind = OpenKind::Table {
            alignments: alignments.clone(),
            rows: Vec::new(),
        };
        let row = make_row(&leaf, true);
        if let OpenKind::Table { rows, .. } = &mut self.open[para].kind {
            rows.push(row);
        }
        let _ = span_start;
    }

    fn add_table_row(&mut self) {
        let tip = self.open.len() - 1;
        self.advance_offset(self.first_nonspace - self.offset, false);
        let mut leaf = Leaf::default();
        let start = self.offset;
        let text = self.line[start..].trim_end();
        leaf.push(text, self.line_start + start, text.len());
        let ncols = match &self.open[tip].kind {
            OpenKind::Table { alignments, .. } => alignments.len(),
            _ => 0,
        };
        let mut row = make_row(&leaf, false);
        // Pad or truncate body rows to the header width.
        while row.cells.len() < ncols {
            let end = row.span.end;
            row.cells.push((Span::new(end, end), Leaf::default()));
        }
        row.cells.truncate(ncols);
        if let OpenKind::Table { rows, .. } = &mut self.open[tip].kind {
            rows.push(row);
        }
    }
}

/// Split a table row into raw cell strings on unescaped pipes.
/// Returns `None` if the row is empty after stripping outer pipes.
fn split_row(row: &str) -> Option<Vec<&str>> {
    let row = row.trim_matches([' ', '\t']);
    let bytes = row.as_bytes();
    let mut cells = Vec::new();
    let mut cell_start = 0;
    let mut i = 0;
    let started_pipe = bytes.first() == Some(&b'|');
    if started_pipe {
        cell_start = 1;
        i = 1;
    }
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() => i += 2,
            b'|' => {
                cells.push(&row[cell_start..i]);
                i += 1;
                cell_start = i;
            }
            _ => i += 1,
        }
    }
    if cell_start < bytes.len() {
        cells.push(&row[cell_start..]);
    } else if (!started_pipe || bytes.last() != Some(&b'|'))
        && cell_start == bytes.len()
        && bytes.last() != Some(&b'|')
    {
        cells.push("");
    }
    if cells.is_empty() { None } else { Some(cells) }
}

/// Build a PreRow from a one-line leaf, splitting cells and handling `\|`.
fn make_row(leaf: &Leaf, header: bool) -> PreRow {
    let content = leaf.content.strip_suffix('\n').unwrap_or(&leaf.content);
    let trimmed = content.trim_matches([' ', '\t']);
    let lead = content.len() - content.trim_start_matches([' ', '\t']).len();
    let base = lead;
    let bytes = trimmed.as_bytes();
    let mut cells: Vec<(Span, Leaf)> = Vec::new();
    let mut i = 0;
    let mut cell_start = 0;
    if bytes.first() == Some(&b'|') {
        i = 1;
        cell_start = 1;
    }
    let mut push_cell = |a: usize, b: usize| {
        // Trim cell-internal whitespace.
        let raw = &trimmed[a..b];
        let t_start = a + (raw.len() - raw.trim_start_matches([' ', '\t']).len());
        let t_end = b - (raw.len() - raw.trim_end_matches([' ', '\t']).len());
        let (t_start, t_end) = if t_start >= t_end {
            (a, a)
        } else {
            (t_start, t_end)
        };
        let mut cell = leaf.slice(base + t_start, base + t_end);
        // GFM: `\|` becomes a literal `|` before inline parsing.
        if cell.content.contains("\\|") {
            let mut rebuilt = Leaf::default();
            let cbytes = cell.content.clone();
            let cb = cbytes.as_bytes();
            let mut j = 0;
            let mut run = 0;
            while j < cb.len() {
                if cb[j] == b'\\' && cb.get(j + 1) == Some(&b'|') {
                    let seg = &cbytes[run..j];
                    rebuilt.push(seg, cell.map(run, false), seg.len());
                    rebuilt.push("|", cell.map(j, false), 2);
                    j += 2;
                    run = j;
                } else {
                    j += 1;
                }
            }
            let seg = &cbytes[run..];
            rebuilt.push(seg, cell.map(run, false), seg.len());
            cell = rebuilt;
        }
        let span = Span::new(
            leaf.map(base + t_start, false),
            leaf.map(base + t_end, true),
        );
        cells.push((span, cell));
    };
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() => i += 2,
            b'|' => {
                push_cell(cell_start, i);
                i += 1;
                cell_start = i;
            }
            _ => i += 1,
        }
    }
    if cell_start < bytes.len() {
        push_cell(cell_start, bytes.len());
    }
    let span = Span::new(leaf.map(0, false), leaf.map(leaf.content.len(), true));
    PreRow {
        span,
        header,
        cells,
    }
}

/// `#{1,6}` followed by space/tab or EOL → (level, offset past marker+ws).
fn scan_atx_heading(rest: &str) -> Option<(u8, usize)> {
    let bytes = rest.as_bytes();
    let n = bytes.iter().take_while(|&&c| c == b'#').count();
    if n == 0 || n > 6 {
        return None;
    }
    match bytes.get(n) {
        None => Some((n as u8, n)),
        Some(b' ') | Some(b'\t') => {
            let mut i = n;
            while matches!(bytes.get(i), Some(b' ') | Some(b'\t')) {
                i += 1;
            }
            Some((n as u8, i))
        }
        _ => None,
    }
}

/// Opening code fence: ≥3 backticks or tildes; backtick info strings may
/// not contain backticks. Returns (fence char, length, decoded info).
fn scan_open_fence(rest: &str) -> Option<(u8, usize, Option<String>)> {
    let bytes = rest.as_bytes();
    let ch = *bytes.first()?;
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let len = bytes.iter().take_while(|&&c| c == ch).count();
    if len < 3 {
        return None;
    }
    let info_raw = &rest[len..];
    if ch == b'`' && info_raw.contains('`') {
        return None;
    }
    let info = scan::unescape_string(info_raw.trim_matches([' ', '\t']));
    Some((ch, len, if info.is_empty() { None } else { Some(info) }))
}

/// Setext underline: all `=` or all `-` (≥1), trailing blanks allowed.
fn scan_setext_underline(rest: &str) -> Option<u8> {
    let t = rest.trim_end_matches([' ', '\t']);
    if t.is_empty() {
        return None;
    }
    if t.bytes().all(|c| c == b'=') {
        Some(1)
    } else if t.bytes().all(|c| c == b'-') {
        Some(2)
    } else {
        None
    }
}

/// `[^label]:` at line start → (label, offset past marker and one space).
fn scan_footnote_def(rest: &str) -> Option<(String, usize)> {
    let inner = rest.strip_prefix("[^")?;
    let close = inner.find(']')?;
    let label = &inner[..close];
    if label.is_empty() || label.contains(char::is_whitespace) || label.contains('[') {
        return None;
    }
    let after = &inner[close + 1..];
    if !after.starts_with(':') {
        return None;
    }
    let mut off = 2 + close + 2; // "[^" + label + "]:"
    let bytes = rest.as_bytes();
    while matches!(bytes.get(off), Some(b' ') | Some(b'\t')) {
        off += 1;
    }
    Some((label.to_owned(), off))
}

/// List marker at the start of `rest`. Returns (marker length, kind).
fn parse_list_marker(rest: &str, interrupts_paragraph: bool) -> Option<(usize, ListKind)> {
    let bytes = rest.as_bytes();
    let c = *bytes.first()?;
    if matches!(c, b'*' | b'+' | b'-') {
        if !matches!(bytes.get(1), None | Some(b' ') | Some(b'\t')) {
            return None;
        }
        if interrupts_paragraph && rest[1..].trim_matches([' ', '\t']).is_empty() {
            return None; // empty item can't interrupt a paragraph
        }
        Some((1, ListKind::Bullet(c as char)))
    } else if c.is_ascii_digit() {
        let ndigits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
        if ndigits > 9 {
            return None;
        }
        let delim = *bytes.get(ndigits)?;
        if delim != b'.' && delim != b')' {
            return None;
        }
        if !matches!(bytes.get(ndigits + 1), None | Some(b' ') | Some(b'\t')) {
            return None;
        }
        let start: u64 = rest[..ndigits].parse().ok()?;
        if interrupts_paragraph
            && (start != 1 || rest[ndigits + 1..].trim_matches([' ', '\t']).is_empty())
        {
            return None;
        }
        Some((
            ndigits + 1,
            ListKind::Ordered {
                start,
                delim: delim as char,
            },
        ))
    } else {
        None
    }
}

/// HTML block start conditions 1–7. `allow7` gates type 7 (which cannot
/// interrupt a paragraph or a lazy paragraph line).
fn scan_html_block_start(rest: &str, allow7: bool) -> Option<u8> {
    if !rest.starts_with('<') {
        return None;
    }
    let low = rest.to_ascii_lowercase();
    for t in ["<pre", "<script", "<style", "<textarea"] {
        if low.starts_with(t) {
            let after = low.as_bytes().get(t.len());
            if matches!(after, None | Some(b' ') | Some(b'\t') | Some(b'>')) {
                return Some(1);
            }
        }
    }
    if rest.starts_with("<!--") {
        return Some(2);
    }
    if rest.starts_with("<?") {
        return Some(3);
    }
    if rest.starts_with("<![CDATA[") {
        return Some(5);
    }
    if rest.len() > 2 && rest.as_bytes()[1] == b'!' && rest.as_bytes()[2].is_ascii_alphabetic() {
        return Some(4);
    }
    // Type 6: known tag names.
    const TAGS: &[&str] = &[
        "address",
        "article",
        "aside",
        "base",
        "basefont",
        "blockquote",
        "body",
        "caption",
        "center",
        "col",
        "colgroup",
        "dd",
        "details",
        "dialog",
        "dir",
        "div",
        "dl",
        "dt",
        "fieldset",
        "figcaption",
        "figure",
        "footer",
        "form",
        "frame",
        "frameset",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "head",
        "header",
        "hr",
        "html",
        "iframe",
        "legend",
        "li",
        "link",
        "main",
        "menu",
        "menuitem",
        "nav",
        "noframes",
        "ol",
        "optgroup",
        "option",
        "p",
        "param",
        "search",
        "section",
        "summary",
        "table",
        "tbody",
        "td",
        "tfoot",
        "th",
        "thead",
        "title",
        "tr",
        "track",
        "ul",
    ];
    let name_start = if low.starts_with("</") { 2 } else { 1 };
    let name: String = low[name_start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    if TAGS.contains(&name.as_str()) {
        let after = &low[name_start + name.len()..];
        if after.is_empty()
            || after.starts_with(' ')
            || after.starts_with('\t')
            || after.starts_with('>')
            || after.starts_with("/>")
        {
            return Some(6);
        }
    }
    // Type 7: a complete tag alone on the line.
    if allow7 {
        let closing = rest.starts_with("</");
        if let Some(end) = scan::scan_html_tag(rest, 0)
            && rest[end..].trim_matches([' ', '\t']).is_empty()
        {
            if closing {
                return Some(7);
            }
            let name: String = rest[1..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect::<String>()
                .to_ascii_lowercase();
            if !matches!(name.as_str(), "pre" | "script" | "style" | "textarea") {
                return Some(7);
            }
        }
    }
    None
}

/// Strip an ATX heading's optional closing sequence and surrounding spaces.
fn strip_heading_suffix(leaf: &mut Leaf) {
    leaf.trim_end();
    let content = &leaf.content;
    let trimmed = content.trim_end_matches('#');
    if trimmed.len() != content.len() {
        // Closing run must be preceded by a space/tab (or be the whole text)
        // and not by a backslash.
        if trimmed.is_empty() {
            leaf.truncate_content(0);
        } else if trimmed.ends_with([' ', '\t']) {
            leaf.truncate_content(trimmed.len());
            leaf.trim_end();
        }
    }
}

impl Leaf {
    fn truncate_content(&mut self, n: usize) {
        self.content.truncate(n);
        self.segs.retain_mut(|s| {
            if s.c_start >= n {
                return false;
            }
            if s.c_start + s.c_len > n {
                let cut = s.c_start + s.c_len - n;
                s.c_len -= cut;
                if s.s_len >= cut && s.c_len + cut == s.s_len {
                    s.s_len -= cut;
                }
            }
            true
        });
    }
}

/// Blockquote → Alert when the first line is exactly `[!KIND]`.
fn finalize_blockquote(span: Span, mut children: Vec<PreBlock>, alerts: bool) -> PreBlock {
    let alert = alerts
        .then_some(())
        .and(children.first_mut())
        .and_then(|first| {
            if let PreKind::Paragraph(leaf) = &mut first.kind {
                let line_end = leaf.content.find('\n').unwrap_or(leaf.content.len());
                let first_line = leaf.content[..line_end].trim_end();
                let kind = match first_line.to_ascii_uppercase().as_str() {
                    "[!NOTE]" => Some(AlertKind::Note),
                    "[!TIP]" => Some(AlertKind::Tip),
                    "[!IMPORTANT]" => Some(AlertKind::Important),
                    "[!WARNING]" => Some(AlertKind::Warning),
                    "[!CAUTION]" => Some(AlertKind::Caution),
                    _ => None,
                }?;
                let drop = (line_end + 1).min(leaf.content.len());
                leaf.drop_prefix(drop);
                Some((kind, leaf.content.is_empty()))
            } else {
                None
            }
        });
    match alert {
        Some((kind, first_empty)) => {
            if first_empty {
                children.remove(0);
            }
            PreBlock {
                span,
                kind: PreKind::Alert { kind, children },
                last_line_blank: false,
            }
        }
        None => PreBlock {
            span,
            kind: PreKind::BlockQuote(children),
            last_line_blank: false,
        },
    }
}

/// GFM task-list marker on the item's first paragraph: `[ ] ` / `[x] `.
fn extract_task_marker(children: &mut [PreBlock]) -> Option<bool> {
    let first = children.first_mut()?;
    let PreKind::Paragraph(leaf) = &mut first.kind else {
        return None;
    };
    let bytes = leaf.content.as_bytes();
    if bytes.len() < 4 || bytes[0] != b'[' || bytes[2] != b']' {
        return None;
    }
    let checked = match bytes[1] {
        b' ' => false,
        b'x' | b'X' => true,
        _ => return None,
    };
    if !matches!(bytes[3], b' ' | b'\t') {
        return None;
    }
    let mut drop = 4;
    while matches!(leaf.content.as_bytes().get(drop), Some(b' ') | Some(b'\t')) {
        drop += 1;
    }
    leaf.drop_prefix(drop);
    Some(checked)
}

/// cmark's S_ends_with_blank_line: follow last children of lists/items.
fn ends_with_blank(pre: &PreBlock) -> bool {
    let mut node = pre;
    loop {
        match &node.kind {
            PreKind::List { items, .. } if !items.is_empty() => {
                node = items.last().expect("non-empty");
            }
            PreKind::Item { children, .. } if !children.is_empty() => {
                node = children.last().expect("non-empty");
            }
            _ => return node.last_line_blank,
        }
    }
}

/// cmark's list-finalize tightness rule.
fn compute_tight(items: &[PreBlock]) -> bool {
    for (i, item) in items.iter().enumerate() {
        let has_next = i + 1 < items.len();
        if ends_with_blank(item) && has_next {
            return false;
        }
        if let PreKind::Item { children, .. } = &item.kind {
            for (j, sub) in children.iter().enumerate() {
                let sub_has_next = j + 1 < children.len();
                if ends_with_blank(sub) && (has_next || sub_has_next) {
                    return false;
                }
            }
        }
    }
    true
}

/// Strip link reference definitions from the head of a closed (or closing)
/// paragraph, adding them to the map. Returns bytes consumed.
fn extract_refdefs_into(leaf: &mut Leaf, refmap: &mut RefMap) -> usize {
    let mut consumed = 0;
    loop {
        let rest = &leaf.content[consumed..];
        let Some((len, label, dest, title)) = scan_refdef(rest) else {
            break;
        };
        refmap
            .entry(scan::normalize_label(&label))
            .or_insert((dest, title));
        consumed += len;
    }
    if consumed > 0 {
        leaf.drop_prefix(consumed);
    }
    consumed
}

/// Scan one reference definition at the start of `s` (paragraph content:
/// leading indent already stripped per line). Returns
/// (bytes consumed through the end of the definition, label, dest, title).
fn scan_refdef(s: &str) -> Option<(usize, String, String, String)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    // Up to 3 leading spaces (only possible on continuation lines).
    while i < 3 && bytes.get(i) == Some(&b' ') {
        i += 1;
    }
    let (label_end, label) = scan::scan_link_label(s, i)?;
    if label.trim().is_empty() {
        return None;
    }
    if bytes.get(label_end) != Some(&b':') {
        return None;
    }
    let mut i = label_end + 1;
    // Optional whitespace incl. up to one newline.
    let mut newlines = 0;
    while let Some(&c) = bytes.get(i) {
        match c {
            b' ' | b'\t' => i += 1,
            b'\n' if newlines == 0 => {
                newlines += 1;
                i += 1;
            }
            _ => break,
        }
    }
    let (dest_end, dest_raw) = scan::scan_link_destination(s, i)?;
    if dest_end == i && !s[i..].starts_with('<') {
        return None;
    }
    let dest = scan::unescape_string(dest_raw);
    // Position after destination; try title.
    let mut j = dest_end;
    let mut saw_ws = false;
    let mut title_newlines = 0;
    while let Some(&c) = bytes.get(j) {
        match c {
            b' ' | b'\t' => {
                saw_ws = true;
                j += 1;
            }
            b'\n' if title_newlines == 0 => {
                title_newlines += 1;
                saw_ws = true;
                j += 1;
            }
            _ => break,
        }
    }
    // The destination line must end (or a title must follow whitespace).
    let dest_line_ok = {
        // Only spaces/tabs between dest_end and the next newline/EOF.
        let mut k = dest_end;
        loop {
            match bytes.get(k) {
                None | Some(b'\n') => break true,
                Some(b' ') | Some(b'\t') => k += 1,
                _ => break false,
            }
        }
    };
    if saw_ws
        && j < bytes.len()
        && matches!(bytes[j], b'"' | b'\'' | b'(')
        && let Some((title_end, title_raw)) = scan_multiline_title(s, j)
    {
        // Title's final line must contain nothing else.
        let mut k = title_end;
        let title_line_ok = loop {
            match bytes.get(k) {
                None | Some(b'\n') => break true,
                Some(b' ') | Some(b'\t') => k += 1,
                _ => break false,
            }
        };
        if title_line_ok {
            let end = if bytes.get(k) == Some(&b'\n') {
                k + 1
            } else {
                k
            };
            return Some((
                end,
                label.to_owned(),
                dest,
                scan::unescape_string(title_raw),
            ));
        }
    }
    if !dest_line_ok {
        return None;
    }
    // No title: consume through the destination's line end.
    let mut k = dest_end;
    while matches!(bytes.get(k), Some(b' ') | Some(b'\t')) {
        k += 1;
    }
    let end = if bytes.get(k) == Some(&b'\n') {
        k + 1
    } else {
        k
    };
    Some((end, label.to_owned(), dest, String::new()))
}

/// A link title that may span multiple lines (but no blank line).
fn scan_multiline_title(s: &str, pos: usize) -> Option<(usize, &str)> {
    let bytes = s.as_bytes();
    let open = *bytes.get(pos)?;
    let close = match open {
        b'"' => b'"',
        b'\'' => b'\'',
        b'(' => b')',
        _ => return None,
    };
    let mut i = pos + 1;
    while i < bytes.len() {
        let c = bytes[i];
        if c == close {
            return Some((i + 1, &s[pos + 1..i]));
        }
        if c == b'(' && open == b'(' {
            return None;
        }
        if c == b'\n' && bytes.get(i + 1) == Some(&b'\n') {
            return None; // blank line ends the attempt
        }
        if c == b'\\' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_punctuation() {
            i += 2;
        } else {
            i += 1;
        }
    }
    None
}
