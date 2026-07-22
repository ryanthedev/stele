//! AST→HTML serializer — **conformance-harness shim only**.
//!
//! Exists so the parser can be measured against the vendored CommonMark and
//! GFM spec tests, which are stated as markdown→HTML pairs. It mirrors
//! cmark's html.c output byte-for-byte for the constructs the suites cover.
//! It is *not* a product surface: stele never renders HTML.
//!
//! Iterative (explicit work stack) — safe on hostile nesting.

use crate::Document;
use crate::ast::{Alignment, Block, BlockKind, Inline, InlineKind, ListKind};

/// Render the document as cmark-compatible HTML.
pub fn to_html(doc: &Document) -> String {
    let mut out = Out::default();
    let mut work: Vec<W<'_>> = doc
        .blocks()
        .iter()
        .rev()
        .map(|b| W::Block(b, false))
        .collect();
    while let Some(w) = work.pop() {
        step(w, &mut work, &mut out);
    }
    out.buf
}

#[derive(Default)]
struct Out {
    buf: String,
}

impl Out {
    /// cmark's `cr()`: newline unless the buffer already ends with one.
    fn cr(&mut self) {
        if !self.buf.is_empty() && !self.buf.ends_with('\n') {
            self.buf.push('\n');
        }
    }
    fn lit(&mut self, s: &str) {
        self.buf.push_str(s);
    }
    fn esc(&mut self, s: &str) {
        escape_html(s, &mut self.buf);
    }
}

