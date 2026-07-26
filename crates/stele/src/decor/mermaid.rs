//! Mermaid fence rendering, wired into the document as a source-text
//! preprocessor (P7 wiring).
//!
//! `Decor::highlight` is a per-line, layout-preserving transform, so it
//! cannot turn a one-line ` ```mermaid ` fence body into a multi-line
//! box-drawing grid. Instead this runs before `Document::parse`: it finds
//! top-level ` ```mermaid ` code blocks, renders each through
//! [`mermaid::render`], and replaces the fence with a plain code fence
//! holding the rendered grid. On any render failure the original fence is
//! left untouched — an unknown `mermaid` language tag simply displays as a
//! plain code block, which is the documented fallback.
//!
//! Only *top-level* mermaid fences are handled: a fence nested in a list or
//! blockquote is left as a plain block, so span replacement can never
//! overlap a parent block's span.

use std::borrow::Cow;

use ast::{BlockKind, Document};

/// Parses `source` with its top-level mermaid fences rendered — the load
/// path's text→[`Document`] step.
///
/// **Exactly one parse when there is no renderable mermaid fence** (DW-2.5).
/// The old shape was [`preprocess`] followed by a second `Document::parse` in
/// `main`, which parsed every document twice however few fences it held —
/// and finding the fences is itself what the first parse is for, so the
/// second one was pure waste on the overwhelmingly common no-mermaid path.
/// A document that *does* render a fence still costs two: the spliced text is
/// different text, and there is no way to know what it parses to without
/// parsing it.
pub fn parse(source: &str) -> Document {
    let doc = crate::loader::counted_parse(source);
    match rendered(source, &doc) {
        None => doc,
        Some(spliced) => crate::loader::counted_parse(&spliced),
    }
}

/// Replaces top-level ` ```mermaid ` blocks in `source` with a rendered
/// box-drawing grid (as a plain code fence). Returns `source` unchanged when
/// there are no renderable mermaid blocks.
///
/// The text-only half of [`parse`], kept for callers that want the
/// preprocessed *source* rather than the document it parses to.
pub fn preprocess(source: &str) -> Cow<'_, str> {
    match rendered(source, &crate::loader::counted_parse(source)) {
        None => Cow::Borrowed(source),
        Some(spliced) => Cow::Owned(spliced),
    }
}

/// The splice, given `source` and the document it already parsed to.
///
/// Returns `None` when nothing was replaced — which is what lets [`parse`]
/// keep `doc` instead of parsing again.
fn rendered(source: &str, doc: &Document) -> Option<String> {
    // Collect (span, rendered) for each top-level mermaid fence that renders.
    let mut repls: Vec<(usize, usize, String)> = Vec::new();
    for block in doc.blocks() {
        let BlockKind::CodeBlock { info, literal, .. } = &block.kind else {
            continue;
        };
        let is_mermaid = info
            .as_deref()
            .map(str::trim)
            .and_then(|s| s.split_whitespace().next())
            .is_some_and(|lang| lang.eq_ignore_ascii_case("mermaid"));
        if !is_mermaid {
            continue;
        }
        // Failure (empty, unsupported, parse error, too wide) -> leave the
        // original fence: it renders as a plain code block.
        if let Ok(grid) = mermaid::render(literal) {
            let fenced = as_plain_fence(&grid);
            repls.push((block.span.start, block.span.end, fenced));
        }
    }

    if repls.is_empty() {
        return None;
    }

    // Splice from the end backwards so earlier spans keep their offsets.
    repls.sort_by_key(|(start, _, _)| *start);
    let mut out = source.to_string();
    for (start, end, text) in repls.into_iter().rev() {
        out.replace_range(start..end, &text);
    }
    Some(out)
}

