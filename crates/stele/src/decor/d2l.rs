//! Dive-into-Deep-Learning source markdown, made readable.
//!
//! d2l's books are written in a Sphinx-flavoured markdown that its own build
//! tool expands. A plain CommonMark reader shows the machinery instead of the
//! text: `:label:` lines that name nothing a reader can see, `:numref:` roles
//! where a figure number belongs, `:begin_tab:` markers around framework
//! variants, and every code block tagged ` ```{.python .input} ` — an info
//! string no syntax highlighter recognises, so the Python in a Python book
//! renders grey.
//!
//! Four rewrites, all of them subtractive:
//!
//! | Source | Becomes |
//! |---|---|
//! | an own-line `` :label:`x` ``, `` :eqlabel:`x` ``, `` :width:`320px` `` | nothing |
//! | an own-line `` :begin_tab:`x` `` / `:end_tab:` | nothing; the content between stays |
//! | `` :numref:`x` ``, `` :eqref:`x` `` | `` `x` `` — the target, as a code span |
//! | `` :cite:`a,b` `` | `(a, b)` |
//! | `` :citet:`a` `` | `a` |
//! | ` ```{.python .input} ` + `%%tab all` | ` ```python `, and the `%%tab` line goes |
//!
//! ## Why the citation forms differ
//!
//! `:cite:` is a parenthetical citation and `:citet:` a textual one — d2l
//! renders the first as "(Legendre, 1805)" and the second as "Legendre
//! (1805)", mid-sentence. Keeping that distinction costs nothing and reads
//! the way the author wrote the sentence around it.
//!
//! ## What dropping `:begin_tab:` costs
//!
//! The marker carries the framework name, and nothing downstream can show
//! it: GFM has no tab construct, so the alternative to dropping the marker is
//! leaving `:begin_tab:`mxnet`` on screen as literal text. A file that ends
//! with four framework-specific "Discussions" links therefore reads as four
//! identical paragraphs. That is a real loss of information, taken knowingly
//! — the content between the markers is what the reader came for, and the
//! markers themselves were never readable.
//!
//! Parentheses rather than the more conventional square brackets because a
//! `` :citet:`Field.1987` `` really does appear **inside image alt text** in
//! the corpus (`![Figure taken from :citet:`Field.1987`: …](…)`). Brackets
//! there would sit inside the image's own `[…]` label, where the parser has
//! to count them; parentheses cannot be mistaken for anything.

use std::borrow::Cow;

use super::fences::{Scan, strip_eol};

/// The roles that are dropped when they own a line outright. `:end_tab:`
/// carries no value; the rest are followed by a backtick-quoted argument.
const OWN_LINE_ROLES: [&str; 4] = [":label:", ":eqlabel:", ":width:", ":begin_tab:"];

