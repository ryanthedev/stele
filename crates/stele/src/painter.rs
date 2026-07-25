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

use highlight::{HYPERLINK_CLOSE, hyperlink_open};
use layout::{LayoutTree, Line, LineItem, Reserved, ReservedLine, Run, Semantic, StyleId};
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

/// What painting one run consumed, so the caller can keep the running column
/// (an inline media box needs to know where on the row it starts) and know
/// whether the line ended mid-run.
#[derive(Debug, Clone, Copy, Default)]
struct PaintedRun {
    /// Cells consumed.
    width: u16,
    /// Whether any glyphs were actually written (a run can consume zero
    /// cells and still need an SGR reset behind it).
    wrote_text: bool,
    /// The run hit the end of the row: stop painting this line.
    line_full: bool,
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
    ///
    /// Once `SYNC_BEGIN` is on the wire, `SYNC_END` follows it on **every**
    /// exit path, error included. Mode 2026 is a global terminal mode, not a
    /// screen-buffer one: the restore sequence run on exit and on panic
    /// (`terminal.rs`) does not reset it, so a frame that `?`s out of a
    /// half-written block hands the user back a shell that has stopped
    /// painting — and any I/O error on a pty (EAGAIN when full, EPIPE, a short
    /// write over ssh) is enough to do it. The paint's own error is what gets
    /// returned; a failure to close is only reported if nothing else failed.
    pub fn frame(
        &mut self,
        tree: &LayoutTree,
        scroll: usize,
        size: Size,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        out.write_all(SYNC_BEGIN)?;
        let painted = self.frame_body(tree, scroll, size, out);
        let closed = out.write_all(SYNC_END).and_then(|()| out.flush());
        painted.and(closed)
    }

    /// The body of [`frame`](Self::frame), between the synchronized-update
    /// begin and end. Split out so every `?` in here is caught by the caller
    /// rather than escaping past the closing sequence.
    fn frame_body(
        &mut self,
        tree: &LayoutTree,
        scroll: usize,
        size: Size,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        // Every frame, media or not: the sink needs a reliable boundary to
        // reset per-frame state and sweep placements whose node has scrolled
        // out of view entirely (no `paint` fires for those).
        self.media.begin_frame(out);
        // Columns the *last* viewport row consumed. The build stamp shares
        // that row with the document, so this is what decides whether there
        // is room for it (see `paint_build_stamp`).
        let mut last_row_cols: u16 = 0;
        for row in 0..size.height {
            write!(out, "\x1b[{};1H", row + 1)?;
            let idx = scroll.saturating_add(row as usize);
            if idx < tree.line_count()
                && let Some(line) = tree.lines(idx..idx + 1).next()
            {
                if row + 1 == size.height {
                    last_row_cols = match line {
                        // A media box's cells belong to the terminal's
                        // compositor, not to us: nothing here can say whether
                        // the stamp would land above or below the raster. Claim
                        // the whole row so the stamp stands down.
                        Line::Reserved(_) => size.width,
                        _ => line.width().min(size.width),
                    };
                }
                self.paint_line(line, row, size, out)?;
            }
            out.write_all(CLEAR_TO_EOL)?;
        }
        self.paint_build_stamp(size, last_row_cols, out)
    }

    /// Paints the build's commit sha dim in the bottom-right corner.
    ///
    /// This viewer's entire output is pixels in a terminal — there is nothing
    /// to grep and no log to read, so "the fix doesn't work" and "that binary
    /// predates the fix" look identical from the outside. Both cost real
    /// debugging time when confused. The stamp makes them distinguishable at a
    /// glance, and a `-dirty` suffix says the tree had uncommitted edits.
    ///
    /// Painted after the content and inside the synchronized-update block, so
    /// it never tears and never shifts a document line. It is skipped entirely
    /// rather than truncated when the last viewport row cannot hold it without
    /// colliding with text: `last_row_cols` is how many columns that row's
    /// document content already consumed.
    ///
    /// That comparison has to be against the *content*, not against the
    /// viewport. The stamp is dim, so a collision does not read as chrome
    /// overlapping text — it reads as the document's own last words, silently
    /// replaced by a hex string.
    fn paint_build_stamp(
        &mut self,
        size: Size,
        last_row_cols: u16,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        let stamp = crate::cli::BUILD_SHA;
        let width = self
            .width_engine
            .display_width(stamp)
            .min(u16::MAX as usize) as u16;
        // +1 so it never butts directly against content on the same row.
        if size.height == 0 || width == 0 || size.width <= width.saturating_add(1) {
            return Ok(());
        }
        // The stamp occupies `[size.width - width, size.width)`; the content
        // occupies `[0, last_row_cols)`. Leave at least one blank cell between
        // them, or paint nothing at all.
        if size.width.saturating_sub(last_row_cols) <= width {
            return Ok(());
        }
        write!(out, "\x1b[{};{}H", size.height, size.width - width + 1)?;
        write!(out, "\x1b[2m")?;
        out.write_all(stamp.as_bytes())?;
        out.write_all(SGR_RESET)?;
        Ok(())
    }

