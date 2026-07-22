//! Behavior tests past the DW floor: the public seam downstream phases
//! (layout sizing, media resolution) build against, extension toggles, and
//! input-normalization edges the implementation surfaced.

use ast::{
    AlertKind, Block, BlockKind, Document, Inline, InlineKind, ListKind, NodeRef, ParseOptions,
};

fn find_inline(doc: &Document, pred: impl Fn(&InlineKind) -> bool) -> Option<&Inline> {
    doc.nodes().find_map(|n| match n {
        NodeRef::Inline(i) if pred(&i.kind) => Some(i),
        _ => None,
    })
}

fn find_block(doc: &Document, pred: impl Fn(&BlockKind) -> bool) -> Option<&Block> {
    doc.nodes().find_map(|n| match n {
        NodeRef::Block(b) if pred(&b.kind) => Some(b),
        _ => None,
    })
}

/// The Phase 6 seam: image path and math TeX resolve from a NodeId.
#[test]
fn test_media_nodes_resolve_by_id() {
    let doc = Document::parse("An ![alt](path/img.png) and $e^{i\\pi}$ math.\n");
    let img = find_inline(&doc, |k| matches!(k, InlineKind::Image { .. })).expect("image parsed");
    let math = find_inline(&doc, |k| matches!(k, InlineKind::Math { .. })).expect("math parsed");

    // Resolve back purely through NodeId, as P6 will.
    match doc.node(img.id) {
        Some(NodeRef::Inline(i)) => match &i.kind {
            InlineKind::Image { dest, .. } => assert_eq!(dest, "path/img.png"),
            other => panic!("resolved to non-image {other:?}"),
        },
        other => panic!("image id resolved to {other:?}"),
    }
    match doc.node(math.id) {
        Some(NodeRef::Inline(i)) => match &i.kind {
            InlineKind::Math { display, tex } => {
                assert!(!display);
                assert_eq!(tex, "e^{i\\pi}");
            }
            other => panic!("resolved to non-math {other:?}"),
        },
        other => panic!("math id resolved to {other:?}"),
    }
}

#[test]
fn test_node_ids_are_dense_preorder() {
    let doc = Document::parse("# h\n\npara *em* text\n\n- a\n- b\n");
    let ids: Vec<usize> = doc.nodes().map(|n| n.id().index()).collect();
    let expected: Vec<usize> = (0..doc.node_count()).collect();
    assert_eq!(ids, expected, "pre-order ids must be dense and ordered");
    // An id past this document's node count does not resolve.
    let bigger = Document::parse("a *b* c *d* e *f* g *h* i *j* k *l* m\n\n- x\n- y\n- z\n");
    assert!(bigger.node_count() > doc.node_count(), "test setup");
    let big_id = bigger.nodes().last().expect("nodes").id();
    assert!(big_id.index() >= doc.node_count(), "test setup");
    assert!(doc.node(big_id).is_none());
}

#[test]
fn test_extension_toggles() {
    let md = "www.example.com and ~~struck~~ and $x$\n\n| a |\n| - |\n| b |\n";
    let full = Document::parse(md);
    assert!(find_inline(&full, |k| matches!(k, InlineKind::Link { .. })).is_some());
    assert!(find_inline(&full, |k| matches!(k, InlineKind::Strikethrough(_))).is_some());
    assert!(find_inline(&full, |k| matches!(k, InlineKind::Math { .. })).is_some());
    assert!(find_block(&full, |k| matches!(k, BlockKind::Table { .. })).is_some());

    let cm = Document::parse_with(md, &ParseOptions::commonmark());
    assert!(find_inline(&cm, |k| matches!(k, InlineKind::Link { .. })).is_none());
    assert!(find_inline(&cm, |k| matches!(k, InlineKind::Strikethrough(_))).is_none());
    assert!(find_inline(&cm, |k| matches!(k, InlineKind::Math { .. })).is_none());
    assert!(find_block(&cm, |k| matches!(k, BlockKind::Table { .. })).is_none());
}

