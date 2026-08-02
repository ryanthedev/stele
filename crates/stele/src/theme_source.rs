//! Finding a user's theme file and turning it into the [`highlight::Theme`]
//! the painter resolves through.
//!
//! `crates/highlight` owns what a theme *is* and refuses to touch a
//! filesystem; this module is the other half — where the file lives, which
//! one wins, and what `T` means once one is loaded. The split is what lets
//! the parser be tested against hostile input with no fixture directory.
//!
//! Two ways in, and they differ deliberately in how loudly they fail. A
//! `--theme` path was typed by the user this run, so a missing one is an
//! error: they asked for something specific and silently ignoring it would
//! leave them staring at the wrong colours wondering why. The config path is
//! the opposite — almost nobody has one, and it must cost nothing to not
//! have.

use std::path::{Path, PathBuf};
use std::{env, fmt, fs, io};

use highlight::{ColorMode, Theme, ThemeError, ThemeFile, Variant};
use layout::Chrome;

/// Where a theme lives when the user has not named one.
const CONFIG_RELATIVE_PATH: &str = "stele/theme.toml";

/// A theme file named explicitly that could not be used.
///
/// Only reachable from `--theme`. The config path never produces one of
/// these — a missing or unreadable config is a warning at most.
#[derive(Debug)]
pub enum ThemeLoadError {
    Unreadable { path: PathBuf, source: io::Error },
    Invalid { path: PathBuf, source: ThemeError },
}

