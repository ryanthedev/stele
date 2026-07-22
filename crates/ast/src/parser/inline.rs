//! Phase 2: inline parsing of one leaf's raw text.
//!
//! Pseudocode (spec appendix "phase 2" + cmark's inlines.c):
//!
//! ```text
//! single left-to-right scan over the content:
//!     ordinary bytes accumulate in a pending text buffer
//!     `  -> code span (runs pre-indexed; unmatched runs are literal)
//!     \  -> escape / hard break
//!     &  -> entity (decoded into pending)
//!     <  -> autolink | raw HTML | literal
//!     $  -> math span (runs pre-indexed, code-span-like matching)
//!     [ ![ -> bracket stack push
//!     ]  -> try inline/reference/footnote link; ONE attempt per opener
//!     * _ ~ -> delimiter run with flanking flags, onto delimiter stack
//!     w/: /@ -> GFM autolink literal (www / scheme / email lookback)
//! then process_emphasis over the whole delimiter stack
//! (links run it early, bounded to their bracket's stack position)
//! ```
//!
//! Linear-time guards: `openers_bottom` in `process_emphasis`, one match
//! attempt per bracket opener (failed openers pop), pre-indexed backtick
//! and dollar runs, bracket/nesting caps.

use super::scan;
use super::{Leaf, ParseOptions, RefMap};
use crate::ast::{Inline, InlineKind, NodeId};
use std::collections::HashMap;

const MAX_BRACKET_DEPTH: usize = 200;
const NIL: i32 = -1;

struct INode {
    kind: InlineKind,
    c_start: usize,
    c_end: usize,
    prev: i32,
    next: i32,
    alive: bool,
}

struct Delim {
    node: usize,
    ch: u8,
    orig_len: usize,
    len: usize,
    can_open: bool,
    can_close: bool,
    prev: i32,
    next: i32,
    alive: bool,
}

struct Bracket {
    node: usize,
    image: bool,
    active: bool,
    /// Delimiter-stack top at push time; bounds process_emphasis.
    delim_bottom: i32,
    /// Content offset just past `[` (label text starts here).
    label_start: usize,
}

pub(crate) struct InlineCx<'a> {
    pub refmap: &'a RefMap,
    pub footnotes: &'a dyn Fn(&str) -> bool,
    pub opts: ParseOptions,
}

pub(crate) fn parse_inlines_cx(leaf: &Leaf, cx: &InlineCx) -> Vec<Inline> {
    let mut p = InlineParser::new(leaf, cx);
    p.run();
    p.assemble()
}

struct InlineParser<'a> {
    content: &'a str,
    leaf: &'a Leaf,
    cx: &'a InlineCx<'a>,
    pos: usize,
    nodes: Vec<INode>,
    head: i32,
    tail: i32,
    delims: Vec<Delim>,
    delim_top: i32,
    brackets: Vec<Bracket>,
    pending: String,
    pending_start: usize,
    /// Pre-indexed backtick and dollar runs: char -> (positions, lens).
    runs: HashMap<u8, Vec<(usize, usize)>>,
}

impl<'a> InlineParser<'a> {
    fn new(leaf: &'a Leaf, cx: &'a InlineCx<'a>) -> InlineParser<'a> {
        let content = leaf.content.as_str();
        let mut runs: HashMap<u8, Vec<(usize, usize)>> = HashMap::new();
        let bytes = content.as_bytes();
        // Backtick runs are indexed *raw* (escapes do not work inside code
        // spans, so `foo\`bar` closes at the escaped-looking backtick).
        // Dollar runs are escape-aware.
        let mut i = 0;
        while i < bytes.len() {
            let c = bytes[i];
            if c == b'`' || (c == b'$' && !is_escaped(bytes, i)) {
                let start = i;
                while i < bytes.len() && bytes[i] == c {
                    i += 1;
                }
                runs.entry(c).or_default().push((start, i - start));
            } else {
                i += 1;
            }
        }
        InlineParser {
            content,
            leaf,
            cx,
            pos: 0,
            nodes: Vec::new(),
            head: NIL,
            tail: NIL,
            delims: Vec::new(),
            delim_top: NIL,
            brackets: Vec::new(),
            pending: String::new(),
            pending_start: 0,
            runs,
        }
    }

    // --- node-list plumbing ---

    fn append_node(&mut self, kind: InlineKind, c_start: usize, c_end: usize) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(INode {
            kind,
            c_start,
            c_end,
            prev: self.tail,
            next: NIL,
            alive: true,
        });
        if self.tail != NIL {
            self.nodes[self.tail as usize].next = idx as i32;
        } else {
            self.head = idx as i32;
        }
        self.tail = idx as i32;
        idx
    }

