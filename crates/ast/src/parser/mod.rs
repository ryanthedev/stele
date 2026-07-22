//! Parser internals: two phases, exactly as the spec's appendix describes.
//!
//! Phase 1 ([`block`]) consumes the source line by line, maintaining a stack
//! of open blocks, and produces a tree of [`PreBlock`]s whose leaves hold raw
//! text plus a content→source offset map. Link reference definitions are
//! collected into a [`RefMap`] as paragraphs close.
//!
//! Phase 2 ([`inline`]) runs after the whole document is blocked (so
//! references defined *after* use resolve), parsing each leaf's raw text
//! into [`Inline`] trees with the delimiter-stack algorithm.
//!
//! The conversion walk in this module stitches the phases together
//! iteratively (explicit stack — hostile nesting cannot overflow the call
//! stack here).

use crate::ast::{AlertKind, Alignment, Block, BlockKind, ListKind, NodeId, Span};
use std::collections::HashMap;

mod block;
mod entities;
mod inline;
pub(crate) mod scan;
mod tables;

/// Maximum container-block nesting depth. Markers past the cap are treated
/// as literal text; conformance inputs nest < 20.
pub(crate) const MAX_CONTAINER_DEPTH: usize = 200;

/// Extension toggles. `Default` enables everything (stele's product
/// configuration); [`ParseOptions::commonmark`] disables all extensions for
/// pure CommonMark 0.31.2 parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseOptions {
    /// GFM tables.
    pub tables: bool,
    /// GFM strikethrough (`~~`).
    pub strikethrough: bool,
    /// GFM task-list items (`- [x]`).
    pub task_lists: bool,
    /// GFM autolink literals (www./http(s)/email in plain text).
    pub autolinks: bool,
    /// `$…$` / `$$…$$` math spans.
    pub math: bool,
    /// Footnote definitions and references.
    pub footnotes: bool,
    /// GitHub alerts (`> [!NOTE]`).
    pub alerts: bool,
    /// YAML frontmatter at the document head.
    pub frontmatter: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        ParseOptions {
            tables: true,
            strikethrough: true,
            task_lists: true,
            autolinks: true,
            math: true,
            footnotes: true,
            alerts: true,
            frontmatter: true,
        }
    }
}

impl ParseOptions {
    /// Pure CommonMark 0.31.2: every extension off.
    pub fn commonmark() -> Self {
        ParseOptions {
            tables: false,
            strikethrough: false,
            task_lists: false,
            autolinks: false,
            math: false,
            footnotes: false,
            alerts: false,
            frontmatter: false,
        }
    }
}

/// One segment of the content→source map: `c_len` bytes of leaf content
/// starting at `c_start` correspond to `s_len` source bytes at `s_start`.
/// When `c_len == s_len` the mapping is byte-for-byte; otherwise (tab
/// expansion, escaped pipes, line terminators) positions inside map to the
/// segment start.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Seg {
    pub c_start: usize,
    pub c_len: usize,
    pub s_start: usize,
    pub s_len: usize,
}

/// Raw text of a leaf block plus its source map.
#[derive(Debug, Clone, Default)]
pub(crate) struct Leaf {
    pub content: String,
    pub segs: Vec<Seg>,
}

impl Leaf {
    /// Append `content_bytes` (already extracted/normalized) that came from
    /// `src[s_start..s_start + s_len]`.
    pub fn push(&mut self, content_bytes: &str, s_start: usize, s_len: usize) {
        let c_start = self.content.len();
        self.content.push_str(content_bytes);
        self.segs.push(Seg {
            c_start,
            c_len: content_bytes.len(),
            s_start,
            s_len,
        });
    }

    /// Map a content offset to a source offset. `round_up` picks the end of
    /// a non-linear segment instead of its start (use for span ends).
    pub fn map(&self, c_pos: usize, round_up: bool) -> usize {
        let idx = self.segs.partition_point(|s| s.c_start + s.c_len <= c_pos);
        match self.segs.get(idx) {
            Some(s) => {
                if s.c_len == s.s_len {
                    s.s_start + (c_pos - s.c_start)
                } else if round_up && c_pos > s.c_start {
                    s.s_start + s.s_len
                } else {
                    s.s_start
                }
            }
            None => self.segs.last().map(|s| s.s_start + s.s_len).unwrap_or(0),
        }
    }

    /// Span covering content range `start..end` in source coordinates.
    pub fn span(&self, start: usize, end: usize) -> Span {
        Span::new(self.map(start, false), self.map(end, true))
    }

