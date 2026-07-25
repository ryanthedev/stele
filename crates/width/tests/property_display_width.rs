//! DW-3.2 and DW-3.3: the two invariants the engine must hold for
//! *arbitrary* input, not just the curated corpus.
//!
//! **On what DW-3.2 can honestly be asserted against.** The gate's wording —
//! *"`display_width` equals the sum of `cluster_width` over `graphemes`"* — is
//! also, verbatim, `display_width`'s implementation (`engine.rs`), so a test
//! that recomputes the right-hand side with the same two functions is
//! `assert_eq!(f(s), f(s))` and cannot fail for any input. That is what this
//! file used to contain, and only that. The expected value has to come from
//! somewhere the engine isn't, and there are exactly three such places here:
//!
//! 1. **The live-measured corpus** (`corpus/ghostty-1.3.1-widths.json`, 206
//!    clusters whose widths came from cursor deltas in a real Ghostty 1.3.1
//!    window, measured by a binary that never links this engine). Summing
//!    *measured* widths and comparing against `display_width` of the
//!    concatenation grounds the whole-string answer in a measurement — see
//!    [`test_dw_3_2_display_width_of_a_concatenation_equals_the_sum_of_measured_widths`]
//!    and its proptest form.
//! 2. **Hand-computed known answers** for whole strings, derived from the
//!    corpus's per-category facts rather than from any code —
//!    [`test_dw_3_2_hand_computed_string_widths`].
//! 3. **Algebra**: additivity across a forced grapheme boundary, over
//!    proptest-arbitrary strings —
//!    [`test_dw_3_2_display_width_is_additive_across_a_forced_cluster_boundary`].
//!
//! The separator that makes 1 and 3 sound is `U+000A`. UAX #29's GB4/GB5 break
//! before *and* after LF unconditionally — they are ordered ahead of GB9's
//! `× (Extend | ZWJ)`, so unlike a space, an LF cannot be absorbed into a
//! neighbouring cluster even when the next cluster begins with a combining
//! mark or a joiner (six corpus cases do). And an LF advances the cursor zero
//! columns, so it contributes nothing to the expected sum. Both premises are
//! asserted, not assumed.

use std::sync::OnceLock;

use proptest::prelude::*;

use width::{WidthConfig, WidthEngine, graphemes};

mod common;

use common::{MeasuredCase, load_pinned_corpus};

/// A separator that provably preserves cluster boundaries on both sides and
/// costs no columns. See this file's module doc for why it is LF and not a
/// space.
const BOUNDARY: &str = "\n";

fn corpus_cases() -> &'static [MeasuredCase] {
    static CASES: OnceLock<Vec<MeasuredCase>> = OnceLock::new();
    CASES.get_or_init(|| load_pinned_corpus().cases)
}

/// Joins measured clusters into one string with [`BOUNDARY`] between them, and
/// returns it alongside the sum of their *measured* widths — the expected
/// `display_width`, carried entirely by the artifact.
fn joined_with_expected_width(cases: &[&MeasuredCase]) -> (String, usize) {
    let mut s = String::new();
    let mut expected = 0usize;
    for case in cases {
        if !s.is_empty() {
            s.push_str(BOUNDARY);
        }
        s.push_str(&case.cluster);
        expected += usize::from(case.measured_width);
    }
    (s, expected)
}

/// The two premises every corpus-grounded assertion below rests on. Asserted
/// rather than argued: if either stopped holding, the expected sums would be
/// quietly wrong instead of loudly so.
#[test]
fn test_the_boundary_separator_costs_nothing_and_isolates_itself() {
    let engine = WidthEngine::new(WidthConfig::default());
    assert_eq!(
        engine.cluster_width(BOUNDARY),
        0,
        "an LF advances the cursor zero columns, so it must add nothing to a string's width"
    );
    // GB4/GB5, checked against the segmenter for the two shapes that would
    // break the scheme: a cluster ending in a joiner before the separator, and
    // a cluster starting with a combining mark after it.
    let s = format!("a\u{200D}{BOUNDARY}\u{0301}");
    assert_eq!(
        graphemes(&s).collect::<Vec<_>>(),
        vec!["a\u{200D}", BOUNDARY, "\u{0301}"],
        "the separator must be its own cluster even between a trailing ZWJ and a leading \
         combining mark"
    );
}

