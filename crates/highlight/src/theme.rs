//! The owned theme layer: maps every [`StyleId`] (both `Semantic`, from
//! `crates/layout`, and `Capture`, allocated by [`crate::role`]) to concrete
//! attributes, in built-in dark and light variants selected by the
//! terminal's own reported background (OSC 11, Spike A-verified) — never
//! from a user theme file (explicitly OUT of scope).

use layout::{AlertTone, Semantic, StyleId};

use crate::color::{self, Color, ColorMode};
use crate::role::{self, Capture};

/// SGR-facing attributes resolved for one [`StyleId`]. Field-for-field
/// identical to `stele::painter::Style` by construction (this crate cannot
/// depend on `stele` — the dependency runs the other way) so
/// `crates/stele/src/decor`'s bridge is a straight copy, not a translation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
}

/// Which built-in theme is active. Selected once, at construction, from the
/// terminal's OSC 11 background reply — never re-derived per call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Variant {
    Dark,
    Light,
}

/// Number of distinct-colored `Semantic` roles (see [`semantic_role_index`]).
/// Capture roles (excluding the uncolored `Plain` catch-all) are appended
/// after these, so the full palette has `SEMANTIC_ROLES + (role::ALL.len() - 1)`
/// entries — kept in one constant so the palette size and the role-index
/// functions can never drift apart.
const SEMANTIC_ROLES: usize = 20;

/// The owned theme: a built-in [`Variant`] plus a [`ColorMode`]. Construct
/// with [`Theme::new`]; [`Theme::resolve`] is the total function P7 commits
/// to (`crates/highlight`'s half of the P5 `Decor` contract).
#[derive(Debug, Clone)]
pub struct Theme {
    variant: Variant,
    color_mode: ColorMode,
    /// Built once at construction (see [`build_palette`]) rather than
    /// recomputed per [`Theme::resolve`] call — `resolve` runs on every
    /// painted run, every frame.
    palette: Vec<Color>,
}

impl Theme {
    pub fn new(variant: Variant, color_mode: ColorMode) -> Theme {
        Theme {
            variant,
            color_mode,
            palette: build_palette(variant),
        }
    }

    /// Which built-in variant this theme resolves against.
    pub fn variant(&self) -> Variant {
        self.variant
    }

    /// Resolves any `StyleId` to concrete attributes. Total over both
    /// `Semantic` (exhaustive match, so a new variant fails to compile here
    /// rather than silently painting plain — mirrors the P5
    /// `StructuralDecor` contract) and `Capture` (never panics: an id
    /// outside `crate::role`'s allocated range degrades to `Plain` inside
    /// [`Capture::from_id`], not a crash).
    pub fn resolve(&self, id: StyleId) -> Style {
        match id {
            StyleId::Semantic(semantic) => self.resolve_semantic(semantic),
            StyleId::Capture(raw) => self.resolve_capture(Capture::from_id(raw)),
        }
    }

    fn resolve_semantic(&self, semantic: Semantic) -> Style {
        let attrs = semantic_attrs(semantic);
        let fg = semantic_role_index(semantic).and_then(|idx| self.color_for(idx));
        Style { fg, ..attrs }
    }

    fn resolve_capture(&self, capture: Capture) -> Style {
        let bold = matches!(capture, Capture::Keyword | Capture::KeywordControl);
        let italic_dim = matches!(capture, Capture::Comment | Capture::CommentDoc);
        let fg = capture_role_index(capture).and_then(|idx| self.color_for(idx));
        Style {
            fg,
            bold,
            dim: italic_dim,
            italic: italic_dim,
            ..Style::default()
        }
    }

    fn color_for(&self, role_index: usize) -> Option<Color> {
        let truecolor = *self.palette.get(role_index).expect(
            "role_index is always < TOTAL_ROLES by construction of the *_role_index functions",
        );
        color::apply_mode(truecolor, self.color_mode)
    }
}

