//! Every theme in `themes/` must load. A shipped theme that does not is a
//! bug the user finds in their terminal, which is the worst place to find it —
//! so it fails here instead.
//!
//! The lint half is deliberately strict. These are the files people will copy
//! to start their own, so a warning in one of them teaches the warning is
//! normal.

use std::fs;
use std::path::PathBuf;

use highlight::{SYNTAX_ROLES, THEMEABLE_ROLES, ThemeFile, ThemeWarning};

fn themes_dir() -> PathBuf {
    // `CARGO_MANIFEST_DIR` is `crates/stele`; the themes live at the root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("themes")
}

fn shipped_themes() -> Vec<(String, String)> {
    let dir = themes_dir();
    let mut themes: Vec<(String, String)> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("no themes directory at {}: {e}", dir.display()))
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
    assert!(!themes.is_empty(), "no themes found in {}", dir.display());
    themes
}

/// DW-4.2: a broken shipped theme fails the suite rather than a user's day.
#[test]
fn test_every_shipped_theme_parses_without_warnings() {
    for (name, source) in shipped_themes() {
        let (_, warnings) = ThemeFile::parse(&source)
            .unwrap_or_else(|e| panic!("themes/{name} does not parse: {e}"));
        assert!(
            warnings.is_empty(),
            "themes/{name} parses with warnings: {}",
            warnings
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
}

/// DW-4.1: every shipped theme is legible. Contrast is the hard half of the
/// lint and these files are what people copy to start their own, so an
/// illegible one propagates.
///
/// 256-colour collisions are deliberately *not* asserted away, and that is a
/// property of the format rather than a gap in these themes. The xterm cube
/// gives six steps per channel — 216 colours, of which only six are greys —
/// so a palette coherent enough to read as one theme cannot also be 33-ways
/// distinct once quantized. `build_palette` only manages it for the built-ins
/// by *generating* colours through a dedup loop, which is why its own heading
/// ramp had to be hand-picked and exempted. Each theme documents the merge in
/// a header comment; what must never regress is legibility.
#[test]
fn test_every_shipped_theme_is_legible() {
    for (name, source) in shipped_themes() {
        let (theme, _) = ThemeFile::parse(&source).expect("parses");
        let illegible: Vec<String> = theme
            .lint()
            .iter()
            .filter(|w| matches!(w, ThemeWarning::LowContrast { .. }))
            .map(ToString::to_string)
            .collect();
        assert!(
            illegible.is_empty(),
            "themes/{name} has {} role(s) under their contrast floor: {}",
            illegible.len(),
            illegible.join("; ")
        );
    }
}

/// A shipped theme should be a *complete* worked example — someone reading one
/// to learn the format should see every role they can set, not a subset that
/// leaves them guessing which names exist.
#[test]
fn test_every_shipped_theme_sets_every_role() {
    for (name, source) in shipped_themes() {
        let (theme, _) = ThemeFile::parse(&source).expect("parses");
        assert_eq!(
            theme.overrides.semantic_len(),
            THEMEABLE_ROLES.len(),
            "themes/{name} sets {} of {} roles — a shipped theme is also the \
             documentation, so it should show all of them",
            theme.overrides.semantic_len(),
            THEMEABLE_ROLES.len()
        );
    }
}

/// The same completeness rule for `[syntax]`, and here it is more than
/// documentation: naming one capture hands the theme the whole code block, so
/// a shipped theme that set half the table would paint the other half in
/// `text` and look broken in exactly the place people judge a theme.
#[test]
fn test_every_shipped_theme_sets_every_syntax_role() {
    for (name, source) in shipped_themes() {
        let (theme, _) = ThemeFile::parse(&source).expect("parses");
        assert_eq!(
            theme.overrides.capture_len(),
            SYNTAX_ROLES.len(),
            "themes/{name} sets {} of {} syntax roles — a partial [syntax] table \
             paints the rest in `text`",
            theme.overrides.capture_len(),
            SYNTAX_ROLES.len()
        );
    }
}

/// Between them the shipped themes must cover both appearances, or the light
/// path ships untested by any real file.
#[test]
fn test_the_shipped_themes_cover_both_appearances() {
    let appearances: Vec<highlight::Variant> = shipped_themes()
        .iter()
        .map(|(_, source)| ThemeFile::parse(source).expect("parses").0.appearance)
        .collect();
    assert!(
        appearances.contains(&highlight::Variant::Dark),
        "no shipped dark theme"
    );
    assert!(
        appearances.contains(&highlight::Variant::Light),
        "no shipped light theme"
    );
}

/// DW-4.4: the documented role list cannot rot away from the real one.
///
/// `docs/theming.md` is where someone learns what they may name, so a role
/// missing from it is a colour nobody knows they can set, and a role listed
/// but removed is advice that silently stops working. Neither fails anything
/// else in this suite, which is exactly why this exists.
#[test]
fn test_the_theming_doc_lists_exactly_the_themeable_roles() {
    let doc = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("docs")
            .join("theming.md"),
    )
    .expect("docs/theming.md");

    for role in THEMEABLE_ROLES {
        // Backticked, so prose that happens to contain the word does not
        // count as documenting the role.
        let listed = doc.contains(&format!("`{role}`"))
            // The six headings are documented as a range rather than one by one.
            || (role.starts_with("heading") && doc.contains("`heading1`–`heading6`"));
        assert!(
            listed,
            "docs/theming.md never mentions `{role}` — a role nobody knows they can set"
        );
    }

    for role in SYNTAX_ROLES {
        assert!(
            doc.contains(&format!("`{role}`")),
            "docs/theming.md never mentions `{role}` — a syntax colour nobody \
             knows they can set"
        );
    }
}
