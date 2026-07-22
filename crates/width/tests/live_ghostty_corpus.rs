//! Produces `corpus/ghostty-<version>-widths.json`, the committed DW-3.1
//! artifact, by driving a real Ghostty window via `probe::Launcher` (the
//! same harness `crates/probe`'s own spikes use — no second harness).
//!
//! **Ignored by default**, following `crates/probe/tests/live_ghostty.rs`'s
//! convention: CI has no Ghostty (see `docs/spikes/ghostty-caps.md`), and
//! this needs a real macOS GUI session with Ghostty installed. Run
//! locally, from `crates/width`:
//!
//! ```sh
//! cargo test -p width --test live_ghostty_corpus --features corpus-tool -- --ignored --nocapture
//! ```
//!
//! `tests/corpus_agreement.rs` is the test that actually gates DW-3.1 on
//! every `cargo test --workspace` run, by checking `WidthEngine` against
//! whatever this tool most recently measured and committed.

use std::path::PathBuf;
use std::time::Duration;

use probe::Launcher;

fn scratch_out_path(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "stele-width-corpus-{label}-{}.json",
        std::process::id()
    ));
    p
}

#[test]
#[ignore = "requires a real Ghostty install and an interactive GUI session"]
fn measures_the_full_corpus_against_live_ghostty_and_writes_the_pinned_artifact() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let cases_path = manifest_dir.join("corpus/cases.json");
    assert!(
        cases_path.exists(),
        "corpus case list missing at {cases_path:?}"
    );

    let launcher = Launcher::default_macos();
    let tool_bin = PathBuf::from(env!("CARGO_BIN_EXE_measure_corpus"));
    let out_path = scratch_out_path("results");
    let _ = std::fs::remove_file(&out_path);

    let bytes = launcher
        .run_probe(
            &tool_bin,
            &[
                "--cases",
                cases_path.to_str().unwrap(),
                "--out",
                out_path.to_str().unwrap(),
            ],
            &out_path,
            // 206 cases, two DSR round trips each — generous but bounded.
            Duration::from_secs(60),
        )
        .expect("Launcher should drive a real Ghostty session through the full corpus within 60s");

    let json: serde_json::Value =
        serde_json::from_slice(&bytes).expect("measure_corpus must emit valid JSON");
    let cases = json["cases"]
        .as_array()
        .expect("report must have a top-level `cases` array");
    assert!(
        cases.len() >= 200,
        "expected the full ≥200-case corpus, got {}",
        cases.len()
    );

    let version = json["ghostty_version"]
        .as_str()
        .expect("report must record the measured Ghostty version");

    let committed_path = manifest_dir.join(format!("corpus/ghostty-{version}-widths.json"));
    std::fs::write(&committed_path, &bytes)
        .unwrap_or_else(|e| panic!("failed to write committed corpus at {committed_path:?}: {e}"));
    eprintln!("wrote {} measured cases to {committed_path:?}", cases.len());

    let _ = std::fs::remove_file(&out_path);
}