    fn remove_node(&mut self, idx: usize) {
        let (prev, next) = (self.nodes[idx].prev, self.nodes[idx].next);
        if prev != NIL {
            self.nodes[prev as usize].next = next;
        } else {
            self.head = next;
        }
        if next != NIL {
            self.nodes[next as usize].prev = prev;
        } else {
            self.tail = prev;
        }
        self.nodes[idx].alive = false;
    }

    fn flush(&mut self) {
        self.flush_at(self.pos);
    }

    /// Flush pending text with an explicit span end (used when trailing
    /// characters were trimmed out of the pending buffer, e.g. the spaces
    /// of a hard break — the text span must not cover them).
    fn flush_at(&mut self, end: usize) {
        if !self.pending.is_empty() {
            let text = std::mem::take(&mut self.pending);
            self.append_node(InlineKind::Text(text), self.pending_start, end);
        }
        self.pending_start = self.pos;
    }

    fn push_pending(&mut self, s: &str) {
        if self.pending.is_empty() {
            self.pending_start = self.pos;
        }
        self.pending.push_str(s);
    }

    // --- main scan ---

    fn run(&mut self) {
        let bytes = self.content.as_bytes();
        while self.pos < bytes.len() {
            let c = bytes[self.pos];
            match c {
                b'\n' => self.handle_newline(),
                b'\\' => self.handle_backslash(),
                b'`' => self.handle_backtick(),
                b'&' => self.handle_entity(),
                b'<' => self.handle_lt(),
                b'$' => self.handle_dollar(),
                b'[' => {
                    self.flush();
                    let n = self.append_node(InlineKind::Text("[".into()), self.pos, self.pos + 1);
                    self.push_bracket(n, false, self.pos + 1);
                    self.pos += 1;
                    self.pending_start = self.pos;
                }
                b'!' if bytes.get(self.pos + 1) == Some(&b'[') => {
                    self.flush();
                    let n = self.append_node(InlineKind::Text("![".into()), self.pos, self.pos + 2);
                    self.push_bracket(n, true, self.pos + 2);
                    self.pos += 2;
                    self.pending_start = self.pos;
                }
                b']' => self.handle_close_bracket(),
                b'*' | b'_' => self.handle_delim(c),
                b'~' => {
                    if self.cx.opts.strikethrough {
                        self.handle_delim(c);
                    } else {
                        self.consume_char();
                    }
                }
                b'w' | b'W' => {
                    if !self.try_www_autolink() {
                        self.consume_char();
                    }
                }
                b':' => {
                    if !self.try_scheme_autolink() {
                        self.consume_char();
                    }
                }
                b'@' => {
                    if !self.try_email_autolink() {
                        self.consume_char();
                    }
                }
                _ => self.consume_char(),
            }
        }
        self.flush();
        self.process_emphasis(NIL);
    }

    fn consume_char(&mut self) {
        let ch = self.content[self.pos..]
            .chars()
            .next()
            .unwrap_or('\u{FFFD}');
        if self.pending.is_empty() {
            self.pending_start = self.pos;
        }
        self.pending.push(ch);
        self.pos += ch.len_utf8();
    }

    fn handle_newline(&mut self) {
        // Trailing spaces decide hard vs soft break. A space only counts if it
        // is literal in the source: per spec §2.5 an entity reference cannot
        // stand in place of a character that defines structure, so the decoded
        // space in `a&#32; \n` leaves just one real space and a soft break.
        // `pending` holds decoded text, so intersect it with the source run.
        let src = &self.content.as_bytes()[..self.pos];
        let src_spaces = src.len() - src.iter().rposition(|&b| b != b' ').map_or(0, |i| i + 1);
        let pending_spaces = self.pending.len() - self.pending.trim_end_matches(' ').len();
        let n_spaces = src_spaces.min(pending_spaces);
        let hard = n_spaces >= 2;
        self.pending.truncate(self.pending.len() - n_spaces);
        let start = self.pos.saturating_sub(n_spaces);
        self.flush_at(start);
        let kind = if hard {
            InlineKind::HardBreak
        } else {
            InlineKind::SoftBreak
        };
        self.append_node(kind, start, self.pos + 1);
        self.pos += 1;
        // Leading spaces of the next line are ignored.
        while self.content.as_bytes().get(self.pos) == Some(&b' ') {
            self.pos += 1;
        }
        self.pending_start = self.pos;
    }

