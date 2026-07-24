//! End-to-end wiring tests for the Phase 7 integration the build agent could
//! not reach from inside its file scope: that a code fence's LANGUAGE and a
//! link's URL actually travel `Document -> layout -> Painter::frame` and drive
//! real highlighting and OSC 8 emission. These exercise the seams the
//! orchestrator wired (Run.aux, painter link emission), not just the P7
//! crates' unit-level correctness.

use ast::Document;
use layout::{LayoutConfig, NullSizer, layout};
use stele::decor::themed::ThemedDecor;
use stele::painter::{Painter, Size};
use width::{WidthConfig, WidthEngine};

fn engine() -> WidthEngine {
    WidthEngine::new(WidthConfig::default())
}

fn render(src: &str, width: u16, height: u16) -> Vec<u8> {
    let doc = Document::parse(src);
    let tree = layout(&doc, width, &LayoutConfig::default(), &engine(), &NullSizer);
    let mut painter = Painter::new(engine());
    painter.register_decor(Box::new(ThemedDecor::detect(None)));
    let mut out = Vec::new();
    painter
        .frame(&tree, 0, Size { width, height }, &mut out)
        .unwrap();
    out
}

/// A fenced code block's language reaches the highlighter: a `rust` fence
/// with keywords produces MORE than the single plain run a language-less
/// block would, i.e. the grammar actually fired. Guards against a regression
/// to the old hardcoded `highlight(text, None)`.
#[test]
fn test_code_fence_language_reaches_the_highlighter() {
    // NO_COLOR must not be set for this process, or every role collapses to
    // structural and no SGR color codes appear. Assert on run partitioning
    // via SGR-reset count instead of specific colors, which holds regardless.
    let highlighted = render("```rust\nfn main() {}\n```\n", 80, 6);
    let plain = render("```\nfn main() {}\n```\n", 80, 6);

    let count = |b: &[u8]| b.windows(4).filter(|w| *w == b"\x1b[0m").count();
    assert!(
        count(&highlighted) > count(&plain),
        "a `rust` fence must produce more styled runs than a plain fence \
         (highlighted resets={}, plain resets={}) — the language did not \
         reach the highlighter",
        count(&highlighted),
        count(&plain)
    );
}

/// An allowed-scheme link emits a sanitized OSC 8 open/close around its text.
#[test]
fn test_allowed_link_emits_osc8_hyperlink() {
    let out = render("see [the site](https://example.com/x) here\n", 80, 4);
    let text = String::from_utf8_lossy(&out);
    assert!(
        text.contains("\x1b]8;;https://example.com/x\x1b\\"),
        "expected an OSC 8 open for the link, got {text:?}"
    );
    assert!(
        text.contains("\x1b]8;;\x1b\\"),
        "expected an OSC 8 close after the link"
    );
    // The visible label still renders (split into per-word runs by the
    // wrap/OSC 8 framing, so check the words rather than the joined string).
    assert!(text.contains("the") && text.contains("site"));
}

/// A hostile-scheme link NEVER reaches an OSC 8 sequence, but its text still
/// renders as plain styled content.
#[test]
fn test_hostile_link_scheme_never_reaches_osc8() {
    for src in [
        "click [x](javascript:alert(1)) now\n",
        "click [x](data:text/html;base64,PHNjcmlwdD4=) now\n",
    ] {
        let out = render(src, 80, 4);
        let text = String::from_utf8_lossy(&out);
        assert!(
            !text.contains("\x1b]8;;"),
            "a disallowed scheme must not emit any OSC 8 sequence, got {text:?}"
        );
        assert!(text.contains('x'), "the link label must still render");
    }
}

/// A URL carrying embedded control bytes cannot inject: the sanitized URL on
/// the wire has no raw ESC/BEL and the `;` framing byte is stripped.
#[test]
fn test_link_url_with_control_bytes_is_sanitized_on_the_wire() {
    // http URL with an embedded ESC and a ';' that could frame a new command.
    let src = "a [x](https://e.com/\x1b]1337;evil) b\n";
    let out = render(src, 80, 4);
    // The only ESC bytes present must be the painter's own known sequences —
    // no ESC from the URL survived. Scan the OSC 8 open specifically: it must
    // contain neither a raw ESC mid-URL nor a ';' from the payload.
    let text = String::from_utf8_lossy(&out);
    if let Some(start) = text.find("\x1b]8;;") {
        let after = &text[start + 5..];
        let url_end = after.find("\x1b\\").expect("OSC 8 must be ST-terminated");
        let url = &after[..url_end];
        assert!(
            !url.contains('\x1b'),
            "raw ESC leaked into the URL: {url:?}"
        );
        assert!(
            !url.contains(';'),
            "unstripped ';' framing byte in URL: {url:?}"
        );
    }
    // The injected OSC/APC opener `ESC ] 1337` must never appear as an ACTIVE
    // sequence on the wire (its leading ESC stripped everywhere). The bytes
    // `]1337;evil` rendering as inert visible TEXT is fine — that is not an
    // escape sequence.
    assert!(
        !text.contains("\x1b]1337"),
        "an active injected OSC/APC sequence reached the wire: {text:?}"
    );
}
