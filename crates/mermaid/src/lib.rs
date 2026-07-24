//! Mermaid fence rendering (P7): a genuine Unicode box-drawing text grid
//! via the `mermaid-text` crate (Spike B verdict,
//! `docs/spikes/highlight-engine.md`), with a verified-ASCII variant that
//! repairs a gap in that crate's own documented guarantee.
//!
//! **Carried-forward gotcha, and how this crate handles it.**
//! `mermaid-text` 0.57.0's `render_ascii()` documents (and doctests) that
//! its output is unconditionally ASCII — every character `< 0x80`. The
//! spike measured this to be **false** for decision-node (`{...}`) diamond
//! shapes: the crate's own `to_ascii()` substitution table has no arm for
//! the diagonal box-drawing glyphs (`╱` U+2571, `╲` U+2572) those shapes
//! use, so they pass through unconverted. This crate does not repeat that
//! mistake by re-asserting the same guarantee without checking it:
//! [`render_ascii_verified`] runs a second substitution pass
//! ([`verify_ascii`]) that repairs exactly this gap (and degrades any
//! *other* still-unrecognized non-ASCII character to `?` rather than
//! silently letting it through), so its ASCII claim is actually true.
//!
//! **Fallback contract.** Every public render function here returns
//! `Result<String, Error>`. `Err` — for any reason (`EmptyInput`,
//! `UnsupportedDiagram`, `ParseError`, `TooWide`) — means "not renderable
//! as a diagram"; the caller's job (wiring this into the document pipeline
//! is out of this phase's file scope: see the discovery doc) is to fall
//! back to displaying the original fence body as a plain code block.
#![forbid(unsafe_code)]

pub use mermaid_text::Error;

/// Renders `source` (a mermaid fence body, without the surrounding
/// ` ```mermaid`/` ``` ` delimiters) as a Unicode box-drawing text block.
pub fn render(source: &str) -> Result<String, Error> {
    mermaid_text::render(source)
}

/// Same contract as [`render`], but the returned string is *actually*
/// ASCII-only (`s.chars().all(|c| c.is_ascii())` always holds) — see the
/// module docs for why this needs its own verification pass rather than
/// trusting `mermaid-text::render_ascii()`'s documented guarantee as-is.
pub fn render_ascii_verified(source: &str) -> Result<String, Error> {
    let rendered = mermaid_text::render_ascii(source)?;
    Ok(verify_ascii(&rendered))
}

/// Forces every remaining non-ASCII character in `s` to a safe ASCII
/// substitute. The two diagonal box-drawing glyphs the upstream crate's
/// own `to_ascii()` misses (verified in the spike) get a faithful
/// substitute; a full diagonal cross gets one too for the same reason
/// (same substitution table gap, same shape family); anything else
/// unrecognized — a future upstream gap, or content this crate has never
/// seen — degrades to `?` rather than silently re-claiming a guarantee
/// that was never actually checked.
fn verify_ascii(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii() {
                c
            } else {
                match c {
                    '\u{2571}' => '/',  // ╱ BOX DRAWINGS LIGHT DIAGONAL UPPER RIGHT TO LOWER LEFT
                    '\u{2572}' => '\\', // ╲ BOX DRAWINGS LIGHT DIAGONAL UPPER LEFT TO LOWER RIGHT
                    '\u{2573}' => 'X',  // ╳ BOX DRAWINGS LIGHT DIAGONAL CROSS
                    _ => '?',
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLOWCHART_WITH_DECISION: &str =
        "graph TD\n  A[Start] --> B{Decision}\n  B -->|Yes| C[Do thing]\n  B -->|No| D[Skip]\n";

    #[test]
    fn test_dw_7_3_renders_a_real_flowchart_fixture() {
        let out = render(FLOWCHART_WITH_DECISION).expect("valid flowchart must render");
        assert!(out.contains("Start"));
        assert!(out.contains("Decision"));
        assert!(out.contains("Do thing"));
        assert!(out.contains("Skip"));
        // A genuine text-grid render, not a stub: multiple lines, and real
        // box-drawing glyphs, not just the labels themselves.
        assert!(out.lines().count() > 3);
        assert!(!out.is_ascii());
    }

    #[test]
    fn test_dw_7_3_parse_failure_falls_back_to_err_for_caller_to_use_a_code_fence() {
        // `classDiagram`'s colon-shorthand member form is explicitly,
        // documentedly unsupported by `mermaid-text` v1 (`src/class.rs`):
        // a genuine parse failure, not merely an unrecognized diagram type.
        let malformed = "classDiagram\nclass Animal\nAnimal : +name String\n";
        assert!(
            matches!(render(malformed), Err(Error::ParseError(_))),
            "expected ParseError for colon-shorthand member form"
        );
    }

    #[test]
    fn test_dw_7_3_empty_input_is_err() {
        assert!(matches!(render(""), Err(Error::EmptyInput)));
        assert!(matches!(render("   \n"), Err(Error::EmptyInput)));
    }

    #[test]
    fn test_dw_7_3_unsupported_diagram_kind_is_err() {
        let unsupported = "unknownDiagramType title Roadmap\n";
        assert!(matches!(
            render(unsupported),
            Err(Error::UnsupportedDiagram(_))
        ));
    }

    #[test]
    fn test_render_ascii_verified_repairs_the_known_diagonal_glyph_gap() {
        // The exact shape the spike found `mermaid_text::render_ascii()`
        // fails to fully convert: a flowchart with a decision-node diamond.
        let unicode = mermaid_text::render(FLOWCHART_WITH_DECISION).unwrap();
        assert!(
            unicode.contains('\u{2571}') || unicode.contains('\u{2572}'),
            "fixture no longer exercises the diagonal-glyph gap this test targets"
        );
        let upstream_ascii = mermaid_text::render_ascii(FLOWCHART_WITH_DECISION).unwrap();
        assert!(
            !upstream_ascii.is_ascii(),
            "upstream's own gap must still be present for this test to be meaningful; if this now \
             fails, mermaid-text has fixed the gap and this crate's post-filter is merely redundant, \
             not wrong"
        );

        let verified = render_ascii_verified(FLOWCHART_WITH_DECISION).unwrap();
        assert!(
            verified.is_ascii(),
            "render_ascii_verified must be truly ASCII-only: {verified:?}"
        );
        assert!(verified.contains("Start"));
        assert!(verified.contains("Decision"));
    }

    #[test]
    fn test_render_ascii_verified_is_ascii_for_diagrams_without_decision_nodes_too() {
        let simple = "graph LR\n  A[Start] --> B[End]\n";
        let verified = render_ascii_verified(simple).unwrap();
        assert!(verified.is_ascii());
        assert!(verified.contains("Start"));
        assert!(verified.contains("End"));
    }

    #[test]
    fn test_verify_ascii_degrades_unknown_non_ascii_to_question_mark() {
        assert_eq!(
            verify_ascii("a\u{2571}b\u{2572}c\u{2573}d\u{00e9}e"),
            "a/b\\cXd?e"
        );
    }

    #[test]
    fn test_render_ascii_verified_propagates_errors_same_as_render() {
        assert!(matches!(render_ascii_verified(""), Err(Error::EmptyInput)));
    }
}
