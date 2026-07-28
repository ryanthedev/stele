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
/// The first [`HEADING_RUNGS`] entries are the heading ramp, not one shared
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
const SEMANTIC_ROLES: usize = HEADING_RUNGS + 19;

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

/// How many palette slots the heading ramp occupies — one per level.
///
/// This was 3, with two levels sharing a tier, because six rungs of one hue
/// could not all stay legible *and* stay distinct after 256-color
/// downsampling (see [`HEADING_RAMP`]). That constraint bound only while
/// **color was the signal** for heading depth. It no longer is: every heading
/// now carries a run of `level` block markers, and the count is what a reader
/// reads. The ramp became cohesion — it says "these are all headings" — so
/// two adjacent rungs landing on the same 256-color cell is a cosmetic loss,
/// not an ambiguity, and the ramp can span all six levels.
///
/// Raising this shifts every semantic role after the ramp by three slots, so
/// every colored capture role takes a new color. That is the repaint
/// [`SEMANTIC_ROLES`] warns about, done deliberately here rather than by
/// accident — `test_capture_colors_are_pinned_so_a_new_role_cannot_silently_restyle_code_blocks`
/// is the gate it had to pass through.
const HEADING_RUNGS: usize = 6;

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
    /// A user theme's colours, empty for a built-in. Consulted *ahead* of
    /// `palette` — see [`Theme::resolve_semantic`].
    overrides: ThemeOverrides,
}

impl Theme {
    pub fn new(variant: Variant, color_mode: ColorMode) -> Theme {
        Theme::with_overrides(variant, color_mode, ThemeOverrides::new())
    }

    /// A built-in variant with a user theme laid over it.
    ///
    /// The variant is not decoration here: it is what every role the theme
    /// *didn't* name resolves to. That is what makes a partial theme work at
    /// all, and it is why a theme file declares an appearance — a theme that
    /// sets six colours and inherits twenty-five needs those twenty-five to
    /// come from the right end of the page.
    pub fn with_overrides(
        variant: Variant,
        color_mode: ColorMode,
        overrides: ThemeOverrides,
    ) -> Theme {
        Theme {
            variant,
            color_mode,
            palette: build_palette(variant),
            overrides,
        }
    }