impl fmt::Display for ThemeLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThemeLoadError::Unreadable { path, source } => {
                write!(f, "could not read theme {}: {source}", path.display())
            }
            ThemeLoadError::Invalid { path, source } => {
                write!(f, "{}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for ThemeLoadError {}

/// The theme a session runs with, and everything `T` needs to flip it.
///
/// Holds the parsed file rather than a built [`Theme`] because the variant
/// changes at runtime and the palette is rebuilt per variant anyway; keeping
/// the overrides lets `T` swap the base out from under them.
#[derive(Debug)]
pub struct ThemeSource {
    file: Option<ThemeFile>,
    color_mode: ColorMode,
    /// The variant currently in force.
    variant: Variant,
    /// Whether the user theme's colours are applied right now. Always false
    /// when `file` is `None`; toggled by [`ThemeSource::toggle`].
    user_active: bool,
    /// Everything worth telling the user, already rendered to strings —
    /// `ThemeWarning` lives in `highlight` and the status line only needs the
    /// text.
    warnings: Vec<String>,
}

impl ThemeSource {
    /// Resolves which theme to use.
    ///
    /// `explicit` is `--theme`; `config` is the default path, which callers
    /// get from [`default_config_path`] and tests supply directly. `detected`
    /// is the variant the OSC 11 reply implied, used only when no theme file
    /// declares an appearance of its own.
    pub fn load(
        explicit: Option<&Path>,
        config: Option<&Path>,
        detected: Variant,
        color_mode: ColorMode,
    ) -> Result<ThemeSource, ThemeLoadError> {
        let mut warnings = Vec::new();

        // An explicitly named theme is the only one whose absence is fatal.
        let loaded = match explicit {
            Some(path) => Some(
                read_theme(path, &mut warnings).map_err(|source| match source {
                    ReadFailure::Io(source) => ThemeLoadError::Unreadable {
                        path: path.to_path_buf(),
                        source,
                    },
                    ReadFailure::Parse(source) => ThemeLoadError::Invalid {
                        path: path.to_path_buf(),
                        source,
                    },
                })?,
            ),
            None => match config {
                // A config path that is simply absent is the common case and
                // says nothing. One that exists but will not load is worth a
                // warning — the user did write it.
                Some(path) if path.is_file() => match read_theme(path, &mut warnings) {
                    Ok(theme) => Some(theme),
                    Err(failure) => {
                        warnings.push(format!("{}: {failure}", path.display()));
                        None
                    }
                },
                _ => None,
            },
        };

        let variant = loaded.as_ref().map_or(detected, |t| t.appearance);
        Ok(ThemeSource {
            user_active: loaded.is_some(),
            file: loaded,
            color_mode,
            variant,
            warnings,
        })
    }

    /// A source with no user theme — the shape every existing caller had
    /// before theme files existed.
    pub fn built_in(variant: Variant, color_mode: ColorMode) -> ThemeSource {
        ThemeSource {
            file: None,
            color_mode,
            variant,
            user_active: false,
            warnings: Vec::new(),
        }
    }

    /// The theme to paint with right now.
    pub fn theme(&self) -> Theme {
        match (self.user_active, &self.file) {
            (true, Some(file)) => {
                Theme::with_overrides(self.variant, self.color_mode, file.overrides.clone())
            }
            _ => Theme::new(self.variant, self.color_mode),
        }
    }

    /// What `T` does.
    ///
    /// With no theme file this is the dark/light flip it has always been.
    /// With one, it swaps between the user's theme and the *built-in* of the
    /// opposite appearance — which is what `T` is actually for: the room got
    /// bright, and the escape hatch has to lead somewhere legible rather than
    /// to the same theme's other half, which a single-appearance file does
    /// not have.
    pub fn toggle(&mut self) {
        match &self.file {
            Some(file) if self.user_active => {
                self.user_active = false;
                self.variant = opposite(file.appearance);
            }
            Some(file) => {
                self.user_active = true;
                self.variant = file.appearance;
            }
            None => self.variant = opposite(self.variant),
        }
    }

    /// The theme's own name, when a user theme is loaded and named.
    pub fn name(&self) -> Option<&str> {
        self.file
            .as_ref()
            .map(|f| f.name.as_str())
            .filter(|n| !n.is_empty())
    }

    pub fn variant(&self) -> Variant {
        self.variant
    }

    pub fn color_mode(&self) -> ColorMode {
        self.color_mode
    }

    /// Whether a user theme's colours are currently applied.
    pub fn user_active(&self) -> bool {
        self.user_active && self.file.is_some()
    }

    /// The furniture the loaded theme asked for, or the default when there is
    /// no theme file.
    ///
    /// Read from the file whether or not [`user_active`](Self::user_active) is
    /// set, which is the one place geometry deliberately parts company with
    /// colour. `T` swaps a palette; it is a lighting change, and a reader who
    /// hits it because the room got bright is not asking for their gutter to
    /// disappear and every line on the page to move two cells left. The
    /// colours the gutter is painted *in* still follow `T` like everything
    /// else, because those are colours.
    pub fn chrome(&self) -> Chrome {
        self.file
            .as_ref()
            .map_or_else(Chrome::default, |f| f.chrome)
    }

    /// Problems to surface, worst-first is not meaningful here so they stay
    /// in discovery order — the order someone reading their own file expects.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// The one line the status row gets, and a count when there is more.
    ///
    /// A status row is one line: pouring six warnings into it truncates the
    /// first and hides the rest, which is worse than saying how many there
    /// are and showing one.
    pub fn status_summary(&self) -> Option<String> {
        let first = self.warnings.first()?;
        Some(match self.warnings.len() {
            1 => format!("theme: {first}"),
            n => format!("theme: {first} (+{} more)", n - 1),
        })
    }
}

/// Which built-in sits at the other end from `variant`. Exhaustive with no
/// wildcard, so a third variant is a compile error here rather than a `T`
/// that silently stops reaching one of them.
fn opposite(variant: Variant) -> Variant {
    match variant {
        Variant::Dark => Variant::Light,
        Variant::Light => Variant::Dark,
    }
}

enum ReadFailure {
    Io(io::Error),
    Parse(ThemeError),
}

impl fmt::Display for ReadFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadFailure::Io(e) => write!(f, "{e}"),
            ReadFailure::Parse(e) => write!(f, "{e}"),
        }
    }
}

fn read_theme(path: &Path, warnings: &mut Vec<String>) -> Result<ThemeFile, ReadFailure> {
    let source = fs::read_to_string(path).map_err(ReadFailure::Io)?;
    let (theme, parse_warnings) = ThemeFile::parse(&source).map_err(ReadFailure::Parse)?;
    // Lint runs here rather than inside `parse` because it answers a
    // different question — "can I read this file" versus "will you be able to
    // read the result" — and only the first one is the parser's business.
    for warning in parse_warnings.iter().chain(theme.lint().iter()) {
        warnings.push(warning.to_string());
    }
    Ok(theme)
}