/// **DW-3.2's primary evidence.** `display_width` over a string built from
/// every cluster in the pinned corpus must equal the sum of the widths a live
/// Ghostty reported for those clusters. Nothing in the expected value comes
/// from this crate: the widths were measured by
/// `src/bin/measure_corpus.rs`, which never links the engine.
///
/// **Catches:** any `display_width` that mis-segments (a `chars()`-based sum
/// reports 4 for a flag pair, not 2), that double-counts or drops a cluster,
/// that saturates or clamps a total, or that disagrees with `cluster_width`
/// about any of the 206 measured clusters in a multi-cluster context.
///
/// **Does not catch:** a cluster whose width is wrong *and* absent from the
/// corpus — the corpus is 206 hand-chosen clusters over eight categories, and
/// `crates/width/tests/corpus_agreement.rs`'s coverage note applies here too
/// (e.g. no measured case pairs a ZWJ with a non-emoji base). Nor does it
/// catch a segmentation bug that only shows up between two *adjacent*
/// clusters, because the separator deliberately prevents adjacency; the
/// hand-computed cases below cover that shape.
///
/// Reference configuration only (`ambiguous_wide: false`). The corpus was
/// measured in one Ghostty configuration, and that configuration renders the
/// East Asian Ambiguous class at one cell; holding the `ambiguous_wide: true`
/// engine to those same numbers would assert that the policy bit does nothing.
/// The CJK policy's own hand-computed cases are in
/// [`test_dw_3_2_hand_computed_string_widths_under_the_cjk_ambiguous_policy`].
#[test]
fn test_dw_3_2_display_width_of_a_concatenation_equals_the_sum_of_measured_widths() {
    let cases: Vec<&MeasuredCase> = corpus_cases().iter().collect();
    assert!(
        cases.len() >= 200,
        "DW-3.2's ground truth is the pinned corpus; found only {} cases",
        cases.len()
    );

    let engine = WidthEngine::new(WidthConfig {
        ambiguous_wide: false,
    });
    let (whole, expected) = joined_with_expected_width(&cases);
    assert_eq!(
        engine.display_width(&whole),
        expected,
        "display_width over {} concatenated corpus clusters disagreed with the sum of their \
         live-Ghostty-measured widths",
        cases.len()
    );

    // Per category as well as in bulk: a single wrong cluster inside a
    // 206-cluster sum is a needle, and the category name is the haystack's
    // label.
    let mut by_category: std::collections::BTreeMap<&str, Vec<&MeasuredCase>> =
        std::collections::BTreeMap::new();
    for case in &cases {
        by_category.entry(&case.category).or_default().push(case);
    }
    for (category, cases) in by_category {
        let (s, expected) = joined_with_expected_width(&cases);
        assert_eq!(
            engine.display_width(&s),
            expected,
            "category {category:?} ({} clusters): display_width disagreed with the measured sum",
            cases.len()
        );
    }
}

