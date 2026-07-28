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

use crate::color::Color;
use crate::hazard::strip_display_hazards;
use crate::theme::{
    AA_NON_TEXT, AA_NORMAL_TEXT, THEMEABLE_ROLES, ThemeOverrides, Variant, contrast_ratio,
    is_structural, reference_background, role_name, semantic_from_name,
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
        match table.get("colors") {
            Some(toml::Value::Table(colors)) => {
                for (key, value) in colors {
                    let Some(semantic) = semantic_from_name(key) else {
                        warnings.push(ThemeWarning::UnknownRole {
                            name: key.clone(),
                            suggestion: closest_role(key),
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
                        Some(color) => {
                            overrides.insert(semantic, color);
                        }
                        None => warnings.push(ThemeWarning::BadColor {
                            role: key.clone(),
                            value: text.clone(),
                        }),
                    }
                }
            }
            Some(_) => warnings.push(ThemeWarning::WrongType {
                key: "colors".to_string(),
                expected: "a table",
            }),
            // A theme with no `[colors]` is inert but not wrong — it is what
            // you get halfway through writing one.
            None => {}
        }

        Ok((
            ThemeFile {
                name,
                appearance,
                overrides,
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

/// The closest themeable role to `name`, when one is close enough to be worth
/// suggesting. A typo in a theme file is silent by nature — the colour simply
/// does not appear — so naming the near miss is most of the fix.
fn closest_role(name: &str) -> Option<&'static str> {
    let lowered = name.to_ascii_lowercase();
    let mut best: Option<(usize, &'static str)> = None;
    for candidate in THEMEABLE_ROLES {
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

/// Every role name a theme may set, sorted — for `docs/theming.md` and for
/// error messages that want to list the alternatives.
pub fn themeable_role_names() -> BTreeSet<&'static str> {
    THEMEABLE_ROLES.iter().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(theme.overrides.len(), 1);
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
            theme.overrides.len(),
            THEMEABLE_ROLES.len(),
            "every name in THEMEABLE_ROLES must reach a distinct role"
        );
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
}
