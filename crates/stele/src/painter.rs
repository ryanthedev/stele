//! `Painter` — the paint boundary and terminal-injection barricade.
//!
//! [`Painter::frame`] is the single place document text meets the wire: it
//! sanitizes every run's text (C0/DEL/C1 control code points stripped),
//! clips runs to the viewport width by grapheme cluster through the shared
//! [`WidthEngine`], and wraps the whole viewport repaint in a mode-2026
//! synchronized-update block. No cell-grid differ exists — every call is a
//! full, deterministic repaint of the visible lines. It writes to any
//! `&mut dyn Write`, so tests capture and assert on the exact byte stream
//! without a real terminal.

use std::io::{self, Write};

use layout::{LayoutTree, Line, Reserved, Run, Semantic, StyleId};
use width::WidthEngine;

use crate::decor::{Decor, StructuralDecor};
use crate::media::{MediaSink, NoopMediaSink};

/// A viewport extent in terminal cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u16,
    pub height: u16,
}

/// A single cell position (0-indexed, column then row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPos {
    pub x: u16,
    pub y: u16,
}

/// A cell rectangle a [`crate::media::MediaSink`] paints into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

/// A truecolor RGB triple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// SGR attributes resolved for one run. Structural only in P5 (bold/dim/
/// italic/underline); truecolor `fg`/`bg` are wired for P7's theme layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
}

/// Begin/end a mode-2026 synchronized-update block: the terminal buffers
/// the whole frame and swaps it in atomically, so a full-viewport repaint
/// never tears. Verified against a live Ghostty (`docs/spikes/ghostty-caps.md`,
/// item 5: recognized, reset by default) and, byte-for-byte, against
/// crossterm 0.29's own `BeginSynchronizedUpdate`/`EndSynchronizedUpdate`.
const SYNC_BEGIN: &[u8] = b"\x1b[?2026h";
const SYNC_END: &[u8] = b"\x1b[?2026l";
/// Erase from the cursor to the end of the line — always emitted after a
/// line's content, so a shorter new line can never leave stale characters
/// from a longer previous one.
const CLEAR_TO_EOL: &[u8] = b"\x1b[K";
const SGR_RESET: &[u8] = b"\x1b[0m";

/// Whole-viewport immediate painter: no cell-grid differ, one full repaint
/// per [`Painter::frame`] call, wrapped in a mode-2026 synchronized-update
/// block. Deterministic given `(tree, scroll, size)`.
pub struct Painter {
    width_engine: WidthEngine,
    media: Box<dyn MediaSink>,
    decor: Box<dyn Decor>,
}

impl Painter {
    /// Builds a painter using `width_engine` to recompute run widths after
    /// [`Decor::highlight`], with the true no-op media sink and the
    /// structural decor resolver as defaults — the stubs this phase paints
    /// with until P6/P7 register their own.
    pub fn new(width_engine: WidthEngine) -> Self {
        Painter {
            width_engine,
            media: Box::new(NoopMediaSink),
            decor: Box::new(StructuralDecor),
        }
    }

    /// Registers a media sink (P6). Replaces the no-op default.
    pub fn register_media(&mut self, media: Box<dyn MediaSink>) {
        self.media = media;
    }

    /// Registers a decor resolver (P7). Replaces the structural default.
    pub fn register_decor(&mut self, decor: Box<dyn Decor>) {
        self.decor = decor;
    }

    /// Paints one full viewport repaint of `tree` starting at line `scroll`,
    /// sized `size`, to `out`. Deterministic: identical arguments always
    /// produce identical bytes. Never panics on any `tree`/`scroll`/`size`
    /// combination — an out-of-range `scroll` (past the document tail)
    /// simply paints blank rows, matching [`LayoutTree::lines`]'s own
    /// clamping behavior.
    pub fn frame(
        &mut self,
        tree: &LayoutTree,
        scroll: usize,
        size: Size,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        out.write_all(SYNC_BEGIN)?;
        for row in 0..size.height {
            write!(out, "\x1b[{};1H", row + 1)?;
            let idx = scroll.saturating_add(row as usize);
            if idx < tree.line_count()
                && let Some(line) = tree.lines(idx..idx + 1).next()
            {
                self.paint_line(line, row, size.width, out)?;
            }
            out.write_all(CLEAR_TO_EOL)?;
        }
        out.write_all(SYNC_END)?;
        out.flush()
    }

