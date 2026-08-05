//! Quarto (and Pandoc, and Docusaurus) fenced divs, made readable.
//!
//! A `:::` div is Pandoc's generic container. Nothing in CommonMark knows
//! what it is, so a viewer shows the colons as literal text and the reader
//! gets a paragraph of punctuation around content that was meant to stand
//! out. The one class of div that *has* a CommonMark+GFM equivalent is the
//! callout, and stele already renders GFM alerts — so a callout is rewritten
//! into one and everything else is left exactly as the author wrote it.
//!
//! ```text
//! ::: {.callout-note}          > [!NOTE]
//! ## Title                 →   >
//! Body.                        > ## Title
//! :::                          > Body.
//! ```
//!
//! Three smaller rewrites travel with it, because they are the same
//! "Pandoc attribute syntax a CommonMark reader shows raw" problem: `{#id}`
//! trailing an ATX heading, `[text]{.class}` spans, and own-line
//! `{{< shortcode >}}` calls.
//!
//! ## Why every body line gets a `>`, blank ones included (R9)
//!
//! A blank line **ends** a blockquote. Prefixing only the non-blank lines of
//! a callout body would quote its first paragraph and release everything
//! after the first blank line back into the document — the rest of the
//! callout would escape, and the closing `:::` would come back as literal
//! text. So a blank body line becomes a bare `>`, and a line inside a fence
//! inside the callout gets the prefix too: to the blockquote it is body like
//! any other, and the fence would otherwise be half in and half out.
//!
//! ## Why divs are matched by colon-run length
//!
//! Pandoc nests divs by giving the outer one more colons — the corpus has a
//! five-colon div wrapping a three-colon one. Matching the closer by string
//! equality against `":::"` would close the outer div at the inner one's
//! closer and leave the document's structure inverted from there down. A
//! stack keyed on run length is the only thing that reads it the way Pandoc
//! does.
//!
//! ## Why an unclosed div stops the whole pass
//!
//! R10. A div left open at end of file means the nesting below it is
//! unknowable, and under `--watch` that is what a file looks like for the
//! moment between an editor's two writes. Guessing would reshuffle the
//! document for one frame; declining leaves it stable.

use std::borrow::Cow;

use super::fences::{Scan, container_content, strip_eol};

/// A `:::` line, once recognised.
#[derive(Debug, PartialEq, Eq)]
enum DivLine<'a> {
    /// Colon run length, and the div's first class if it declares one.
    Open(usize, Option<&'a str>),
    /// Colon run length.
    Close(usize),
}

/// One callout div that will become a GFM alert.
struct Callout {
    open: usize,
    close: usize,
    kind: &'static str,
}

/// Rewrites Quarto callouts, heading ids, spans and shortcodes in `source`.
///
/// Returns `source` untouched when there is nothing to rewrite, and — per
/// R10 — when a fence or a div is left open at end of file.
pub fn apply(source: &str) -> Cow<'_, str> {
    let scan = Scan::of(source);
    if !scan.balanced() {
        return Cow::Borrowed(source);
    }
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let Some(callouts) = callouts(&lines, &scan) else {
        return Cow::Borrowed(source);
    };

    // `depth[i]` counts the rewritten callouts that *contain* line `i` —
    // strictly contain, so a callout's own delimiters carry their parent's
    // depth and the nesting arithmetic below stays off by nothing.
    let mut depth = vec![0usize; lines.len()];
    let mut opens: Vec<Option<&'static str>> = vec![None; lines.len()];
    let mut drops = vec![false; lines.len()];
    for callout in &callouts {
        opens[callout.open] = Some(callout.kind);
        drops[callout.close] = true;
        for entry in &mut depth[callout.open + 1..callout.close] {
            *entry += 1;
        }
    }

    let mut out = String::new();
    let mut changed = !callouts.is_empty();
    for (index, raw) in lines.iter().enumerate() {
        let line = strip_eol(raw);
        let eol = &raw[line.len()..];

        if drops[index] {
            continue;
        }
        if let Some(kind) = opens[index] {
            // One line becomes two, and `eol` is reused for both. It cannot
            // be the empty string here: `eol` is only empty on a file's final
            // line, and a callout opener always has its own closer on a later
            // line, so an opener is never the final line.
            let quote = quote_prefix(depth[index] + 1);
            out.push_str(&quote);
            out.push_str(kind);
            out.push_str(eol);
            out.push_str(quote.trim_end());
            out.push_str(eol);
            continue;
        }

        // Protection governs the *inline* rewrites only. The quote prefix is
        // structural: a fence inside a callout body is still body, and
        // leaving its lines unquoted is exactly the escape R9 describes.
        let editable = !scan.is_protected(index);
        if editable && is_shortcode_line(line) {
            changed = true;
            continue;
        }
        let content = match editable.then(|| rewrite_inline(line)).flatten() {
            Some(rewritten) => {
                changed = true;
                Cow::Owned(rewritten)
            }
            None => Cow::Borrowed(line),
        };

        if depth[index] == 0 {
            out.push_str(&content);
        } else if content.is_empty() {
            // `>` rather than `> ` only for a line that was *empty*. A line
            // of spaces keeps them behind the marker: it is still a blank
            // line to the block parser, and inside a quoted code fence those
            // spaces are the reader's indentation.
            out.push_str(quote_prefix(depth[index]).trim_end());
        } else {
            out.push_str(&quote_prefix(depth[index]));
            out.push_str(&content);
        }
        out.push_str(eol);
    }

    if changed {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(source)
    }
}

