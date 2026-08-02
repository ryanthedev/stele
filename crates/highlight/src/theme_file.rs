//! Parsing a user-authored theme file into [`ThemeOverrides`].
//!
//! A theme is one self-contained TOML file, because a theme is meant to be
//! *shared*: you send someone a file, they save it, it works. That shape is
//! what decides most of what follows — every role is optional so a five-line
//! theme is a whole theme, and the file declares which built-in it sits on
//! top of so the roles it doesn't name come from the right end of the page.
//!
//! ```toml
//! name = "Ember"
//! appearance = "dark"
//!
//! [colors]
//! text = "#c8d0e0"
//! heading1 = "#ff8844"
//! ```
//!
//! **This module does no I/O.** It takes a `&str` and reads no environment,
//! which is what keeps it testable against hostile input without a fixture
//! directory; `crates/stele` owns finding the file and handing over its
//! contents.
//!
//! **Its input is untrusted.** A theme file arrives the way a document does —
//! downloaded from someone, possibly hostile — and it is the second such path
//! into the renderer. Two rules follow. Parsing goes through a real TOML
//! parser rather than any hand-rolled splitting, and every borrowed string
//! that reaches a warning's `Display` is passed through
//! [`crate::strip_display_hazards`] first, so a `name` field full of escape
//! sequences cannot repaint the status row it gets reported in.
//!
//! **Nothing here fails on a bad value.** A malformed colour costs you that
//! colour and nothing else: the role falls through to the built-in, a warning
//! records it, and the rest of the file still applies. The single hard error
//! is TOML that does not parse at all, where there is nothing to salvage.

use std::collections::BTreeSet;
use std::fmt;

use layout::{Chrome, Padding};

use crate::color::Color;
use crate::hazard::strip_display_hazards;
use crate::role;
use crate::theme::{
    AA_NON_TEXT, AA_NORMAL_TEXT, BACKGROUND_ROLES, SYNTAX_ROLES, THEMEABLE_ROLES, ThemeOverrides,
    Variant, background_from_name, capture_from_name, capture_name, contrast_ratio, is_structural,
    reference_background, role_name, semantic_from_name,
};

/// The largest theme file worth reading. A theme is a few dozen short lines;
/// anything past this is not one, and refusing early keeps a pathological
/// file from becoming a pathological parse.
const MAX_THEME_BYTES: usize = 256 * 1024;

/// A parsed theme file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeFile {
    /// The theme's own name, for the status line. Already stripped of
    /// display hazards — it came from an untrusted file.
    pub name: String,
    /// Which built-in variant this theme lays over, and therefore what every
    /// role it does not name resolves to.
    pub appearance: Variant,
    /// The colours the file actually set.
    pub overrides: ThemeOverrides,
    /// The `[layout]` table: padding, gutter, reading line. Geometry rather
    /// than colour, and the one part of a theme file that is not a palette.
    ///
    /// It is here because it is the same kind of promise the colours are — you
    /// send someone a file and their page looks like yours — and because the
    /// alternative, a second config file for four integers, is a second thing
    /// to find, name and document. Every key is optional; the default is the
    /// frame stele drew before any of this existed.
    pub chrome: Chrome,
}

/// The one way parsing fails outright: nothing could be salvaged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeError {
    /// The file is not TOML. Carries the parser's own message, sanitized.
    Syntax(String),
    /// The file is larger than [`MAX_THEME_BYTES`].
    TooLarge { bytes: usize },
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThemeError::Syntax(message) => write!(f, "not a valid theme file: {message}"),
            ThemeError::TooLarge { bytes } => write!(
                f,
                "theme file is {bytes} bytes, over the {MAX_THEME_BYTES}-byte limit"
            ),
        }
    }
}

impl std::error::Error for ThemeError {}

/// Something wrong with a theme that did not stop it loading.
///
/// Every variant is recoverable by construction — the theme still applies,
/// minus whatever this names. They are reported rather than raised because
/// the alternative, refusing a whole theme over one bad hex, costs the user
/// every colour they got right.
#[derive(Debug, Clone, PartialEq)]
pub enum ThemeWarning {
    /// A key under `[colors]` that names no role.
    UnknownRole {
        name: String,
        /// The closest real role name, when one is close enough to suggest.
        suggestion: Option<&'static str>,
    },
    /// A role whose value is not a colour this can read.
    BadColor { role: String, value: String },
    /// A value of the wrong TOML type.
    WrongType { key: String, expected: &'static str },
    /// No `appearance` key. Defaulted to dark.
    MissingAppearance,
    /// An `appearance` that is neither `dark` nor `light`.
    UnknownAppearance { value: String },
    /// A themed colour that does not clear WCAG AA against the appearance's
    /// reference background. Honoured anyway — the user asked for it.
    ///
    /// `floor` is the bar this role answers to: 4.5:1 for anything a reader
    /// reads, 3:1 for structural glyphs. See `highlight::theme::is_structural`.
    LowContrast {
        role: &'static str,
        ratio: f64,
        floor: f64,
    },
    /// Two themed roles that land on the same cell once downsampled to the
    /// 256-colour cube, and so are indistinguishable in a 256-colour
    /// terminal.
    Downsample256Collision {
        first: &'static str,
        second: &'static str,
    },
    /// A `[layout]` key whose value is a number outside the range it may take.
    /// Clamped to the nearest end and honoured — the author asked for "a lot
    /// of padding", and the largest amount available is a better reading of
    /// that than none.
    OutOfRange {
        key: String,
        value: i64,
        min: i64,
        max: i64,
    },
    /// A key under `[layout]` that names no setting.
    UnknownSetting {
        name: String,
        suggestion: Option<&'static str>,
    },
    /// Text that would be unreadable on the band under the reading line.
    ///
    /// Its own role clears AA against the *page*, which is the check every
    /// other colour gets and which says nothing about the one row where the
    /// ground moves. A theme whose `current_line_bg` lands on top of its
    /// `text` produces exactly one illegible row, wherever the reader is
    /// standing — the hardest kind of contrast fault to attribute, because it
    /// follows you.
    LowContrastOnCurrentLine {
        role: &'static str,
        ratio: f64,
        floor: f64,
    },
    /// A `[syntax]` table that names some captures and not others.
    ///
    /// Not an error, and the distinction matters: naming one capture hands the
    /// theme the whole code block (see `ThemeOverrides::owns_syntax`), so the
    /// ones left out do not keep their old colours — they take `text`. That is
    /// a legible outcome and sometimes the intended one, but it is never what
    /// someone who simply hasn't finished typing expects to see, so it is said
    /// out loud.
    IncompleteSyntax {
        named: usize,
        total: usize,
        /// The first unnamed capture, in the order `SYNTAX_ROLES` lists them,
        /// to make the message actionable rather than merely accurate.
        first_missing: &'static str,
    },
}

impl fmt::Display for ThemeWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThemeWarning::UnknownRole { name, suggestion } => {
                write!(f, "unknown role `{}`", strip_display_hazards(name))?;
                match suggestion {
                    Some(close) => write!(f, " — did you mean `{close}`?"),
                    None => Ok(()),
                }
            }
            ThemeWarning::BadColor { role, value } => write!(
                f,
                "`{}` is not a colour: `{}` — expected #rgb or #rrggbb",
                strip_display_hazards(role),
                strip_display_hazards(value)
            ),
            ThemeWarning::WrongType { key, expected } => {
                write!(f, "`{}` should be {expected}", strip_display_hazards(key))
            }
            ThemeWarning::MissingAppearance => {
                write!(f, "no `appearance` set — assuming dark")
            }
            ThemeWarning::UnknownAppearance { value } => write!(
                f,
                "unknown appearance `{}` — assuming dark",
                strip_display_hazards(value)
            ),
            ThemeWarning::LowContrast { role, ratio, floor } => write!(
                f,
                "`{role}` is {ratio:.1}:1 against the page — under WCAG AA's {floor}:1"
            ),
            ThemeWarning::Downsample256Collision { first, second } => write!(
                f,
                "`{first}` and `{second}` are identical in a 256-colour terminal"
            ),
            ThemeWarning::OutOfRange {
                key,
                value,
                min,
                max,
            } => write!(
                f,
                "`{}` is {value} — clamped to {min}..={max}",
                strip_display_hazards(key)
            ),
            ThemeWarning::UnknownSetting { name, suggestion } => {
                write!(
                    f,
                    "unknown `[layout]` key `{}`",
                    strip_display_hazards(name)
                )?;
                match suggestion {
                    Some(close) => write!(f, " — did you mean `{close}`?"),
                    None => Ok(()),
                }
            }
            ThemeWarning::LowContrastOnCurrentLine { role, ratio, floor } => write!(
                f,
                "`{role}` is {ratio:.1}:1 against `current_line_bg` — under WCAG AA's {floor}:1"
            ),
            ThemeWarning::IncompleteSyntax {
                named,
                total,
                first_missing,
            } => write!(
                f,
                "`[syntax]` sets {named} of {total} roles — the rest (`{first_missing}`, …) \
                 fall back to `text`"
            ),
        }
    }
}