/// Structural (non-color) attributes per `Semantic` role — exhaustive, no
/// wildcard arm, so a new `Semantic` variant is a compile error here rather
/// than a silent plain style.
fn semantic_attrs(semantic: Semantic) -> Style {
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
        | Semantic::FootnoteLabel => bold,
        Semantic::AlertTitle(_) => bold,
        Semantic::Emph | Semantic::ImageAlt | Semantic::MathTex => italic,
        Semantic::Link | Semantic::FootnoteRef => underline,
        Semantic::BlockquoteMarker
        | Semantic::Rule
        | Semantic::TableBorder
        | Semantic::Html
        | Semantic::FrontMatter
        | Semantic::OverflowIndicator
        | Semantic::Strikethrough => dim,
        Semantic::Text
        | Semantic::CodeInline
        | Semantic::CodeBlock
        | Semantic::ListMarker
        | Semantic::TaskMarker => Style::default(),
    }
}

/// Assigns each colored `Semantic` role a stable palette index in
/// `0..SEMANTIC_ROLES`. `None` means "this role never carries color" (its
/// [`semantic_attrs`] styling — bold/dim/italic only — is the whole story).
/// Exhaustive for the same reason as [`semantic_attrs`].
fn semantic_role_index(semantic: Semantic) -> Option<usize> {
    Some(match semantic {
        Semantic::Heading(_) => 0,
        Semantic::CodeInline => 1,
        Semantic::Link => 2,
        Semantic::ImageAlt => 3,
        Semantic::ListMarker => 4,
        Semantic::TaskMarker => 5,
        Semantic::BlockquoteMarker => 6,
        Semantic::AlertTitle(AlertTone::Note) => 7,
        Semantic::AlertTitle(AlertTone::Tip) => 8,
        Semantic::AlertTitle(AlertTone::Important) => 9,
        Semantic::AlertTitle(AlertTone::Warning) => 10,
        Semantic::AlertTitle(AlertTone::Caution) => 11,
        Semantic::Rule => 12,
        Semantic::TableBorder => 13,
        Semantic::TableHeader => 14,
        Semantic::FootnoteRef => 15,
        Semantic::FootnoteLabel => 16,
        Semantic::Html => 17,
        Semantic::FrontMatter => 18,
        Semantic::OverflowIndicator => 19,
        Semantic::Text
        | Semantic::Strong
        | Semantic::Emph
        | Semantic::Strikethrough
        | Semantic::CodeBlock
        | Semantic::MathTex => {
            return None;
        }
    })
}

/// Assigns each colored `Capture` role a palette index past the `Semantic`
/// block. `Plain` (the unmapped-scope catch-all) never carries color.
fn capture_role_index(capture: Capture) -> Option<usize> {
    if capture == Capture::Plain {
        return None;
    }
    role::ALL
        .iter()
        .position(|&r| r == capture)
        .map(|pos| SEMANTIC_ROLES + pos)
}

/// The golden angle, in turns (fraction of a full hue rotation) rather than
/// degrees — successive multiples spread points around a circle with
/// minimal clustering for any prefix length, which is exactly the property
/// wanted here: every role gets a hue far from every other role's, by
/// construction, rather than by hand-picked hex values a reviewer would
/// have to eyeball for collisions.
const GOLDEN_ANGLE_TURNS: f64 = 0.618_033_988_75;

/// Total number of colored roles this theme allocates a palette slot to —
/// `SEMANTIC_ROLES` plus every non-`Plain` `Capture`. Exposed for tests that
/// must exercise every role, not just a hand-picked sample.
pub fn role_count() -> usize {
    SEMANTIC_ROLES + (role::ALL.len() - 1)
}

/// A candidate truecolor for palette-generation `attempt` N: hue rotates by
/// the golden angle per attempt (spreading hues with minimal clustering),
/// while lightness/saturation additionally cycle through five bands
/// (`attempt % 5`) so the search sweeps a genuine 3-D spread through color
/// space rather than a single hue ring at one fixed lightness — a single
/// ring, once snapped to the 256-color cube's 6 steps per channel, does not
/// reliably carry `role_count()` (44) distinct cells (this was verified
/// empirically: an earlier single-ring version of this palette collided).
fn candidate_color(attempt: usize, variant: Variant) -> Color {
    let hue = (attempt as f64) * GOLDEN_ANGLE_TURNS;
    let (base_saturation, base_lightness) = match variant {
        Variant::Dark => (0.62, 0.66),
        Variant::Light => (0.68, 0.36),
    };
    const BAND_OFFSETS: [(f64, f64); 5] = [
        (0.0, 0.0),
        (0.16, -0.12),
        (-0.16, 0.12),
        (0.08, 0.20),
        (-0.08, -0.20),
    ];
    let (lightness_offset, saturation_offset) = BAND_OFFSETS[attempt % BAND_OFFSETS.len()];
    let lightness = (base_lightness + lightness_offset).clamp(0.10, 0.90);
    let saturation = (base_saturation + saturation_offset).clamp(0.30, 0.95);
    color::hsl_to_rgb(hue, saturation, lightness)
}