/// **DW-3.2, hand-computed.** Whole-string widths worked out by hand from the
/// corpus's per-category measurements (CJK and Hangul syllables 2, combining
/// marks 0, flag pairs 2, a VS16 promotion 2, ASCII 1), covering the shape the
/// corpus-grounded test deliberately excludes: clusters written *adjacent*,
/// where segmentation of the concatenation is where the answer is decided.
///
/// **Catches:** per-codepoint summing (`"🇺🇸🇯🇵"` would be 8, not 4), a
/// segmenter that pairs regional indicators greedily across a boundary
/// (`"🇺🇸🇯🇵"` → 2), combining marks inflating a base (`"x́̂̃y"` → 5), and the
/// ZWJ-widening regression that `correction.rs` documents (`"a\u{200D}b"` → 3).
///
/// **Does not catch:** anything about clusters whose correct width nobody has
/// measured — these expectations are reasoned, not measured, which is why they
/// sit *beside* the corpus test rather than instead of it.
#[test]
fn test_dw_3_2_hand_computed_string_widths() {
    let engine = WidthEngine::new(WidthConfig::default());
    for (s, expected, why) in [
        ("hello", 5usize, "five ASCII letters"),
        ("\u{4E2D}\u{6587}", 4, "two wide CJK ideographs"),
        ("e\u{0301}", 1, "decomposed é: base 1 + combining mark 0"),
        (
            "e\u{0301}x",
            2,
            "the combining mark attaches to `e`, `x` still counts",
        ),
        (
            "x\u{0301}\u{0302}\u{0303}y",
            2,
            "three stacked marks on one base add nothing",
        ),
        (
            "\u{1F1FA}\u{1F1F8}\u{1F1EF}\u{1F1F5}",
            4,
            "two adjacent flag pairs: GB12/13 pairs them 2+2, not 4 letters or 1 flag",
        ),
        (
            "a\u{200D}b",
            2,
            "a dangling ZWJ widens nothing — see correction.rs's GB9 note",
        ),
        (
            "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}!",
            3,
            "one ZWJ family glyph (2) plus an exclamation mark",
        ),
        (
            "\u{2764}\u{FE0F}\u{2661}\u{FE0F}",
            3,
            "VS16 promotes the Emoji=YES heart to 2 and leaves the Emoji=NO one at 1",
        ),
        ("\u{D55C}\u{AE00}", 4, "two precomposed Hangul syllables"),
        (
            "\u{1100}\u{1161}\u{11A8}",
            2,
            "conjoining Hangul jamo compose into one wide syllable",
        ),
        ("", 0, "the empty string"),
        (
            "\u{200B}\u{200C}\u{FEFF}",
            0,
            "three zero-width format characters, each its own cluster",
        ),
        (
            "a\u{0301}\u{4E2D}\u{1F1FA}\u{1F1F8}b",
            6,
            "1 + 2 + 2 + 1 across four adjacent cluster kinds",
        ),
        (
            // The string the deleted tautological case
            // `test_dw_3_2_holds_for_a_string_of_mixed_multi_codepoint_clusters`
            // used to assert `f(s) == f(s)` over; kept, now with a number.
            "e\u{0301}\u{1F1FA}\u{1F1F8}\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F466}hello",
            10,
            "decomposed é (1) + flag pair (2) + ZWJ family (2) + `hello` (5)",
        ),
    ] {
        assert_eq!(
            engine.display_width(s),
            expected,
            "expected width {expected} for {s:?} ({why}); clusters segmented as {:?}",
            graphemes(s).collect::<Vec<_>>()
        );
    }
}

