//! CLI surface: `stele <file.md>` and its flags. Parsing and the two
//! cross-flag/cross-filesystem rules clap cannot express ([`Cli::source`],
//! [`Cli::start`]) — every other flag is acted on by `main.rs`, which is
//! where the flag doc comments below point.

use std::fmt;
use std::path::{Path, PathBuf};

use clap::Parser;

use crate::loader::{DocumentSource, LoadOptions};

/// Commit + build time this binary was produced from, stamped by `build.rs`.
/// Carried by `--version` and painted in the viewport corner, so "the fix does
/// not work" can always be told apart from "that is not the fix you built."
pub const BUILD: &str = env!("STELE_BUILD");

/// Just the short sha (plus `-dirty`), for the in-viewport stamp where
/// columns are scarce.
pub const BUILD_SHA: &str = env!("STELE_BUILD_SHA");

/// What `--version` prints: the release, then the build it came from —
/// `0.2.0 (abc1234 2026-08-07T…)`. A bug report needs both, and only one of
/// them was reachable before. `-V` stays the bare release number, which is
/// what a script grepping for a version wants.
pub const LONG_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("STELE_BUILD"), ")");

/// `stele <file.md>` — a terminal markdown viewer.
#[derive(Debug, Parser)]
#[command(
    name = "stele",
    version,
    long_version = LONG_VERSION,
    about = "A terminal markdown viewer for Ghostty"
)]
pub struct Cli {
    /// The markdown file to open, `-` to read the document from stdin, a
    /// directory to browse, or omitted to browse the current directory.
    /// With `-`, keys are read from `/dev/tty` instead (see
    /// [`Cli::source`]). See [`Cli::start`] for how a bare directory is told
    /// apart from a file.
    pub file: Option<PathBuf>,

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

    /// Draws no images and no formulae: an image shows its alt text and a
    /// formula its TeX source, in the flow of the page. Nothing is
    /// transmitted to the terminal and no space is reserved, so the page is
    /// exactly as tall as its text.
    #[arg(long)]
    pub no_images: bool,

    /// Shows YAML frontmatter as ordinary content instead of hiding it.
    /// `main.rs` passes this to `decor::frontmatter::apply` before the parse.
    #[arg(long)]
    pub frontmatter: bool,

    /// Reads the file as written, without rewriting Dive-into-Deep-Learning
    /// roles, Quarto callouts or CodeCogs equation images into their plain
    /// markdown equivalents.
    ///
    /// Those rewrites delete and reshape source lines, which is a large
    /// thing to do quietly — so, like `--frontmatter`, they come with a way
    /// to say no. `--no-rewrite` leaves frontmatter hiding and mermaid
    /// rendering alone; both predate this flag and neither is changing.
    #[arg(long)]
    pub no_rewrite: bool,

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

    /// Downloads `![alt](https://…)` images and draws them, instead of
    /// showing their alt text.
    ///
    /// **Off unless you type it, and that is a stance rather than caution.**
    /// A markdown file whose images are URLs is a file that reports who read
    /// it, when, and from where, to whoever wrote it — opening a document is
    /// not consent to be counted. So stele asks for nothing by default and
    /// makes exactly zero requests until this flag says otherwise.
    ///
    /// What it fetches is bounded on every axis (`http`/`https` only, a
    /// connect and a request timeout, a whole-document budget, a response-size
    /// ceiling and a redirect cap), cached under `$XDG_CACHE_HOME/stele` so a
    /// second read costs nothing, and allowed to fail: an image that times
    /// out, 404s or comes back as something other than a picture goes back to
    /// being its alt text, and never stops the document opening.
    ///
    /// Composes with `--no-rewrite` rather than being cancelled by it — see
    /// `README.md`.
    #[cfg(feature = "remote-images")]
    #[arg(long)]
    pub fetch_remote: bool,

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
/// together, or a filesystem fact [`Cli::start`] needs settled before the
/// terminal is touched at all. Separate from [`crate::loader::LoadError`] on
/// purpose: every variant here is decided before a single document byte is
/// read.
#[derive(Debug)]
pub enum CliError {
    /// `--watch -`. Every word of the message names one half of the
    /// conflict, so the reader is not left to infer which flag to drop.
    WatchStdin,
    /// `--watch <directory>`. The same shape as [`CliError::WatchStdin`] —
    /// `--watch` polls a file's mtime, and a directory has no single file to
    /// poll — named the same way, so both the flag and what it was combined
    /// with are in the message.
    WatchDirectory(PathBuf),
    /// The current directory could not be determined at all (e.g. it was
    /// deleted out from under the process) — reached only by a bare `stele`
    /// with no positional argument, since every other path names its own
    /// directory instead of asking the OS for one.
    Cwd(std::io::Error),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::WatchStdin => write!(
                f,
                "--watch cannot be combined with `-`: stdin is a stream read \
                 once to end, not a file whose changes can be watched"
            ),
            CliError::WatchDirectory(path) => write!(
                f,
                "--watch cannot be combined with a directory ({}): there is \
                 no single file to poll for changes",
                path.display()
            ),
            CliError::Cwd(err) => {
                write!(f, "could not determine the current directory: {err}")
            }
        }
    }
}