impl ThemeFile {
    /// Parses a theme, collecting every problem rather than stopping at the
    /// first — someone fixing a theme wants the whole list, not one round
    /// trip per typo.
    pub fn parse(source: &str) -> Result<(ThemeFile, Vec<ThemeWarning>), ThemeError> {
        if source.len() > MAX_THEME_BYTES {
            return Err(ThemeError::TooLarge {
                bytes: source.len(),
            });
        }
        let table: toml::Table = source
            .parse()
            .map_err(|e: toml::de::Error| ThemeError::Syntax(strip_display_hazards(e.message())))?;

        let mut warnings = Vec::new();

        let name = match table.get("name") {
            Some(toml::Value::String(s)) => strip_display_hazards(s),
            Some(_) => {
                warnings.push(ThemeWarning::WrongType {
                    key: "name".to_string(),
                    expected: "a string",
                });
                String::new()
            }
            None => String::new(),
        };

        let appearance = match table.get("appearance") {
            Some(toml::Value::String(s)) if s == "dark" => Variant::Dark,
            Some(toml::Value::String(s)) if s == "light" => Variant::Light,
            Some(toml::Value::String(s)) => {
                warnings.push(ThemeWarning::UnknownAppearance { value: s.clone() });
                Variant::Dark
            }
            Some(_) => {
                warnings.push(ThemeWarning::WrongType {
                    key: "appearance".to_string(),
                    expected: "\"dark\" or \"light\"",
                });
                Variant::Dark
            }
            None => {
                warnings.push(ThemeWarning::MissingAppearance);
                Variant::Dark
            }
        };

        let mut overrides = ThemeOverrides::new();
        // Backgrounds share the `[colors]` table with foregrounds, so one pass
        // resolves both and the entry decides which channel it lands in. A
        // name that is neither is where `UnknownRole` comes from, which is why
        // the suggestion list is both vocabularies concatenated: someone who
        // typed `code_block_bh` wants to hear about `code_block_bg`.
        for (role, color) in color_table(
            &table,
            "colors",
            colors_from_name,
            &colors_vocabulary(),
            &mut warnings,
        ) {
            match role {
                ColorRole::Foreground(semantic) => overrides.insert(semantic, color),
                ColorRole::Background(semantic) => overrides.insert_background(semantic, color),
            };
        }
        for (capture, color) in color_table(
            &table,
            "syntax",
            capture_from_name,
            SYNTAX_ROLES,
            &mut warnings,
        ) {
            overrides.insert_capture(capture, color);
        }

        // Said once for the whole table rather than once per unnamed capture:
        // a theme that sets three of twenty-five would otherwise bury the
        // status row under twenty-two warnings that are all the same fact.
        if overrides.owns_syntax()
            && let Some(missing) = role::ALL
                .iter()
                .find(|&&capture| overrides.capture(capture).is_none())
        {
            warnings.push(ThemeWarning::IncompleteSyntax {
                named: overrides.capture_len(),
                total: role::ALL.len(),
                first_missing: capture_name(*missing),
            });
        }

        let chrome = chrome_table(&table, &mut warnings);

        Ok((
            ThemeFile {
                name,
                appearance,
                overrides,
                chrome,
            },
            warnings,
        ))
    }

