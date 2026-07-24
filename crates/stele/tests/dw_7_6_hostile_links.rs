//! DW-7.6, document-level: every link destination in
//! `fixtures/hostile-links.md` is extracted from a real parsed
//! `ast::Document` and run through `highlight`'s OSC 8 barricade
//! (`crates/highlight/src/hyperlink.rs`) — disallowed schemes must never
//! produce an emittable sequence. Control-byte-injection cases are
//! exercised directly in `crates/highlight`'s own unit tests (a markdown
//! fixture file is the wrong place to embed literal raw control bytes);
//! this test's job is the scheme-allowlist barricade against real,
//! parsed, hostile markdown link syntax.

use ast::{Document, InlineKind, NodeRef};

const FIXTURE: &str = include_str!("../../../fixtures/hostile-links.md");

fn link_destinations(doc: &Document) -> Vec<String> {
    doc.nodes()
        .filter_map(|node| match node {
            NodeRef::Inline(inline) => match &inline.kind {
                InlineKind::Link { dest, .. } => Some(dest.clone()),
                _ => None,
            },
            NodeRef::Block(_) => None,
        })
        .collect()
}

#[test]
fn test_dw_7_6_hostile_schemes_in_the_fixture_never_reach_an_osc_8_sequence() {
    let doc = Document::parse(FIXTURE);
    let dests = link_destinations(&doc);
    assert_eq!(dests.len(), 4, "fixture must carry exactly 4 links");

    let safe = dests
        .iter()
        .find(|d| d.starts_with("https://"))
        .expect("safe link must parse");
    assert!(highlight::hyperlink_open(safe).is_some());

    let hostile: Vec<&String> = dests.iter().filter(|d| d != &safe).collect();
    assert_eq!(hostile.len(), 3);
    for dest in hostile {
        assert_eq!(
            highlight::hyperlink_open(dest),
            None,
            "hostile destination {dest:?} must never produce an OSC 8 sequence"
        );
        assert_eq!(highlight::sanitize_url(dest), None);
    }
}

#[test]
fn test_dw_7_6_only_the_allowed_schemes_survive_a_mixed_batch() {
    let batch = [
        "https://example.com",
        "http://example.com",
        "file:///etc/passwd",
        "mailto:a@example.com",
        "javascript:alert(1)",
        "data:text/html,evil",
        "vbscript:evil()",
        "ftp://example.com",
    ];
    let allowed_count = batch
        .iter()
        .filter(|url| highlight::hyperlink_open(url).is_some())
        .count();
    assert_eq!(
        allowed_count, 4,
        "exactly the 4 allowlisted schemes must survive"
    );
}
