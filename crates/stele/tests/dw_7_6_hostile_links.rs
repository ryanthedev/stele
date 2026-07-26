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

/// DW-6.5, the *activation* half of the same corpus.
///
/// `highlight`'s barricade governs painting an OSC 8 hyperlink and allows
/// four schemes; following a link spawns a process and allows two. So every
/// hostile destination in the fixture must be refused by
/// `stele::link::LinkTarget`, and — the part that matters — refused *before*
/// anything is spawned, which is why the classification is checked directly
/// rather than through a `Navigator` with a live opener behind it.
#[test]
fn test_dw_6_5_every_hostile_fixture_destination_is_refused_activation_without_a_process() {
    use stele::link::{LinkError, LinkTarget};

    let doc = Document::parse(FIXTURE);
    let dests = link_destinations(&doc);
    assert_eq!(dests.len(), 4, "fixture must carry exactly 4 links");

    for dest in &dests {
        let classified = LinkTarget::classify(dest);
        if dest.starts_with("https://") {
            assert!(
                matches!(classified, Ok(LinkTarget::Url(_))),
                "the safe link must still be followable: {dest:?}"
            );
            continue;
        }
        let err = classified.expect_err("a hostile scheme must not classify as followable");
        assert!(
            matches!(err, LinkError::UnsupportedScheme(_)),
            "{dest:?} produced {err:?}"
        );
        assert!(
            err.to_string().contains("http/https only"),
            "the refusal must say why: {err}"
        );
    }
}

/// The two barricades are deliberately different widths, and that is easy to
/// "tidy" into one. `file:` and `mailto:` may be *painted* as hyperlinks —
/// the terminal decides what to do with a click on one — but stele must never
/// hand either to a process itself.
#[test]
fn test_dw_6_3_the_activation_allowlist_is_tighter_than_the_display_allowlist() {
    use stele::link::{LinkError, LinkTarget};

    for url in ["file:///etc/passwd", "mailto:a@example.com"] {
        assert!(
            highlight::hyperlink_open(url).is_some(),
            "{url} is still paintable as an OSC 8 hyperlink"
        );
        assert!(
            matches!(
                LinkTarget::classify(url),
                Err(LinkError::UnsupportedScheme(_))
            ),
            "{url} must never be handed to the OS opener"
        );
    }
    for url in ["http://example.com/x", "https://example.com/x"] {
        assert!(highlight::hyperlink_open(url).is_some());
        assert!(matches!(LinkTarget::classify(url), Ok(LinkTarget::Url(_))));
    }
}
