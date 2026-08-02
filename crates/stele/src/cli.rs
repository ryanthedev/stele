//! CLI surface: `stele <file.md>` and its flags. Parsing and the one
//! cross-flag rule clap cannot express ([`Cli::source`]) — every other flag is
//! acted on by `main.rs`, which is where the flag doc comments below point.

use std::fmt;
use std::path::{Path, PathBuf};

use clap::Parser;

use crate::loader::DocumentSource;

/// Commit + build time this binary was produced from, stamped by `build.rs`.
/// Shown by `--version` and painted in the viewport corner, so "the fix does
/// not work" can always be told apart from "that is not the fix you built."
pub const BUILD: &str = env!("STELE_BUILD");

/// Just the short sha (plus `-dirty`), for the in-viewport stamp where
/// columns are scarce.
pub const BUILD_SHA: &str = env!("STELE_BUILD_SHA");

/// `stele <file.md>` — a terminal markdown viewer.
#[derive(Debug, Parser)]
#[command(
    name = "stele",
    version,
    long_version = BUILD,
    about = "A terminal markdown viewer for Ghostty"
)]
pub struct Cli {
    /// The markdown file to open, or `-` to read the document from stdin.
    /// With `-`, keys are read from `/dev/tty` instead (see
    /// [`Cli::source`]).
    pub file: PathBuf,

    /// Reloads the document whenever the file changes on disk, preserving
    /// the scroll anchor. Polled on the event loop's own timeout — no
    /// filesystem watcher, no extra thread. Meaningless with `-`, and
    /// rejected in combination with it (DW-2.3).
    #[arg(long)]
    pub watch: bool,

    /// Clamps content width to at most this many cells (default 100; see
    /// `layout::LayoutConfig`).
    #[arg(long)]
    pub max_width: Option<u16>,

    /// Disables image and math rendering: alt text / TeX source is shown
    /// instead. `main.rs` folds this into `graphics_disabled`, which selects
    /// `ImageSizer::disabled` and `NoopMediaSink`, so no box is ever reserved
    /// and the media sink is never invoked.
    #[arg(long)]
    pub no_images: bool,

    /// Shows YAML frontmatter as ordinary content instead of hiding it.
    /// `main.rs` passes this to `decor::frontmatter::apply` before the parse.
    #[arg(long)]
    pub frontmatter: bool,

    /// Uses the theme in this file instead of the built-in colors, and
    /// instead of `~/.config/stele/theme.toml` if one is there.
    ///
    /// Unlike the config path, a file named here must exist and load: you
    /// asked for these colors specifically, so being quietly given different
    /// ones would be the wrong kindness. See `crate::theme_source`.
    #[arg(long, value_name = "FILE")]
    pub theme: Option<PathBuf>,

    /// Numbers every rendered row in a gutter down the left of the page.
    ///
    /// Overrides `line_numbers` in the theme's `[layout]` table, so a theme
    /// that turns the gutter off cannot stop you asking for it on this run.
    /// The numbers count *rendered* rows, not source lines — see
    /// `docs/theming.md`.
    #[arg(long)]
    pub line_numbers: bool,

    /// Strips every piece of furniture for this run: no padding, no gutter,
    /// no band under the reading line, whatever the theme says.
    ///
    /// The escape hatch for piping, for recording a clean screenshot, and for
    /// the reader who wants the page and nothing else. Wins over
    /// `--line-numbers` if both are given, because "none of it" is the more
    /// specific request.
    #[arg(long)]
    pub no_chrome: bool,
}

/// A combination of flags that parse individually but cannot mean anything
/// together. Separate from [`crate::loader::LoadError`] on purpose: this one
/// is decided before a single byte is read, and before the terminal is
/// touched at all.
#[derive(Debug, PartialEq, Eq)]
pub enum CliError {
    /// `--watch -`. Every word of the message names one half of the
    /// conflict, so the reader is not left to infer which flag to drop.
    WatchStdin,
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::WatchStdin => write!(
                f,
                "--watch cannot be combined with `-`: stdin is a stream read \
                 once to end, not a file whose changes can be watched"
            ),
        }
    }
}

