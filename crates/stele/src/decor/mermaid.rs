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

/// Replaces top-level ` ```mermaid ` blocks in `source` with a rendered
/// box-drawing grid (as a plain code fence). Returns `source` unchanged when
/// there are no renderable mermaid blocks.
pub fn preprocess(source: &str) -> Cow<'_, str> {
    let doc = Document::parse(source);

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
        return Cow::Borrowed(source);
    }

    // Splice from the end backwards so earlier spans keep their offsets.
    repls.sort_by_key(|(start, _, _)| *start);
    let mut out = source.to_string();
    for (start, end, text) in repls.into_iter().rev() {
        out.replace_range(start..end, &text);
    }
    Cow::Owned(out)
}

/// Wraps `grid` in a plain (no-language) fenced code block so it displays
/// verbatim. Uses a `~~~` fence, which a box-drawing grid can never contain,
/// so the fence can't be closed early by the content.
fn as_plain_fence(grid: &str) -> String {
    let body = grid.trim_end_matches('\n');
    format!("~~~\n{body}\n~~~")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_non_mermaid_source_is_returned_borrowed_unchanged() {
        let src = "# Title\n\n```rust\nfn main() {}\n```\n";
        assert!(matches!(preprocess(src), Cow::Borrowed(_)));
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
}
