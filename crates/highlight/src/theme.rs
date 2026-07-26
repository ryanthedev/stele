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

/// Number of distinct-colored `Semantic` roles allocated *before* the
/// capture block (see [`semantic_role_index`]). Capture roles (excluding the
/// uncolored `Plain` catch-all) follow these, and anything added later
/// follows the captures — see [`TRAILING_ROLE_BASE`]. Kept in one constant
/// so the palette size and the role-index functions can never drift apart.
/// The first [`HEADING_TIERS`] entries are the heading ramp, not one shared
/// heading slot.
///
/// **Do not raise this to make room for a new role.** [`build_palette`]
/// generates slots in index order, so every slot at or after an inserted one
/// takes the color that used to belong to its neighbour. Raising it by two
/// for the Phase 4 search roles silently repainted all 24 colored capture
/// roles — `Comment` moved from `rgb(232,205,186)` to `rgb(153,243,134)`,
/// `Keyword` from `rgb(193,110,103)` to `rgb(222,33,183)` — which no
/// invariant here forbids and no test caught, because distinctness survives
/// any permutation. New roles go at [`TRAILING_ROLE_BASE`], where appending
/// is inert.
const SEMANTIC_ROLES: usize = 22;

/// The first palette slot past every historically-allocated role. A role
/// added here inherits a brand-new color and disturbs nothing that already
/// had one, which is the property that makes adding a role a local change.
const TRAILING_ROLE_BASE: usize = SEMANTIC_ROLES + CAPTURE_ROLES;

/// Colored `Capture` roles: every variant of [`role::ALL`] except `Plain`,
/// which never carries color.
const CAPTURE_ROLES: usize = role::ALL.len() - 1;

/// How many heading levels CommonMark defines. `crates/layout` guarantees
/// `Semantic::Heading`'s level is 1–6; every consumer here still clamps, so
/// a level outside that range degrades to the nearest rung rather than
/// panicking on an out-of-range palette index.
const HEADING_LEVELS: u8 = 6;

/// How many palette slots the heading ramp occupies — two levels per tier.
/// See [`HEADING_RAMP`] for why this is 3 and not 6.
const HEADING_TIERS: usize = 3;

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
        let fg = semantic_role_index(semantic).and_then(|idx| self.color_for(idx));
        // Headings pick their ladder from whether a color actually came back,
        // not from the color *mode*: a role that resolves to `None` here is
        // monochrome in practice however it got that way.
        let attrs = match semantic {
            Semantic::Heading(level) => heading_attrs(level, fg.is_none()),
            other => semantic_attrs(other),
        };
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
        // Headings never route here — `resolve_semantic` sends them to
        // `heading_attrs` with the monochrome flag this function cannot see.
        // The arm exists so the match stays exhaustive without a wildcard.
        Semantic::Heading(level) => heading_attrs(level, true),
        Semantic::Strong | Semantic::TableHeader | Semantic::FootnoteLabel => bold,
        Semantic::AlertTitle(_) => bold,
        Semantic::Emph | Semantic::ImageAlt | Semantic::MathTex => italic,
        Semantic::Link | Semantic::FootnoteRef => underline,
        // Kept identical to the themeless table in `crates/stele/src/decor`,
        // and for that table's reason rather than this one's: under
        // `ColorMode::NoColor` these attributes are all a reader gets here
        // too, so `SearchMatch` sharing `Link`'s bare underline would make a
        // match inside a link invisible. `italic` is the free combination
        // that separates both search roles from `Link`, `FootnoteRef` and
        // `Heading(1)` alike.
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