/// Builds `role_count()` truecolor palette entries for `variant`, greedily
/// skipping any candidate whose *256-color-downsampled* value collides with
/// an already-accepted entry. Guaranteeing distinctness under the harder
/// (lossier) 256-color constraint also guarantees it under truecolor —
/// this is what makes DW-7.2's "theme roles downsample to distinct colors"
/// invariant true by construction rather than by hoping a formula happens
/// to avoid collisions.
fn build_palette(variant: Variant) -> Vec<Color> {
    let total = role_count();
    let mut palette = Vec::with_capacity(total);
    let mut used_downsampled = std::collections::HashSet::with_capacity(total);
    let mut attempt = 0usize;
    while palette.len() < total {
        let candidate = candidate_color(attempt, variant);
        if used_downsampled.insert(color::downsample_256(candidate)) {
            palette.push(candidate);
        }
        attempt += 1;
        assert!(
            attempt < 100_000,
            "palette generation could not find {total} distinct 256-downsampled colors for {variant:?} \
             after {attempt} attempts — widen candidate_color's search bands"
        );
    }
    palette
}

/// Parses a terminal's OSC 11 background-color query reply (Spike A, item
/// 8: `"\x1b]11;rgb:1a1a/1b1b/2626\x1b\\"`) and classifies it as
/// [`Variant::Dark`] or [`Variant::Light`] by perceived luminance
/// (ITU-R BT.709 relative luminance weights). Returns `None` for anything
/// that doesn't parse as an `rgb:` triple — including an empty/unanswered
/// query — so the caller can apply the plan's documented fallback (default
/// dark).
pub fn variant_from_osc11_reply(reply: &[u8]) -> Option<Variant> {
    let text = std::str::from_utf8(reply).ok()?;
    let after = text.split_once("rgb:")?.1;
    let end = after.find(['\u{1b}', '\u{7}']).unwrap_or(after.len());
    let mut channels = after[..end].splitn(3, '/');
    let r = parse_channel(channels.next()?)?;
    let g = parse_channel(channels.next()?)?;
    let b = parse_channel(channels.next()?)?;
    let luminance = 0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b);
    Some(if luminance < 128.0 {
        Variant::Dark
    } else {
        Variant::Light
    })
}

