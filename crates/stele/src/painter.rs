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

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::rc::Rc;

use highlight::{HYPERLINK_CLOSE, HighlightCache, hyperlink_open};
use layout::{LayoutTree, Line, LineItem, Reserved, ReservedLine, Run, Semantic, StyleId};
use width::WidthEngine;

use crate::app::{LinkSpan, Match, StatusLine, TocRow, line_text_len};
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
/// Reverse video — the link-selection indicator (DW-6.1) and the TOC
/// overlay's selected row use the same attribute, for the same reason: it is
/// the one thing legible against whatever background either theme variant
/// happens to be painting on.
const SGR_REVERSE: &[u8] = b"\x1b[7m";

/// The search matches to highlight in one frame (DW-4.4).
///
/// Borrowed rather than owned, and passed as an argument rather than stored
/// on the [`Painter`], so [`Painter::frame`]'s "identical arguments always
/// produce identical bytes" promise still holds with search active — hidden
/// per-frame state on the painter would quietly break it.
#[derive(Debug, Clone, Copy, Default)]
pub struct SearchOverlay<'a> {
    /// Every match in the document, in tree order. The painter takes the
    /// slice whole and picks out the visible ones itself; the caller has no
    /// viewport to filter by.
    pub matches: &'a [Match],
    /// Index into `matches` of the one the reader is on, painted in the
    /// distinct [`Semantic::SearchCurrent`] role. `None` when there is
    /// nothing to be current among.
    pub current: Option<usize>,
}

impl SearchOverlay<'_> {
    fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }
}

/// Both document overlays for one frame: search matches to highlight
/// (DW-4.4) and the run ranges of the selected link (DW-6.1).
///
/// One parameter rather than two because they travel together everywhere —
/// [`Painter::frame_with_overlays`] takes them, [`RowOverlays`] is their
/// per-line projection — and because naming the pair is what says they are
/// *composable features of one frame* rather than alternative frame kinds.
/// Both default to off, which is what keeps `frame`, `frame_with_status`,
/// `frame_with_search` and `frame_with_selection` as one-liners over the same
/// implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameOverlays<'a> {
    pub search: SearchOverlay<'a>,
    /// Absolute-line-addressed run ranges of the selected link. Empty outside
    /// [`crate::app::Mode::LinkSelect`].
    pub selected: &'a [LinkSpan],
}

/// One restyled stretch of a single tree line: a half-open byte range into
/// that line's painted text, and the role to paint it in.
///
/// A [`Match`] can span a wrap boundary, so one match becomes one `Span` per
/// line it touches — this is the projected, per-row form the paint loop can
/// actually use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
    role: Semantic,
}

/// Everything one painted row needs beyond the line itself: both overlays,
/// already resolved for *this* line.
///
/// Bundled rather than passed as two more parameters because the paint loop
/// hands them down three levels — `frame_body` → `paint_line` → `paint_items`
/// → `paint_run` — and four positional arguments of which two are overlays is
/// exactly the shape `cc-routine-and-class-design` warns about. It is also
/// the place to say the thing neither field says alone: **the two overlays
/// compose.** A search span replaces a run's style role; a selection sets the
/// reverse attribute on top of whatever role won. A match inside the selected
/// link reads as both.
#[derive(Debug, Clone, Copy, Default)]
struct RowOverlays<'a> {
    /// Search highlights on this line, addressed in byte offsets into the
    /// line's painted text (DW-4.4). Empty when no search is active.
    spans: &'a [Span],
    /// The inclusive `LineItem` index range of the selected link's run group
    /// on this line, if the selected link paints here at all (DW-6.1).
    selected_items: Option<(usize, usize)>,
}

impl RowOverlays<'_> {
    /// Whether the item at `index` is part of the selected link.
    fn selects(&self, index: usize) -> bool {
        self.selected_items
            .is_some_and(|(first, last)| index >= first && index <= last)
    }
}

/// Whole-viewport immediate painter: no cell-grid differ, one full repaint
/// per [`Painter::frame`] call, wrapped in a mode-2026 synchronized-update
/// block. Deterministic given `(tree, scroll, size, status, overlay)`.
pub struct Painter {
    width_engine: WidthEngine,
    media: Box<dyn MediaSink>,
    decor: Box<dyn Decor>,
    /// Memoized [`Decor::highlight`] results (DW-4.6/DW-4.7). The one piece
    /// of state that survives between frames on purpose: highlighting a
    /// code line is a pure function of `(text, lang)`, and re-deriving it
    /// per frame is what made code-heavy frames ~40x slower than their
    /// budget. See [`highlight::HighlightCache`] for the bound and for why
    /// a timeout fallback is never retained.
    highlight_cache: HighlightCache,
}

impl Painter {
    /// Builds a painter using `width_engine` to recompute run widths after
    /// [`Decor::highlight`], with the true no-op media sink and the
    /// structural decor resolver as defaults — the stubs this phase paints
    /// with until P6/P7 register their own.
    pub fn new(width_engine: WidthEngine) -> Self {
        Painter::with_cache_capacity(width_engine, highlight::DEFAULT_CACHE_CAPACITY)
    }

    /// [`Painter::new`] with an explicit highlight-cache bound.
    ///
    /// `capacity == 0` disables memoization, which reproduces the
    /// pre-Phase-4 paint path exactly — that is the baseline arm of DW-4.7's
    /// committed benchmark, kept as a real constructor rather than a test
    /// hook so "10x faster than before" is measured against a *before* that
    /// still compiles and still runs.
    pub fn with_cache_capacity(width_engine: WidthEngine, capacity: usize) -> Self {
        Painter {
            width_engine,
            media: Box::new(NoopMediaSink),
            decor: Box::new(StructuralDecor),
            highlight_cache: HighlightCache::new(capacity),
        }
    }

    /// Registers a media sink (P6). Replaces the no-op default.
    pub fn register_media(&mut self, media: Box<dyn MediaSink>) {
        self.media = media;
    }

    /// Registers a decor resolver (P7). Replaces the structural default.
    ///
    /// Drops the highlight cache with it. Highlight *runs* happen to be
    /// theme-independent today (the theme only enters at `resolve`), so `T`
    /// would survive without this — but a decor swap is precisely the event
    /// that invalidates a memo of a decor's answers, and one `clear()` per
    /// keypress is free next to the relayout the same keypress triggers.
    pub fn register_decor(&mut self, decor: Box<dyn Decor>) {
        self.decor = decor;
        self.highlight_cache.clear();
    }

    /// Tells the registered sink the document was reloaded (`--watch`), so it
    /// can drop per-`NodeId` state a re-parse has invalidated. Forwarded
    /// rather than exposed as a sink accessor: the painter owns the sink, and
    /// handing it out would let a caller paint through it outside a frame.
    pub fn reload_media(&mut self, doc: std::rc::Rc<ast::Document>, out: &mut dyn Write) {
        self.media.reload_document(doc, out);
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
        self.frame_with_status(tree, scroll, size, &StatusLine::default(), out)
    }

    /// [`Painter::frame`] plus a reserved status row (DW-1.1) painted from
    /// `status`, one row below `size.height` content rows — so `size` here
    /// is the *content* viewport (what [`crate::app::AppState::size`]
    /// already is once the caller has subtracted the status row from the
    /// real terminal height), not the full terminal.
    ///
    /// This is the real implementation; [`Painter::frame`] is this call with
    /// an empty [`StatusLine`], which is why every frame — not just the ones
    /// that bother to pass real status content — reserves the row.
    pub fn frame_with_status(
        &mut self,
        tree: &LayoutTree,
        scroll: usize,
        size: Size,
        status: &StatusLine,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        self.frame_with_overlays(tree, scroll, size, status, FrameOverlays::default(), out)
    }