#[test]
fn test_frontmatter_only_at_document_start() {
    let doc = Document::parse("---\ntitle: x\n---\n\nbody\n");
    assert!(matches!(
        doc.blocks().first().map(|b| &b.kind),
        Some(BlockKind::FrontMatter { .. })
    ));
    if let Some(BlockKind::FrontMatter { literal }) = doc.blocks().first().map(|b| &b.kind) {
        assert_eq!(literal, "title: x\n");
    }
    // Not at start → thematic break / setext per CommonMark.
    let doc = Document::parse("body\n\n---\ntitle: x\n---\n");
    assert!(
        !doc.blocks()
            .iter()
            .any(|b| matches!(b.kind, BlockKind::FrontMatter { .. }))
    );
    // Unclosed → not frontmatter.
    let doc = Document::parse("---\ntitle: x\n");
    assert!(
        !doc.blocks()
            .iter()
            .any(|b| matches!(b.kind, BlockKind::FrontMatter { .. }))
    );
}

#[test]
fn test_alerts_all_kinds() {
    let md = "> [!NOTE]\n> a\n\n> [!TIP]\n> b\n\n> [!IMPORTANT]\n> c\n\n> [!WARNING]\n> d\n\n> [!CAUTION]\n> e\n\n> [!OTHER]\n> f\n";
    let doc = Document::parse(md);
    let kinds: Vec<AlertKind> = doc
        .blocks()
        .iter()
        .filter_map(|b| match &b.kind {
            BlockKind::Alert { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            AlertKind::Note,
            AlertKind::Tip,
            AlertKind::Important,
            AlertKind::Warning,
            AlertKind::Caution
        ]
    );
    // The bogus marker stays a blockquote.
    assert!(
        doc.blocks()
            .iter()
            .any(|b| matches!(b.kind, BlockKind::BlockQuote { .. }))
    );
}

#[test]
fn test_task_list_items() {
    let doc = Document::parse("- [ ] open\n- [x] done\n- plain\n");
    let checks: Vec<Option<bool>> = doc
        .nodes()
        .filter_map(|n| match n {
            NodeRef::Block(b) => match b.kind {
                BlockKind::ListItem { checked, .. } => Some(checked),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(checks, vec![Some(false), Some(true), None]);
}

#[test]
fn test_footnotes_resolve_only_when_defined() {
    let doc = Document::parse("See[^yes] and [^no].\n\n[^yes]: defined\n");
    assert!(find_inline(&doc, |k| matches!(k, InlineKind::FootnoteReference { .. })).is_some());
    let refs: Vec<String> = doc
        .nodes()
        .filter_map(|n| match n {
            NodeRef::Inline(i) => match &i.kind {
                InlineKind::FootnoteReference { label } => Some(label.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(refs, vec!["yes"]);
    assert!(find_block(&doc, |k| matches!(k, BlockKind::FootnoteDefinition { .. })).is_some());
}

#[test]
fn test_crlf_spans_index_original_bytes() {
    let src = "# CRLF\r\n\r\npara one\r\nline two\r\n";
    let doc = Document::parse(src);
    assert_eq!(doc.source(), src);
    let heading =
        find_block(&doc, |k| matches!(k, BlockKind::Heading { .. })).expect("heading parsed");
    assert_eq!(&src[heading.span.start..heading.span.end], "# CRLF");
    // Paragraph inlines: text nodes don't include the \r.
    for n in doc.nodes() {
        if let NodeRef::Inline(i) = n
            && let InlineKind::Text(t) = &i.kind
        {
            assert!(!t.contains('\r'), "content leaked a CR: {t:?}");
        }
    }
}

#[test]
fn test_nul_becomes_replacement_char() {
    let doc = Document::parse("a\0b\n");
    assert_eq!(doc.source(), "a\u{FFFD}b\n");
    let text = find_inline(&doc, |k| matches!(k, InlineKind::Text(_))).expect("text");
    if let InlineKind::Text(t) = &text.kind {
        assert_eq!(t, "a\u{FFFD}b");
    }
}

#[test]
fn test_empty_and_trivial_documents() {
    assert_eq!(Document::parse("").blocks().len(), 0);
    assert_eq!(Document::parse("").node_count(), 0);
    assert_eq!(Document::parse("\n\n\n").blocks().len(), 0);
    let doc = Document::parse("x");
    assert_eq!(doc.blocks().len(), 1);
    assert!(matches!(doc.blocks()[0].kind, BlockKind::Paragraph { .. }));
}

#[test]
fn test_list_metadata() {
    let doc = Document::parse("5) five\n6) six\n");
    let list = find_block(&doc, |k| matches!(k, BlockKind::List { .. })).expect("list");
    if let BlockKind::List { kind, tight, items } = &list.kind {
        assert_eq!(
            *kind,
            ListKind::Ordered {
                start: 5,
                delim: ')'
            }
        );
        assert!(*tight);
        assert_eq!(items.len(), 2);
    }
}

#[test]
fn test_raw_html_preserved_uninterpreted() {
    let doc = Document::parse("<div onclick=\"evil()\">\nraw\n</div>\n\nx <b>y</b>\n");
    let html = find_block(&doc, |k| matches!(k, BlockKind::HtmlBlock { .. })).expect("html block");
    if let BlockKind::HtmlBlock { literal } = &html.kind {
        // Uninterpreted: the exact bytes, no attribute model, no DOM.
        assert_eq!(literal, "<div onclick=\"evil()\">\nraw\n</div>");
    }
    assert!(find_inline(&doc, |k| matches!(k, InlineKind::HtmlInline(_))).is_some());
}

// --- Phase 2 gate-review regressions (judge findings, 2026-07-22) ---

/// Spec §2.5: an entity reference cannot stand in place of a character that
/// defines structure, so a decoded space never counts toward a hard break.
#[test]
fn test_entity_space_does_not_make_a_hard_break() {
    // One literal space + one decoded space => soft break, and the decoded
    // space survives as text rather than being trimmed away.
    let doc = Document::parse("a&#32; \nb\n");
    assert!(
        find_inline(&doc, |k| matches!(k, InlineKind::SoftBreak)).is_some(),
        "one literal space must not be promoted to a hard break"
    );
    assert!(find_inline(&doc, |k| matches!(k, InlineKind::HardBreak)).is_none());
    let text: String = doc
        .nodes()
        .filter_map(|n| match n {
            NodeRef::Inline(i) => match &i.kind {
                InlineKind::Text(t) => Some(t.as_str()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert!(text.starts_with("a "), "decoded space kept, got {text:?}");

    // Two literal spaces still hard-break, decoded space or not.
    let doc = Document::parse("a&#32;  \nb\n");
    assert!(find_inline(&doc, |k| matches!(k, InlineKind::HardBreak)).is_some());
}

/// The 999 limit on link labels is counted in characters, not bytes.
#[test]
fn test_link_label_limit_counts_chars_not_bytes() {
    // 400 three-byte chars = 1200 bytes but only 400 chars: well under the cap.
    let label: String = "é".repeat(400);
    let src = format!("[{label}]\n\n[{label}]: /url\n");
    let doc = Document::parse(&src);
    assert!(
        find_inline(&doc, |k| matches!(k, InlineKind::Link { .. })).is_some(),
        "a 400-char label must resolve even though it is 800 bytes"
    );
}

/// GFM extended autolink schemes are http, https, and ftp (gfm-spec §6.9,
/// example 628) — and nothing else.
#[test]
fn test_extended_autolink_schemes_are_exactly_http_https_ftp() {
    for src in ["see http://a.b/x", "see https://a.b/x", "see ftp://a.b/x"] {
        let doc = Document::parse(&format!("{src}\n"));
        assert!(
            find_inline(&doc, |k| matches!(k, InlineKind::Link { .. })).is_some(),
            "{src} must autolink"
        );
    }
    let doc = Document::parse("see gopher://a.b/x\n");
    assert!(
        find_inline(&doc, |k| matches!(k, InlineKind::Link { .. })).is_none(),
        "gopher:// is not an extended autolink scheme"
    );
}
