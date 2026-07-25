//! The public seam: [`WidthEngine`] and [`graphemes`]. Signature pinned by
//! the plan (`WidthEngine::new`, `cluster_width`, `display_width`, free fn
//! `graphemes`) — this is the cross-phase contract P4's layout engine builds
//! against directly.

use unicode_segmentation::UnicodeSegmentation;

use crate::correction::corrected_cluster_width;

/// Ambiguous-width policy. East Asian Width's "Ambiguous" class (Greek,
/// Cyrillic, box-drawing, a handful of symbols) is 1 cell in most Western
/// terminal configurations and 2 in CJK-locale configurations; Ghostty
/// itself doesn't negotiate this, so it's a caller-supplied policy rather
/// than something the engine can measure once and assume forever.
#[derive(Default)]
pub struct WidthConfig {
    /// `true` asks for the CJK-locale interpretation of the Ambiguous class;
    /// `false` (the default measured against this project's reference Ghostty
    /// configuration — see `corpus/ghostty-1.3.1-widths.json`) keeps them
    /// at 1.
    ///
    /// **It does not reach the whole class**, which is worth knowing before
    /// relying on it: the widening comes from `unicode-width`'s `width_cjk`
    /// table, and that table returns 2 for Ambiguous *symbols* and
    /// box-drawing (`×` U+00D7, `°` U+00B0, `←` U+2190, `─` U+2500) while
    /// returning 1 for the Ambiguous Greek and Cyrillic **letters** (`α`
    /// U+03B1, `Ω` U+03A9, `а` U+0430). Verified character by character
    /// against `unicode-width` 0.2 and pinned by
    /// `tests/property_display_width.rs`'s
    /// `test_dw_3_2_hand_computed_string_widths_under_the_cjk_ambiguous_policy`.
    /// Neither half is measured against a CJK-configured Ghostty — the pinned
    /// corpus covers the reference configuration only — so this bit is a
    /// caller-supplied policy in the honest sense: unmeasured, and only as
    /// good as the table behind it.
    pub ambiguous_wide: bool,
}

/// Grapheme clusters in, Ghostty cell counts out. Holds nothing but its
/// configured ambiguous-width policy — construction never fails, there is
/// no I/O, and every method is a pure function of its argument plus that
/// one policy bit.
pub struct WidthEngine {
    config: WidthConfig,
}

impl WidthEngine {
    /// Builds an engine against the given ambiguous-width policy.
    pub fn new(config: WidthConfig) -> WidthEngine {
        WidthEngine { config }
    }

    /// The cell width of a single extended grapheme cluster (as yielded by
    /// [`graphemes`]) once Ghostty's own corrections are applied: VS15/VS16
    /// presentation overrides, ZWJ sequences, regional-indicator flag
    /// pairs, and Fitzpatrick skin-tone modifiers. Always 0, 1, or 2 —
    /// never more, regardless of how many codepoints `cluster` packs in.
    ///
    /// `cluster` is expected to be a single grapheme cluster; passing a
    /// multi-cluster string still returns a bounded, sensible answer (the
    /// correction layer degrades gracefully) but callers who want a whole
    /// string's width should use [`WidthEngine::display_width`] instead.
    pub fn cluster_width(&self, cluster: &str) -> u16 {
        corrected_cluster_width(cluster, self.config.ambiguous_wide)
    }

    /// The total cell width of `s`: the sum of [`WidthEngine::cluster_width`]
    /// over every grapheme cluster [`graphemes`] segments it into.
    ///
    /// Short-circuits to `s.len()` when every byte of `s` is printable ASCII
    /// (DW-1.7) — the common case for source code and English prose, and
    /// measured 40x slower than necessary on code-heavy viewports because
    /// every visible character otherwise pays for grapheme segmentation plus
    /// the correction layer. In that byte range each byte is simultaneously
    /// one UTF-8 byte, one grapheme cluster (every special case in
    /// `correction.rs` requires a codepoint outside it — flag pairs,
    /// VS15/16, ZWJ, and Fitzpatrick modifiers are all non-ASCII), and one
    /// terminal cell, and the East Asian Width class of printable ASCII is
    /// always Narrow/Neutral regardless of [`WidthConfig::ambiguous_wide`] —
    /// so the shortcut is policy-independent and answers identically to the
    /// full path for every such string. See
    /// [`WidthEngine::display_width_via_graphemes`] for the exact
    /// pre-shortcut computation, kept as a named oracle both this
    /// equivalence and the committed benchmark check against.
    pub fn display_width(&self, s: &str) -> usize {
        if is_printable_ascii(s) {
            return s.len();
        }
        self.display_width_via_graphemes(s)
    }

    /// The full grapheme-segmentation path [`WidthEngine::display_width`]
    /// takes for anything the ASCII fast path declines — every byte outside
    /// 0x20..=0x7E, which includes C0/DEL control bytes (scored 0 by
    /// [`WidthEngine::cluster_width`], not 1, so they must never take the
    /// fast path) and every multi-byte grapheme.
    pub fn display_width_via_graphemes(&self, s: &str) -> usize {
        graphemes(s).map(|g| self.cluster_width(g) as usize).sum()
    }
}

