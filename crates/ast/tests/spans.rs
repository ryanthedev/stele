//! DW-2.4: spans reconstruct the exact source slice for every node in the
//! fixture corpus.
//!
//! For every node of every `fixtures/*.md` document:
//! - the span is in bounds, ordered, and on `char` boundaries — the slice
//!   `&source[span]` always reconstructs without panicking;
//! - child spans nest inside their parent's span;
//! - sibling spans are ordered and non-overlapping — except under a
//!   `TableRow`, where only ordering is required (see below);
//! - for plain `Text` leaves (no escapes/entities/line breaks in the
//!   slice) the slice *equals* the node content byte-for-byte.
//!
//! `Document::node` round-trips every id (the NodeId seam Phases 4/6
//! build against).

use ast::{BlockKind, Document, InlineKind, NodeRef};
use std::path::PathBuf;

fn fixture_paths() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("fixtures/ exists")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    paths.sort();
    assert!(paths.len() >= 15, "fixture corpus unexpectedly small");
    paths
}

fn check_node(doc: &Document, node: NodeRef<'_>, fixture: &str) {
    let src = doc.source();
    let sp = node.span();
    assert!(sp.start <= sp.end, "{fixture}: inverted span {sp:?}");
    assert!(sp.end <= src.len(), "{fixture}: span out of bounds {sp:?}");
    assert!(
        src.is_char_boundary(sp.start) && src.is_char_boundary(sp.end),
        "{fixture}: span not on char boundary {sp:?}"
    );
    // The reconstruction itself: this slice must not panic.
    let slice = &src[sp.start..sp.end];

    // Plain text leaves: slice == content exactly.
    //
    // Escapes and entities are excluded because the content is the decoded
    // form. A newline in the slice is excluded for the same reason one step
    // further out: a `Text` node never holds one (breaks are their own
    // nodes), so a multi-line slice means the parser assembled the content
    // from fragments — a grid table's cell joins its lines with a single
    // space, dropping the `|` borders and the padding in between. The
    // equality would be guaranteed to fail there and proves nothing;
    // the in-bounds, ordering and nesting checks above still apply.
    if let NodeRef::Inline(i) = node
        && let InlineKind::Text(content) = &i.kind
        && !slice.contains(['\\', '&', '\n'])
    {
        assert_eq!(
            slice, content,
            "{fixture}: Text node content diverges from its source slice"
        );
    }

    // Children nest and are ordered.
    //
    // Non-overlap is the normal rule, and it holds everywhere markdown is
    // one-dimensional. A Pandoc grid table is not: a cell written over three
    // lines interleaves, byte for byte, with the cell beside it, so the second
    // cell's span necessarily *starts inside* the first cell's span. Every
    // column's span is still the true source range of its own content — the
    // columns simply share lines. Ordering by start is the strongest invariant
    // a 2-D grid can satisfy.
    //
    // The exemption is granted by *evidence*, not by node kind. A `TableRow`
    // is also what the GFM pipe parser produces, and a pipe row is ordinary
    // 1-D source whose cells cannot legitimately overlap — exempting every
    // row would quietly drop that coverage for a construct that never needed
    // it. So a row is only excused when its cells actually span more than one
    // line, which is the thing that makes non-overlap impossible.
    let cells_may_interleave = matches!(
        node,
        NodeRef::Block(b) if matches!(b.kind, BlockKind::TableRow { .. })
    ) && node
        .children()
        .any(|cell| src[cell.span().start..cell.span().end].contains('\n'));
    let mut prev_end: Option<usize> = None;
    let mut prev_start: Option<usize> = None;
    for child in node.children() {
        let csp = child.span();
        assert!(
            csp.start >= sp.start && csp.end <= sp.end,
            "{fixture}: child span {csp:?} escapes parent {sp:?}"
        );
        if cells_may_interleave {
            if let Some(ps) = prev_start {
                assert!(
                    csp.start >= ps,
                    "{fixture}: table cells out of order: {csp:?} starts before {ps}"
                );
            }
        } else if let Some(pe) = prev_end {
            assert!(
                csp.start >= pe,
                "{fixture}: sibling spans overlap or unordered: {csp:?} after end {pe}"
            );
        }
        prev_end = Some(csp.end);
        prev_start = Some(csp.start);
    }
}

#[test]
fn test_dw_2_4_spans_reconstruct_source() {
    for path in fixture_paths() {
        let fixture = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).expect("fixture readable");
        let doc = Document::parse(&text);
        let mut count = 0;
        for node in doc.nodes() {
            check_node(&doc, node, &fixture);
            count += 1;
        }
        assert!(count > 0, "{fixture}: parsed to zero nodes");
        assert_eq!(count, doc.node_count(), "{fixture}: node_count mismatch");
    }
}

#[test]
fn test_dw_2_4_node_id_round_trip() {
    for path in fixture_paths() {
        let fixture = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).expect("fixture readable");
        let doc = Document::parse(&text);
        for node in doc.nodes() {
            let id = node.id();
            let resolved = doc
                .node(id)
                .unwrap_or_else(|| panic!("{fixture}: id {id:?} does not resolve"));
            assert_eq!(resolved.id(), id, "{fixture}: resolved wrong node");
            assert_eq!(
                resolved.span(),
                node.span(),
                "{fixture}: resolved node has different span"
            );
        }
    }
}

#[test]
fn test_dw_2_4_ids_stable_across_parses() {
    for path in fixture_paths() {
        let text = std::fs::read_to_string(&path).expect("fixture readable");
        let a = Document::parse(&text);
        let b = Document::parse(&text);
        let ids_a: Vec<_> = a.nodes().map(|n| (n.id(), n.span())).collect();
        let ids_b: Vec<_> = b.nodes().map(|n| (n.id(), n.span())).collect();
        assert_eq!(ids_a, ids_b, "NodeIds must be deterministic per source");
    }
}