impl std::error::Error for CliError {}

/// The conventional argument for "read the document from standard input".
const STDIN_ARG: &str = "-";

impl Cli {
    /// Which [`DocumentSource`] these arguments name, or why they name none.
    ///
    /// This is where the `-` convention lives, and the only place: `main`
    /// never compares the path to `"-"`, and neither does the loader. The
    /// rejected combination is checked here rather than left to clap's
    /// `conflicts_with` because the conflict is with a positional *value*
    /// (`-`), not with another flag — clap has no rule for that.
    pub fn source(&self) -> Result<DocumentSource, CliError> {
        if self.file != Path::new(STDIN_ARG) {
            return Ok(DocumentSource::Path(self.file.clone()));
        }
        if self.watch {
            return Err(CliError::WatchStdin);
        }
        Ok(DocumentSource::Stdin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dw_5_6_cli_parses_max_width_flag() {
        let cli = Cli::parse_from(["stele", "file.md", "--max-width", "60"]);
        assert_eq!(cli.max_width, Some(60));
        assert_eq!(cli.file, std::path::PathBuf::from("file.md"));
    }

    #[test]
    fn test_max_width_defaults_to_none_when_absent() {
        let cli = Cli::parse_from(["stele", "file.md"]);
        assert_eq!(cli.max_width, None);
        assert!(!cli.no_images);
        assert!(!cli.frontmatter);
        assert!(!cli.watch);
    }

    #[test]
    fn test_dw_2_1_a_bare_dash_names_the_stdin_source() {
        let cli = Cli::parse_from(["stele", "-"]);
        assert_eq!(cli.source(), Ok(DocumentSource::Stdin));
    }

    #[test]
    fn test_any_other_path_names_a_file_source_dash_or_not() {
        // `-` is only the stdin convention when it is the *whole* argument;
        // a file that merely starts with one is still a file.
        assert_eq!(
            Cli::parse_from(["stele", "notes.md"]).source(),
            Ok(DocumentSource::Path(PathBuf::from("notes.md")))
        );
        assert_eq!(
            Cli::parse_from(["stele", "--", "-notes.md"]).source(),
            Ok(DocumentSource::Path(PathBuf::from("-notes.md")))
        );
    }

    /// DW-2.3: the two cannot mean anything together, and the message must
    /// name *both* halves — a reader told only "invalid arguments" has to
    /// guess which one to drop.
    #[test]
    fn test_dw_2_3_watch_with_stdin_is_rejected_naming_both_flags() {
        let cli = Cli::parse_from(["stele", "-", "--watch"]);
        let err = cli.source().unwrap_err();
        assert_eq!(err, CliError::WatchStdin);

        let message = err.to_string();
        assert!(message.contains("--watch"), "message was: {message}");
        assert!(message.contains('`'), "message was: {message}");
        assert!(message.contains("stdin"), "message was: {message}");
    }

    /// The rejection is about the *combination*: each half alone is fine.
    #[test]
    fn test_dw_2_3_watch_on_a_file_and_stdin_without_watch_both_resolve() {
        assert_eq!(
            Cli::parse_from(["stele", "notes.md", "--watch"]).source(),
            Ok(DocumentSource::Path(PathBuf::from("notes.md")))
        );
        assert_eq!(
            Cli::parse_from(["stele", "-"]).source(),
            Ok(DocumentSource::Stdin)
        );
    }

    #[test]
    fn test_flags_are_parsed_and_stored() {
        let cli = Cli::parse_from(["stele", "file.md", "--no-images", "--frontmatter"]);
        assert!(cli.no_images);
        assert!(cli.frontmatter);
    }
}