/// **DW-3.2 under the other ambiguous-width policy**, hand-computed. The
/// corpus cannot ground this configuration — it was measured in one Ghostty
/// configuration, which renders the East Asian Ambiguous class at one cell —
/// so the expectations here are hand-computed against `unicode-width`'s
/// `width_cjk` table, verified character by character.
///
/// **What that verification turned up, recorded because `WidthConfig`'s doc
/// comment used to imply otherwise:** `width_cjk` does *not* widen every
/// UAX #11 Ambiguous character. Measured against `unicode-width` 0.2:
/// `×` U+00D7, `°` U+00B0, `←` U+2190 and `♡` U+2661 return 2, but the Greek
/// and Cyrillic *letters* — `α` U+03B1, `Ω` U+03A9, `а` U+0430 — return 1,
/// the same as under the reference policy. So `ambiguous_wide: true` widens
/// symbols and box-drawing (which is what `crates/layout` uses it for) and
/// leaves Greek/Cyrillic text alone. Both halves are asserted here so the
/// behavior is pinned rather than assumed either way.
#[test]
fn test_dw_3_2_hand_computed_string_widths_under_the_cjk_ambiguous_policy() {
    let engine = WidthEngine::new(WidthConfig {
        ambiguous_wide: true,
    });
    for (s, expected, why) in [
        (
            "\u{00D7}\u{00B0}\u{2190}",
            6usize,
            "×, ° and ← are Ambiguous and `width_cjk` widens them → 2 each",
        ),
        (
            "a\u{00D7}b",
            4,
            "ASCII stays 1; only the Ambiguous symbol doubles",
        ),
        (
            "\u{2500}\u{253C}",
            4,
            "box-drawing ─ and ┼ — the case `crates/layout`'s table rules rely on",
        ),
        (
            "\u{4E2D}\u{6587}",
            4,
            "already-Wide CJK is unaffected by the policy",
        ),
        (
            "\u{00D7}\u{0301}",
            2,
            "a combining mark on an Ambiguous base adds nothing to its 2",
        ),
        (
            "hello",
            5,
            "no Ambiguous characters, so identical to the reference policy",
        ),
        (
            "\u{03B1}\u{03B2}",
            2,
            "Greek letters are UAX #11 Ambiguous but `width_cjk` reports 1 for them — \
             the policy bit does not reach them",
        ),
        ("\u{0430}\u{0431}", 2, "same for Cyrillic letters"),
    ] {
        assert_eq!(
            engine.display_width(s),
            expected,
            "expected width {expected} for {s:?} under ambiguous_wide ({why})"
        );
    }
    // And the policy bit must actually be doing something, which no test
    // asserted before: the same string is narrower under the reference policy.
    let reference = WidthEngine::new(WidthConfig {
        ambiguous_wide: false,
    });
    assert_eq!(reference.display_width("\u{00D7}\u{00B0}\u{2190}"), 3);
    assert_eq!(reference.display_width("\u{2500}\u{253C}"), 2);
}