    /// Drop the first `n` content bytes (used when reference definitions are
    /// stripped from a paragraph head).
    pub fn drop_prefix(&mut self, n: usize) {
        self.content.drain(..n);
        self.segs.retain_mut(|s| {
            if s.c_start + s.c_len <= n {
                return false;
            }
            if s.c_start < n {
                let cut = n - s.c_start;
                s.c_start = 0;
                s.c_len -= cut;
                if s.s_len >= cut {
                    // linear segments shift; non-linear keep their source start
                    if s.c_len + cut == s.s_len {
                        s.s_start += cut;
                        s.s_len -= cut;
                    }
                }
            } else {
                s.c_start -= n;
            }
            true
        });
    }

    /// Trim trailing whitespace from the content (segs keep source ends).
    pub fn trim_end(&mut self) {
        let new_len = self.content.trim_end().len();
        self.content.truncate(new_len);
        self.segs.retain_mut(|s| {
            if s.c_start >= new_len {
                return false;
            }
            if s.c_start + s.c_len > new_len {
                let cut = s.c_start + s.c_len - new_len;
                s.c_len -= cut;
                if s.s_len >= cut && s.c_len + cut == s.s_len {
                    s.s_len -= cut;
                }
            }
            true
        });
    }

    /// Sub-leaf for content range `start..end` (used for table cells).
    pub fn slice(&self, start: usize, end: usize) -> Leaf {
        let mut out = Leaf::default();
        for s in &self.segs {
            let a = s.c_start.max(start);
            let b = (s.c_start + s.c_len).min(end);
            if a >= b {
                continue;
            }
            let (s_start, s_len) = if s.c_len == s.s_len {
                (s.s_start + (a - s.c_start), b - a)
            } else {
                (s.s_start, s.s_len)
            };
            out.segs.push(Seg {
                c_start: a - start,
                c_len: b - a,
                s_start,
                s_len,
            });
        }
        out.content = self.content[start..end].to_owned();
        out
    }
}

/// Reference definitions: normalized label → (destination, title).
pub(crate) type RefMap = HashMap<String, (String, String)>;

/// Block-phase output tree. Leaves carry [`Leaf`]s, not inlines, so the
/// inline phase can run once the [`RefMap`] is complete.
#[derive(Debug)]
pub(crate) struct PreBlock {
    pub span: Span,
    pub kind: PreKind,
    /// Did the last line this block received end blank? (cmark's
    /// last_line_blank; drives list tightness.)
    pub last_line_blank: bool,
}

#[derive(Debug)]
pub(crate) enum PreKind {
    Paragraph(Leaf),
    Heading {
        level: u8,
        leaf: Leaf,
    },
    ThematicBreak,
    BlockQuote(Vec<PreBlock>),
    Alert {
        kind: AlertKind,
        children: Vec<PreBlock>,
    },
    List {
        kind: ListKind,
        tight: bool,
        items: Vec<PreBlock>,
    },
    Item {
        checked: Option<bool>,
        children: Vec<PreBlock>,
    },
    CodeBlock {
        info: Option<String>,
        literal: String,
        fenced: bool,
    },
    HtmlBlock {
        literal: String,
    },
    Table {
        alignments: Vec<Alignment>,
        rows: Vec<PreRow>,
    },
    FootnoteDef {
        label: String,
        children: Vec<PreBlock>,
    },
    FrontMatter {
        literal: String,
    },
}

#[derive(Debug)]
pub(crate) struct PreRow {
    pub span: Span,
    pub header: bool,
    pub cells: Vec<(Span, Leaf)>,
}

/// Parse a document (already NUL-cleaned by `Document::parse_with`).
pub(crate) fn parse(src: &str, opts: &ParseOptions) -> Vec<Block> {
    let (pre, refmap, footnotes) = block::parse_blocks(src, opts);
    let cx = inline::InlineCx {
        refmap: &refmap,
        footnotes: &|label: &str| opts.footnotes && footnotes.contains(label),
        opts: opts.clone(),
    };
    pre.into_iter().map(|b| convert(b, &cx)).collect()
}

