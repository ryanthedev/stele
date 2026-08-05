//! The decoration hook seam (P7 registers a real theme/highlight
//! implementation here). P5 paints with the structural default: identity
//! `highlight`, and a `resolve` table mapping each [`Semantic`] role to a
//! structural [`Style`] (headings on a bold-to-dim ladder by level, dim
//! blockquote gutters, and so on) — no color, since the full theme engine
//! arrives in P7.

use std::borrow::Cow;

use highlight::Highlighted;
use layout::{Run, Semantic, StyleId};

use crate::painter::Style;

/// `latex.codecogs.com` image links back into the `$…$` maths they encode.
pub mod codecogs;
/// Dive-into-Deep-Learning's Sphinx roles (`:label:`, `:numref:`, `:cite:`)
/// and its ` ```{.python .input} ` fences.
pub mod d2l;
/// The shared primitive the source-text passes classify lines with: which
/// lines are inside a fenced code block or a Pandoc grid table, and are
/// therefore off limits to a rewrite.
pub mod fences;
/// `--frontmatter` visibility (parsed in `cli.rs`; this module makes the
/// flag do something): strips a leading YAML frontmatter block from the
/// source before parsing, unless the flag says to show it.
pub mod frontmatter;
/// Renders ` ```mermaid ` fences to box-drawing grids as a source-text
/// preprocessor run before `Document::parse`.
pub mod mermaid;
/// Quarto/Pandoc `:::` callout divs into GFM alerts, plus `{#id}` heading
/// attributes, `[text]{.class}` spans and `{{< shortcode >}}` lines.
pub mod quarto;
/// P7's implementation: bridges `crates/highlight`'s theme/highlighter
/// onto this trait. Registrable in one line via [`crate::painter::Painter::register_decor`]:
/// `painter.register_decor(Box::new(themed::ThemedDecor::detect(bg_reply)))`.
pub mod themed;

/// The characters that mean something to CommonMark by *how many of them
/// there are in a row* — the ones a source rewrite can break by butting two
/// runs together.
const RUN_DELIMITERS: [char; 2] = ['`', '$'];

/// `replacement`, with a space added on whichever side would otherwise run
/// one of its edge characters into an identical neighbour.
///
/// CommonMark matches a delimiter run against a later run **of the same
/// length**, so `` `a` `` immediately followed by `` `b` `` is not two code
/// spans: the closing backtick of the first and the opening backtick of the
/// second merge into a single two-backtick run, the outer pair closes around
/// the lot, and the reader gets one code span with literal backticks inside
/// it. `$…$` maths breaks the same way — `$a$$b$` is one formula reading
/// `a$$b`.
///
/// Every pass here that emits a *delimited* construct has to ask this before
/// committing to one, and the reason is worth stating plainly: the author did
/// not write those delimiters, we did. Two constructs the author wrote next
/// to each other are their business; two we manufactured and jammed together
/// is a document we corrupted. This phase's standing rule is that a missed
/// rewrite is recoverable and a corrupted document is not, and emitting
/// markup the parser reads as neither is worse than either.
///
/// A space is the separator rather than declining the rewrite, because both
/// alternatives are worse in a way the reader can see: declining leaves raw
/// `:eqref:` or a bare CodeCogs URL in the prose, and dropping the delimiters
/// leaves one of a matched pair styled and the other not. Two constructs
/// written flush against each other were never going to read as one word
/// anyway. **The neighbour may also be the author's own text** — a rewrite
/// landing directly in front of their code span merges with it just as
/// readily as with another rewrite, which is why `after` is consulted and not
/// only what we have already emitted.
pub(crate) fn spaced_to_not_merge<'a>(
    before: Option<char>,
    replacement: &'a str,
    after: Option<char>,
) -> Cow<'a, str> {
    let merges = |left: Option<char>, right: Option<char>| match (left, right) {
        (Some(left), Some(right)) => left == right && RUN_DELIMITERS.contains(&left),
        _ => false,
    };
    let pad_left = merges(before, replacement.chars().next());
    let pad_right = merges(replacement.chars().next_back(), after);
    match (pad_left, pad_right) {
        (false, false) => Cow::Borrowed(replacement),
        (true, false) => Cow::Owned(format!(" {replacement}")),
        (false, true) => Cow::Owned(format!("{replacement} ")),
        (true, true) => Cow::Owned(format!(" {replacement} ")),
    }
}

