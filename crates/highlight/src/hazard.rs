//! The one definition of "code point that must not reach the terminal".
//!
//! Four places in this workspace need that answer: the OSC 8 URI sanitizer
//! ([`crate::sanitize_url`]), the paint-boundary text sanitizer
//! (`crates/stele/src/painter.rs`), the URL-activation validator
//! (`crates/stele/src/link.rs`), and the frame-invariant checker that is
//! supposed to catch any of the three regressing
//! (`crates/stele/tests/painter_frame.rs`).
//!
//! They used to each carry their own `matches!` arm listing the ranges.
//! That is not a stylistic duplication — it failed. When the deprecated
//! format controls (U+206A-206F) were added to the URI sanitizer, the other
//! three kept the old set, and the *checker* was one of the three: the test
//! whose entire job is noticing a control code point on the wire was blind
//! to the exact range that had just been declared hostile. A barricade
//! written down four times is four barricades, and they drift silently
//! because nothing compares them.
//!
//! So the set lives here, once, and the four sites call it. Adding a range
//! is now a one-line edit that every site inherits, and the checker cannot
//! fall behind what it checks.
//!
//! This crate is the home rather than `stele` because the dependency edge
//! runs `stele -> highlight`: a definition here is reachable from all four
//! sites, and one in `stele` would not be reachable from `sanitize_url`.

/// True for a code point that must never reach the terminal inside text or
/// inside an escape sequence's payload.
///
/// Three families, each hostile for its own reason:
///
/// - **C0 (U+0000-001F), DEL (U+007F), C1 (U+0080-009F)** — these *are*
///   escape sequences, or start one. An unfiltered ESC in run text or in an
///   OSC 8 URI lets a document author inject a sequence of their own choosing
///   into a frame this workspace believes it authored.
/// - **Bidi embeddings, overrides, and isolates (U+202A-202E, U+2066-2069)**
///   — Trojan Source. These reorder the glyphs *after* them, so the text a
///   reader sees is not the text that is there. In a link they let a
///   destination read as one thing and be another.
/// - **Deprecated format controls (U+206A-206F)** — invisible, unimplemented
///   by modern engines, and meaningful to nothing. Anything that only ever
///   arrives to exploit a parser's leniency is refused on principle rather
///   than left as the next reviewer's finding.
///
/// **Directional marks (LRM U+200E, RLM U+200F, ALM U+061C) are deliberately
/// absent.** They influence ordering, they do not override it, and Arabic or
/// Hebrew prose legitimately needs them — stripping them would corrupt honest
/// documents to defend against a spoof they cannot mount. This is the one
/// judgment call in the set, and it is why the set is enumerated by hand
/// rather than taken wholesale from a Unicode category.
pub fn is_display_hazard(c: char) -> bool {
    matches!(
        c as u32,
        0x00..=0x1F | 0x7F | 0x80..=0x9F      // C0, DEL, C1 controls
        | 0x202A..=0x202E                     // bidi embedding / override
        | 0x2066..=0x2069                     // bidi isolate
        | 0x206A..=0x206F,                    // deprecated format controls
    )
}

/// `s` with every [`is_display_hazard`] code point removed.
///
/// Stripped rather than replaced with U+FFFD. Substitution is the better
/// answer when the tampered text is something a human reads — the damage
/// becomes visible instead of silent — but the two callers here are a URI
/// and a run of already-laid-out text. A U+FFFD in a URI percent-encodes to
/// `%EF%BF%BD` on its way to a browser and turns a spoofed-but-working link
/// into a 404, trading a display lie for a broken destination; a U+FFFD in
/// run text occupies a cell the layout engine did not measure, so a line
/// that was exactly `content_width` cells wide becomes one cell too wide and
/// wraps. Removal keeps both invariants.
pub fn strip_display_hazards(s: &str) -> String {
    s.chars().filter(|&c| !is_display_hazard(c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three families, at both ends of each range — an off-by-one in a
    /// range bound is the failure mode a midpoint sample cannot see.
    #[test]
    fn test_every_hostile_range_is_hazardous_at_both_bounds() {
        for cp in [
            0x00, 0x1F, // C0
            0x7F, // DEL
            0x80, 0x9F, // C1
            0x202A, 0x202E, // bidi embedding / override
            0x2066, 0x2069, // bidi isolate
            0x206A, 0x206F, // deprecated format controls
        ] {
            let c = char::from_u32(cp).expect("every bound is a valid scalar value");
            assert!(
                is_display_hazard(c),
                "U+{cp:04X} is inside a declared-hostile range but reports safe"
            );
        }
    }

    /// The judgment call, pinned. If someone later "simplifies" the set to a
    /// Unicode category these three go with it, and RTL documents break in a
    /// way no other test in this workspace would notice.
    #[test]
    fn test_directional_marks_survive_because_rtl_prose_needs_them() {
        for (cp, name) in [(0x200E, "LRM"), (0x200F, "RLM"), (0x061C, "ALM")] {
            let c = char::from_u32(cp).expect("every mark is a valid scalar value");
            assert!(
                !is_display_hazard(c),
                "{name} (U+{cp:04X}) is a directional mark, not an override — \
                 stripping it corrupts honest RTL text"
            );
        }
    }

    /// The code points immediately outside each range. A range written one
    /// too wide strips ordinary text: U+0020 is a space and U+00A0 a
    /// non-breaking space, both of which every document contains.
    ///
    /// The isolate and deprecated-control ranges are contiguous
    /// (U+2069 then U+206A), so that seam has no outside neighbor to sample
    /// — U+2065 and U+2070 bracket the pair.
    #[test]
    fn test_ordinary_text_just_outside_each_range_is_left_alone() {
        for cp in [0x20, 0x7E, 0xA0, 0x2029, 0x202F, 0x2065, 0x2070] {
            let c = char::from_u32(cp).expect("every neighbor is a valid scalar value");
            assert!(
                !is_display_hazard(c),
                "U+{cp:04X} is ordinary text but reports hazardous — a range is too wide"
            );
        }
    }

    #[test]
    fn test_strip_removes_hazards_and_keeps_everything_else() {
        let dirty = "https://example.com/\u{202e}exe.dm\u{2069}/\u{206b}safe\u{200f}.md";
        assert_eq!(
            strip_display_hazards(dirty),
            "https://example.com/exe.dm/safe\u{200f}.md",
            "the strip must remove overrides, isolates and deprecated controls \
             while leaving the RLM and every ordinary character in place"
        );
    }
}