    fn paint_line(
        &mut self,
        line: &Line,
        row: u16,
        size: Size,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        match line {
            Line::Items(items) => self.paint_items(items, row, size.width, out),
            Line::Reserved(line) => self.paint_reserved(line, row, size, out),
        }
    }

    /// Paint one row of a standalone reserved box: its container prefix
    /// (blockquote gutter, list marker) as text, then the box itself handed to
    /// the media sink at the column the prefix ends at.
    ///
    /// `rect` is not this single row: it is the box's **visible remainder**
    /// from this row down — `y` here, `height` the rows of the box still to
    /// come before either the box or the viewport ends. On the box's first
    /// painted row (the only call at which a sink can place an image) that is
    /// exactly the box's on-screen extent, which is the one thing a sink
    /// cannot work out for itself: it sees neither the viewport height nor
    /// the rows that scrolled off the top. Handing it a bare `height: 1` is
    /// what let a top-truncated box get placed at full height from the top
    /// visible row, painting the image down over the text below it.
    fn paint_reserved(
        &mut self,
        line: &ReservedLine,
        row: u16,
        size: Size,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        let reserved = &line.boxed;
        let mut painted_any = false;
        let mut col: u16 = 0;
        for run in &line.prefix {
            let remaining = size.width.saturating_sub(col);
            if remaining == 0 {
                break;
            }
            let painted = self.paint_run(run, remaining, out)?;
            col = col.saturating_add(painted.width);
            painted_any |= painted.wrote_text;
            if painted.line_full {
                break;
            }
        }
        if painted_any {
            out.write_all(SGR_RESET)?;
        }
        // The box begins where layout put it — after the prefix — not at
        // column 0: a box inside a blockquote belongs beside the gutter bar,
        // not painted across it. `x` comes from the laid-out prefix width
        // rather than from what actually got painted, so the box lands in the
        // same column whether or not a narrow viewport clipped the gutter.
        let x = line.prefix_width();
        if x >= size.width {
            // The whole box is off the right edge of this (narrower than
            // laid-out) viewport. Painting nothing beats placing at column 0.
            return Ok(());
        }
        let rows_left = reserved.rows.saturating_sub(reserved.row);
        let rows_to_viewport_bottom = size.height.saturating_sub(row);
        let rect = CellRect {
            x,
            y: row,
            width: reserved.cols.min(size.width - x),
            height: rows_left.min(rows_to_viewport_bottom).max(1),
        };
        self.media.paint(reserved, rect, out);
        Ok(())
    }

    /// Paint one line's items left to right, tracking the absolute column so
    /// an inline media box knows where on the row it starts.
    fn paint_items(
        &mut self,
        items: &[LineItem],
        row: u16,
        width: u16,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        let mut col: u16 = 0;
        let mut painted_any = false;
        for item in items {
            let remaining = width.saturating_sub(col);
            if remaining == 0 {
                break;
            }
            match item {
                LineItem::Run(run) => {
                    let painted = self.paint_run(run, remaining, out)?;
                    col = col.saturating_add(painted.width);
                    painted_any |= painted.wrote_text;
                    if painted.line_full {
                        break;
                    }
                }
                LineItem::Box(reserved) => {
                    let cols = reserved.cols.min(remaining);
                    self.paint_inline_box(reserved, col, row, cols, out)?;
                    col = col.saturating_add(cols);
                    painted_any = true;
                }
            }
        }
        if painted_any {
            out.write_all(SGR_RESET)?;
        }
        Ok(())
    }