    /// The checks the built-in variants must pass as hard assertions, run
    /// against a user theme as advice.
    ///
    /// Split out of [`ThemeFile::parse`] rather than folded into it because
    /// the two answer different questions — parse asks "can I read this",
    /// lint asks "will you be able to read it" — and only the first is
    /// mandatory. Both use the same arithmetic the built-ins are tested with,
    /// so a colour cannot be legible to one and not the other.
    pub fn lint(&self) -> Vec<ThemeWarning> {
        let mut warnings = Vec::new();
        let page = reference_background(self.appearance);

        // Sorted so the report is stable across runs — a HashMap's order is
        // not, and an unstable warning list makes a fixed theme look changed.
        let mut named: Vec<(&'static str, layout::Semantic, Color)> = self
            .overrides
            .iter()
            .filter_map(|(semantic, color)| role_name(semantic).map(|name| (name, semantic, color)))
            .collect();
        named.sort_by_key(|(name, _, _)| *name);

        for (name, semantic, color) in &named {
            // Two floors, because a `─` is not a paragraph. WCAG 1.4.3 asks
            // 4.5:1 of text; 1.4.11 asks 3:1 of a graphical object. Holding a
            // table border to the text bar would force every theme to paint
            // its furniture as loudly as its prose.
            let floor = if is_structural(*semantic) {
                AA_NON_TEXT
            } else {
                AA_NORMAL_TEXT
            };
            let ratio = contrast_ratio(*color, page);
            if ratio < floor {
                warnings.push(ThemeWarning::LowContrast {
                    role: name,
                    ratio,
                    floor,
                });
            }
        }

        // Only colours that *differ* and then merge. Two roles the author
        // deliberately painted the same colour are a choice — `link` and
        // `alert_note` sharing one blue is a theme with a palette, not a
        // theme with a bug — and warning about it would train the reader to
        // ignore this warning where it does mean something: two colours that
        // look distinct until the terminal quantizes them.
        let mut seen: Vec<(&'static str, layout::Semantic, Color, Color)> = Vec::new();
        for (name, semantic, color) in &named {
            let cell = crate::color::downsample_256(*color);
            let collision = seen.iter().find(|(_, other_role, truecolor, quantized)| {
                *quantized == cell
                    && *truecolor != *color
                    // Two heading rungs merging is allowed, for the same
                    // reason `build_palette` stopped forbidding it in the
                    // built-ins: heading depth is carried by the *count* of
                    // block markers, so rungs that quantize together cost a
                    // reader nothing they need. A heading merging with an
                    // ordinary role is still worth saying — that one really
                    // does make two different things look like one.
                    && !(is_heading(*semantic) && is_heading(*other_role))
            });
            match collision {
                Some((first, _, _, _)) => warnings.push(ThemeWarning::Downsample256Collision {
                    first,
                    second: name,
                }),
                None => seen.push((name, *semantic, *color, cell)),
            }
        }

        warnings.extend(self.lint_current_line(&named));
        warnings.extend(self.lint_syntax());
        warnings
    }

    /// Contrast against the band under the reading line, for the roles that
    /// can end up standing on it.
    ///
    /// A second pass rather than a wider first one, because it asks a
    /// different question. The main check asks "is this colour legible on the
    /// page"; this asks "is it still legible on the one row whose ground
    /// moves". A theme can pass the first and fail this — `text` at 8:1
    /// against the page and 1.9:1 against a band the author picked to be
    /// bold — and the failure is the nastiest kind to report from a bug
    /// report, because the illegible row is wherever the reader happens to be
    /// standing and moves when they move.
    ///
    /// Only runs when the theme names `current_line_bg`. The built-in wash is
    /// within 1.34:1 of the reference background by construction, so measuring
    /// against it instead of the page would differ by less than the rounding
    /// in the message and would fire on themes that set no band at all.
    ///
    /// Every role is measured, not just `text`: the reading line can land on a
    /// heading, a table border or a footnote label just as easily, and a band
    /// that eats exactly one of them is a band that looks broken once a
    /// document happens to put that role under the cursor.
    fn lint_current_line(
        &self,
        named: &[(&'static str, layout::Semantic, Color)],
    ) -> Vec<ThemeWarning> {
        let Some(band) = self.overrides.background(layout::Semantic::CurrentLine) else {
            return Vec::new();
        };
        let mut warnings = Vec::new();
        for (name, semantic, color) in named {
            let floor = if is_structural(*semantic) {
                AA_NON_TEXT
            } else {
                AA_NORMAL_TEXT
            };
            let ratio = contrast_ratio(*color, band);
            if ratio < floor {
                warnings.push(ThemeWarning::LowContrastOnCurrentLine {
                    role: name,
                    ratio,
                    floor,
                });
            }
        }
        warnings
    }

    /// The same two checks over `[syntax]`, run as a separate pass rather than
    /// folded into the semantic one.
    ///
    /// Contrast is asked of every capture at the text floor — a keyword is
    /// read, and `is_structural` has no answer for a token because none of
    /// them are furniture.
    ///
    /// The 256-colour check deliberately does **not** cross the two sets. It
    /// asks "will these two look like one colour", and that question only has
    /// teeth for colours a reader sees side by side: two captures share a code
    /// block, and two semantic roles share a page, but a `keyword` quantizing
    /// onto the same cell as `table_border` costs nobody anything — they never
    /// appear in the same place. Crossing them would emit dozens of warnings
    /// for a full theme and train the reader to ignore all of them, including
    /// the ones inside a code block that do matter.
    fn lint_syntax(&self) -> Vec<ThemeWarning> {
        let mut warnings = Vec::new();
        // Syntax colours are not measured against the page — they are measured
        // against whatever the code block is sitting on. A theme that sets
        // `code_block_bg` has moved the ground under every token in the block,
        // and reporting a keyword's contrast against the page after that would
        // be arithmetic about a pair of colours the reader never sees adjacent.
        //
        // Falling back to the page when no slab is set is not quite right
        // either — there is still the built-in wash — but the built-in is
        // within 1.2:1 of the reference by construction, so the two answers
        // differ by less than the rounding in the message.
        let page = self
            .overrides
            .background(layout::Semantic::CodeBlock)
            .unwrap_or_else(|| reference_background(self.appearance));

        let mut named: Vec<(&'static str, Color)> = self
            .overrides
            .capture_iter()
            .map(|(capture, color)| (capture_name(capture), color))
            .collect();
        named.sort_by_key(|(name, _)| *name);

        for (name, color) in &named {
            let ratio = contrast_ratio(*color, page);
            if ratio < AA_NORMAL_TEXT {
                warnings.push(ThemeWarning::LowContrast {
                    role: name,
                    ratio,
                    floor: AA_NORMAL_TEXT,
                });
            }
        }

        let mut seen: Vec<(&'static str, Color, Color)> = Vec::new();
        for (name, color) in &named {
            let cell = crate::color::downsample_256(*color);
            let collision = seen
                .iter()
                .find(|(_, truecolor, quantized)| *quantized == cell && *truecolor != *color);
            match collision {
                Some((first, _, _)) => warnings.push(ThemeWarning::Downsample256Collision {
                    first,
                    second: name,
                }),
                None => seen.push((name, *color, cell)),
            }
        }

        warnings
    }
}

/// Whether a role is one of the six heading rungs.
fn is_heading(semantic: layout::Semantic) -> bool {
    matches!(
        semantic,
        layout::Semantic::Heading(_) | layout::Semantic::HeadingRung(_)
    )
}

/// `#rgb` or `#rrggbb`, case-insensitive, leading `#` required.
///
/// Deliberately narrow. Named colours, `rgb()` functions and bare hex without
/// the `#` are all things a theme author might reasonably try, and every one
/// of them is a *warned* miss rather than a silent guess — a colour system
/// that guesses is one that paints something nobody chose.
fn parse_hex(text: &str) -> Option<Color> {
    let body = text.strip_prefix('#')?;
    if !body.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let nibble = |i: usize| u8::from_str_radix(&body[i..=i], 16).ok();
    match body.len() {
        3 => {
            let (r, g, b) = (nibble(0)?, nibble(1)?, nibble(2)?);
            // `#abc` means `#aabbcc`, so each nibble doubles rather than
            // shifting — `0xa` is `0xaa`, not `0xa0`.
            Some(Color::new(r * 17, g * 17, b * 17))
        }
        6 => {
            let pair = |i: usize| u8::from_str_radix(&body[i..i + 2], 16).ok();
            Some(Color::new(pair(0)?, pair(2)?, pair(4)?))
        }
        _ => None,
    }
}

/// Which channel a `[colors]` entry paints into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorRole {
    Foreground(layout::Semantic),
    Background(layout::Semantic),
}

/// Resolves a `[colors]` key, foreground names first.
///
/// The two vocabularies are disjoint — `background_from_name` answers only for
/// names ending `_bg`, which no foreground role uses — so the order is for
/// readability rather than precedence, and
/// `test_no_name_is_both_a_foreground_and_a_background` keeps it that way.
fn colors_from_name(name: &str) -> Option<ColorRole> {
    if let Some(semantic) = semantic_from_name(name) {
        return Some(ColorRole::Foreground(semantic));
    }
    background_from_name(name).map(ColorRole::Background)
}

/// Every name legal under `[colors]`, for the did-you-mean suggestion.
fn colors_vocabulary() -> Vec<&'static str> {
    THEMEABLE_ROLES
        .iter()
        .chain(BACKGROUND_ROLES.iter())
        .copied()
        .collect()
}

/// Reads one `name = "#rrggbb"` table into resolved roles, warning about
/// every entry it could not use and returning the ones it could.
///
/// Shared by `[colors]` and `[syntax]`, which differ only in which names are
/// legal: the type checking, the hex grammar, the suggestion on a typo and the
/// one-bad-value-costs-one-colour rule are identical in both, and two copies
/// of them would be two places for the rules to drift. `resolve` is what makes
/// them different, and `candidates` is the list a near miss is measured
/// against — pass the same pair or a typo in `[syntax]` will suggest a
/// semantic role.
fn color_table<R>(
    table: &toml::Table,
    section: &'static str,
    resolve: impl Fn(&str) -> Option<R>,
    candidates: &[&'static str],
    warnings: &mut Vec<ThemeWarning>,
) -> Vec<(R, Color)> {
    let entries = match table.get(section) {
        Some(toml::Value::Table(entries)) => entries,
        Some(_) => {
            warnings.push(ThemeWarning::WrongType {
                key: section.to_string(),
                expected: "a table",
            });
            return Vec::new();
        }
        // A theme with no `[colors]` — or no `[syntax]` — is inert but not
        // wrong. It is what you get halfway through writing one, and for
        // `[syntax]` it is also what every theme written before the table
        // existed looks like.
        None => return Vec::new(),
    };

    let mut resolved = Vec::new();
    for (key, value) in entries {
        let Some(role) = resolve(key) else {
            warnings.push(ThemeWarning::UnknownRole {
                name: key.clone(),
                suggestion: closest(key, candidates),
            });
            continue;
        };
        let toml::Value::String(text) = value else {
            warnings.push(ThemeWarning::WrongType {
                key: key.clone(),
                expected: "a colour string like \"#c8d0e0\"",
            });
            continue;
        };
        match parse_hex(text) {
            Some(color) => resolved.push((role, color)),
            None => warnings.push(ThemeWarning::BadColor {
                role: key.clone(),
                value: text.clone(),
            }),
        }
    }
    resolved
}

/// Every key a theme file's `[layout]` table may set, in the order
/// `docs/theming.md` lists them. Public surface, like [`THEMEABLE_ROLES`].
pub const LAYOUT_SETTINGS: &[&str] = &[
    "padding_left",
    "padding_right",
    "padding_top",
    "padding_bottom",
    "line_numbers",
    "gutter_gap",
    "current_line",
    "scrolloff",
];

/// The widest padding a theme may ask for on one side.
///
/// Not a guess about taste — the clamp exists because this file is untrusted,
/// and a `padding_left = 60000` is a `u16` away from arithmetic nobody
/// reviewed. Sixty-four cells is past any real margin and short of anything
/// that can overflow. It is not the *effective* limit in a narrow terminal:
/// [`Chrome::fit`] drops the whole chrome long before this.
const MAX_PADDING: i64 = 64;
/// The widest gap between the gutter and the page. Past a handful of cells the
/// numbers stop reading as a gutter and start reading as a second column.
const MAX_GUTTER_GAP: i64 = 8;
/// The most rows a reader may keep between the reading line and the edge.
/// Larger values are silently equivalent to "keep it centred", which
/// `scrolloff` reaching half the viewport already achieves.
const MAX_SCROLLOFF: i64 = 32;

/// Reads the `[layout]` table, defaulting every key it does not find.
///
/// The same never-fatal policy the colour tables follow, for the same reason:
/// a theme is a file people share, and one bad value should cost that value
/// and nothing else. A padding out of range is clamped rather than dropped —
/// somebody who wrote `padding_left = 200` wants a wide margin, and the widest
/// available is a better reading of that than none at all.
fn chrome_table(table: &toml::Table, warnings: &mut Vec<ThemeWarning>) -> Chrome {
    let mut chrome = Chrome::default();
    let entries = match table.get("layout") {
        Some(toml::Value::Table(entries)) => entries,
        Some(_) => {
            warnings.push(ThemeWarning::WrongType {
                key: "layout".to_string(),
                expected: "a table",
            });
            return chrome;
        }
        None => return chrome,
    };

    let mut padding = Padding::default();
    for (key, value) in entries {
        match key.as_str() {
            "padding_left" => padding.left = cells(key, value, MAX_PADDING, warnings, 0),
            "padding_right" => padding.right = cells(key, value, MAX_PADDING, warnings, 0),
            "padding_top" => padding.top = cells(key, value, MAX_PADDING, warnings, 0),
            "padding_bottom" => padding.bottom = cells(key, value, MAX_PADDING, warnings, 0),
            "gutter_gap" => {
                chrome.gutter_gap = cells(key, value, MAX_GUTTER_GAP, warnings, chrome.gutter_gap)
            }
            "scrolloff" => {
                chrome.scrolloff = cells(key, value, MAX_SCROLLOFF, warnings, chrome.scrolloff)
            }
            "line_numbers" => chrome.line_numbers = flag(key, value, warnings, chrome.line_numbers),
            "current_line" => chrome.current_line = flag(key, value, warnings, chrome.current_line),
            _ => warnings.push(ThemeWarning::UnknownSetting {
                name: key.clone(),
                suggestion: closest(key, LAYOUT_SETTINGS),
            }),
        }
    }
    chrome.padding = padding;
    chrome
}

/// One `[layout]` integer, clamped to `0..=max`. `fallback` on a wrong type,
/// so a `padding_left = "2"` costs that key and leaves the rest of the table
/// standing.
fn cells(
    key: &str,
    value: &toml::Value,
    max: i64,
    warnings: &mut Vec<ThemeWarning>,
    fallback: u16,
) -> u16 {
    let Some(raw) = value.as_integer() else {
        warnings.push(ThemeWarning::WrongType {
            key: key.to_string(),
            expected: "a whole number of cells",
        });
        return fallback;
    };
    if raw < 0 || raw > max {
        warnings.push(ThemeWarning::OutOfRange {
            key: key.to_string(),
            value: raw,
            min: 0,
            max,
        });
    }
    // Safe by the clamp: `max` is well under `u16::MAX` at every call site.
    raw.clamp(0, max) as u16
}

/// One `[layout]` boolean. TOML has a real boolean type, so `"true"` is a
/// wrong type rather than a truthy string — a theme that meant `true` and
/// quoted it should hear about it once instead of behaving unpredictably.
fn flag(key: &str, value: &toml::Value, warnings: &mut Vec<ThemeWarning>, fallback: bool) -> bool {
    match value.as_bool() {
        Some(on) => on,
        None => {
            warnings.push(ThemeWarning::WrongType {
                key: key.to_string(),
                expected: "true or false",
            });
            fallback
        }
    }
}

/// The closest name in `candidates` to `name`, when one is close enough to be
/// worth suggesting. A typo in a theme file is silent by nature — the colour
/// simply does not appear — so naming the near miss is most of the fix.
fn closest(name: &str, candidates: &[&'static str]) -> Option<&'static str> {
    let lowered = name.to_ascii_lowercase();
    let mut best: Option<(usize, &'static str)> = None;
    for candidate in candidates {
        let distance = edit_distance(&lowered, candidate);
        // Two edits on a short name is a typo; on a long one it still is.
        // Three starts suggesting unrelated roles at each other.
        if distance > 2 {
            continue;
        }
        if best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, candidate));
        }
    }
    best.map(|(_, name)| name)
}

/// Levenshtein distance, two rows rather than a full matrix.
fn edit_distance(a: &str, b: &str) -> usize {
    let b_chars: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b_chars.len()).collect();
    let mut current = vec![0usize; b_chars.len() + 1];
    for (i, a_char) in a.chars().enumerate() {
        current[0] = i + 1;
        for (j, &b_char) in b_chars.iter().enumerate() {
            let substitution = previous[j] + usize::from(a_char != b_char);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b_chars.len()]
}

/// Every `[colors]` role name a theme may set, sorted — for
/// `docs/theming.md` and for error messages that want to list the
/// alternatives.
pub fn themeable_role_names() -> BTreeSet<&'static str> {
    THEMEABLE_ROLES.iter().copied().collect()
}

/// Every `[syntax]` role name a theme may set, sorted.
pub fn syntax_role_names() -> BTreeSet<&'static str> {
    SYNTAX_ROLES.iter().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::role::Capture;
    use layout::Semantic;

    /// The shape the whole design is for: name one role, inherit the rest.
    /// A theme this small has to be a *complete* theme, with nothing to
    /// complain about, or "download someone's theme" becomes "download
    /// someone's theme and then fix it".
    #[test]
    fn test_a_theme_naming_one_role_is_a_complete_theme() {
        let (theme, warnings) = ThemeFile::parse(
            "name = \"Minimal\"\nappearance = \"dark\"\n\n[colors]\ntext = \"#c8d0e0\"\n",
        )
        .expect("parses");
        assert_eq!(theme.name, "Minimal");
        assert_eq!(theme.appearance, Variant::Dark);
        assert_eq!(theme.overrides.semantic_len(), 1);
        assert_eq!(
            theme.overrides.get(Semantic::Text),
            Some(Color::new(0xc8, 0xd0, 0xe0))
        );
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    /// Every role name in the public list must actually be settable. If one
    /// were not, `docs/theming.md` would be advertising a colour nobody can
    /// set — the exact failure a shared theme format cannot afford.
    #[test]
    fn test_every_public_role_name_can_be_set_from_a_file() {
        let mut source = String::from("appearance = \"dark\"\n[colors]\n");
        for role in THEMEABLE_ROLES {
            source.push_str(&format!("{role} = \"#123456\"\n"));
        }
        let (theme, warnings) = ThemeFile::parse(&source).expect("parses");
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        assert_eq!(
            theme.overrides.semantic_len(),
            THEMEABLE_ROLES.len(),
            "every name in THEMEABLE_ROLES must reach a distinct role"
        );
    }

    /// A `[syntax]` table naming every capture is complete and silent. The
    /// same promise `[colors]` makes, for the other half of the format.
    #[test]
    fn test_every_syntax_role_name_can_be_set_from_a_file() {
        let (theme, warnings) = ThemeFile::parse(&full_syntax_theme()).expect("parses");
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        assert_eq!(
            theme.overrides.capture_len(),
            SYNTAX_ROLES.len(),
            "every name in SYNTAX_ROLES must reach a distinct capture"
        );
        assert!(theme.overrides.owns_syntax());
    }

    /// The one thing about `[syntax]` a user cannot guess: naming one capture
    /// takes the whole block, so the rest fall back to `text` rather than
    /// keeping the colours they had. Warned once for the table, not once per
    /// missing role — twenty-two lines of the same fact is not a status row.
    #[test]
    fn test_a_partial_syntax_table_warns_once_and_names_what_it_left_out() {
        let (theme, warnings) =
            ThemeFile::parse("appearance = \"dark\"\n[syntax]\nkeyword = \"#ff88aa\"\n")
                .expect("parses");
        assert!(theme.overrides.owns_syntax());

        let incomplete: Vec<&ThemeWarning> = warnings
            .iter()
            .filter(|w| matches!(w, ThemeWarning::IncompleteSyntax { .. }))
            .collect();
        assert_eq!(
            incomplete.len(),
            1,
            "expected exactly one incompleteness warning: {warnings:?}"
        );
        assert_eq!(
            *incomplete[0],
            ThemeWarning::IncompleteSyntax {
                named: 1,
                total: SYNTAX_ROLES.len(),
                // The first name in `role::ALL`, which `keyword` is not.
                first_missing: "attribute",
            }
        );
    }

    /// No `[syntax]` at all is the pre-existing world and must stay silent —
    /// every theme written before the table existed is this shape.
    #[test]
    fn test_a_theme_with_no_syntax_table_says_nothing_about_syntax() {
        let (theme, warnings) =
            ThemeFile::parse("appearance = \"dark\"\n[colors]\ntext = \"#c8d0e0\"\n")
                .expect("parses");
        assert!(!theme.overrides.owns_syntax());
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    /// An empty `[syntax]` header is halfway through typing one, not a request
    /// to blank every syntax colour. It leaves the generated palette alone.
    #[test]
    fn test_an_empty_syntax_table_does_not_take_over_the_block() {
        let (theme, warnings) =
            ThemeFile::parse("appearance = \"dark\"\n[syntax]\n").expect("parses");
        assert!(!theme.overrides.owns_syntax());
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    /// A typo under `[syntax]` must be measured against the capture names. If
    /// the suggestion came from `THEMEABLE_ROLES`, a mistyped `strng` would be
    /// answered with `strong` — a real role, in the wrong table, which is a
    /// worse hint than none.
    #[test]
    fn test_a_typo_under_syntax_suggests_a_capture_not_a_semantic_role() {
        let (_, warnings) =
            ThemeFile::parse("appearance = \"dark\"\n[syntax]\nstrng = \"#ff88aa\"\n")
                .expect("parses");
        let suggestion = warnings.iter().find_map(|w| match w {
            ThemeWarning::UnknownRole { name, suggestion } if name == "strng" => Some(*suggestion),
            _ => None,
        });
        assert_eq!(
            suggestion,
            Some(Some("string")),
            "expected `string` from the syntax vocabulary: {warnings:?}"
        );
    }

    /// And the reverse, so the two vocabularies cannot quietly become one:
    /// `keyword` is a real capture and means nothing under `[colors]`.
    #[test]
    fn test_a_capture_name_under_colors_is_an_unknown_role() {
        let (theme, warnings) =
            ThemeFile::parse("appearance = \"dark\"\n[colors]\nkeyword = \"#ff88aa\"\n")
                .expect("parses");
        assert_eq!(theme.overrides.semantic_len(), 0);
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, ThemeWarning::UnknownRole { name, .. } if name == "keyword")),
            "expected `keyword` to be unknown under [colors]: {warnings:?}"
        );
    }

    /// One bad hex under `[syntax]` costs that capture and nothing else —
    /// the same rule `[colors]` follows, proven separately because it is a
    /// separate code path's worth of nothing going wrong.
    #[test]
    fn test_a_bad_colour_under_syntax_costs_one_capture() {
        let (theme, warnings) = ThemeFile::parse(
            "appearance = \"dark\"\n[syntax]\nkeyword = \"crimson\"\nstring = \"#88ff88\"\n",
        )
        .expect("parses");
        assert_eq!(theme.overrides.capture_len(), 1);
        assert_eq!(
            theme.overrides.capture(Capture::String),
            Some(Color::new(0x88, 0xff, 0x88))
        );
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, ThemeWarning::BadColor { role, .. } if role == "keyword")),
            "expected a BadColor for keyword: {warnings:?}"
        );
    }