enum W<'a> {
    Block(&'a Block, bool),
    /// (text, cr_before, cr_after)
    Close(&'static str, bool, bool),
    Inline(&'a Inline),
    Inlines(&'a [Inline]),
    Lit(&'a str),
}

fn step<'a>(w: W<'a>, work: &mut Vec<W<'a>>, out: &mut Out) {
    match w {
        W::Lit(s) => out.lit(s),
        W::Close(s, cr_before, cr_after) => {
            if cr_before {
                out.cr();
            }
            out.lit(s);
            if cr_after {
                out.cr();
            }
        }
        W::Inlines(list) => {
            for i in list.iter().rev() {
                work.push(W::Inline(i));
            }
        }
        W::Block(b, tight) => block(b, tight, work, out),
        W::Inline(i) => inline(i, work, out),
    }
}

fn block<'a>(b: &'a Block, tight: bool, work: &mut Vec<W<'a>>, out: &mut Out) {
    match &b.kind {
        BlockKind::Paragraph { children } => {
            if tight {
                work.push(W::Inlines(children));
            } else {
                out.cr();
                out.lit("<p>");
                work.push(W::Close("</p>", false, true));
                work.push(W::Inlines(children));
            }
        }
        BlockKind::Heading { level, children } => {
            out.cr();
            let (open, close) = heading_tags(*level);
            out.lit(open);
            work.push(W::Close(close, false, true));
            work.push(W::Inlines(children));
        }
        BlockKind::ThematicBreak => {
            out.cr();
            out.lit("<hr />");
            out.cr();
        }
        BlockKind::BlockQuote { children } => {
            out.cr();
            out.lit("<blockquote>\n");
            work.push(W::Close("</blockquote>", true, true));
            for c in children.iter().rev() {
                work.push(W::Block(c, false));
            }
        }
        BlockKind::List { kind, tight, items } => {
            out.cr();
            let close = match kind {
                ListKind::Bullet(_) => {
                    out.lit("<ul>\n");
                    "</ul>"
                }
                ListKind::Ordered { start, .. } => {
                    if *start == 1 {
                        out.lit("<ol>\n");
                    } else {
                        out.lit(&format!("<ol start=\"{start}\">\n"));
                    }
                    "</ol>"
                }
            };
            work.push(W::Close(close, true, true));
            for item in items.iter().rev() {
                work.push(W::Block(item, *tight));
            }
        }
        BlockKind::ListItem { checked, children } => {
            out.cr();
            out.lit("<li>");
            work.push(W::Close("</li>", false, true));
            // GFM task checkbox precedes the first paragraph's content.
            let checkbox: Option<&'static str> = checked.map(|c| {
                if c {
                    "<input checked=\"\" disabled=\"\" type=\"checkbox\"> "
                } else {
                    "<input disabled=\"\" type=\"checkbox\"> "
                }
            });
            let mut rest: Vec<W<'a>> = Vec::new();
            for (i, c) in children.iter().enumerate() {
                if i == 0
                    && let Some(cb) = checkbox
                {
                    if tight {
                        rest.push(W::Lit(cb));
                        rest.push(W::Block(c, tight));
                        continue;
                    } else if let BlockKind::Paragraph { children } = &c.kind {
                        rest.push(W::Lit("\n<p>"));
                        rest.push(W::Lit(cb));
                        rest.push(W::Inlines(children));
                        rest.push(W::Lit("</p>\n"));
                        continue;
                    }
                }
                rest.push(W::Block(c, tight));
            }
            for r in rest.into_iter().rev() {
                work.push(r);
            }
        }
        BlockKind::CodeBlock { info, literal, .. } => {
            out.cr();
            match info
                .as_deref()
                .map(|i| i.split_whitespace().next().unwrap_or(""))
            {
                Some(lang) if !lang.is_empty() => {
                    out.lit("<pre><code class=\"language-");
                    out.esc(lang);
                    out.lit("\">");
                }
                _ => out.lit("<pre><code>"),
            }
            out.esc(literal);
            out.lit("</code></pre>");
            out.cr();
        }
        BlockKind::HtmlBlock { literal } => {
            out.cr();
            out.lit(literal);
            out.cr();
        }
        BlockKind::Table { alignments, rows } => {
            // Self-contained: rendered eagerly (cells hold only inlines).
            out.cr();
            out.lit("<table>\n");
            let render_row = |r: &Block, tag: &str, out: &mut Out| {
                let BlockKind::TableRow { cells, .. } = &r.kind else {
                    return;
                };
                out.lit("<tr>\n");
                for (ci, cell) in cells.iter().enumerate() {
                    let BlockKind::TableCell { children } = &cell.kind else {
                        continue;
                    };
                    let align = match alignments.get(ci) {
                        Some(Alignment::Left) => " align=\"left\"",
                        Some(Alignment::Center) => " align=\"center\"",
                        Some(Alignment::Right) => " align=\"right\"",
                        _ => "",
                    };
                    out.lit(&format!("<{tag}{align}>"));
                    out.lit(&inlines_to_html(children));
                    out.lit(&format!("</{tag}>\n"));
                }
                out.lit("</tr>\n");
            };
            let mut body_seen = false;
            for r in rows {
                let header = matches!(r.kind, BlockKind::TableRow { header: true, .. });
                if header {
                    out.lit("<thead>\n");
                    render_row(r, "th", out);
                    out.lit("</thead>\n");
                } else {
                    if !body_seen {
                        out.lit("<tbody>\n");
                        body_seen = true;
                    }
                    render_row(r, "td", out);
                }
            }
            if body_seen {
                out.lit("</tbody>\n");
            }
            out.lit("</table>");
            out.cr();
        }
        BlockKind::TableRow { .. } | BlockKind::TableCell { .. } => {
            // Rendered inside the Table arm; unreachable at top level.
        }
        BlockKind::FootnoteDefinition { label, children } => {
            out.cr();
            out.lit("<div class=\"footnote-definition\" id=\"fn-");
            out.esc(label);
            out.lit("\">");
            work.push(W::Close("</div>", true, true));
            for c in children.iter().rev() {
                work.push(W::Block(c, false));
            }
        }
        BlockKind::Alert { kind, children } => {
            out.cr();
            let name = match kind {
                crate::AlertKind::Note => "note",
                crate::AlertKind::Tip => "tip",
                crate::AlertKind::Important => "important",
                crate::AlertKind::Warning => "warning",
                crate::AlertKind::Caution => "caution",
            };
            out.lit(&format!("<div class=\"alert alert-{name}\">\n"));
            work.push(W::Close("</div>", true, true));
            for c in children.iter().rev() {
                work.push(W::Block(c, false));
            }
        }
        BlockKind::FrontMatter { .. } => {}
    }
}

fn heading_tags(level: u8) -> (&'static str, &'static str) {
    match level {
        1 => ("<h1>", "</h1>"),
        2 => ("<h2>", "</h2>"),
        3 => ("<h3>", "</h3>"),
        4 => ("<h4>", "</h4>"),
        5 => ("<h5>", "</h5>"),
        _ => ("<h6>", "</h6>"),
    }
}