    /// Paint one text run into at most `remaining` cells, reporting how much
    /// of the row it consumed and whether the line is finished.
    fn paint_run(
        &mut self,
        run: &Run,
        remaining: u16,
        out: &mut dyn Write,
    ) -> io::Result<PaintedRun> {
        // `Decor::highlight` is code-block-specific (see the decor
        // module docs): every other run passes through unchanged.
        let expanded = if run.style_id == StyleId::Semantic(Semantic::CodeBlock) {
            // `run.aux` carries the code fence's language (threaded from
            // layout), so the highlighter can pick a grammar instead of
            // always falling back to a plain block.
            self.decor.highlight(&run.text, run.aux.as_deref())
        } else {
            vec![run.clone()]
        };
        let mut painted = PaintedRun::default();
        for expanded_run in &expanded {
            let left = remaining.saturating_sub(painted.width);
            if left == 0 {
                painted.line_full = true;
                return Ok(painted);
            }
            let sanitized = sanitize(&expanded_run.text);
            let (clipped_text, clipped_width) = clip_to_width(&sanitized, &self.width_engine, left);
            if clipped_text.is_empty() {
                // A non-empty run that painted nothing means its next
                // cluster is wider than the cells left: the line is
                // full. Stop here — a narrower later run must not fill
                // the straggler cell out of document order.
                if !sanitized.is_empty() {
                    painted.line_full = true;
                    return Ok(painted);
                }
                continue;
            }
            let truncated = clipped_text.len() < sanitized.len();
            // A link run carries its destination in `aux`; wrap the text
            // in an OSC 8 hyperlink. `hyperlink::open` sanitizes the URL
            // (scheme allowlist, control/`;` bytes stripped) and returns
            // None for a hostile or disallowed URL, so nothing is emitted
            // and the text still renders as plain styled text.
            let link_open = if expanded_run.style_id == StyleId::Semantic(Semantic::Link) {
                expanded_run.aux.as_deref().and_then(hyperlink_open)
            } else {
                None
            };
            if let Some(seq) = &link_open {
                out.write_all(seq.as_bytes())?;
            }
            let style = self.decor.resolve(expanded_run.style_id);
            write_sgr(out, &style)?;
            out.write_all(clipped_text.as_bytes())?;
            if link_open.is_some() {
                out.write_all(HYPERLINK_CLOSE.as_bytes())?;
            }
            painted.wrote_text = true;
            painted.width = painted.width.saturating_add(clipped_width);
            if truncated {
                painted.line_full = true;
                return Ok(painted);
            }
        }
        Ok(painted)
    }

    /// Paint an inline media box riding this row's baseline at column `col`.
    ///
    /// The box's cells are blanked first and unconditionally. `paint_frame`
    /// only issues `CLEAR_TO_EOL` *after* a line is painted, so cells the
    /// cursor skips keep the previous frame's glyphs — and a math raster is
    /// rendered on a transparent background, so anything stale underneath
    /// shows straight through it.
    fn paint_inline_box(
        &mut self,
        reserved: &Reserved,
        col: u16,
        row: u16,
        cols: u16,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        out.write_all(SGR_RESET)?;
        for _ in 0..cols {
            out.write_all(b" ")?;
        }
        let rect = CellRect {
            x: col,
            y: row,
            width: cols,
            height: 1,
        };
        self.media.paint(reserved, rect, out);
        // The sink parks the cursor wherever placing left it; put it back at
        // the cell after the box so the rest of the line paints in order.
        write!(out, "\x1b[{};{}H", row + 1, col.saturating_add(cols) + 1)?;
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
///
/// `pub(crate)` because the barricade has a second entrance: [`MediaSink`]
/// implementations write text straight to the wire (image alt text, the txm
/// math grid, literal TeX source) without passing through [`Painter::paint_run`],
/// so they must carry the same guarantee. It used to be a second, hand-copied
/// body in `media::sink`, with a comment explaining that `painter.rs` was out
/// of that phase's file scope — a scheduling fact that read to the next reader
/// as an instruction to keep duplicating. One function is the invariant; two
/// identical functions are a request to diff two files.
pub(crate) fn sanitize(text: &str) -> String {
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
///
/// `pub(crate)` for the same reason [`sanitize`] is: the media sink's fallback
/// rows are clipped to their reserved box with exactly this budget loop, and
/// the copy that used to live there had already drifted (it discarded the
/// measured width instead of saturating it). A CJK or txm double-width glyph
/// that overruns its box wraps on the bottom viewport row and scrolls the
/// alternate screen, so the two call sites need the *same* budget arithmetic,
/// not two that look alike.
pub(crate) fn clip_to_width(text: &str, engine: &WidthEngine, max_width: u16) -> (String, u16) {
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
        // Two rows deliberately: a one-row box now rides a text line as a
        // `LineItem::Box`, so only a taller box still takes the standalone
        // `Line::Reserved` path this test is about.
        let sizer = AlwaysSizes(CellSize { cols: 10, rows: 2 });
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
        // visible in the frame — once per reserved row.
        assert_eq!(calls.borrow().len(), 2);
        assert_eq!(calls.borrow()[0].width, 10);
        assert_eq!(calls.borrow()[0].x, 0, "a block box starts at column 0");
    }

    #[test]
    fn test_inline_box_places_at_its_own_column_and_text_resumes_after_it() {
        // The whole point of the baseline path: the sink is handed the box's
        // real column (not 0), and the run following the box is painted after
        // it rather than on a line of its own.
        let doc = Document::parse("ab $x$ cd\n");
        let sizer = AlwaysSizes(CellSize { cols: 4, rows: 1 });
        let tree = layout(&doc, 40, &LayoutConfig::default(), &engine(), &sizer);
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
                    width: 40,
                    height: 3,
                },
                &mut buf,
            )
            .unwrap();

        let calls = calls.borrow();
        assert_eq!(calls.len(), 1, "one inline box painted");
        assert_eq!(calls[0].x, 3, "placed after \"ab \", not at column 0");
        assert_eq!(calls[0].width, 4);
        assert_eq!(calls[0].height, 1, "an inline box is one row");

        let text = String::from_utf8_lossy(&buf);
        assert!(text.contains("ab "), "text before the box: {text:?}");
        assert!(text.contains(" cd"), "text after the box: {text:?}");
        // The cursor is put back at the cell after the box (0-indexed col 7
        // -> 1-indexed 8) so the trailing run lands in the right place.
        assert!(
            text.contains("\x1b[1;8H"),
            "cursor restored after the box: {text:?}"
        );
    }

