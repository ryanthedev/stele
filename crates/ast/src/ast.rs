//! The typed, span-carrying AST.
//!
//! Every node is a [`Block`] or [`Inline`]: a stable [`NodeId`], a byte-range
//! [`Span`] into the original source, and a typed kind. Downstream phases
//! (layout, media) address nodes by `NodeId` and resolve them back through
//! [`Document::node`](crate::Document::node).

/// A byte range into the source text handed to `Document::parse`.
///
/// Always in-bounds and on `char` boundaries: `&source[span.start..span.end]`
/// never panics. The slice is the *source* the node spans; node content can
/// differ from it (entity references, backslash escapes, stripped container
/// markers, U+0000 → U+FFFD replacement).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub(crate) fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }
}

/// Stable identity of one node within one [`Document`](crate::Document).
///
/// Ids are assigned in pre-order after parsing: a given (source, parser
/// version) pair always yields the same ids. They are meaningless across
/// documents. This is the single node-identity type shared with the layout
/// and media phases (`IntrinsicSizer` keys on it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub(crate) u32);

impl NodeId {
    /// The raw index value. Dense: `0..Document::node_count()`.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A block-level node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub id: NodeId,
    pub span: Span,
    pub kind: BlockKind,
}

/// An inline-level node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inline {
    pub id: NodeId,
    pub span: Span,
    pub kind: InlineKind,
}

/// Ordered-list numbering style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    /// `-`, `+`, or `*` bullets; the marker character is recorded.
    Bullet(char),
    /// `1.` / `1)` numbering; start number and delimiter (`.` or `)`).
    Ordered { start: u64, delim: char },
}

/// GFM table column alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    None,
    Left,
    Center,
    Right,
}

/// GitHub alert flavor (`> [!NOTE]` …).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

/// Typed block-level content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph {
        children: Vec<Inline>,
    },
    /// ATX or setext heading, level 1–6.
    Heading {
        level: u8,
        children: Vec<Inline>,
    },
    ThematicBreak,
    BlockQuote {
        children: Vec<Block>,
    },
    /// A list; every child is a `ListItem`.
    List {
        kind: ListKind,
        tight: bool,
        items: Vec<Block>,
    },
    /// One list item. `checked` is `Some` for GFM task-list items.
    ListItem {
        checked: Option<bool>,
        children: Vec<Block>,
    },
    /// Fenced or indented code. `info` is the full fence info string
    /// (entity- and escape-decoded), `None` for indented code.
    CodeBlock {
        info: Option<String>,
        literal: String,
        fenced: bool,
    },
    /// Raw HTML block, preserved uninterpreted (rendering phases show it
    /// as literal text; the conformance shim emits it verbatim).
    HtmlBlock {
        literal: String,
    },
    /// GFM table; every child is a `TableRow`.
    Table {
        alignments: Vec<Alignment>,
        rows: Vec<Block>,
    },
    /// One table row; every child is a `TableCell`.
    TableRow {
        header: bool,
        cells: Vec<Block>,
    },
    TableCell {
        children: Vec<Inline>,
    },
    /// `[^label]: …` footnote definition (extension).
    FootnoteDefinition {
        label: String,
        children: Vec<Block>,
    },
    /// GitHub alert: a blockquote whose first line is `[!KIND]` (extension).
    Alert {
        kind: AlertKind,
        children: Vec<Block>,
    },
    /// YAML frontmatter fenced by `---` at the very start of the document
    /// (extension). `literal` is the raw text between the fences.
    FrontMatter {
        literal: String,
    },
}

/// Typed inline-level content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineKind {
    /// Literal text (entities and escapes already decoded; never contains
    /// a newline — line breaks are `SoftBreak`/`HardBreak` nodes).
    Text(String),
    SoftBreak,
    HardBreak,
    /// Code span content (normalized per spec).
    Code(String),
    /// Raw inline HTML, preserved uninterpreted.
    HtmlInline(String),
    Emph(Vec<Inline>),
    Strong(Vec<Inline>),
    /// GFM strikethrough (`~~…~~`).
    Strikethrough(Vec<Inline>),
    /// Link: CommonMark inline/reference/autolink and GFM autolink literals
    /// all normalize to this.
    Link {
        dest: String,
        title: String,
        children: Vec<Inline>,
    },
    /// Image; `children` carry the alt text.
    Image {
        dest: String,
        title: String,
        children: Vec<Inline>,
    },
    /// `$…$` (inline) or `$$…$$` (display) math (extension). `tex` is the
    /// raw TeX source, undecoded.
    Math {
        display: bool,
        tex: String,
    },
    /// `[^label]` footnote reference (extension).
    FootnoteReference {
        label: String,
    },
}

/// A borrowed reference to any node in the tree.
#[derive(Debug, Clone, Copy)]
pub enum NodeRef<'a> {
    Block(&'a Block),
    Inline(&'a Inline),
}

impl<'a> NodeRef<'a> {
    pub fn id(self) -> NodeId {
        match self {
            NodeRef::Block(b) => b.id,
            NodeRef::Inline(i) => i.id,
        }
    }

    pub fn span(self) -> Span {
        match self {
            NodeRef::Block(b) => b.span,
            NodeRef::Inline(i) => i.span,
        }
    }

    /// The `idx`-th child of this node, in document order.
    pub fn child(self, idx: usize) -> Option<NodeRef<'a>> {
        match self {
            NodeRef::Block(b) => match &b.kind {
                BlockKind::Paragraph { children }
                | BlockKind::Heading { children, .. }
                | BlockKind::TableCell { children } => children.get(idx).map(NodeRef::Inline),
                BlockKind::BlockQuote { children }
                | BlockKind::ListItem { children, .. }
                | BlockKind::FootnoteDefinition { children, .. }
                | BlockKind::Alert { children, .. } => children.get(idx).map(NodeRef::Block),
                BlockKind::List { items, .. } => items.get(idx).map(NodeRef::Block),
                BlockKind::Table { rows, .. } => rows.get(idx).map(NodeRef::Block),
                BlockKind::TableRow { cells, .. } => cells.get(idx).map(NodeRef::Block),
                BlockKind::ThematicBreak
                | BlockKind::CodeBlock { .. }
                | BlockKind::HtmlBlock { .. }
                | BlockKind::FrontMatter { .. } => None,
            },
            NodeRef::Inline(i) => match &i.kind {
                InlineKind::Emph(children)
                | InlineKind::Strong(children)
                | InlineKind::Strikethrough(children)
                | InlineKind::Link { children, .. }
                | InlineKind::Image { children, .. } => children.get(idx).map(NodeRef::Inline),
                InlineKind::Text(_)
                | InlineKind::SoftBreak
                | InlineKind::HardBreak
                | InlineKind::Code(_)
                | InlineKind::HtmlInline(_)
                | InlineKind::Math { .. }
                | InlineKind::FootnoteReference { .. } => None,
            },
        }
    }

    /// Iterate this node's direct children in document order.
    pub fn children(self) -> impl Iterator<Item = NodeRef<'a>> {
        (0..).map_while(move |i| self.child(i))
    }
}