    fn paint_line(
        &mut self,
        line: &Line,
        row: u16,
        width: u16,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        match line {
            Line::Runs(runs) => self.paint_runs(runs, width, out),
            Line::Reserved(reserved) => self.paint_reserved(reserved, row, width, out),
        }
    }

    fn paint_reserved(
        &mut self,
        reserved: &Reserved,
        row: u16,
        width: u16,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        let rect = CellRect {
            x: 0,
            y: row,
            width: reserved.cols.min(width),
            height: 1,
        };
        self.media.paint(reserved, rect, out);
        Ok(())
    }

    fn paint_runs(&mut self, runs: &[Run], width: u16, out: &mut dyn Write) -> io::Result<()> {
        let mut remaining = width;
        let mut painted_any = false;
        'line: for run in runs {
            if remaining == 0 {
                break;
            }
            // `Decor::highlight` is code-block-specific (see the decor
            // module docs): every other run passes through unchanged.
            let expanded = if run.style_id == StyleId::Semantic(Semantic::CodeBlock) {
                self.decor.highlight(&run.text, None)
            } else {
                vec![run.clone()]
            };
            for expanded_run in &expanded {
                if remaining == 0 {
                    break 'line;
                }
                let sanitized = sanitize(&expanded_run.text);
                let (clipped_text, clipped_width) =
                    clip_to_width(&sanitized, &self.width_engine, remaining);
                if clipped_text.is_empty() {
                    // A non-empty run that painted nothing means its next
                    // cluster is wider than the cells left: the line is
                    // full. Stop here — a narrower later run must not fill
                    // the straggler cell out of document order.
                    if !sanitized.is_empty() {
                        break 'line;
                    }
                    continue;
                }
                let truncated = clipped_text.len() < sanitized.len();
                let style = self.decor.resolve(expanded_run.style_id);
                write_sgr(out, &style)?;
                out.write_all(clipped_text.as_bytes())?;
                painted_any = true;
                remaining = remaining.saturating_sub(clipped_width);
                if truncated {
                    break 'line;
                }
            }
        }
        if painted_any {
            out.write_all(SGR_RESET)?;
        }
        Ok(())
    }
}

/// The terminal-injection barricade: strips C0 (0x00-0x1F), DEL (0x7F), and
/// C1 (0x80-0x9F) control code points from `text`, plus the Unicode bidi
/// override/isolate formatting characters (U+202A-202E, U+2066-2069). After
/// this runs, the only escape bytes that can reach the wire come from this
/// module's own hardcoded sequences above — ESC (0x1B) falls inside the
/// stripped C0 range, so no injected OSC/APC/DECSET sequence can survive
/// with its leading ESC intact.
///
/// The bidi strip is defense against Trojan-Source-style visual spoofing: a
/// document viewer of untrusted markdown must not let override characters
/// reorder rendered text to disguise a link or command. The directional
/// *marks* (LRM/RLM/ALM) that legitimate RTL text needs are left intact;
/// only the override and isolate controls are removed.
fn sanitize(text: &str) -> String {
    text.chars()
        .filter(|&c| {
            !matches!(
                c as u32,
                0x00..=0x1F | 0x7F | 0x80..=0x9F      // C0, DEL, C1 controls
                | 0x202A..=0x202E | 0x2066..=0x2069,  // bidi override / isolate
            )
        })
        .collect()
}

/// Clips `text` to at most `max_width` cells, measuring by grapheme cluster
/// through `engine` — never by byte length, so a multi-byte cluster is
/// never split mid-character. Width decisions are accumulated in `usize`
/// and only the final stored value is saturated to `u16`, per the layout
/// crate's own width-seam gotcha (never `display_width(..) as u16` bare).
fn clip_to_width(text: &str, engine: &WidthEngine, max_width: u16) -> (String, u16) {
    let budget = max_width as usize;
    let mut used = 0usize;
    let mut out = String::with_capacity(text.len());
    for cluster in width::graphemes(text) {
        let w = engine.cluster_width(cluster) as usize;
        if used + w > budget {
            break;
        }
        used += w;
        out.push_str(cluster);
    }
    (out, used.min(u16::MAX as usize) as u16)
}

