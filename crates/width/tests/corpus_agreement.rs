//! DW-3.1: 100% agreement between `WidthEngine` and the committed,
//! live-Ghostty-measured corpus artifact.
//!
//! This test needs no `corpus-tool` feature and no live Ghostty session —
//! it reads the pinned JSON artifact that `tests/live_ghostty_corpus.rs`
//! (run manually, once per Ghostty version) produced and committed. That
//! split is what lets DW-3.1 gate every `cargo test --workspace` run in CI
//! (which has no Ghostty — see `docs/spikes/ghostty-caps.md`) while still
//! being backed by a real measured run rather than hand-authored
//! expectations.

use std::path::PathBuf;

use serde::Deserialize;

use width::{WidthConfig, WidthEngine};

#[derive(Deserialize)]
struct MeasuredCase {
    id: String,
    category: String,
    cluster: String,
    measured_width: u16,
}

#[derive(Deserialize)]
struct MeasuredCorpus {
    ghostty_version: Option<String>,
    cases: Vec<MeasuredCase>,
}

/// Locates the single committed `corpus/ghostty-<version>-widths.json`
/// artifact. Failing loudly (rather than silently skipping) if it's
/// missing or ambiguous is deliberate: DW-3.1 is a hard requirement, not a
/// best-effort check.
fn load_pinned_corpus() -> MeasuredCorpus {
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

#[test]
fn test_dw_3_1_engine_agrees_with_live_ghostty_over_the_full_corpus() {
    let corpus = load_pinned_corpus();
    assert!(
        !corpus.ghostty_version.as_deref().unwrap_or("").is_empty(),
        "pinned corpus artifact must record the Ghostty version it was measured against"
    );
    assert!(
        corpus.cases.len() >= 200,
        "DW-3.1 requires a ≥200-case corpus, found {}",
        corpus.cases.len()
    );

    let engine = WidthEngine::new(WidthConfig::default());
    let mut disagreements = Vec::new();
    for case in &corpus.cases {
        let got = engine.cluster_width(&case.cluster);
        if got != case.measured_width {
            disagreements.push(format!(
                "{} [{}]: engine={got} ghostty={} (codepoints {:?})",
                case.id,
                case.category,
                case.measured_width,
                case.cluster.chars().map(|c| c as u32).collect::<Vec<_>>()
            ));
        }
    }

    assert!(
        disagreements.is_empty(),
        "WidthEngine disagreed with live-Ghostty-measured widths on {}/{} cases:\n{}",
        disagreements.len(),
        corpus.cases.len(),
        disagreements.join("\n")
    );
}

#[test]
fn every_corpus_category_named_by_dw_3_1_is_represented() {
    let corpus = load_pinned_corpus();
    let categories: std::collections::HashSet<&str> =
        corpus.cases.iter().map(|c| c.category.as_str()).collect();
    for expected in [
        "cjk_common",
        "family_zwj_sequences",
        "flag_pairs",
        "vs16_promotions",
        "vs15_demotions",
        "combining_marks",
        "hangul_precomposed",
        "hangul_jamo_sequences",
    ] {
        assert!(
            categories.contains(expected),
            "DW-3.1 names {expected} as a required corpus category, but it's absent from the \
             committed artifact"
        );
    }
}