/// The character immediately in front of an insertion point at `cursor`,
/// whichever side of the seam it came from.
///
/// A rewriting pass keeps two positions: `copied`, how much of the source
/// line it has committed, and `cursor`, where it is now. When the two differ
/// there is untouched source between them and the neighbouring character is
/// the last byte of that gap; when they are equal the insertion point sits
/// flush against whatever the pass last emitted, which is where a collision
/// between two replacements happens.
pub(crate) fn char_before(out: &str, line: &str, copied: usize, cursor: usize) -> Option<char> {
    if cursor > copied {
        line[..cursor].chars().next_back()
    } else {
        out.chars().next_back()
    }
}

/// P7's hook: turns a code-block line into syntax-highlighted runs, and
/// resolves any [`StyleId`] to concrete SGR attributes.
pub trait Decor {
    /// Highlights one code-block line. `line_text` is the raw line content,
    /// `lang` the fence's language tag if known. Returned runs leave
    /// `width` unset (`0`) — the painter recomputes it via the width
    /// engine after this call.
    ///
    /// The [`Highlighted::cacheable`] flag is how an implementation tells
    /// the painter whether it may memoize this answer across frames
    /// (Phase 4's highlight cache). Say `false` for anything produced by a
    /// timeout or an error fallback — that is a statement about *this
    /// attempt*, not about the line, and caching it would make a transient
    /// stall permanent. An implementation with no such failure mode should
    /// say `true` unconditionally.
    fn highlight(&self, line_text: &str, lang: Option<&str>) -> Highlighted;

    /// Resolves a style identity to concrete attributes.
    fn resolve(&self, style_id: StyleId) -> Style;
}

/// The structural default: no syntax highlighting (identity), structural
/// (non-color) attributes per semantic role. This is what P5 actually
/// paints with.
#[derive(Debug, Default)]
pub struct StructuralDecor;

impl Decor for StructuralDecor {
    fn highlight(&self, line_text: &str, _lang: Option<&str>) -> Highlighted {
        // Identity: no highlighting engine is wired yet (P7), so the whole
        // line comes back unchanged as a single run, still tagged as code.
        // Always cacheable — this path has no timeout and no failure mode,
        // so its answer is a pure function of `line_text`.
        Highlighted {
            runs: vec![Run {
                text: line_text.to_string(),
                style_id: StyleId::Semantic(Semantic::CodeBlock),
                width: 0,
                aux: None,
            }],
            cacheable: true,
        }
    }

    fn resolve(&self, style_id: StyleId) -> Style {
        match style_id {
            StyleId::Semantic(semantic) => structural_style(semantic),
            // A capture-range id with no highlight engine registered:
            // no structural precedent exists for it, so it paints plain.
            StyleId::Capture(_) => Style::default(),
        }
    }
}