impl PartialEq for CliError {
    /// Structural equality except for [`CliError::Cwd`]'s `io::Error`, which
    /// does not implement it. Every existing test compares the other two
    /// variants; a `Cwd` test compares the rendered message instead (see
    /// `test_dw_3_16_...`).
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (CliError::WatchStdin, CliError::WatchStdin) => true,
            (CliError::WatchDirectory(a), CliError::WatchDirectory(b)) => a == b,
            (CliError::Cwd(_), CliError::Cwd(_)) => true,
            _ => false,
        }
    }
}

impl std::error::Error for CliError {}

/// What `stele`'s arguments resolved to (Phase 3): a document to load, or a
/// directory to browse. The split point between "read a file" and "walk a
/// filesystem" — everything downstream of [`Cli::start`] acts on one or the
/// other, never on `Cli` itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// Load and render this document — `main.rs`'s pre-Phase-3 path,
    /// unchanged.
    Document(DocumentSource),
    /// Browse this directory instead of loading anything.
    ///
    /// **Absolute and lexically normalized**, whatever was typed:
    /// [`crate::explore::absolute_dir`] runs on the way out of
    /// [`Cli::start`], so `stele .`, `stele docs` and `stele ./docs/` all
    /// arrive here as a path whose parent directory is a real directory
    /// rather than the empty path `Path::parent` answers for a
    /// single-component relative path. No `canonicalize`: a symlinked
    /// directory keeps the name the reader typed.
    Explore(PathBuf),
}

/// The conventional argument for "read the document from standard input".
const STDIN_ARG: &str = "-";

impl Cli {
    /// Which [`DocumentSource`] `self.file` names, or why it names none.
    ///
    /// This is where the `-` convention lives, and the only place: `main`
    /// never compares the path to `"-"`, and neither does the loader. The
    /// rejected combination is checked here rather than left to clap's
    /// `conflicts_with` because the conflict is with a positional *value*
    /// (`-`), not with another flag — clap has no rule for that.
    ///
    /// A missing `file` answers [`DocumentSource::Scratch`] — "these
    /// arguments name no document" is a real, total answer once a bare
    /// `stele` is a valid invocation, not an error this function raises.
    /// [`Cli::start`] never actually reaches this arm (a missing `file`
    /// resolves straight to [`Start::Explore`] at the current directory
    /// without asking `source` at all), but the mapping stays honest for any
    /// other caller.
    pub fn source(&self) -> Result<DocumentSource, CliError> {
        let Some(file) = &self.file else {
            return Ok(DocumentSource::Scratch);
        };
        if file != Path::new(STDIN_ARG) {
            return Ok(DocumentSource::Path(file.clone()));
        }
        if self.watch {
            return Err(CliError::WatchStdin);
        }
        Ok(DocumentSource::Stdin)
    }