/// Wraps `grid` in a plain (no-language) fenced code block so it displays
/// verbatim.
///
/// The fence is `~` repeated one *more* time than the longest run of tildes
/// that starts any line of `grid`, and at least three. That is the
/// CommonMark rule this wrapper depends on: a tilde fence is closed by the
/// first following line whose leading tilde run is at least as long as the
/// opening fence. A wrapper that is not strictly longer than the content's
/// own longest leading run lets the *grid* close the block early — and then
/// the wrapper's real closing fence opens a NEW code block that swallows
/// every block after the diagram, to EOF.
///
/// The old fixed `~~~` claimed a box-drawing grid "can never contain"
/// `~~~`. It can: diagram labels are arbitrary author text, and
/// `mermaid-text` renders `gantt`, `journey` and `timeline` labels flush
/// left with no box border, so tildes in a label reach column 0 unguarded.
/// Computing the fence from the content makes this correct for *any* grid
/// rather than for the diagram kinds that happen to draw a border.
///
/// Leading whitespace is stripped before measuring rather than only the
/// three spaces CommonMark allows a closing fence to be indented by. That
/// over-counts a deeply indented run, which can only ever make the fence
/// longer than strictly necessary — never shorter, which is the failure
/// that matters.
fn as_plain_fence(grid: &str) -> String {
    let body = grid.trim_end_matches('\n');
    let longest_leading_run = body
        .lines()
        .map(|line| line.trim_start().bytes().take_while(|&b| b == b'~').count())
        .max()
        .unwrap_or(0);
    let fence = "~".repeat(longest_leading_run.saturating_add(1).max(3));
    format!("{fence}\n{body}\n{fence}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_non_mermaid_source_is_returned_borrowed_unchanged() {
        let src = "# Title\n\n```rust\nfn main() {}\n```\n";
        assert!(matches!(preprocess(src), Cow::Borrowed(_)));
        // The same fact one layer down: nothing was spliced, which is what
        // lets `parse` keep the document it already has.
        assert!(rendered(src, &Document::parse(src)).is_none());
    }

    #[test]
    fn test_unrenderable_mermaid_is_left_as_its_original_fence() {
        // Empty body -> mermaid::render errs -> fence left untouched.
        let src = "```mermaid\n```\n";
        let out = preprocess(src);
        assert!(out.contains("```mermaid"), "unrenderable fence must remain");
    }

    #[test]
    fn test_renderable_mermaid_becomes_a_plain_fence_grid() {
        let src = "before\n\n```mermaid\ngraph TD\n  A-->B\n```\n\nafter\n";
        let out = preprocess(src);
        // Only replaced when the crate actually renders it; if this mermaid
        // version renders `graph TD`, the mermaid tag is gone and a plain
        // fence remains. If not, the test below still holds (fallback).
        if mermaid::render("graph TD\n  A-->B\n").is_ok() {
            assert!(!out.contains("```mermaid"));
            assert!(out.contains("~~~"));
        }
        assert!(out.contains("before") && out.contains("after"));
    }

    /// The wrapper fence must survive *any* grid content, not just the
    /// diagram kinds that draw a border. Assert the round trip through the
    /// real parser: one code block, and its literal is the whole grid.
    #[test]
    fn test_a_grid_whose_own_lines_start_with_tildes_stays_one_code_block() {
        // A run longer than any fence `as_plain_fence` used to emit, a run
        // exactly at the old fence length, and an indented run.
        let grid = "~~~~~~~~~~~~\nordinary line\n~~~\n  ~~~~~\ntail\n";
        let fenced = as_plain_fence(grid);
        assert!(
            fenced.starts_with("~~~~~~~~~~~~~\n"),
            "fence must be strictly longer than the longest leading run: {fenced:?}"
        );

        let doc = Document::parse(&fenced);
        let blocks = doc.blocks();
        assert_eq!(blocks.len(), 1, "expected exactly one block: {blocks:?}");
        let BlockKind::CodeBlock { literal, info, .. } = &blocks[0].kind else {
            panic!("expected a code block, got {:?}", blocks[0].kind);
        };
        assert_eq!(info.as_deref(), None);
        assert_eq!(literal, grid, "the grid must survive verbatim");
    }

    /// End to end: a `gantt` label of tildes reaches column 0 (mermaid-text
    /// renders gantt/journey/timeline labels flush left, unguarded by a box
    /// border) and used to close the wrapper, turning every block after the
    /// diagram into code-block text.
    #[test]
    fn test_a_hostile_gantt_label_cannot_swallow_the_rest_of_the_document() {
        for tildes in ["~~~~", "~~~~~~~~~~~~~~~~"] {
            let src = format!(
                "# Doc\n\n\
                 ```mermaid\n\
                 gantt\n  title T\n  section {tildes}\n  {tildes} :a1, 2024-01-01, 30d\n\
                 ```\n\n\
                 # After the diagram\n\n\
                 This paragraph must survive.\n"
            );
            // mermaid-text is pinned (`=0.57.0`); if gantt ever stops
            // rendering, the fence is never emitted and this test would
            // silently assert nothing.
            assert!(
                mermaid::render(&format!(
                    "gantt\n  title T\n  section {tildes}\n  {tildes} :a1, 2024-01-01, 30d\n"
                ))
                .is_ok(),
                "gantt must render for this test to exercise the wrapper"
            );

            let out = preprocess(&src);
            let doc = Document::parse(&out);
            let kinds: Vec<&BlockKind> = doc.blocks().iter().map(|b| &b.kind).collect();

            // Assert the actual block structure, not a count of headings.
            assert_eq!(
                kinds.len(),
                4,
                "expected heading/code/heading/paragraph, got {kinds:?}"
            );
            assert!(matches!(kinds[0], BlockKind::Heading { level: 1, .. }));
            assert!(matches!(kinds[1], BlockKind::CodeBlock { .. }));
            let BlockKind::Heading { level, children } = kinds[2] else {
                panic!("block 3 must still be a heading, got {:?}", kinds[2]);
            };
            assert_eq!(*level, 1);
            assert_eq!(
                children
                    .iter()
                    .map(|inline| match &inline.kind {
                        ast::InlineKind::Text(t) => t.as_str(),
                        _ => "",
                    })
                    .collect::<String>(),
                "After the diagram"
            );
            assert!(
                matches!(kinds[3], BlockKind::Paragraph { .. }),
                "the trailing paragraph must still be a paragraph, got {:?}",
                kinds[3]
            );

            // And the diagram's own bar row must still be inside the fence.
            let BlockKind::CodeBlock { literal, .. } = kinds[1] else {
                unreachable!()
            };
            assert!(
                literal.contains(tildes),
                "the hostile label must survive inside the code block: {literal:?}"
            );
        }
    }

    /// DW-2.5, at the seam that owns the rule: the no-mermaid path must reuse
    /// the parse it already did to *look* for fences, not throw it away and
    /// parse the identical text again.
    #[test]
    fn test_dw_2_5_a_fence_free_document_is_parsed_once_and_a_rendered_one_twice() {
        let plain = "# Title\n\n```rust\nfn main() {}\n```\n";
        let before = crate::loader::parse_count();
        let doc = parse(plain);
        assert_eq!(crate::loader::parse_count() - before, 1);
        assert_eq!(doc.blocks().len(), 2);

        assert!(
            mermaid::render("graph TD\n  A-->B\n").is_ok(),
            "graph TD must render for the second half of this test to mean anything"
        );
        let with_fence = "```mermaid\ngraph TD\n  A-->B\n```\n";
        let before = crate::loader::parse_count();
        parse(with_fence);
        assert_eq!(crate::loader::parse_count() - before, 2);
    }

    /// `parse` and `preprocess` must not drift: whatever text the one splices
    /// is the text the other parses.
    #[test]
    fn test_parse_agrees_with_parsing_the_preprocessed_text() {
        for src in [
            "# Title\n\nplain\n",
            "```mermaid\ngraph TD\n  A-->B\n```\n",
            "```mermaid\n```\n",
        ] {
            assert_eq!(
                format!("{:?}", parse(src).blocks()),
                format!("{:?}", Document::parse(&preprocess(src)).blocks()),
                "parse and preprocess disagree on {src:?}"
            );
        }
    }
}
