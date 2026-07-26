//! OSC 8 hyperlink emission for untrusted URLs (DW-7.6, security-sensitive).
//!
//! An OSC 8 sequence is `ESC ] 8 ; params ; URI ST` (or BEL-terminated).
//! Interpolating an attacker-controlled URL into that template is a
//! *separate* injection channel from the run-text sanitization
//! `crates/stele/src/painter.rs` already does at the paint boundary — that
//! barricade strips C0/C1 bytes from displayed *text*, but a URL destined
//! for the middle of an escape sequence this module builds never passes
//! through it. This module is the barricade for that channel: every public
//! entry point here validates and sanitizes before emitting a single byte
//! of escape sequence.
//!
//! Barricade policy (scheme allowlist first, then strip): only `http`,
//! `https`, `file`, and `mailto` schemes are accepted at all — anything
//! else (`javascript:`, `data:`, a bare relative path, garbage) is
//! rejected outright, `None`, before any sanitization even runs. An
//! accepted URL then has C0 (0x00-0x1F), DEL (0x7F), C1 (0x80-0x9F), and
//! `;` bytes stripped: C0/DEL/C1 because they could reintroduce an ESC
//! that starts a new, injected sequence; `;` because it is OSC 8's own
//! parameter separator — an unstripped `;` inside the URI could be
//! misread as closing the URI field early and starting a new one. The
//! Unicode bidi formatting controls go too, for the reason spelled out on
//! [`sanitize_url`].

/// Schemes this crate will ever emit an OSC 8 hyperlink for. Deliberately
/// small and explicit — an allowlist, not a denylist — so a new hostile
/// scheme (`vbscript:`, `data:`, a future one nobody has thought of yet)
/// is rejected by default rather than needing to be enumerated.
const ALLOWED_SCHEMES: [&str; 4] = ["http", "https", "file", "mailto"];

/// Validates and sanitizes `url` for use inside an OSC 8 sequence. Returns
/// `None` if the scheme isn't in [`ALLOWED_SCHEMES`] — including malformed
/// URLs with no recognizable scheme at all. The returned string, if any,
/// contains no C0/DEL/C1 control bytes, no `;`, and no Unicode bidi
/// formatting control.
///
/// **The bidi strip is the Trojan-Source half of this barricade, and it is
/// here rather than at a call site on purpose.** A URI is not printed as
/// glyphs by the emulator, so the naive reading is that reordering controls
/// inside it are inert — but the OSC 8 URI is exactly what a terminal shows
/// the reader when they ask where a link goes (Ghostty's hover/status
/// preview) and exactly what "copy link address" puts on their clipboard.
/// A `U+202E` in the middle of an otherwise honest `https://` URL flips the
/// remainder of that preview, so the destination the reader inspects before
/// clicking is not the destination the sequence carries. The overrides and
/// isolates (U+202A-202E, U+2066-2069) do the reordering; the deprecated
/// format controls (U+206A-206F) are invisible, unimplemented, and have no
/// business in a URL either, so they go with them rather than being left as
/// the next reviewer's finding.
///
/// **Stripped, not replaced with U+FFFD.** Making tampering visible is the
/// better answer when the tampered text is something a human reads, which
/// is why it is tempting here — but a URI is machine-consumed. A U+FFFD
/// left in the middle of a path percent-encodes into `%EF%BF%BD` on its way
/// to a browser and turns a spoofed-but-working link into a 404, trading a
/// display lie for a broken destination. Stripping also keeps one policy
/// across the whole barricade: C0/DEL/C1 and `;` are already stripped here,
/// and `crates/stele/src/painter.rs`'s `sanitize` strips the same bidi
/// controls out of the *displayed label* of that same link. A link whose
/// label and whose URI disagreed about what a control character means would
/// be a barricade with a seam in it.
///
/// The directional *marks* (LRM/RLM/ALM) legitimate RTL text needs are left
/// alone, matching that painter-side policy: they influence ordering, they
/// do not override it.
pub fn sanitize_url(url: &str) -> Option<String> {
    let scheme = url.split_once(':').map(|(s, _)| s)?;
    if !ALLOWED_SCHEMES
        .iter()
        .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
    {
        return None;
    }
    let cleaned: String = url
        .chars()
        .filter(|&c| {
            !matches!(
                c as u32,
                0x00..=0x1F | 0x7F | 0x80..=0x9F      // C0, DEL, C1 controls
                | 0x202A..=0x202E                     // bidi embedding / override
                | 0x2066..=0x2069                     // bidi isolate
                | 0x206A..=0x206F,                    // deprecated format controls
            ) && c != ';'
        })
        .collect();
    // A scheme match on the raw `url` is meaningless if stripping emptied
    // it out or mangled the scheme prefix itself (e.g. control bytes
    // embedded *inside* "http" — `h\x1btp://...`) — re-validate the
    // cleaned string's own scheme rather than trusting the pre-strip check.
    let cleaned_scheme = cleaned.split_once(':').map(|(s, _)| s)?;
    if !ALLOWED_SCHEMES
        .iter()
        .any(|allowed| cleaned_scheme.eq_ignore_ascii_case(allowed))
    {
        return None;
    }
    Some(cleaned)
}