/// `depth` levels of blockquote marker.
fn quote_prefix(depth: usize) -> String {
    "> ".repeat(depth)
}

/// Every callout div that will be rewritten, or `None` when the document's
/// divs do not balance and the pass must decline.
///
/// The stack is keyed on colon-run length. A closer pops the innermost open
/// div with the same run length and everything opened inside it; a closer
/// that matches nothing open is a malformed document, and so is a div still
/// open at the end.
fn callouts(lines: &[&str], scan: &Scan) -> Option<Vec<Callout>> {
    let mut stack: Vec<(usize, usize, Option<&'static str>)> = Vec::new();
    let mut found = Vec::new();
    for (index, raw) in lines.iter().enumerate() {
        if scan.is_protected(index) {
            continue;
        }
        let line = strip_eol(raw);
        match div_line(line) {
            None => {}
            Some(DivLine::Open(run, class)) => {
                // A callout is only rewritten when its opener is at the
                // document's top level. Reproducing the exact continuation
                // indent of a div nested in a list item is a different
                // problem, and getting it wrong breaks the list; leaving
                // such a callout alone merely leaves it looking like Quarto.
                let kind = class.filter(|_| is_top_level(line)).and_then(alert_kind);
                stack.push((run, index, kind));
            }
            Some(DivLine::Close(run)) => {
                let at = stack
                    .iter()
                    .rposition(|(open_run, _, _)| *open_run == run)?;
                for (_, open, kind) in stack.drain(at..) {
                    if let Some(kind) = kind {
                        found.push(Callout {
                            open,
                            close: index,
                            kind,
                        });
                    }
                }
            }
        }
    }
    stack.is_empty().then_some(found)
}

/// The `:::` line `line` is, if it is one.
///
/// Container markers are stripped first, so a closer indented four spaces
/// inside a list item — the commonest shape in the corpus — is still a
/// closer.
fn div_line(line: &str) -> Option<DivLine<'_>> {
    let body = container_content(line);
    let run = body.bytes().take_while(|&b| b == b':').count();
    if run < 3 {
        return None;
    }
    let rest = body[run..].trim();
    // Pandoc lets a closer (and an opener) carry a trailing colon run.
    if rest.trim_end_matches(':').is_empty() {
        return Some(DivLine::Close(run));
    }
    Some(DivLine::Open(run, div_class(rest)))
}

/// The first class named by a div's attribute text.
///
/// Two spellings, and they are treated differently on purpose. A braced
/// `{.callout-note}` is Pandoc's attribute syntax, where a class can be
/// anything at all — so only the `callout-` prefixed names are claimed, and
/// `::: {.warning}` (a real generic div in the corpus) is left alone. A bare
/// `::: callout-note` or `:::note` is Quarto's and Docusaurus's shorthand,
/// which exists *only* for admonitions, so the unprefixed names are safe to
/// claim there.
fn div_class(rest: &str) -> Option<&str> {
    match rest.strip_prefix('{') {
        Some(inner) => {
            let inner = inner.strip_suffix('}')?;
            inner
                .split_whitespace()
                .find_map(|token| token.strip_prefix('.'))
                .filter(|class| class.starts_with("callout-"))
        }
        None => rest.split_whitespace().next(),
    }
}

