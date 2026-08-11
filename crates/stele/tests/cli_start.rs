//! `Cli::start` (Phase 3): what `stele`'s positional argument resolves to —
//! a document to load, or a directory to browse — asserted entirely through
//! `Cli::parse_from` and `start()`, never through the real binary. That
//! split is deliberate: `start` decides *before* a single document byte is
//! read or the terminal is touched, so it is exactly the kind of decision
//! this crate's own convention (`cli.rs`'s module doc) says belongs where a
//! unit test can reach it.
//!
//! DW-3.12 (bare `stele` opens the explorer on a real pty) and half of
//! DW-3.13 (a directory argument never shows "Is a directory" in a rendered
//! frame) still need the real binary — those live in `explorer_keys.rs`
//! beside the rest of this phase's pty coverage, not here.

use std::path::PathBuf;

use clap::Parser;

use stele::cli::{Cli, CliError, Start};
use stele::loader::DocumentSource;

/// A scratch directory unique to `tag`, so tests running concurrently in the
/// same process cannot see each other's fixtures. Mirrors the idiom at
/// `tests/link_interaction.rs:47-68`.
fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("stele-cli-start-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// DW-3.11: a file argument names a document to load.
#[test]
fn test_dw_3_11_start_maps_a_file_argument_to_document() {
    let dir = scratch_dir("file");
    let file = dir.join("notes.md");
    std::fs::write(&file, "# hi\n").unwrap();

    let cli = Cli::parse_from(["stele", file.to_str().unwrap()]);
    assert_eq!(
        cli.start().unwrap(),
        Start::Document(DocumentSource::Path(file))
    );
}

/// DW-3.11/DW-3.13: a directory argument browses instead of loading — and,
/// since `start` never calls `DocumentSource::load_with` to decide this, it
/// never produces the "Is a directory" `read_to_end` error either.
#[test]
fn test_dw_3_11_start_maps_a_directory_argument_to_explore() {
    let dir = scratch_dir("dir");
    let cli = Cli::parse_from(["stele", dir.to_str().unwrap()]);
    assert_eq!(cli.start().unwrap(), Start::Explore(dir));
}

/// DW-3.11: no argument at all browses the current directory.
#[test]
fn test_dw_3_11_start_maps_no_argument_to_explore_at_cwd() {
    let cli = Cli::parse_from(["stele"]);
    let cwd = std::env::current_dir().unwrap();
    assert_eq!(cli.start().unwrap(), Start::Explore(cwd));
}

/// Dirty case for DW-3.11/DW-3.13: a symlink to a directory is still a
/// directory — `fs::metadata` follows it, so this must not misclassify as a
/// file just because the argument itself is a link.
#[cfg(unix)]
#[test]
fn test_a_symlink_to_a_directory_is_still_explore() {
    let dir = scratch_dir("symlink-target");
    let target = dir.join("real");
    std::fs::create_dir(&target).unwrap();
    let link = dir.join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let cli = Cli::parse_from(["stele", link.to_str().unwrap()]);
    assert_eq!(cli.start().unwrap(), Start::Explore(link));
}

/// DW-3.14: `--watch <dir>` is rejected, naming both the flag and the
/// directory — the same shape as `--watch -`.
#[test]
fn test_dw_3_14_watch_with_a_directory_is_rejected_naming_both() {
    let dir = scratch_dir("watch-dir");
    let cli = Cli::parse_from(["stele", dir.to_str().unwrap(), "--watch"]);
    let err = cli.start().unwrap_err();
    assert_eq!(err, CliError::WatchDirectory(dir.clone()));
    let message = err.to_string();
    assert!(message.contains("--watch"), "message was: {message}");
    assert!(
        message.contains(dir.to_str().unwrap()),
        "message was: {message}"
    );
}

/// Dirty case for DW-3.14: `--watch` combined with a symlink to a directory
/// is rejected the same way a literal directory is.
#[cfg(unix)]
#[test]
fn test_watch_with_a_symlink_to_a_directory_is_rejected() {
    let dir = scratch_dir("watch-symlink");
    let target = dir.join("real");
    std::fs::create_dir(&target).unwrap();
    let link = dir.join("link");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let cli = Cli::parse_from(["stele", link.to_str().unwrap(), "--watch"]);
    assert!(matches!(
        cli.start().unwrap_err(),
        CliError::WatchDirectory(_)
    ));
}

/// DW-3.14: `-` still means stdin, and `--watch -` keeps its existing
/// rejection — `start` must not have disturbed either.
#[test]
fn test_dw_3_14_dash_still_means_stdin_and_watch_dash_still_rejected() {
    assert_eq!(
        Cli::parse_from(["stele", "-"]).start().unwrap(),
        Start::Document(DocumentSource::Stdin)
    );
    assert_eq!(
        Cli::parse_from(["stele", "-", "--watch"])
            .start()
            .unwrap_err(),
        CliError::WatchStdin
    );
}

/// DW-3.16: a nonexistent directory is not decided here at all — it falls
/// through to `Start::Document` and, downstream, to the ordinary
/// `DocumentSource::load_with` error path, which is what keeps
/// `test_dw_5_4_missing_file_exits_nonzero_with_clean_stderr`
/// (`tests/cli_errors.rs`, a different plan's anchored test) producing the
/// same "could not read file" message it always has, rather than a new
/// message minted here.
#[test]
fn test_dw_3_16_a_nonexistent_path_falls_through_to_the_ordinary_load_error() {
    let path = PathBuf::from("/nonexistent/stele-cli-start-does-not-exist");
    let cli = Cli::parse_from(["stele", path.to_str().unwrap()]);
    assert_eq!(
        cli.start().unwrap(),
        Start::Document(DocumentSource::Path(path))
    );
}

/// Dirty case for DW-3.11/DW-3.16: an existing path that is neither a
/// directory nor absent — here, a Unix domain socket — is still a
/// `Document`, left to the ordinary load path exactly as it was before
/// `start` existed; `start` only special-cases directories.
#[cfg(unix)]
#[test]
fn test_an_existing_non_directory_special_file_is_still_document() {
    let dir = scratch_dir("device-node");
    let path = dir.join("sock");
    let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();

    let cli = Cli::parse_from(["stele", path.to_str().unwrap()]);
    assert_eq!(
        cli.start().unwrap(),
        Start::Document(DocumentSource::Path(path))
    );
}
