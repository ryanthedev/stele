//! `crates/ast` — owned CommonMark 0.31.2 + GFM parser for stele.
//!
//! One entry point: [`Document::parse`]. It is **total** — any `&str`,
//! however hostile, produces a `Document`; the worst case is plain text.
//! Nodes carry byte-range [`Span`]s into the original source and stable,
//! dense [`NodeId`]s; downstream phases (layout sizing, media resolution)
//! address nodes by id and resolve them back via [`Document::node`].
//!
//! Hardening (input is untrusted):
//! - no `unsafe` anywhere (`forbid`), no panics on any input (fuzz-verified);
//! - U+0000 becomes U+FFFD in content (spans still address original bytes);
//! - container nesting, bracket nesting, and total tree depth are capped
//!   (see [`MAX_TREE_DEPTH`]) so *recursive consumers of the tree are safe*;
//!   past-cap constructs degrade to literal text, never to an error;
//! - parsing is linear-time on pathological delimiter/bracket nesting
//!   (`openers_bottom`, bracket deactivation, single pass over lines).
//!
//! The [`html`] module is a conformance-harness shim, not a product surface.

#![forbid(unsafe_code)]

mod ast;
/// Conformance-harness shim only — not a product surface. Hidden from docs so
/// it does not read as public API to downstream phases.
#[doc(hidden)]
pub mod html;
mod parser;

pub use ast::{
    AlertKind, Alignment, Block, BlockKind, Inline, InlineKind, ListKind, NodeId, NodeRef, Span,
};
pub use parser::ParseOptions;

/// Upper bound on the depth of any parsed tree, counting both block and
/// inline nesting. Recursing over a `Document` with stack frames bounded by
/// this constant is safe. Enforced by construction caps plus a flattening
/// pass; conformance inputs sit far below it.
pub const MAX_TREE_DEPTH: usize = 700;

/// A parsed markdown document: the owned source text, the block tree, and
/// the `NodeId` index.
#[derive(Debug, Clone)]
pub struct Document {
    source: String,
    blocks: Vec<Block>,
    /// Indexed by `NodeId`: (parent id or `u32::MAX` for roots, index of
    /// this node within its parent's children — or within `blocks` for
    /// roots). Supports `node()` resolution by climbing then descending.
    parents: Vec<(u32, u32)>,
}

impl Document {
    /// Parse untrusted markdown text with every extension enabled (the
    /// product configuration). Infallible: the worst case is plain text.
    pub fn parse(source: &str) -> Document {
        Document::parse_with(source, &ParseOptions::default())
    }

    /// Parse with explicit extension toggles.
    /// `ParseOptions::commonmark()` is pure CommonMark 0.31.2 — the profile
    /// the conformance suite measures.
    pub fn parse_with(source: &str, opts: &ParseOptions) -> Document {
        // U+0000 → U+FFFD per spec §2.3 (insecure characters). The cleaned
        // text becomes the document's source, so every span indexes it
        // consistently; NUL-free input (the normal case) is stored as-is.
        let source: String = if source.contains('\0') {
            source.replace('\0', "\u{FFFD}")
        } else {
            source.to_owned()
        };
        let blocks = parser::parse(&source, opts);
        let mut doc = Document {
            source,
            blocks,
            parents: Vec::new(),
        };
        doc.number_nodes();
        doc
    }

    /// The source text this document was parsed from. Identical to the
    /// `parse` input except that U+0000 bytes appear as U+FFFD; all node
    /// spans index into this string.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The top-level blocks in document order.
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Total number of nodes; `NodeId` indexes are dense in `0..node_count()`.
    pub fn node_count(&self) -> usize {
        self.parents.len()
    }