    fn handle_backslash(&mut self) {
        let next = self.content.as_bytes().get(self.pos + 1).copied();
        match next {
            Some(b'\n') => {
                self.flush();
                self.append_node(InlineKind::HardBreak, self.pos, self.pos + 2);
                self.pos += 2;
                while self.content.as_bytes().get(self.pos) == Some(&b' ') {
                    self.pos += 1;
                }
                self.pending_start = self.pos;
            }
            Some(c) if c.is_ascii_punctuation() => {
                if self.pending.is_empty() {
                    self.pending_start = self.pos;
                }
                self.pending.push(c as char);
                self.pos += 2;
            }
            _ => {
                self.push_pending("\\");
                self.pos += 1;
            }
        }
    }

    fn handle_backtick(&mut self) {
        let bytes = self.content.as_bytes();
        let start = self.pos;
        let mut end = start;
        while bytes.get(end) == Some(&b'`') {
            end += 1;
        }
        let len = end - start;
        if let Some(close) = self.find_matching_run(b'`', start, len) {
            let inner = &self.content[end..close];
            let mut text: String = inner.replace('\n', " ");
            if text.len() >= 2
                && text.starts_with(' ')
                && text.ends_with(' ')
                && !text.bytes().all(|b| b == b' ')
            {
                text = text[1..text.len() - 1].to_owned();
            }
            self.flush();
            self.append_node(InlineKind::Code(text), start, close + len);
            self.pos = close + len;
            self.pending_start = self.pos;
        } else {
            self.push_pending(&self.content[start..end]);
            self.pos = end;
        }
    }

    /// First run of `ch` with exactly `len` chars strictly after `start`.
    fn find_matching_run(&self, ch: u8, start: usize, len: usize) -> Option<usize> {
        let runs = self.runs.get(&ch)?;
        let from = runs.partition_point(|&(p, _)| p <= start);
        runs[from..]
            .iter()
            .find(|&&(_, l)| l == len)
            .map(|&(p, _)| p)
    }

    fn handle_entity(&mut self) {
        if let Some((len, decoded)) = scan::scan_entity(self.content, self.pos) {
            if self.pending.is_empty() {
                self.pending_start = self.pos;
            }
            self.pending.push_str(&decoded);
            self.pos += len;
        } else {
            self.push_pending("&");
            self.pos += 1;
        }
    }

    fn handle_lt(&mut self) {
        if let Some((end, dest, text, _email)) = scan::scan_autolink(self.content, self.pos) {
            self.flush();
            let child = Inline {
                id: NodeId(0),
                span: self.leaf.span(self.pos + 1, end - 1),
                kind: InlineKind::Text(text),
            };
            self.append_node(
                InlineKind::Link {
                    dest,
                    title: String::new(),
                    children: vec![child],
                },
                self.pos,
                end,
            );
            self.pos = end;
            self.pending_start = self.pos;
        } else if let Some(end) = scan::scan_html_tag(self.content, self.pos)
            .or_else(|| scan::scan_html_special(self.content, self.pos))
        {
            self.flush();
            self.append_node(
                InlineKind::HtmlInline(self.content[self.pos..end].to_owned()),
                self.pos,
                end,
            );
            self.pos = end;
            self.pending_start = self.pos;
        } else {
            self.push_pending("<");
            self.pos += 1;
        }
    }

