//! DW-3.2 and DW-3.3: the two invariants the engine must hold for
//! *arbitrary* input, not just the curated corpus.

use proptest::prelude::*;

use width::{WidthConfig, WidthEngine, graphemes};

fn sum_of_cluster_widths(engine: &WidthEngine, s: &str) -> usize {
    graphemes(s).map(|g| engine.cluster_width(g) as usize).sum()
}

proptest! {
    /// DW-3.2: `display_width` must equal the sum of `cluster_width` over
    /// `graphemes`, for both ambiguous-width policies.
    #[test]
    fn test_dw_3_2_display_width_equals_sum_of_cluster_widths_narrow(s in ".{0,200}") {
        let engine = WidthEngine::new(WidthConfig { ambiguous_wide: false });
        prop_assert_eq!(engine.display_width(&s), sum_of_cluster_widths(&engine, &s));
    }

    #[test]
    fn test_dw_3_2_display_width_equals_sum_of_cluster_widths_wide(s in ".{0,200}") {
        let engine = WidthEngine::new(WidthConfig { ambiguous_wide: true });
        prop_assert_eq!(engine.display_width(&s), sum_of_cluster_widths(&engine, &s));
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

/// DW-3.2, concrete case: concatenating several multi-codepoint clusters
/// (combining marks, a ZWJ sequence, a flag pair) exercises the identity
/// on exactly the kind of string the property strategy above is unlikely
/// to stumble into on its own.
#[test]
fn test_dw_3_2_holds_for_a_string_of_mixed_multi_codepoint_clusters() {
    let engine = WidthEngine::new(WidthConfig::default());
    // "é" (decomposed) + a flag pair + a ZWJ family emoji + plain ASCII.
    let s = "e\u{0301}\u{1F1FA}\u{1F1F8}\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F466}hello";
    assert_eq!(engine.display_width(s), sum_of_cluster_widths(&engine, s));
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
