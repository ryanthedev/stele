//! CLI surface: `stele <file.md>`, plus flag plumbing consumed here (parsed
//! and stored) or by later phases (parsed and stored, not yet acted on).

use std::path::PathBuf;

use clap::Parser;

/// `stele <file.md>` — a terminal markdown viewer.
#[derive(Debug, Parser)]
#[command(
    name = "stele",
    version,
    about = "A terminal markdown viewer for Ghostty"
)]
pub struct Cli {
    /// The markdown file to open.
    pub file: PathBuf,

    /// Clamps content width to at most this many cells (default 100; see
    /// `layout::LayoutConfig`).
    #[arg(long)]
    pub max_width: Option<u16>,

    /// Reserved for P6: disables image rendering. Parsed and stored here;
    /// not yet acted on.
    #[arg(long)]
    pub no_images: bool,

    /// Reserved for P7: shows YAML frontmatter instead of hiding it.
    /// Parsed and stored here; not yet acted on.
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