/// True when every byte of `s` is printable ASCII, 0x20 (space) through
/// 0x7E (`~`) inclusive — deliberately narrower than [`str::is_ascii`],
/// which also accepts C0/DEL control bytes (0x00-0x1F, 0x7F). Those control
/// bytes are ASCII but are not width-1: `correction.rs`'s
/// `base_char_width` scores them 0 (`unicode-width` returns `None` for
/// them), so a fast path keyed on `is_ascii` alone would silently disagree
/// with the slow path on exactly the pinned Ghostty-corpus-adjacent dirty
/// case this function exists to exclude. A free function, not a method: the
/// decision needs no [`WidthConfig`] — see [`WidthEngine::display_width`]'s
/// doc comment for why the policy bit cannot change the answer here.
fn is_printable_ascii(s: &str) -> bool {
    s.bytes().all(|b| (0x20..=0x7E).contains(&b))
}

/// Segments `s` into extended grapheme clusters (UAX #29). Segmentation
/// itself doesn't depend on any width policy — only what a cluster's width
/// *is* varies with [`WidthConfig`], not where cluster boundaries fall —
/// so this is a free function rather than a method.
pub fn graphemes(s: &str) -> impl Iterator<Item = &str> {
    s.graphemes(true)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use serde::Deserialize;

    use super::*;

    fn engine() -> WidthEngine {
        WidthEngine::new(WidthConfig::default())
    }

    #[test]
    fn test_is_printable_ascii_accepts_the_full_visible_range_and_rejects_its_neighbours() {
        assert!(is_printable_ascii(""));
        assert!(is_printable_ascii(" "));
        assert!(is_printable_ascii("~"));
        assert!(is_printable_ascii(
            "the quick brown fox jumps over 13 lazy dogs! (#42)"
        ));
        // One byte on either side of the printable range.
        assert!(!is_printable_ascii("\u{1F}"));
        assert!(!is_printable_ascii("\u{7F}"));
        // A control byte hiding inside otherwise-ASCII text.
        assert!(!is_printable_ascii("ok\tok"));
        assert!(!is_printable_ascii("ok\nok"));
        // Non-ASCII entirely.
        assert!(!is_printable_ascii("café"));
        assert!(!is_printable_ascii("日本語"));
    }

    /// **DW-1.7, pure-ASCII correctness.** For strings the fast path
    /// actually takes, its answer must equal both `s.len()` and the old
    /// per-cluster-sum formula — two independent-in-spirit checks (a byte
    /// count that needs no grapheme machinery at all, and the literal
    /// pre-change computation) rather than one path re-measuring itself.
    #[test]
    fn test_dw_1_7_ascii_fast_path_agrees_with_the_old_formula_on_pure_printable_ascii() {
        let e = engine();
        for s in [
            "",
            " ",
            "a",
            "~",
            "hello, world!",
            "The quick brown fox jumps over the lazy dog 42 times.",
            &"x".repeat(5_000),
        ] {
            assert!(
                is_printable_ascii(s),
                "fixture must exercise the fast path: {s:?}"
            );
            assert_eq!(e.display_width(s), s.len());
            assert_eq!(e.display_width(s), e.display_width_via_graphemes(s));
        }
    }

    /// **DW-1.7, dirty case: control bytes and multi-byte graphemes must not
    /// take the fast path.** Each fixture is deliberately *not* pure
    /// printable ASCII, so `display_width` must fall through to
    /// `display_width_via_graphemes` — asserted by checking the two agree,
    /// which would not by itself catch a fast path that wrongly activated
    /// (both sides could be wrong the same way), so `is_printable_ascii`'s
    /// own `false` verdict on each fixture (asserted first) is what actually
    /// proves the branch was declined.
    #[test]
    fn test_dw_1_7_control_bytes_and_multibyte_graphemes_do_not_take_the_fast_path() {
        let e = engine();
        for (s, why) in [
            ("a\tb", "a tab is ASCII but not printable-range"),
            ("a\nb", "a newline is ASCII but not printable-range"),
            ("a\u{7F}b", "DEL is ASCII but not printable-range"),
            ("a\u{0301}", "a combining mark is multi-byte, non-ASCII"),
            ("日本語", "CJK is multi-byte, non-ASCII"),
            ("\u{1F1FA}\u{1F1F8}", "a flag pair is multi-byte, non-ASCII"),
        ] {
            assert!(
                !is_printable_ascii(s),
                "{why}: {s:?} must decline the fast path"
            );
            assert_eq!(
                e.display_width(s),
                e.display_width_via_graphemes(s),
                "{why}: {s:?}"
            );
        }
        // The control-byte case's actual number, spelled out rather than
        // only compared against itself: a tab contributes 0, not 1, so a
        // fast path that wrongly counted "ASCII bytes" as 1-per-byte would
        // report 3 for "a\tb", not the correct 2.
        assert_eq!(e.display_width("a\tb"), 2);
    }

    /// **DW-1.7, exercised by the pinned Ghostty corpus.** Every corpus
    /// cluster, padded on both sides with plain ASCII text: the *whole*
    /// string is never pure-ASCII (the cluster itself guarantees that), so
    /// this proves the fast path is correctly declined for real
    /// Ghostty-measured non-ASCII content embedded in ordinary prose — not
    /// only for hand-picked fixtures — and that the boundary still produces
    /// the Ghostty-verified total. Reads the same committed artifact
    /// `crates/width/tests/common/` loads, duplicated minimally here since
    /// `crates/width/tests/**` is outside this phase's file scope.
    #[test]
    fn test_dw_1_7_display_width_matches_ghostty_measurements_for_ascii_padded_corpus_clusters() {
        #[derive(Deserialize)]
        struct Case {
            cluster: String,
            measured_width: u16,
        }
        #[derive(Deserialize)]
        struct Corpus {
            cases: Vec<Case>,
        }

        let corpus_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus");
        let path = std::fs::read_dir(&corpus_dir)
            .unwrap_or_else(|e| panic!("failed to read {corpus_dir:?}: {e}"))
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .find(|p| {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                name.starts_with("ghostty-") && name.ends_with("-widths.json")
            })
            .unwrap_or_else(|| panic!("no committed corpus artifact found in {corpus_dir:?}"));
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {path:?}: {e}"));
        let corpus: Corpus = serde_json::from_slice(&bytes)
            .unwrap_or_else(|e| panic!("failed to parse corpus JSON: {e}"));
        assert!(corpus.cases.len() >= 200, "expected the full pinned corpus");

        let e = engine();
        let mut declined_fast_path = 0usize;
        for case in &corpus.cases {
            let padded = format!("ok {} yo", case.cluster);
            // Most corpus clusters are non-ASCII by construction (the corpus
            // covers CJK, flags, ZWJ, VS15/16, combining marks, Hangul), so
            // most padded strings correctly decline the fast path — a few
            // ASCII baseline entries (e.g. a literal `"a"`) legitimately take
            // it, and that is fine: the assertion below holds either way.
            if !is_printable_ascii(&padded) {
                declined_fast_path += 1;
            }
            let expected = 3 + usize::from(case.measured_width) + 3; // "ok " + cluster + " yo"
            assert_eq!(
                e.display_width(&padded),
                expected,
                "cluster {:?} padded with ASCII disagreed with the Ghostty-measured total",
                case.cluster
            );
        }
        assert!(
            declined_fast_path > 100,
            "expected most of the pinned corpus to exercise the slow path when ASCII-padded, \
             only {declined_fast_path}/{} did",
            corpus.cases.len()
        );
    }

    /// **DW-1.7, the committed speed claim.** The ASCII fast path must be at
    /// least 2x faster than [`WidthEngine::display_width_via_graphemes`] (the
    /// exact pre-fast-path computation) on ASCII-only input, recorded in this
    /// same harness so a future regression that quietly loses the fast path
    /// is caught by a failing assertion rather than an unmeasured guess.
    ///
    /// A large, repeated input and a wall-clock comparison rather than a
    /// hard latency budget: CI hosts vary, but grapheme segmentation plus
    /// the correction layer per character (the slow path, run 5,000 times
    /// per character below) against a single byte-length check (the fast
    /// path) is architecturally a large-constant-factor difference — the
    /// audit that motivated this phase measured 40x on code-heavy content —
    /// so a 2x floor has ample margin against ordinary scheduling noise.
    #[test]
    fn test_dw_1_7_ascii_fast_path_is_at_least_2x_faster_than_the_old_formula() {
        let e = engine();
        let sample: String = "the quick brown fox jumps over the lazy dog; ".repeat(200);
        assert!(is_printable_ascii(&sample));
        const ITERATIONS: usize = 2_000;

        // Warm up (page faults, branch predictor) before either measurement.
        for _ in 0..50 {
            std::hint::black_box(e.display_width(&sample));
            std::hint::black_box(e.display_width_via_graphemes(&sample));
        }

        let start = Instant::now();
        for _ in 0..ITERATIONS {
            std::hint::black_box(e.display_width(&sample));
        }
        let fast = start.elapsed();

        let start = Instant::now();
        for _ in 0..ITERATIONS {
            std::hint::black_box(e.display_width_via_graphemes(&sample));
        }
        let slow = start.elapsed();

        assert!(
            fast.as_nanos() * 2 <= slow.as_nanos(),
            "ASCII fast path must be at least 2x faster than the pre-change formula: \
             fast={fast:?} slow={slow:?} over {ITERATIONS} iterations of a {}-byte string",
            sample.len()
        );
    }
}