    /// An empty container occupies its line, and the proof has to be on the
    /// wire: layout can place a gutter run on a row and the painter still owe
    /// the user nothing visible. Asserts the exact bytes for that row — the
    /// CUP to row 3 column 1, the dim SGR the blockquote role resolves to, and
    /// the bar glyph itself — so a regression that drops the row, drops the
    /// glyph, or shifts either one fails here.
    #[test]
    fn test_empty_blockquote_paints_its_gutter_bar_on_its_own_row() {
        let doc = Document::parse("before\n\n>\n\nafter\n");
        let tree = layout(&doc, 40, &LayoutConfig::default(), &engine(), &NullSizer);
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
        let wire = String::from_utf8(buf).expect("utf-8");
        // Row 3 (1-indexed), between the blank row after "before" and the
        // blank row before "after".
        assert!(
            wire.contains("\x1b[3;1H\x1b[0m\x1b[2m\u{2502} \x1b[0m\x1b[K"),
            "the empty quote's row must carry its dim gutter bar: {wire:?}"
        );
        // And it is its own row: the rows either side of it are blank, and
        // "after" is pushed down to row 5 rather than sitting where the quote
        // would have been had it vanished.
        assert!(wire.contains("\x1b[2;1H\x1b[K"), "row 2 blank: {wire:?}");
        assert!(wire.contains("\x1b[4;1H\x1b[K"), "row 4 blank: {wire:?}");
        assert!(
            wire.contains("\x1b[5;1H\x1b[0mafter"),
            "\"after\" stays on row 5: {wire:?}"
        );
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
            row: 0,
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

    /// A writer that fails once, mid-frame, and works again after — the
    /// realistic pty failure (EAGAIN on a full buffer, a short write over
    /// ssh), as opposed to a device that is gone for good.
    struct FailsOnce {
        fail_after: usize,
        written: usize,
        failed: bool,
        seen: Vec<u8>,
    }
    impl Write for FailsOnce {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if !self.failed && self.written + buf.len() > self.fail_after {
                self.failed = true;
                return Err(io::Error::other("would block"));
            }
            self.written += buf.len();
            self.seen.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_frame_closes_the_sync_update_block_after_a_recoverable_write_error() {
        // Mode 2026 is a *global* terminal mode: a frame that returns early
        // with the block still open leaves the user on a shell that has
        // stopped painting, and the exit/panic restore is the only thing left
        // to save them. The close has to be attempted on the error path.
        let doc = Document::parse("hello world\n\nsecond paragraph\n");
        let tree = layout(&doc, 40, &LayoutConfig::default(), &engine(), &NullSizer);
        let mut painter = Painter::new(engine());
        let mut out = FailsOnce {
            fail_after: 20,
            written: 0,
            failed: false,
            seen: Vec::new(),
        };
        let res = painter.frame(
            &tree,
            0,
            Size {
                width: 40,
                height: 10,
            },
            &mut out,
        );
        assert!(res.is_err(), "the paint must still report the failure");
        let wire = String::from_utf8_lossy(&out.seen).into_owned();
        assert!(
            wire.starts_with("\x1b[?2026h"),
            "the block was opened: {wire:?}"
        );
        assert!(
            wire.ends_with("\x1b[?2026l"),
            "the block must be closed even though the paint failed: {wire:?}"
        );
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