    /// Whether any user override is in effect.
    pub fn is_themed(&self) -> bool {
        !self.overrides.is_empty()
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
        // The override is consulted *first*, and the order is load-bearing
        // rather than stylistic. `Text`, `Strong`, `Emph`, `Strikethrough`,
        // `CodeBlock` and `MathTex` have no palette slot at all — they return
        // `None` from `semantic_role_index` so body prose inherits the
        // terminal's own foreground, which is the right default and stays the
        // default. Consulting the palette first and the override second would
        // work for every role that has a slot and silently do nothing for the
        // six that don't, i.e. it would fail exactly on "let me colour my
        // body text", the request this whole path exists to serve.
        //
        // `apply_mode` runs on the override too, so `NO_COLOR` strips a user
        // colour the same way it strips a built-in one. A theme file is a set
        // of colours; `NO_COLOR` means no colours, and a file cannot be a way
        // around that.
        let fg = self
            .overrides
            .get(semantic)
            .and_then(|c| color::apply_mode(c, self.color_mode))
            .or_else(|| semantic_role_index(semantic).and_then(|idx| self.color_for(idx)));
        // Headings pick their ladder from whether a color actually came back,
        // not from the color *mode*: a role that resolves to `None` here is
        // monochrome in practice however it got that way.
        let attrs = match semantic {
            Semantic::Heading(level) => heading_attrs(level, fg.is_none()),
            other => semantic_attrs(other),
        };
        // The H1 wash. Gated on `fg` rather than on the color mode for the
        // same reason the ladder is: a heading with no color resolved is
        // monochrome in practice, and a background under NO_COLOR would be
        // a color arriving through the back door.
        // Matched on the *clamped* level, not the literal 1 — every other
        // heading consumer here clamps, and an out-of-range `Heading(0)` that
        // resolved to H1's colour but not H1's wash would be a style no level
        // actually has.
        let bg = match semantic {
            Semantic::Heading(level) if fg.is_some() && level.clamp(1, HEADING_LEVELS) == 1 => {
                Some(heading_wash(self.variant))
            }
            _ => None,
        };
        Style { fg, bg, ..attrs }
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
        // The ramp color and nothing else. A marker is a block glyph and a
        // rule band is a line glyph; bold or italic on either is at best
        // invisible and at worst a font-dependent thickening of the fade.
        Semantic::HeadingRung(_) => Style::default(),
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
    // Weight and slope only. The other half of the ladder is *case* — H3/H4
    // render uppercase and H5/H6 title case — which is a text transform, not
    // an SGR attribute, so it cannot live in `Style`. See `heading_case`.
    //
    // This no longer maintains a six-way distinct attribute ladder for
    // monochrome. It doesn't need one: every heading is preceded by a run of
    // `level` block markers, and that count survives `ColorMode::NoColor`
    // untouched (the markers are glyphs; only their color is stripped).
    // Combined with case, the only pair sharing an attribute set *and* a case
    // is H1/H2, which the count separates 1-from-2 — the easiest reading in
    // the set.
    let _ = monochrome;
    match level.clamp(1, HEADING_LEVELS) {
        1 | 2 => bold,
        3 => bold,
        4 | 5 => Style::default(),
        _ => italic,
    }
}

/// Assigns each colored `Semantic` role a stable palette index in
/// `0..SEMANTIC_ROLES`. `None` means "this role never carries color" (its
/// [`semantic_attrs`] styling — bold/dim/italic only — is the whole story).
/// Exhaustive for the same reason as [`semantic_attrs`].
fn semantic_role_index(semantic: Semantic) -> Option<usize> {
    Some(match semantic {
        // Heading levels take one palette slot each (0–5). `heading_rung`
        // clamps, so an out-of-range level lands on a real rung instead of
        // indexing past the ramp. Every index below is `HEADING_RUNGS + n`
        // rather than a bare literal, so growing or shrinking the ramp again
        // moves them together instead of silently overlapping it.
        // Both spellings land on the same rung. `HeadingRung` is the ramp
        // color without `Heading`'s wash or attributes — see its doc in
        // `layout` — so it must not consume a slot of its own.
        Semantic::Heading(level) | Semantic::HeadingRung(level) => heading_rung(level),
        Semantic::CodeInline => HEADING_RUNGS,
        Semantic::Link => HEADING_RUNGS + 1,
        Semantic::ImageAlt => HEADING_RUNGS + 2,
        Semantic::ListMarker => HEADING_RUNGS + 3,
        Semantic::TaskMarker => HEADING_RUNGS + 4,
        Semantic::BlockquoteMarker => HEADING_RUNGS + 5,
        Semantic::AlertTitle(AlertTone::Note) => HEADING_RUNGS + 6,
        Semantic::AlertTitle(AlertTone::Tip) => HEADING_RUNGS + 7,
        Semantic::AlertTitle(AlertTone::Important) => HEADING_RUNGS + 8,
        Semantic::AlertTitle(AlertTone::Warning) => HEADING_RUNGS + 9,
        Semantic::AlertTitle(AlertTone::Caution) => HEADING_RUNGS + 10,
        Semantic::Rule => HEADING_RUNGS + 11,
        Semantic::TableBorder => HEADING_RUNGS + 12,
        Semantic::TableHeader => HEADING_RUNGS + 13,
        Semantic::FootnoteRef => HEADING_RUNGS + 14,
        Semantic::FootnoteLabel => HEADING_RUNGS + 15,
        Semantic::Html => HEADING_RUNGS + 16,
        Semantic::FrontMatter => HEADING_RUNGS + 17,
        Semantic::OverflowIndicator => HEADING_RUNGS + 18,
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

/// WCAG 2.1's contrast floor for normal-size text (1.4.3).
pub const AA_NORMAL_TEXT: f64 = 4.5;

/// WCAG 2.1's contrast floor for non-text content (1.4.11) — the bar a
/// graphical object has to clear to be *perceivable*, as opposed to readable.
pub const AA_NON_TEXT: f64 = 3.0;

/// Whether a role paints structure rather than language, and so answers to
/// [`AA_NON_TEXT`] rather than [`AA_NORMAL_TEXT`].
///
/// The line is drawn at "does a reader read it". A table border, a rule, a
/// blockquote's gutter bar and a list bullet are glyphs that *show* shape;
/// nobody reads them, and holding a `─` to the same bar as a paragraph would
/// force every theme to paint its furniture as loudly as its prose. Alt text,
/// frontmatter and raw HTML go the other way — they look like decoration and
/// are words.
///
/// Exhaustive with no wildcard arm, so a new role has to be classified rather
/// than defaulting into whichever answer happens to be first.
pub fn is_structural(semantic: Semantic) -> bool {
    match semantic {
        Semantic::Rule
        | Semantic::TableBorder
        | Semantic::BlockquoteMarker
        | Semantic::ListMarker
        | Semantic::TaskMarker
        | Semantic::OverflowIndicator => true,
        Semantic::Text
        | Semantic::Heading(_)
        | Semantic::HeadingRung(_)
        | Semantic::Emph
        | Semantic::Strong
        | Semantic::Strikethrough
        | Semantic::CodeInline
        | Semantic::CodeBlock
        | Semantic::Link
        | Semantic::ImageAlt
        | Semantic::MathTex
        | Semantic::AlertTitle(_)
        | Semantic::TableHeader
        | Semantic::FootnoteRef
        | Semantic::FootnoteLabel
        | Semantic::Html
        | Semantic::FrontMatter
        | Semantic::SearchMatch
        | Semantic::SearchCurrent => false,
    }
}

/// The background each variant's colours are measured against.
///
/// A terminal's real background is whatever the user set, and stele only ever
/// learns an approximation of it from the OSC 11 reply. These are the two
/// references every contrast check in this crate uses instead: the Spike A
/// dark reference, and plain white for light. Shared rather than repeated so
/// a built-in's hard assertion and a user theme's warning cannot drift onto
/// different numbers and disagree about the same colour.
pub fn reference_background(variant: Variant) -> Color {
    match variant {
        Variant::Dark => Color::new(0x1a, 0x1b, 0x26),
        Variant::Light => Color::new(0xff, 0xff, 0xff),
    }
}

/// WCAG 2.1 relative luminance.
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

/// WCAG 2.1 contrast ratio between two colours, from 1.0 (identical) to
/// 21.0 (black on white). Symmetric — order does not matter.
pub fn contrast_ratio(a: Color, b: Color) -> f64 {
    let (x, y) = (relative_luminance(a), relative_luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Every name a theme file may use, in the order [`docs/theming.md`] lists
/// them. This is the *public* surface of the style system: a name here is a
/// promise, because it appears in files people write and share.
///
/// Not every `Semantic` is here, and the omissions are deliberate — see
/// [`role_name`], which returns `None` for each and says why.
pub const THEMEABLE_ROLES: &[&str] = &[
    "text",
    "heading1",
    "heading2",
    "heading3",
    "heading4",
    "heading5",
    "heading6",
    "emphasis",
    "strong",
    "strikethrough",
    "code_inline",
    "code_block",
    "link",
    "image_alt",
    "math",
    "list_marker",
    "task_marker",
    "blockquote",
    "alert_note",
    "alert_tip",
    "alert_important",
    "alert_warning",
    "alert_caution",
    "rule",
    "table_border",
    "table_header",
    "footnote_ref",
    "footnote_label",
    "html",
    "front_matter",
    "overflow",
    "search_match",
    "search_current",
];

/// The name a theme file uses for `semantic`, or `None` when the role is
/// deliberately not themeable.
///
/// Exhaustive with no wildcard arm, like every other table over `Semantic`,
/// so a new variant is a compile error until someone decides whether people
/// may name it. That decision is the point of this function: a name in a file
/// someone else wrote is a compatibility promise, and the cheapest time to
/// refuse one is before it ships.
///
/// Two roles are refused today. `HeadingRung` is an internal spelling of a
/// heading's own rung colour used by the depth markers and the ember rule;
/// naming it separately would let a theme desynchronise a heading from its
/// own markers, so it instead *follows* `Heading` through
/// [`canonical_role`]. `Plain` capture text has never carried colour.
pub fn role_name(semantic: Semantic) -> Option<&'static str> {
    Some(match semantic {
        Semantic::Text => "text",
        Semantic::Heading(level) => match level.clamp(1, HEADING_LEVELS) {
            1 => "heading1",
            2 => "heading2",
            3 => "heading3",
            4 => "heading4",
            5 => "heading5",
            _ => "heading6",
        },
        Semantic::Emph => "emphasis",
        Semantic::Strong => "strong",
        Semantic::Strikethrough => "strikethrough",
        Semantic::CodeInline => "code_inline",
        Semantic::CodeBlock => "code_block",
        Semantic::Link => "link",
        Semantic::ImageAlt => "image_alt",
        Semantic::MathTex => "math",
        Semantic::ListMarker => "list_marker",
        Semantic::TaskMarker => "task_marker",
        Semantic::BlockquoteMarker => "blockquote",
        Semantic::AlertTitle(AlertTone::Note) => "alert_note",
        Semantic::AlertTitle(AlertTone::Tip) => "alert_tip",
        Semantic::AlertTitle(AlertTone::Important) => "alert_important",
        Semantic::AlertTitle(AlertTone::Warning) => "alert_warning",
        Semantic::AlertTitle(AlertTone::Caution) => "alert_caution",
        Semantic::Rule => "rule",
        Semantic::TableBorder => "table_border",
        Semantic::TableHeader => "table_header",
        Semantic::FootnoteRef => "footnote_ref",
        Semantic::FootnoteLabel => "footnote_label",
        Semantic::Html => "html",
        Semantic::FrontMatter => "front_matter",
        Semantic::OverflowIndicator => "overflow",
        Semantic::SearchMatch => "search_match",
        Semantic::SearchCurrent => "search_current",
        // Follows `Heading` rather than carrying a name of its own.
        Semantic::HeadingRung(_) => return None,
    })
}

/// The role a theme file's `name` refers to, or `None` if no role does.
///
/// The inverse of [`role_name`] over [`THEMEABLE_ROLES`], and
/// `test_every_themeable_role_name_round_trips` is what holds the two
/// together — a table that disagreed with its own inverse would hand a
/// theme's colour to the wrong role, which is worse than refusing the name.
pub fn semantic_from_name(name: &str) -> Option<Semantic> {
    Some(match name {
        "text" => Semantic::Text,
        "heading1" => Semantic::Heading(1),
        "heading2" => Semantic::Heading(2),
        "heading3" => Semantic::Heading(3),
        "heading4" => Semantic::Heading(4),
        "heading5" => Semantic::Heading(5),
        "heading6" => Semantic::Heading(6),
        "emphasis" => Semantic::Emph,
        "strong" => Semantic::Strong,
        "strikethrough" => Semantic::Strikethrough,
        "code_inline" => Semantic::CodeInline,
        "code_block" => Semantic::CodeBlock,
        "link" => Semantic::Link,
        "image_alt" => Semantic::ImageAlt,
        "math" => Semantic::MathTex,
        "list_marker" => Semantic::ListMarker,
        "task_marker" => Semantic::TaskMarker,
        "blockquote" => Semantic::BlockquoteMarker,
        "alert_note" => Semantic::AlertTitle(AlertTone::Note),
        "alert_tip" => Semantic::AlertTitle(AlertTone::Tip),
        "alert_important" => Semantic::AlertTitle(AlertTone::Important),
        "alert_warning" => Semantic::AlertTitle(AlertTone::Warning),
        "alert_caution" => Semantic::AlertTitle(AlertTone::Caution),
        "rule" => Semantic::Rule,
        "table_border" => Semantic::TableBorder,
        "table_header" => Semantic::TableHeader,
        "footnote_ref" => Semantic::FootnoteRef,
        "footnote_label" => Semantic::FootnoteLabel,
        "html" => Semantic::Html,
        "front_matter" => Semantic::FrontMatter,
        "overflow" => Semantic::OverflowIndicator,
        "search_match" => Semantic::SearchMatch,
        "search_current" => Semantic::SearchCurrent,
        _ => return None,
    })
}

/// The single spelling of a role for override lookup.
///
/// Two normalisations, and both exist so a lookup cannot miss a colour the
/// user did set. Heading levels clamp, because `Heading(0)` and `Heading(9)`
/// resolve to a real rung's colour everywhere else in this file and an
/// override keyed on the literal level would be invisible to them. And
/// `HeadingRung(n)` folds onto `Heading(n)`, so theming `heading2` moves that
/// heading's depth markers and its ember rule band with it — a heading whose
/// markers stayed the built-in colour would look like a rendering fault.
fn canonical_role(semantic: Semantic) -> Semantic {
    match semantic {
        Semantic::Heading(level) | Semantic::HeadingRung(level) => {
            Semantic::Heading(level.clamp(1, HEADING_LEVELS))
        }
        other => other,
    }
}

/// A theme file's colours: sparse by construction, because *absent* is the
/// whole design. A role nobody named is not in here, so it falls through to
/// the generated palette and a five-line theme is a complete theme.
///
/// Keyed by [`canonical_role`], never by the raw `Semantic`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeOverrides {
    by_role: std::collections::HashMap<Semantic, Color>,
}

impl ThemeOverrides {
    pub fn new() -> ThemeOverrides {
        ThemeOverrides::default()
    }

    /// The colour set for `semantic`, before [`ColorMode`] is applied —
    /// `Theme::resolve` owns that step, so `NO_COLOR` strips an override
    /// exactly as it strips a built-in colour.
    pub fn get(&self, semantic: Semantic) -> Option<Color> {
        self.by_role.get(&canonical_role(semantic)).copied()
    }

    pub fn insert(&mut self, semantic: Semantic, color: Color) -> Option<Color> {
        self.by_role.insert(canonical_role(semantic), color)
    }

    pub fn is_empty(&self) -> bool {
        self.by_role.is_empty()
    }

    pub fn len(&self) -> usize {
        self.by_role.len()
    }

    /// Every override, for a caller that must inspect them all — the lint
    /// pass that checks a user theme's contrast, and the tests.
    pub fn iter(&self) -> impl Iterator<Item = (Semantic, Color)> + '_ {
        self.by_role.iter().map(|(k, v)| (*k, *v))
    }
}

impl FromIterator<(Semantic, Color)> for ThemeOverrides {
    fn from_iter<I: IntoIterator<Item = (Semantic, Color)>>(iter: I) -> ThemeOverrides {
        let mut overrides = ThemeOverrides::new();
        for (semantic, color) in iter {
            overrides.insert(semantic, color);
        }
        overrides
    }
}

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

/// The heading ramp's hue now lives per-variant in [`HEADING_RAMP`], because
/// dark and light no longer share one: dark is Tron cyan (0.52) and light is
/// newsprint ink (0.075). It used to be a single constant at hue 0.0, which
/// [`candidate_color`]'s attempt 0 also lands on.
///
/// [`build_palette`] still starts its golden-angle loop at attempt 1. The
/// original reason has lapsed on dark — headings moved off hue 0, so attempt
/// 0 no longer shadows them there — but light's ink sits at 0.075, close
/// enough to hue 0 that the skip keeps earning its place. It also costs
/// nothing: the golden angle never revisits hue 0.
/// `test_no_ordinary_role_lands_on_the_heading_family` is what actually
/// enforces the separation, in both variants.
const PALETTE_FIRST_ATTEMPT: usize = 1;

/// Saturation and per-level lightness for the heading ramp, loudest first,
/// where "loud" means *furthest from the background*: lightness descends on
/// dark, ascends on light.
///
/// Six rungs, one per level. The earlier three-tier table existed because a
/// six-rung ramp put H6 at 3.13:1 and H5 at 4.20:1 on dark — under WCAG AA's
/// 4.5:1 for text — and the light variant's H6 at 2.73:1. These rungs are
/// chosen to clear AA anyway: the shallowest measures 7.53:1 (light H6) and
/// nothing falls below it. The 256-color collisions the old table could not
/// tolerate are now allowed, because the block-marker count carries the
/// level; see [`HEADING_RUNGS`].
///
/// `test_heading_rungs_clear_wcag_aa_against_the_reference_backgrounds` is
/// what stops a "nicer" value from quietly going under AA.
const HEADING_RAMP: [(f64, f64, [f64; HEADING_RUNGS]); 2] = [
    // Dark — Tron: electric cyan igniting out of near-black. Hue 0.52 is
    // ~187°. Measured against `#1a1b26`: 13.35 → 10.57:1.
    (0.52, 0.95, [0.80, 0.74, 0.69, 0.64, 0.59, 0.55]),
    // Light — newsprint: warm, nearly neutral ink laid on paper. The ramp
    // runs the other way because on paper "receding" means approaching the
    // page, not approaching black. Against `#ffffff`: 16.01 → 7.53:1.
    (0.075, 0.08, [0.13, 0.17, 0.21, 0.25, 0.29, 0.33]),
];

/// Which rung of the color ramp a heading level draws from — one each, now
/// that the ramp spans all six levels. Clamps, so an out-of-range level lands
/// on a real rung instead of indexing past the ramp.
fn heading_rung(level: u8) -> usize {
    usize::from(level.clamp(1, HEADING_LEVELS) - 1)
}

/// The band laid behind the document title's line.
///
/// Deliberately close to the page — 1.23:1 against the dark reference and
/// 1.13:1 against the light one. It is not trying to be legible on its own;
/// it is trying to be *felt*, and anything louder competes with the heading
/// text sitting on it. Because it is a background rather than a mark, no
/// contrast floor applies to the band itself — what must clear AA is the
/// heading text *against* it, which
/// `test_the_h1_wash_never_costs_the_title_its_contrast` checks directly
/// rather than assuming the page-background measurement carries over.
fn heading_wash(variant: Variant) -> Color {
    let (hue, saturation, _) = match variant {
        Variant::Dark => HEADING_RAMP[0],
        Variant::Light => HEADING_RAMP[1],
    };
    let lightness = match variant {
        Variant::Dark => 0.115,
        Variant::Light => 0.945,
    };
    color::hsl_to_rgb(hue, saturation, lightness)
}

/// One rung of the heading color ramp.
fn heading_ramp_color(rung: usize, variant: Variant) -> Color {
    let (hue, saturation, lightnesses) = match variant {
        Variant::Dark => HEADING_RAMP[0],
        Variant::Light => HEADING_RAMP[1],
    };
    color::hsl_to_rgb(hue, saturation, lightnesses[rung.min(HEADING_RUNGS - 1)])
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

    // Slots 0..HEADING_RUNGS first, so the ramp keeps one hue instead of
    // drawing golden-angle-separated hues — a rainbow of heading levels would
    // read as unrelated roles rather than one hierarchy.
    //
    // These rungs are deliberately NOT held to the 256-downsample
    // distinctness rule the other roles pass through. Six rungs of one hue do
    // collide once quantized to the cube's 6 steps per channel (dark loses
    // H4/H5; light loses H1/H2 and H3/H4), and this used to be a hard error.
    // It is allowed now because the block-marker count carries heading depth
    // — see [`HEADING_RUNGS`]. Their downsampled cells are still *recorded*,
    // so no ordinary role may land on one: a heading looking like a heading is
    // cosmetic, a `CodeInline` looking like a heading is not.
    for rung in 0..HEADING_RUNGS {
        let candidate = heading_ramp_color(rung, variant);
        used_downsampled.insert(color::downsample_256(candidate));
        palette.push(candidate);
    }

    // Attempt 0 sits at hue 0, near enough to light's newsprint ink (0.075)
    // that the 256-cell filter below cannot separate them: its color survives
    // as a distinct *cell* while looking like a heading rung. Skipping it
    // costs nothing — the golden angle never revisits hue 0 — and it keeps
    // `CodeInline`, the role that inherits this slot, off the heading family.
    // See [`PALETTE_FIRST_ATTEMPT`] for why the skip outlived dark's move to
    // cyan.
    let mut attempt = PALETTE_FIRST_ATTEMPT;
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

    use layout::{AlertTone, HeadingCase, Semantic, heading_case};

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
            // Heading rungs are exempt from mutual distinctness: six rungs of
            // one hue genuinely cannot all survive the 256-cube's 6 steps per
            // channel (dark loses H4/H5, light loses H1/H2 and H3/H4), and
            // that is accepted because the block-marker count carries depth.
            // Everything else must still be distinct from everything else
            // *including* from every heading rung — a `CodeInline` that reads
            // as a heading is a real ambiguity, unlike H4 reading as H5.
            let mut seen = HashSet::new();
            let down = |c| {
                color::apply_mode(c, ColorMode::Downsample256)
                    .expect("Downsample256 always yields a color")
            };
            for (idx, &truecolor) in palette.iter().enumerate().skip(HEADING_RUNGS) {
                assert!(
                    seen.insert(down(truecolor)),
                    "role index {idx} collided with an earlier role after 256-color downsample ({variant:?}): {:?}",
                    down(truecolor)
                );
            }
            for (rung, &truecolor) in palette.iter().enumerate().take(HEADING_RUNGS) {
                assert!(
                    !seen.contains(&down(truecolor)),
                    "heading rung {rung} downsamples onto a cell an ordinary role already took \
                     ({variant:?}): {:?}",
                    down(truecolor)
                );
            }
            assert_eq!(seen.len(), role_count() - HEADING_RUNGS);

            // Cross-check the same invariant through the public `resolve`
            // entry point (not just the internal palette function): every
            // colored non-heading role must still land on one of the `seen`
            // colors, with no two distinct roles resolving to the same one.
            // Headings are excluded on both sides — they are the exempt set
            // above, and folding them in would just re-assert the collision
            // this test now permits.
            let theme = Theme::new(variant, ColorMode::Downsample256);
            let mut resolved = HashSet::new();
            for semantic in all_semantics() {
                if matches!(semantic, Semantic::Heading(_)) {
                    continue;
                }
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
                // Deliberately absent from the list below. `HeadingRung(n)`
                // resolves to the same palette slot as `Heading(n)` on
                // purpose — it is that rung's color minus the wash — so
                // feeding it to a distinctness test would assert it must
                // differ from the thing it is defined to match.
                Semantic::HeadingRung(_) => {}
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
    /// Pinned in both variants and at both ends of the capture block, so an
    /// insertion anywhere inside it fails here. If a deliberate palette
    /// change ever makes these wrong, the fix is to re-record them in the
    /// same commit as the change — not to loosen the assertion.
    ///
    /// **Re-recorded once, deliberately.** Growing the heading ramp from
    /// three tiers to six rungs ([`HEADING_RUNGS`]) inserted three slots
    /// ahead of the capture block and moved every capture color — the exact
    /// failure mode this test was written to catch. It caught it. The values
    /// below are the post-change palette, and the change was wanted: the
    /// ramp legitimately lives at the front of the palette, and the
    /// alternative (scattering three heading rungs out at
    /// `TRAILING_ROLE_BASE`) would have split one ramp across two disjoint
    /// index ranges to preserve colors that no user has seen, since the
    /// default theme changed in the same commit. A *role* being added still
    /// must not do this; that is still the rule.
    #[test]
    fn test_capture_colors_are_pinned_so_a_new_role_cannot_silently_restyle_code_blocks() {
        let expected = [
            (Variant::Dark, Capture::Keyword, Color::new(134, 243, 175)),
            (Variant::Dark, Capture::Comment, Color::new(115, 222, 190)),
            (Variant::Dark, Capture::String, Color::new(222, 33, 81)),
            (Variant::Light, Capture::Keyword, Color::new(64, 201, 69)),
            (Variant::Light, Capture::Comment, Color::new(201, 195, 64)),
            (Variant::Light, Capture::String, Color::new(154, 29, 35)),
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
            let mut rungs = HashSet::new();
            for level in 1..=HEADING_LEVELS {
                let style = theme.resolve(StyleId::Semantic(Semantic::Heading(level)));
                let fg = style.fg.expect("every heading level carries a rung color");
                rungs.insert(fg);
                // Identity is (color, attributes, case) — the ramp alone no
                // longer has to separate levels, and neither do attributes.
                let identity = (style, heading_case(level));
                assert!(
                    !seen.contains(&identity),
                    "H{level} is indistinguishable from a shallower level in {variant:?}: \
                     {identity:?}"
                );
                assert!(
                    !style.dim,
                    "H{level} carries SGR faint on top of a ramp color in {variant:?} — \
                     that is the combination that fell under WCAG AA"
                );
                seen.push(identity);
            }
            assert_eq!(
                rungs.len(),
                HEADING_RUNGS,
                "the six levels should draw on exactly {HEADING_RUNGS} rung colors in truecolor"
            );

            // With color stripped, attributes and case are all that is left in
            // the *style*. They no longer separate all six on their own, and
            // are not required to: every heading is preceded by a run of
            // `level` block markers, which are glyphs and survive NoColor. So
            // this pins exactly how far the style alone gets, and names the
            // pair that leans on the count — if that set ever grows, this
            // fails and the growth has to be argued for.
            let no_color = Theme::new(variant, ColorMode::NoColor);
            let mut collapsed = Vec::new();
            let mut bare = Vec::new();
            for level in 1..=HEADING_LEVELS {
                let style = no_color.resolve(StyleId::Semantic(Semantic::Heading(level)));
                assert_eq!(style.fg, None);
                let identity = (style, heading_case(level));
                if bare.contains(&identity) {
                    collapsed.push(level);
                }
                bare.push(identity);
            }
            assert_eq!(
                collapsed,
                vec![2],
                "under NoColor only H2 may share H1's style-and-case (the marker count, 1 vs 2, \
                 is what separates them); in {variant:?} the collapsed set was {collapsed:?}"
            );
        }
    }

    /// The ramp is a hierarchy, so its contrast against the background must
    /// fall monotonically with depth — H4 may not out-shout H3. Now that the
    /// ramp has one rung per level, this holds between *every adjacent pair*
    /// rather than only across tier boundaries. Asserted through resolved
    /// colors rather than the generator, so it also covers the level → rung
    /// mapping.
    #[test]
    fn test_heading_ramp_is_monotone_in_contrast() {
        for variant in [Variant::Dark, Variant::Light] {
            let theme = Theme::new(variant, ColorMode::Truecolor);
            let luminance = |level: u8| -> f64 {
                let fg = theme
                    .resolve(StyleId::Semantic(Semantic::Heading(level)))
                    .fg
                    .expect("every heading level carries a rung color");
                0.2126 * f64::from(fg.r) + 0.7152 * f64::from(fg.g) + 0.0722 * f64::from(fg.b)
            };
            for level in 2..=HEADING_LEVELS {
                let (deeper, shallower) = (luminance(level), luminance(level - 1));
                match variant {
                    // Dark background: louder = brighter, so luminance falls.
                    Variant::Dark => assert!(
                        deeper < shallower,
                        "H{level} is brighter than H{} on dark ({deeper} >= {shallower})",
                        level - 1
                    ),
                    // Light background: louder = darker, so luminance rises.
                    Variant::Light => assert!(
                        deeper > shallower,
                        "H{level} is darker than H{} on light ({deeper} <= {shallower})",
                        level - 1
                    ),
                }
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
        // H1 bold · H2 bold · H3 bold (rendered uppercase) · H4 plain
        // (uppercase) · H5 plain (title case) · H6 italic (title case).
        // Weight and slope only; `case` below carries the rest, and the
        // marker count carries what neither does.
        let ladder = [bold, bold, bold, Style::default(), Style::default(), italic];
        let case = [
            HeadingCase::AsWritten,
            HeadingCase::AsWritten,
            HeadingCase::Upper,
            HeadingCase::Upper,
            HeadingCase::Title,
            HeadingCase::Title,
        ];
        // Stripping color changes nothing about the attribute ladder — there
        // is no separate monochrome ladder any more, because the markers do
        // not need one.
        let (colored, monochrome) = (ladder, ladder);

        for variant in [Variant::Dark, Variant::Light] {
            let theme = Theme::new(variant, ColorMode::Truecolor);
            let no_color = Theme::new(variant, ColorMode::NoColor);
            for level in 1..=HEADING_LEVELS {
                let idx = usize::from(level - 1);
                let got = theme.resolve(StyleId::Semantic(Semantic::Heading(level)));
                assert_eq!(
                    Style {
                        fg: None,
                        bg: None,
                        ..got
                    },
                    colored[idx],
                    "H{level}'s colored attributes in {variant:?}"
                );
                // Exactly one level carries the wash, and it is the title.
                assert_eq!(
                    got.bg.is_some(),
                    level == 1,
                    "H{level} background in {variant:?}: only H1 wears the wash"
                );
                assert_eq!(
                    no_color.resolve(StyleId::Semantic(Semantic::Heading(level))),
                    monochrome[idx],
                    "H{level}'s monochrome attributes in {variant:?}"
                );
                assert_eq!(heading_case(level), case[idx], "H{level}'s case transform");
            }

            // ...and that every level now owns its own rung, rather than
            // pairing up. The old ladder asserted 1-2, 3-4, 5-6 shared a
            // color; the inverse is the invariant now.
            let fg = |level: u8| {
                theme
                    .resolve(StyleId::Semantic(Semantic::Heading(level)))
                    .fg
                    .expect("every heading level carries a rung color")
            };
            for level in 2..=HEADING_LEVELS {
                assert_ne!(
                    fg(level - 1),
                    fg(level),
                    "H{} and H{level} must sit on different rungs in {variant:?}",
                    level - 1
                );
            }
        }
    }

    /// `HeadingRung(n)` is the ramp's rung `n` with nothing else attached —
    /// same color as `Heading(n)`, no background, no attributes. Heading
    /// *decorations* (the depth markers, the ember rule's bands) index the
    /// ramp by position rather than by their heading's level, so they all
    /// reach rung 1; taking that rung through `Heading(1)` handed them the H1
    /// wash as well. The two halves of this test are what make the separation
    /// real: same fg keeps the fade, absent bg keeps the band on H1's line.
    #[test]
    fn test_a_heading_rung_is_the_ramp_color_without_the_wash() {
        for variant in [Variant::Dark, Variant::Light] {
            let theme = Theme::new(variant, ColorMode::Truecolor);
            for rung in 1..=HEADING_LEVELS {
                let decoration = theme.resolve(StyleId::Semantic(Semantic::HeadingRung(rung)));
                let heading = theme.resolve(StyleId::Semantic(Semantic::Heading(rung)));
                assert_eq!(
                    decoration.fg, heading.fg,
                    "HeadingRung({rung}) must share Heading({rung})'s rung in {variant:?}, \
                     or the run fade breaks and it costs a palette slot"
                );
                assert!(
                    decoration.bg.is_none(),
                    "HeadingRung({rung}) carries a background in {variant:?} — that is the \
                     wash leaking onto markers and rule bands: {decoration:?}"
                );
            }
            assert!(
                theme
                    .resolve(StyleId::Semantic(Semantic::Heading(1)))
                    .bg
                    .is_some(),
                "Heading(1) lost its wash in {variant:?}, so this test proves nothing"
            );
        }
    }

    /// The H1 wash sits *behind* the title, so the contrast that matters is
    /// title-against-band, not title-against-page. Those are different
    /// numbers, and only one of them is what a reader has to read — a wash
    /// nudged toward the text's own lightness would pass every other check in
    /// this file while making the title harder to read than before it existed.
    #[test]
    fn test_the_h1_wash_never_costs_the_title_its_contrast() {
        for variant in [Variant::Dark, Variant::Light] {
            // Both the truecolor values and the 256-color cells they snap to:
            // a terminal in 256-color mode paints the cells, not the ideals.
            for mode in [ColorMode::Truecolor, ColorMode::Downsample256] {
                let theme = Theme::new(variant, mode);
                let h1 = theme.resolve(StyleId::Semantic(Semantic::Heading(1)));
                let (fg, bg) = (
                    h1.fg.expect("H1 carries a rung color"),
                    h1.bg.expect("H1 carries the wash"),
                );
                let ratio = contrast_ratio(fg, bg);
                assert!(
                    ratio >= AA_NORMAL_TEXT,
                    "H1 title on its own wash is {ratio:.2}:1 in {variant:?}/{mode:?} — \
                     under AA. The wash has to stay near the page, not drift toward the text."
                );
            }
        }
    }

    /// The heading ramp reserves its hue, so no ordinary role may resolve to
    /// a color that reads as the same hue at a similar lightness. The specific
    /// regression: with `build_palette`'s golden-angle loop starting at
    /// attempt 0, `CodeInline` inherited hue 0 and landed 11.5 rgb units from
    /// the tier-3 heading color on dark — indistinguishable, and inline code
    /// sits next to headings constantly. The threshold is the palette's own
    /// pre-existing dark-variant floor (18.4 between two non-heading roles),
    /// rounded down; this asserts only the heading rungs against everything
    /// else, since tightening the whole palette is a separate question.
    ///
    /// This matters *more* now, not less. Heading rungs are allowed to
    /// collide with each other after 256-downsampling, so `build_palette` no
    /// longer asserts their distinctness — but their cells are still recorded
    /// precisely so an ordinary role cannot take one, and this is what proves
    /// the recording works in both variants.
    #[test]
    fn test_no_ordinary_role_lands_on_the_heading_family() {
        const MIN_RGB_DISTANCE: f64 = 15.0;
        for variant in [Variant::Dark, Variant::Light] {
            let palette = build_palette(variant);
            for (rung, &heading) in palette.iter().take(HEADING_RUNGS).enumerate() {
                for (idx, &other) in palette.iter().enumerate().skip(HEADING_RUNGS) {
                    let squared = |a: u8, b: u8| (f64::from(a) - f64::from(b)).powi(2);
                    let distance = (squared(heading.r, other.r)
                        + squared(heading.g, other.g)
                        + squared(heading.b, other.b))
                    .sqrt();
                    assert!(
                        distance >= MIN_RGB_DISTANCE,
                        "role slot {idx} ({other:?}) is {distance:.1} rgb units from heading \
                         rung {rung} ({heading:?}) in {variant:?} — they will read as the same color"
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
    fn test_heading_rungs_clear_wcag_aa_against_the_reference_backgrounds() {
        // `contrast_ratio` and `reference_background` rather than a copy
        // local to this test: a user theme's low-contrast *warning* runs the
        // same two functions, so a colour cannot be judged legible by one and
        // illegible by the other.
        for variant in [Variant::Dark, Variant::Light] {
            let background = reference_background(variant);
            // Both the truecolor value and the 256-color cell it snaps to have
            // to clear the bar — a terminal in 256-color mode paints the cell.
            for mode in [ColorMode::Truecolor, ColorMode::Downsample256] {
                let theme = Theme::new(variant, mode);
                for level in 1..=HEADING_LEVELS {
                    let fg = theme
                        .resolve(StyleId::Semantic(Semantic::Heading(level)))
                        .fg
                        .expect("every heading level carries a rung color");
                    let ratio = contrast_ratio(fg, background);
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

    /// The name table and its inverse must agree on every role. They are two
    /// hand-written matches, and a disagreement would not fail to compile —
    /// it would hand a theme's colour to the wrong role, or drop it. Driven
    /// off `THEMEABLE_ROLES` rather than a list written here, so the public
    /// constant is what is actually under test.
    #[test]
    fn test_every_themeable_role_name_round_trips() {
        for name in THEMEABLE_ROLES {
            let semantic = semantic_from_name(name).unwrap_or_else(|| {
                panic!("THEMEABLE_ROLES lists {name:?}, which resolves to no role")
            });
            assert_eq!(
                role_name(semantic),
                Some(*name),
                "{name:?} resolves to {semantic:?}, which names itself differently"
            );
        }
    }

    /// The other direction: a role that has a name must be reachable by it.
    /// Without this, a role could be silently un-themeable — `role_name`
    /// would answer, `THEMEABLE_ROLES` would omit it, and no file could ever
    /// name it because the docs and the parser both read the constant.
    #[test]
    fn test_every_named_role_appears_in_the_public_list() {
        for semantic in all_semantics() {
            let Some(name) = role_name(semantic) else {
                continue;
            };
            assert!(
                THEMEABLE_ROLES.contains(&name),
                "{semantic:?} names itself {name:?}, which THEMEABLE_ROLES omits — \
                 nothing could ever set it"
            );
        }
    }

    /// An out-of-range heading level names a real rung rather than panicking
    /// or inventing a name no file could contain. `Heading(0)` and
    /// `Heading(9)` reach `resolve` from a malformed document, and every
    /// other table in this file clamps them.
    #[test]
    fn test_an_out_of_range_heading_still_names_a_real_role() {
        for level in [0u8, 7, 9, u8::MAX] {
            let name = role_name(Semantic::Heading(level))
                .unwrap_or_else(|| panic!("Heading({level}) has no name"));
            assert!(
                THEMEABLE_ROLES.contains(&name),
                "Heading({level}) named itself {name:?}, which is not a themeable role"
            );
        }
        assert_eq!(
            role_name(Semantic::Heading(0)),
            role_name(Semantic::Heading(1))
        );
        assert_eq!(
            role_name(Semantic::Heading(9)),
            role_name(Semantic::Heading(6))
        );
    }

    /// The requirement this whole seam exists for. `Text` has no palette slot
    /// — it resolves to `None` so body prose inherits the terminal's own
    /// foreground — and it must still be settable from a theme.
    #[test]
    fn test_an_override_colours_a_role_that_has_no_palette_slot() {
        let plain = Theme::new(Variant::Dark, ColorMode::Truecolor);
        assert_eq!(
            plain.resolve(StyleId::Semantic(Semantic::Text)).fg,
            None,
            "body text must inherit the terminal foreground with no theme"
        );

        let wanted = Color::new(0x9a, 0xbc, 0xde);
        let themed = Theme::with_overrides(
            Variant::Dark,
            ColorMode::Truecolor,
            [(Semantic::Text, wanted)].into_iter().collect(),
        );
        assert_eq!(
            themed.resolve(StyleId::Semantic(Semantic::Text)).fg,
            Some(wanted),
            "an override must reach a role the palette never colours"
        );
    }

    /// An empty overlay must change nothing at all. This is what lets the
    /// overlay ship without touching the no-config path: if it is inert when
    /// unused, every existing invariant test is still testing the shipped
    /// rendering.
    #[test]
    fn test_an_empty_overlay_resolves_identically_to_the_built_in() {
        for variant in [Variant::Dark, Variant::Light] {
            for mode in [
                ColorMode::Truecolor,
                ColorMode::Downsample256,
                ColorMode::NoColor,
            ] {
                let built_in = Theme::new(variant, mode);
                let overlaid = Theme::with_overrides(variant, mode, ThemeOverrides::new());
                for semantic in all_semantics() {
                    assert_eq!(
                        built_in.resolve(StyleId::Semantic(semantic)),
                        overlaid.resolve(StyleId::Semantic(semantic)),
                        "{semantic:?} differs under an empty overlay in {variant:?}/{mode:?}"
                    );
                }
            }
        }
    }

    /// `NO_COLOR` means no colours, and a theme file is a set of colours — so
    /// a file cannot be a way around it. Every role, including the six with
    /// no palette slot, must still come back with no foreground.
    #[test]
    fn test_no_color_strips_a_user_theme_as_thoroughly_as_a_built_in() {
        let every_role: ThemeOverrides = all_semantics()
            .into_iter()
            .map(|s| (s, Color::new(0xff, 0x00, 0xff)))
            .collect();
        let theme = Theme::with_overrides(Variant::Dark, ColorMode::NoColor, every_role);
        for semantic in all_semantics() {
            let style = theme.resolve(StyleId::Semantic(semantic));
            assert_eq!(
                style.fg, None,
                "{semantic:?} carries a user colour under NO_COLOR"
            );
            assert_eq!(
                style.bg, None,
                "{semantic:?} carries a background under NO_COLOR"
            );
        }
    }

    /// A heading's depth markers and its ember rule are painted through
    /// `HeadingRung`, which has no name of its own. Theming `heading2` must
    /// move them too — a heading whose title changed colour while its markers
    /// stayed built-in would read as a rendering fault, not a theme.
    #[test]
    fn test_theming_a_heading_moves_its_markers_and_rule_with_it() {
        let wanted = Color::new(0x12, 0x34, 0x56);
        let theme = Theme::with_overrides(
            Variant::Dark,
            ColorMode::Truecolor,
            [(Semantic::Heading(2), wanted)].into_iter().collect(),
        );
        assert_eq!(
            theme.resolve(StyleId::Semantic(Semantic::Heading(2))).fg,
            Some(wanted)
        );
        assert_eq!(
            theme
                .resolve(StyleId::Semantic(Semantic::HeadingRung(2)))
                .fg,
            Some(wanted),
            "the rung a marker and a rule band paint with must follow its heading"
        );
        // And an untouched level must not move.
        assert_ne!(
            theme
                .resolve(StyleId::Semantic(Semantic::HeadingRung(3)))
                .fg,
            Some(wanted),
            "theming heading2 must not repaint heading3's rung"
        );
    }

    /// `HeadingRung` must stay unnameable. If a file could set it, a theme
    /// could point a heading and its own markers at different colours.
    #[test]
    fn test_the_internal_heading_rung_role_is_not_themeable() {
        for level in 1..=HEADING_LEVELS {
            assert_eq!(
                role_name(Semantic::HeadingRung(level)),
                None,
                "HeadingRung({level}) must not be nameable from a theme file"
            );
        }
        assert!(!THEMEABLE_ROLES.contains(&"heading_rung"));
    }
}
