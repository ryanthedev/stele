//! Integration tests over `Painter::frame`'s exact byte output — the tests
//! that prove the paint boundary is a real terminal-injection barricade
//! (DW-5.5) and that whole-viewport repaint is correctly synchronized
//! (DW-5.2), plus the CLI-driven width clamp (DW-5.6).

use ast::Document;
use layout::{LayoutConfig, NullSizer, layout};
use stele::painter::{Painter, Size};
use width::{WidthConfig, WidthEngine};

fn engine() -> WidthEngine {
    WidthEngine::new(WidthConfig::default())
}

/// The complete vocabulary of escape sequences `Painter::frame` may emit.
/// Anything else on the wire — including an injected DECSET like `?1049h`
/// that the old looser scanner waved through — is a barricade breach.
const FRAME_FIXED_SEQUENCES: &[&[u8]] = &[
    b"\x1b[?2026h", // synchronized-update begin
    b"\x1b[?2026l", // synchronized-update end
    b"\x1b[K",      // clear-to-end-of-line
    b"\x1b[0m",     // SGR reset
];

/// Asserts the byte stream contains ONLY the painter's own sequences and
/// sanitized printable content — no injected or foreign control bytes.
///
/// Two rules, both a whitelist (guilty until proven known-good, the correct
/// posture for a security barricade):
/// 1. No raw C1 control (0x80-0x9F, including the 8-bit CSI 0x9B), no BEL or
///    other C0/DEL byte except ESC — content is sanitized and the framing is
///    ESC-based only, so any such byte is a sanitize regression.
/// 2. Every ESC opens either a fixed sequence above, a cursor-position
///    `ESC [ <digits> ; 1 H` (the painter always homes to column 1), or an
///    SGR `ESC [ <digits/;> m`. A DECSET/OSC/APC or any other shape fails.
fn assert_only_known_escapes(bytes: &[u8]) {
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if (0x80..=0x9F).contains(&b) || (b < 0x20 && b != 0x1B) || b == 0x7F {
            panic!(
                "raw control byte {b:#x} on the wire at {i}: {:?}",
                &bytes[i..(i + 8).min(bytes.len())]
            );
        }
        if b != 0x1B {
            i += 1;
            continue;
        }
        if let Some(seq) = FRAME_FIXED_SEQUENCES
            .iter()
            .find(|seq| bytes[i..].starts_with(seq))
        {
            i += seq.len();
            continue;
        }
        // Variable sequence: cursor position or SGR. Params are digits and
        // semicolons only — note `?` is deliberately NOT accepted here, so a
        // private-mode set like `?1049h` cannot masquerade as one.
        let rest = &bytes[i + 1..];
        assert!(
            rest.first() == Some(&b'['),
            "ESC at byte {i} not followed by '[': {:?}",
            &bytes[i..(i + 8).min(bytes.len())]
        );
        let mut j = 1;
        while j < rest.len() && matches!(rest[j], b'0'..=b'9' | b';') {
            j += 1;
        }
        assert!(
            j < rest.len(),
            "unterminated/foreign CSI at byte {i}: {:?}",
            &bytes[i..(i + 12).min(bytes.len())]
        );
        let terminator = rest[j];
        let params = &rest[1..j];
        let known = match terminator {
            b'H' => params.ends_with(b";1"), // cursor home to column 1
            b'm' => true,                    // SGR: digits/semicolons already checked
            _ => false,
        };
        assert!(
            known,
            "CSI at byte {i} not a painter shape (term {:#x}, params {:?}): {:?}",
            terminator,
            params,
            &bytes[i..(i + 12).min(bytes.len())]
        );
        i += 2 + j;
    }
}