/// The heading ladder, loudest (H1) to quietest (H6). Which ladder depends on
/// whether color is available, because the two cases need different things:
///
/// - **Colored** (`monochrome == false`): the ramp already separates the three
///   tiers, so these attributes only need to distinguish the two levels
///   *inside* a tier. `dim` is deliberately absent — SGR faint blends the
///   glyph toward the background, and stacking it on the quietest ramp tier
///   drops that tier to roughly 2.4:1 against the reference dark background,
///   well under WCAG AA, no matter which color the tier wears.
/// - **Monochrome** (`ColorMode::NoColor`, and the themeless
///   `crates/stele/src/decor` path): nothing else carries level, so all six
///   rungs must differ by attributes alone. `dim` earns its place here — it
///   attenuates the terminal's own foreground rather than an already-dimmed
///   ramp color, the same trade `BlockquoteMarker` and `Rule` already make.
fn heading_attrs(level: u8, monochrome: bool) -> Style {
    let bold = Style {
        bold: true,
        ..Style::default()
    };
    let italic = Style {
        italic: true,
        ..Style::default()
    };
    match (level.clamp(1, HEADING_LEVELS), monochrome) {
        (1, _) => Style {
            underline: true,
            ..bold
        },
        (2, _) => bold,
        (3, _) => Style {
            italic: true,
            ..bold
        },
        (4, _) => italic,
        // Tier 3 in color: plain and italic, distinguished from the identical
        // pair in tier 2 by the ramp color they carry.
        (5, false) => Style::default(),
        (_, false) => italic,
        (5, true) => Style {
            dim: true,
            ..Style::default()
        },
        (_, true) => Style {
            dim: true,
            italic: true,
            ..Style::default()
        },
    }
}