    fn handle_dollar(&mut self) {
        if !self.cx.opts.math {
            self.consume_char();
            return;
        }
        let bytes = self.content.as_bytes();
        let start = self.pos;
        let mut end = start;
        while bytes.get(end) == Some(&b'$') {
            end += 1;
        }
        let len = end - start;
        let opener_ok = len <= 2
            && self.content[end..]
                .chars()
                .next()
                .is_some_and(|c| !c.is_whitespace());
        if opener_ok
            && let Some(close) = self.find_matching_run(b'$', start, len)
            && self.content[..close]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_whitespace())
            && close > end
        {
            let tex = self.content[end..close].to_owned();
            self.flush();
            self.append_node(
                InlineKind::Math {
                    display: len == 2,
                    tex,
                },
                start,
                close + len,
            );
            self.pos = close + len;
            self.pending_start = self.pos;
        } else {
            self.push_pending(&self.content[start..end]);
            self.pos = end;
        }
    }

    fn handle_delim(&mut self, ch: u8) {
        let bytes = self.content.as_bytes();
        let start = self.pos;
        let mut end = start;
        while bytes.get(end) == Some(&ch) {
            end += 1;
        }
        let len = end - start;
        let before = self.content[..start].chars().next_back();
        let after = self.content[end..].chars().next();
        let ws_before = before.is_none_or(scan_ws);
        let ws_after = after.is_none_or(scan_ws);
        let punct_before = before.is_some_and(scan::is_unicode_punctuation);
        let punct_after = after.is_some_and(scan::is_unicode_punctuation);
        let left_flanking = !ws_after && (!punct_after || ws_before || punct_before);
        let right_flanking = !ws_before && (!punct_before || ws_after || punct_after);
        let (can_open, can_close) = match ch {
            b'*' => (left_flanking, right_flanking),
            b'_' => (
                left_flanking && (!right_flanking || punct_before),
                right_flanking && (!left_flanking || punct_after),
            ),
            _ => (left_flanking, right_flanking), // '~'
        };
        // GFM: only runs of 1 or 2 tildes are strikethrough delimiters.
        let is_delim = (ch != b'~' || len <= 2) && (can_open || can_close);
        if !is_delim {
            // Literal run: keep it in pending so autolink lookback works.
            self.push_pending(&self.content[start..end]);
            self.pos = end;
            return;
        }
        self.flush();
        let node = self.append_node(
            InlineKind::Text(self.content[start..end].to_owned()),
            start,
            end,
        );
        {
            let idx = self.delims.len();
            self.delims.push(Delim {
                node,
                ch,
                orig_len: len,
                len,
                can_open,
                can_close,
                prev: self.delim_top,
                next: NIL,
                alive: true,
            });
            if self.delim_top != NIL {
                self.delims[self.delim_top as usize].next = idx as i32;
            }
            self.delim_top = idx as i32;
        }
        self.pos = end;
        self.pending_start = self.pos;
    }

    // --- brackets and links ---

    fn push_bracket(&mut self, node: usize, image: bool, label_start: usize) {
        if self.brackets.len() >= MAX_BRACKET_DEPTH {
            // Cap: the opener stays literal text; no bracket entry.
            return;
        }
        self.brackets.push(Bracket {
            node,
            image,
            active: true,
            delim_bottom: self.delim_top,
            label_start,
        });
    }

    fn handle_close_bracket(&mut self) {
        self.flush();
        let Some(bracket) = self.brackets.pop() else {
            self.push_pending("]");
            self.pos += 1;
            return;
        };
        if !bracket.active {
            self.push_pending("]");
            self.pos += 1;
            return;
        }
        let after = self.pos + 1;
        let raw_label = &self.content[bracket.label_start..self.pos];

        // Footnote reference?
        if !bracket.image
            && let Some(fl) = raw_label.strip_prefix('^')
            && !fl.is_empty()
            && !fl.contains(char::is_whitespace)
            && (self.cx.footnotes)(fl)
        {
            let start = self.nodes[bracket.node].c_start;
            // Drop the opener node and any label text nodes after it.
            self.drop_from(bracket.node);
            self.append_node(
                InlineKind::FootnoteReference {
                    label: fl.to_owned(),
                },
                start,
                after,
            );
            self.pos = after;
            self.pending_start = self.pos;
            return;
        }

        // 1. Inline link `(dest "title")`?
        let mut matched: Option<(String, String, usize)> = None;
        if self.content.as_bytes().get(after) == Some(&b'(') {
            matched = self.scan_inline_link(after);
        }
        // 2. Reference link?
        if matched.is_none() {
            let (label, link_end) = match self.content.as_bytes().get(after) {
                Some(b'[') => match scan::scan_link_label(self.content, after) {
                    Some((end, l)) if !l.trim().is_empty() => (l.to_owned(), end),
                    Some((end, _)) => (raw_label.to_owned(), end), // collapsed `[]`
                    None => (raw_label.to_owned(), after),
                },
                _ => (raw_label.to_owned(), after),
            };
            if label.chars().count() <= 999
                && let Some((dest, title)) = self.cx.refmap.get(&scan::normalize_label(&label))
            {
                matched = Some((dest.clone(), title.clone(), link_end));
            }
        }

        let Some((dest, title, link_end)) = matched else {
            // One attempt per opener: the bracket is gone for good.
            self.push_pending("]");
            self.pos += 1;
            return;
        };

        // Resolve emphasis inside the label, then collect children.
        self.process_emphasis(bracket.delim_bottom);
        let children = self.extract_after(bracket.node);
        let start = self.nodes[bracket.node].c_start;
        self.remove_node(bracket.node);
        let kind = if bracket.image {
            InlineKind::Image {
                dest,
                title,
                children,
            }
        } else {
            InlineKind::Link {
                dest,
                title,
                children,
            }
        };
        self.append_node(kind, start, link_end);
        if !bracket.image {
            for b in &mut self.brackets {
                if !b.image {
                    b.active = false;
                }
            }
        }
        self.pos = link_end;
        self.pending_start = self.pos;
    }

    /// Parse `(dest "title")` at `open` (which points at `(`).
    /// Returns (dest, title, end offset past `)`).
    fn scan_inline_link(&self, open: usize) -> Option<(String, String, usize)> {
        let bytes = self.content.as_bytes();
        let mut i = open + 1;
        i = skip_ws(bytes, i);
        let (dest, mut i) = if bytes.get(i) == Some(&b')') {
            (String::new(), i)
        } else {
            match scan::scan_link_destination(self.content, i) {
                Some((end, raw)) => (scan::unescape_string(raw), end),
                None => (String::new(), i),
            }
        };
        let before_ws = i;
        i = skip_ws(bytes, i);
        let mut title = String::new();
        if i > before_ws
            && let Some((end, raw)) = scan::scan_link_title(self.content, i)
        {
            title = scan::unescape_string(raw);
            i = skip_ws(bytes, end);
        }
        if bytes.get(i) == Some(&b')') {
            Some((dest, title, i + 1))
        } else {
            None
        }
    }

    /// Unlink and assemble every node after `node` (to list end).
    fn extract_after(&mut self, node: usize) -> Vec<Inline> {
        let mut out = Vec::new();
        let mut cur = self.nodes[node].next;
        while cur != NIL {
            let idx = cur as usize;
            cur = self.nodes[idx].next;
            let inl = self.take_inline(idx);
            out.push(inl);
        }
        self.nodes[node].next = NIL;
        self.tail = node as i32;
        merge_text(out)
    }

    /// Drop (unlink) `node` and everything after it.
    fn drop_from(&mut self, node: usize) {
        let mut cur = node as i32;
        let prev = self.nodes[node].prev;
        while cur != NIL {
            let idx = cur as usize;
            cur = self.nodes[idx].next;
            self.nodes[idx].alive = false;
        }
        if prev != NIL {
            self.nodes[prev as usize].next = NIL;
        } else {
            self.head = NIL;
        }
        self.tail = prev;
    }

    fn take_inline(&mut self, idx: usize) -> Inline {
        let n = &mut self.nodes[idx];
        n.alive = false;
        Inline {
            id: NodeId(0),
            span: self.leaf.span(n.c_start, n.c_end),
            kind: std::mem::replace(&mut n.kind, InlineKind::SoftBreak),
        }
    }

    // --- emphasis ---

    /// cmark's process_emphasis with the openers_bottom optimization.
    fn process_emphasis(&mut self, stack_bottom: i32) {
        // openers_bottom[closer_len % 3][char class]
        let mut openers_bottom = [[stack_bottom; 3]; 3];
        // Find the first delimiter above stack_bottom.
        let mut closer = {
            let mut d = self.delim_top;
            let mut first = NIL;
            while d != NIL && d > stack_bottom {
                first = d;
                d = self.delims[d as usize].prev;
            }
            first
        };
        while closer != NIL {
            let c = closer as usize;
            if !self.delims[c].alive || !self.delims[c].can_close {
                closer = self.delims[c].next;
                continue;
            }
            let ch = self.delims[c].ch;
            let class = delim_class(ch);
            let bottom = openers_bottom[self.delims[c].orig_len % 3][class];
            let mut opener = self.delims[c].prev;
            let mut opener_found = false;
            while opener != NIL && opener > stack_bottom && opener > bottom {
                let o = opener as usize;
                if self.delims[o].alive && self.delims[o].can_open && self.delims[o].ch == ch {
                    let odd = (self.delims[c].can_open || self.delims[o].can_close)
                        && (self.delims[o].orig_len + self.delims[c].orig_len).is_multiple_of(3)
                        && !(self.delims[o].orig_len.is_multiple_of(3)
                            && self.delims[c].orig_len.is_multiple_of(3));
                    let tilde_mismatch =
                        ch == b'~' && self.delims[o].orig_len != self.delims[c].orig_len;
                    if !odd && !tilde_mismatch {
                        opener_found = true;
                        break;
                    }
                }
                opener = self.delims[o].prev;
            }
            let old_closer = closer;
            if opener_found {
                closer = self.insert_emph(opener as usize, c);
            } else {
                closer = self.delims[c].next;
            }
            if !opener_found {
                let oc = old_closer as usize;
                openers_bottom[self.delims[oc].orig_len % 3][class] = self.delims[oc].prev;
                if !self.delims[oc].can_open {
                    self.remove_delim(oc);
                }
            }
        }
        // Remove all delimiters above stack_bottom.
        let mut d = self.delim_top;
        while d != NIL && d > stack_bottom {
            let prev = self.delims[d as usize].prev;
            self.remove_delim(d as usize);
            d = prev;
        }
    }

    fn remove_delim(&mut self, idx: usize) {
        if !self.delims[idx].alive {
            return;
        }
        let (prev, next) = (self.delims[idx].prev, self.delims[idx].next);
        if prev != NIL {
            self.delims[prev as usize].next = next;
        }
        if next != NIL {
            self.delims[next as usize].prev = prev;
        } else {
            self.delim_top = prev;
        }
        self.delims[idx].alive = false;
    }

    /// Create an Emph/Strong/Strikethrough from `opener`..`closer` runs.
    /// Returns the next closer to consider.
    fn insert_emph(&mut self, opener: usize, closer: usize) -> i32 {
        let ch = self.delims[closer].ch;
        let use_delims = if ch == b'~' {
            self.delims[closer].len
        } else if self.delims[closer].len >= 2 && self.delims[opener].len >= 2 {
            2
        } else {
            1
        };
        let opener_node = self.delims[opener].node;
        let closer_node = self.delims[closer].node;

        // Shrink the two text runs.
        self.delims[opener].len -= use_delims;
        self.delims[closer].len -= use_delims;
        let (o_start, o_end) = {
            let n = &mut self.nodes[opener_node];
            n.c_end -= use_delims;
            if let InlineKind::Text(t) = &mut n.kind {
                t.truncate(t.len() - use_delims);
            }
            (n.c_end, n.c_end + use_delims)
        };
        let (c_start, c_end) = {
            let n = &mut self.nodes[closer_node];
            n.c_start += use_delims;
            if let InlineKind::Text(t) = &mut n.kind {
                *t = t[use_delims..].to_owned();
            }
            (n.c_start - use_delims, n.c_start)
        };
        let _ = (o_start, c_end);

        // Remove delimiters between opener and closer.
        let mut d = self.delims[closer].prev;
        while d != NIL && d as usize != opener {
            let prev = self.delims[d as usize].prev;
            self.remove_delim(d as usize);
            d = prev;
        }

        // Collect nodes strictly between the two text runs.
        let mut children = Vec::new();
        let mut cur = self.nodes[opener_node].next;
        while cur != NIL && cur as usize != closer_node {
            let idx = cur as usize;
            cur = self.nodes[idx].next;
            children.push(self.take_inline(idx));
        }
        let children = merge_text(children);
        let kind = match (ch, use_delims) {
            (b'~', _) => InlineKind::Strikethrough(children),
            (_, 2) => InlineKind::Strong(children),
            _ => InlineKind::Emph(children),
        };
        // Splice: opener_node -> emph -> closer_node.
        let emph = self.nodes.len();
        self.nodes.push(INode {
            kind,
            c_start: o_start,
            c_end,
            prev: opener_node as i32,
            next: closer_node as i32,
            alive: true,
        });
        let _ = o_end;
        let _ = c_start;
        self.nodes[opener_node].next = emph as i32;
        self.nodes[closer_node].prev = emph as i32;

        // Drop emptied runs.
        if self.delims[opener].len == 0 {
            self.remove_node(opener_node);
            self.remove_delim(opener);
        }
        if self.delims[closer].len == 0 {
            self.remove_node(closer_node);
            let next = self.delims[closer].next;
            self.remove_delim(closer);
            next
        } else {
            closer as i32
        }
    }

    // --- GFM autolink literals ---

    fn boundary_before(&self, pos: usize) -> bool {
        self.content[..pos]
            .chars()
            .next_back()
            .is_none_or(|c| c.is_whitespace() || matches!(c, '*' | '_' | '~' | '('))
    }

    fn try_www_autolink(&mut self) -> bool {
        if !self.cx.opts.autolinks {
            return false;
        }
        let rest = &self.content[self.pos..];
        let rb = rest.as_bytes();
        if !(rb.len() >= 4 && rb[..4].eq_ignore_ascii_case(b"www.")) {
            return false;
        }
        if !self.boundary_before(self.pos) {
            return false;
        }
        let Some(len) = scan_autolink_tail(rest, 0) else {
            return false;
        };
        let text = &self.content[self.pos..self.pos + len];
        self.emit_autolink(self.pos, len, format!("http://{text}"));
        true
    }

    fn try_scheme_autolink(&mut self) -> bool {
        if !self.cx.opts.autolinks {
            return false;
        }
        let rest = &self.content[self.pos..];
        if !rest.starts_with("://") {
            return false;
        }
        // Look back for the scheme in pending text, and verify the same
        // bytes sit in the content right before `pos` (pending may hold
        // entity-decoded text whose length differs from the source).
        let cb = self.content.as_bytes();
        // GFM extended autolink schemes: http, https, ftp (gfm-spec §6.9).
        let scheme_len = ["https", "http", "ftp"]
            .iter()
            .find(|s| {
                self.pending.to_ascii_lowercase().ends_with(*s)
                    && self.pos >= s.len()
                    && cb[self.pos - s.len()..self.pos].eq_ignore_ascii_case(s.as_bytes())
                    && self.boundary_before(self.pos - s.len())
            })
            .map(|s| s.len());
        let Some(scheme_len) = scheme_len else {
            return false;
        };
        let start = self.pos - scheme_len;
        let after_slashes = 3; // "://"
        let Some(tail) = scan_autolink_tail(&self.content[self.pos + after_slashes..], 0) else {
            return false;
        };
        if tail == 0 {
            return false;
        }
        // Rewind the scheme out of pending.
        self.pending.truncate(self.pending.len() - scheme_len);
        let len = scheme_len + after_slashes + tail;
        let text = self.content[start..start + len].to_owned();
        self.pos = start;
        self.emit_autolink(start, len, text);
        true
    }

    fn try_email_autolink(&mut self) -> bool {
        if !self.cx.opts.autolinks {
            return false;
        }
        // Look back for the local part in pending.
        let local_len = self
            .pending
            .bytes()
            .rev()
            .take_while(|&c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'+' | b'-'))
            .count();
        if local_len == 0 || local_len > self.pos {
            return false;
        }
        let start = self.pos - local_len;
        // The pending tail must be the literal content bytes (an entity-
        // decoded local part would misalign the offsets).
        if self.content.as_bytes()[start..self.pos]
            != self.pending.as_bytes()[self.pending.len() - local_len..]
        {
            return false;
        }
        if !self.boundary_before(start) {
            return false;
        }
        // Forward: domain with at least one dot; segments of alphanumerics,
        // `-`, `_`; may not end with `-` or `_`; trailing `.`/`-`/`_` split off.
        let after = &self.content[self.pos + 1..];
        let dlen = after
            .bytes()
            .take_while(|&c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_'))
            .count();
        let mut domain = &after[..dlen];
        while domain.ends_with('.') {
            domain = &domain[..domain.len() - 1];
        }
        if domain.is_empty() || !domain.contains('.') || domain.starts_with('.') {
            return false;
        }
        // The last character may not be `-` or `_` (and they are not
        // trimmable: their presence invalidates the whole match).
        if domain.ends_with(['-', '_']) {
            return false;
        }
        self.pending.truncate(self.pending.len() - local_len);
        let len = local_len + 1 + domain.len();
        let text = self.content[start..start + len].to_owned();
        self.pos = start;
        self.emit_autolink(start, len, format!("mailto:{text}"));
        true
    }

    fn emit_autolink(&mut self, start: usize, len: usize, dest: String) {
        self.flush();
        let text = self.content[start..start + len].to_owned();
        let child = Inline {
            id: NodeId(0),
            span: self.leaf.span(start, start + len),
            kind: InlineKind::Text(text),
        };
        self.append_node(
            InlineKind::Link {
                dest,
                title: String::new(),
                children: vec![child],
            },
            start,
            start + len,
        );
        self.pos = start + len;
        self.pending_start = self.pos;
    }

    // --- assembly ---

    fn assemble(&mut self) -> Vec<Inline> {
        let mut out = Vec::new();
        let mut cur = self.head;
        while cur != NIL {
            let idx = cur as usize;
            cur = self.nodes[idx].next;
            if self.nodes[idx].alive {
                out.push(self.take_inline(idx));
            }
        }
        merge_text(out)
    }
}

