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

use width::{WidthConfig, WidthEngine};

mod common;

use common::load_pinned_corpus;

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
