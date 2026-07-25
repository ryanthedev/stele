//! Unsafe-code hardening for the `stele` crate.
//!
//! The crate root carries `#![deny(unsafe_code)]` rather than `forbid` for
//! exactly one reason: `terminal::signals` has to call `sigaction`/`write`/
//! `tcsetattr` so a fatal signal can put the terminal back (DW-5.1), and
//! `forbid` cannot be overridden by an inner `#[allow]`. `deny` can — and
//! silently, from anywhere in the crate. These tests are what buys the
//! difference back: the crate-root attribute must still be there, and the
//! `allow(unsafe_code)` escape hatch must appear under `src/**` exactly once,
//! on the signals module. A second one anywhere else fails the CI `test` job.
//!
//! Same idiom as `crates/ast/tests/hardening.rs::test_dw_2_5_forbid_unsafe_present`,
//! which reads its own `lib.rs` so removing the attribute fails CI.
//!
//! **Scoped to `src/**` deliberately.** `tests/common/pty.rs` legitimately
//! carries a test-scope `#![allow(unsafe_code)]` — it drives the real binary on
//! a real pty, which needs raw `posix_openpt`/`ioctl`/`kill`. Test code is not
//! shipped in the binary, so it is out of scope here; only `src/**` is asserted.
//! (That harness used to be duplicated verbatim in `tests/quit_restore.rs` and
//! `tests/signal_restore.rs`; consolidating it into one module removed a
//! test-scope unsafe surface rather than adding one, and moved none of it into
//! `src/**`, so the count below is unchanged at one.)

use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.rs` file under `src/`, recursively — `media/` and `decor/` are
/// subdirectories, so a flat `read_dir` would miss them and the guard would
/// pass over the very files most likely to grow an `unsafe` block.
fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("src/ readable") {
        let path = entry.expect("readable dir entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Occurrences of `allow(unsafe_code)` under `src/**`, as `path:line`
/// relative to `src/`, sorted for a stable failure message.
fn unsafe_allows() -> Vec<String> {
    let src = src_dir();
    let mut sources = Vec::new();
    rust_sources(&src, &mut sources);
    sources.sort();
    assert!(
        sources.len() >= 5,
        "the walk found only {} source files — it is not seeing src/**",
        sources.len()
    );

    let mut found = Vec::new();
    for path in &sources {
        let text = std::fs::read_to_string(path).expect("source file readable");
        for (i, line) in text.lines().enumerate() {
            if line.contains("allow(unsafe_code)") {
                let rel = path.strip_prefix(&src).unwrap_or(path);
                found.push(format!("{}:{}", rel.display(), i + 1));
            }
        }
    }
    found
}

#[test]
fn test_dw_5_1_crate_root_denies_unsafe_code() {
    let lib = std::fs::read_to_string(src_dir().join("lib.rs")).expect("lib.rs readable");
    assert!(
        lib.contains("#![deny(unsafe_code)]"),
        "crate root must deny unsafe_code — it is the lint that makes every \
         `unsafe` block outside terminal::signals a hard error"
    );
}

#[test]
fn test_the_unsafe_escape_hatch_exists_only_in_the_signals_module() {
    let found = unsafe_allows();
    assert_eq!(
        found.len(),
        1,
        "`allow(unsafe_code)` must appear exactly once under src/**, on \
         terminal::signals. Found: {found:?}. The crate lint is `deny`, which \
         an inner `#[allow]` silently overrides, so a new one here is a new \
         unsafe surface with no review gate. If it is genuinely needed, say so \
         in this test."
    );
    assert!(
        found[0].starts_with("terminal.rs:"),
        "the only `allow(unsafe_code)` must be in terminal.rs, found at {}",
        found[0]
    );
}

#[test]
fn test_the_escape_hatch_governs_the_signals_module_and_nothing_wider() {
    let terminal =
        std::fs::read_to_string(src_dir().join("terminal.rs")).expect("terminal.rs readable");
    let lines: Vec<&str> = terminal.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.contains("allow(unsafe_code)"))
        .expect("terminal.rs carries the escape hatch");

    // An inner attribute (`#![allow(...)]`) would relax the whole file rather
    // than one module; the outer form must attach to `mod signals`.
    assert!(
        lines[at].trim().starts_with("#[allow("),
        "the escape hatch must be an outer attribute on one item, not a \
         file-wide `#![allow]`: {:?}",
        lines[at]
    );
    let governed = lines[at + 1..]
        .iter()
        .find(|line| !line.trim_start().starts_with('#'))
        .map(|line| line.trim());
    assert_eq!(
        governed,
        Some("mod signals {"),
        "the escape hatch must attach to `mod signals`, not to whatever item \
         happens to follow it"
    );
}
