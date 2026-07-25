//! The pinned live-Ghostty corpus artifact, shared by the two integration
//! tests that need it: `corpus_agreement.rs` (DW-3.1, per-cluster agreement)
//! and `property_display_width.rs` (DW-3.2, whole-string sums grounded in the
//! same measurements).
//!
//! `#![allow(dead_code)]` because Cargo compiles this module separately into
//! *each* test binary that declares `mod common;`, and anything a given binary
//! doesn't happen to use would otherwise warn under `-D warnings`. The
//! alternative — one struct per consumer — would let the two tests read the
//! artifact with different expectations, which is exactly what a shared
//! loader is here to prevent.
#![allow(dead_code)]

use std::path::PathBuf;

use serde::Deserialize;

/// One live-measured case: the cluster as it was typed into a real Ghostty
/// window, and the cursor advance that window reported.
#[derive(Deserialize)]
pub struct MeasuredCase {
    pub id: String,
    pub category: String,
    pub cluster: String,
    pub measured_width: u16,
}

#[derive(Deserialize)]
pub struct MeasuredCorpus {
    pub ghostty_version: Option<String>,
    pub cases: Vec<MeasuredCase>,
}

/// Locates the single committed `corpus/ghostty-<version>-widths.json`
/// artifact. Failing loudly (rather than silently skipping) if it's
/// missing or ambiguous is deliberate: DW-3.1 is a hard requirement, not a
/// best-effort check.
pub fn load_pinned_corpus() -> MeasuredCorpus {
    let corpus_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus");
    let mut matches: Vec<PathBuf> = std::fs::read_dir(&corpus_dir)
        .unwrap_or_else(|e| panic!("failed to read {corpus_dir:?}: {e}"))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with("ghostty-") && name.ends_with("-widths.json")
        })
        .collect();
    matches.sort();

    assert!(
        !matches.is_empty(),
        "no committed corpus artifact found in {corpus_dir:?} — run \
         `cargo test -p width --test live_ghostty_corpus --features corpus-tool -- --ignored` \
         against a real Ghostty session to produce one"
    );
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one pinned corpus artifact, found {matches:?} — commit only the \
         current Ghostty version's corpus"
    );

    let bytes = std::fs::read(&matches[0])
        .unwrap_or_else(|e| panic!("failed to read {:?}: {e}", matches[0]));
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("failed to parse corpus JSON: {e}"))
}
