//! Frontmatter visibility (`--frontmatter`, already parsed in `cli.rs`;
//! this module is what makes the flag mean something). YAML frontmatter is
//! hidden by default: [`apply`] strips the leading `---`-fenced block from
//! the source text *before* it ever reaches `ast::Document::parse`, so a
//! hidden block never lays out at all — not merely dimmed or styled away.
//! Passing `show_frontmatter: true` (the `--frontmatter` flag) leaves the
//! source untouched, so `ast`/`layout`'s existing `Semantic::FrontMatter`
//! handling (already committed, P2–P4) renders it as literal text exactly
//! as it does today.
//!
//! Wiring this in is one more line for the orchestrator, alongside
//! registering [`super::themed::ThemedDecor`]:
//! `let source = frontmatter::apply(&raw_source, cli.frontmatter);` before
//! `Document::parse(&source)`.

use std::borrow::Cow;

/// Returns `source` unchanged when `show_frontmatter` is `true`, or with
/// its leading YAML frontmatter block (if any) removed when `false`. Only
/// a frontmatter fence at the very start of the document counts (mirrors
/// `ast`'s own "fenced by `---` at the very start of the document" rule) —
/// a `---` appearing later (a thematic break, a setext heading underline)
/// is never touched.
pub fn apply(source: &str, show_frontmatter: bool) -> Cow<'_, str> {
    if show_frontmatter {
        return Cow::Borrowed(source);
    }
    match frontmatter_span(source) {
        Some(end) => Cow::Owned(source[end..].to_string()),
        None => Cow::Borrowed(source),
    }
}

/// The byte offset just past the closing `---` fence (and its trailing
/// newline, if present) of a leading frontmatter block, or `None` if
/// `source` doesn't open with one.
fn frontmatter_span(source: &str) -> Option<usize> {
    let mut offset = 0usize;
    let mut lines = source.split_inclusive('\n');
    let first = lines.next()?;
    if first.trim_end_matches(['\n', '\r']) != "---" {
        return None;
    }
    offset += first.len();
    for line in lines {
        offset += line.len();
        if line.trim_end_matches(['\n', '\r']) == "---" {
            return Some(offset);
        }
    }
    // Opened but never closed: not a valid frontmatter block (matches
    // `ast`'s own requirement of a matching closing fence) — leave the
    // source untouched rather than guessing where it should have ended.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "---\ntitle: stele fixture\ntags: [markdown, viewer]\ndraft: false\n---\n\n# After frontmatter\n\nBody text.\n";

    #[test]
    fn test_dw_7_5_frontmatter_hidden_by_default() {
        let hidden = apply(FIXTURE, false);
        assert!(!hidden.contains("title: stele fixture"));
        assert!(hidden.starts_with("\n# After frontmatter"));
    }

    #[test]
    fn test_dw_7_5_frontmatter_flag_shows_it_unchanged() {
        let shown = apply(FIXTURE, true);
        assert_eq!(shown, FIXTURE);
    }

    #[test]
    fn test_document_without_frontmatter_is_unaffected_either_way() {
        let plain = "# Just a heading\n\nBody.\n";
        assert_eq!(apply(plain, false), plain);
        assert_eq!(apply(plain, true), plain);
    }

    #[test]
    fn test_unclosed_frontmatter_fence_is_left_untouched() {
        let unclosed = "---\ntitle: no closing fence\n\n# Heading\n";
        assert_eq!(apply(unclosed, false), unclosed);
    }

    #[test]
    fn test_thematic_break_mid_document_is_not_mistaken_for_frontmatter() {
        let doc = "# Heading\n\nabc\n\n---\n\nmore text\n";
        assert_eq!(apply(doc, false), doc);
    }

    #[test]
    fn test_empty_source_never_panics() {
        assert_eq!(apply("", false), "");
        assert_eq!(apply("", true), "");
    }
}