    /// [`Painter::frame_with_status`] plus search-match highlighting
    /// (DW-4.4). A thin default of [`Painter::frame_with_overlays`] with no
    /// link selected.
    pub fn frame_with_search(
        &mut self,
        tree: &LayoutTree,
        scroll: usize,
        size: Size,
        status: &StatusLine,
        overlay: SearchOverlay<'_>,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        let overlays = FrameOverlays {
            search: overlay,
            selected: &[],
        };
        self.frame_with_overlays(tree, scroll, size, status, overlays, out)
    }

    /// [`Painter::frame_with_status`] plus the link-selection indicator
    /// (DW-6.1). A thin default of [`Painter::frame_with_overlays`] with no
    /// search active.
    pub fn frame_with_selection(
        &mut self,
        tree: &LayoutTree,
        scroll: usize,
        size: Size,
        status: &StatusLine,
        selected: &[LinkSpan],
        out: &mut dyn Write,
    ) -> io::Result<()> {
        let overlays = FrameOverlays {
            search: SearchOverlay::default(),
            selected,
        };
        self.frame_with_overlays(tree, scroll, size, status, overlays, out)
    }

    /// The one document-frame implementation: content rows, both overlays,
    /// and the reserved status row, inside one synchronized-update block.
    ///
    /// **Both overlays, in one call, on purpose.** Search highlighting
    /// (DW-4.4) and the link-selection indicator (DW-6.1) arrived in
    /// different phases and each began as its own wrapper around the paint
    /// path. Two wrappers is two paint paths the moment a reader has a query
    /// active and then presses `Tab` — whichever wrapper the event loop
    /// picked would silently drop the other's highlighting. They are not
    /// alternatives: a search restyles bytes by *role*, a selection sets the
    /// reverse *attribute* over whatever role won, so they compose rather
    /// than compete. `frame`, `frame_with_status`, `frame_with_search` and
    /// `frame_with_selection` are all this call with features defaulted off,
    /// which keeps every existing call site working and every frame on one
    /// code path.
    ///
    /// Determinism still holds: like `overlay`, `selected` is an argument
    /// rather than painter state, so identical arguments still produce
    /// identical bytes.
    pub fn frame_with_overlays(
        &mut self,
        tree: &LayoutTree,
        scroll: usize,
        size: Size,
        status: &StatusLine,
        overlays: FrameOverlays<'_>,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        let spans = visible_spans(tree, scroll, size.height, overlays.search);
        out.write_all(SYNC_BEGIN)?;
        let painted = self
            .frame_body(tree, scroll, size, &spans, overlays.selected, out)
            .and_then(|()| self.paint_status_row(status, size, out));
        let closed = out.write_all(SYNC_END).and_then(|()| out.flush());
        painted.and(closed)
    }

    /// Paints the TOC overlay (DW-3.2): `rows` over the whole content
    /// viewport, then the same reserved status row every other frame gets.
    ///
    /// **It is a frame, not a decoration**, and that is what DW-3.3 turns on.
    /// It opens the media sink's frame boundary exactly as
    /// [`Painter::frame_with_status`] does, which takes every live placement
    /// off the screen; because no reserved box is painted afterwards, nothing
    /// puts one back, so an overlay frame ends with the terminal drawing
    /// nothing — and the next document frame re-places precisely the boxes it
    /// paints. Skipping `begin_frame` here would leave the previous screen's
    /// images floating over the overlay, which is the exact bug the frame
    /// boundary was introduced to kill.
    ///
    /// Rows past the end of `rows` are blanked rather than left alone: the
    /// overlay owns the whole viewport, so document text under a short TOC
    /// must not show through. The build stamp stands down for the same
    /// reason.
    pub fn frame_overlay(
        &mut self,
        rows: &[TocRow],
        size: Size,
        status: &StatusLine,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        out.write_all(SYNC_BEGIN)?;
        let painted = self
            .overlay_body(rows, size, out)
            .and_then(|()| self.paint_status_row(status, size, out));
        let closed = out.write_all(SYNC_END).and_then(|()| out.flush());
        painted.and(closed)
    }

    /// The body of [`frame_overlay`](Self::frame_overlay), split out for the
    /// same reason [`frame_body`](Self::frame_body) is: every `?` in here is
    /// caught before the synchronized-update block is closed.
    fn overlay_body(&mut self, rows: &[TocRow], size: Size, out: &mut dyn Write) -> io::Result<()> {
        self.media.begin_frame(out);
        for row in 0..size.height {
            write!(out, "\x1b[{};1H", row + 1)?;
            if let Some(entry) = rows.get(usize::from(row)) {
                let sanitized = sanitize(&entry.text);
                let (clipped, _) = clip_to_width(&sanitized, &self.width_engine, size.width);
                // Reverse video for the selection: the overlay has no theme
                // roles of its own, and reverse is the one attribute that is
                // legible against whatever background the terminal is using
                // in either theme variant.
                if entry.selected {
                    out.write_all(b"\x1b[7m")?;
                }
                out.write_all(clipped.as_bytes())?;
                if entry.selected {
                    out.write_all(SGR_RESET)?;
                }
            }
            out.write_all(CLEAR_TO_EOL)?;
        }
        Ok(())
    }

    /// Paints `status` into the row immediately below the `size.height`
    /// content rows (DW-1.1: content occupies `0..size.height`, the status
    /// row is the one after it — never inside that range, so it can never
    /// overpaint content).
    ///
    /// Row arithmetic is done in `u32` specifically so `size.height ==
    /// u16::MAX` cannot wrap the 1-indexed row number back to 0 — a
    /// degenerate viewport this painter otherwise promises never to panic
    /// on (see [`Painter::frame`]'s doc comment).
    fn paint_status_row(
        &mut self,
        status: &StatusLine,
        size: Size,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        if size.width == 0 {
            return Ok(());
        }
        let row = u32::from(size.height) + 1;
        write!(out, "\x1b[{row};1H")?;
        let text = status.render();
        if !text.is_empty() {
            let sanitized = sanitize(&text);
            let (clipped, _) = clip_to_width(&sanitized, &self.width_engine, size.width);
            if !clipped.is_empty() {
                write!(out, "\x1b[2m")?;
                out.write_all(clipped.as_bytes())?;
                out.write_all(SGR_RESET)?;
            }
        }
        out.write_all(CLEAR_TO_EOL)?;
        Ok(())
    }

    /// The body of [`frame`](Self::frame), between the synchronized-update
    /// begin and end. Split out so every `?` in here is caught by the caller
    /// rather than escaping past the closing sequence.
    fn frame_body(
        &mut self,
        tree: &LayoutTree,
        scroll: usize,
        size: Size,
        spans: &BTreeMap<usize, Vec<Span>>,
        selected: &[LinkSpan],
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
                // Both overlays are addressed by *absolute line index*, not
                // by viewport row, so both are looked up by the line this row
                // is showing — scrolling must not move either highlight onto
                // different text.
                let overlays = RowOverlays {
                    spans: spans.get(&idx).map_or(&[][..], Vec::as_slice),
                    selected_items: selected
                        .iter()
                        .find(|span| span.line == idx)
                        .map(|span| (span.first_item, span.last_item)),
                };
                self.paint_line(line, row, size, overlays, out)?;
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
        overlays: RowOverlays<'_>,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        match line {
            Line::Items(items) => self.paint_items(items, row, size.width, overlays, out),
            // A reserved media row carries no text runs: no searchable text
            // (see `app::append_line_text`) and no link runs, so neither
            // overlay can address it.
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
            let painted = self.paint_run(run, remaining, 0, &[], false, out)?;
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
    /// an inline media box knows where on the row it starts — and the
    /// absolute *byte* offset, which is what a search span is addressed in.
    ///
    /// The two counters cannot be one: a run's byte length and its cell
    /// width differ for anything outside printable ASCII, and a box consumes
    /// columns while contributing no bytes at all.
    fn paint_items(
        &mut self,
        items: &[LineItem],
        row: u16,
        width: u16,
        overlays: RowOverlays<'_>,
        out: &mut dyn Write,
    ) -> io::Result<()> {
        let mut col: u16 = 0;
        let mut byte = 0usize;
        let mut painted_any = false;
        for (index, item) in items.iter().enumerate() {
            let remaining = width.saturating_sub(col);
            if remaining == 0 {
                break;
            }
            let reverse = overlays.selects(index);
            match item {
                LineItem::Run(run) => {
                    let painted =
                        self.paint_run(run, remaining, byte, overlays.spans, reverse, out)?;
                    byte += run.text.len();
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
    ///
    /// `base` is the run's byte offset within its line and `spans` the
    /// line's search highlights, both in the same coordinate system, so a
    /// match that starts mid-run restyles exactly its own bytes.
    ///
    /// `reverse` paints this run as the selected link (DW-6.1). The two
    /// overlays are applied at different levels and therefore compose rather
    /// than fight: a search span replaces a run's *role* before any SGR is
    /// written, while `reverse` is an attribute emitted as its own SGR
    /// *after* [`write_sgr`] — a match inside the selected link still reads
    /// as a match, and still reads as selected. It has to come after
    /// `write_sgr` because that opens with an unconditional reset, which
    /// would wipe a reverse attribute set ahead of it.
    fn paint_run(
        &mut self,
        run: &Run,
        remaining: u16,
        base: usize,
        spans: &[Span],
        reverse: bool,
        out: &mut dyn Write,
    ) -> io::Result<PaintedRun> {
        let expanded = self.expand(run);
        // Search highlighting is applied *after* syntax highlighting and
        // overrides it, which is the phase's stated precedence: a match
        // inside a code block must read as a match first (DW-4.4).
        let overlaid = if spans.is_empty() {
            None
        } else {
            Some(apply_overlay(&expanded, base, spans))
        };
        let runs: &[Run] = overlaid.as_deref().unwrap_or(&expanded);
        let mut painted = PaintedRun::default();
        for expanded_run in runs {
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
            if reverse {
                out.write_all(SGR_REVERSE)?;
            }
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

    /// The runs one tree run paints as: a code-block run's syntax-highlighted
    /// partition, memoized across frames, or the run itself for everything
    /// else.
    ///
    /// **This is the frame-budget fix (DW-4.6/DW-4.7).** Before it, every
    /// visible code line was re-parsed by tree-sitter on every frame — a
    /// measured 2,525 µs against a 64 µs budget on code-heavy viewports,
    /// invisible while scrolling was the only interaction and unmissable once
    /// search repaints on every keystroke. The answer is a pure function of
    /// `(text, lang)`, so the second frame asks the cache instead of the
    /// highlighter. `Rc<[Run]>` rather than `Vec<Run>` so a hit is a refcount
    /// bump and not a deep copy of the strings.
    fn expand(&mut self, run: &Run) -> Rc<[Run]> {
        // `Decor::highlight` is code-block-specific (see the decor module
        // docs): every other run passes through unchanged.
        if run.style_id != StyleId::Semantic(Semantic::CodeBlock) {
            return Rc::from(vec![run.clone()]);
        }
        // `run.aux` carries the code fence's language (threaded from
        // layout), so the highlighter can pick a grammar instead of always
        // falling back to a plain block.
        let decor = &self.decor;
        self.highlight_cache
            .get_or_compute(&run.text, run.aux.as_deref(), |text, lang| {
                decor.highlight(text, lang)
            })
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

/// Projects `overlay`'s matches onto the rows `scroll..scroll + height`,
/// keyed by tree line index.
///
/// Two jobs, and both need the tree. A match is stored as one range that may
/// run past the end of the line it starts on (see [`Match`]), so it has to be
/// *cut* at each line's length to become per-row spans — that is what makes a
/// match straddling a wrap boundary highlight on both lines rather than on
/// neither. And only the visible rows are worth cutting: a document may hold
/// thousands of matches and a viewport shows tens of lines.
fn visible_spans(
    tree: &LayoutTree,
    scroll: usize,
    height: u16,
    overlay: SearchOverlay<'_>,
) -> BTreeMap<usize, Vec<Span>> {
    let mut spans: BTreeMap<usize, Vec<Span>> = BTreeMap::new();
    if overlay.is_empty() || height == 0 {
        return spans;
    }
    let last_visible = scroll.saturating_add(height as usize);
    for (index, found) in overlay.matches.iter().enumerate() {
        // Matches are in tree order, so everything past the viewport's
        // bottom is past it for good.
        if found.line >= last_visible {
            break;
        }
        let role = if overlay.current == Some(index) {
            Semantic::SearchCurrent
        } else {
            Semantic::SearchMatch
        };
        project_match(tree, found, role, scroll, last_visible, &mut spans);
    }
    spans
}

/// Cuts one match into per-line spans, keeping only those inside
/// `scroll..last_visible`.
///
/// Walks forward from the match's own line, subtracting each line's text
/// length from what is left of the range. A match that ends inside its first
/// line — nearly all of them — costs one iteration.
fn project_match(
    tree: &LayoutTree,
    found: &Match,
    role: Semantic,
    scroll: usize,
    last_visible: usize,
    into: &mut BTreeMap<usize, Vec<Span>>,
) {
    let mut line = found.line;
    let mut start = found.range.start;
    let mut remaining = found.range.len();
    while remaining > 0 && line < last_visible && line < tree.line_count() {
        let length = line_text_len(tree, line);
        // A zero-length line inside the block — a blank line in a code fence,
        // the gap between the items of a loose list. It is a real row that
        // contributes no bytes, so the match simply continues past it.
        //
        // This case has to be taken *before* the staleness guard below, and
        // getting that order wrong is the whole reason this comment exists:
        // on such a line `start` and `length` are both 0, so `start >=
        // length` reads true and the walk used to abandon every fragment
        // after the blank row. `fn foo() {` / blank / `}` searched for `{}`
        // highlighted the brace and left the closing one plain.
        if length == 0 && start == 0 {
            line += 1;
            continue;
        }
        // The match claims bytes this line does not have: the tree it was
        // computed against is not the tree being painted. Stop rather than
        // restyle whatever now sits at those offsets. Reachable only with a
        // genuinely out-of-range offset now that an empty line is handled
        // above — `start` is 0 on every line after the first.
        if start >= length {
            return;
        }
        let end = (start + remaining).min(length);
        if line >= scroll {
            into.entry(line)
                .or_default()
                .push(Span { start, end, role });
        }
        remaining -= end - start;
        line += 1;
        start = 0;
    }
}

/// Re-partitions `runs` at `spans`' boundaries, restyling every covered
/// piece with the span's role. `base` is the byte offset of `runs`' first
/// character within its line, the coordinate system the spans are in.
///
/// Only called when there is at least one span on the line — the no-search
/// path never allocates here.
fn apply_overlay(runs: &[Run], base: usize, spans: &[Span]) -> Vec<Run> {
    let mut out = Vec::with_capacity(runs.len());
    let mut offset = base;
    for run in runs {
        split_run(run, offset, spans, &mut out);
        offset += run.text.len();
    }
    out
}

/// Splits one run at every span boundary that falls inside it, appending the
/// pieces to `out` in paint order. `base` is this run's byte offset within
/// its line.
fn split_run(run: &Run, base: usize, spans: &[Span], out: &mut Vec<Run>) {
    let length = run.text.len();
    let mut cursor = 0usize;
    for span in spans {
        // Clip the span into this run's own 0..length coordinates.
        let start = span.start.saturating_sub(base).min(length).max(cursor);
        let end = span.end.saturating_sub(base).min(length);
        if end <= start {
            continue;
        }
        // Offsets come from matching over this very text, so they land on
        // char boundaries by construction. Checked anyway: slicing a `&str`
        // mid-character panics, and this runs inside a painted frame where
        // the cheap failure is a missed highlight, not a dead viewer.
        if !run.text.is_char_boundary(start) || !run.text.is_char_boundary(end) {
            continue;
        }
        push_piece(run, cursor..start, None, out);
        push_piece(run, start..end, Some(span.role), out);
        cursor = end;
    }
    push_piece(run, cursor..length, None, out);
}

/// Appends `run`'s `range` as its own run, restyled to `role` when one is
/// given. An empty range appends nothing.
fn push_piece(
    run: &Run,
    range: std::ops::Range<usize>,
    role: Option<Semantic>,
    out: &mut Vec<Run>,
) {
    if range.is_empty() {
        return;
    }
    out.push(Run {
        text: run.text[range].to_string(),
        style_id: role.map_or(run.style_id, StyleId::Semantic),
        // Unset, exactly as `Decor::highlight` leaves it: the painter
        // re-measures every piece through the width engine.
        width: 0,
        // Carried through so a split link piece keeps its destination. The
        // matched piece's own `style_id` is no longer `Link`, so it paints
        // as a highlight rather than as a hyperlink — search wins, per
        // DW-4.4, and the unmatched halves of the label still link.
        aux: run.aux.clone(),
    });
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
    // Delegated rather than enumerated. This function used to carry its own
    // copy of the range list and fell a range behind `highlight`'s when the
    // deprecated format controls were added there — the drift the shared
    // definition exists to make impossible.
    highlight::strip_display_hazards(text)
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

/// The screen columns each item of a text line occupies, as
/// `(start, end_exclusive)` pairs indexed exactly as `items` is.
///
/// The mouse hit-test's oracle (DW-6.6). It **re-measures** through the same
/// [`sanitize`] + [`clip_to_width`] pair [`Painter::paint_items`] paints with,
/// rather than summing `Run.width`: a run that lies about its own width would
/// otherwise put the clickable region somewhere the glyphs are not, and
/// `docs/code-standards.md` names trusting `Run.width` as the mistake that
/// hides exactly this class of bug.
///
/// An item that the row ran out of room for gets a zero-width entry at the
/// clip column, so indices stay aligned with `items` and a click past the clip
/// matches nothing.
pub(crate) fn item_columns(
    items: &[LineItem],
    engine: &WidthEngine,
    width: u16,
) -> Vec<(u16, u16)> {
    let mut columns = Vec::with_capacity(items.len());
    let mut col: u16 = 0;
    for item in items {
        let remaining = width.saturating_sub(col);
        let painted = match item {
            LineItem::Run(run) => {
                let sanitized = sanitize(&run.text);
                clip_to_width(&sanitized, engine, remaining).1
            }
            LineItem::Box(reserved) => reserved.cols.min(remaining),
        };
        columns.push((col, col.saturating_add(painted)));
        col = col.saturating_add(painted);
    }
    columns
}

/// Writes the SGR sequence for `style`: an unconditional reset (so runs
/// never bleed a previous run's attributes) followed by one combined
/// attribute/color sequence when `style` carries any.
///
/// Emits the same bytes it always has, without allocating. It used to build
/// a `Vec<String>` of `format!`ed codes and `join(";")` them — three or four
/// heap allocations per run, on a path that runs once per styled span per
/// frame. A syntax-highlighted code line is ~20 spans, so a code-heavy
/// viewport was spending ~90 µs a frame here, measured: more than the entire
/// rest of a cached frame's paint cost put together, and the largest single
/// item left once the highlight cache removed the tree-sitter parse
/// (DW-4.7). The bytes are assembled straight into `out` instead.
fn write_sgr(out: &mut dyn Write, style: &Style) -> io::Result<()> {
    out.write_all(SGR_RESET)?;
    // The reset alone *is* the plain style, and plain is the common case
    // for prose — this returns before touching the parameter machinery.
    if *style == Style::default() {
        return Ok(());
    }
    out.write_all(b"\x1b[")?;
    let mut written = false;
    for (enabled, code) in [
        (style.bold, b"1".as_slice()),
        (style.dim, b"2".as_slice()),
        (style.italic, b"3".as_slice()),
        (style.underline, b"4".as_slice()),
    ] {
        if !enabled {
            continue;
        }
        written = write_separated(out, written, code)?;
    }
    if let Some(fg) = style.fg {
        written = write_separated(out, written, b"38;2")?;
        write_channels(out, fg)?;
    }
    if let Some(bg) = style.bg {
        write_separated(out, written, b"48;2")?;
        write_channels(out, bg)?;
    }
    out.write_all(b"m")
}

/// Writes one SGR parameter, prefixed with `;` if something came before it.
/// Returns `true`, the updated "something has been written" flag, so the
/// caller threads it through rather than tracking it at each call site.
fn write_separated(out: &mut dyn Write, written: bool, code: &[u8]) -> io::Result<bool> {
    if written {
        out.write_all(b";")?;
    }
    out.write_all(code)?;
    Ok(true)
}

/// The `;r;g;b` tail of a truecolor parameter, each channel rendered into a
/// stack buffer rather than a `String`.
fn write_channels(out: &mut dyn Write, color: Color) -> io::Result<()> {
    for channel in [color.r, color.g, color.b] {
        out.write_all(b";")?;
        out.write_all(decimal(channel).as_bytes())?;
    }
    Ok(())
}

/// A `u8` as decimal ASCII, in a fixed stack buffer. Three digits is the
/// whole range, so this never truncates and never allocates.
fn decimal(value: u8) -> DecimalU8 {
    let mut buf = [0u8; 3];
    let mut len = 0;
    let mut remaining = value;
    loop {
        buf[2 - len] = b'0' + (remaining % 10);
        len += 1;
        remaining /= 10;
        if remaining == 0 {
            break;
        }
    }
    DecimalU8 { buf, len }
}

/// The decimal digits of a `u8`, right-aligned in a three-byte buffer.
struct DecimalU8 {
    buf: [u8; 3],
    len: usize,
}

impl DecimalU8 {
    fn as_bytes(&self) -> &[u8] {
        &self.buf[3 - self.len..]
    }
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

    /// The paint boundary strips *exactly* what `highlight::is_display_hazard`
    /// names — no narrower.
    ///
    /// This is the anti-drift pin. `sanitize` used to enumerate its own
    /// ranges, and when the deprecated format controls were added to the OSC 8
    /// sanitizer this function kept the old set: two barricades in the same
    /// codebase disagreeing about what a control character is. Anyone who
    /// re-inlines a list here fails this test rather than shipping the same
    /// divergence again.
    #[test]
    fn test_sanitize_strips_the_whole_shared_hazard_set_not_a_local_subset() {
        for cp in 0u32..=0x2100 {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            let survived = sanitize(&c.to_string()).contains(c);
            assert_eq!(
                survived,
                !highlight::is_display_hazard(c),
                "U+{cp:04X}: the painter and the shared hazard predicate disagree \
                 (painter kept it: {survived})"
            );
        }
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

    // ---- Search highlighting and the highlight cache (Phase 4) -----------

    use std::cell::Cell;

    use crossterm::event::{KeyCode, KeyEvent};
    use highlight::Highlighted;

    use crate::app::{AppState, FileInfo};
    use crate::decor::themed::ThemedDecor;

    /// Lays out `source` and drives a real search through the real key
    /// table, so these tests exercise the same path the binary does rather
    /// than a hand-built match list that could disagree with it.
    fn searched(source: &str, size: Size, query: &str) -> AppState {
        let tree = layout(
            &Document::parse(source),
            size.width,
            &LayoutConfig::default(),
            &engine(),
            &NullSizer,
        );
        let mut state = AppState::new(tree, size, FileInfo::default());
        for code in std::iter::once(KeyCode::Char('/'))
            .chain(query.chars().map(KeyCode::Char))
            .chain(std::iter::once(KeyCode::Enter))
        {
            state.handle_key_event(KeyEvent::from(code));
        }
        state
    }

    /// One frame of `state`, with its status row and search overlay, as a
    /// UTF-8 string of the exact bytes that went to the terminal.
    fn painted(painter: &mut Painter, state: &mut AppState) -> String {
        let status = state.status();
        let mut buf = Vec::new();
        painter
            .frame_with_search(
                state.tree(),
                state.scroll(),
                state.size(),
                &status,
                state.search_overlay(),
                &mut buf,
            )
            .expect("painting to a Vec cannot fail");
        String::from_utf8(buf).expect("utf-8")
    }

    /// The exact SGR introducer a role resolves to through `decor`, so these
    /// tests compare against what the style table actually says rather than
    /// against a hardcoded escape that would silently stop matching it.
    fn sgr_for(decor: &dyn Decor, role: Semantic) -> String {
        let mut buf = Vec::new();
        write_sgr(&mut buf, &decor.resolve(StyleId::Semantic(role))).expect("Vec write");
        String::from_utf8(buf).expect("utf-8")
    }

    /// How many bytes of text `wire` actually paints in `sgr`'s style: the
    /// run of characters after each occurrence of that introducer, up to the
    /// next escape.
    ///
    /// Counting bytes rather than asserting one substring is what catches a
    /// *partially* highlighted match. "The wire contains the highlight
    /// sequence" is satisfied by a single highlighted character out of four,
    /// which is exactly the defect this exists to detect.
    fn highlighted_bytes(wire: &str, sgr: &str) -> usize {
        wire.match_indices(sgr)
            .map(|(at, _)| {
                let rest = &wire[at + sgr.len()..];
                rest.find('\x1b').unwrap_or(rest.len())
            })
            .sum()
    }

    #[test]
    fn test_dw_4_4_every_visible_match_is_highlighted_and_the_current_one_is_distinct() {
        // Four hits on four separate lines, all inside a 10-row viewport.
        let source = "needle one\n\nfiller\n\nneedle two\n\nneedle three\n\nneedle four\n";
        let size = Size {
            width: 40,
            height: 10,
        };
        let mut state = searched(source, size, "needle");
        assert_eq!(state.search().matches.len(), 4);

        let mut painter = Painter::new(engine());
        let wire = painted(&mut painter, &mut state);

        let plain_match = sgr_for(painter.decor.as_ref(), Semantic::SearchMatch);
        let current = sgr_for(painter.decor.as_ref(), Semantic::SearchCurrent);
        assert_ne!(
            plain_match, current,
            "the current match must resolve to a different style than the rest"
        );

        // Every visible match is highlighted...
        assert_eq!(
            wire.matches(&format!("{plain_match}needle")).count(),
            3,
            "three non-current matches must each be highlighted: {wire:?}"
        );
        // ...and exactly one of them is the current one.
        assert_eq!(
            wire.matches(&format!("{current}needle")).count(),
            1,
            "exactly one match may wear the current style: {wire:?}"
        );

        // Stepping to the next match moves the distinct style with it, and
        // does not create a second one.
        state.handle_key_event(KeyEvent::from(KeyCode::Char('n')));
        let wire = painted(&mut painter, &mut state);
        assert_eq!(wire.matches(&format!("{current}needle")).count(), 1);
        assert_eq!(wire.matches(&format!("{plain_match}needle")).count(), 3);
    }

    /// Nothing outside a match may be restyled. A highlight that leaked into
    /// the surrounding run would still pass a "the match is highlighted"
    /// assertion, so this pins the other edge: the words either side keep
    /// their own style and the matched text is exactly the query.
    #[test]
    fn test_a_highlight_covers_the_match_and_nothing_around_it() {
        let size = Size {
            width: 40,
            height: 5,
        };
        let mut state = searched("before needle after\n", size, "needle");
        let mut painter = Painter::new(engine());
        let wire = painted(&mut painter, &mut state);
        let current = sgr_for(painter.decor.as_ref(), Semantic::SearchCurrent);

        // The row reads: plain "before ", highlighted "needle", plain " after".
        let highlighted = format!("{current}needle");
        assert!(wire.contains(&highlighted), "{wire:?}");
        let after = wire
            .split_once(&highlighted)
            .expect("the highlight is on the wire")
            .1;
        assert!(
            after.starts_with("\x1b[0m after"),
            "the text after the match must revert to its own style: {after:?}"
        );
        assert!(
            wire.contains("before "),
            "the text before the match is untouched: {wire:?}"
        );
    }

    /// The wrap-boundary constraint, on the wire: one match split across two
    /// rows must be highlighted on *both* of them. The wrap point is read
    /// out of the laid-out tree rather than assumed.
    #[test]
    fn test_dw_4_4_a_match_spanning_a_wrap_boundary_is_highlighted_on_both_lines() {
        let source: String = (0..200).map(|i| format!("w{i:04} ")).collect();
        let size = Size {
            width: 40,
            height: 10,
        };
        let tree = layout(
            &Document::parse(&source),
            size.width,
            &LayoutConfig::default(),
            &engine(),
            &NullSizer,
        );
        let mut first = String::new();
        let mut second = String::new();
        crate::app::append_line_text(&tree, 0, &mut first);
        crate::app::append_line_text(&tree, 1, &mut second);
        assert_eq!(tree.block_at(0), tree.block_at(1), "the fixture must wrap");

        let tail: String = first[first.len() - 3..].to_string();
        let head: String = second[..3].to_string();
        let query = format!("{tail}{head}");

        let mut state = searched(&source, size, &query);
        assert_eq!(state.search().matches.len(), 1, "one straddling match");

        let mut painter = Painter::new(engine());
        let wire = painted(&mut painter, &mut state);
        let current = sgr_for(painter.decor.as_ref(), Semantic::SearchCurrent);

        assert!(
            wire.contains(&format!("{current}{tail}")),
            "the first half must be highlighted on row 1: {wire:?}"
        );
        assert!(
            wire.contains(&format!("{current}{head}")),
            "the second half must be highlighted on row 2: {wire:?}"
        );
        // Two pieces, one per row — not one piece painted twice on one row.
        let row_two = wire.split("\x1b[2;1H").nth(1).expect("a second row");
        assert!(
            row_two.starts_with(&format!("{current}{head}")),
            "row 2 must open with the continuation half, highlighted: {row_two:?}"
        );
        // ...and between them they account for the whole query, not part of it.
        assert_eq!(
            highlighted_bytes(&wire, &current),
            query.len(),
            "every byte of the match must be highlighted: {wire:?}"
        );
    }

    /// The same walk, over the case that is *not* a wrap: a blank line inside
    /// a code fence.
    ///
    /// Lines are joined per block for matching, so a match can span a blank
    /// line exactly as it spans a wrap — and the projection has to tell that
    /// legitimately empty line apart from a stale offset into a tree that has
    /// been replaced. It did not: the `start >= length` staleness guard read
    /// `0 >= 0` on the empty row and abandoned the rest of the match, so
    /// `{` was highlighted and `}` was painted plain two rows below. Byte
    /// counting rather than a `contains` check is what makes this visible —
    /// a partially highlighted match still contains the highlight sequence.
    #[test]
    fn test_dw_4_4_a_match_spanning_a_blank_line_inside_a_block_is_highlighted_in_full() {
        let size = Size {
            width: 40,
            height: 8,
        };
        for (source, query) in [
            // A blank line inside a code fence, and the brace pair a reader
            // would plausibly search for across it.
            ("```rust\nfn foo() {\n\n}\n```\n", "{}"),
            // The same shape with a real fragment on each of two
            // non-adjacent rows, rather than one character on each.
            ("```rust\nabc\n\ndef\n```\n", "cdef"),
            // A loose list: the same zero-length continuation line, reached
            // through completely different layout machinery. The query spans
            // from the first item's text into the second item's bullet,
            // which is where the block's joined text puts the boundary.
            ("- item one\n\n- item two\n", "one\u{2022}"),
        ] {
            let mut state = searched(source, size, query);
            assert_eq!(
                state.search().matches.len(),
                1,
                "fixture {source:?} must produce exactly one match for {query:?}"
            );
            // The blank line is real, and it is inside the same block — the
            // whole premise of the case.
            let spanned = &state.search().matches[0];
            assert!(
                (spanned.line + 1..spanned.line + spanned.range.end)
                    .any(|line| crate::app::line_text_len(state.tree(), line) == 0),
                "fixture {source:?} must span a zero-length line, or it cannot \
                 distinguish this defect from an ordinary one-line match"
            );

            let mut painter = Painter::new(engine());
            let wire = painted(&mut painter, &mut state);
            let current = sgr_for(painter.decor.as_ref(), Semantic::SearchCurrent);
            assert_eq!(
                highlighted_bytes(&wire, &current),
                query.len(),
                "every byte of {query:?} must be highlighted in {source:?}: {wire:?}"
            );
        }
    }

    /// The stated precedence: inside a code block that is *also* syntax
    /// highlighted, the search highlight wins. Run with the real themed
    /// decor, so the token under the match genuinely carries a capture style
    /// that has to be overridden.
    #[test]
    fn test_dw_4_4_a_search_highlight_wins_over_syntax_highlighting_in_a_code_block() {
        let source = "```rust\nlet needle = 1;\n```\n";
        let size = Size {
            width: 40,
            height: 6,
        };
        let mut state = searched(source, size, "needle");
        assert_eq!(state.search().matches.len(), 1);

        let mut painter = Painter::new(engine());
        let decor = ThemedDecor::new(highlight::Theme::new(
            highlight::Variant::Dark,
            highlight::ColorMode::Truecolor,
        ));
        // Capture what the *syntax* highlighter alone would paint this token
        // as, before the overlay gets a say.
        let syntax = decor.highlight("let needle = 1;", Some("rust"));
        let token = syntax
            .runs
            .iter()
            .find(|r| r.text.contains("needle"))
            .expect("the identifier is its own run");
        let syntax_sgr = sgr_for(&decor, Semantic::CodeBlock);
        let current = sgr_for(&decor, Semantic::SearchCurrent);
        painter.register_decor(Box::new(decor));

        let wire = painted(&mut painter, &mut state);
        assert!(
            wire.contains(&format!("{current}needle")),
            "the match must paint in the search-current style, not its capture \
             style ({:?}): {wire:?}",
            token.style_id
        );
        assert!(
            !wire.contains(&format!("{syntax_sgr}needle")),
            "the code-block style must not still be painting the match: {wire:?}"
        );
        // The rest of the line still gets its syntax highlighting — the
        // overlay overrode the match, not the whole run. Counted as distinct
        // truecolor sequences rather than by looking for one token's text:
        // the tokens around the match are painted as their own runs, each
        // reset-terminated, so "let " never appears contiguously on the wire.
        let colors: std::collections::HashSet<&str> = wire
            .match_indices("38;2;")
            .filter_map(|(at, _)| wire[at..].split_once('m').map(|(seq, _)| seq))
            .collect();
        assert!(
            colors.len() >= 3,
            "the tokens around the match must keep their own capture colors, \
             found {colors:?} in {wire:?}"
        );
    }

    /// A `Decor` that counts how many times the underlying highlighter was
    /// actually asked. `cacheable` is a constructor parameter so the same
    /// spy covers both the memoized and the never-memoized case.
    struct CountingDecor {
        calls: std::rc::Rc<Cell<usize>>,
        cacheable: bool,
    }

    impl Decor for CountingDecor {
        fn highlight(&self, line_text: &str, _lang: Option<&str>) -> Highlighted {
            self.calls.set(self.calls.get() + 1);
            Highlighted {
                runs: vec![Run {
                    text: line_text.to_string(),
                    style_id: StyleId::Semantic(Semantic::CodeBlock),
                    width: 0,
                    aux: None,
                }],
                cacheable: self.cacheable,
            }
        }

        fn resolve(&self, style_id: StyleId) -> Style {
            StructuralDecor.resolve(style_id)
        }
    }

    fn code_fence(lines: usize) -> String {
        let mut source = String::from("```rust\n");
        for i in 0..lines {
            source.push_str(&format!("let value_{i:03} = {i} + 1;\n"));
        }
        source.push_str("```\n");
        source
    }

    /// A *representative* code fence for the DW-4.7 benchmark: real Rust at
    /// real line lengths, rather than the one-expression-per-line filler
    /// `code_fence` produces.
    ///
    /// The distinction is the whole measurement. Tree-sitter's cost per line
    /// scales with how much syntax is on it, while the painter's cost is
    /// capped by the viewport width (a long line is clipped, a long parse is
    /// not) — so `let value_007 = 7 + 1;` understates the very cost this
    /// phase removes. Measured on the two fixtures side by side: 3.6x on the
    /// filler, 15-20x on this. The audit that motivated the phase measured
    /// 2,525 µs per frame on real code-heavy content, which is the shape this
    /// fixture reproduces and the filler does not.
    fn realistic_code_fence(rows: usize) -> String {
        const SNIPPET: &str = "\
pub fn resolve_placement(&mut self, node: NodeId, rect: CellRect) -> Option<Placement> {
    let existing = self.placements.get(&node).copied().filter(|p| p.rect == rect);
    if let Some(placement) = existing {
        self.touch_lru(node);
        return Some(placement);
    }
    let raster = self.decoder.decode(&self.source_for(node)?, rect.width, rect.height)?;
    let placement = Placement { id: self.next_id(), rect, generation: self.generation };
    self.evict_lru_if_needed(rect.width as usize * rect.height as usize);
    self.placements.insert(node, placement);
    self.transmit(&raster, placement, &mut self.out).ok()?;
    Some(placement)
}
impl<'a, W: Write + 'a> Iterator for FrameSweep<'a, W> {
    type Item = Result<SweptPlacement, SweepError>;
    fn next(&mut self) -> Option<Self::Item> {
        while let Some((node, placement)) = self.pending.pop_front() {
            if self.visible.contains(&node) || placement.generation == self.generation {
                continue;
            }
            return Some(self.evict(node, placement).map_err(SweepError::Io));
        }
        None
    }
}
";
        let mut source = String::from("```rust\n");
        for line in SNIPPET.lines().cycle().take(rows) {
            source.push_str(line);
            source.push('\n');
        }
        source.push_str("```\n");
        source
    }

    #[test]
    fn test_dw_4_6_two_frames_over_the_same_code_block_invoke_the_highlighter_once() {
        const LINES: usize = 12;
        let doc = Document::parse(&code_fence(LINES));
        let tree = layout(&doc, 60, &LayoutConfig::default(), &engine(), &NullSizer);
        let size = Size {
            width: 60,
            height: 20,
        };

        let calls = std::rc::Rc::new(Cell::new(0));
        let mut painter = Painter::new(engine());
        painter.register_decor(Box::new(CountingDecor {
            calls: calls.clone(),
            cacheable: true,
        }));

        let mut buf = Vec::new();
        painter.frame(&tree, 0, size, &mut buf).unwrap();
        let after_first = calls.get();
        assert_eq!(
            after_first, LINES,
            "the first frame must highlight every visible code line"
        );

        buf.clear();
        painter.frame(&tree, 0, size, &mut buf).unwrap();
        assert_eq!(
            calls.get(),
            after_first,
            "the second frame over identical content must not re-invoke the \
             highlighter — it invoked it {} more times",
            calls.get() - after_first
        );
    }

    /// The constraint's other half, end to end: a decor that reports its
    /// answer as a fallback must be re-asked on every frame. Without this,
    /// one 250 ms timeout would freeze a plain-text line into the rest of
    /// the session.
    #[test]
    fn test_a_non_cacheable_highlight_is_re_invoked_on_every_frame() {
        const LINES: usize = 6;
        let doc = Document::parse(&code_fence(LINES));
        let tree = layout(&doc, 60, &LayoutConfig::default(), &engine(), &NullSizer);
        let size = Size {
            width: 60,
            height: 20,
        };

        let calls = std::rc::Rc::new(Cell::new(0));
        let mut painter = Painter::new(engine());
        painter.register_decor(Box::new(CountingDecor {
            calls: calls.clone(),
            cacheable: false,
        }));

        let mut buf = Vec::new();
        for _ in 0..3 {
            buf.clear();
            painter.frame(&tree, 0, size, &mut buf).unwrap();
        }
        assert_eq!(
            calls.get(),
            LINES * 3,
            "a timeout fallback must never be retained"
        );
    }

    /// The cache must be free where it cannot help: a document with no code
    /// block never reaches `Decor::highlight` at all, so prose pays neither
    /// a lookup nor an eviction for a feature it does not use.
    #[test]
    fn test_a_document_with_no_code_block_never_invokes_the_highlighter() {
        let source: String = (0..30)
            .map(|i| format!("Paragraph {i} of `inline code` and ordinary prose.\n\n"))
            .collect();
        let doc = Document::parse(&source);
        let tree = layout(&doc, 60, &LayoutConfig::default(), &engine(), &NullSizer);
        let calls = std::rc::Rc::new(Cell::new(0));
        let mut painter = Painter::new(engine());
        painter.register_decor(Box::new(CountingDecor {
            calls: calls.clone(),
            cacheable: true,
        }));
        let mut buf = Vec::new();
        painter
            .frame(
                &tree,
                0,
                Size {
                    width: 60,
                    height: 20,
                },
                &mut buf,
            )
            .unwrap();
        assert_eq!(
            calls.get(),
            0,
            "inline code is not a code block; nothing here needs highlighting"
        );
    }

    /// Registering a new decor must not serve the old one's answers.
    #[test]
    fn test_registering_a_decor_drops_the_cache_built_by_the_previous_one() {
        let doc = Document::parse(&code_fence(4));
        let tree = layout(&doc, 60, &LayoutConfig::default(), &engine(), &NullSizer);
        let size = Size {
            width: 60,
            height: 20,
        };
        let mut painter = Painter::new(engine());
        let mut buf = Vec::new();
        painter.frame(&tree, 0, size, &mut buf).unwrap();

        let calls = std::rc::Rc::new(Cell::new(0));
        painter.register_decor(Box::new(CountingDecor {
            calls: calls.clone(),
            cacheable: true,
        }));
        buf.clear();
        painter.frame(&tree, 0, size, &mut buf).unwrap();
        assert_eq!(
            calls.get(),
            4,
            "the new decor must be asked about every line, cache or no cache"
        );
    }

    /// **DW-4.7, the committed speed claim.** A code-heavy frame must be at
    /// least 10x cheaper than it was before this phase, measured against the
    /// pre-change path itself — `with_cache_capacity(0)` never retains
    /// anything, so its paint path *is* the old one — in this same harness,
    /// on this same host, over this same fixture.
    ///
    /// **The 10x floor is a release-profile claim, and this test says so.**
    /// This ratio is not a constant of the code — it moves with the build
    /// profile, because the two costs it divides are not equally sensitive
    /// to optimization. Measured on this fixture and host, three runs each:
    ///
    /// | | uncached | cached | ratio |
    /// |---|---|---|---|
    /// | optimized | 1.15–1.24 ms/frame | 94–97 µs/frame | 12.2–12.9x |
    /// | unoptimized | 6.41–6.81 ms/frame | 1.47–1.49 ms/frame | 4.3–4.6x |
    ///
    /// Both arms get slower without `-O`; they just do not get slower at the
    /// same rate. The baseline slows about 5.5x while the cached arm slows
    /// about 15x, and dividing one by the other is what compresses 12x into
    /// 4x. (An earlier version of this comment claimed the removed cost was
    /// profile-*independent* — "optimized C whatever profile the crate is
    /// built in". Its own numbers refute that: a profile-independent parse
    /// could not have slowed 5.5x. The conclusion below was right for the
    /// wrong reason, which is worth saying plainly so nobody builds on it.)
    ///
    /// The optimized number is the one that describes the binary readers
    /// run, and the 64 µs frame budget the original audit set is unreachable
    /// in a debug build for reasons that have nothing to do with this cache.
    /// So the full floor is asserted where it means something, and an
    /// unoptimized run still asserts a real (weaker) floor rather than
    /// skipping: a regression that loses the cache outright scores ~1x and
    /// fails either way, which is what keeps `cargo test` honest. What that
    /// does *not* cover is a partial regression — something recovering only
    /// 6x optimized would pass the default suite silently, so the release
    /// arm needs to be run to gate the headline claim.
    ///
    /// Fixture: real Rust at real line lengths in an 80-column viewport, the
    /// most ordinary terminal there is. See [`realistic_code_fence`] for why
    /// the shape of the code matters to what this measures.
    #[test]
    fn test_dw_4_7_cached_code_heavy_frames_are_at_least_10x_faster_than_the_uncached_baseline() {
        use std::time::Instant;

        /// The claim, in an optimized build: the binary readers actually run.
        const OPTIMIZED_FLOOR: u32 = 10;
        /// The floor an unoptimized build still has to clear, so the default
        /// `cargo test` run gates on frame *time* and not just on the call
        /// count DW-4.6 already pins. Measured at 4.4x; 3 leaves margin for
        /// a loaded CI host without letting a lost cache through.
        const UNOPTIMIZED_FLOOR: u32 = 3;

        const VISIBLE_LINES: u16 = 40;
        const WIDTH: u16 = 80;
        const FRAMES: usize = 20;
        let doc = Document::parse(&realistic_code_fence(usize::from(VISIBLE_LINES)));
        let tree = layout(&doc, WIDTH, &LayoutConfig::default(), &engine(), &NullSizer);
        let size = Size {
            width: WIDTH,
            height: VISIBLE_LINES,
        };

        let themed = || {
            Box::new(ThemedDecor::new(highlight::Theme::new(
                highlight::Variant::Dark,
                highlight::ColorMode::Truecolor,
            ))) as Box<dyn Decor>
        };

        let mut baseline = Painter::with_cache_capacity(engine(), 0);
        baseline.register_decor(themed());
        let mut cached = Painter::new(engine());
        cached.register_decor(themed());

        // Warm both: page faults, lumis' own lazy grammar init, and the
        // cached painter's first (necessarily cold) frame.
        let mut buf = Vec::new();
        for painter in [&mut baseline, &mut cached] {
            buf.clear();
            painter.frame(&tree, 0, size, &mut buf).unwrap();
        }

        let start = Instant::now();
        for _ in 0..FRAMES {
            buf.clear();
            baseline.frame(&tree, 0, size, &mut buf).unwrap();
        }
        let uncached = start.elapsed();

        let start = Instant::now();
        for _ in 0..FRAMES {
            buf.clear();
            cached.frame(&tree, 0, size, &mut buf).unwrap();
        }
        let elapsed = start.elapsed();

        // Both arms must paint the same thing. A "faster" frame that quietly
        // dropped the syntax highlighting would win this benchmark and be
        // worthless — the cache may change what a frame *costs*, never what
        // it *says*.
        let mut fast_bytes = Vec::new();
        cached.frame(&tree, 0, size, &mut fast_bytes).unwrap();
        let mut slow_bytes = Vec::new();
        baseline.frame(&tree, 0, size, &mut slow_bytes).unwrap();
        assert_eq!(
            fast_bytes, slow_bytes,
            "the cache must change the cost of a frame, never its bytes"
        );

        let floor = if cfg!(debug_assertions) {
            UNOPTIMIZED_FLOOR
        } else {
            OPTIMIZED_FLOOR
        };
        let ratio = uncached.as_secs_f64() / elapsed.as_secs_f64();
        println!(
            "DW-4.7: {FRAMES} frames x {VISIBLE_LINES} code lines at {WIDTH} cols — \
             uncached {:?}/frame, cached {:?}/frame, {ratio:.1}x (floor {floor}x, \
             {} build)",
            uncached / FRAMES as u32,
            elapsed / FRAMES as u32,
            if cfg!(debug_assertions) {
                "unoptimized"
            } else {
                "optimized"
            }
        );
        assert!(
            elapsed.as_nanos() * u128::from(floor) <= uncached.as_nanos(),
            "cached code-heavy frames must be at least {floor}x faster than the \
             pre-change baseline: cached={elapsed:?} uncached={uncached:?} \
             ({ratio:.1}x) over {FRAMES} frames of {VISIBLE_LINES} code lines"
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

    // ------------------------------------------------------------- Phase 6

    fn linked_tree(source: &str, width: u16) -> LayoutTree {
        let doc = Document::parse(source);
        layout(&doc, width, &LayoutConfig::default(), &engine(), &NullSizer)
    }

    /// The cells of a painted frame's row `row` (1-indexed) that carry the
    /// reverse-video attribute, as a `(start, text)` pair per reversed span.
    ///
    /// Replays the SGR stream the way a terminal would rather than searching
    /// for a substring: `\x1b[7m` set somewhere and reset somewhere else is
    /// not the same claim as "these glyphs are reversed".
    fn reversed_spans(frame: &str, row: u16) -> Vec<(usize, String)> {
        let chars: Vec<char> = frame.chars().collect();
        let mut spans: Vec<(usize, String)> = Vec::new();
        let mut cur_row: u16 = 1;
        let mut col = 0usize;
        let mut reverse = false;
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\u{1b}' && chars.get(i + 1) == Some(&'[') {
                let mut j = i + 2;
                while j < chars.len() && !chars[j].is_ascii_alphabetic() {
                    j += 1;
                }
                let params: String = chars[i + 2..j].iter().collect();
                match chars.get(j) {
                    Some('H') => {
                        let mut it = params.split(';');
                        cur_row = it.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                        col = it
                            .next()
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(1)
                            .saturating_sub(1);
                    }
                    Some('m') => {
                        // `write_sgr` opens with a reset, so any `m` sequence
                        // that is not exactly `7` clears the attribute.
                        reverse = params == "7";
                    }
                    _ => {}
                }
                i = j + 1;
                continue;
            }
            if cur_row == row && reverse {
                match spans.last_mut() {
                    Some((start, text)) if *start + text.chars().count() == col => {
                        text.push(chars[i])
                    }
                    _ => spans.push((col, chars[i].to_string())),
                }
            }
            col += 1;
            i += 1;
        }
        spans
    }

    #[test]
    fn test_dw_6_1_the_selected_link_is_painted_in_reverse_video_and_its_neighbour_is_not() {
        let tree = linked_tree("See [alpha](a.md) then [beta](b.md) end.\n", 60);
        let size = Size {
            width: 60,
            height: 4,
        };
        let mut painter = Painter::new(engine());

        // With no selection nothing on the row is reversed.
        let mut plain = Vec::new();
        painter
            .frame_with_status(&tree, 0, size, &StatusLine::default(), &mut plain)
            .expect("paint");
        assert!(
            reversed_spans(&String::from_utf8_lossy(&plain), 1).is_empty(),
            "an unselected document must paint no reverse video"
        );

        // Select the *second* link's run and only its glyphs come back reversed.
        let Some(Line::Items(items)) = tree.lines(0..1).next() else {
            panic!("the fixture's first line must be text");
        };
        let beta = items
            .iter()
            .position(|item| match item {
                LineItem::Run(run) => run.text == "beta",
                LineItem::Box(_) => false,
            })
            .expect("a run painting `beta`");
        let mut selected = Vec::new();
        painter
            .frame_with_selection(
                &tree,
                0,
                size,
                &StatusLine::default(),
                &[LinkSpan {
                    line: 0,
                    first_item: beta,
                    last_item: beta,
                }],
                &mut selected,
            )
            .expect("paint");
        let spans = reversed_spans(&String::from_utf8_lossy(&selected), 1);
        assert_eq!(
            spans.len(),
            1,
            "exactly one span must be reversed, got {spans:?}"
        );
        assert_eq!(spans[0].1, "beta", "and it must be the selected link");
    }

    /// **Both overlays on one frame.** Search highlighting restyles bytes by
    /// role; the link indicator sets the reverse attribute over whatever role
    /// won. They are not alternatives, and the one frame call has to carry
    /// both — a per-mode wrapper would silently drop one.
    #[test]
    fn test_a_search_highlight_and_a_link_indicator_paint_on_the_same_frame() {
        let size = Size {
            width: 60,
            height: 4,
        };
        // The query matches text *outside* the link, so the two overlays are
        // independently observable on the same row.
        let mut state = searched("needle here [alpha](a.md) tail\n", size, "needle");
        assert_eq!(state.search().matches.len(), 1);

        // Select the link the way the key table does, so the spans come from
        // the same code the event loop uses.
        state.handle_key_event(KeyEvent::from(KeyCode::Tab));
        let selected = state.selection_spans();
        assert!(!selected.is_empty(), "the link must be selected");

        let mut painter = Painter::new(engine());
        let status = state.status();
        let mut buf = Vec::new();
        painter
            .frame_with_overlays(
                state.tree(),
                state.scroll(),
                size,
                &status,
                FrameOverlays {
                    search: state.search_overlay(),
                    selected: &selected,
                },
                &mut buf,
            )
            .expect("painting to a Vec cannot fail");
        let wire = String::from_utf8(buf).expect("utf-8");

        let current = sgr_for(painter.decor.as_ref(), Semantic::SearchCurrent);
        assert_eq!(
            highlighted_bytes(&wire, &current),
            "needle".len(),
            "the match must still be highlighted with a link selected: {wire:?}"
        );
        let reversed = reversed_spans(&wire, 1);
        assert!(
            reversed.iter().any(|(_, text)| text.contains("alpha")),
            "…and the link must still be indicated: {reversed:?}"
        );
    }

    /// The composition where they overlap: a match *inside* the selected link
    /// must read as both. The search role wins the colour, the reverse
    /// attribute rides on top — neither cancels the other.
    #[test]
    fn test_a_match_inside_the_selected_link_reads_as_a_match_and_as_selected() {
        let size = Size {
            width: 60,
            height: 4,
        };
        let mut state = searched("prose [needle](a.md) tail\n", size, "needle");
        assert_eq!(state.search().matches.len(), 1);
        state.handle_key_event(KeyEvent::from(KeyCode::Tab));
        let selected = state.selection_spans();
        assert!(!selected.is_empty());

        let mut painter = Painter::new(engine());
        let status = state.status();
        let mut buf = Vec::new();
        painter
            .frame_with_overlays(
                state.tree(),
                state.scroll(),
                size,
                &status,
                FrameOverlays {
                    search: state.search_overlay(),
                    selected: &selected,
                },
                &mut buf,
            )
            .expect("painting to a Vec cannot fail");
        let wire = String::from_utf8(buf).expect("utf-8");

        let current = sgr_for(painter.decor.as_ref(), Semantic::SearchCurrent);
        assert!(
            wire.contains(&format!("{current}\x1b[7mneedle")),
            "the match role must be written first and the reverse attribute \
             on top of it, so the text is both: {wire:?}"
        );
    }

    #[test]
    fn test_dw_6_1_a_selection_on_another_line_does_not_reverse_this_one() {
        let tree = linked_tree("[one](a.md)\n\n[two](b.md)\n", 40);
        let size = Size {
            width: 40,
            height: 6,
        };
        let mut painter = Painter::new(engine());
        let mut buf = Vec::new();
        // Line 2 is the blank between the two paragraphs; naming it must
        // reverse nothing at all.
        painter
            .frame_with_selection(
                &tree,
                0,
                size,
                &StatusLine::default(),
                &[LinkSpan {
                    line: 1,
                    first_item: 0,
                    last_item: 9,
                }],
                &mut buf,
            )
            .expect("paint");
        let wire = String::from_utf8_lossy(&buf).into_owned();
        assert!(reversed_spans(&wire, 1).is_empty());
        assert!(reversed_spans(&wire, 3).is_empty());
    }

    /// The indicator is addressed by absolute line index, not by viewport
    /// row: scrolling must move the reverse video with the text it marks.
    #[test]
    fn test_dw_6_1_the_indicator_follows_the_line_it_names_across_a_scroll() {
        let source = "filler\n\nfiller\n\nSee [target](t.md) here.\n";
        let tree = linked_tree(source, 60);
        let line = (0..tree.line_count())
            .find(|&i| match tree.lines(i..i + 1).next() {
                Some(Line::Items(items)) => items.iter().any(|item| match item {
                    LineItem::Run(run) => run.text == "target",
                    LineItem::Box(_) => false,
                }),
                Some(Line::Reserved(_)) | None => false,
            })
            .expect("the fixture must paint `target` somewhere");
        let Some(Line::Items(items)) = tree.lines(line..line + 1).next() else {
            panic!("text line");
        };
        let item = items
            .iter()
            .position(|it| matches!(it, LineItem::Run(run) if run.text == "target"))
            .expect("a run");
        let span = LinkSpan {
            line,
            first_item: item,
            last_item: item,
        };
        let size = Size {
            width: 60,
            height: 3,
        };
        let mut painter = Painter::new(engine());

        // Every scroll that still shows the marked line, so the row it lands
        // on differs between iterations — which is the whole claim.
        for scroll in line.saturating_sub(2)..=line {
            let mut buf = Vec::new();
            painter
                .frame_with_selection(
                    &tree,
                    scroll,
                    size,
                    &StatusLine::default(),
                    &[span],
                    &mut buf,
                )
                .expect("paint");
            let wire = String::from_utf8_lossy(&buf).into_owned();
            let row = u16::try_from(line - scroll + 1).expect("small");
            let spans = reversed_spans(&wire, row);
            assert_eq!(
                spans.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>(),
                vec!["target"],
                "at scroll {scroll} the indicator must be on row {row}: {spans:?}"
            );
        }
    }

    #[test]
    fn test_dw_6_6_item_columns_agree_with_the_columns_the_painter_writes_to() {
        // The oracle is the painted frame itself: every item's reported start
        // column must be where its glyphs actually land.
        let tree = linked_tree("ab [cd](x.md) efg\n", 40);
        let Some(Line::Items(items)) = tree.lines(0..1).next() else {
            panic!("text line");
        };
        let columns = item_columns(items, &engine(), 40);

        let mut painter = Painter::new(engine());
        let mut buf = Vec::new();
        painter
            .frame_with_status(
                &tree,
                0,
                Size {
                    width: 40,
                    height: 2,
                },
                &StatusLine::default(),
                &mut buf,
            )
            .expect("paint");
        let row = crate::painter::tests::render_row_local(&String::from_utf8_lossy(&buf), 1, 40);

        for (item, &(start, end)) in items.iter().zip(&columns) {
            let LineItem::Run(run) = item else { continue };
            if run.text.trim().is_empty() {
                continue;
            }
            let painted: String = row
                .chars()
                .skip(usize::from(start))
                .take(usize::from(end - start))
                .collect();
            assert_eq!(
                painted, run.text,
                "item_columns put {:?} at {start}..{end}, but the frame has {painted:?} there",
                run.text
            );
        }
    }

    #[test]
    fn test_item_columns_measures_double_width_clusters_rather_than_counting_them() {
        // A CJK run is two cells per cluster; a hit-test that counted
        // characters would put every later item two columns to the left of
        // where it paints.
        let tree = linked_tree("漢字 [x](a.md)\n", 40);
        let Some(Line::Items(items)) = tree.lines(0..1).next() else {
            panic!("text line");
        };
        let columns = item_columns(items, &engine(), 40);
        let cjk = items
            .iter()
            .position(|item| matches!(item, LineItem::Run(run) if run.text.contains('漢')))
            .expect("a CJK run");
        let (start, end) = columns[cjk];
        assert_eq!(
            end - start,
            4,
            "two double-width clusters occupy four cells, not two"
        );
    }

    #[test]
    fn test_item_columns_clips_at_the_viewport_edge_and_stays_index_aligned() {
        // Links are what split a line into several items — layout coalesces
        // same-style text into one run, so a plain sentence is a single item
        // and could not exercise the alignment claim at all.
        let tree = linked_tree("aaaa [bbbb](b.md) cccc [dddd](d.md)\n", 60);
        let Some(Line::Items(items)) = tree.lines(0..1).next() else {
            panic!("text line");
        };
        assert!(items.len() > 3, "the fixture must produce several items");

        let columns = item_columns(items, &engine(), 6);
        assert_eq!(
            columns.len(),
            items.len(),
            "every item must get an entry so indices stay aligned with `items`"
        );
        assert!(
            columns.iter().all(|&(start, end)| start <= 6 && end <= 6),
            "nothing may be reported past the viewport: {columns:?}"
        );
        assert!(
            columns
                .last()
                .is_some_and(|&(start, end)| start == end && start == 6),
            "once the budget is spent every later item must be zero-width at \
             the clip column, so a click past the clip matches nothing: {columns:?}"
        );
    }

    /// A copy of the integration harness's cell-grid replay, small enough to
    /// live here: `tests/common/render.rs` is not reachable from a unit test.
    pub(super) fn render_row_local(frame: &str, row: u16, width: usize) -> String {
        let mut grid = vec![' '; width];
        let chars: Vec<char> = frame.chars().collect();
        let mut cur_row: u16 = 1;
        let mut col = 0usize;
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\u{1b}' {
                let mut j = i + 2;
                while j < chars.len() && !chars[j].is_ascii_alphabetic() {
                    j += 1;
                }
                let params: String = chars[i + 2..j].iter().collect();
                match chars.get(j) {
                    Some('H') => {
                        let mut it = params.split(';');
                        cur_row = it.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                        col = it
                            .next()
                            .and_then(|v| v.parse::<usize>().ok())
                            .unwrap_or(1)
                            .saturating_sub(1);
                    }
                    Some('K') if cur_row == row => {
                        for c in grid.iter_mut().skip(col) {
                            *c = ' ';
                        }
                    }
                    _ => {}
                }
                i = j + 1;
                continue;
            }
            if cur_row == row && col < width {
                grid[col] = chars[i];
            }
            col += 1;
            i += 1;
        }
        grid.into_iter().collect()
    }
}
