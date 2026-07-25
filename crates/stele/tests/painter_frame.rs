//! Integration tests over `Painter::frame`'s exact byte output — the tests
//! that prove the paint boundary is a real terminal-injection barricade
//! (DW-5.5) and that whole-viewport repaint is correctly synchronized
//! (DW-5.2), plus the CLI-driven width clamp (DW-5.6) and the degenerate
//! viewports the framing has to survive.
//!
//! One scanner, [`assert_only_known_escapes`], answers the barricade question
//! for every fixture here. There used to be two — a byte-oriented one in this
//! file and a code-point one beside the hostile-document fixture — and the byte
//! one was wrong: UTF-8 continuation bytes land in `0x80..=0x9F` (`│` is
//! `E2 94 82`, `•` is `E2 80 A2`), so it would have panicked on any
//! box-drawing or bullet glyph. It survived only because its own fixture was
//! ASCII and registered no `ThemedDecor`; the first person to put a blockquote
//! in the DW-5.5 fixture would have got a baffling failure out of the
//! *barricade* test. The surviving scanner decodes first, the way a terminal
//! does, and keeps the stricter CUP field check the byte version had.

mod common;

use ast::Document;
use layout::{LayoutConfig, NullSizer, layout};
use stele::decor::themed::ThemedDecor;
use stele::painter::{Painter, Size};

use common::fixtures::engine;

/// The complete vocabulary of escape sequences `Painter::frame` may emit.
/// Anything else on the wire — including an injected DECSET like `?1049h`
/// that the old looser scanner waved through — is a barricade breach.
const FRAME_FIXED_SEQUENCES: &[&str] = &[
    "\u{1b}[?2026h", // synchronized-update begin
    "\u{1b}[?2026l", // synchronized-update end
    "\u{1b}[K",      // clear-to-end-of-line
    "\u{1b}[0m",     // SGR reset
];

/// Asserts the byte stream contains ONLY the painter's own sequences and
/// sanitized printable content — no injected or foreign control sequences.
///
/// Scans by **code point**, not by byte: the wire is UTF-8 and a terminal
/// decodes it before dispatching controls, so U+009B (encoded `C2 9B`) is a
/// live 8-bit CSI even though neither of its bytes is `0x9B` alone — while
/// `E2 94 82` contains a `0x94` that is not a control at all. A byte scan
/// gets both of those backwards.
///
/// Three rules, all whitelists (guilty until proven known-good, the correct
/// posture for a security barricade):
/// 1. No C0/DEL/C1 control code point except ESC — content is sanitized and
///    the framing is ESC-based only, so any such code point is a sanitize
///    regression.
/// 2. No bidi override or isolate (U+202A-U+202E, U+2066-U+2069). Not a
///    control in the C1 sense, but the Trojan Source class: they reorder what
///    a reader sees without changing a byte of the text. `painter::sanitize`
///    strips exactly this range, and this is what asserts it.
/// 3. Every ESC opens either a fixed sequence above, a cursor position
///    `ESC [ <digits> ; <digits> H`, an SGR `ESC [ <digits/;> m`, or a
///    well-formed OSC 8 hyperlink whose URI carries no controls. A
///    DECSET/APC/OSC of any other shape fails.
fn assert_only_known_escapes(bytes: &[u8]) {
    let text = String::from_utf8(bytes.to_vec()).expect("the painter emits valid UTF-8");
    let c: Vec<char> = text.chars().collect();
    let at = |i: usize| -> String { c[i..(i + 20).min(c.len())].iter().collect() };
    let forbidden = |cp: u32| matches!(cp, 0x00..=0x1F | 0x7F | 0x80..=0x9F | 0x202A..=0x202E | 0x2066..=0x2069);

    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch != '\u{1b}' {
            assert!(
                !forbidden(ch as u32),
                "raw control code point {:#x} on the wire at char {i}: {:?}",
                ch as u32,
                at(i)
            );
            i += 1;
            continue;
        }
        let rest: String = c[i..].iter().collect();
        if let Some(seq) = FRAME_FIXED_SEQUENCES.iter().find(|s| rest.starts_with(**s)) {
            i += seq.chars().count();
            continue;
        }
        // OSC 8 hyperlink: `ESC ] 8 ; ; <uri> ESC \`, both the open and the
        // (empty-URI) close. The URI is content, so it gets rule 1 and 2 too.
        if rest.starts_with("\u{1b}]8;;") {
            let body: String = rest.chars().skip(5).collect();
            let end = body
                .find("\u{1b}\\")
                .unwrap_or_else(|| panic!("unterminated OSC 8 at {i}: {:?}", at(i)));
            for u in body[..end].chars() {
                assert!(
                    !forbidden(u as u32),
                    "control code point {:#x} inside an OSC 8 URI at {i}",
                    u as u32
                );
            }
            i += 5 + body[..end].chars().count() + 2;
            continue;
        }
        // Variable sequence: cursor position or SGR. Params are digits and
        // semicolons only — note `?` is deliberately NOT accepted here, so a
        // private-mode set like `?1049h` cannot masquerade as one.
        assert_eq!(
            c.get(i + 1),
            Some(&'['),
            "ESC at char {i} not followed by '[': {:?}",
            at(i)
        );
        let mut j = i + 2;
        while j < c.len() && (c[j].is_ascii_digit() || c[j] == ';') {
            j += 1;
        }
        assert!(
            j < c.len(),
            "unterminated/foreign CSI at char {i}: {:?}",
            at(i)
        );
        let params: String = c[i + 2..j].iter().collect();
        let known = match c[j] {
            // Cursor position, `row;col`. This used to require `;1` — every
            // cursor move went to column 1, because the painter only ever
            // started a row. That is no longer true: an inline media box is
            // placed at its own column mid-line and the cursor is restored to
            // the cell after it, and the build stamp is painted in the
            // bottom-right corner. Both are the painter's own arithmetic, not
            // anything a document can reach.
            //
            // Restricting the column was never what made this barricade hold.
            // Document text cannot inject an escape because `sanitize()`
            // strips C0/DEL/C1 from every run before it is written — rule 1
            // above is what asserts that. What matters here is that a CSI
            // carries only well-formed numeric parameters, which the
            // digits-and-semicolons scan already guarantees; this adds that a
            // cursor move is exactly two non-empty fields, so a malformed or
            // overlong one still fails.
            'H' => {
                let fields: Vec<&str> = params.split(';').collect();
                fields.len() == 2 && fields.iter().all(|f| !f.is_empty())
            }
            'm' => true, // SGR: digits/semicolons already checked
            _ => false,
        };
        assert!(
            known,
            "CSI at char {i} is not a painter shape (terminator {:?}, params {params:?}): {:?}",
            c[j],
            at(i)
        );
        i = j + 1;
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
        assert_sync_paired(&buf, &format!("scroll {scroll}"));
        scroll += size.height as usize;
        frames += 1;
    }
    assert!(
        frames > 400,
        "expected many frames scrolling a 10k+-line doc, got {frames}"
    );
}