proptest! {
    /// **DW-3.2 in property form over arbitrary strings**, with the expected
    /// value still carried by the live measurements: proptest picks an
    /// arbitrary-length, arbitrary-order selection of corpus clusters, and the
    /// expected width is the sum of what Ghostty reported for exactly those.
    ///
    /// This is the "for arbitrary strings" half of the gate that the old
    /// tautology claimed to cover. The strings are arbitrary in length,
    /// composition, and order; what they are *not* is arbitrary in content,
    /// because a string with no ground truth has no expected width.
    ///
    /// The index range is deliberately wider than the corpus and taken modulo
    /// its length below, so growing the corpus never silently shrinks the
    /// range this samples from.
    #[test]
    fn test_dw_3_2_arbitrary_selections_of_measured_clusters_sum_to_their_measured_widths(
        picks in prop::collection::vec(0usize..1024, 0..64)
    ) {
        let engine = WidthEngine::new(WidthConfig { ambiguous_wide: false });
        let cases = corpus_cases();
        let chosen: Vec<&MeasuredCase> =
            picks.iter().map(|i| &cases[i % cases.len()]).collect();
        let (s, expected) = joined_with_expected_width(&chosen);
        prop_assert_eq!(
            engine.display_width(&s),
            expected,
            "{} clusters: {:?}",
            chosen.len(),
            chosen.iter().map(|c| c.id.as_str()).collect::<Vec<_>>()
        );
    }

    /// **DW-3.2, algebraic form.** Splitting a string at a forced cluster
    /// boundary must split its width too. Unlike the identity the gate is
    /// worded as, this one relates *two different segmentations* — the two
    /// halves segmented separately, and the whole segmented once — so it is
    /// not true by construction.
    ///
    /// **Catches:** a `display_width` that is not a pure per-cluster sum: one
    /// that clamps or saturates a total, carries state across clusters, or
    /// lets a cluster's width depend on what precedes it in the string.
    /// **Does not catch:** a per-codepoint sum, which is equally additive —
    /// that is what the corpus and hand-computed cases are for.
    #[test]
    fn test_dw_3_2_display_width_is_additive_across_a_forced_cluster_boundary(
        a in ".{0,100}",
        b in ".{0,100}",
    ) {
        for ambiguous_wide in [false, true] {
            let engine = WidthEngine::new(WidthConfig { ambiguous_wide });
            let joined = format!("{a}{BOUNDARY}{b}");
            prop_assert_eq!(
                engine.display_width(&joined),
                engine.display_width(&a) + engine.display_width(&b),
                "width of {:?} + {:?} joined at a forced boundary", a, b
            );
        }
    }

    /// A consistency check, not evidence for DW-3.2: `display_width` *is*
    /// literally `graphemes(s).map(cluster_width).sum()` today, so this cannot
    /// fail against the current implementation — it is `assert_eq!(f(s), f(s))`.
    /// It is kept as a change-detector for the day `display_width` grows a
    /// faster path (an ASCII fast lane, a cached prefix, a SIMD scan) and stops
    /// being that expression: on that day this becomes a real assertion, and it
    /// is cheaper to keep than to remember to add. The gate itself is
    /// discharged by the corpus-grounded, hand-computed, and additivity tests
    /// above.
    #[test]
    fn test_display_width_still_agrees_with_a_literal_per_cluster_sum(s in ".{0,200}") {
        for ambiguous_wide in [false, true] {
            let engine = WidthEngine::new(WidthConfig { ambiguous_wide });
            let sum: usize = graphemes(&s).map(|g| engine.cluster_width(g) as usize).sum();
            prop_assert_eq!(engine.display_width(&s), sum);
        }
    }

    /// DW-3.3 (property form): no grapheme cluster of an arbitrary string
    /// ever reports a width greater than 2.
    #[test]
    fn test_dw_3_3_no_cluster_reports_width_over_two(s in ".{0,200}") {
        let engine = WidthEngine::new(WidthConfig::default());
        for g in graphemes(&s) {
            prop_assert!(
                engine.cluster_width(g) <= 2,
                "cluster {:?} (codepoints {:?}) reported width > 2",
                g,
                g.chars().map(|c| c as u32).collect::<Vec<_>>()
            );
        }
    }
}

/// DW-3.3, concrete zero-width classes: standalone joiners/format
/// characters and a bare combining mark must report exactly 0, not "small
/// but nonzero" or an off-by-one.
#[test]
fn test_dw_3_3_zero_width_classes_report_exactly_zero() {
    let engine = WidthEngine::new(WidthConfig::default());
    for cluster in [
        "\u{200B}", // zero width space
        "\u{200C}", // zero width non-joiner
        "\u{200D}", // zero width joiner, standalone (no sequence to join)
        "\u{FEFF}", // zero width no-break space / BOM
        "\u{2060}", // word joiner
        "\u{0301}", // bare combining acute accent, no base char
    ] {
        assert_eq!(
            engine.cluster_width(cluster),
            0,
            "expected {:?} (U+{:04X}) to report width 0",
            cluster,
            cluster.chars().next().unwrap() as u32
        );
    }
}

/// DW-3.3, concrete upper bound: every category in the corpus's widest
/// classes (CJK, flags, ZWJ families, VS16 promotions) still caps at 2 —
/// spelled out directly rather than only relying on the corpus file.
#[test]
fn test_dw_3_3_wide_classes_cap_at_two_not_higher() {
    let engine = WidthEngine::new(WidthConfig::default());
    for cluster in [
        "中",
        "\u{1F1FA}\u{1F1F8}", // US flag pair
        "\u{2764}\u{FE0F}",   // VS16 promotion
        "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}", // family ZWJ x4
        "\u{1F44D}\u{1F3FD}", // thumbsup + fitzpatrick
    ] {
        assert_eq!(engine.cluster_width(cluster), 2);
    }
}