/// Convert one block-phase tree into the public AST, running the inline
/// phase on each leaf. Iterative: children are converted with an explicit
/// work stack, then reassembled.
fn convert(pre: PreBlock, cx: &inline::InlineCx) -> Block {
    // Post-order over an explicit stack: expand nodes, then rebuild.
    enum Work {
        Expand(PreBlock),
        Build {
            span: Span,
            shell: Shell,
            n_children: usize,
        },
    }
    /// A `PreKind` with its children removed (they travel through the stack).
    enum Shell {
        Paragraph(Leaf),
        Heading {
            level: u8,
            leaf: Leaf,
        },
        ThematicBreak,
        BlockQuote,
        Alert(AlertKind),
        List {
            kind: ListKind,
            tight: bool,
        },
        Item {
            checked: Option<bool>,
        },
        CodeBlock {
            info: Option<String>,
            literal: String,
            fenced: bool,
        },
        HtmlBlock(String),
        Table {
            alignments: Vec<Alignment>,
            rows: Vec<PreRow>,
        },
        FootnoteDef(String),
        FrontMatter(String),
    }

    let nil = NodeId(0); // all ids are assigned later by Document::number_nodes
    let mut work = vec![Work::Expand(pre)];
    let mut done: Vec<Block> = Vec::new();
    while let Some(item) = work.pop() {
        match item {
            Work::Expand(PreBlock { span, kind, .. }) => {
                let (shell, children) = match kind {
                    PreKind::Paragraph(leaf) => (Shell::Paragraph(leaf), vec![]),
                    PreKind::Heading { level, leaf } => (Shell::Heading { level, leaf }, vec![]),
                    PreKind::ThematicBreak => (Shell::ThematicBreak, vec![]),
                    PreKind::BlockQuote(c) => (Shell::BlockQuote, c),
                    PreKind::Alert { kind, children } => (Shell::Alert(kind), children),
                    PreKind::List { kind, tight, items } => (Shell::List { kind, tight }, items),
                    PreKind::Item { checked, children } => (Shell::Item { checked }, children),
                    PreKind::CodeBlock {
                        info,
                        literal,
                        fenced,
                    } => (
                        Shell::CodeBlock {
                            info,
                            literal,
                            fenced,
                        },
                        vec![],
                    ),
                    PreKind::HtmlBlock { literal } => (Shell::HtmlBlock(literal), vec![]),
                    PreKind::Table { alignments, rows } => {
                        (Shell::Table { alignments, rows }, vec![])
                    }
                    PreKind::FootnoteDef { label, children } => {
                        (Shell::FootnoteDef(label), children)
                    }
                    PreKind::FrontMatter { literal } => (Shell::FrontMatter(literal), vec![]),
                };
                work.push(Work::Build {
                    span,
                    shell,
                    n_children: children.len(),
                });
                // Children pushed in order; popped last-first, so reverse.
                for c in children.into_iter().rev() {
                    work.push(Work::Expand(c));
                }
            }
            Work::Build {
                span,
                shell,
                n_children,
            } => {
                let children: Vec<Block> = done.split_off(done.len() - n_children);
                let kind = match shell {
                    Shell::Paragraph(leaf) => BlockKind::Paragraph {
                        children: inline::parse_inlines_cx(&leaf, cx),
                    },
                    Shell::Heading { level, leaf } => BlockKind::Heading {
                        level,
                        children: inline::parse_inlines_cx(&leaf, cx),
                    },
                    Shell::ThematicBreak => BlockKind::ThematicBreak,
                    Shell::BlockQuote => BlockKind::BlockQuote { children },
                    Shell::Alert(kind) => BlockKind::Alert { kind, children },
                    Shell::List { kind, tight } => BlockKind::List {
                        kind,
                        tight,
                        items: children,
                    },
                    Shell::Item { checked } => BlockKind::ListItem { checked, children },
                    Shell::CodeBlock {
                        info,
                        literal,
                        fenced,
                    } => BlockKind::CodeBlock {
                        info,
                        literal,
                        fenced,
                    },
                    Shell::HtmlBlock(literal) => BlockKind::HtmlBlock { literal },
                    Shell::Table { alignments, rows } => BlockKind::Table {
                        alignments,
                        rows: rows
                            .into_iter()
                            .map(|row| Block {
                                id: nil,
                                span: row.span,
                                kind: BlockKind::TableRow {
                                    header: row.header,
                                    cells: row
                                        .cells
                                        .into_iter()
                                        .map(|(cspan, leaf)| Block {
                                            id: nil,
                                            span: cspan,
                                            kind: BlockKind::TableCell {
                                                children: inline::parse_inlines_cx(&leaf, cx),
                                            },
                                        })
                                        .collect(),
                                },
                            })
                            .collect(),
                    },
                    Shell::FootnoteDef(label) => BlockKind::FootnoteDefinition { label, children },
                    Shell::FrontMatter(literal) => BlockKind::FrontMatter { literal },
                };
                done.push(Block {
                    id: nil,
                    span,
                    kind,
                });
            }
        }
    }
    debug_assert_eq!(done.len(), 1);
    done.pop().unwrap_or(Block {
        id: nil,
        span: Span::new(0, 0),
        kind: BlockKind::Paragraph { children: vec![] },
    })
}