/// The GFM alert marker a callout class maps to.
///
/// Quarto's five kinds are exactly `ast::AlertKind`'s five, so the mapping is
/// total and needs no fallback. Docusaurus's `info` and `danger` are
/// deliberately absent: neither has an obvious counterpart here, and
/// inventing one would be a guess made on the reader's behalf.
///
/// The text emitted is what `crates/ast/src/parser/block.rs`'s
/// `finalize_blockquote` matches on — a first paragraph line that
/// `trim_end`s and uppercases to exactly `[!NOTE]`. Anything else is an
/// ordinary blockquote with a literal `[!NOTE]` in it.
fn alert_kind(class: &str) -> Option<&'static str> {
    match class.strip_prefix("callout-").unwrap_or(class) {
        "note" => Some("[!NOTE]"),
        "tip" => Some("[!TIP]"),
        "important" => Some("[!IMPORTANT]"),
        "warning" => Some("[!WARNING]"),
        "caution" => Some("[!CAUTION]"),
        _ => None,
    }
}

/// Whether `line` begins at the top level: at most three leading spaces, and
/// the colons immediately after them.
fn is_top_level(line: &str) -> bool {
    let indent = line.bytes().take_while(|&b| b == b' ').count();
    indent <= 3 && line[indent..].starts_with(':')
}

/// Whether the whole of `line` is a `{{< … >}}` shortcode call.
///
/// Only own-line calls are dropped. An inline mention — the corpus explains
/// the syntax with `` `{{{< video >}}}` `` mid-sentence — is prose about
/// shortcodes and belongs to the reader.
/// No length check rides along with these two: a string cannot both start
/// with `{{<` and end with `>}}` in fewer than six bytes, because the two
/// would have to overlap and disagree about the byte they share.
fn is_shortcode_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("{{<") && trimmed.ends_with(">}}")
}

/// `line` with its heading id and bracketed spans unwrapped, or `None` when
/// it has neither.
fn rewrite_inline(line: &str) -> Option<String> {
    let stripped = strip_heading_attributes(line);
    let unwrapped = unwrap_spans(stripped.as_ref());
    match (stripped, unwrapped) {
        (_, Some(text)) => Some(text),
        (Cow::Owned(text), None) => Some(text),
        (Cow::Borrowed(_), None) => None,
    }
}

/// An ATX heading with a trailing Pandoc attribute block removed.
///
/// Only `{#id …}` and `{.class …}` are taken, and only from a heading: a
/// paragraph ending in a brace group is far more likely to be code, JSON or
/// prose about braces than it is to be an attribute block.
fn strip_heading_attributes(line: &str) -> Cow<'_, str> {
    let indent = line.bytes().take_while(|&b| b == b' ').count();
    if indent > 3 {
        return Cow::Borrowed(line);
    }
    let hashes = line[indent..].bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) || !line[indent + hashes..].starts_with([' ', '\t']) {
        return Cow::Borrowed(line);
    }
    let trimmed = line.trim_end();
    let Some(open) = trimmed.rfind('{') else {
        return Cow::Borrowed(line);
    };
    if !trimmed.ends_with('}') {
        return Cow::Borrowed(line);
    }
    // `open` is the *last* `{`, so a `}` inside the group means the line ends
    // in a brace group that closed and reopened — `# H {.c} x}` — which is
    // prose about braces rather than one attribute block.
    let attributes = &trimmed[open + 1..trimmed.len() - 1];
    if !attributes.starts_with(['#', '.']) || attributes.contains('}') {
        return Cow::Borrowed(line);
    }
    Cow::Owned(trimmed[..open].trim_end().to_string())
}

/// `line` with every `[text]{.class}` span replaced by its text, or `None`
/// when it holds none.
fn unwrap_spans(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut out = String::new();
    let mut copied = 0usize;
    let mut cursor = 0usize;
    let mut rewrote = false;

    while cursor < bytes.len() {
        if bytes[cursor] != b'[' {
            cursor += 1;
            continue;
        }
        let Some((text, end)) = span_at(line, cursor) else {
            cursor += 1;
            continue;
        };
        // A span's replacement is the author's own text, so this pass adds no
        // delimiter of its own — but unwrapping two spans that each begin and
        // end with a code span still butts their backticks together, and
        // `[`a`]{.c}[`b`]{.c}` would come out as the single merged run
        // `` `a``b` ``. Same rule, same reason as the other two passes.
        let before = crate::decor::char_before(&out, line, copied, cursor);
        let after = line[end..].chars().next();
        out.push_str(&line[copied..cursor]);
        out.push_str(&crate::decor::spaced_to_not_merge(before, text, after));
        copied = end;
        cursor = end;
        rewrote = true;
    }

    if !rewrote {
        return None;
    }
    out.push_str(&line[copied..]);
    Some(out)
}