/// Where a theme lives when the user has not named one: `$XDG_CONFIG_HOME`
/// if set, else `$HOME/.config`. `None` when neither is set, which is a
/// perfectly ordinary state (a bare `env -i`) and must not panic.
pub fn default_config_path() -> Option<PathBuf> {
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg).join(CONFIG_RELATIVE_PATH));
    }
    let home = env::var_os("HOME").filter(|v| !v.is_empty())?;
    Some(
        PathBuf::from(home)
            .join(".config")
            .join(CONFIG_RELATIVE_PATH),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use highlight::StyleId;
    use layout::Semantic;

    fn scratch(tag: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("stele-theme-{tag}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, body).expect("write theme");
        path
    }

    /// The whole point: a file's colour reaches the resolved theme.
    #[test]
    fn test_an_explicit_theme_colours_the_document() {
        let dir = scratch("explicit");
        let path = write(
            &dir,
            "t.toml",
            "name = \"T\"\nappearance = \"dark\"\n[colors]\ntext = \"#aabbcc\"\n",
        );
        let source = ThemeSource::load(Some(&path), None, Variant::Dark, ColorMode::Truecolor)
            .expect("loads");
        let style = source.theme().resolve(StyleId::Semantic(Semantic::Text));
        assert_eq!(style.fg, Some(highlight::Color::new(0xaa, 0xbb, 0xcc)));
        assert_eq!(source.name(), Some("T"));
    }

    /// With nothing configured, the resolved theme must be exactly the
    /// built-in — this is what keeps the no-config path unchanged for
    /// everyone who never writes a theme.
    #[test]
    fn test_no_theme_anywhere_resolves_to_the_built_in() {
        let source =
            ThemeSource::load(None, None, Variant::Dark, ColorMode::Truecolor).expect("loads");
        assert!(!source.user_active());
        assert!(source.warnings().is_empty());
        let built_in = Theme::new(Variant::Dark, ColorMode::Truecolor);
        let resolved = source.theme();
        for level in 1..=6 {
            let id = StyleId::Semantic(Semantic::Heading(level));
            assert_eq!(resolved.resolve(id), built_in.resolve(id));
        }
        assert_eq!(
            resolved.resolve(StyleId::Semantic(Semantic::Text)).fg,
            None,
            "body text must still inherit the terminal foreground"
        );
    }

    /// A path the user typed must not fail quietly.
    #[test]
    fn test_an_explicit_path_that_is_missing_is_an_error() {
        let missing = scratch("missing").join("nope.toml");
        let err = ThemeSource::load(Some(&missing), None, Variant::Dark, ColorMode::Truecolor)
            .expect_err("must fail");
        assert!(
            err.to_string().contains("nope.toml"),
            "the message must name the path: {err}"
        );
    }

    /// A path the user never typed must fail silently — nearly nobody has a
    /// config file, and a warning for its absence would be noise for all of
    /// them.
    #[test]
    fn test_a_missing_config_path_is_silent() {
        let missing = scratch("silent").join("theme.toml");
        let source = ThemeSource::load(None, Some(&missing), Variant::Light, ColorMode::Truecolor)
            .expect("loads");
        assert!(source.warnings().is_empty(), "{:?}", source.warnings());
        assert_eq!(
            source.variant(),
            Variant::Light,
            "with no theme, the detected variant stands"
        );
    }

    /// A config file that exists but is broken is worth saying so about — the
    /// user did write it — but must not cost them the document.
    #[test]
    fn test_a_broken_config_theme_warns_and_falls_back() {
        let dir = scratch("broken");
        let path = write(&dir, "theme.toml", "name = \"unclosed");
        let source = ThemeSource::load(None, Some(&path), Variant::Dark, ColorMode::Truecolor)
            .expect("a broken config must not be fatal");
        assert!(!source.user_active());
        assert!(!source.warnings().is_empty(), "a broken config must warn");
    }

    /// A directory where a theme should be is an I/O error, not a panic.
    #[test]
    fn test_a_directory_in_place_of_a_theme_is_handled() {
        let dir = scratch("isdir");
        assert!(
            ThemeSource::load(Some(&dir), None, Variant::Dark, ColorMode::Truecolor).is_err(),
            "a directory must be an error"
        );
        // And via the config path, where `is_file()` rejects it silently.
        let source = ThemeSource::load(None, Some(&dir), Variant::Dark, ColorMode::Truecolor)
            .expect("must not be fatal");
        assert!(!source.user_active());
    }

    /// A theme's declared appearance decides which built-in it lays over,
    /// beating whatever the terminal's background implied — the file is the
    /// more specific statement.
    #[test]
    fn test_a_themes_appearance_beats_the_detected_variant() {
        let dir = scratch("appearance");
        let path = write(
            &dir,
            "t.toml",
            "appearance = \"light\"\n[colors]\ntext = \"#222\"\n",
        );
        let source = ThemeSource::load(Some(&path), None, Variant::Dark, ColorMode::Truecolor)
            .expect("loads");
        assert_eq!(source.variant(), Variant::Light);
    }

    /// `T` with a user theme has to reach somewhere legible. A single-
    /// appearance file has no other half, so the escape hatch is the built-in
    /// at the opposite end.
    #[test]
    fn test_toggling_a_user_theme_reaches_the_opposite_built_in_and_back() {
        let dir = scratch("toggle");
        let path = write(
            &dir,
            "t.toml",
            "appearance = \"dark\"\n[colors]\ntext = \"#aabbcc\"\n",
        );
        let mut source = ThemeSource::load(Some(&path), None, Variant::Dark, ColorMode::Truecolor)
            .expect("loads");
        assert!(source.user_active());
        assert_eq!(source.variant(), Variant::Dark);

        source.toggle();
        assert!(!source.user_active(), "T must leave the user theme");
        assert_eq!(source.variant(), Variant::Light);
        assert_eq!(
            source.theme().resolve(StyleId::Semantic(Semantic::Text)).fg,
            None,
            "the built-in must not carry the user's body colour"
        );

        source.toggle();
        assert!(source.user_active(), "T must come back");
        assert_eq!(source.variant(), Variant::Dark);
        assert_eq!(
            source.theme().resolve(StyleId::Semantic(Semantic::Text)).fg,
            Some(highlight::Color::new(0xaa, 0xbb, 0xcc))
        );
    }

    /// With no theme file, `T` is exactly the flip it always was.
    #[test]
    fn test_toggling_without_a_theme_is_the_original_dark_light_flip() {
        let mut source = ThemeSource::built_in(Variant::Dark, ColorMode::Truecolor);
        source.toggle();
        assert_eq!(source.variant(), Variant::Light);
        source.toggle();
        assert_eq!(source.variant(), Variant::Dark);
    }

    /// `NO_COLOR` outranks a theme file. A theme is a set of colours, and a
    /// file cannot be a way around asking for none.
    #[test]
    fn test_no_color_outranks_a_theme_file() {
        let dir = scratch("nocolor");
        let path = write(
            &dir,
            "t.toml",
            "appearance = \"dark\"\n[colors]\ntext = \"#aabbcc\"\n",
        );
        let source =
            ThemeSource::load(Some(&path), None, Variant::Dark, ColorMode::NoColor).expect("loads");
        assert_eq!(
            source.theme().resolve(StyleId::Semantic(Semantic::Text)).fg,
            None
        );
    }

    /// The status row is one line. Six warnings must become one line plus a
    /// count, not a truncated wall.
    #[test]
    fn test_many_warnings_become_one_line_and_a_count() {
        let dir = scratch("summary");
        let path = write(
            &dir,
            "t.toml",
            "[colors]\nnope1 = \"#fff\"\nnope2 = \"#fff\"\nnope3 = \"#fff\"\n",
        );
        let source = ThemeSource::load(Some(&path), None, Variant::Dark, ColorMode::Truecolor)
            .expect("loads");
        let summary = source.status_summary().expect("warnings exist");
        assert!(summary.starts_with("theme: "), "{summary}");
        assert!(summary.contains("more)"), "{summary}");
        assert_eq!(summary.lines().count(), 1, "the status row is one line");
    }

    /// `$HOME` unset is an ordinary state, not a crash.
    #[test]
    fn test_a_config_path_is_optional_rather_than_assumed() {
        // Not asserting on the live environment — just that the function is
        // total and returns a `stele/theme.toml` when it returns anything.
        if let Some(path) = default_config_path() {
            assert!(path.ends_with(CONFIG_RELATIVE_PATH), "{}", path.display());
        }
    }
}