/// Structural (non-color) attributes per semantic role. Exhaustive over
/// [`Semantic`] so a new variant fails to compile here rather than silently
/// painting plain.
fn structural_style(semantic: Semantic) -> Style {
    let bold = Style {
        bold: true,
        ..Style::default()
    };
    let dim = Style {
        dim: true,
        ..Style::default()
    };
    let italic = Style {
        italic: true,
        ..Style::default()
    };
    let underline = Style {
        underline: true,
        ..Style::default()
    };
    match semantic {
        Semantic::Heading(level) => heading_style(level),
        // Plain, and that is the whole point of the themeless path: the depth
        // markers and the rule are glyphs, and the *count* of them is what
        // carries heading level once color is gone. Giving them an attribute
        // here would decorate the signal without adding to it.
        Semantic::HeadingRung(_) => Style::default(),
        Semantic::Strong
        | Semantic::TableHeader
        | Semantic::FootnoteLabel
        | Semantic::AlertTitle(_) => bold,
        Semantic::Emph | Semantic::ImageAlt | Semantic::MathTex => italic,
        Semantic::Link | Semantic::FootnoteRef => underline,
        // This path carries no color at all, so the attribute set is the
        // *only* thing that separates any role from any other — which makes
        // a shared attribute set a genuine collision here, not the harmless
        // duplicate it is on the themed path where a ramp color still
        // distinguishes them.
        //
        // Both search roles therefore need combinations nothing else uses.
        // Bare `underline` is taken by `Link`/`FootnoteRef` and
        // `bold + underline` by `Heading(1)`, so the search pair adds
        // `italic` to clear both: a match reads as underlined-and-slanted,
        // and the current one adds weight on top (DW-4.4/DW-4.8).
        Semantic::SearchMatch => Style {
            italic: true,
            ..underline
        },
        Semantic::SearchCurrent => Style {
            italic: true,
            underline: true,
            ..bold
        },
        Semantic::BlockquoteMarker
        | Semantic::Rule
        | Semantic::TableBorder
        | Semantic::CodeInline
        | Semantic::Html
        | Semantic::FrontMatter
        | Semantic::OverflowIndicator
        | Semantic::ListMarker
        | Semantic::Strikethrough
        | Semantic::LineNumber
        | Semantic::GutterBorder => dim,
        // The reading line on the themeless path is the *absence* of dim in a
        // column that is otherwise entirely dim. That is a real difference and
        // the only one available: `reverse` is spent on link selection, and
        // bold would move the digits (see `highlight::theme`'s arm).
        //
        // What this path cannot do is paint the band — `CurrentLine` is a
        // colour and there are none here. So with no theme, the reading line
        // is marked by its number and its separator brightening, and by
        // nothing else. Stated rather than worked around: a `reverse` band
        // across the page would be the one loud thing on a monochrome screen.
        Semantic::LineNumberCurrent | Semantic::CurrentLine => Style::default(),
        Semantic::Text | Semantic::TaskMarker | Semantic::CodeBlock => Style::default(),
    }
}