/// Parses one X11-style `rgb:` channel group (1-4 hex digits representing a
/// fixed-point fraction of `16^digits - 1`) down to an 8-bit value. Never
/// panics on malformed input — an OSC reply is terminal-controlled, not
/// attacker-controlled, but this still validates rather than assumes
/// (cc-defensive-programming: barricades validate even trusted-ish sources
/// when parsing is nontrivial).
fn parse_channel(group: &str) -> Option<u8> {
    if group.is_empty() || group.len() > 4 || !group.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(group, 16).ok()?;
    let max = 16u32
        .checked_pow(group.len() as u32)?
        .saturating_sub(1)
        .max(1);
    Some(((value * 255) / max) as u8)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use layout::{AlertTone, Semantic};

    use super::*;

    #[test]
    fn test_resolve_is_exhaustive_and_dispatches_by_kind() {
        let theme = Theme::new(Variant::Dark, ColorMode::Truecolor);
        let heading = theme.resolve(StyleId::Semantic(Semantic::Heading(1)));
        assert!(heading.bold);
        assert!(heading.fg.is_some());

        let plain_capture = theme.resolve(StyleId::Capture(Capture::Plain.id()));
        assert_eq!(plain_capture.fg, None);

        let keyword_capture = theme.resolve(StyleId::Capture(Capture::Keyword.id()));
        assert!(keyword_capture.fg.is_some());
        assert!(keyword_capture.bold);
    }

    #[test]
    fn test_dw_7_2_no_color_mode_clears_fg_bg_everywhere_but_keeps_structural() {
        let theme = Theme::new(Variant::Dark, ColorMode::NoColor);
        for semantic in all_semantics() {
            let style = theme.resolve(StyleId::Semantic(semantic));
            assert_eq!(
                style.fg, None,
                "{semantic:?} carried a color under NO_COLOR"
            );
            assert_eq!(style.bg, None);
        }
        for &capture in role::ALL.iter() {
            let style = theme.resolve(StyleId::Capture(capture.id()));
            assert_eq!(style.fg, None, "{capture:?} carried a color under NO_COLOR");
        }
        // Structural attributes must still come through — NO_COLOR removes
        // color, not all styling.
        let heading = theme.resolve(StyleId::Semantic(Semantic::Heading(1)));
        assert!(heading.bold);
    }

    #[test]
    fn test_dw_7_2_downsample_256_keeps_every_role_distinct_dark_and_light() {
        for variant in [Variant::Dark, Variant::Light] {
            let palette = build_palette(variant);
            assert_eq!(palette.len(), role_count());
            let mut seen = HashSet::new();
            for (idx, &truecolor) in palette.iter().enumerate() {
                let color = color::apply_mode(truecolor, ColorMode::Downsample256)
                    .expect("Downsample256 always yields a color");
                assert!(
                    seen.insert(color),
                    "role index {idx} collided with an earlier role after 256-color downsample ({variant:?}): {color:?}"
                );
            }
            assert_eq!(seen.len(), role_count());

            // Cross-check the same invariant through the public `resolve`
            // entry point (not just the internal palette function): every
            // colored semantic role and every colored capture role must
            // still land on one of the `seen` colors, with no two distinct
            // roles resolving to the same one.
            let theme = Theme::new(variant, ColorMode::Downsample256);
            let mut resolved = HashSet::new();
            for semantic in all_semantics() {
                if let Some(fg) = theme.resolve(StyleId::Semantic(semantic)).fg {
                    resolved.insert(fg);
                }
            }
            for &capture in role::ALL.iter() {
                if let Some(fg) = theme.resolve(StyleId::Capture(capture.id())).fg {
                    resolved.insert(fg);
                }
            }
            assert_eq!(resolved, seen);
        }
    }

    fn all_semantics() -> Vec<Semantic> {
        let mut v = vec![
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
        ];
        for level in 1..=6 {
            v.push(Semantic::Heading(level));
        }
        for tone in [
            AlertTone::Note,
            AlertTone::Tip,
            AlertTone::Important,
            AlertTone::Warning,
            AlertTone::Caution,
        ] {
            v.push(Semantic::AlertTitle(tone));
        }
        v
    }

    #[test]
    fn test_dw_7_2_truecolor_mode_emits_color() {
        let theme = Theme::new(Variant::Dark, ColorMode::Truecolor);
        assert!(
            theme
                .resolve(StyleId::Semantic(Semantic::Heading(1)))
                .fg
                .is_some()
        );
    }

    #[test]
    fn test_variant_from_osc11_reply_dark_and_light() {
        // Spike A's actual captured reply for the reference Tokyo Night
        // dark theme.
        let dark = b"\x1b]11;rgb:1a1a/1b1b/2626\x1b\\";
        assert_eq!(variant_from_osc11_reply(dark), Some(Variant::Dark));

        let light = b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\";
        assert_eq!(variant_from_osc11_reply(light), Some(Variant::Light));
    }

    #[test]
    fn test_variant_from_osc11_reply_bel_terminator_and_short_hex() {
        let reply = b"\x1b]11;rgb:00/00/00\x07";
        assert_eq!(variant_from_osc11_reply(reply), Some(Variant::Dark));
    }

    #[test]
    fn test_variant_from_osc11_reply_unparseable_is_none_not_a_panic() {
        assert_eq!(variant_from_osc11_reply(b""), None);
        assert_eq!(variant_from_osc11_reply(b"garbage"), None);
        assert_eq!(
            variant_from_osc11_reply(b"\x1b]11;rgb:zz/zz/zz\x1b\\"),
            None
        );
        assert_eq!(variant_from_osc11_reply(b"\x1b]11;rgb:11/22\x1b\\"), None);
    }
}
