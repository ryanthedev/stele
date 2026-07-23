//! The decoration hook seam (P7 registers a real theme/highlight
//! implementation here). P5 paints with the structural default: identity
//! `highlight`, and a `resolve` table mapping each [`Semantic`] role to a
//! structural [`Style`] (bold headings, dim blockquote gutters, and so on)
//! — no color, since the full theme engine arrives in P7.

use layout::{Run, Semantic, StyleId};

use crate::painter::Style;

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
        Semantic::Heading(_)
        | Semantic::Strong
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
}