/// The heading ladder, themeless half: six distinct attribute sets, loudest
/// (H1) to quietest (H6). This path never carries color, so the ladder is
/// the *only* thing distinguishing levels here — no two rungs may collide,
/// and `dim` is safe to spend on the deepest two because it attenuates the
/// terminal's own foreground rather than an already-quiet ramp color.
///
/// Deliberately *not* `highlight::theme`'s `heading_attrs`, which stopped
/// keeping six distinct sets once it could lean on the ramp, the band and the
/// case transform — none of which exist here. This path has no colour to
/// pair with, and no wash: the levels this file cannot distinguish, nothing
/// else on the screen will. It also keeps `dim`, which the themed ladder
/// dropped because stacking SGR faint on the quietest ramp tier falls under
/// WCAG AA; against the terminal's own foreground it does not. Both sides pin
/// their ladder with a test, and the two tables are separate for the same
/// reason the rest of this function is: the structural defaults and the
/// themed ones genuinely diverge elsewhere (`CodeInline`, `ListMarker`).
fn heading_style(level: u8) -> Style {
    let bold = Style {
        bold: true,
        ..Style::default()
    };
    match level.clamp(1, 6) {
        1 => Style {
            underline: true,
            ..bold
        },
        2 => bold,
        3 => Style {
            italic: true,
            ..bold
        },
        4 => Style {
            italic: true,
            ..Style::default()
        },
        5 => Style {
            dim: true,
            ..Style::default()
        },
        _ => Style {
            dim: true,
            italic: true,
            ..Style::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::path::PathBuf;

    use layout::AlertTone;

    use super::*;

    /// Every `fixtures/*.md`, which is the corpus the parser and the layout
    /// engine are already pinned against.
    fn fixture_paths() -> Vec<PathBuf> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
            .expect("fixtures/ exists")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
            .collect();
        paths.sort();
        assert!(paths.len() >= 15, "fixture corpus unexpectedly small");
        paths
    }

    /// The source-text passes must be invisible on documents that hold none
    /// of the syntax they exist for — and "invisible" means the same
    /// allocation comes back, not merely an equal string. A pass that
    /// rebuilt every document would pass an equality check and quietly cost
    /// a copy of the file on every `--watch` reload.
    ///
    /// `fixtures/preprocessors.md` is the one document here that *is* meant
    /// to be rewritten, so it is excluded by name; every other fixture,
    /// including `preprocessor-lookalikes.md` — which is nothing but
    /// constructs that resemble the ones these passes take — must come back
    /// borrowed. Without that second file this assertion would be vacuous:
    /// before it was added, no fixture in the tree contained `:::`,
    /// `:label:`, `{{<` or `]{.` at all.
    #[test]
    fn test_no_fixture_but_the_preprocessor_one_is_rewritten_by_any_pass() {
        let mut checked = 0usize;
        let mut saw_lookalikes = false;
        for path in fixture_paths() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name == "preprocessors.md" {
                continue;
            }
            saw_lookalikes |= name == "preprocessor-lookalikes.md";
            let source = std::fs::read_to_string(&path).unwrap();
            for (pass_name, output) in [
                ("codecogs", codecogs::apply(&source)),
                ("d2l", d2l::apply(&source)),
                ("quarto", quarto::apply(&source)),
            ] {
                assert!(
                    matches!(output, Cow::Borrowed(_)),
                    "{name} was rewritten by {pass_name}"
                );
            }
            checked += 1;
        }
        assert!(
            saw_lookalikes,
            "the look-alike fixture must be in the sweep"
        );
        assert!(checked >= 15);
    }

    /// Every delimiter character that survives into a `Text` node of
    /// `source`, once parsed.
    ///
    /// This is the oracle for the whole collision class, and it is the right
    /// shape for it: a merged run does not announce itself in the text, it
    /// announces itself by the parser reading a delimiter as prose. If a
    /// backtick or a dollar we emitted turns up as literal text the reader
    /// will see, something merged.
    fn delimiters_left_in_prose(source: &str) -> Vec<String> {
        let doc = ast::Document::parse(source);
        doc.nodes()
            .filter_map(|node| match node {
                ast::NodeRef::Inline(inline) => match &inline.kind {
                    ast::InlineKind::Text(text) if text.contains('`') || text.contains('$') => {
                        Some(text.clone())
                    }
                    _ => None,
                },
                ast::NodeRef::Block(_) => None,
            })
            .collect()
    }

    /// Two replacements emitted flush against each other merge their
    /// delimiter runs, and the reader gets literal backticks or dollars in
    /// their prose instead of the construct we meant. Every pass that emits a
    /// delimited construct is checked here, in one place, because the bug is
    /// a property of splicing rather than of any one pass — `d2l` had it
    /// first and `codecogs` had exactly the same shape waiting.
    ///
    /// All inputs are synthetic: nothing in the corpus writes two of these
    /// flush together. They are pinned anyway, because the failure mode is
    /// document corruption by a rewriting pass, and this phase's rule is that
    /// a missed rewrite is recoverable while a corrupted document is not.
    #[test]
    fn test_no_pass_lets_two_adjacent_replacements_merge_their_delimiter_runs() {
        // (pass, source, what the reader must end up with)
        let cases: [(&str, String, &str); 7] = [
            (
                "d2l role beside role",
                ":eqref:`eq_a`:eqref:`eq_b`\n".to_string(),
                "`eq_a` `eq_b`\n",
            ),
            (
                "d2l role beside the author's own code span",
                ":eqref:`eq_a``literal`\n".to_string(),
                "`eq_a` `literal`\n",
            ),
            (
                "codecogs image beside image",
                "![a](http://latex.codecogs.com/svg.latex?x)\
                 ![b](http://latex.codecogs.com/svg.latex?y)\n"
                    .to_string(),
                "$x$ $y$\n",
            ),
            (
                "codecogs image beside the author's own maths",
                "![a](http://latex.codecogs.com/svg.latex?x)$y$\n".to_string(),
                "$x$ $y$\n",
            ),
            (
                "quarto span beside span",
                "[`a`]{.mark}[`b`]{.mark}\n".to_string(),
                "`a` `b`\n",
            ),
            (
                "quarto span beside the author's own code span",
                "[`a`]{.mark}`b`\n".to_string(),
                "`a` `b`\n",
            ),
            // The middle role is boxed in: a replacement on its left and the
            // author's own code span on its right, so it needs a separator on
            // both sides at once.
            (
                "d2l role hemmed in on both sides",
                ":eqref:`eq_a`:eqref:`eq_b``literal`\n".to_string(),
                "`eq_a` `eq_b` `literal`\n",
            ),
        ];

        for (what, source, expected) in cases {
            let rewritten = match what.split_whitespace().next() {
                Some("d2l") => d2l::apply(&source).into_owned(),
                Some("codecogs") => codecogs::apply(&source).into_owned(),
                _ => quarto::apply(&source).into_owned(),
            };
            assert_eq!(rewritten, expected, "{what}");
            assert_eq!(
                delimiters_left_in_prose(&rewritten),
                Vec::<String>::new(),
                "{what}: a delimiter leaked into the reader's prose ({rewritten:?})"
            );
        }
    }

    /// The separator is only inserted where a run would actually merge. A
    /// rewrite that padded every replacement would be adding whitespace to
    /// the author's sentences for no reason, and the corpus is full of these.
    #[test]
    fn test_a_replacement_with_room_beside_it_is_not_padded() {
        assert_eq!(
            &*d2l::apply("See :numref:`fig_a` and :eqref:`eq_b`.\n"),
            "See `fig_a` and `eq_b`.\n"
        );
        assert_eq!(
            &*codecogs::apply("where ![a](http://latex.codecogs.com/svg.latex?x) is real\n"),
            "where $x$ is real\n"
        );
        assert_eq!(&*quarto::apply("[a]{.mark} and [b]{.mark}\n"), "a and b\n");
        // Adjacent, but with nothing that can merge: no padding either.
        assert_eq!(&*quarto::apply("[a]{.mark}[b]{.mark}\n"), "ab\n");
        assert_eq!(
            &*d2l::apply(":citet:`A.1`:citet:`B.2`\n"),
            "A.1B.2\n",
            "plain replacements carry no delimiter to merge"
        );
    }

    /// The other direction, on the fixture that *is* full of the syntax.
    /// Asserted on the parsed document rather than on the text, so a rewrite
    /// that produces plausible-looking markdown the parser reads some other
    /// way still fails.
    #[test]
    fn test_the_preprocessor_fixture_loses_its_machinery_and_gains_its_alerts() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/preprocessors.md");
        let source = std::fs::read_to_string(&path).unwrap();

        // The load path's order, spelled out rather than reached through
        // `loader::preprocess`, so this test fails if the passes stop
        // composing even while the loader still calls them.
        let after_codecogs = codecogs::apply(&source).into_owned();
        let after_d2l = d2l::apply(&after_codecogs).into_owned();
        let rewritten = quarto::apply(&after_d2l).into_owned();
        let doc = ast::Document::parse(&rewritten);
        let dumped = format!("{:?}", doc.blocks());

        for gone in [
            ":label:",
            ":eqlabel:",
            ":width:",
            ":numref:",
            ":eqref:",
            ":cite:",
            ":citet:",
            ":begin_tab:",
            ":end_tab:",
            "%%tab",
            ".input",
            "latex.codecogs.com",
            ":::",
            "{{<",
            "{#spans-and-shortcodes}",
            "]{.",
        ] {
            assert!(!dumped.contains(gone), "{gone} survived into the AST");
        }

        // Six callouts in the fixture, in the order it declares them.
        let alerts: Vec<ast::AlertKind> = doc
            .blocks()
            .iter()
            .filter_map(|block| match &block.kind {
                ast::BlockKind::Alert { kind, .. } => Some(*kind),
                _ => None,
            })
            .collect();
        assert_eq!(
            alerts,
            vec![
                ast::AlertKind::Note,
                ast::AlertKind::Tip,
                ast::AlertKind::Important,
                ast::AlertKind::Warning,
                ast::AlertKind::Caution,
                ast::AlertKind::Note,
            ],
            "callouts read out of the fixture in declaration order"
        );
    }

    #[test]
    fn test_identity_highlight_returns_unchanged_single_run() {
        let decor = StructuralDecor;
        let highlighted = decor.highlight("let x = 1;", Some("rust"));
        let runs = &highlighted.runs;
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "let x = 1;");
        assert_eq!(runs[0].width, 0);
        assert_eq!(runs[0].style_id, StyleId::Semantic(Semantic::CodeBlock));
        assert!(
            highlighted.cacheable,
            "the identity path has no failure mode, so its answer memoizes"
        );
    }

    #[test]
    fn test_resolve_is_exhaustive_and_structural_only() {
        let decor = StructuralDecor;
        let heading = decor.resolve(StyleId::Semantic(Semantic::Heading(1)));
        assert!(heading.bold);
        assert!(heading.fg.is_none() && heading.bg.is_none());

        let quote = decor.resolve(StyleId::Semantic(Semantic::BlockquoteMarker));
        assert!(quote.dim);

        let link = decor.resolve(StyleId::Semantic(Semantic::Link));
        assert!(link.underline);

        let alert = decor.resolve(StyleId::Semantic(Semantic::AlertTitle(AlertTone::Warning)));
        assert!(alert.bold);

        let plain = decor.resolve(StyleId::Semantic(Semantic::Text));
        assert_eq!(plain, Style::default());

        let capture = decor.resolve(StyleId::Capture(0));
        assert_eq!(capture, Style::default());
    }

    /// The themeless path has no color at all, so if two heading levels share
    /// an attribute set they are literally indistinguishable on screen. This
    /// is the test that would have caught the original collapse: every level
    /// resolved to plain `bold`, which passes any "is H1 bold?" assertion.
    #[test]
    fn test_heading_levels_resolve_to_six_distinct_structural_styles() {
        let decor = StructuralDecor;
        let mut seen = Vec::new();
        for level in 1..=6u8 {
            let style = decor.resolve(StyleId::Semantic(Semantic::Heading(level)));
            assert!(
                style.fg.is_none() && style.bg.is_none(),
                "H{level} carried color on the structural path"
            );
            assert_ne!(
                style,
                Style::default(),
                "H{level} is indistinguishable from body text"
            );
            assert!(
                !seen.contains(&style),
                "H{level} shares an attribute set with a shallower level: {style:?}"
            );
            seen.push(style);
        }
    }

    /// Distinctness alone permits any permutation of the six styles, so pin
    /// the ladder the module doc actually promises: loud (bold) to quiet (dim).
    #[test]
    fn test_structural_heading_ladder_maps_each_level_to_its_specified_style() {
        let decor = StructuralDecor;
        let bold = Style {
            bold: true,
            ..Style::default()
        };
        let expected = [
            Style {
                underline: true,
                ..bold
            },
            bold,
            Style {
                italic: true,
                ..bold
            },
            Style {
                italic: true,
                ..Style::default()
            },
            Style {
                dim: true,
                ..Style::default()
            },
            Style {
                dim: true,
                italic: true,
                ..Style::default()
            },
        ];
        for level in 1..=6u8 {
            assert_eq!(
                decor.resolve(StyleId::Semantic(Semantic::Heading(level))),
                expected[usize::from(level - 1)],
                "H{level}"
            );
        }
    }

    /// Every `Semantic` variant, so a test can check a property across all
    /// of them instead of a hand-picked sample.
    ///
    /// The `match` inside has no wildcard arm, so adding a `Semantic`
    /// variant is a **compile error here** until the list below names it —
    /// the same discipline the style tables use, applied to the list that
    /// checks them. That guard is not decoration: the first version of the
    /// DW-4.8 test below checked five roles and happened to omit `Link`,
    /// which is precisely the role `SearchMatch` was colliding with, so the
    /// test passed over a real collision.
    fn every_semantic() -> Vec<Semantic> {
        fn _exhaustiveness_guard(semantic: Semantic) {
            match semantic {
                Semantic::Text
                | Semantic::Heading(_)
                | Semantic::Emph
                | Semantic::Strong
                | Semantic::Strikethrough
                | Semantic::CodeInline
                | Semantic::CodeBlock
                | Semantic::Link
                | Semantic::ImageAlt
                | Semantic::MathTex
                | Semantic::ListMarker
                | Semantic::TaskMarker
                | Semantic::BlockquoteMarker
                | Semantic::AlertTitle(_)
                | Semantic::Rule
                | Semantic::TableBorder
                | Semantic::TableHeader
                | Semantic::FootnoteRef
                | Semantic::FootnoteLabel
                | Semantic::Html
                | Semantic::FrontMatter
                | Semantic::OverflowIndicator
                | Semantic::SearchMatch
                | Semantic::SearchCurrent
                | Semantic::LineNumber
                | Semantic::LineNumberCurrent
                | Semantic::GutterBorder
                | Semantic::CurrentLine => {}
                // Deliberately absent from the list below, for the reason
                // given in `highlight::theme`'s matching guard: it is defined
                // to resolve like `Heading(n)` minus the wash, so a
                // distinctness sweep would assert the opposite.
                Semantic::HeadingRung(_) => {}
            }
        }

        let mut all = vec![
            Semantic::Text,
            Semantic::Emph,
            Semantic::Strong,
            Semantic::Strikethrough,
            Semantic::CodeInline,
            Semantic::CodeBlock,
            Semantic::Link,
            Semantic::ImageAlt,
            Semantic::MathTex,
            Semantic::ListMarker,
            Semantic::TaskMarker,
            Semantic::BlockquoteMarker,
            Semantic::Rule,
            Semantic::TableBorder,
            Semantic::TableHeader,
            Semantic::FootnoteRef,
            Semantic::FootnoteLabel,
            Semantic::Html,
            Semantic::FrontMatter,
            Semantic::OverflowIndicator,
            Semantic::SearchMatch,
            Semantic::SearchCurrent,
            Semantic::LineNumber,
            Semantic::LineNumberCurrent,
            Semantic::GutterBorder,
            Semantic::CurrentLine,
        ];
        all.extend((1..=6u8).map(Semantic::Heading));
        all.extend(
            [
                AlertTone::Note,
                AlertTone::Tip,
                AlertTone::Important,
                AlertTone::Warning,
                AlertTone::Caution,
            ]
            .map(Semantic::AlertTitle),
        );
        all
    }

    /// DW-4.8's themeless half. This path has no palette at all, so the
    /// 256-color argument does not apply to it — what has to hold instead is
    /// the property that argument exists to protect: a search match must be
    /// tellable from everything it can sit on or next to.
    ///
    /// Checked against **every** role rather than a sample, because a sample
    /// is what let the original collision through: `SearchMatch` resolved to
    /// bare `underline`, identical to `Link` and `FootnoteRef`, and
    /// `SearchCurrent` to `bold + underline`, identical to `Heading(1)` —
    /// none of which the five-role list happened to include.
    #[test]
    fn test_dw_4_8_search_roles_are_distinguishable_on_the_themeless_path_too() {
        let decor = StructuralDecor;
        let style = |semantic| decor.resolve(StyleId::Semantic(semantic));
        let matched = style(Semantic::SearchMatch);
        let current = style(Semantic::SearchCurrent);

        assert_ne!(
            matched, current,
            "the current match must not resolve identically to the others"
        );
        for semantic in every_semantic() {
            if matches!(semantic, Semantic::SearchMatch | Semantic::SearchCurrent) {
                continue;
            }
            assert_ne!(
                style(semantic),
                matched,
                "{semantic:?} is indistinguishable from a search match on the \
                 themeless path, where attributes are the only channel there is"
            );
            assert_ne!(
                style(semantic),
                current,
                "{semantic:?} is indistinguishable from the current match on the \
                 themeless path"
            );
        }
        // Nothing here may smuggle in color — this path has none to give.
        assert!(matched.fg.is_none() && matched.bg.is_none());
        assert!(current.fg.is_none() && current.bg.is_none());
    }

    /// Levels outside CommonMark's 1–6 cannot reach here through `crates/layout`,
    /// but `Semantic::Heading` is constructible with any `u8` — clamping, not
    /// panicking, is the contract.
    #[test]
    fn test_out_of_range_heading_level_clamps_to_the_nearest_rung() {
        let decor = StructuralDecor;
        let h1 = decor.resolve(StyleId::Semantic(Semantic::Heading(1)));
        let h6 = decor.resolve(StyleId::Semantic(Semantic::Heading(6)));
        assert_eq!(decor.resolve(StyleId::Semantic(Semantic::Heading(0))), h1);
        assert_eq!(decor.resolve(StyleId::Semantic(Semantic::Heading(9))), h6);
        assert_eq!(decor.resolve(StyleId::Semantic(Semantic::Heading(255))), h6);
    }
}