/// Writes the SGR sequence for `style`: an unconditional reset (so runs
/// never bleed a previous run's attributes) followed by one combined
/// attribute/color sequence when `style` carries any.
fn write_sgr(out: &mut dyn Write, style: &Style) -> io::Result<()> {
    out.write_all(SGR_RESET)?;
    let mut codes = Vec::new();
    if style.bold {
        codes.push("1".to_string());
    }
    if style.dim {
        codes.push("2".to_string());
    }
    if style.italic {
        codes.push("3".to_string());
    }
    if style.underline {
        codes.push("4".to_string());
    }
    if let Some(fg) = style.fg {
        codes.push(format!("38;2;{};{};{}", fg.r, fg.g, fg.b));
    }
    if let Some(bg) = style.bg {
        codes.push(format!("48;2;{};{};{}", bg.r, bg.g, bg.b));
    }
    if !codes.is_empty() {
        write!(out, "\x1b[{}m", codes.join(";"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use ast::Document;
    use layout::{CellSize, IntrinsicSizer, LayoutConfig, NullSizer, layout};
    use width::WidthConfig;

    use super::*;
    use crate::media::MediaSink;

    fn engine() -> WidthEngine {
        WidthEngine::new(WidthConfig::default())
    }

    #[test]
    fn test_sanitize_strips_c0_c1_and_del_but_keeps_printable_text() {
        let hostile = "before\x1b[?2026hafter\u{9b}mid\x7ftail";
        let clean = sanitize(hostile);
        assert_eq!(clean, "before[?2026haftermidtail");
        assert!(
            !clean
                .chars()
                .any(|c| matches!(c as u32, 0x00..=0x1F | 0x7F | 0x80..=0x9F))
        );
    }

    #[test]
    fn test_sanitize_strips_bidi_overrides_but_keeps_directional_marks() {
        // RLO...PDF and an isolate must go (Trojan-Source spoofing vector);
        // an RLM mark that legitimate RTL text relies on must stay.
        let hostile = "user\u{202e}txt.exe\u{202c} \u{2066}iso\u{2069}\u{200f}ok";
        let clean = sanitize(hostile);
        assert_eq!(clean, "usertxt.exe iso\u{200f}ok");
        assert!(
            !clean
                .chars()
                .any(|c| matches!(c as u32, 0x202A..=0x202E | 0x2066..=0x2069))
        );
    }

    #[test]
    fn test_sanitize_is_a_no_op_on_ordinary_text() {
        assert_eq!(sanitize("hello, world — 日本語"), "hello, world — 日本語");
    }

    #[test]
    fn test_clip_to_width_truncates_at_grapheme_boundary_not_byte_length() {
        let e = engine();
        // Each CJK character is 2 cells wide; a byte-length clip would
        // split a multi-byte character instead of stopping before it.
        // Budget 6 fits all three (3*2=6); budget 5 only fits two, since a
        // third would overshoot (6 > 5) — never a partial character.
        let (clipped, width) = clip_to_width("日本語abc", &e, 6);
        assert_eq!(clipped, "日本語");
        assert_eq!(width, 6);

        let (clipped2, width2) = clip_to_width("日本語abc", &e, 5);
        assert_eq!(clipped2, "日本");
        assert_eq!(width2, 4);
    }

    #[test]
    fn test_clip_to_width_keeps_everything_under_budget() {
        let e = engine();
        let (clipped, width) = clip_to_width("hello", &e, 80);
        assert_eq!(clipped, "hello");
        assert_eq!(width, 5);
    }

    #[test]
    fn test_clip_to_width_zero_budget_yields_empty() {
        let e = engine();
        let (clipped, width) = clip_to_width("hello", &e, 0);
        assert_eq!(clipped, "");
        assert_eq!(width, 0);
    }

    struct AlwaysSizes(CellSize);
    impl IntrinsicSizer for AlwaysSizes {
        fn size(&self, _node: ast::NodeId, _doc: &Document) -> Option<CellSize> {
            Some(self.0)
        }
    }

    struct SpyMedia {
        paint_calls: std::rc::Rc<std::cell::RefCell<Vec<CellRect>>>,
    }
    impl MediaSink for SpyMedia {
        fn paint(&mut self, _reserved: &Reserved, rect: CellRect, _out: &mut dyn Write) {
            self.paint_calls.borrow_mut().push(rect);
        }
        fn evict(&mut self, _node_id: ast::NodeId, _out: &mut dyn Write) {}
    }

    #[test]
    fn test_reserved_line_dispatches_to_registered_media_sink() {
        let doc = Document::parse("![alt](pic.png)\n");
        let sizer = AlwaysSizes(CellSize { cols: 10, rows: 1 });
        let tree = layout(&doc, 80, &LayoutConfig::default(), &engine(), &sizer);
        let mut painter = Painter::new(engine());
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        painter.register_media(Box::new(SpyMedia {
            paint_calls: calls.clone(),
        }));
        let mut buf = Vec::new();
        painter
            .frame(
                &tree,
                0,
                Size {
                    width: 80,
                    height: 3,
                },
                &mut buf,
            )
            .unwrap();
        // The true no-op default would never invoke a sink at all; a
        // registered sink must actually be called when a Reserved line is
        // visible in the frame.
        assert_eq!(calls.borrow().len(), 1);
        assert_eq!(calls.borrow()[0].width, 10);
    }

    #[test]
    fn test_noop_media_sink_never_writes_and_never_panics() {
        let doc = Document::parse("hello\n");
        let node_id = doc.blocks()[0].id;
        let mut sink = NoopMediaSink;
        let reserved = Reserved {
            node_id,
            cols: 10,
            rows: 1,
        };
        let rect = CellRect {
            x: 0,
            y: 0,
            width: 10,
            height: 1,
        };
        let mut buf = Vec::new();
        sink.paint(&reserved, rect, &mut buf);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_frame_blanks_document_shorter_than_viewport_no_wraparound() {
        let doc = Document::parse("only line\n");
        let tree = layout(&doc, 40, &LayoutConfig::default(), &engine(), &NullSizer);
        assert_eq!(tree.line_count(), 1);
        let mut painter = Painter::new(engine());
        let mut buf = Vec::new();
        painter
            .frame(
                &tree,
                0,
                Size {
                    width: 40,
                    height: 5,
                },
                &mut buf,
            )
            .unwrap();
        let text = String::from_utf8_lossy(&buf);
        // 5 rows worth of cursor positioning must appear even though the
        // document only has 1 line — the remaining 4 rows are blank
        // (moved-to and cleared, no content), not a repeat of line 1.
        for row in 1..=5 {
            assert!(text.contains(&format!("\x1b[{row};1H")));
        }
        assert_eq!(text.matches("only line").count(), 1);
    }

    #[test]
    fn test_frame_clips_to_actual_viewport_narrower_than_layout_width() {
        // Layout is clamped up to the 24-cell floor even though the real
        // terminal is only 10 cells wide; the painter must still clip to
        // the real viewport width passed to `frame`, independent of the
        // tree's own (wider) laid-out width.
        let doc = Document::parse("abcdefghijklmnopqrstuvwxyz\n");
        let config = LayoutConfig {
            min_width: 24,
            max_width: 100,
        };
        let tree = layout(&doc, 10, &config, &engine(), &NullSizer);
        assert_eq!(tree.width(), 24);
        let mut painter = Painter::new(engine());
        let mut buf = Vec::new();
        painter
            .frame(
                &tree,
                0,
                Size {
                    width: 10,
                    height: 1,
                },
                &mut buf,
            )
            .unwrap();
        let text = String::from_utf8_lossy(&buf);
        assert!(text.contains("abcdefghij"));
        assert!(!text.contains("abcdefghijk"));
    }
}
