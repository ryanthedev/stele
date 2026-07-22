//! The Ghostty correction layer: everything `unicode-width`'s per-codepoint
//! table gets wrong for a whole *cluster*. `unicode-width` answers "how
//! wide is this one codepoint"; Ghostty (mode 2027 grapheme clustering,
//! confirmed ON by default — `docs/spikes/ghostty-caps.md` item 6) answers
//! "how far does the cursor move after this cluster", and those two
//! questions disagree exactly at the cases this module owns:
//!
//! - **VS15/VS16** (U+FE0E/U+FE0F): force text (narrow) or emoji (wide)
//!   presentation — but *only* on a base character Unicode actually
//!   recognizes as emoji-capable (`Emoji=YES`). Live-measured corpus
//!   evidence for this: U+2661 WHITE HEART SUIT + VS16 stayed width 1 in
//!   Ghostty (`corpus/ghostty-1.3.1-widths.json`, case `vs16_17`) — unlike
//!   U+2764 HEAVY BLACK HEART + VS16, which promotes to 2. The difference
//!   is exactly `Emoji=YES` (2764) vs `Emoji=NO` (2661): a selector
//!   attached to a character with no emoji-presentation form to select
//!   does nothing. Gating on `Emoji=YES` (via `unicode-properties`'s UCD
//!   table, not a hand-maintained list) is what makes this match live
//!   Ghostty instead of over-generalizing "any character + VS16 ⇒ wide".
//! - **ZWJ sequences** (U+200D joins): render as one glyph, not the sum of
//!   the joined codepoints' widths.
//! - **Regional-indicator flag pairs** (GB12/GB13): two codepoints render
//!   as one 2-cell flag, not two 2-cell letters (4 cells).
//! - **Fitzpatrick modifiers** (U+1F3FB..=U+1F3FF): attach to the preceding
//!   emoji without adding width.
//!
//! Every other cluster (plain letters, combining marks, precomposed CJK
//! and Hangul, conjoining Hangul jamo sequences) falls through to the
//! "general" rule: the max per-codepoint width in the cluster, which is
//! exactly what `unicode-width` already gets right — combining marks and
//! variation selectors report 0 there, so they never inflate the max.

use unicode_properties::UnicodeEmoji;
use unicode_properties::emoji::{is_regional_indicator, is_zwj};
use unicode_width::UnicodeWidthChar;

const VS15: char = '\u{FE0E}';
const VS16: char = '\u{FE0F}';

/// Fitzpatrick skin-tone modifiers, U+1F3FB..=U+1F3FF.
fn is_fitzpatrick_modifier(c: char) -> bool {
    ('\u{1F3FB}'..='\u{1F3FF}').contains(&c)
}

/// A grapheme cluster made of exactly two regional-indicator letters — a
/// flag pair per GB12/GB13. `unicode-segmentation` already enforces that a
/// well-formed extended grapheme cluster contains regional indicators only
/// in pairs, so checking the count is sufficient without re-deriving the
/// pairing rule here.
fn is_flag_pair(cluster: &str) -> bool {
    let mut chars = cluster.chars();
    matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(a), Some(b), None) if is_regional_indicator(a) && is_regional_indicator(b)
    )
}

/// A single codepoint's base width, per `unicode-width`'s East Asian Width
/// and general-category tables. `ambiguous_wide` selects the
/// Ambiguous-class policy (ambiguous chars are 2 cells wide in a
/// CJK-locale terminal configuration, 1 otherwise); combining marks,
/// variation selectors, and other zero-width classes already come back 0
/// from this table, and control characters (`None`) are treated as
/// contributing no width of their own — control-byte policy belongs to
/// the layout/paint layers, not here.
fn base_char_width(c: char, ambiguous_wide: bool) -> u16 {
    let w = if ambiguous_wide {
        c.width_cjk()
    } else {
        c.width()
    };
    w.unwrap_or(0) as u16
}

/// The character immediately before a trailing selector, if any — the
/// base whose `Emoji` property gates whether that selector has an effect.
fn char_before_last(cluster: &str) -> Option<char> {
    cluster.chars().rev().nth(1)
}

/// The full correction layer, applied to one grapheme cluster. See the
/// module doc for the special cases; everything else falls through to the
/// max-per-codepoint general rule. Always returns 0, 1, or 2.
pub(crate) fn corrected_cluster_width(cluster: &str, ambiguous_wide: bool) -> u16 {
    if cluster.is_empty() {
        return 0;
    }

    if is_flag_pair(cluster) {
        return 2;
    }

    let base_is_emoji_capable = char_before_last(cluster).is_some_and(|c| c.is_emoji_char());
    if cluster.ends_with(VS16) && base_is_emoji_capable {
        return 2;
    }
    if cluster.ends_with(VS15) && base_is_emoji_capable {
        return 1;
    }
    // `> 1`: a *sequence* joined by ZWJ or modified by a skin tone, not a
    // bare ZWJ/modifier with nothing to join or modify. Live-measured
    // regression case `zerowidth_2` (a standalone U+200D, its own cluster
    // since there's nothing for it to join) is 0 in Ghostty, not 2 — the
    // general rule below already gets that right via `unicode-width`.
    if cluster.chars().count() > 1
        && (cluster.chars().any(is_zwj) || cluster.chars().any(is_fitzpatrick_modifier))
    {
        // A ZWJ sequence or an emoji + skin-tone modifier renders as a
        // single glyph in Ghostty regardless of how many codepoints join
        // it — see the corpus's `family_zwj_sequences` and
        // `fitzpatrick_modifiers` categories, both measured at 2.
        return 2;
    }

    cluster
        .chars()
        .map(|c| base_char_width(c, ambiguous_wide))
        .max()
        .unwrap_or(0)
        .min(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_cluster_is_zero_width() {
        assert_eq!(corrected_cluster_width("", false), 0);
    }

    #[test]
    fn plain_ascii_is_one() {
        assert_eq!(corrected_cluster_width("a", false), 1);
    }

    #[test]
    fn recognizes_a_flag_pair() {
        // US flag: REGIONAL INDICATOR SYMBOL LETTER U + S.
        assert_eq!(corrected_cluster_width("\u{1F1FA}\u{1F1F8}", false), 2);
    }

    #[test]
    fn vs16_promotes_default_text_presentation_to_wide() {
        // HEAVY BLACK HEART + VS16 — Emoji=YES, so the selector applies.
        assert_eq!(corrected_cluster_width("\u{2764}\u{FE0F}", false), 2);
    }

    #[test]
    fn vs15_demotes_default_emoji_presentation_to_narrow() {
        // WATCH + VS15 — Emoji=YES, so the selector applies.
        assert_eq!(corrected_cluster_width("\u{231A}\u{FE0E}", false), 1);
    }

    #[test]
    fn vs16_on_a_non_emoji_base_has_no_effect() {
        // WHITE HEART SUIT (Emoji=NO) + VS16: live-Ghostty-measured
        // regression case `vs16_17` — the selector has nothing to select,
        // so the cluster stays at its plain narrow width.
        assert_eq!(corrected_cluster_width("\u{2661}\u{FE0F}", false), 1);
    }
}