    /// Syntax colours are read, so they answer to the text floor. A theme that
    /// paints comments almost into the page is legal and gets said out loud.
    #[test]
    fn test_lint_flags_an_illegible_capture() {
        let (theme, _) =
            ThemeFile::parse("appearance = \"dark\"\n[syntax]\ncomment = \"#1b1c27\"\n")
                .expect("parses");
        assert!(
            theme.lint().iter().any(|w| matches!(
                w,
                ThemeWarning::LowContrast {
                    role: "comment",
                    floor,
                    ..
                } if *floor == AA_NORMAL_TEXT
            )),
            "a near-background comment must be reported: {:?}",
            theme.lint()
        );
    }

    /// The 256-colour check runs inside the capture set, where two colours
    /// really do sit side by side in one block.
    #[test]
    fn test_lint_flags_two_captures_that_quantize_together() {
        let (theme, _) = ThemeFile::parse(
            "appearance = \"dark\"\n[syntax]\nkeyword = \"#ff0000\"\nstring = \"#fe0101\"\n",
        )
        .expect("parses");
        assert!(
            theme
                .lint()
                .iter()
                .any(|w| matches!(w, ThemeWarning::Downsample256Collision { .. })),
            "two near-identical captures must be reported: {:?}",
            theme.lint()
        );
    }