#[test]
fn test_dw_5_2_full_scroll_frames_are_2026_paired() {
    let source: String = (0..5_100).map(|i| format!("line {i}\n\n")).collect();
    let doc = Document::parse(&source);
    let tree = layout(&doc, 80, &LayoutConfig::default(), &engine(), &NullSizer);
    assert!(
        tree.line_count() >= 10_000,
        "fixture must produce a 10k+-line document (got {})",
        tree.line_count()
    );

    let mut painter = Painter::new(engine());
    let size = Size {
        width: 80,
        height: 24,
    };
    let mut scroll = 0usize;
    let mut frames = 0usize;
    while scroll < tree.line_count() {
        let mut buf = Vec::new();
        painter.frame(&tree, scroll, size, &mut buf).unwrap();

        let begin_count = buf.windows(8).filter(|w| *w == b"\x1b[?2026h").count();
        let end_count = buf.windows(8).filter(|w| *w == b"\x1b[?2026l").count();
        assert_eq!(
            begin_count, 1,
            "frame at scroll {scroll} missing a sync begin"
        );
        assert_eq!(end_count, 1, "frame at scroll {scroll} missing a sync end");
        // Begin must be the very first thing written, end the very last.
        assert!(buf.starts_with(b"\x1b[?2026h"));
        assert!(buf.ends_with(b"\x1b[?2026l"));

        scroll += size.height as usize;
        frames += 1;
    }
    assert!(
        frames > 400,
        "expected many frames scrolling a 10k+-line doc, got {frames}"
    );
}

#[test]
fn test_dw_5_5_injection_fixture_renders_inert() {
    // Raw ESC, OSC, APC, and DECSET bytes embedded directly in paragraph
    // text. CommonMark has no special meaning for any of these bytes, so
    // they parse straight through as literal text content.
    let source = format!(
        "bare esc: [{esc}]\n\nosc: [{esc}]93;4;1;rgb:ff/00/00{bel}\n\napc: {esc}_Gi=1,a=q{esc}\\\n\ndecset: {esc}[?1049h\n\nc1csi: {c1}31m and bidi {rlo}evil{pdf}\n",
        esc = '\u{1b}',
        bel = '\u{7}',
        c1 = '\u{9b}',    // 8-bit CSI — must not survive as a raw byte
        rlo = '\u{202e}', // right-to-left override (Trojan Source)
        pdf = '\u{202c}', // pop directional formatting
    );
    let doc = Document::parse(&source);
    let tree = layout(&doc, 80, &LayoutConfig::default(), &engine(), &NullSizer);

    let mut painter = Painter::new(engine());
    let mut buf = Vec::new();
    painter
        .frame(
            &tree,
            0,
            Size {
                width: 80,
                height: 20,
            },
            &mut buf,
        )
        .unwrap();

    assert_only_known_escapes(&buf);

    // Sanity check the fixture actually reached the painter: the
    // surrounding plain-text labels should survive (sanitize only strips
    // control code points, not ordinary printable text).
    let text = String::from_utf8_lossy(&buf);
    assert!(text.contains("bare esc"));
    assert!(text.contains("osc:"));
    assert!(text.contains("apc:"));
    assert!(text.contains("decset:"));
}

#[test]
fn test_dw_5_6_max_width_flag_clamps_layout_width() {
    // Simulates the CLI wiring in `main.rs`: `--max-width 60` on a 100-col
    // terminal must clamp content width to 60, never the full terminal
    // width.
    let doc = Document::parse("some paragraph text\n");
    let config = LayoutConfig {
        min_width: 24,
        max_width: 60,
    };
    let terminal_width = 100;
    let tree = layout(&doc, terminal_width, &config, &engine(), &NullSizer);
    assert_eq!(tree.width(), 60);
}

#[test]
fn test_width_below_min_clamps_up_and_painter_still_clips_to_real_viewport() {
    let doc = Document::parse("x".repeat(200).as_str());
    let config = LayoutConfig {
        min_width: 24,
        max_width: 100,
    };
    // Real terminal reports width 5 — below the floor.
    let tree = layout(&doc, 5, &config, &engine(), &NullSizer);
    assert_eq!(tree.width(), 24);

    let mut painter = Painter::new(engine());
    let mut buf = Vec::new();
    painter
        .frame(
            &tree,
            0,
            Size {
                width: 5,
                height: 1,
            },
            &mut buf,
        )
        .unwrap();
    let text = String::from_utf8_lossy(&buf);
    // Only 5 'x' characters may appear as painted content, even though the
    // tree itself is laid out 24 cells wide.
    let content_run: String = text.chars().filter(|c| *c == 'x').collect();
    assert_eq!(content_run.len(), 5);
}