/// Builds a complete OSC 8 "open hyperlink" sequence for `url`, or `None`
/// if [`sanitize_url`] rejects it. The sequence is ST-terminated
/// (`ESC \`), matching this codebase's other OSC usage
/// (`crates/stele/src/painter.rs`'s mode-2026 sequences).
pub fn open(url: &str) -> Option<String> {
    let sanitized = sanitize_url(url)?;
    Some(format!("\x1b]8;;{sanitized}\x1b\\"))
}

/// Closes any currently open OSC 8 hyperlink. Always safe to emit (no
/// attacker-controlled content), including when no link is open.
pub const CLOSE: &str = "\x1b]8;;\x1b\\";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allowed_schemes_produce_a_sequence() {
        for url in [
            "http://example.com/page",
            "https://example.com/page?q=1",
            "file:///etc/hosts",
            "mailto:user@example.com",
            "HTTPS://EXAMPLE.COM", // scheme match is case-insensitive
        ] {
            let seq = open(url).unwrap_or_else(|| panic!("expected {url} to be allowed"));
            assert!(seq.starts_with("\x1b]8;;"));
            assert!(seq.ends_with("\x1b\\"));
            assert!(seq.contains(url) || seq.to_lowercase().contains(&url.to_lowercase()));
        }
    }

    #[test]
    fn test_dw_7_6_javascript_scheme_never_reaches_a_sequence() {
        assert_eq!(sanitize_url("javascript:alert(1)"), None);
        assert_eq!(open("javascript:alert(1)"), None);
    }

    #[test]
    fn test_dw_7_6_data_scheme_never_reaches_a_sequence() {
        assert_eq!(
            sanitize_url("data:text/html,<script>alert(1)</script>"),
            None
        );
        assert_eq!(open("data:text/html,evil"), None);
    }

    #[test]
    fn test_dw_7_6_other_disallowed_schemes_rejected() {
        for url in [
            "vbscript:evil()",
            "ftp://example.com/x",
            "about:blank",
            "no-scheme-at-all",
        ] {
            assert_eq!(sanitize_url(url), None, "expected {url} to be rejected");
        }
    }

    #[test]
    fn test_dw_7_6_embedded_control_bytes_never_reach_the_sequence() {
        let hostile = "http://example.com/\x1b]8;;javascript:alert(1)\x07/page";
        let seq = open(hostile).expect("scheme is allowed; only the body is hostile");
        assert_scan_no_stray_escapes(&seq);
        assert!(!seq.contains('\x07'));
    }

    #[test]
    fn test_dw_7_6_scheme_hidden_behind_control_bytes_is_still_rejected() {
        // "h<ESC>ttp://evil" must not be treated as an allowed scheme just
        // because stripping control bytes would later reveal "http".
        let hostile = "h\x1bttp://example.com/x";
        assert_eq!(sanitize_url(hostile), None);
    }

    #[test]
    fn test_dw_7_6_semicolon_in_url_is_stripped_not_passed_through() {
        let url = "http://example.com/a;b;c";
        let seq = open(url).unwrap();
        assert!(
            !seq[5..].contains(';'),
            "a stray `;` in the URI could be misread as a new OSC 8 parameter field: {seq:?}"
        );
    }

    #[test]
    fn test_bidi_controls_in_a_url_never_reach_the_sequence() {
        // An honest-looking host with an override and an isolate buried in
        // the path: rendered in a terminal's link preview, the tail reads
        // backwards and the reader inspects a destination that is not the
        // one the sequence carries.
        let hostile = "https://example.com/\u{202e}dm.exe\u{202c}/\u{2066}safe.md\u{2069}\u{206f}x";
        let seq = open(hostile).expect("scheme is allowed; only the body is hostile");
        assert_no_bidi_controls(&seq);
        // Everything else about the URL survives — this is a strip, not a
        // rejection, so the link still works.
        assert!(
            seq.contains("https://example.com/dm.exe/safe.mdx"),
            "{seq:?}"
        );
    }

    /// Asserts no Unicode bidi formatting control survived anywhere in
    /// `seq` — the reordering overrides and isolates, plus the deprecated
    /// format controls. Directional marks are deliberately not listed:
    /// they are permitted, here and in the painter.
    fn assert_no_bidi_controls(seq: &str) {
        for c in seq.chars() {
            assert!(
                !matches!(c as u32, 0x202A..=0x202E | 0x2066..=0x2069 | 0x206A..=0x206F),
                "bidi control U+{:04X} survived into an OSC 8 sequence: {seq:?}",
                c as u32
            );
        }
    }

    /// Scans `seq` byte-by-byte for anything that is not part of a
    /// known-good OSC 8 sequence: after the fixed `ESC ] 8 ; ;` opening and
    /// before the fixed `ESC \` closing, every remaining byte must be a
    /// normal printable/URL byte — never another ESC, C1, or BEL. A prior
    /// phase shipped a security test whose allowlist was looser than
    /// advertised (see phase discovery); this scan is deliberately tight.
    fn assert_scan_no_stray_escapes(seq: &str) {
        let body = seq
            .strip_prefix("\x1b]8;;")
            .and_then(|s| s.strip_suffix("\x1b\\"))
            .expect("sequence must have exactly one open/close pair");
        for b in body.bytes() {
            assert!(
                !matches!(b, 0x00..=0x1F | 0x7F | 0x80..=0x9F),
                "raw control/escape byte {b:#04x} found inside an OSC 8 body: {seq:?}"
            );
        }
    }
}