    /// Resolve a `NodeId` back to its node, or `None` if the id is out of
    /// range for this document.
    ///
    /// Ids are dense pre-order indices, not document-tagged: an id minted by a
    /// *different* document resolves to whatever node sits at that index here
    /// rather than to `None`. Ids are stable across re-parses of identical
    /// source; holding one across a re-parse of *edited* source is a bug.
    pub fn node(&self, id: NodeId) -> Option<NodeRef<'_>> {
        if id.index() >= self.parents.len() {
            return None;
        }
        // Climb to a root collecting child indices, then walk back down.
        let mut path = Vec::new();
        let mut cur = id.0;
        loop {
            let (parent, idx) = self.parents[cur as usize];
            path.push(idx as usize);
            if parent == u32::MAX {
                break;
            }
            cur = parent;
        }
        let mut it = path.iter().rev();
        let mut node = NodeRef::Block(self.blocks.get(*it.next()?)?);
        for &idx in it {
            node = node.child(idx)?;
        }
        Some(node)
    }

    /// Iterate every node in pre-order (document order). Iterative — safe
    /// on any parse result regardless of nesting.
    pub fn nodes(&self) -> impl Iterator<Item = NodeRef<'_>> {
        let mut stack: Vec<NodeRef<'_>> = self.blocks.iter().rev().map(NodeRef::Block).collect();
        std::iter::from_fn(move || {
            let node = stack.pop()?;
            let mut children: Vec<_> = node.children().collect();
            children.reverse();
            stack.extend(children);
            Some(node)
        })
    }

    /// Assign pre-order ids and build the parent index. Iterative (explicit
    /// stack), and enforces [`MAX_TREE_DEPTH`] by flattening any node whose
    /// children would exceed the cap into a plain-text leaf.
    fn number_nodes(&mut self) {
        enum Slot<'a> {
            Block(&'a mut Block),
            Inline(&'a mut Inline),
        }
        let mut parents: Vec<(u32, u32)> = Vec::new();
        // (node, parent id or MAX, index within parent, depth)
        let mut stack: Vec<(Slot<'_>, u32, u32, usize)> = self
            .blocks
            .iter_mut()
            .enumerate()
            .rev()
            .map(|(i, b)| (Slot::Block(b), u32::MAX, i as u32, 0))
            .collect();
        while let Some((slot, parent, idx, depth)) = stack.pop() {
            let id = parents.len() as u32;
            parents.push((parent, idx));
            match slot {
                Slot::Block(b) => {
                    b.id = NodeId(id);
                    if depth + 1 >= MAX_TREE_DEPTH {
                        flatten_block(b);
                    }
                    let kids = block_children_mut(b);
                    push_children(&mut stack, kids, id, depth + 1);
                }
                Slot::Inline(i) => {
                    i.id = NodeId(id);
                    if depth + 1 >= MAX_TREE_DEPTH {
                        flatten_inline(i);
                    }
                    if let Some(children) = inline_children_mut(i) {
                        for (ci, c) in children.iter_mut().enumerate().rev() {
                            stack.push((Slot::Inline(c), id, ci as u32, depth + 1));
                        }
                    }
                }
            }
        }
        self.parents = parents;

        enum Children<'a> {
            None,
            Blocks(&'a mut [Block]),
            Inlines(&'a mut [Inline]),
        }

        fn block_children_mut(b: &mut Block) -> Children<'_> {
            match &mut b.kind {
                BlockKind::Paragraph { children }
                | BlockKind::Heading { children, .. }
                | BlockKind::TableCell { children } => Children::Inlines(children),
                BlockKind::BlockQuote { children }
                | BlockKind::ListItem { children, .. }
                | BlockKind::FootnoteDefinition { children, .. }
                | BlockKind::Alert { children, .. } => Children::Blocks(children),
                BlockKind::List { items, .. } => Children::Blocks(items),
                BlockKind::Table { rows, .. } => Children::Blocks(rows),
                BlockKind::TableRow { cells, .. } => Children::Blocks(cells),
                BlockKind::ThematicBreak
                | BlockKind::CodeBlock { .. }
                | BlockKind::HtmlBlock { .. }
                | BlockKind::FrontMatter { .. } => Children::None,
            }
        }

        fn inline_children_mut(i: &mut Inline) -> Option<&mut [Inline]> {
            match &mut i.kind {
                InlineKind::Emph(c)
                | InlineKind::Strong(c)
                | InlineKind::Strikethrough(c)
                | InlineKind::Link { children: c, .. }
                | InlineKind::Image { children: c, .. } => Some(c),
                _ => None,
            }
        }

        fn push_children<'a>(
            stack: &mut Vec<(Slot<'a>, u32, u32, usize)>,
            kids: Children<'a>,
            id: u32,
            depth: usize,
        ) {
            match kids {
                Children::None => {}
                Children::Blocks(bs) => {
                    for (ci, c) in bs.iter_mut().enumerate().rev() {
                        stack.push((Slot::Block(c), id, ci as u32, depth));
                    }
                }
                Children::Inlines(is) => {
                    for (ci, c) in is.iter_mut().enumerate().rev() {
                        stack.push((Slot::Inline(c), id, ci as u32, depth));
                    }
                }
            }
        }

        /// Replace a past-cap block's children with nothing (leaf) — the
        /// span still covers the source, so no text is lost to callers that
        /// read source slices. Construction caps make this unreachable for
        /// blocks in practice; it is a belt-and-suspenders guard.
        fn flatten_block(b: &mut Block) {
            let span = b.span;
            b.kind = match std::mem::replace(&mut b.kind, BlockKind::ThematicBreak) {
                k @ (BlockKind::ThematicBreak
                | BlockKind::CodeBlock { .. }
                | BlockKind::HtmlBlock { .. }
                | BlockKind::FrontMatter { .. }) => k,
                _ => BlockKind::Paragraph {
                    children: vec![Inline {
                        id: NodeId(0), // renumbered when visited
                        span,
                        kind: InlineKind::Text(String::new()),
                    }],
                },
            };
        }

        /// Replace a past-cap inline container with a text leaf holding its
        /// plain-text content (collected iteratively).
        fn flatten_inline(node: &mut Inline) {
            let kind = std::mem::replace(&mut node.kind, InlineKind::SoftBreak);
            let children = match kind {
                InlineKind::Emph(c) | InlineKind::Strong(c) | InlineKind::Strikethrough(c) => c,
                InlineKind::Link { children, .. } | InlineKind::Image { children, .. } => children,
                other => {
                    node.kind = other;
                    return;
                }
            };
            let mut text = String::new();
            let mut work: Vec<Inline> = children.into_iter().rev().collect();
            while let Some(c) = work.pop() {
                match c.kind {
                    InlineKind::Text(s) | InlineKind::Code(s) | InlineKind::HtmlInline(s) => {
                        text.push_str(&s)
                    }
                    InlineKind::SoftBreak | InlineKind::HardBreak => text.push(' '),
                    InlineKind::Math { tex, .. } => text.push_str(&tex),
                    InlineKind::FootnoteReference { label } => text.push_str(&label),
                    InlineKind::Emph(mut c)
                    | InlineKind::Strong(mut c)
                    | InlineKind::Strikethrough(mut c)
                    | InlineKind::Link {
                        children: mut c, ..
                    }
                    | InlineKind::Image {
                        children: mut c, ..
                    } => {
                        c.reverse();
                        work.append(&mut c);
                    }
                }
            }
            node.kind = InlineKind::Text(text);
        }
    }
}
