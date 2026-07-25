//! The decoration hook seam (P7 registers a real theme/highlight
//! implementation here). P5 paints with the structural default: identity
//! `highlight`, and a `resolve` table mapping each [`Semantic`] role to a
//! structural [`Style`] (headings on a bold-to-dim ladder by level, dim
//! blockquote gutters, and so on) — no color, since the full theme engine
//! arrives in P7.

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
    fn highlight(&self, line_text: &str, lang: Option<&str>) -> Vec<Run>;

    /// Resolves a style identity to concrete attributes.
    fn resolve(&self, style_id: StyleId) -> Style;
}

/// The structural default: no syntax highlighting (identity), structural
/// (non-color) attributes per semantic role. This is what P5 actually
/// paints with.
#[derive(Debug, Default)]
pub struct StructuralDecor;

impl Decor for StructuralDecor {
    fn highlight(&self, line_text: &str, _lang: Option<&str>) -> Vec<Run> {
        // Identity: no highlighting engine is wired yet (P7), so the whole
        // line comes back unchanged as a single run, still tagged as code.
        vec![Run {
            text: line_text.to_string(),
            style_id: StyleId::Semantic(Semantic::CodeBlock),
            width: 0,
            aux: None,
        }]
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
        let runs = decor.highlight("let x = 1;", Some("rust"));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "let x = 1;");
        assert_eq!(runs[0].width, 0);
        assert_eq!(runs[0].style_id, StyleId::Semantic(Semantic::CodeBlock));
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