/// The span beginning at `start` — `(its text, offset just past it)`.
///
/// The brace group must be a Pandoc attribute block (`{.class}`, `{#id}`),
/// which is what keeps this away from a link's `[text](dest)` and from a
/// reference definition's `[label]: dest`.
fn span_at(line: &str, start: usize) -> Option<(&str, usize)> {
    let bytes = line.as_bytes();
    let mut cursor = start + 1;
    let mut depth = 1usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 1,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    // As in `codecogs::image_at`: the loop can only leave with `depth == 0`
    // by breaking at a `]`, so testing both is testing one thing twice. The
    // invariant is asserted instead, which says the same thing without
    // leaving behind a branch no input can reach.
    if depth != 0 {
        return None;
    }
    debug_assert_eq!(
        bytes.get(cursor),
        Some(&b']'),
        "depth reaches zero only at a `]`"
    );
    if bytes.get(cursor + 1) != Some(&b'{') {
        return None;
    }
    let attributes_start = cursor + 2;
    let end = line[attributes_start..].find('}')? + attributes_start;
    let attributes = &line[attributes_start..end];
    if !attributes.starts_with(['.', '#']) {
        return None;
    }
    Some((&line[start + 1..cursor], end + 1))
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use ast::{AlertKind, BlockKind, Document};

    use super::*;

    /// The alert kind the real parser reads out of `source`, which is the
    /// only oracle that matters: `finalize_blockquote` is strict about the
    /// exact bytes, and a rewrite the parser reads as an ordinary blockquote
    /// has failed however good the text looks.
    fn alert_of(source: &str) -> Option<AlertKind> {
        let doc = Document::parse(source);
        doc.blocks().iter().find_map(|block| match &block.kind {
            BlockKind::Alert { kind, .. } => Some(*kind),
            _ => None,
        })
    }

    /// Every opening spelling in the corpus, plus the Docusaurus one that is
    /// not in the corpus but is the commonest admonition in the wild.
    #[test]
    fn test_every_callout_spelling_becomes_the_alert_it_names() {
        let cases = [
            ("::: {.callout-note}", AlertKind::Note),
            (":::{.callout-note}", AlertKind::Note),
            ("::: callout-note", AlertKind::Note),
            (":::note", AlertKind::Note),
            ("::: {.callout-tip}", AlertKind::Tip),
            ("::: {.callout-important}", AlertKind::Important),
            ("::: {.callout-warning}", AlertKind::Warning),
            ("::: {.callout-caution}", AlertKind::Caution),
            (":::warning", AlertKind::Warning),
        ];
        for (opener, expected) in cases {
            let src = format!("{opener}\nBody text.\n:::\n");
            let out = apply(&src);
            assert_eq!(alert_of(&out), Some(expected), "for {opener}: {out:?}");
        }
    }

    /// R9, and the reason this pass exists in the shape it does. Without the
    /// bare `>` on the blank line, the blockquote ends there: the second
    /// paragraph escapes into the document and the closing `:::` comes back
    /// as literal text.
    #[test]
    fn test_a_callout_containing_a_blank_line_stays_one_blockquote() {
        let src = "::: {.callout-note}\nFirst paragraph.\n\nSecond paragraph.\n:::\n\nOutside.\n";
        let out = apply(src);
        let doc = Document::parse(&out);

        assert!(!out.contains(":::"), "the div fences must be gone: {out:?}");
        let BlockKind::Alert { kind, children } = &doc.blocks()[0].kind else {
            panic!("expected an alert, got {:?}", doc.blocks()[0].kind);
        };
        assert_eq!(*kind, AlertKind::Note);
        assert_eq!(
            children.len(),
            2,
            "both paragraphs must be inside the alert: {children:?}"
        );
        // And exactly one block escaped, which is the one that should have.
        assert_eq!(doc.blocks().len(), 2, "{:?}", doc.blocks());
        assert!(matches!(doc.blocks()[1].kind, BlockKind::Paragraph { .. }));
    }

    /// R9's other half: a fence inside the callout body is body too. Left
    /// unquoted its opening delimiter is outside the blockquote and its
    /// contents are not code at all.
    #[test]
    fn test_a_fence_inside_a_callout_body_is_quoted_with_the_rest_of_it() {
        let src = "::: {.callout-tip}\nTry this:\n\n```python\nx = 1\n\ny = 2\n```\n:::\n";
        let out = apply(src);
        let doc = Document::parse(&out);
        let BlockKind::Alert { kind, children } = &doc.blocks()[0].kind else {
            panic!("expected an alert, got {:?}", doc.blocks()[0].kind);
        };
        assert_eq!(*kind, AlertKind::Tip);
        assert_eq!(
            doc.blocks().len(),
            1,
            "nothing may escape: {:?}",
            doc.blocks()
        );
        let BlockKind::CodeBlock { literal, info, .. } = &children[1].kind else {
            panic!(
                "expected the fence to survive as code, got {:?}",
                children[1].kind
            );
        };
        assert_eq!(info.as_deref(), Some("python"));
        assert_eq!(
            literal, "x = 1\n\ny = 2\n",
            "the blank line inside the code must survive"
        );
    }

    /// The corpus's five-colon div wrapping a three-colon one. Matching a
    /// closer by string equality against `":::"` closes the outer div at the
    /// inner one's closer, and every div after that is off by one level.
    #[test]
    fn test_a_five_colon_div_is_closed_by_five_colons_and_not_by_three() {
        let src = concat!(
            "::::: {#special .sidebar}\n",
            "\n",
            "::: {.callout-warning}\n",
            "Here is a warning.\n",
            ":::\n",
            "\n",
            "More content.\n",
            ":::::\n",
        );
        let out = apply(src);
        // The sidebar is not a callout, so its colons stay; the warning's do
        // not. Getting the nesting wrong shows up as the wrong one going.
        assert!(out.contains("::::: {#special .sidebar}"), "{out:?}");
        assert!(out.contains("\n:::::\n"), "{out:?}");
        assert!(!out.contains("callout-warning"), "{out:?}");
        assert!(out.contains("> [!WARNING]"), "{out:?}");
        assert!(out.contains("> Here is a warning."), "{out:?}");
    }

    #[test]
    fn test_a_callout_nested_inside_a_callout_nests_its_blockquote() {
        let src = concat!(
            "::::: {.callout-note}\n",
            "Outer.\n",
            "\n",
            "::: {.callout-tip}\n",
            "Inner.\n",
            ":::\n",
            ":::::\n",
        );
        let out = apply(src);
        assert!(out.contains("> > [!TIP]"), "{out:?}");
        let doc = Document::parse(&out);
        let BlockKind::Alert { kind, children } = &doc.blocks()[0].kind else {
            panic!("expected an alert, got {:?}", doc.blocks()[0].kind);
        };
        assert_eq!(*kind, AlertKind::Note);
        let inner = children
            .iter()
            .find_map(|block| match &block.kind {
                BlockKind::Alert { kind, .. } => Some(*kind),
                _ => None,
            })
            .expect("the inner callout must be an alert inside the outer one");
        assert_eq!(inner, AlertKind::Tip);
    }

    /// The stack has to close the **innermost** div of a matching run length,
    /// not the first one it can find. A plain `:::` div inside a
    /// three-colon callout — the commonest nesting there is, since Quarto
    /// writes both at three colons — is where the difference shows: closing
    /// the outer one first leaves the callout's real closer with nothing
    /// open to match, and R10 then declines the whole document.
    #[test]
    fn test_a_same_length_div_nested_in_a_callout_closes_before_the_callout() {
        let src = concat!(
            "::: {.callout-note}\n",
            "::: {.border}\n",
            "x\n",
            ":::\n",
            "y\n",
            ":::\n",
        );
        let out = apply(src);
        assert_eq!(
            &*out, "> [!NOTE]\n>\n> ::: {.border}\n> x\n> :::\n> y\n",
            "the inner div stays, quoted, and only the callout's own fences go"
        );
        let doc = Document::parse(&out);
        assert_eq!(alert_of(&out), Some(AlertKind::Note));
        assert_eq!(
            doc.blocks().len(),
            1,
            "nothing may escape: {:?}",
            doc.blocks()
        );
    }

    /// A callout that is not at the document's top level is left alone —
    /// reproducing a list item's continuation indent is a different problem,
    /// and getting it wrong breaks the list rather than the callout.
    #[test]
    fn test_a_callout_indented_into_a_list_item_is_not_rewritten() {
        let src = "- item\n\n    ::: {.callout-note}\n    Body.\n    :::\n";
        assert!(matches!(apply(src), Cow::Borrowed(_)));
        // Three spaces is still top level, so that one *is* rewritten — the
        // boundary is real rather than a blanket refusal to touch anything
        // indented.
        assert!(matches!(
            apply("   ::: {.callout-note}\n   Body.\n   :::\n"),
            Cow::Owned(_)
        ));
    }

    /// A closing fence written as colons and spaces is still a closing fence.
    /// Pandoc permits the trailing run, and a line that holds nothing but
    /// colons cannot be opening anything.
    #[test]
    fn test_a_closer_written_as_two_colon_runs_still_closes_the_callout() {
        let out = apply("::: {.callout-note}\nBody.\n::: :::\n");
        assert_eq!(&*out, "> [!NOTE]\n>\n> Body.\n");
        assert_eq!(alert_of(&out), Some(AlertKind::Note));
    }

    /// Everything that is not one of the five callout kinds is left exactly
    /// as written — including a braced `{.warning}`, which is a generic
    /// Pandoc div in this corpus and not an admonition at all.
    #[test]
    fn test_an_unrecognised_div_class_is_left_completely_untouched() {
        for opener in [
            "::: {.border .p-3}",
            "::: {.list-table header-rows=1 tbl-colwidths=\"[50, 50]\"}",
            "::: pad-to-code-block",
            "::: {.hidden}",
            "::: {.warning}",
            "::: {}",
            ":::info",
            ":::danger",
        ] {
            let src = format!("{opener}\nBody.\n:::\n");
            assert!(
                matches!(apply(&src), Cow::Borrowed(_)),
                "{opener} was rewritten"
            );
        }
    }

    /// The closers in the corpus's list-table are indented four spaces
    /// inside a list item. A scanner anchored at column 0 never sees them,
    /// leaves the stack non-empty, and — by R10 — would decline the whole
    /// document.
    #[test]
    fn test_indented_closers_inside_a_list_item_still_balance() {
        let src = concat!(
            "::: {.list-table}\n",
            "- - ``` markdown\n",
            "    <https://quarto.org>\n",
            "    ```\n",
            "  - ::: pad-to-code-block\n",
            "    <https://quarto.org>\n",
            "    :::\n",
            ":::\n",
            "\n",
            "::: {.callout-note}\n",
            "After the table.\n",
            ":::\n",
        );
        let out = apply(src);
        assert!(
            out.contains("> [!NOTE]"),
            "the callout after a balanced list-table must still be rewritten: {out:?}"
        );
        assert!(out.contains("  - ::: pad-to-code-block"), "{out:?}");
    }

    /// R10: an unclosed div means the nesting below it is unknowable, so the
    /// document comes back exactly as it was.
    #[test]
    fn test_an_unclosed_div_leaves_the_whole_document_untouched() {
        let src = "::: {.callout-note}\nHalf a save.\n\n## Heading {#an-id}\n";
        assert!(matches!(apply(src), Cow::Borrowed(_)));
        // A stray closer with nothing open is the same kind of malformed.
        let src = "Text.\n\n:::\n\n## Heading {#an-id}\n";
        assert!(matches!(apply(src), Cow::Borrowed(_)));
    }

    #[test]
    fn test_an_unclosed_fence_leaves_the_whole_document_untouched() {
        let src = "```\n::: {.callout-note}\nBody.\n:::\n";
        assert!(matches!(apply(src), Cow::Borrowed(_)));
    }

    #[test]
    fn test_a_div_inside_a_fence_is_shown_rather_than_rewritten() {
        let src = "``` markdown\n::: {.callout-note}\nBody.\n:::\n```\n";
        assert!(matches!(apply(src), Cow::Borrowed(_)));
    }

    /// A grid table's columns are positional. This callout sits inside a
    /// cell, where changing the line's length by even one byte moves every
    /// `|` after it.
    #[test]
    fn test_a_callout_inside_a_grid_table_cell_is_left_byte_for_byte() {
        let src = concat!(
            "+---------------------+---------------------+\n",
            "| Markdown            | Output              |\n",
            "+=====================+=====================+\n",
            "| ::: {.callout-note} | A note callout.     |\n",
            "| Body.               |                     |\n",
            "| :::                 |                     |\n",
            "+---------------------+---------------------+\n",
        );
        assert!(matches!(apply(src), Cow::Borrowed(_)));
    }

    #[test]
    fn test_a_heading_id_is_stripped_and_a_braced_paragraph_is_not() {
        assert_eq!(&*apply("## Headings {#headings}\n"), "## Headings\n");
        assert_eq!(
            &*apply("## Divs and Spans {#sec-divs-and-spans}\n"),
            "## Divs and Spans\n"
        );
        assert_eq!(&*apply("# Heading {.heading-output}\n"), "# Heading\n");
        // Not a heading, and not an attribute block.
        assert!(matches!(
            apply("A paragraph ending in {a brace}\n"),
            Cow::Borrowed(_)
        ));
        assert!(matches!(
            apply("## A heading about {json: 1}\n"),
            Cow::Borrowed(_)
        ));
        assert!(matches!(apply("#not-a-heading {#id}\n"), Cow::Borrowed(_)));
    }

    #[test]
    fn test_a_bracketed_span_is_unwrapped_and_a_link_is_not() {
        assert_eq!(
            &*apply("[This text is smallcaps]{.smallcaps}\n"),
            "This text is smallcaps\n"
        );
        assert_eq!(
            &*apply("[This is *some text*]{.class key=\"val\"}\n"),
            "This is *some text*\n"
        );
        // A link, a reference definition and a footnote reference all share
        // the opening bracket and none of them is a span.
        assert!(matches!(
            apply("[Quarto](https://quarto.org)\n"),
            Cow::Borrowed(_)
        ));
        assert!(matches!(
            apply("[label]: https://example.com\n"),
            Cow::Borrowed(_)
        ));
        assert!(matches!(apply("A note[^1] here.\n"), Cow::Borrowed(_)));
    }

    /// [`span_at`] is a byte scanner that decides which bytes of a reader's
    /// line get replaced, so its edges matter more than its middle. Span text
    /// is arbitrary author prose: it holds brackets, and it holds backslashes
    /// in front of brackets.
    #[test]
    fn test_span_at_finds_the_whole_span_however_its_text_is_bracketed() {
        // (line, unwrapped text, offset just past the closing brace)
        let cases = [
            ("[plain]{.c}", "plain", 11),
            ("[a [b] c]{.c}", "a [b] c", 13),
            ("[[deep]]{.c}", "[deep]", 12),
            ("[a \\] b]{.c}", "a \\] b", 12),
            ("[x]{#id}", "x", 8),
            ("[x]{.c key=\"v\"}", "x", 15),
        ];
        for (line, text, end) in cases {
            assert_eq!(span_at(line, 0), Some((text, end)), "for {line:?}");
        }
    }

    /// The refusals, which are the dangerous half — a scanner that keeps
    /// hunting for a `}` after a `]` that was never a span's will unwrap
    /// bytes that belonged to something else. The link, reference and
    /// footnote cases are checked against the real parser, which is what
    /// actually decides what those brackets mean.
    #[test]
    fn test_span_at_refuses_brackets_that_belong_to_something_else() {
        for line in [
            "[link](https://example.com)",
            "[reference][ref]",
            "[label]: https://example.com",
            "a footnote[^1] here",
            // A `]` with a brace group that is not a Pandoc attribute block.
            "[x]{not-an-attribute}",
            "[x] {.c}",
            // Truncated in each of the three places it can be truncated.
            "[unclosed text",
            "[x]{unterminated",
            "[x]",
        ] {
            let start = line.find('[').expect("every case here holds a bracket");
            assert_eq!(span_at(line, start), None, "span_at accepted {line:?}");
        }

        // And at document level: the three real constructs keep their own
        // node kinds rather than being flattened into text.
        let doc = Document::parse(&apply(
            "A [link](https://x.test), a [ref][r] and a note[^1].\n\n[r]: https://y.test\n\n[^1]: F.\n",
        ));
        let dumped = format!("{:?}", doc.blocks());
        assert!(dumped.contains("Link"), "{dumped}");
        assert!(dumped.contains("FootnoteReference"), "{dumped}");
    }

    /// Where the span scanner resumes is as load-bearing as where it starts:
    /// too short and it re-reads bytes it consumed, too long and it steps
    /// over the next span. Two spans on a line, the first bracket-heavy,
    /// shows both.
    #[test]
    fn test_a_second_span_on_the_line_survives_a_bracket_heavy_first_one() {
        assert_eq!(
            &*apply("[a [b] c]{.smallcaps} and [d]{.mark} end\n"),
            "a [b] c and d end\n"
        );
        // An unclosed bracket earlier on the line must not stop the scanner
        // from reaching the real span after it.
        assert_eq!(
            &*apply("see [unclosed and [x]{.mark}\n"),
            "see [unclosed and x\n"
        );
    }

    /// The heading-attribute rule has three edges: how deep the indent may
    /// be, how many hashes count, and what may sit inside the brace group.
    #[test]
    fn test_the_heading_attribute_rule_holds_at_each_of_its_edges() {
        // Three spaces is still a heading; four is an indented code block.
        assert_eq!(&*apply("   ## H {#id}\n"), "   ## H\n");
        assert!(matches!(apply("    ## H {#id}\n"), Cow::Borrowed(_)));
        // One hash and six hashes are headings; seven is not.
        assert_eq!(&*apply("# H {#id}\n"), "# H\n");
        assert_eq!(&*apply("###### H {#id}\n"), "###### H\n");
        assert!(matches!(apply("####### H {#id}\n"), Cow::Borrowed(_)));
        // A hash run with no space after it is not a heading.
        assert!(matches!(apply("##H {#id}\n"), Cow::Borrowed(_)));
        // A brace group that closed and reopened is prose about braces.
        assert!(matches!(apply("## H {.c} x}\n"), Cow::Borrowed(_)));
        // Trailing whitespace after the group is still the group.
        assert_eq!(&*apply("## H {#id}   \n"), "## H\n");
    }

    #[test]
    fn test_an_own_line_shortcode_is_dropped_and_an_inline_mention_is_not() {
        assert_eq!(
            &*apply("a\n\n{{< include _kbd.qmd >}}\n\nb\n"),
            "a\n\n\nb\n"
        );
        assert!(matches!(
            apply("You can include videos using the `{{{< video >}}}` shortcode.\n"),
            Cow::Borrowed(_)
        ));
    }

    /// Deleting a whole line is the most destructive thing this pass does, so
    /// it takes **both** ends of the shape to earn it. Either end alone is
    /// something else: a line that only opens is a shortcode caught
    /// half-written — which is exactly what `--watch` reads between an
    /// editor's two saves — and a line that only closes is a sentence that
    /// happens to end with the syntax it was describing. Dropping either
    /// would delete a line of the reader's document on the strength of three
    /// characters.
    #[test]
    fn test_a_line_needs_both_ends_of_a_shortcode_before_it_is_deleted() {
        for kept in [
            // Opens but never closes: half a save.
            "{{< include _kbd.qmd",
            "{{<",
            // Closes but never opens: prose about the syntax.
            "The syntax is {{< video >}}",
            ">}}",
            // Neither end.
            "{{ include }}",
        ] {
            let src = format!("a\n\n{kept}\n\nb\n");
            assert!(
                matches!(apply(&src), Cow::Borrowed(_)),
                "deleted a line that is not a whole shortcode: {kept:?}"
            );
        }
        // The complete shape still goes, so the rule above is a restriction
        // rather than a refusal.
        assert_eq!(&*apply("a\n\n{{< video >}}\n\nb\n"), "a\n\n\nb\n");
    }

    #[test]
    fn test_an_ordinary_document_comes_back_borrowed() {
        let src = "# Title\n\nA paragraph with a [link](x) and `code`.\n\n> [!NOTE]\n> Already an alert.\n";
        assert!(matches!(apply(src), Cow::Borrowed(_)));
    }

    #[test]
    fn test_crlf_line_endings_survive_the_rewrite() {
        let src = "::: {.callout-note}\r\nBody.\r\n:::\r\n";
        assert_eq!(&*apply(src), "> [!NOTE]\r\n>\r\n> Body.\r\n");
    }

    /// `mermaid.rs:280`'s drift test for this pass. The claim is about the
    /// parsed document — five callout kinds in, five `AlertKind`s out, and
    /// nothing left over — so it is checked there rather than on the text.
    #[test]
    fn test_the_rewritten_text_parses_to_the_alerts_it_claims() {
        let src = concat!(
            "::: {.callout-note}\nN\n:::\n\n",
            "::: {.callout-tip}\nT\n:::\n\n",
            "::: {.callout-important}\nI\n:::\n\n",
            "::: {.callout-warning}\nW\n:::\n\n",
            "::: {.callout-caution}\nC\n:::\n",
        );
        let out = apply(src);
        let doc = Document::parse(&out);
        let kinds: Vec<AlertKind> = doc
            .blocks()
            .iter()
            .filter_map(|block| match &block.kind {
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
                AlertKind::Caution,
            ]
        );
        assert_eq!(doc.blocks().len(), 5, "{:?}", doc.blocks());
        assert!(!format!("{:?}", doc.blocks()).contains(":::"));
    }
}