/// Exactly one synchronized-update block per frame, opened by the very first
/// bytes and closed by the very last.
///
/// Stated as `== 1` and as `starts_with`/`ends_with`, never as "the begins
/// balance the ends": `0 == 0` satisfies a balance, so a painter that emitted
/// nothing at all would pass — which is exactly the case the degenerate
/// viewports below produce most of.
fn assert_sync_paired(buf: &[u8], label: &str) {
    let begins = buf.windows(8).filter(|w| *w == b"\x1b[?2026h").count();
    let ends = buf.windows(8).filter(|w| *w == b"\x1b[?2026l").count();
    assert_eq!(begins, 1, "{label}: expected exactly one sync begin");
    assert_eq!(ends, 1, "{label}: expected exactly one sync end");
    assert!(
        buf.starts_with(b"\x1b[?2026h"),
        "{label}: the frame must open with the sync begin"
    );
    assert!(
        buf.ends_with(b"\x1b[?2026l"),
        "{label}: the frame must close with the sync end"
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
        c1 = '\u{9b}',    // 8-bit CSI — must not survive as a raw code point
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

/// A hostile document that plants raw ESC, CSI, OSC, APC, BEL and C1 bytes in
/// **every** document context the parser has, not just paragraph text: the
/// DW-5.5 fixture above covers the paragraph rung, this covers frontmatter,
/// headings, emphasis, code spans, links, autolinks, images, blockquotes,
/// lists, tables, fences, inline and block HTML, and both math forms.
fn hostile_document() -> String {
    let esc = "\u{1b}";
    let bel = "\u{7}";
    let csi8 = "\u{9b}"; // C1 CSI
    let osc8 = "\u{9d}"; // C1 OSC
    let apc8 = "\u{9f}"; // C1 APC
    let st8 = "\u{9c}"; // C1 ST
    format!(
        "---\ntitle: {esc}[?1049h evil frontmatter\n---\n\n\
         # Heading {esc}[31mred{esc}[0m {csi8}2J\n\n\
         Paragraph with {esc}]0;retitled{bel} and {apc8}Gf=100;AAAA{st8} and {osc8}8;;file:///etc/passwd{st8}.\n\n\
         *emph {esc}[5m* and `code span {esc}[?2026l` and **strong {csi8}6n**.\n\n\
         [link text {esc}[1m](http://example.com/{esc}]8;;javascript:alert(1){bel})\n\n\
         [bad scheme](javascript:alert{esc}(1))\n\n\
         ![alt text {esc}_Gf=100{esc}\\\\ that is quite long and wraps around the box](./missing-{esc}[2J.png)\n\n\
         > blockquote {esc}[?25l hidden cursor\n\n\
         - list item {esc}[2J\n- second {csi8}0m\n\n\
         | col {esc}[7m | b {csi8}K |\n|---|---|\n| cell {apc8}Gq=1{st8} | {esc}[38;2;0;0;0m |\n\n\
         ```rust\nfn main() {{ /* {esc}[?1049h {csi8}2J */ let x = \"{esc}]0;t{bel}\"; }}\n```\n\n\
         <div onclick=\"{esc}[?2026h\">literal html {csi8}J</div>\n\n\
         Inline html <span>{esc}[31m</span> tail.\n\n\
         Math: $x^{{{esc}[2J}} + \\evil{esc}[?1049h$ and display:\n\n\
         $$\n{esc}[?1049h \\frac{{{csi8}2J}}{{b}}\n$$\n\n\
         ~~~\n{esc}[?1049h plain fence {apc8}Gz{st8}\n~~~\n\n\
         Autolink <http://example.com/{esc}[2J> done.\n"
    )
}

#[test]
fn test_hostile_document_emits_no_foreign_escapes_at_any_width_or_scroll() {
    let src = stele::decor::frontmatter::apply(&hostile_document(), true).into_owned();
    let src = stele::decor::mermaid::preprocess(&src).into_owned();
    let doc = Document::parse(&src);
    for width in [1u16, 2, 10, 40, 80, 200] {
        let config = LayoutConfig {
            min_width: 24,
            max_width: 100,
        };
        let tree = layout(&doc, width, &config, &engine(), &NullSizer);
        for scroll in [0usize, 1, 5, tree.line_count().saturating_sub(1), 10_000] {
            let mut painter = Painter::new(engine());
            painter.register_decor(Box::new(ThemedDecor::detect(None)));
            let mut buf = Vec::new();
            painter
                .frame(&tree, scroll, Size { width, height: 24 }, &mut buf)
                .unwrap();
            assert_only_known_escapes(&buf);
        }
    }
}

/// The same hostile document, but through the REAL media path: an enabled
/// `ImageSizer` reserves boxes, and the `GfxMediaSink` writes alt text / txm
/// grid rows / literal TeX source directly to the wire, bypassing `Painter`'s
/// own sanitize. Both fallback rungs are forced so no kitty APC is emitted and
/// every byte on the wire is text the sink produced itself.
#[test]
fn test_hostile_document_through_the_media_sink_emits_no_foreign_escapes() {
    use stele::ImageSizer;
    use stele::media::GfxMediaSink;

    let src = stele::decor::mermaid::preprocess(&hostile_document()).into_owned();
    let doc = Document::parse(&src);
    let sizer = ImageSizer::new(".");
    for width in [10u16, 40, 80] {
        let config = LayoutConfig {
            min_width: 24,
            max_width: 100,
        };
        let tree = layout(&doc, width, &config, &engine(), &sizer);
        for scroll in [0usize, 3, 9, tree.line_count().saturating_sub(2)] {
            let mut sink = GfxMediaSink::new(doc.clone(), ".");
            sink.force_ratex_failure_for_test(true);
            sink.force_txm_failure_for_test(true);
            let mut painter = Painter::new(engine());
            painter.register_media(Box::new(sink));
            painter.register_decor(Box::new(ThemedDecor::detect(None)));
            let mut buf = Vec::new();
            painter
                .frame(&tree, scroll, Size { width, height: 24 }, &mut buf)
                .unwrap();
            assert_only_known_escapes(&buf);
        }
    }
}

/// Degenerate viewports and adversarial scroll offsets: never panic, always
/// exactly one properly-anchored 2026 block, never more rows addressed than
/// the viewport has.
#[test]
fn test_degenerate_viewports_and_scrolls_still_emit_one_anchored_sync_block() {
    let src: String = (0..40)
        .map(|i| format!("para {i} with some words\n\n"))
        .collect();
    let doc = Document::parse(&src);
    let tree = layout(&doc, 80, &LayoutConfig::default(), &engine(), &NullSizer);
    let n = tree.line_count();
    for &(w, h) in &[
        (0u16, 0u16),
        (0, 24),
        (24, 0),
        (1, 1),
        (1, 40),
        (40, 1),
        (u16::MAX, 1),
        (2, 2),
    ] {
        for &scroll in &[0usize, 1, n - 1, n, n + 1, usize::MAX] {
            let mut painter = Painter::new(engine());
            let mut buf = Vec::new();
            painter
                .frame(
                    &tree,
                    scroll,
                    Size {
                        width: w,
                        height: h,
                    },
                    &mut buf,
                )
                .unwrap();
            assert_sync_paired(&buf, &format!("{w}x{h} scroll {scroll}"));
            let s = String::from_utf8_lossy(&buf);
            let rows = s.matches(";1H").count();
            assert!(
                rows <= h as usize + 1,
                "{rows} row homes for a {h}-row viewport at {w}x{h}"
            );
        }
    }
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