    /// Resolves `self.file` to a [`Start`]: a document to load, or a
    /// directory to browse — the one place that distinguishes them, so
    /// `main.rs` never has to `stat` an argument itself.
    ///
    /// **Three cases, decided in this order:**
    /// 1. No argument at all: browse the current directory. Getting *that*
    ///    directory can itself fail (deleted out from under the process),
    ///    which is the one way this function's own I/O can produce
    ///    [`CliError::Cwd`] rather than a filesystem fact about the
    ///    argument.
    /// 2. `-`: unchanged — delegates to [`Cli::source`] for the stdin/
    ///    `--watch` rule.
    /// 3. Anything else: `stat`s the path. A directory becomes
    ///    [`Start::Explore`] (refusing `--watch`, which has no single file
    ///    to poll); anything else — a regular file, and deliberately a path
    ///    that fails to `stat` at all — becomes [`Start::Document`] and is
    ///    left to the ordinary load path to accept or refuse. **That last
    ///    part is load-bearing, not an oversight**: a missing file must keep
    ///    producing exactly the message `DocumentSource::load_with` has
    ///    always produced (`could not read file: …`), not a different one
    ///    minted here before the loader ever runs.
    ///
    /// An unreadable-but-existing directory is deliberately *not* a case
    /// here — [`Start::Explore`] is returned regardless, and
    /// `explore::Listing::read`'s own total, panic-free error handling
    /// (a notice row naming the OS error) is what the reader sees once the
    /// explorer opens. Checking readability here would mean reading the
    /// directory twice, in two places that could disagree.
    pub fn start(&self) -> Result<Start, CliError> {
        let Some(file) = &self.file else {
            let cwd = std::env::current_dir().map_err(CliError::Cwd)?;
            return self.explore_or_watch_conflict(cwd);
        };
        if file == Path::new(STDIN_ARG) {
            return self.source().map(Start::Document);
        }
        match std::fs::metadata(file) {
            Ok(meta) if meta.is_dir() => self.explore_or_watch_conflict(file.clone()),
            // Anything else — a regular file, a device node the loader will
            // refuse in its own way, or a path that does not exist at all —
            // is a `Document`. See this method's doc for why a failed `stat`
            // is not decided here.
            Ok(_) | Err(_) => Ok(Start::Document(DocumentSource::Path(file.clone()))),
        }
    }

    /// [`Start::Explore(dir)`], unless `--watch` was also given — `--watch`
    /// polls one file's mtime, and a directory has none, so the two are
    /// rejected together the same way `--watch -` is. Shared by both routes
    /// [`Cli::start`] can reach a directory from (a bare invocation and an
    /// explicit positional), so the rule cannot drift between them.
    fn explore_or_watch_conflict(&self, dir: PathBuf) -> Result<Start, CliError> {
        if self.watch {
            // The refusal names what the reader typed, not the normalized
            // form: `stele --watch .` should complain about `.`.
            return Err(CliError::WatchDirectory(dir));
        }
        Ok(Start::Explore(crate::explore::absolute_dir(&dir)))
    }

    /// The source-text policy these flags name.
    ///
    /// Lives here rather than in `main.rs` on purpose: `main.rs` is not
    /// unit-tested (its own module doc says so), and "what does the default
    /// command line do about the network" is exactly the question that must
    /// not rest on an untested mapping. With this function the whole chain —
    /// no flag, `None` policy, no call site, no request — is assertable from
    /// `Cli::parse_from`.
    pub fn load_options(&self) -> LoadOptions {
        LoadOptions {
            show_frontmatter: self.frontmatter,
            rewrite_source: !self.no_rewrite,
            remote: self.remote_policy(),
        }
    }

