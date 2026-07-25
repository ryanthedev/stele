//! CLI surface: `stele <file.md>` and its flags. Parsing only — every flag is
//! acted on by `main.rs`, which is where the flag doc comments below point.

use std::path::PathBuf;

use clap::Parser;

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
    /// The markdown file to open.
    pub file: PathBuf,

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
    }

    #[test]
    fn test_flags_are_parsed_and_stored() {
        let cli = Cli::parse_from(["stele", "file.md", "--no-images", "--frontmatter"]);
        assert!(cli.no_images);
        assert!(cli.frontmatter);
    }
}