/// Rewrites d2l's Sphinx roles and code fences in `source`.
///
/// Returns `source` untouched when there is nothing to rewrite, and — per
/// R10 — when a fence is left open at end of file.
pub fn apply(source: &str) -> Cow<'_, str> {
    let scan = Scan::of(source);
    if !scan.balanced() {
        return Cow::Borrowed(source);
    }
    let lines: Vec<&str> = source.split_inclusive('\n').collect();

    /// What happens to one line. `Keep` copies it through verbatim.
    enum Edit {
        Drop,
        Replace(String),
    }
    let mut edits: Vec<Option<Edit>> = (0..lines.len()).map(|_| None).collect();

    // Fences first, because the fence pass is the only one allowed to touch
    // a protected line — it is editing the block's *own* delimiter and its
    // first body line, not its contents.
    for fence in scan.fences() {
        if !fence.top_level {
            continue;
        }
        let Some(language) = input_fence_language(&fence.info) else {
            continue;
        };
        let open_line = strip_eol(lines[fence.open]);
        let indent = &open_line[..open_line.len() - open_line.trim_start().len()];
        edits[fence.open] = Some(Edit::Replace(format!("{indent}```{language}")));

        // `%%tab all` is a build-tool directive on the block's first line. It
        // is only ever the *first* line, so a `%%tab` deeper in the block is
        // real Python (a comment, a string) and is left alone.
        let body = fence.open + 1;
        if fence.close.is_some_and(|close| close > body)
            && strip_eol(lines[body]).trim_start().starts_with("%%tab")
        {
            edits[body] = Some(Edit::Drop);
        }
    }

    for (index, raw) in lines.iter().enumerate() {
        if scan.is_protected(index) || edits[index].is_some() {
            continue;
        }
        let line = strip_eol(raw);
        if is_own_line_role(line) {
            edits[index] = Some(Edit::Drop);
        } else if let Some(rewritten) = rewrite_roles(line) {
            edits[index] = Some(Edit::Replace(rewritten));
        }
    }

    let mut out = String::new();
    let mut changed = false;
    for (index, raw) in lines.iter().enumerate() {
        match &edits[index] {
            None => out.push_str(raw),
            Some(Edit::Drop) => changed = true,
            Some(Edit::Replace(text)) => {
                changed = true;
                out.push_str(text);
                out.push_str(&raw[strip_eol(raw).len()..]);
            }
        }
    }
    if changed {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(source)
    }
}

/// Whether `line` is nothing but a role that should disappear.
///
/// The whole line has to be the role. `:numref:` and `:cite:` open sentences
/// in this corpus (`:numref:`fig_single_neuron` depicts …`), so "starts with
/// a colon" is not enough to justify deleting a line — and those two are not
/// in the list anyway, precisely because they carry meaning a reader wants.
fn is_own_line_role(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed == ":end_tab:" {
        return true;
    }
    OWN_LINE_ROLES.iter().any(|role| {
        trimmed
            .strip_prefix(role)
            .and_then(|rest| rest.strip_prefix('`'))
            .and_then(|rest| rest.strip_suffix('`'))
            .is_some_and(|value| !value.is_empty() && !value.contains('`'))
    })
}

