//! DW-7.3 against the actual `fixtures/mermaid.md` corpus: extracts each
//! ` ```mermaid ` fence body and asserts the render/fallback contract
//! holds for real fixture content, not just inline test strings.

const FIXTURE: &str = include_str!("../../../fixtures/mermaid.md");

/// Minimal fence extractor: pulls the body text between consecutive
/// ` ```mermaid` / ` ``` ` fence markers, in document order. Deliberately
/// not a full markdown parse (that's `crates/ast`'s job, out of this
/// phase's scope) — just enough to pull real fixture bodies for this test.
fn mermaid_fence_bodies(markdown: &str) -> Vec<String> {
    let mut bodies = Vec::new();
    let mut lines = markdown.lines();
    while let Some(line) = lines.by_ref().next() {
        if line.trim() == "```mermaid" {
            let mut body = String::new();
            for inner in lines.by_ref() {
                if inner.trim() == "```" {
                    break;
                }
                body.push_str(inner);
                body.push('\n');
            }
            bodies.push(body);
        }
    }
    bodies
}

#[test]
fn test_dw_7_3_fixture_has_both_a_renderable_and_a_fallback_case() {
    let bodies = mermaid_fence_bodies(FIXTURE);
    assert_eq!(
        bodies.len(),
        2,
        "fixture must carry exactly one renderable and one fallback case"
    );

    let renderable = &bodies[0];
    let rendered = mermaid::render(renderable).expect("first fixture fence must render");
    assert!(rendered.contains("Start"));
    assert!(rendered.contains("Decision"));

    let fallback = &bodies[1];
    let err = mermaid::render(fallback).expect_err("second fixture fence must fail to render");
    assert!(
        matches!(err, mermaid::Error::UnsupportedDiagram(_)),
        "expected the fixture's fallback fence to be an unsupported diagram, got {err:?}"
    );
}
