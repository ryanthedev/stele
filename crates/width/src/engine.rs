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
    pub fn display_width(&self, s: &str) -> usize {
        graphemes(s).map(|g| self.cluster_width(g) as usize).sum()
    }
}

/// Segments `s` into extended grapheme clusters (UAX #29). Segmentation
/// itself doesn't depend on any width policy — only what a cluster's width
/// *is* varies with [`WidthConfig`], not where cluster boundaries fall —
/// so this is a free function rather than a method.
pub fn graphemes(s: &str) -> impl Iterator<Item = &str> {
    s.graphemes(true)
}