/// `line` with its inline roles rewritten, or `None` when it holds none.
fn rewrite_roles(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut out = String::new();
    let mut copied = 0usize;
    let mut cursor = 0usize;
    let mut rewrote = false;

    while cursor < bytes.len() {
        if bytes[cursor] != b':' {
            cursor += 1;
            continue;
        }
        // A role directly behind a backtick is *inside a code span* — the
        // shape `` `:numref:` `` is how prose talks *about* the role, and
        // rewriting it would delete the thing the sentence is naming.
        //
        // Unless that backtick is one this loop just consumed itself.
        // `cursor == copied` means the previous role's own closing backtick
        // is the byte behind us, so `:eqref:`a`:eqref:`b`` — two roles with
        // nothing between them — would otherwise lose its second half to a
        // code span that does not exist.
        //
        // No `cursor > 0` guard rides along with it: `cursor != copied`
        // already implies one, because `cursor` never falls behind `copied`
        // and the two start equal at zero. `cursor` is therefore at least
        // one here and `cursor - 1` cannot wrap.
        if cursor != copied && bytes[cursor - 1] == b'`' {
            cursor += 1;
            continue;
        }
        let Some((role, key, end)) = role_at(line, cursor) else {
            cursor += 1;
            continue;
        };
        // Two roles with nothing between them would emit two code spans back
        // to back, and their backticks merge into one run that CommonMark
        // does not read as two spans at all — `:eqref:`a`:eqref:`b`` came out
        // as the literal `a``b`, backticks and all, in the reader's prose.
        // The neighbour on the *other* side matters just as much: a role
        // written flush against the author's own code span merges with that.
        //
        // Nothing in the corpus writes roles adjacently; this exists because
        // a pass that rewrites a reader's document must fail toward *their*
        // text, never toward something neither they nor the parser meant.
        // See `super::spaced_to_not_merge` for why a space, and not a
        // declined rewrite.
        let before = crate::decor::char_before(&out, line, copied, cursor);
        let after = line[end..].chars().next();
        let rendered = render_role(role, key);
        out.push_str(&line[copied..cursor]);
        out.push_str(&crate::decor::spaced_to_not_merge(before, &rendered, after));
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

/// The role beginning at `start` — `(role, argument, offset just past it)`.
///
/// The argument must be non-empty and hold no whitespace. Every key in the
/// corpus is a bare identifier or a comma-joined list of them, and the
/// restriction is what stops a stray colon in prose from pairing with a
/// distant backtick and swallowing half a sentence.
fn role_at(line: &str, start: usize) -> Option<(&'static str, &str, usize)> {
    let rest = &line[start..];
    let role = [":numref:", ":eqref:", ":cite:", ":citet:"]
        .into_iter()
        .find(|role| rest.starts_with(role))?;
    let after = rest.strip_prefix(role)?.strip_prefix('`')?;
    let close = after.find('`')?;
    let key = &after[..close];
    if key.is_empty() || key.contains(char::is_whitespace) {
        return None;
    }
    let end = start + role.len() + 1 + close + 1;
    Some((role, key, end))
}

/// The plain text a role's argument becomes.
///
/// No trimming of the split keys: [`role_at`] has already refused any
/// argument with whitespace anywhere in it, so there is nothing here for a
/// trim to remove. The empty-key filter *is* reachable — `` :cite:`a,,b` ``
/// is a typo an author can make, and it should not print a citation slot
/// with nothing in it.
fn render_role(role: &str, key: &str) -> String {
    let keys: Vec<&str> = key.split(',').filter(|k| !k.is_empty()).collect();
    match role {
        ":cite:" => format!("({})", keys.join(", ")),
        ":citet:" => keys.join(", "),
        // `:numref:`/`:eqref:` point at a labelled figure, section or
        // equation. Without d2l's numbering pass there is no number to
        // print, so the target's own name is the most honest stand-in — and
        // as a code span it reads as an identifier rather than as prose.
        _ => format!("`{}`", keys.join(", ")),
    }
}

/// The language a ` ```{.python .input} `-style info string names, or `None`
/// when the fence is not one of d2l's executable blocks.
///
/// `.input` is the marker that makes this d2l's rather than Pandoc's: a
/// generic Quarto ` ```{.python} ` block is left alone, as is ` ```{mermaid} `
/// (which stele renders itself) and ` ```{=html} `.
fn input_fence_language(info: &str) -> Option<String> {
    let inner = info.strip_prefix('{')?.strip_suffix('}')?;
    let classes: Vec<&str> = inner
        .split_whitespace()
        .filter_map(|token| token.strip_prefix('.'))
        .collect();
    if !classes.contains(&"input") {
        return None;
    }
    let language = classes.into_iter().find(|class| *class != "input")?;
    if language.is_empty() {
        return None;
    }
    Some(language.to_string())
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use ast::{BlockKind, Document, InlineKind};

    use super::*;

    /// Every line here is copied verbatim out of the corpus files this pass
    /// was built for, so the expectations are pinned to real input rather
    /// than to input invented to match the implementation.
    #[test]
    fn test_the_own_line_roles_from_the_corpus_all_disappear() {
        let src = concat!(
            "# Linear Regression\n",
            ":label:`sec_linear_regression`\n",
            "\n",
            "![Transformer](../img/transformer.svg)\n",
            ":width:`320px`\n",
            ":label:`fig_transformer`\n",
            "\n",
            "$$x = 1$$\n",
            ":eqlabel:`eq_price-area`\n",
            "\n",
            ":begin_tab:`mxnet`\n",
            "\n",
            "Use the MXNet build.\n",
            "\n",
            ":end_tab:\n",
        );
        let out = apply(src);
        assert!(!out.contains(":label:"));
        assert!(!out.contains(":eqlabel:"));
        assert!(!out.contains(":width:"));
        assert!(!out.contains(":begin_tab:"));
        assert!(!out.contains(":end_tab:"));
        // The content between the tab markers is what the markers were
        // wrapping; dropping it would be the actual bug.
        assert!(out.contains("Use the MXNet build."));

        // The oracle: the parser sees a heading, an image paragraph, display
        // maths and one prose paragraph — no stray directive text anywhere.
        let doc = Document::parse(&out);
        assert!(matches!(
            doc.blocks()[0].kind,
            BlockKind::Heading { level: 1, .. }
        ));
        assert_eq!(doc.blocks().len(), 4, "{:?}", doc.blocks());
    }

    /// `:numref:` and `:cite:` open sentences in this corpus. A rule that
    /// deleted lines starting with a colon-role would delete the sentence.
    #[test]
    fn test_a_role_that_opens_a_sentence_is_rewritten_not_deleted() {
        let src = ":numref:`fig_single_neuron` depicts the model.\n";
        let out = apply(src);
        assert_eq!(&*out, "`fig_single_neuron` depicts the model.\n");
        assert!(!is_own_line_role(
            ":numref:`fig_single_neuron` depicts the model."
        ));
    }

    #[test]
    fn test_a_multi_key_citation_keeps_every_key_it_was_given() {
        let src = "Least squares :cite:`Legendre.1805,Gauss.1809` is old.\n";
        assert_eq!(
            &*apply(src),
            "Least squares (Legendre.1805, Gauss.1809) is old.\n"
        );
        let src = "See :citet:`Hubel.Wiesel.1959,Hubel.Wiesel.1962` for more.\n";
        assert_eq!(
            &*apply(src),
            "See Hubel.Wiesel.1959, Hubel.Wiesel.1962 for more.\n"
        );
        // A doubled comma is a typo, not an anonymous third citation.
        assert_eq!(&*apply("x :cite:`a,,b` y\n"), "x (a, b) y\n");
        assert_eq!(&*apply("x :cite:`a,` y\n"), "x (a) y\n");
    }

    /// The one place a role is nested inside another construct in the whole
    /// corpus. Asserted through the parser: the image must survive as an
    /// image, with its destination intact and the citation folded into its
    /// alt text.
    #[test]
    fn test_a_citation_inside_image_alt_text_does_not_corrupt_the_image() {
        let src = "![Figure taken from :citet:`Field.1987`: coding with six channels.](https://example.com/field.png)\n";
        let out = apply(src);
        let doc = Document::parse(&out);
        let BlockKind::Paragraph { children } = &doc.blocks()[0].kind else {
            panic!("expected a paragraph, got {:?}", doc.blocks()[0].kind);
        };
        assert_eq!(children.len(), 1, "{children:?}");
        let InlineKind::Image { dest, children, .. } = &children[0].kind else {
            panic!(
                "the image must still be an image, got {:?}",
                children[0].kind
            );
        };
        assert_eq!(dest, "https://example.com/field.png");
        let alt: String = children
            .iter()
            .map(|inline| match &inline.kind {
                InlineKind::Text(text) => text.as_str(),
                _ => "",
            })
            .collect();
        assert_eq!(
            alt,
            "Figure taken from Field.1987: coding with six channels."
        );
    }

    /// Every `%%tab` spelling that occurs in the corpus, plus the fence
    /// rewrite that makes the Python highlight as Python.
    #[test]
    fn test_every_tab_variant_is_dropped_and_the_fence_becomes_python() {
        for tab in [
            "%%tab all",
            "%%tab mxnet",
            "%%tab mxnet, pytorch",
            "%%tab pytorch, mxnet, tensorflow",
            "%%tab mxnet, tensorflow, jax",
        ] {
            let src = format!("```{{.python .input}}\n{tab}\nx = 1\n```\n");
            let out = apply(&src);
            assert_eq!(&*out, "```python\nx = 1\n```\n", "for {tab}");

            let doc = Document::parse(&out);
            let BlockKind::CodeBlock { info, literal, .. } = &doc.blocks()[0].kind else {
                panic!("expected a code block, got {:?}", doc.blocks()[0].kind);
            };
            assert_eq!(info.as_deref(), Some("python"));
            assert_eq!(literal, "x = 1\n");
        }
    }

    /// `%%tab` is a directive only on the block's first line. Anywhere else
    /// it is a Python comment, a string, or a magic — the reader's, not the
    /// build tool's.
    #[test]
    fn test_a_tab_line_that_is_not_the_first_line_of_the_block_survives() {
        let src = "```{.python .input}\nx = 1\n%%tab all\n```\n";
        assert_eq!(&*apply(src), "```python\nx = 1\n%%tab all\n```\n");
    }

    /// The `.input` marker is what makes a fence d2l's. Quarto and Pandoc
    /// brace-attribute fences use the same syntax for other purposes, and
    /// `{mermaid}` is a block stele renders itself.
    #[test]
    fn test_only_input_fences_are_retagged() {
        assert_eq!(
            input_fence_language("{.python .input}").as_deref(),
            Some("python")
        );
        assert_eq!(
            input_fence_language("{.input .python}").as_deref(),
            Some("python")
        );
        assert_eq!(input_fence_language("{.python}"), None);
        assert_eq!(input_fence_language("{mermaid}"), None);
        assert_eq!(input_fence_language("{=html}"), None);
        assert_eq!(input_fence_language("python"), None);
        assert_eq!(input_fence_language("{.input}"), None);
    }

    /// Two roles with nothing between them stay two code spans.
    ///
    /// Emitting them back to back merges their backticks into one run that
    /// CommonMark does not read as two spans, and the reader gets the literal
    /// `` a``b `` — backticks and all — in their prose. The oracle is the
    /// parser, not the string: whatever separator this pass chooses, what has
    /// to be true is that `ast` sees two `Code` nodes and no stray backtick
    /// survives in a `Text` node.
    ///
    /// The input is synthetic; nothing in the corpus writes roles adjacently.
    /// It is pinned anyway because a pass that rewrites a reader's document
    /// has to fail toward *their* text, and the old behaviour — dropping the
    /// second role entirely — at least failed safe. This one did not.
    #[test]
    fn test_two_adjacent_roles_stay_two_code_spans_instead_of_merging_their_backticks() {
        let rewritten = apply(":eqref:`eq_a`:eqref:`eq_b`\n");
        let doc = ast::Document::parse(&rewritten);
        let codes: Vec<String> = doc
            .nodes()
            .filter_map(|n| match n {
                ast::NodeRef::Inline(i) => match &i.kind {
                    ast::InlineKind::Code(text) => Some(text.clone()),
                    _ => None,
                },
                ast::NodeRef::Block(_) => None,
            })
            .collect();
        assert_eq!(
            codes,
            vec!["eq_a".to_string(), "eq_b".to_string()],
            "both targets must survive as their own code spans; got {rewritten:?}"
        );
        let text_has_backtick = doc.nodes().any(|n| {
            matches!(n, ast::NodeRef::Inline(i)
                if matches!(&i.kind, ast::InlineKind::Text(t) if t.contains('`')))
        });
        assert!(
            !text_has_backtick,
            "a backtick leaked into the reader's prose: {rewritten:?}"
        );
    }

    /// Three roles on one line, asserted as an exact string. The scanner
    /// resumes at the offset [`role_at`] reports, so an offset one byte short
    /// re-reads a backtick it already consumed and one byte long eats the
    /// character after the role — neither of which a "does the AST still
    /// contain `:cite:`?" assertion can see.
    #[test]
    fn test_several_roles_on_one_line_each_consume_exactly_their_own_bytes() {
        assert_eq!(
            &*apply("See :numref:`fig_a`, :cite:`A.1,B.2` and :citet:`C.3`.\n"),
            "See `fig_a`, (A.1, B.2) and C.3.\n"
        );
        // A role as the very last thing on the line, with no byte after it.
        assert_eq!(
            &*apply("As shown in :numref:`fig_z`\n"),
            "As shown in `fig_z`\n"
        );
        // Two roles with nothing between them get a separator, or their
        // backticks merge into a run CommonMark does not read as two spans.
        // `test_two_adjacent_roles_stay_two_code_spans_instead_of_merging_their_backticks`
        // is the one that states *why* through the parser; this pins the bytes.
        assert_eq!(&*apply(":eqref:`e1`:eqref:`e2`\n"), "`e1` `e2`\n");
    }

    /// What [`role_at`] refuses, and why each refusal is there. The argument
    /// restriction is the load-bearing one: without it a colon in ordinary
    /// prose pairs with a backtick further down the line and swallows the
    /// text in between.
    #[test]
    fn test_a_role_needs_a_backticked_argument_with_no_spaces_in_it() {
        for line in [
            // No backtick after the role at all.
            "See :numref: fig_a for details.",
            // A backtick that never closes.
            "See :numref:`fig_a for details.",
            // An empty argument.
            "See :numref:`` for details.",
            // An argument with whitespace: this is prose, and the closing
            // backtick belongs to some other code span.
            "The :cite: role takes `a key` and prints it.",
            // A colon that opens nothing.
            ":not-a-role at the start of a line",
            "a ratio of 3:1 at 10:30",
        ] {
            let src = format!("{line}\n");
            assert!(matches!(apply(&src), Cow::Borrowed(_)), "rewrote {line:?}");
        }
    }

    /// The own-line rule takes the whole line or nothing. A directive with
    /// prose after it is a sentence, and deleting the line would delete the
    /// sentence with it.
    #[test]
    fn test_an_own_line_directive_must_be_the_whole_line() {
        assert_eq!(&*apply("   :label:`sec_x`\n"), "");
        assert_eq!(&*apply(":end_tab:\n"), "");
        for kept in [
            ":label:`sec_x` and then some prose",
            "prose and then :label:`sec_x`",
            ":label:``",
            ":label:sec_x",
            ":end_tab",
            ":end_tab:x",
        ] {
            let src = format!("{kept}\n");
            assert!(matches!(apply(&src), Cow::Borrowed(_)), "dropped {kept:?}");
        }
    }

    /// The fence half at its edges: a block with no body at all, and a block
    /// whose entire body is the directive being removed.
    #[test]
    fn test_an_input_fence_is_retagged_even_when_it_has_nothing_in_it() {
        assert_eq!(&*apply("```{.python .input}\n```\n"), "```python\n```\n");
        assert_eq!(
            &*apply("```{.python .input}\n%%tab all\n```\n"),
            "```python\n```\n"
        );
        // An unclosed `{.python .input}` fence is R10's case, not this one:
        // the document comes back whole.
        assert!(matches!(
            apply("```{.python .input}\n%%tab all\nx = 1\n"),
            Cow::Borrowed(_)
        ));
    }

    /// The fence rewrite replaces the opener line outright, so it may only
    /// touch a fence whose opener it can reproduce. A top-level one keeps
    /// whatever indentation it had; a fence opened behind a list marker is
    /// left alone, because rewriting its line would have to reproduce the
    /// marker and the item's continuation indent as well.
    #[test]
    fn test_the_fence_rewrite_keeps_indentation_and_declines_list_nested_fences() {
        assert_eq!(
            &*apply("   ```{.python .input}\n   %%tab all\n   x = 1\n   ```\n"),
            "   ```python\n   x = 1\n   ```\n",
            "a top-level fence keeps the three spaces it was written with"
        );
        assert!(
            matches!(
                apply("- - ```{.python .input}\n    %%tab all\n    x = 1\n    ```\n"),
                Cow::Borrowed(_)
            ),
            "a list-nested fence is not this pass's to rewrite"
        );
    }

    /// A class list that names `.input` but no language leaves the fence
    /// alone rather than emitting an empty info string, which would tag the
    /// block as a language called nothing.
    #[test]
    fn test_an_input_fence_with_no_language_is_left_as_it_was() {
        for info in ["{.input}", "{. .input}", "{}", "{.input .}"] {
            assert_eq!(input_fence_language(info), None, "for {info:?}");
        }
        assert!(matches!(
            apply("```{.input}\nx = 1\n```\n"),
            Cow::Borrowed(_)
        ));
    }

    /// The code-span guard, isolated. Most prose that mentions a role is
    /// already refused by [`role_at`]'s no-whitespace rule, which makes it
    /// easy to believe this guard is tested when nothing reaches it — the
    /// sentence below is the shape that does: a backticked `:numref:`
    /// immediately followed by a backticked identifier, so the argument
    /// [`role_at`] would find is a legitimate one and the *only* thing
    /// standing between the author's sentence and a rewrite is the backtick
    /// behind the role.
    #[test]
    fn test_a_role_whose_argument_would_parse_is_still_refused_behind_a_backtick() {
        for line in [
            "Sphinx writes `:numref:`sec_intro` for a cross-reference.",
            "`:eqref:`eq_a` is the syntax.",
        ] {
            let src = format!("{line}\n");
            assert!(
                matches!(apply(&src), Cow::Borrowed(_)),
                "rewrote a role the author was quoting: {line:?}"
            );
        }
    }

    #[test]
    fn test_a_role_inside_a_code_span_is_left_for_the_reader_to_see() {
        let src = "The `:label:` role names a target, as does `:numref:`.\n";
        assert!(matches!(apply(src), Cow::Borrowed(_)));
    }

    #[test]
    fn test_a_role_inside_a_fence_is_not_rewritten() {
        let src = "```markdown\n:label:`sec_x`\n\nSee :numref:`fig_y`.\n```\n";
        assert!(matches!(apply(src), Cow::Borrowed(_)));
    }

    /// R10: a document caught between an editor's two writes comes back
    /// exactly as it was, rather than having its tail reclassified.
    #[test]
    fn test_an_unclosed_fence_leaves_the_whole_document_untouched() {
        let src = "```{.python .input}\n%%tab all\nx = 1\n\n:label:`sec_x`\n";
        assert!(matches!(apply(src), Cow::Borrowed(_)));
    }

    #[test]
    fn test_an_ordinary_document_comes_back_borrowed() {
        let src = "# Title\n\nA paragraph with a ratio of 3:1 and a time of 10:30.\n\n```python\nx = {1: 2}\n```\n";
        assert!(matches!(apply(src), Cow::Borrowed(_)));
    }

    #[test]
    fn test_crlf_line_endings_survive_the_rewrite() {
        let src = "See :numref:`fig_x`.\r\n:label:`sec_y`\r\nafter\r\n";
        assert_eq!(&*apply(src), "See `fig_x`.\r\nafter\r\n");
    }

    /// `mermaid.rs:280`'s drift test for this pass: the promise is about the
    /// *document*, so it is checked on the parsed document. A rewrite that
    /// produces plausible text the parser reads differently — a fence whose
    /// info string did not take, a role that ate a bracket — fails here.
    #[test]
    fn test_no_sphinx_role_survives_into_the_parsed_document() {
        let sources = [
            concat!(
                "# T\n",
                ":label:`sec_t`\n",
                "\n",
                "See :numref:`fig_a` and :cite:`A.1,B.2`, per :citet:`C.3`.\n",
                "\n",
                "```{.python .input}\n",
                "%%tab all\n",
                "print(1)\n",
                "```\n",
            ),
            "plain document\n",
        ];
        for src in sources {
            let out = apply(src);
            let doc = Document::parse(&out);
            let dumped = format!("{:?}", doc.blocks());
            for role in [":label:", ":numref:", ":eqref:", ":cite:", ":citet:"] {
                assert!(
                    !dumped.contains(role),
                    "{role} survived into the AST of {src:?}"
                );
            }
            assert!(!dumped.contains("%%tab"), "a tab directive survived");
            assert!(!dumped.contains(".input"), "an input fence tag survived");
        }
    }
}
