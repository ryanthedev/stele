//! DW-7.4, document-level: every footnote label in `fixtures/footnotes.md`
//! gets a ref/back-reference anchor pair that matches 1:1, extracted from a
//! real parsed `ast::Document` rather than a hand-picked label list.

use ast::{BlockKind, Document, InlineKind};
use highlight::{backref_osc8, def_anchor_id, ref_anchor_id, ref_osc8};

const FIXTURE: &str = include_str!("../../../fixtures/footnotes.md");

fn collect_labels(doc: &Document) -> (Vec<String>, Vec<String>) {
    let mut refs = Vec::new();
    let mut defs = Vec::new();
    for node in doc.nodes() {
        match node {
            ast::NodeRef::Inline(inline) => {
                if let InlineKind::FootnoteReference { label } = &inline.kind {
                    refs.push(label.clone());
                }
            }
            ast::NodeRef::Block(block) => {
                if let BlockKind::FootnoteDefinition { label, .. } = &block.kind {
                    defs.push(label.clone());
                }
            }
        }
    }
    (refs, defs)
}

#[test]
fn test_dw_7_4_fixture_ref_and_def_labels_produce_matching_anchor_pairs() {
    let doc = Document::parse(FIXTURE);
    let (refs, defs) = collect_labels(&doc);
    // Sanity on the fixture itself: `ast` renders an undefined ref
    // (`[^ghost]`, no matching definition) as literal text rather than an
    // `InlineKind::FootnoteReference` node — the fixture's own comment
    // says as much ("stays literal") — so it never appears in `refs` at
    // all. Only every *defined* label needs a matching anchor pair.
    assert!(refs.contains(&"one".to_string()));
    assert!(refs.contains(&"two".to_string()));
    assert!(!refs.contains(&"ghost".to_string()));
    assert_eq!(defs, vec!["one".to_string(), "two".to_string()]);

    for label in &defs {
        let ref_seq = ref_osc8(label);
        let backref_seq = backref_osc8(label);
        let ref_id = ref_anchor_id(label);
        let def_id = def_anchor_id(label);
        assert!(ref_seq.contains(&format!("id={ref_id}")));
        assert!(ref_seq.contains(&format!("#{def_id}")));
        assert!(backref_seq.contains(&format!("id={def_id}")));
        assert!(backref_seq.contains(&format!("#{ref_id}")));
    }

    // Every defined label's pair of anchor ids is unique across the whole
    // fixture — no two footnotes can navigate to each other's anchors.
    let mut all_ids = std::collections::HashSet::new();
    for label in &defs {
        assert!(all_ids.insert(ref_anchor_id(label)));
        assert!(all_ids.insert(def_anchor_id(label)));
    }
    assert_eq!(all_ids.len(), defs.len() * 2);
}

/// `ast` never even emits a `FootnoteReference` node for an undefined
/// label (the fixture's `[^ghost]` parses as literal text — verified
/// above), so this crate's anchor functions never see it in practice. They
/// are still pure functions of any label string, with no dependency on a
/// definition existing — this test pins that they would build a
/// well-formed anchor for such a label anyway, rather than assuming the
/// AST layer's "undefined refs stay literal" behavior is the only thing
/// standing between a bad label and a malformed sequence.
#[test]
fn test_anchor_functions_are_pure_and_do_not_depend_on_a_definition_existing() {
    let seq = ref_osc8("ghost");
    assert!(seq.starts_with("\x1b]8;id=fnref-ghost;#fn-ghost\x1b\\"));
}
