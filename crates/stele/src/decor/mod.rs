//! The decoration hook seam (P7 registers a real theme/highlight
//! implementation here). P5 paints with the structural default: identity
//! `highlight`, and a `resolve` table mapping each [`Semantic`] role to a
//! structural [`Style`] (headings on a bold-to-dim ladder by level, dim
//! blockquote gutters, and so on) — no color, since the full theme engine
//! arrives in P7.

use highlight::Highlighted;
use layout::{Run, Semantic, StyleId};

use crate::painter::Style;

/// `--frontmatter` visibility (parsed in `cli.rs`; this module makes the
/// flag do something): strips a leading YAML frontmatter block from the
/// source before parsing, unless the flag says to show it.
pub mod frontmatter;
/// Renders ` ```mermaid ` fences to box-drawing grids as a source-text
/// preprocessor run before `Document::parse`.
pub mod mermaid;
/// P7's implementation: bridges `crates/highlight`'s theme/highlighter
/// onto this trait. Registrable in one line via [`crate::painter::Painter::register_decor`]:
/// `painter.register_decor(Box::new(themed::ThemedDecor::detect(bg_reply)))`.
pub mod themed;

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
        | Semantic::Strikethrough => dim,
        Semantic::Text | Semantic::TaskMarker | Semantic::CodeBlock => Style::default(),
    }
}

/// The heading ladder, themeless half: six distinct attribute sets, loudest
/// (H1) to quietest (H6). This path never carries color, so the ladder is
/// the *only* thing distinguishing levels here — no two rungs may collide,
/// and `dim` is safe to spend on the deepest two because it attenuates the
/// terminal's own foreground rather than an already-quiet ramp color.
///
/// Kept identical to the monochrome branch of `highlight::theme`'s
/// `heading_attrs`; that crate's colored branch deliberately differs (it
/// drops `dim`, since stacking SGR faint on the quietest ramp tier falls
/// under WCAG AA). Both sides pin their ladder with a test, and the two
/// tables are separate for the same reason the rest of this function is: the
/// structural defaults and the themed ones genuinely diverge elsewhere
/// (`CodeInline`, `ListMarker`).
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
    use layout::AlertTone;

    use super::*;

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
                | Semantic::SearchCurrent => {}
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