/// Assigns each colored `Semantic` role a stable palette index in
/// `0..SEMANTIC_ROLES`. `None` means "this role never carries color" (its
/// [`semantic_attrs`] styling — bold/dim/italic only — is the whole story).
/// Exhaustive for the same reason as [`semantic_attrs`].
fn semantic_role_index(semantic: Semantic) -> Option<usize> {
    Some(match semantic {
        // Heading levels share three palette slots (0–2), two levels per
        // tier: the ramp carries the tier, `heading_attrs` separates the two
        // levels inside it. `heading_tier` clamps, so an out-of-range level
        // lands on a real tier instead of indexing past the ramp.
        Semantic::Heading(level) => heading_tier(level),
        Semantic::CodeInline => 3,
        Semantic::Link => 4,
        Semantic::ImageAlt => 5,
        Semantic::ListMarker => 6,
        Semantic::TaskMarker => 7,
        Semantic::BlockquoteMarker => 8,
        Semantic::AlertTitle(AlertTone::Note) => 9,
        Semantic::AlertTitle(AlertTone::Tip) => 10,
        Semantic::AlertTitle(AlertTone::Important) => 11,
        Semantic::AlertTitle(AlertTone::Warning) => 12,
        Semantic::AlertTitle(AlertTone::Caution) => 13,
        Semantic::Rule => 14,
        Semantic::TableBorder => 15,
        Semantic::TableHeader => 16,
        Semantic::FootnoteRef => 17,
        Semantic::FootnoteLabel => 18,
        Semantic::Html => 19,
        Semantic::FrontMatter => 20,
        Semantic::OverflowIndicator => 21,
        // The two search roles take their own palette slots rather than
        // sharing one, so DW-4.8's "distinct after 256-color downsampling"
        // holds by construction of `build_palette` — the same greedy
        // distinctness filter every other role passes through. Placed past
        // the capture block rather than inserted at 22/23: see
        // [`SEMANTIC_ROLES`] for the colors that move if a role is inserted
        // instead of appended.
        Semantic::SearchMatch => TRAILING_ROLE_BASE,
        Semantic::SearchCurrent => TRAILING_ROLE_BASE + 1,
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
/// every `Semantic` role, every non-`Plain` `Capture`, and the trailing
/// roles past them. Exposed for tests that must exercise every role, not
/// just a hand-picked sample.
pub fn role_count() -> usize {
    TRAILING_ROLE_BASE + TRAILING_ROLES
}

/// How many roles sit past the capture block: `SearchMatch` and
/// `SearchCurrent`.
const TRAILING_ROLES: usize = 2;

/// A candidate truecolor for palette-generation `attempt` N: hue rotates by
/// the golden angle per attempt (spreading hues with minimal clustering),
/// while lightness/saturation additionally cycle through five bands
/// (`attempt % 5`) so the search sweeps a genuine 3-D spread through color
/// space rather than a single hue ring at one fixed lightness — a single
/// ring, once snapped to the 256-color cube's 6 steps per channel, does not
/// reliably carry `role_count()` (46) distinct cells (this was verified
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

/// The heading ramp's hue, in turns — the hue headings have always carried
/// (`candidate_color`'s attempt 0 lands here too), so the ramp stays in the
/// family the theme already used, even though every tier's *lightness* is new
/// and no tier reproduces the old heading color exactly.
///
/// Because this hue is now reserved, [`build_palette`] starts its
/// golden-angle loop at attempt 1: leaving attempt 0 in the general pool
/// handed `CodeInline` a color 11.5 rgb units from the tier-3 heading color on
/// dark (`#de7373` against `#e07b7b` — indistinguishable in truecolor), where
/// the palette's tightest pre-existing pair was 18.4 apart.
///
/// Every other role's palette slot shifts by `HEADING_TIERS - 1` as a result;
/// nothing persists a palette index across runs, so the renumbering itself is
/// invisible outside this file, though the colors those roles resolve to do
/// change.
const HEADING_HUE_TURNS: f64 = 0.0;

/// Saturation and per-tier lightness for the heading ramp, loudest first,
/// where "loud" means *furthest from the background*: lightness descends on
/// dark, ascends on light.
///
/// Three tiers, not six, and the reason is legibility rather than taste. The
/// ramp holds one hue, and one hue offers only about six cells that survive
/// the 256-color cube's 6-steps-per-channel quantization — spanning all six
/// forces the deep end down to where it stops being readable (measured
/// against the Spike A reference background `#1a1b26`: a six-rung version of
/// this table put H6 at 3.13:1 and H5 at 4.20:1, both under WCAG AA's 4.5:1,
/// and the light variant's H6 at 2.73:1). Three tiers keep every heading
/// color at 5.9:1 or better in both variants; [`heading_attrs`] separates the
/// two levels inside a tier. Move a value and [`build_palette`] refuses to
/// build rather than ship two tiers that look identical in 256-color mode;
/// `test_heading_tiers_clear_wcag_aa_against_the_reference_backgrounds` is
/// what stops a "nicer" value from quietly going back under AA.
const HEADING_RAMP: [(f64, [f64; HEADING_TIERS]); 2] = [
    // Dark background: pale → deep.
    (0.62, [0.86, 0.76, 0.68]),
    // Light background: deep → pale.
    (0.68, [0.22, 0.30, 0.40]),
];

/// Which color tier a heading level draws from: levels 1–2 the loudest, 3–4
/// the middle, 5–6 the quietest.
fn heading_tier(level: u8) -> usize {
    usize::from(level.clamp(1, HEADING_LEVELS) - 1) / 2
}

/// One tier of the heading color ramp.
fn heading_ramp_color(tier: usize, variant: Variant) -> Color {
    let (saturation, lightnesses) = match variant {
        Variant::Dark => HEADING_RAMP[0],
        Variant::Light => HEADING_RAMP[1],
    };
    color::hsl_to_rgb(HEADING_HUE_TURNS, saturation, lightnesses[tier])
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

    // Slots 0..HEADING_TIERS first, so the ramp keeps one hue instead of
    // drawing golden-angle-separated hues — a rainbow of heading levels would
    // read as unrelated roles rather than one hierarchy. These still pass
    // through the same 256-downsample distinctness filter as every other
    // role, so DW-7.2's invariant holds by construction here too.
    for tier in 0..HEADING_TIERS {
        let candidate = heading_ramp_color(tier, variant);
        assert!(
            used_downsampled.insert(color::downsample_256(candidate)),
            "heading ramp tier {tier} downsamples onto a cell an earlier tier already took \
             ({variant:?}) — two heading tiers would be the same color in 256-color mode; \
             pick a different lightness in HEADING_RAMP"
        );
        palette.push(candidate);
    }

    // Attempt 0 shares the heading ramp's hue (both sit at hue 0), and the
    // 256-cell filter below is too coarse to catch that: its color survives as
    // a distinct *cell* while looking identical to a heading tier. Skipping it
    // costs nothing — the golden angle never revisits hue 0 — and it is what
    // keeps `CodeInline`, the role that inherits this slot, off the heading
    // family. See [`HEADING_HUE_TURNS`] for the measurement.
    let mut attempt = 1usize;
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

    /// Every `Semantic` variant. The `match` has no wildcard arm, so a new
    /// variant is a **compile error here** until this list names it.
    ///
    /// The guard is load-bearing rather than tidy. This list is what
    /// `test_dw_7_2_downsample_256_keeps_every_role_distinct...` compares the
    /// generated palette against, and the "no wildcard arms over `Semantic`"
    /// rule that protects the style tables does not reach a `vec![]`. When
    /// Phase 4 added two variants the list silently went stale, and that test
    /// failed with an opaque set-inequality dump rather than naming the roles
    /// it was missing.
    fn all_semantics() -> Vec<Semantic> {
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
            Semantic::SearchMatch,
            Semantic::SearchCurrent,
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

    /// DW-4.8. The two search roles are new palette slots, so
    /// `build_palette`'s greedy filter already guarantees they land on their
    /// own 256-color cells — this asserts that guarantee through the public
    /// `resolve` entry point, role by role, rather than trusting the
    /// generator. `test_dw_7_2_downsample_256_keeps_every_role_distinct...`
    /// covers the whole palette as a set; this names the two roles the phase
    /// added, so a failure says *which* roles collided.
    #[test]
    fn test_dw_4_8_search_roles_stay_distinct_from_every_other_role_after_256_downsample() {
        for variant in [Variant::Dark, Variant::Light] {
            let theme = Theme::new(variant, ColorMode::Downsample256);
            let color = |semantic: Semantic| theme.resolve(StyleId::Semantic(semantic)).fg;

            let matched = color(Semantic::SearchMatch)
                .expect("SearchMatch must carry a palette color, not paint plain");
            let current = color(Semantic::SearchCurrent)
                .expect("SearchCurrent must carry a palette color, not paint plain");
            assert_ne!(
                matched, current,
                "the current match must be a different 256-color cell from the \
                 other matches in {variant:?} — otherwise `n` moves nothing visible"
            );

            for semantic in all_semantics() {
                if matches!(semantic, Semantic::SearchMatch | Semantic::SearchCurrent) {
                    continue;
                }
                let Some(other) = color(semantic) else {
                    continue;
                };
                assert_ne!(
                    other, matched,
                    "{semantic:?} downsamples onto SearchMatch's cell in {variant:?}"
                );
                assert_ne!(
                    other, current,
                    "{semantic:?} downsamples onto SearchCurrent's cell in {variant:?}"
                );
            }

            // Capture roles too: a match inside a code block sits directly
            // beside these, which is where a collision would actually be seen.
            for &capture in role::ALL.iter() {
                let Some(other) = theme.resolve(StyleId::Capture(capture.id())).fg else {
                    continue;
                };
                assert_ne!(
                    other, matched,
                    "capture {capture:?} downsamples onto SearchMatch's cell in {variant:?}"
                );
                assert_ne!(
                    other, current,
                    "capture {capture:?} downsamples onto SearchCurrent's cell in {variant:?}"
                );
            }
        }
    }

    /// **Regression gate for the whole palette's stability.** Every existing
    /// role's color must survive a role being added.
    ///
    /// This exists because it did not. Phase 4 raised `SEMANTIC_ROLES` from
    /// 22 to 24 to make room for the two search roles, which shifted
    /// `capture_role_index = SEMANTIC_ROLES + pos` by two and silently gave
    /// all 24 colored capture roles somebody else's color — every keyword
    /// and comment in every code block in the viewer repainted. Nothing
    /// caught it: `test_dw_7_2_downsample_256_keeps_every_role_distinct...`
    /// asserts the palette is a *set* of distinct colors, and a permutation
    /// of a set is still that set.
    ///
    /// The values below are the pre-Phase-4 colors, read off the base commit
    /// and diffed against the current palette to confirm they were restored
    /// exactly. Pinned in both variants and at both ends of the capture
    /// block, so an insertion anywhere inside it fails here. If a deliberate
    /// palette change ever makes these wrong, the fix is to re-record them
    /// in the same commit as the change — not to loosen the assertion.
    #[test]
    fn test_capture_colors_are_pinned_so_a_new_role_cannot_silently_restyle_code_blocks() {
        let expected = [
            (Variant::Dark, Capture::Keyword, Color::new(193, 110, 103)),
            (Variant::Dark, Capture::Comment, Color::new(232, 205, 186)),
            (Variant::Dark, Capture::String, Color::new(134, 194, 243)),
            (Variant::Light, Capture::Keyword, Color::new(13, 122, 211)),
            (Variant::Light, Capture::Comment, Color::new(201, 195, 64)),
            (Variant::Light, Capture::String, Color::new(211, 45, 13)),
        ];
        for (variant, capture, color) in expected {
            let theme = Theme::new(variant, ColorMode::Truecolor);
            assert_eq!(
                theme.resolve(StyleId::Capture(capture.id())).fg,
                Some(color),
                "{capture:?} changed color in {variant:?} — a role was inserted \
                 into the palette rather than appended at TRAILING_ROLE_BASE, \
                 which moves every slot after it"
            );
        }
    }

    /// The structural half of the same guarantee: the capture block starts
    /// where it has always started, so the pinned colors above are not
    /// merely two lucky slots.
    #[test]
    fn test_the_capture_block_still_begins_immediately_after_the_semantic_roles() {
        assert_eq!(capture_role_index(role::ALL[0]), Some(SEMANTIC_ROLES));
        // `capture_role_index` is `SEMANTIC_ROLES + position in ALL`, so the
        // uncolored `Plain` has to sit *last* or it would punch an unused
        // hole in the middle of the block and push the final capture one
        // slot past the palette's end.
        assert_eq!(role::ALL.last(), Some(&Capture::Plain));
        assert_eq!(capture_role_index(Capture::Plain), None);
        assert_eq!(
            capture_role_index(role::ALL[CAPTURE_ROLES - 1]),
            Some(TRAILING_ROLE_BASE - 1),
            "the last colored capture must sit immediately before the trailing block"
        );
        assert_eq!(
            semantic_role_index(Semantic::SearchMatch),
            Some(TRAILING_ROLE_BASE),
            "a new role belongs past the captures, not inside them"
        );
        // No assertion that `TRAILING_ROLE_BASE == SEMANTIC_ROLES +
        // CAPTURE_ROLES`: that is its definition, so the check could not
        // fail. The two above are the ones with content — they read the
        // index *functions*, which is where a real mistake would live.
        assert_eq!(role_count(), TRAILING_ROLE_BASE + TRAILING_ROLES);
        assert_eq!(
            build_palette(Variant::Dark).len(),
            role_count(),
            "the palette must have a slot for every role the index functions hand out"
        );
    }

    /// Color is not the only channel, and it is the one a `NO_COLOR`
    /// terminal does not have. The two roles must still be told apart by
    /// their attributes alone, and must still read as *matches* rather than
    /// as some unrelated role.
    #[test]
    fn test_dw_4_8_search_roles_differ_by_attributes_even_with_color_stripped() {
        let theme = Theme::new(Variant::Dark, ColorMode::NoColor);
        let style = |semantic| theme.resolve(StyleId::Semantic(semantic));
        let matched = style(Semantic::SearchMatch);
        let current = style(Semantic::SearchCurrent);
        assert_eq!(matched.fg, None);
        assert_eq!(current.fg, None);
        assert_ne!(
            matched, current,
            "with color stripped the attributes are all that is left"
        );
        assert!(
            matched.underline && current.underline,
            "both read as matches"
        );
        assert!(
            current.bold && !matched.bold,
            "weight is what separates the current match: {current:?} vs {matched:?}"
        );

        // Under NO_COLOR this table has exactly the problem the themeless
        // `StructuralDecor` path has: attributes are the only channel, so
        // sharing an attribute set with another role *is* a collision.
        // Checked against every role rather than a sample — `Link` and
        // `Heading(1)` are the two these roles used to collide with, and are
        // exactly the two a hand-picked sample is most likely to omit.
        for semantic in all_semantics() {
            if matches!(semantic, Semantic::SearchMatch | Semantic::SearchCurrent) {
                continue;
            }
            assert_ne!(
                style(semantic),
                matched,
                "{semantic:?} is indistinguishable from a search match under NO_COLOR"
            );
            assert_ne!(
                style(semantic),
                current,
                "{semantic:?} is indistinguishable from the current match under NO_COLOR"
            );
        }
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

    /// Level must survive whether or not the terminal has color. With color,
    /// the (tier color, attributes) pair is what distinguishes a level — two
    /// levels sharing a tier must differ in attributes, and two levels sharing
    /// attributes must differ in tier. Without color, the attribute ladder
    /// carries all six alone, which is where this started.
    #[test]
    fn test_heading_levels_are_distinct_in_color_and_in_attributes() {
        for variant in [Variant::Dark, Variant::Light] {
            let theme = Theme::new(variant, ColorMode::Truecolor);
            let mut seen = Vec::new();
            let mut tiers = HashSet::new();
            for level in 1..=HEADING_LEVELS {
                let style = theme.resolve(StyleId::Semantic(Semantic::Heading(level)));
                let fg = style.fg.expect("every heading level carries a tier color");
                tiers.insert(fg);
                assert!(
                    !seen.contains(&style),
                    "H{level} is indistinguishable from a shallower level in {variant:?}: {style:?}"
                );
                assert!(
                    !style.dim,
                    "H{level} carries SGR faint on top of a ramp color in {variant:?} — \
                     that is the combination that fell under WCAG AA"
                );
                seen.push(style);
            }
            assert_eq!(
                tiers.len(),
                HEADING_TIERS,
                "the six levels should draw on exactly {HEADING_TIERS} tier colors"
            );

            // The attribute half must stand alone with color stripped.
            let no_color = Theme::new(variant, ColorMode::NoColor);
            let mut bare_attrs = Vec::new();
            for level in 1..=HEADING_LEVELS {
                let style = no_color.resolve(StyleId::Semantic(Semantic::Heading(level)));
                assert_eq!(style.fg, None);
                assert!(
                    !bare_attrs.contains(&style),
                    "H{level} collapsed onto another level under NO_COLOR: {style:?}"
                );
                bare_attrs.push(style);
            }
        }
    }

    /// The ramp is a hierarchy, so its contrast against the background must
    /// fall monotonically with depth — tier 2 may not out-shout tier 1.
    /// Asserted through resolved colors rather than the generator, so it also
    /// covers the level → tier mapping.
    #[test]
    fn test_heading_ramp_is_monotone_in_contrast() {
        for variant in [Variant::Dark, Variant::Light] {
            let theme = Theme::new(variant, ColorMode::Truecolor);
            let luminance = |level: u8| -> f64 {
                let fg = theme
                    .resolve(StyleId::Semantic(Semantic::Heading(level)))
                    .fg
                    .expect("every heading level carries a tier color");
                0.2126 * f64::from(fg.r) + 0.7152 * f64::from(fg.g) + 0.0722 * f64::from(fg.b)
            };
            // Levels 1, 3, 5 are the first level of each tier.
            for level in [3u8, 5] {
                let (deeper, shallower) = (luminance(level), luminance(level - 2));
                match variant {
                    // Dark background: louder = brighter, so luminance falls.
                    Variant::Dark => assert!(
                        deeper < shallower,
                        "H{level} is brighter than H{} on dark ({deeper} >= {shallower})",
                        level - 2
                    ),
                    // Light background: louder = darker, so luminance rises.
                    Variant::Light => assert!(
                        deeper > shallower,
                        "H{level} is darker than H{} on light ({deeper} <= {shallower})",
                        level - 2
                    ),
                }
                assert_eq!(
                    luminance(level),
                    luminance(level + 1),
                    "H{level} and H{} share a tier and must share its color",
                    level + 1
                );
            }
        }
    }

    /// Uniqueness tests permit any *permutation* of the ladder — swap H5's and
    /// H6's attributes, or map H2 onto tier 2, and they still pass. This pins
    /// the actual promised mapping, level by level, in both ladders.
    #[test]
    fn test_heading_ladder_maps_each_level_to_its_specified_style() {
        let bold = Style {
            bold: true,
            ..Style::default()
        };
        let italic = Style {
            italic: true,
            ..Style::default()
        };
        let colored = [
            Style {
                underline: true,
                ..bold
            },
            bold,
            Style {
                italic: true,
                ..bold
            },
            italic,
            Style::default(),
            italic,
        ];
        let monochrome = [
            colored[0],
            colored[1],
            colored[2],
            colored[3],
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

        for variant in [Variant::Dark, Variant::Light] {
            let theme = Theme::new(variant, ColorMode::Truecolor);
            let no_color = Theme::new(variant, ColorMode::NoColor);
            for level in 1..=HEADING_LEVELS {
                let idx = usize::from(level - 1);
                let got = theme.resolve(StyleId::Semantic(Semantic::Heading(level)));
                assert_eq!(
                    Style { fg: None, ..got },
                    colored[idx],
                    "H{level}'s colored attributes in {variant:?}"
                );
                assert_eq!(
                    no_color.resolve(StyleId::Semantic(Semantic::Heading(level))),
                    monochrome[idx],
                    "H{level}'s monochrome attributes in {variant:?}"
                );
            }

            // ...and that the tiers pair the levels they claim to: 1-2, 3-4,
            // 5-6, each pair sharing one color, each pair differing from the
            // next. Uniqueness alone would allow H2 to sit in tier 2.
            let fg = |level: u8| {
                theme
                    .resolve(StyleId::Semantic(Semantic::Heading(level)))
                    .fg
                    .expect("every heading level carries a tier color")
            };
            for pair_start in [1u8, 3, 5] {
                assert_eq!(
                    fg(pair_start),
                    fg(pair_start + 1),
                    "H{pair_start} and H{} must share a tier color in {variant:?}",
                    pair_start + 1
                );
            }
            assert_ne!(fg(2), fg(3), "tier 1 and tier 2 must differ in {variant:?}");
            assert_ne!(fg(4), fg(5), "tier 2 and tier 3 must differ in {variant:?}");
        }
    }

    /// The heading ramp reserves hue 0, so no ordinary role may resolve to a
    /// color that reads as the same hue at a similar lightness. The specific
    /// regression: with `build_palette`'s golden-angle loop starting at
    /// attempt 0, `CodeInline` inherited hue 0 and landed 11.5 rgb units from
    /// the tier-3 heading color on dark — indistinguishable, and inline code
    /// sits next to headings constantly. The threshold is the palette's own
    /// pre-existing dark-variant floor (18.4 between two non-heading roles),
    /// rounded down; this asserts only the heading tiers against everything
    /// else, since tightening the whole palette is a separate question.
    #[test]
    fn test_no_ordinary_role_lands_on_the_heading_family() {
        const MIN_RGB_DISTANCE: f64 = 15.0;
        for variant in [Variant::Dark, Variant::Light] {
            let palette = build_palette(variant);
            for (tier, &heading) in palette.iter().take(HEADING_TIERS).enumerate() {
                for (idx, &other) in palette.iter().enumerate().skip(HEADING_TIERS) {
                    let squared = |a: u8, b: u8| (f64::from(a) - f64::from(b)).powi(2);
                    let distance = (squared(heading.r, other.r)
                        + squared(heading.g, other.g)
                        + squared(heading.b, other.b))
                    .sqrt();
                    assert!(
                        distance >= MIN_RGB_DISTANCE,
                        "role slot {idx} ({other:?}) is {distance:.1} rgb units from heading \
                         tier {tier} ({heading:?}) in {variant:?} — they will read as the same color"
                    );
                }
            }
        }
    }

    /// The gate on the accessibility regression this ramp was rebuilt to fix:
    /// a six-rung version put the deepest headings at 3.13:1 (dark) and
    /// 2.73:1 (light), both under WCAG AA's 4.5:1 for normal text, *before*
    /// the terminal applied SGR faint. Backgrounds are the Spike A captured
    /// reference (`\x1b]11;rgb:1a1a/1b1b/2626`) and plain white, matching how
    /// [`variant_from_osc11_reply`] classifies each variant.
    #[test]
    fn test_heading_tiers_clear_wcag_aa_against_the_reference_backgrounds() {
        fn relative_luminance(c: Color) -> f64 {
            let channel = |v: u8| {
                let v = f64::from(v) / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
        }
        fn contrast(a: Color, b: Color) -> f64 {
            let (x, y) = (relative_luminance(a), relative_luminance(b));
            (x.max(y) + 0.05) / (x.min(y) + 0.05)
        }

        const AA_NORMAL_TEXT: f64 = 4.5;
        for (variant, background) in [
            (Variant::Dark, Color::new(0x1a, 0x1b, 0x26)),
            (Variant::Light, Color::new(0xff, 0xff, 0xff)),
        ] {
            // Both the truecolor value and the 256-color cell it snaps to have
            // to clear the bar — a terminal in 256-color mode paints the cell.
            for mode in [ColorMode::Truecolor, ColorMode::Downsample256] {
                let theme = Theme::new(variant, mode);
                for level in 1..=HEADING_LEVELS {
                    let fg = theme
                        .resolve(StyleId::Semantic(Semantic::Heading(level)))
                        .fg
                        .expect("every heading level carries a tier color");
                    let ratio = contrast(fg, background);
                    assert!(
                        ratio >= AA_NORMAL_TEXT,
                        "H{level} in {variant:?}/{mode:?} is {ratio:.2}:1 against {background:?} \
                         — under WCAG AA's {AA_NORMAL_TEXT}:1"
                    );
                }
            }
        }
    }

    /// `Semantic::Heading` is constructible with any `u8`; an out-of-range
    /// level must land on a real ramp rung rather than index past the
    /// palette (`color_for`'s `expect` would panic on a frame, in a viewer
    /// that is mid-render).
    #[test]
    fn test_out_of_range_heading_level_clamps_instead_of_indexing_past_the_ramp() {
        let theme = Theme::new(Variant::Dark, ColorMode::Truecolor);
        let resolve = |level: u8| theme.resolve(StyleId::Semantic(Semantic::Heading(level)));
        assert_eq!(resolve(0), resolve(1));
        assert_eq!(resolve(7), resolve(6));
        assert_eq!(resolve(u8::MAX), resolve(6));
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