    /// The process-wide fetcher, if `--fetch-remote` was typed.
    #[cfg(feature = "remote-images")]
    fn remote_policy(&self) -> Option<&'static crate::media::RemoteImages> {
        self.fetch_remote.then(crate::media::remote::production)
    }

    /// Without the `remote-images` feature there is no client compiled in, no
    /// flag to type, and therefore no policy — the same `None` the default
    /// command line produces, reached by a route that has no network code
    /// behind it at all.
    #[cfg(not(feature = "remote-images"))]
    fn remote_policy(&self) -> Option<&'static crate::media::RemoteImages> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dw_5_6_cli_parses_max_width_flag() {
        let cli = Cli::parse_from(["stele", "file.md", "--max-width", "60"]);
        assert_eq!(cli.max_width, Some(60));
        assert_eq!(cli.file, Some(std::path::PathBuf::from("file.md")));
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
        let cli = Cli::parse_from([
            "stele",
            "file.md",
            "--no-images",
            "--frontmatter",
            "--no-rewrite",
        ]);
        assert!(cli.no_images);
        assert!(cli.frontmatter);
        assert!(cli.no_rewrite);
        assert!(!Cli::parse_from(["stele", "file.md"]).no_rewrite);
    }

    /// The privacy default, asserted where it is actually decided.
    ///
    /// Not "the flag defaults to false" — that is a fact about clap. This is
    /// the fact that matters: an ordinary command line produces a policy with
    /// **no fetcher in it**, so `loader::preprocess` has nothing to call and
    /// no request can be made however the rest of the pipeline behaves. The
    /// other flags are checked alongside it so a future edit that reshuffles
    /// this mapping cannot quietly swap two fields.
    #[test]
    fn test_an_ordinary_command_line_names_no_fetcher_at_all() {
        let options = Cli::parse_from(["stele", "file.md"]).load_options();
        assert!(
            options.remote.is_none(),
            "the default command line installed a fetcher"
        );
        assert!(!options.show_frontmatter);
        assert!(options.rewrite_source);

        // And neither does any other flag: nothing but `--fetch-remote` may
        // turn the network on, so every one of them is tried here.
        for args in [
            vec!["stele", "file.md", "--frontmatter"],
            vec!["stele", "file.md", "--no-rewrite"],
            vec!["stele", "file.md", "--no-images"],
            vec!["stele", "file.md", "--watch"],
            vec!["stele", "file.md", "--line-numbers"],
            vec!["stele", "file.md", "--no-chrome"],
            vec!["stele", "file.md", "--max-width", "60"],
            vec!["stele", "-"],
        ] {
            assert!(
                Cli::parse_from(args.clone())
                    .load_options()
                    .remote
                    .is_none(),
                "{args:?} installed a fetcher"
            );
        }
    }

    /// The other half: the flag actually reaches the policy. Without this the
    /// test above would pass just as well against a `--fetch-remote` that
    /// parses and does nothing.
    #[cfg(feature = "remote-images")]
    #[test]
    fn test_fetch_remote_is_the_one_flag_that_installs_a_fetcher() {
        let cli = Cli::parse_from(["stele", "file.md", "--fetch-remote"]);
        assert!(cli.fetch_remote);
        assert!(cli.load_options().remote.is_some());

        // It composes with `--no-rewrite` rather than being cancelled by it —
        // the two answer different questions, and `media::remote`'s module doc
        // argues why. A reader who typed both gets both.
        let both =
            Cli::parse_from(["stele", "file.md", "--fetch-remote", "--no-rewrite"]).load_options();
        assert!(
            both.remote.is_some(),
            "--no-rewrite silently cancelled --fetch-remote"
        );
        assert!(!both.rewrite_source);
    }

    /// `--help` is user-facing text. It may name flags, files and what the
    /// reader will see; it may not name the types and structs that happen to
    /// implement any of it — those are ours to rename, and a reader has no
    /// way to look one up.
    #[test]
    fn test_the_help_text_describes_behaviour_and_never_an_internal_name() {
        let help = <Cli as clap::CommandFactory>::command()
            .render_long_help()
            .to_string();
        for internal in [
            "graphics_disabled",
            "ImageSizer",
            "NoopMediaSink",
            "MediaSink",
            "LoadOptions",
            "Decor",
        ] {
            assert!(
                !help.contains(internal),
                "--help names the internal {internal}:\n{help}"
            );
        }
        // It still has to say what the flags do.
        assert!(help.contains("alt text"), "{help}");
        assert!(help.contains("--no-rewrite"), "{help}");
    }

    /// A bug report quotes one line. `--version` has to answer both questions
    /// that line is asked — which release, and which build of it — because
    /// the release number alone cannot tell a rebuilt working tree from the
    /// tag, and the sha alone means nothing to someone holding a tarball.
    #[test]
    fn test_the_long_version_carries_both_the_release_and_the_build() {
        let long = <Cli as clap::CommandFactory>::command()
            .render_long_version()
            .to_string();
        assert!(
            long.contains(env!("CARGO_PKG_VERSION")),
            "--version dropped the release number:\n{long}"
        );
        assert!(
            long.contains(BUILD_SHA),
            "--version dropped the build stamp:\n{long}"
        );

        // `-V` stays the bare release: scripts parse it, and a build stamp
        // that moves on every rebuild would break them.
        let short = <Cli as clap::CommandFactory>::command()
            .render_version()
            .to_string();
        assert!(
            !short.contains(BUILD_SHA),
            "-V grew the build stamp:\n{short}"
        );
    }
}
