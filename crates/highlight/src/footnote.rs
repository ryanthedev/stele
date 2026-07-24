//! Footnote reference / back-reference OSC 8 fragment anchors (DW-7.4).
//!
//! A `[^label]` reference in running text and its footnote definition's
//! back-reference need matching same-document anchor ids so a terminal
//! that supports OSC 8 `id=` jump targets can navigate between them —
//! terminal-side navigation itself is out of scope (this project targets
//! Ghostty; whether it or any terminal actually acts on same-document OSC
//! 8 `id=` jumps is not asserted here). What *is* committed to, and what
//! this module's tests assert, is the emission contract: the ref's target
//! anchor names the def's own anchor, and the def's back-reference target
//! names the ref's own anchor — a 1:1 matched pair, for every label.

/// A same-document anchor id derived from a footnote label, prefixed by
/// role (`fn-` for the definition side, `fnref-` for the reference site)
/// so the two can never collide with each other even for the same label.
/// Labels are parsed markdown content (`ast::InlineKind::FootnoteReference`,
/// `ast::BlockKind::FootnoteDefinition`) and therefore untrusted document
/// text, not an internal identifier — every non-id-safe character
/// (including `;`, control bytes, or anything that could prematurely
/// terminate an OSC 8 sequence) collapses to `_` rather than passing
/// through, so a hostile label cannot break out of the anchor value.
fn anchor_id(prefix: &str, label: &str) -> String {
    let safe: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{prefix}-{safe}")
}

/// The anchor id a footnote *definition* owns (what a reference jumps to).
pub fn def_anchor_id(label: &str) -> String {
    anchor_id("fn", label)
}

/// The anchor id a footnote *reference* site owns (what a definition's
/// back-reference jumps to).
pub fn ref_anchor_id(label: &str) -> String {
    anchor_id("fnref", label)
}

/// The OSC 8 sequence for a `[^label]` reference in running text: its own
/// id is [`ref_anchor_id`], targeting [`def_anchor_id`] as a same-document
/// fragment. No external URL is ever involved — the id/fragment values
/// are both derived through [`anchor_id`]'s safe alphabet, so
/// `crate::hyperlink`'s scheme allowlist does not apply here (there is no
/// scheme to check).
pub fn ref_osc8(label: &str) -> String {
    build(&ref_anchor_id(label), &def_anchor_id(label))
}

/// The OSC 8 sequence for a footnote definition's back-reference: its own
/// id is [`def_anchor_id`], targeting [`ref_anchor_id`].
pub fn backref_osc8(label: &str) -> String {
    build(&def_anchor_id(label), &ref_anchor_id(label))
}

fn build(own_id: &str, target_anchor: &str) -> String {
    format!("\x1b]8;id={own_id};#{target_anchor}\x1b\\")
}

/// Closes a currently open footnote-anchor hyperlink.
pub const CLOSE: &str = crate::hyperlink::CLOSE;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dw_7_4_ref_and_backref_anchors_match_1_to_1() {
        for label in ["one", "two", "a-long-label", "3"] {
            let ref_seq = ref_osc8(label);
            let backref_seq = backref_osc8(label);
            // The ref's own id is exactly what the backref targets, and
            // vice versa — a matched pair, not just two sequences that
            // happen to mention the same label.
            assert!(ref_seq.contains(&format!("id={}", ref_anchor_id(label))));
            assert!(ref_seq.contains(&format!("#{}", def_anchor_id(label))));
            assert!(backref_seq.contains(&format!("id={}", def_anchor_id(label))));
            assert!(backref_seq.contains(&format!("#{}", ref_anchor_id(label))));
        }
    }

    #[test]
    fn test_distinct_labels_never_collide() {
        let labels = ["one", "two", "three", "one-two", "onetwo"];
        let mut ids = std::collections::HashSet::new();
        for label in labels {
            assert!(ids.insert(def_anchor_id(label)), "collision for {label}");
            assert!(ids.insert(ref_anchor_id(label)), "collision for {label}");
        }
        assert_eq!(ids.len(), labels.len() * 2);
    }

    #[test]
    fn test_hostile_label_cannot_break_out_of_the_anchor_value() {
        let hostile = "one\x1b]8;;javascript:evil\x07;two";
        let seq = ref_osc8(hostile);
        for b in seq.bytes() {
            // Every byte must come from the fixed sequence skeleton or the
            // id-safe alphabet — no raw control byte from the hostile
            // label can have survived into the emitted sequence.
            assert!(
                b.is_ascii_alphanumeric()
                    || matches!(
                        b,
                        b'-' | b'_' | b'#' | b'=' | b';' | 0x1b | b'\\' | b']' | b'8' | b'i' | b'd'
                    ),
                "unexpected byte {b:#04x} in {seq:?}"
            );
        }
        // No stray ESC bytes beyond the two structural ones (open + close).
        assert_eq!(seq.matches('\x1b').count(), 2);
    }
}