    /// But it does not cross into `[colors]`. A `keyword` and a `table_border`
    /// never appear in the same place, so warning about them would be noise —
    /// and noise here trains the reader past the collisions that do matter.
    #[test]
    fn test_lint_does_not_compare_captures_against_semantic_roles() {
        let (theme, _) = ThemeFile::parse(
            "appearance = \"dark\"\n[colors]\ntable_border = \"#ff0000\"\n\
             [syntax]\nkeyword = \"#fe0101\"\n",
        )
        .expect("parses");
        assert!(
            !theme
                .lint()
                .iter()
                .any(|w| matches!(w, ThemeWarning::Downsample256Collision { .. })),
            "a capture and a semantic role must not be compared: {:?}",
            theme.lint()
        );
    }

    /// A theme naming every capture, for tests that need `[syntax]` complete
    /// and silent. Colours are irrelevant and deliberately uniform — the
    /// lint's opinion of them is a different test's business.
    fn full_syntax_theme() -> String {
        let mut source = String::from("appearance = \"dark\"\n[syntax]\n");
        for role in SYNTAX_ROLES {
            source.push_str(&format!("{role} = \"#123456\"\n"));
        }
        source
    }

    /// One bad value costs you that value, never the file. This is the
    /// difference between a theme format people can iterate on and one where
    /// a typo blanks every colour you got right.
    #[test]
    fn test_a_bad_value_costs_only_itself() {
        let (theme, warnings) = ThemeFile::parse(
            "appearance = \"dark\"\n\
             [colors]\n\
             text = \"#c8d0e0\"\n\
             heading1 = \"not-a-colour\"\n\
             heding2 = \"#ff0000\"\n\
             link = 42\n\
             strong = \"#0f0\"\n",
        )
        .expect("parses");

        assert_eq!(
            theme.overrides.get(Semantic::Text),
            Some(Color::new(0xc8, 0xd0, 0xe0)),
            "a good role before the bad ones must survive"
        );
        assert_eq!(
            theme.overrides.get(Semantic::Strong),
            Some(Color::new(0x00, 0xff, 0x00)),
            "a good role after the bad ones must survive"
        );
        assert_eq!(theme.overrides.get(Semantic::Heading(1)), None);
        assert_eq!(theme.overrides.get(Semantic::Link), None);

        assert!(
            warnings.iter().any(|w| matches!(
                w,
                ThemeWarning::BadColor { role, .. } if role == "heading1"
            )),
            "no BadColor warning: {warnings:?}"
        );
        assert!(
            warnings.iter().any(|w| matches!(
                w,
                ThemeWarning::UnknownRole {
                    suggestion: Some("heading2"),
                    ..
                }
            )),
            "a one-letter typo should suggest the real role: {warnings:?}"
        );
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, ThemeWarning::WrongType { key, .. } if key == "link")),
            "no WrongType warning: {warnings:?}"
        );
    }

    /// `#abc` is `#aabbcc`. Getting this wrong gives `#a0b0c0`, which is
    /// merely a slightly darker colour — wrong in a way no one would notice
    /// until their theme looked subtly off against its own documentation.
    #[test]
    fn test_short_hex_expands_by_doubling_not_shifting() {
        let (theme, _) =
            ThemeFile::parse("[colors]\ntext = \"#abc\"\nstrong = \"#fff\"\n").expect("parses");
        assert_eq!(
            theme.overrides.get(Semantic::Text),
            Some(Color::new(0xaa, 0xbb, 0xcc))
        );
        assert_eq!(
            theme.overrides.get(Semantic::Strong),
            Some(Color::new(0xff, 0xff, 0xff)),
            "#fff must be pure white, not #f0f0f0"
        );
    }

    /// A theme file is untrusted input and its strings end up in the status
    /// row. If a `name` or a bad-colour value could carry ESC, a downloaded
    /// theme would repaint the terminal through the error path — the barricade
    /// document text already has, arriving by a second door.
    #[test]
    fn test_no_warning_can_carry_an_escape_sequence_to_the_terminal() {
        // TOML's own grammar rejects a *raw* ESC inside a basic string, so a
        // hostile theme would not write one — it would use TOML's `\uXXXX`
        // escapes, which are legal and decode to exactly the same bytes. That
        // is the input worth defending against, so that is the input here.
        // Quoted keys for the same reason: bare keys cannot hold these bytes.
        let hostile = r##"name = "\u001B]0;pwned\u0007"
appearance = "\u001B[31mred"

[colors]
"\u009Bbogus" = "#fff"
text = "\u001B[5m#fff"
"##;
        let (theme, warnings) = ThemeFile::parse(hostile).expect("parses");
        assert!(
            theme.name.contains("pwned"),
            "the escape must have decoded — otherwise this tests nothing: {:?}",
            theme.name
        );
        assert!(
            !warnings.is_empty(),
            "the hostile input must actually produce warnings, or this proves nothing"
        );

        for warning in &warnings {
            let rendered = warning.to_string();
            for byte in rendered.bytes() {
                assert!(
                    byte >= 0x20 || byte == b'\t',
                    "warning {warning:?} rendered a control byte {byte:#04x}: {rendered:?}"
                );
            }
            assert!(
                !rendered.contains('\u{1b}') && !rendered.contains('\u{9b}'),
                "warning rendered an escape introducer: {rendered:?}"
            );
        }
        for ch in theme.name.chars() {
            assert!(!crate::is_display_hazard(ch), "name kept a hazard: {ch:?}");
        }
    }

    /// The one hard failure, and it must be a failure rather than a panic —
    /// a theme file is the kind of thing people hand-edit at 1am.
    #[test]
    fn test_malformed_toml_is_an_error_not_a_panic() {
        for source in [
            "name = \"unclosed",
            "[colors\ntext = \"#fff\"",
            "= \"no key\"",
            "[colors]\ntext = ",
            "\u{0}\u{1}\u{2}",
        ] {
            assert!(
                matches!(ThemeFile::parse(source), Err(ThemeError::Syntax(_))),
                "expected a syntax error for {source:?}"
            );
        }
    }

    /// A pathological file must be refused by size rather than parsed. The
    /// bound exists so a hostile theme cannot turn into a hostile parse.
    #[test]
    fn test_a_pathological_file_is_refused_by_size() {
        let huge = format!("name = \"{}\"\n", "a".repeat(MAX_THEME_BYTES));
        assert!(matches!(
            ThemeFile::parse(&huge),
            Err(ThemeError::TooLarge { .. })
        ));
        // And a merely large-but-sane file still parses.
        let big = format!(
            "name = \"{}\"\n[colors]\ntext = \"#fff\"\n",
            "a".repeat(1024)
        );
        assert!(ThemeFile::parse(&big).is_ok());
    }

    /// Missing `appearance` is a warning, not an error — and the default has
    /// to be *stated*, because which built-in a partial theme inherits from
    /// decides every colour it did not set.
    #[test]
    fn test_a_missing_appearance_warns_and_defaults_to_dark() {
        let (theme, warnings) = ThemeFile::parse("[colors]\ntext = \"#fff\"\n").expect("parses");
        assert_eq!(theme.appearance, Variant::Dark);
        assert!(warnings.contains(&ThemeWarning::MissingAppearance));

        let (theme, warnings) =
            ThemeFile::parse("appearance = \"neon\"\n[colors]\ntext = \"#fff\"\n").expect("parses");
        assert_eq!(theme.appearance, Variant::Dark);
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, ThemeWarning::UnknownAppearance { .. })),
            "{warnings:?}"
        );
    }

    /// The user's explicit call: an illegible colour loads and is reported,
    /// rather than being refused. The check itself must be real, though —
    /// it runs the same arithmetic the built-ins are asserted against.
    #[test]
    fn test_an_illegible_colour_loads_and_is_reported() {
        let (theme, warnings) =
            ThemeFile::parse("appearance = \"dark\"\n[colors]\ntext = \"#1b1c27\"\n")
                .expect("parses");
        assert!(warnings.is_empty(), "parsing must not judge colours");
        assert_eq!(
            theme.overrides.get(Semantic::Text),
            Some(Color::new(0x1b, 0x1c, 0x27)),
            "the colour must be applied despite being unreadable"
        );

        let lint = theme.lint();
        assert!(
            lint.iter()
                .any(|w| matches!(w, ThemeWarning::LowContrast { role: "text", .. })),
            "near-black text on a near-black page should warn: {lint:?}"
        );
    }

    /// A legible theme must lint clean, or the warning is noise people learn
    /// to ignore.
    #[test]
    fn test_a_legible_theme_lints_clean() {
        let (theme, _) = ThemeFile::parse(
            "appearance = \"dark\"\n[colors]\ntext = \"#e0e6f0\"\nheading1 = \"#ff9955\"\n",
        )
        .expect("parses");
        assert!(theme.lint().is_empty(), "{:?}", theme.lint());
    }

    /// Two roles that differ in truecolor but land on one 256-colour cell are
    /// indistinguishable for anyone in a 256-colour terminal.
    #[test]
    fn test_roles_colliding_in_256_colour_are_reported() {
        let (theme, _) = ThemeFile::parse(
            "appearance = \"dark\"\n[colors]\ntext = \"#e0e6f0\"\nstrong = \"#e1e7f1\"\n",
        )
        .expect("parses");
        let lint = theme.lint();
        assert!(
            lint.iter()
                .any(|w| matches!(w, ThemeWarning::Downsample256Collision { .. })),
            "two near-identical colours should collide once downsampled: {lint:?}"
        );
    }

    /// Lint output must be stable — it is built from a HashMap, whose order
    /// is not. An unstable list makes an unchanged theme look changed.
    #[test]
    fn test_lint_output_is_stable_across_runs() {
        let source = "appearance = \"dark\"\n[colors]\n\
                      text = \"#1b1c27\"\nstrong = \"#1c1d28\"\nlink = \"#1a1b26\"\n";
        let (theme, _) = ThemeFile::parse(source).expect("parses");
        let first: Vec<String> = theme.lint().iter().map(ToString::to_string).collect();
        for _ in 0..8 {
            let (again, _) = ThemeFile::parse(source).expect("parses");
            let repeat: Vec<String> = again.lint().iter().map(ToString::to_string).collect();
            assert_eq!(first, repeat, "lint order is not deterministic");
        }
    }

    /// This module must stay a pure function of its input, so it can be
    /// tested against hostile content without a filesystem and so no
    /// environment can change how a shared theme reads.
    #[test]
    fn test_parsing_touches_neither_the_filesystem_nor_the_environment() {
        // Only the module proper. This test's own body names the very paths
        // it forbids, so scanning the whole file would fail on itself.
        let whole = include_str!("theme_file.rs");
        let module = whole
            .split_once("#[cfg(test)]")
            .map(|(before, _)| before)
            .expect("this file has a test module");
        for forbidden in ["std::fs", "std::env", "std::io", "File::open"] {
            assert!(
                !module.contains(forbidden),
                "theme_file.rs reaches for {forbidden} — crates/stele owns finding the file"
            );
        }
    }

    // ---- [layout] ---------------------------------------------------------

    /// A file with no `[layout]` table is a complete theme, and the frame it
    /// gets is the one stele drew before the table existed.
    #[test]
    fn test_a_theme_with_no_layout_table_keeps_the_default_frame() {
        let (theme, warnings) =
            ThemeFile::parse("appearance = \"dark\"\n[colors]\ntext = \"#c8d0e0\"\n")
                .expect("parses");
        assert_eq!(theme.chrome, Chrome::default());
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn test_every_layout_key_round_trips_into_the_chrome() {
        let (theme, warnings) = ThemeFile::parse(
            "appearance = \"dark\"\n\
             [layout]\n\
             padding_left = 3\n\
             padding_right = 4\n\
             padding_top = 1\n\
             padding_bottom = 2\n\
             line_numbers = true\n\
             gutter_gap = 2\n\
             current_line = false\n\
             scrolloff = 5\n",
        )
        .expect("parses");
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            theme.chrome,
            Chrome {
                padding: Padding {
                    left: 3,
                    right: 4,
                    top: 1,
                    bottom: 2,
                },
                line_numbers: true,
                gutter_gap: 2,
                current_line: false,
                scrolloff: 5,
            }
        );
    }

    /// Out of range is clamped and reported, not dropped.
    ///
    /// Somebody who wrote `padding_left = 200` wants a wide margin, and the
    /// widest available is a better reading of that than none at all — the
    /// same never-fatal policy a bad hex gets.
    #[test]
    fn test_an_out_of_range_padding_is_clamped_and_says_so() {
        let (theme, warnings) =
            ThemeFile::parse("appearance = \"dark\"\n[layout]\npadding_left = 200\n")
                .expect("parses");
        assert_eq!(theme.chrome.padding.left, 64);
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, ThemeWarning::OutOfRange { key, value, .. }
                    if key == "padding_left" && *value == 200)),
            "{warnings:?}"
        );
    }

    /// A negative value takes the same path. TOML has signed integers and a
    /// `u16` does not, so this is the one clamp that would otherwise be a
    /// conversion panic rather than a bad setting.
    #[test]
    fn test_a_negative_padding_clamps_to_zero_rather_than_wrapping() {
        let (theme, warnings) =
            ThemeFile::parse("appearance = \"dark\"\n[layout]\npadding_top = -8\n")
                .expect("parses");
        assert_eq!(theme.chrome.padding.top, 0);
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, ThemeWarning::OutOfRange { .. })),
            "{warnings:?}"
        );
    }

    /// One bad key costs that key and nothing else, and the near miss is named
    /// — a typo in a theme file is silent by nature, so the suggestion is most
    /// of the fix.
    #[test]
    fn test_an_unknown_layout_key_is_reported_with_its_near_miss() {
        let (theme, warnings) = ThemeFile::parse(
            "appearance = \"dark\"\n[layout]\npadding_lft = 3\nline_numbers = true\n",
        )
        .expect("parses");
        assert!(
            theme.chrome.line_numbers,
            "the rest of the table must still apply"
        );
        assert!(
            warnings.iter().any(|w| matches!(
                w,
                ThemeWarning::UnknownSetting { name, suggestion: Some("padding_left") }
                    if name == "padding_lft"
            )),
            "{warnings:?}"
        );
    }

    /// TOML has a real boolean type, so a quoted one is a wrong type rather
    /// than a truthy string: the author should hear about it once instead of
    /// getting behaviour they cannot predict.
    #[test]
    fn test_a_quoted_boolean_is_a_wrong_type_and_leaves_the_default() {
        let (theme, warnings) =
            ThemeFile::parse("appearance = \"dark\"\n[layout]\nline_numbers = \"true\"\n")
                .expect("parses");
        assert!(!theme.chrome.line_numbers);
        assert!(
            warnings
                .iter()
                .any(|w| matches!(w, ThemeWarning::WrongType { key, .. } if key == "line_numbers")),
            "{warnings:?}"
        );
    }

    /// A band the reader's own prose disappears into.
    ///
    /// `text` clears AA against the page here and fails against the band, which
    /// is exactly the fault the ordinary contrast pass cannot see: it produces
    /// one illegible row, wherever the reader is standing, and it follows them.
    #[test]
    fn test_a_band_that_swallows_the_body_text_is_reported() {
        let (theme, _) = ThemeFile::parse(
            "appearance = \"dark\"\n\
             [colors]\n\
             text = \"#c8d0e0\"\n\
             current_line_bg = \"#c0c8d8\"\n",
        )
        .expect("parses");
        let warnings = theme.lint();
        assert!(
            warnings.iter().any(|w| matches!(
                w,
                ThemeWarning::LowContrastOnCurrentLine { role: "text", .. }
            )),
            "a `current_line_bg` that eats `text` must be reported: {warnings:?}"
        );
        // And the page-relative check still passes, which is why the second
        // pass has to exist at all.
        assert!(
            !warnings
                .iter()
                .any(|w| matches!(w, ThemeWarning::LowContrast { role: "text", .. })),
            "`text` is perfectly legible on the page — that is the point"
        );
    }

    /// No band named, no second pass. The built-in wash is within 1.34:1 of
    /// the reference background, so measuring against it would differ by less
    /// than the rounding in the message and would fire on themes that set no
    /// band at all.
    #[test]
    fn test_a_theme_that_names_no_band_gets_no_band_warnings() {
        let (theme, _) = ThemeFile::parse("appearance = \"dark\"\n[colors]\ntext = \"#c8d0e0\"\n")
            .expect("parses");
        assert!(
            !theme
                .lint()
                .iter()
                .any(|w| matches!(w, ThemeWarning::LowContrastOnCurrentLine { .. })),
            "{:?}",
            theme.lint()
        );
    }
}
