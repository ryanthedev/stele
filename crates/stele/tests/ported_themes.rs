//! `themes/ports/` holds faithful ports of published editor themes, and it is
//! a different promise from `themes/`.
//!
//! A theme in `themes/` is stele's own, and answers to stele's bar: it must be
//! legible, full stop (see `shipped_themes.rs`). A port answers to its
//! upstream. Gruvbox's faded yellow, Nord's red, Dracula's comment blue and
//! Tokyo Night's comment grey all sit under WCAG AA, on their own backgrounds
//! as much as on stele's, and correcting them would ship something that merely
//! resembles the theme somebody asked for by name.
//!
//! So legibility is not asserted here. What is asserted is that every such
//! colour is *known* — each port names its own sub-AA roles in an `under-aa:`
//! header line, and this checks that line against the lint. A port cannot
//! quietly acquire an illegible role, and cannot claim one it does not have.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use highlight::{SYNTAX_ROLES, THEMEABLE_ROLES, ThemeFile, ThemeWarning};

fn ports_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("themes")
        .join("ports")
}

fn ports() -> Vec<(String, String)> {
    let dir = ports_dir();
    let mut themes: Vec<(String, String)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("no ports directory at {}: {e}", dir.display()))
        .map(|entry| entry.expect("readable dir entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "toml"))
        .map(|path| {
            let name = path
                .file_name()
                .expect("a file name")
                .to_string_lossy()
                .into_owned();
            (name, fs::read_to_string(&path).expect("readable theme"))
        })
        .collect();
    themes.sort();
    assert!(!themes.is_empty(), "no ports found in {}", dir.display());
    themes
}

/// The `# under-aa:` line a port declares about itself, as a set. An absent
/// line is an error rather than an empty set — a port that never made the
/// claim has not been checked, and silence would read the same as "none".
fn declared_under_aa(source: &str) -> BTreeSet<String> {
    let line = source
        .lines()
        .find_map(|l| l.trim_start_matches('#').trim().strip_prefix("under-aa:"))
        .expect("every port must carry an `# under-aa:` line, even an empty one");
    line.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// What the lint actually finds, in the same spelling the header uses:
/// `role` for a `[colors]` role, `syntax.role` for a capture.
fn actual_under_aa(theme: &ThemeFile) -> BTreeSet<String> {
    let syntax: BTreeSet<&str> = SYNTAX_ROLES.iter().copied().collect();
    theme
        .lint()
        .into_iter()
        .filter_map(|w| match w {
            // A capture and a semantic role can share a name (`link` cannot,
            // but `error` and `label` are only captures while `text` is only
            // semantic) — the sets are disjoint today and this asserts it
            // stays that way by spelling captures distinctly.
            ThemeWarning::LowContrast { role, .. } if syntax.contains(role) => {
                Some(format!("syntax.{role}"))
            }
            ThemeWarning::LowContrast { role, .. } => Some(role.to_string()),
            _ => None,
        })
        .collect()
}

/// A port must load cleanly. Contrast is a `lint` matter and deliberately not
/// checked here, but a bad hex or an unknown role is a mistake in the port
/// itself.
#[test]
fn test_every_port_parses_without_warnings() {
    for (name, source) in ports() {
        let (_, warnings) = ThemeFile::parse(&source)
            .unwrap_or_else(|e| panic!("ports/{name} does not parse: {e}"));
        assert!(
            warnings.is_empty(),
            "ports/{name} parses with warnings: {}",
            warnings
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
}

/// Complete on both halves. A partial `[syntax]` table would paint the unnamed
/// captures in `text`, which on a port would look like the upstream theme
/// losing half its colours.
#[test]
fn test_every_port_sets_every_role() {
    for (name, source) in ports() {
        let (theme, _) = ThemeFile::parse(&source).expect("parses");
        assert_eq!(
            theme.overrides.semantic_len(),
            THEMEABLE_ROLES.len(),
            "ports/{name} sets {} of {} [colors] roles",
            theme.overrides.semantic_len(),
            THEMEABLE_ROLES.len()
        );
        assert_eq!(
            theme.overrides.capture_len(),
            SYNTAX_ROLES.len(),
            "ports/{name} sets {} of {} [syntax] roles",
            theme.overrides.capture_len(),
            SYNTAX_ROLES.len()
        );
    }
}

/// The one that earns this file. Every role a port ships under WCAG AA must be
/// declared in that file's own header, and every role it declares must really
/// be under. Both directions matter: the first stops an illegible colour
/// arriving unannounced, the second stops the header becoming folklore that
/// outlives the colour it described.
#[test]
fn test_every_port_declares_exactly_its_own_illegible_roles() {
    for (name, source) in ports() {
        let (theme, _) = ThemeFile::parse(&source).expect("parses");
        let declared = declared_under_aa(&source);
        let actual = actual_under_aa(&theme);

        let undeclared: Vec<&String> = actual.difference(&declared).collect();
        assert!(
            undeclared.is_empty(),
            "ports/{name} has {} role(s) under WCAG AA that its `under-aa:` line \
             does not mention: {undeclared:?} — a port may be illegible, but not \
             silently",
            undeclared.len()
        );

        let stale: Vec<&String> = declared.difference(&actual).collect();
        assert!(
            stale.is_empty(),
            "ports/{name} declares {stale:?} under WCAG AA, but the lint no longer \
             agrees — the header has outlived the colour"
        );
    }
}

/// Between them the ports must cover both appearances. A light port is the
/// only one of these files that exercises the light reference background, and
/// `gruvbox-light` is the reason the light branch of the mapping was found to
/// be inverted at all.
#[test]
fn test_the_ports_cover_both_appearances() {
    let appearances: Vec<highlight::Variant> = ports()
        .iter()
        .map(|(_, source)| ThemeFile::parse(source).expect("parses").0.appearance)
        .collect();
    assert!(
        appearances.contains(&highlight::Variant::Dark),
        "no dark port"
    );
    assert!(
        appearances.contains(&highlight::Variant::Light),
        "no light port"
    );
}
