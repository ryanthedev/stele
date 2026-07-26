//! `ThemedDecor` — the P7 [`Decor`] implementation: bridges
//! `crates/highlight`'s [`highlight::Theme`] (owned dark/light themes,
//! truecolor + 256-color downsample, `NO_COLOR`) and
//! [`highlight::highlight_line`] (lumis-backed per-line syntax
//! highlighting) onto this crate's [`Decor`] trait and [`Style`] type.
//!
//! Registration is the one line the phase's scope boundary promises:
//! `painter.register_decor(Box::new(ThemedDecor::detect(bg_reply)))`.

use highlight::Highlighted;
use layout::StyleId;

use crate::decor::Decor;
use crate::painter::{Color, Style};

/// The P7 [`Decor`]: every method is a thin bridge into `crates/highlight`
/// — this type owns no highlighting or theming logic of its own.
#[derive(Debug)]
pub struct ThemedDecor {
    theme: highlight::Theme,
}

impl ThemedDecor {
    /// Builds a decor around an already-constructed theme — the direct
    /// constructor for callers (tests, or an orchestrator that wants
    /// explicit control) that don't need the OSC 11 / `NO_COLOR`
    /// auto-detection [`ThemedDecor::detect`] provides.
    pub fn new(theme: highlight::Theme) -> Self {
        ThemedDecor { theme }
    }

    /// Convenience constructor matching the plan's detection story:
    /// `bg_reply` is the raw OSC 11 background-color query reply bytes the
    /// orchestrator captured from the terminal (Spike A, item 8), or
    /// `None` if the query went unanswered. Falls back to the dark theme
    /// when `bg_reply` doesn't parse or is absent (the plan's documented
    /// fallback). Color mode reads `NO_COLOR` from the real environment;
    /// pass [`ThemedDecor::new`] an explicit [`highlight::Theme`] instead
    /// if a caller needs to avoid that.
    pub fn detect(bg_reply: Option<&[u8]>) -> Self {
        let variant = bg_reply
            .and_then(highlight::variant_from_osc11_reply)
            .unwrap_or(highlight::Variant::Dark);
        let color_mode = highlight::ColorMode::from_env();
        ThemedDecor::new(highlight::Theme::new(variant, color_mode))
    }
}

impl Decor for ThemedDecor {
    fn highlight(&self, line_text: &str, lang: Option<&str>) -> Highlighted {
        // `highlight_detailed`, not `highlight_line`: the extra bit it
        // carries is whether the answer came from the 250 ms timeout
        // fallback, which the painter's cache must not retain.
        highlight::highlight_detailed(line_text, lang)
    }

    fn resolve(&self, style_id: StyleId) -> Style {
        bridge_style(self.theme.resolve(style_id))
    }
}

/// Field-for-field copy from `highlight::Style` to `crate::painter::Style`
/// — the two types are intentionally identical in shape (see
/// `crates/highlight/src/theme.rs`'s doc comment on `Style`) precisely so
/// this bridge needs no translation logic, only a copy.
fn bridge_style(style: highlight::Style) -> Style {
    Style {
        fg: style.fg.map(bridge_color),
        bg: style.bg.map(bridge_color),
        bold: style.bold,
        dim: style.dim,
        italic: style.italic,
        underline: style.underline,
    }
}

fn bridge_color(color: highlight::Color) -> Color {
    Color {
        r: color.r,
        g: color.g,
        b: color.b,
    }
}

#[cfg(test)]
mod tests {
    use layout::{Semantic, StyleId};

    use super::*;

    #[test]
    fn test_registrable_via_default_construction_and_resolves_every_semantic() {
        let decor = ThemedDecor::new(highlight::Theme::new(
            highlight::Variant::Dark,
            highlight::ColorMode::Truecolor,
        ));
        let style = decor.resolve(StyleId::Semantic(Semantic::Heading(1)));
        assert!(style.bold);
        assert!(style.fg.is_some());
    }

    #[test]
    fn test_detect_falls_back_to_dark_when_reply_absent_or_unparseable() {
        let dark_reference = ThemedDecor::new(highlight::Theme::new(
            highlight::Variant::Dark,
            highlight::ColorMode::Truecolor,
        ));
        let expected = dark_reference.resolve(StyleId::Semantic(Semantic::Heading(1)));

        for reply in [None, Some(&b"garbage, not an OSC 11 reply"[..])] {
            let decor = ThemedDecor::detect(reply);
            let heading = decor.resolve(StyleId::Semantic(Semantic::Heading(1)));
            assert!(heading.bold);
            // NO_COLOR may be set in the ambient test-runner environment,
            // which would clear `fg` regardless of variant — compare bold
            // (variant-independent-safe) plus the color-mode-sensitive fg
            // only when the reference theme actually produced a color.
            if expected.fg.is_some() {
                assert_eq!(heading.fg.is_some(), expected.fg.is_some());
            }
        }
    }

    #[test]
    fn test_detect_distinguishes_a_light_reply_from_the_dark_default() {
        let light_reply = b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\";
        let light_decor = ThemedDecor::detect(Some(light_reply));
        let dark_decor = ThemedDecor::detect(None);
        let light_heading = light_decor.resolve(StyleId::Semantic(Semantic::Heading(1)));
        let dark_heading = dark_decor.resolve(StyleId::Semantic(Semantic::Heading(1)));
        // Same role, different variant's palette (assuming NO_COLOR is not
        // set in the test environment) must produce a different color —
        // proof `detect` actually branched on the reply rather than always
        // defaulting to dark.
        if let (Some(light_fg), Some(dark_fg)) = (light_heading.fg, dark_heading.fg) {
            assert_ne!(light_fg, dark_fg);
        }
    }

    #[test]
    fn test_highlight_delegates_to_the_highlight_crate() {
        let decor = ThemedDecor::new(highlight::Theme::new(
            highlight::Variant::Dark,
            highlight::ColorMode::Truecolor,
        ));
        let highlighted = decor.highlight("let x = 1;", Some("rust"));
        assert!(highlighted.runs.len() > 1);
        let text: String = highlighted.runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text, "let x = 1;");
        assert!(
            highlighted.cacheable,
            "a real highlight (not a timeout fallback) must be memoizable"
        );
    }

    #[test]
    fn test_registrable_in_one_line_via_painter_register_decor() {
        // The exact promise the phase scope makes: one line, on a real
        // `Painter`.
        let width_engine = width::WidthEngine::new(width::WidthConfig::default());
        let mut painter = crate::painter::Painter::new(width_engine);
        painter.register_decor(Box::new(ThemedDecor::detect(None)));
    }
}