/// Render a run of inlines to a standalone string (table cells).
fn inlines_to_html(inlines: &[Inline]) -> String {
    let mut out = Out::default();
    let mut work: Vec<W<'_>> = vec![W::Inlines(inlines)];
    while let Some(w) = work.pop() {
        step(w, &mut work, &mut out);
    }
    out.buf
}

fn inline<'a>(i: &'a Inline, work: &mut Vec<W<'a>>, out: &mut Out) {
    match &i.kind {
        InlineKind::Text(t) => out.esc(t),
        InlineKind::SoftBreak => out.lit("\n"),
        InlineKind::HardBreak => out.lit("<br />\n"),
        InlineKind::Code(t) => {
            out.lit("<code>");
            out.esc(t);
            out.lit("</code>");
        }
        InlineKind::HtmlInline(t) => out.lit(t),
        InlineKind::Emph(children) => {
            out.lit("<em>");
            work.push(W::Close("</em>", false, false));
            work.push(W::Inlines(children));
        }
        InlineKind::Strong(children) => {
            out.lit("<strong>");
            work.push(W::Close("</strong>", false, false));
            work.push(W::Inlines(children));
        }
        InlineKind::Strikethrough(children) => {
            out.lit("<del>");
            work.push(W::Close("</del>", false, false));
            work.push(W::Inlines(children));
        }
        InlineKind::Link {
            dest,
            title,
            children,
        } => {
            out.lit("<a href=\"");
            escape_href(dest, &mut out.buf);
            out.lit("\"");
            if !title.is_empty() {
                out.lit(" title=\"");
                out.esc(title);
                out.lit("\"");
            }
            out.lit(">");
            work.push(W::Close("</a>", false, false));
            work.push(W::Inlines(children));
        }
        InlineKind::Image {
            dest,
            title,
            children,
        } => {
            out.lit("<img src=\"");
            escape_href(dest, &mut out.buf);
            out.lit("\" alt=\"");
            let mut alt = String::new();
            plain_text(children, &mut alt);
            escape_html(&alt, &mut out.buf);
            out.lit("\"");
            if !title.is_empty() {
                out.lit(" title=\"");
                out.esc(title);
                out.lit("\"");
            }
            out.lit(" />");
        }
        InlineKind::Math { display, tex } => {
            out.lit(if *display {
                "<span class=\"math display\">"
            } else {
                "<span class=\"math inline\">"
            });
            out.esc(tex);
            out.lit("</span>");
        }
        InlineKind::FootnoteReference { label } => {
            out.lit("<sup class=\"footnote-reference\"><a href=\"#fn-");
            escape_href(label, &mut out.buf);
            out.lit("\">");
            out.esc(label);
            out.lit("</a></sup>");
        }
    }
}

/// Plain-text rendering of inlines (image alt text). Iterative.
fn plain_text(inlines: &[Inline], out: &mut String) {
    let mut work: Vec<&Inline> = inlines.iter().rev().collect();
    while let Some(i) = work.pop() {
        match &i.kind {
            InlineKind::Text(t) | InlineKind::Code(t) => out.push_str(t),
            InlineKind::SoftBreak | InlineKind::HardBreak => out.push(' '),
            InlineKind::HtmlInline(_) => {}
            InlineKind::Math { tex, .. } => out.push_str(tex),
            InlineKind::FootnoteReference { label } => out.push_str(label),
            InlineKind::Emph(c)
            | InlineKind::Strong(c)
            | InlineKind::Strikethrough(c)
            | InlineKind::Link { children: c, .. }
            | InlineKind::Image { children: c, .. } => {
                for x in c.iter().rev() {
                    work.push(x);
                }
            }
        }
    }
}

/// cmark's houdini_escape_html: `&` `<` `>` `"`.
pub(crate) fn escape_html(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}

/// cmark's houdini_escape_href: URL-ish characters pass; `&` and `'` become
/// entities; everything else is %XX-encoded per UTF-8 byte.
pub(crate) fn escape_href(s: &str, out: &mut String) {
    const SAFE: &[u8] = b"-_.+!*(),%#@?=;:/$~";
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || SAFE.contains(&b) {
            out.push(b as char);
        } else if b == b'&' {
            out.push_str("&amp;");
        } else if b == b'\'' {
            out.push_str("&#x27;");
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
}