fn delim_class(ch: u8) -> usize {
    match ch {
        b'*' => 0,
        b'_' => 1,
        _ => 2,
    }
}

fn scan_ws(c: char) -> bool {
    scan::is_unicode_whitespace(c)
}

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while matches!(bytes.get(i), Some(b' ') | Some(b'\t') | Some(b'\n')) {
        i += 1;
    }
    i
}

fn is_escaped(bytes: &[u8], pos: usize) -> bool {
    let mut n = 0;
    let mut i = pos;
    while i > 0 && bytes[i - 1] == b'\\' {
        n += 1;
        i -= 1;
    }
    n % 2 == 1
}

/// Merge adjacent Text nodes (span = union).
fn merge_text(items: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(items.len());
    for item in items {
        if let (Some(last), InlineKind::Text(t)) = (out.last_mut(), &item.kind)
            && let InlineKind::Text(prev) = &mut last.kind
        {
            prev.push_str(t);
            last.span.end = item.span.end;
            continue;
        }
        out.push(item);
    }
    out
}

/// GFM autolink tail: consume a www/url body starting at `from` (which sits
/// at the `www.`/domain start), applying domain validation and trailing
/// punctuation trimming. Returns the matched length.
fn scan_autolink_tail(s: &str, from: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    // Domain: alphanumerics, `-`, `_`, `.`.
    let mut i = from;
    while i < bytes.len()
        && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'-' | b'_' | b'.'))
    {
        i += 1;
    }
    let domain = &s[from..i];
    if !domain.contains('.') {
        return None;
    }
    // No underscores in the last two segments.
    let segs: Vec<&str> = domain.split('.').collect();
    let tail_segs = &segs[segs.len().saturating_sub(2)..];
    if tail_segs
        .iter()
        .any(|seg| seg.contains('_') || seg.is_empty())
    {
        // A trailing dot is allowed if it gets trimmed below; treat a final
        // empty segment as trimmable, others fail.
        let non_final_bad = segs[..segs.len() - 1]
            .iter()
            .skip(segs.len().saturating_sub(3))
            .any(|seg| seg.contains('_') || seg.is_empty());
        let final_seg = segs[segs.len() - 1];
        if non_final_bad || (!final_seg.is_empty() && final_seg.contains('_')) {
            return None;
        }
    }
    // Path: until whitespace or `<`.
    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'<' {
        i += 1;
    }
    // Trailing punctuation trimming.
    loop {
        let t = &s[from..i];
        let Some(last) = t.chars().next_back() else {
            break;
        };
        match last {
            '?' | '!' | '.' | ',' | ':' | '*' | '_' | '~' | '\'' | '"' => i -= last.len_utf8(),
            ')' => {
                let opens = t.matches('(').count();
                let closes = t.matches(')').count();
                if closes > opens {
                    i -= 1;
                } else {
                    break;
                }
            }
            ';' => {
                // Strip a trailing entity-like `&xyz;`.
                if let Some(amp) = t.rfind('&') {
                    if t[amp + 1..t.len() - 1]
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric())
                        && t.len() - amp >= 3
                    {
                        i = from + amp;
                    } else {
                        i -= 1;
                    }
                } else {
                    i -= 1;
                }
            }
            _ => break,
        }
    }
    if i <= from { None } else { Some(i - from) }
}
