//! `GfxMediaSink` — the real [`MediaSink`] implementation (P6): owns the
//! [`gfx::Emitter`], the eviction policy, and the RaTeX→txm→literal
//! degradation ladder.
//!
//! **Placement mode deviation (recorded, not silently assumed):** paints
//! via direct placement + repaint-behind, not virtual `U=1` placement — see
//! `crates/gfx`'s crate-level doc comment for why (the spike's own verdict
//! on `U=1` was visually unverified, and no GUI session was available here
//! to run the manual pass it called for).
//!
//! **Frame boundaries come from [`MediaSink::begin_frame`]**, which
//! [`crate::painter::Painter::frame`] calls once per frame before any
//! `paint`, media present or not. They are never inferred from `paint` call
//! order: an earlier revision inferred "new frame" from `rect.y <= previous
//! y`, which broke on a scroll-*up* (every box's `y` increases, so
//! consecutive frames merged and placements were never re-placed, drifting
//! out of alignment with the text) and never swept at all on a frame with no
//! visible media (an image scrolled fully off-screen was never deleted and
//! stayed on the terminal for the rest of the session). The boundary drives
//! two independent mechanisms:
//! - **Placement cap (32 live placements):** a per-frame quota, and nothing
//!   more. Because the boundary unplaces everything (below), the live
//!   placements at any moment are exactly the boxes this frame has painted so
//!   far, so the cap is a straight count of them — there is no eviction
//!   decision left to get wrong. It used to be enforced by evicting an LRU
//!   entry, and since LRU order within a frame is paint order, that
//!   deterministically deleted the boxes at the top-left of the viewport as
//!   soon as a 33rd came into view: transmitted, placed, then deleted inside
//!   one frame, leaving empty cells with nothing painted in their place. A
//!   40-badge README row reaches that. Over the quota a box takes its **text
//!   rung** instead — a rung that paints alt text beats one that paints
//!   nothing.
//! - **Raster residency (a byte budget, in [`residency::RasterCache`]):** how
//!   long the terminal keeps an image's *pixel data*, governed by how many
//!   bytes of raster this process is asking it to hold, with least-recently-
//!   used eviction. This was a frame-count grace period approximating a
//!   one-viewport scroll margin, and its arithmetic was off by one in the
//!   direction that closed the window before any frame could use it — a node
//!   absent from frame *N-1* lost its raster at the top of frame *N*, so a
//!   scroll-back re-transmitted every time. See that module's own doc for why
//!   bytes are the honest unit and frames were not.
//!
//! **Two lifetimes, not one.** Those two mechanisms govern how long the
//! terminal keeps an image's *pixel data*. Whether an image is *drawn* is a
//! separate question with a separate, exact answer: **a box that is not
//! painted in a frame is not on the screen after that frame.** The two used
//! to share one lifetime, and the grace period was applied to both — so an
//! image that scrolled off screen stayed *visible* for two more frames. In a
//! viewer that paints only in response to input (`main.rs` blocks on
//! `event::read()`), one keypress is exactly one frame, so those frames never
//! arrived: a single Ctrl-d left the previous screen's formulas painted on
//! top of the new text until the user happened to press another key.
//!
//! So [`MediaSink::begin_frame`] now *unplaces* every live placement — kitty's
//! `a=d,d=i`, which drops placements and keeps the stored raster — and each
//! box that is painted this frame re-places itself. Nothing can be visible
//! without having been placed in the frame you are looking at. Because the
//! whole frame is inside `Painter`'s mode-2026 synchronized-update block, the
//! unplace/re-place pair for a box that stays on screen is never presented as
//! an intermediate state; and because the *record* survives the unplace, the
//! node keeps its image id and its resident raster, so coming back into view
//! costs a re-place (~20 bytes), not a re-transmit (a base64'd PNG).
//!
//! The two deletes are therefore not interchangeable: `a=d,d=i` (lowercase,
//! [`gfx::Emitter::clear_placements`]) ends *visibility*, `a=d,d=I`
//! (uppercase, [`gfx::Emitter::delete`]) ends *residency* and is what the
//! byte budget emits.
//!
//! The split is now structural, not just documented: visibility lives in this
//! file's `placed` list, residency lives in [`residency::RasterCache`], and
//! neither can see the other's bound. While they shared one map the
//! 32-placement cap was checked against its `len()`, which capped raster
//! retention at 32 as a side effect nobody chose.
//!
//! **Scroll truncation.** A multi-row box is painted one row at a time, and
//! a scrolled viewport shows only a contiguous slice of those rows. Two
//! inputs make that slice paintable, and both arrive through `paint`:
//! [`Reserved::row`] says which row of the box this call is (nonzero ⇒ its
//! top `row` rows are above the viewport), and `rect.height` carries the
//! box's *visible remainder* from this row down (see
//! [`crate::painter::Painter`]'s `paint_reserved`). The placement is then
//! sized `r = visible` and its **source rectangle** cropped to the same
//! fraction of the raster — kitty's `x`/`y`/`w`/`h` keys on `a=p`.
//!
//! This needs no terminal cell-pixel geometry: the raster is rendered for
//! the *whole* box (`rows` cells tall) and the terminal maps it linearly
//! onto the requested cell box, so skipping `row/rows` of the raster's own
//! pixel height skips exactly `row` cell rows. Rendering for the whole box
//! rather than the visible slice also keeps the raster cache scroll-
//! independent: scrolling re-places and re-crops, it never re-rasterizes.
//!
//! Before this, the sink anchored the full box at the first call it saw and
//! emitted `r = rows`, which was right only while the box's top was on
//! screen; a top-truncated box painted its image downward over the document
//! text below it.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;

use ast::{Document, Inline, InlineKind, NodeId, NodeRef};
use layout::Reserved;
use width::WidthEngine;

use crate::media::MediaSink;
use crate::media::residency::{RasterCache, Resident};
use crate::media::sizer::math_baseline_px;
use crate::painter::{CellRect, sanitize};

/// Kitty protocol / DW-6.1 cap: at most this many placements may be live at
/// once, i.e. at most this many media boxes are drawn in any one frame. The
/// 33rd visible box takes its text rung.
///
/// It bounds *placements* and nothing else. It used to be applied to the map
/// that also held rasters, which capped raster retention at 32 as a silent
/// side effect; residency is now [`RasterCache`]'s byte budget and this
/// constant appears nowhere near it.
const CAP: usize = 32;

/// How many bytes of transmitted raster this process asks the terminal to
/// hold at once, before the least-recently-used one is freed.
///
/// A raster is letterboxed to exactly its cell box in pixels, so a full-width,
/// half-screen image at the fallback 24x48 cell geometry is a 2400x2400 PNG —
/// 1-3 MB depending on content — and an inline formula is a few kilobytes.
/// 64 MiB therefore holds tens of full-screen images or thousands of formulas:
/// an order of magnitude past the 32-placement cap that used to bound the same
/// set, which is the coupling DW-3.6 exists to break. Kitty's own default
/// image-storage quota is 320 MB, so this stays well inside what a terminal
/// expects a single client to spend.
const RASTER_BUDGET_BYTES: u64 = 64 * 1024 * 1024;

/// What a node resolved to this frame, remembered so later rows of the
/// same (possibly multi-row) box know what to do without re-resolving.
enum Resolved {
    /// Already transmitted and placed on the box's first *visible* row;
    /// nothing left to do for subsequent rows of the same box.
    Graphics,
    /// A text-fallback rung (alt text, txm grid, or literal TeX source):
    /// pre-sanitized lines, one shown per row of the box.
    Text(Vec<String>),
}

pub struct GfxMediaSink {
    /// Shared with the app rather than copied (DW-2.6). The sink only ever
    /// *reads* the AST — it resolves a `NodeId` to its image path or TeX
    /// source at paint time — so a second copy of every node bought nothing
    /// but the allocation. `Rc`, not `Arc`: the sink is painted from the
    /// event loop's thread and never crosses one.
    doc: Rc<Document>,
    base_dir: PathBuf,
    emitter: gfx::Emitter,
    limits: gfx::Limits,
    /// Queried (or, failing that, assumed) terminal cell-pixel geometry, used
    /// to size the raster target. Set at construction from
    /// [`crate::terminal::query_cell_px`] via [`GfxMediaSink::with_cell_px`],
    /// and settable afterwards by [`GfxMediaSink::set_cell_px`].
    ///
    /// DW-6.5 is exactly "changing this regenerates the raster without
    /// touching the document's layout tree." That holds, but *not* by
    /// construction — the type does hold a [`Document`] and a [`WidthEngine`],
    /// so nothing in the type system stops a future edit from calling
    /// `layout::layout`. What holds it is observable on the wire, and asserted
    /// there: after a cell-size change the placement still claims the same
    /// `c=`/`r=` cell box (no re-layout) while the transmitted raster's pixel
    /// dimensions change (a re-rasterize). See
    /// `test_dw_6_5_geometry_change_regenerates_the_raster_and_keeps_the_cell_box`.
    cell_px: (u32, u32),
    /// This process's slice of the terminal's global image-id namespace — see
    /// [`gfx::IdAllocator`] for why that is not just `1..`.
    ids: gfx::IdAllocator,
    /// **Residency**: whose pixel data the terminal is holding, bounded by a
    /// byte budget. Knows nothing about what is on the screen.
    cache: RasterCache,
    /// **Visibility**: the nodes with a live placement right now, in paint
    /// order. Emptied by the frame boundary's unplace sweep and refilled by
    /// this frame's paints, so `placed.len()` *is* the live-placement count
    /// the [`CAP`] quota is spent against, and membership *is* the answer to
    /// "can the reader see this box".
    ///
    /// A `Vec` rather than a set: it is capped at [`CAP`], and the paint
    /// order it preserves is what makes the unplace sweep's byte stream
    /// deterministic.
    placed: Vec<NodeId>,
    width_engine: WidthEngine,
    row_in_frame: HashMap<NodeId, u16>,
    resolved_this_frame: HashMap<NodeId, Resolved>,
    /// Test-only failure injection for DW-6.2's ladder (each rung must be
    /// provably exercised by a *forced* failure, not just an incidental
    /// one).
    force_ratex_fail: bool,
    force_txm_fail: bool,
    /// Test-only: reinstate the pre-Phase-3 residency window, where a node
    /// absent from the previous frame lost its raster at the top of this one.
    /// Exists so DW-3.5's benchmark measures the real old behaviour rather
    /// than a stand-in for it. See [`GfxMediaSink::simulate_pre_change_residency_for_test`].
    pre_change_residency: bool,
}

impl GfxMediaSink {
    /// `doc` is the parsed document media nodes are resolved by `NodeId`
    /// against; `base_dir` resolves relative image paths (typically the
    /// markdown file's parent directory).
    ///
    /// Takes anything that becomes an `Rc<Document>`, which is both an
    /// `Rc<Document>` the caller already holds — the sharing path the viewer
    /// uses (DW-2.6) — and a bare `Document`, moved into a fresh `Rc`, which
    /// is what a test that owns its own document wants. Neither copies the
    /// AST.
    pub fn new(doc: impl Into<Rc<Document>>, base_dir: impl Into<PathBuf>) -> Self {
        GfxMediaSink {
            doc: doc.into(),
            base_dir: base_dir.into(),
            emitter: gfx::Emitter::new(),
            limits: gfx::Limits::default(),
            cell_px: crate::terminal::FALLBACK_CELL_PX,
            ids: gfx::IdAllocator::for_this_process(),
            cache: RasterCache::new(RASTER_BUDGET_BYTES),
            placed: Vec::new(),
            width_engine: WidthEngine::new(width::WidthConfig::default()),
            row_in_frame: HashMap::new(),
            resolved_this_frame: HashMap::new(),
            force_ratex_fail: false,
            force_txm_fail: false,
            pre_change_residency: false,
        }
    }

    pub fn with_limits(mut self, limits: gfx::Limits) -> Self {
        self.limits = limits;
        self
    }

    /// How many bytes of transmitted raster the terminal is asked to hold
    /// before the least-recently-used one is freed. Defaults to 64 MiB — see
    /// this module's `RASTER_BUDGET_BYTES` for where that number comes from.
    pub fn with_raster_budget(mut self, bytes: u64) -> Self {
        self.cache = RasterCache::new(bytes);
        self
    }

    /// Builder form of [`GfxMediaSink::set_cell_px`], for wiring
    /// [`crate::terminal::query_cell_px`]'s startup answer in at construction.
    pub fn with_cell_px(mut self, cell_px: (u32, u32)) -> Self {
        self.set_cell_px(cell_px);
        self
    }

    /// Updates the cell-pixel geometry a subsequent `paint` scales rasters to.
    ///
    /// `main.rs` calls this once, at startup, with
    /// [`crate::terminal::query_cell_px`]'s answer. It is *not* called again
    /// on resize, and that is a known gap rather than a design: a font-size
    /// change arrives as an ordinary `Event::Resize`, and re-querying at that
    /// point would need a way to reach the sink after
    /// `Painter::register_media` has taken ownership of it, which no accessor
    /// currently provides. Until then a mid-session font-size change leaves
    /// every raster rendered for the old cell size — visible as softness (the
    /// terminal rescales the raster onto the same cell box), not as
    /// misplacement.
    ///
    /// What this changes is raster *resolution* and the cache key that goes
    /// with it; never geometry. The reserved box's cell extent is fixed at
    /// layout time, and `crop_source` measures against the raster's own
    /// pixels. A zero on either axis is refused rather than stored — it would
    /// make every `target_px` zero and every raster a 1×1 pixel.
    pub fn set_cell_px(&mut self, cell_px: (u32, u32)) {
        if cell_px.0 == 0 || cell_px.1 == 0 {
            return;
        }
        self.cell_px = cell_px;
    }

    #[doc(hidden)]
    pub fn force_ratex_failure_for_test(&mut self, fail: bool) {
        self.force_ratex_fail = fail;
    }

    #[doc(hidden)]
    pub fn force_txm_failure_for_test(&mut self, fail: bool) {
        self.force_txm_fail = fail;
    }

    /// Test-only: put the residency window back the way it was before
    /// Phase 3, so a benchmark can measure against the real pre-change code
    /// rather than a re-description of it.
    ///
    /// The old sweep computed `cutoff = frame - (DATA_GRACE_FRAMES + 1)` and
    /// freed every raster whose node's `last_seen_frame <= cutoff`, which
    /// with the shipped grace of 1 is exactly "not painted in the previous
    /// frame" — and it ran at the *top* of the frame, before that frame's
    /// paints could claim anything. Reproduced here as that one sentence,
    /// which is all the arithmetic ever amounted to.
    #[doc(hidden)]
    pub fn simulate_pre_change_residency_for_test(&mut self, on: bool) {
        self.pre_change_residency = on;
    }

    /// How many bytes of raster the terminal is currently holding for this
    /// sink — the quantity [`GfxMediaSink::with_raster_budget`] bounds.
    #[doc(hidden)]
    pub fn resident_bytes(&self) -> u64 {
        self.cache.bytes_resident()
    }

    /// How many nodes have a resident raster. Distinct from the number of
    /// *placed* boxes, which is what [`CAP`] bounds.
    #[doc(hidden)]
    pub fn resident_count(&self) -> usize {
        self.cache.len()
    }

    /// The next image id for a node that has none: the allocator's next id in
    /// this process's slice of the namespace, skipping any that a resident
    /// raster still holds.
    ///
    /// The skip is what makes the allocator's wrap harmless. Ids are handed out
    /// fresh on every re-transmit (a node the byte budget evicted loses its
    /// record and comes back with a new id), so a long session does walk the
    /// counter round; without this check the id that came round could land on a
    /// node whose raster is still resident, and transmitting it would destroy
    /// that node's image and every placement of it. The resident set is bounded
    /// by the byte budget, so the loop terminates.
    fn alloc_id(&mut self) -> gfx::ImageId {
        loop {
            let id = self.ids.allocate();
            if !self.cache.holds_id(id) {
                return id;
            }
        }
    }

    /// Starts a frame: resets per-frame bookkeeping and takes every remaining
    /// placement off the screen, so that only boxes actually painted in this
    /// frame are visible when it ends.
    ///
    /// Driven by `MediaSink::begin_frame`, never inferred from `paint` call
    /// order. Row-order inference was wrong in two ways: on a scroll-up every
    /// box's `y` increases so consecutive frames merged (placements were
    /// never re-placed and drifted out of alignment with the text), and a
    /// frame containing no media never ran the sweep at all, so an image
    /// scrolled fully off-screen was never deleted and stayed on the
    /// terminal indefinitely.
    ///
    /// No raster sweep runs here any more: residency is a byte budget spent at
    /// transmit time, not a countdown checked at the frame boundary. That is
    /// the whole of the fix — the old sweep freed, at the top of frame *N*,
    /// exactly the rasters frame *N* was about to ask for.
    fn begin_frame_inner(&mut self, out: &mut dyn Write) {
        self.row_in_frame.clear();
        self.resolved_this_frame.clear();
        if self.pre_change_residency {
            self.evict_everything_absent_last_frame(out);
        }
        self.unplace_all(out);
    }

    /// The pre-Phase-3 residency sweep, retained only for
    /// [`GfxMediaSink::simulate_pre_change_residency_for_test`]'s benchmark
    /// baseline. Runs before [`Self::unplace_all`], so `placed` still holds
    /// the *previous* frame's boxes — which is precisely the set the old
    /// `last_seen_frame` comparison spared.
    fn evict_everything_absent_last_frame(&mut self, out: &mut dyn Write) {
        let last_frame: HashSet<NodeId> = self.placed.iter().copied().collect();
        let stale: Vec<NodeId> = self
            .cache
            .resident_nodes()
            .filter(|node_id| !last_frame.contains(node_id))
            .collect();
        for node_id in stale {
            self.delete_placement(node_id, out);
        }
    }

    /// Takes every live placement off the screen (`a=d,d=i` — placements
    /// only, raster kept), leaving each node's record, id and pixel data
    /// intact.
    ///
    /// This is what makes visibility exact. At the top of frame N+1 the set
    /// of live placements is exactly what frame N painted; the sink cannot
    /// know yet which of those frame N+1 will paint again, and waiting to
    /// find out means waiting for a frame that an input-driven painter may
    /// never draw. Clearing them all and letting this frame's paints
    /// re-place is the only order in which "visible ⟹ painted this frame"
    /// holds for every frame, including the last one before the user walks
    /// away from the keyboard.
    ///
    /// A box that stays on screen is unplaced and re-placed inside the same
    /// synchronized-update block, so no intermediate state reaches the glass;
    /// the cost is one ~20-byte escape per live image per frame, against a
    /// re-place that was already happening every frame anyway.
    ///
    /// **The record survives, and that is what makes the lowercase delete
    /// safe.** [`gfx::Emitter::delete`]'s doc comment warns that `d=i` would
    /// leave one orphaned raster per eviction, and it is right — but only
    /// because an *evicted* node loses its residency record, so the next paint
    /// allocates a **fresh** id via [`Self::alloc_id`] and the old raster
    /// becomes unreachable. Unplacing is not eviction: nothing is removed from
    /// the [`RasterCache`], so the node keeps its id, the id keeps pointing at
    /// the raster the terminal still holds, and the next `d=I` for that node
    /// frees exactly the raster the `d=i` left behind. Dropping the residency
    /// record here would reintroduce the orphan the uppercase delete exists to
    /// prevent.
    ///
    /// Order is paint order, purely for determinism in the byte stream.
    fn unplace_all(&mut self, out: &mut dyn Write) {
        let live: Vec<gfx::ImageId> = self
            .placed
            .iter()
            .filter_map(|node_id| self.cache.peek(*node_id))
            .map(|resident| resident.id)
            .collect();
        for id in live {
            self.emitter.clear_placements(id, out);
        }
        self.placed.clear();
    }

    /// Whether `incoming` can be given a placement this frame at all —
    /// checked *before* any decode or rasterization, so an over-quota box
    /// degrades to its text rung without paying for a raster it can never put
    /// on screen (and without re-paying on every subsequent frame).
    ///
    /// This is the whole of the [`CAP`] rule now. The frame boundary unplaces
    /// everything, so the live placements are exactly the boxes this frame has
    /// painted so far — there is no stale placement to reclaim and therefore
    /// no eviction decision to get wrong. Over the quota there is no honest
    /// placement to hand out, and taking one from a box already on screen this
    /// frame would blank it, so the caller degrades to its text rung: a rung
    /// that paints alt text beats one that paints nothing.
    fn has_placement_slot(&self, incoming: NodeId) -> bool {
        self.placed.contains(&incoming) || self.placed.len() < CAP
    }

    /// Drops a node entirely: its placement **and** its transmitted raster
    /// (`a=d,d=I`), plus every trace of it here. The residency-ending
    /// counterpart to [`Self::unplace_all`]'s visibility-ending `a=d,d=i`;
    /// a node deleted this way pays for a full re-transmit when it comes
    /// back, and gets a fresh image id when it does.
    fn delete_placement(&mut self, node_id: NodeId, out: &mut dyn Write) {
        if let Some(id) = self.cache.remove(node_id) {
            self.emitter.delete(id, out);
        }
        self.placed.retain(|&id| id != node_id);
    }

    /// Emits the placement for one media box: the cell rect it actually
    /// occupies on screen this frame, cropped to the matching slice of the
    /// source raster.
    ///
    /// Called on the box's first *visible* row, where `reserved.row` is the
    /// count of rows already scrolled above the viewport and `rect.height`
    /// is how many of the box's rows are on screen from here down.
    ///
    /// The single place a placement is created, so it is also the single
    /// place `placed` is appended to — every "this node is on the screen"
    /// claim in this file traces back to exactly one write. The node's raster
    /// must already be resident (a raster has to be transmitted before it can
    /// be placed); no residency means nothing to place, which is how a decode
    /// that failed after a previous frame's placement stays silent rather
    /// than emitting a put for freed pixel data.
    fn place_box(
        &mut self,
        node_id: NodeId,
        reserved: &Reserved,
        rect: CellRect,
        out: &mut dyn Write,
    ) {
        let Some(resident) = self.cache.peek(node_id) else {
            return;
        };
        let (id, raster_px) = (resident.id, resident.raster_px);
        if !self.placed.contains(&node_id) {
            self.placed.push(node_id);
        }
        let rows = reserved.rows.max(1);
        let top_skip = reserved.row.min(rows - 1);
        let visible = rect.height.max(1).min(rows - top_skip);
        let cells = gfx::CellRect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: visible,
        };
        self.emitter.place_cropped(
            id,
            cells,
            crop_source(raster_px, rows, top_skip, visible),
            out,
        );
    }

    /// Writes one fallback-text row **at `rect`'s own origin**, clipped to
    /// `rect.width` in **display cells** measured by grapheme cluster — not
    /// in `char`s.
    ///
    /// The leading `CUP` is load-bearing, not decoration. This sink is
    /// handed a [`CellRect`]; honouring it is the whole contract of the
    /// seam, and the caller's cursor is not where the box is. `Painter`'s
    /// `paint_inline_box` blanks the box by *writing* `cols` spaces, which
    /// leaves the cursor one whole box-width to the right; a fallback row
    /// written at that cursor lands outside the box and is then either
    /// overpainted by the following run or erased by the frame's
    /// `CLEAR_TO_EOL`. Either way the text rungs — image alt text, the txm
    /// Unicode grid, the literal-TeX source — rendered *nothing at all*,
    /// which is exactly the degraded path a user hits when a formula won't
    /// parse or an image won't decode. The graphics rung was never affected:
    /// [`gfx::Emitter`] positions its placements absolutely.
    ///
    /// Positioning here rather than at the call sites also makes the seam
    /// honour whatever `rect.x` it is handed, so a box indented by a
    /// blockquote or list gutter needs no second fix on this side.
    ///
    /// The cell-measured clip is load-bearing too: CJK alt text and txm's
    /// own double-width glyphs occupy up to two cells per character, so a
    /// char-count budget lets the row overflow its reserved box. On the
    /// bottom viewport row that overflow wraps, which scrolls the alternate
    /// screen and corrupts the frame. It is [`crate::painter::clip_to_width`]'s
    /// budget loop, called rather than re-implemented: a second copy of the
    /// cell-budget arithmetic is the drift risk this guard exists to prevent,
    /// and the copy that used to live here had already lost the saturating
    /// width the original returns.
    fn write_text_row(
        engine: &WidthEngine,
        line: Option<&String>,
        rect: CellRect,
        out: &mut dyn Write,
    ) {
        let text = line.map(String::as_str).unwrap_or("");
        let (clipped, _) = crate::painter::clip_to_width(text, engine, rect.width.max(1));
        // Emitted unconditionally, even for an empty row: the frame's
        // per-line `CLEAR_TO_EOL` fires from wherever this leaves the
        // cursor, so a row that writes no glyphs still has to leave it at
        // the box's own origin rather than a box-width past it.
        let _ = write!(
            out,
            "\x1b[{};{}H",
            rect.y.saturating_add(1),
            rect.x.saturating_add(1)
        );
        let _ = out.write_all(clipped.as_bytes());
    }

    fn resolve_content(
        &mut self,
        reserved: &Reserved,
        rect: CellRect,
        out: &mut dyn Write,
    ) -> Resolved {
        let node_id = reserved.node_id;
        let Some(NodeRef::Inline(inline)) = self.doc.node(node_id) else {
            Self::write_text_row(&self.width_engine, None, rect, out);
            return Resolved::Text(vec![String::new()]);
        };
        match inline.kind.clone() {
            InlineKind::Image { dest, children, .. } => {
                let alt = plain_text_of(&children);
                self.resolve_image(node_id, dest, alt, reserved, rect, out)
            }
            InlineKind::Math { tex, .. } => self.resolve_math(node_id, tex, reserved, rect, out),
            _ => {
                Self::write_text_row(&self.width_engine, None, rect, out);
                Resolved::Text(vec![String::new()])
            }
        }
    }

    fn target_px(&self, rect: CellRect, reserved: &Reserved) -> (u32, u32) {
        (
            u32::from(rect.width.max(1)) * self.cell_px.0,
            u32::from(reserved.rows.max(1)) * self.cell_px.1,
        )
    }

    /// Re-places an already-resident raster and reports whether it did.
    ///
    /// `false` means the node has no resident raster, or the resident one was
    /// rendered for a different target size (a resize) and has to be remade.
    /// `true` means this frame cost a re-place and nothing else — which is
    /// what the residency budget is paid for (DW-3.4): a box scrolls off,
    /// comes back, and the PNG never goes on the wire again.
    ///
    /// Shared by both resolvers on purpose. The two arms of the ladder carried
    /// byte-equivalent copies of this and of [`Self::transmit_and_place`], so
    /// the residency/placement invariants had two homes and a fix to one arm
    /// left the other quietly wrong.
    ///
    /// The quota is checked here as well as in [`Self::transmit_and_place`],
    /// and it has to be: once residency stopped being bounded by the
    /// placement cap, a document could hold more resident rasters than the
    /// terminal will accept placements for — and every one of those is a
    /// cache *hit*, which would otherwise walk straight past the only check
    /// there was. A 33rd visible box takes its text rung whether or not its
    /// pixels are already in the terminal.
    fn replace_if_cached(
        &mut self,
        node_id: NodeId,
        target: (u32, u32),
        reserved: &Reserved,
        rect: CellRect,
        out: &mut dyn Write,
    ) -> bool {
        if !self.has_placement_slot(node_id) || self.cache.get(node_id, target).is_none() {
            return false;
        }
        self.place_box(node_id, reserved, rect, out);
        true
    }

    /// Transmits `png` as this node's raster, records its residency, and
    /// places it.
    ///
    /// `false` — nothing written — when the frame's placement quota is spent,
    /// i.e. more than [`CAP`] boxes are visible at once. The caller degrades
    /// to its text rung; taking a slot from a box already on screen would
    /// blank it.
    ///
    /// `target` is the size the raster was *requested* at and is what the
    /// cache key compares; the raster's real pixel size is measured from the
    /// PNG, because only the image path scales to the target exactly (see
    /// [`Resident::raster_px`]).
    ///
    /// The budget's evictions are emitted here, from the ids the cache hands
    /// back — and never for a node in `placed`, so a box already on screen
    /// this frame cannot lose the pixels under it.
    fn transmit_and_place(
        &mut self,
        node_id: NodeId,
        png: &[u8],
        target: (u32, u32),
        reserved: &Reserved,
        rect: CellRect,
        out: &mut dyn Write,
    ) -> bool {
        if !self.has_placement_slot(node_id) {
            return false;
        }
        let id = self
            .cache
            .peek(node_id)
            .map(|resident| resident.id)
            .unwrap_or_else(|| self.alloc_id());
        let raster_px = png_pixel_size(png).unwrap_or(target);
        self.emitter.transmit(id, png, out);
        // Recorded before it is placed, not after: `place_box` is what marks
        // a node visible, and it can only do that for a node it can find.
        let pinned: HashSet<NodeId> = self.placed.iter().copied().collect();
        let freed = self.cache.insert(
            node_id,
            Resident::new(id, target, raster_px, png.len() as u64),
            &pinned,
        );
        for evicted in freed {
            self.emitter.delete(evicted, out);
        }
        self.place_box(node_id, reserved, rect, out);
        true
    }

    fn resolve_image(
        &mut self,
        node_id: NodeId,
        dest: String,
        alt: String,
        reserved: &Reserved,
        rect: CellRect,
        out: &mut dyn Write,
    ) -> Resolved {
        let target = self.target_px(rect, reserved);
        if self.replace_if_cached(node_id, target, reserved, rect, out) {
            return Resolved::Graphics;
        }

        // More than CAP boxes visible at once (an ordinary 40-badge README
        // row does it): there is no placement to be had this frame. Take the
        // alt-text rung now, before paying for a decode whose raster could
        // never reach the screen — and before an eviction that would blank a
        // box already painted this frame. Purely an optimization: the
        // authoritative gate is `transmit_and_place` below, which runs once
        // the raster is in hand so a *failed* decode never costs another
        // node its live placement.
        if !self.has_placement_slot(node_id) {
            return self.degrade_to_text(sanitize(&alt), rect, out);
        }

        let path = self.base_dir.join(&dest);
        let decoded = match gfx::decode::decode_and_scale(&path, target, self.limits) {
            Ok(decoded) => decoded,
            Err(_) => {
                // Decode failed (missing file, malformed/hostile image,
                // exceeds Limits) — degrade to alt text; drop any stale
                // placement so we never leave an orphaned image on screen.
                self.delete_placement(node_id, out);
                return self.degrade_to_text(sanitize(&alt), rect, out);
            }
        };
        if !self.transmit_and_place(node_id, &decoded.png, target, reserved, rect, out) {
            return self.degrade_to_text(sanitize(&alt), rect, out);
        }
        Resolved::Graphics
    }

    /// The ladder's text rung: paint `text` in the box and remember that
    /// this is what the box resolved to, so its later rows do the same.
    fn degrade_to_text(&self, text: String, rect: CellRect, out: &mut dyn Write) -> Resolved {
        Self::write_text_row(&self.width_engine, Some(&text), rect, out);
        Resolved::Text(vec![text])
    }

    fn resolve_math(
        &mut self,
        node_id: NodeId,
        tex: String,
        reserved: &Reserved,
        rect: CellRect,
        out: &mut dyn Write,
    ) -> Resolved {
        let target = self.target_px(rect, reserved);

        if !self.force_ratex_fail {
            if self.replace_if_cached(node_id, target, reserved, rect, out) {
                return Resolved::Graphics;
            }
            // Render at the SAME em the sizer measured this box with, then
            // letterbox that raster onto the box through the *same*
            // `gfx::decode` path an image takes.
            //
            // The letterbox is not optional and it does not belong in `math`:
            // kitty scales a placement to fill its `c=` x `r=` box exactly, so
            // only a raster whose pixels *are* the box escapes being stretched,
            // and producing one means padding per axis — a transparent canvas,
            // which is what `gfx::decode::letterbox_png` already does for
            // photos. `math` tried to approximate it with `ratex_render`'s
            // single symmetric padding and could not: 8 of the 33 fixture
            // formulas missed their box aspect by more than 10%.
            //
            // This used to pass `target.1` — the box's total pixel height —
            // into an argument that means *em size*, so every formula
            // rasterized oversized (by its own em-height factor) and the
            // terminal scaled it back down, burning resolution and stretching
            // the glyphs by whatever the cell rounding left over. Deriving the
            // em from the box instead is no better: it makes glyph size depend
            // on whether a formula happens to have a superscript, so `a+b=c`
            // and `e^{i\pi}+1=0` would render at visibly different sizes on
            // the same line. A fixed em is what keeps a formula looking like
            // the text around it.
            //
            // Skipped entirely when the live-placement cap is full of boxes
            // already painted this frame: rasterizing a formula that could
            // not be placed would burn the render for nothing and, worse,
            // an eviction here would blank a box already on screen. Falling
            // through lands on the txm rung below, which is what the ladder
            // is for.
            //
            // Both steps are on the cache-*miss* path only — `replace_if_cached`
            // returned above for a box whose raster is already resident at this
            // target — so the letterbox costs one decode+encode per formula per
            // (re)size, not one per frame.
            //
            // `letterbox_png` applies `self.limits` to the formula's raster,
            // which `math`'s own ceilings do not imply: `math` bounds pixmap
            // *area*, `gfx` bounds each *axis*. A formula wide enough in em
            // (4096 `W`s, legal TeX, rasterize 17758 px wide even at `math`'s
            // em floor) is declined here and takes the txm rung — a better rung
            // than transmitting a 7x-downscaled smear, and pinned by `math`'s
            // `test_a_formula_too_wide_for_gfx_limits_declines_at_the_seam_not_in_the_corpus`,
            // which also holds the whole fixture corpus inside the cap.
            if self.has_placement_slot(node_id)
                && let Ok(png) =
                    math::render_fitted(&tex, math_baseline_px(self.cell_px), target.0, target.1)
                && let Ok(boxed) = gfx::decode::letterbox_png(png.as_bytes(), target, self.limits)
                && self.transmit_and_place(node_id, &boxed, target, reserved, rect, out)
            {
                return Resolved::Graphics;
            }
        }

        // RaTeX rung failed (real parse error, or the test's forced
        // switch) — drop any stale placement and fall to the txm rung.
        self.delete_placement(node_id, out);
        if !self.force_txm_fail
            && let Some(grid) = math::render_text(&tex)
        {
            let lines: Vec<String> = grid.rows.iter().map(|r| sanitize(r)).collect();
            // This call is the box's first *visible* row, which is grid line
            // `reserved.row` — not grid line 0 — whenever the box's top has
            // scrolled above the viewport.
            let line = text_rung_line(&lines, reserved, 0);
            Self::write_text_row(&self.width_engine, line, rect, out);
            return Resolved::Text(lines);
        }
        // Literal rung: the raw TeX source, sanitized.
        self.degrade_to_text(sanitize(&tex), rect, out)
    }
}

impl MediaSink for GfxMediaSink {
    fn begin_frame(&mut self, out: &mut dyn Write) {
        self.begin_frame_inner(out);
    }

    fn paint(&mut self, reserved: &Reserved, rect: CellRect, out: &mut dyn Write) {
        let row_index = *self.row_in_frame.get(&reserved.node_id).unwrap_or(&0);
        self.row_in_frame.insert(reserved.node_id, row_index + 1);

        if row_index == 0 {
            let resolved = self.resolve_content(reserved, rect, out);
            self.resolved_this_frame.insert(reserved.node_id, resolved);
        } else if let Some(Resolved::Text(lines)) = self.resolved_this_frame.get(&reserved.node_id)
        {
            let line = text_rung_line(lines, reserved, row_index);
            Self::write_text_row(&self.width_engine, line, rect, out);
        }
    }

    fn evict(&mut self, node_id: NodeId, out: &mut dyn Write) {
        self.delete_placement(node_id, out);
    }

    /// Swaps in the reloaded document and forgets every node, screen and
    /// raster both.
    ///
    /// Forgetting is not optional housekeeping — it is the whole reason this
    /// method exists. Every key in `placements` is a `NodeId` into the
    /// *previous* document, and a re-parse renumbers nodes: keeping the map
    /// would leave a repaint of new node 7 reusing the image transmitted for
    /// old node 7, which is a different picture. Dropping the sink and
    /// registering a fresh one is not an alternative either: the old
    /// placements are state on the **terminal**, and a sink that no longer
    /// knows about them can never take them off the screen.
    fn reload_document(&mut self, doc: Rc<Document>, out: &mut dyn Write) {
        // Every resident raster, not every *placed* one: since the residency
        // split those are different sets, and the larger one is what carries
        // the hazard. A raster outlives its placement on purpose — that is
        // what makes a scroll-back cheap — so after a reload the cache is
        // exactly where an image keyed to a node that no longer exists would
        // survive, silently waiting to be re-placed for whatever the new
        // parse happens to number 7. Draining the cache is also what stops
        // `alloc_id` handing a new node an id the terminal still associates
        // with the old document's pixels.
        let known: Vec<NodeId> = self.cache.resident_nodes().collect();
        for node_id in known {
            self.delete_placement(node_id, out);
        }
        // `placed` is a subset of the resident set by construction
        // (`place_box` refuses a node with no raster), so the loop above has
        // already emptied it. Cleared anyway: this method's postcondition is
        // "this sink remembers nothing about the old document", and a
        // postcondition that holds by an invariant declared 400 lines away is
        // one edit from not holding.
        self.placed.clear();
        self.row_in_frame.clear();
        self.resolved_this_frame.clear();
        self.doc = doc;
    }
}

/// Which line of a text rung belongs on the box row currently being painted.
///
/// `nth_paint` is how many rows of this box this frame has already painted,
/// so `0` is the box's first *visible* row; `reserved.row` is that row's index
/// within the whole box, which differs from `nth_paint` by however many of the
/// box's rows have scrolled above the viewport.
///
/// The two rungs need different answers, so this is not one rule:
///
/// - A **multi-row** rung — the txm Unicode grid — is a picture drawn in
///   characters: a superscript raised above its baseline, a fraction bar over
///   its denominator. Its line *k* means "box row *k*" and nothing else, so it
///   indexes by [`Reserved::row`], exactly as the raster path crops by it.
///   Indexing by paint count instead re-anchors the picture on every scroll
///   step: the exponent of `x^2` slides down onto the baseline and the formula
///   reads as a different formula.
/// - A **single-line** rung — image alt text, the literal-TeX source — is not
///   a picture. It has one line and one chance to be seen, and it is what a
///   user gets precisely when something has already failed. Indexing it by box
///   row would make it vanish outright the moment the box is top-truncated,
///   which is worse than showing it one row low, so it stays anchored to the
///   box's first visible row.
///
/// (This is what `FIXER`'s wave-1 note meant by "fixing it properly means
/// deciding per-rung behaviour": the blanket rule is wrong in one direction
/// for each rung.)
fn text_rung_line<'a>(
    lines: &'a [String],
    reserved: &Reserved,
    nth_paint: u16,
) -> Option<&'a String> {
    let index = if lines.len() > 1 {
        reserved.row
    } else {
        nth_paint
    };
    lines.get(usize::from(index))
}

/// The slice of a `rows`-cell-tall box's raster that `visible` rows
/// starting at box row `top_skip` should show, in raster pixels.
///
/// `None` means "the whole raster" — the box is fully on screen, so the
/// placement omits the source keys entirely and kitty's own defaults apply.
///
/// The mapping is proportional, not cell-geometry-derived: the ratio is taken
/// against the raster's *own* measured pixel height, and the terminal scales
/// the source rectangle onto the requested cell box linearly on each axis, so
/// `top_skip/rows` of that height is `top_skip` cell rows whatever the
/// terminal's cell size turns out to be.
///
/// That is a stronger claim than "no cell-pixel query is available", which is
/// what an earlier revision of this comment argued from. It is now also the
/// only surviving claim: the live query *is* wired
/// ([`crate::terminal::query_cell_px`], through
/// [`GfxMediaSink::set_cell_px`]), and it did not change a byte of this
/// function — which is the point. Cell geometry feeds the *raster* stage, where
/// it decides resolution; the crop is measured against the raster's own pixels
/// and is correct for any cell size, queried or assumed.
fn crop_source(
    raster_px: (u32, u32),
    rows: u16,
    top_skip: u16,
    visible: u16,
) -> Option<gfx::SourceRect> {
    if top_skip == 0 && visible >= rows {
        return None;
    }
    let (w, h) = raster_px;
    if w == 0 || h == 0 {
        // Nothing measurable to crop against; a source rect built from this
        // would be a guess, and a wrong `h=` is worse than an uncropped
        // placement (kitty rejects an out-of-range rectangle outright).
        return None;
    }
    let rows = u32::from(rows.max(1));
    let top = u32::from(top_skip) * h / rows;
    let bottom = ((u32::from(top_skip) + u32::from(visible.max(1))) * h / rows).min(h);
    Some(gfx::SourceRect {
        x: 0,
        y: top,
        width: w,
        height: bottom.saturating_sub(top).max(1),
    })
}

/// Reads `(width, height)` out of a PNG's IHDR chunk, which the format
/// requires to be the first chunk immediately after the 8-byte signature.
///
/// Measured rather than assumed. The two raster producers used to disagree —
/// `gfx::decode::decode_and_scale` resized to the requested target exactly,
/// while `math::render_fitted` rasterized at a fixed em and only matched the
/// target's *aspect* — and cropping is in raster pixels, so guessing mis-cropped
/// every formula. Both now finish in the same `gfx::decode` letterbox and land
/// on the target, but the measurement is what makes that a *checked* fact at
/// the seam rather than an assumption inherited from two crates away.
fn png_pixel_size(png: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if png.len() < 24 || !png.starts_with(SIGNATURE) || &png[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(png[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(png[20..24].try_into().ok()?);
    (width > 0 && height > 0).then_some((width, height))
}

fn plain_text_of(children: &[Inline]) -> String {
    let mut out = String::new();
    for child in children {
        append_text(child, &mut out);
    }
    out
}

fn append_text(node: &Inline, out: &mut String) {
    match &node.kind {
        InlineKind::Text(t) | InlineKind::Code(t) => out.push_str(t),
        InlineKind::SoftBreak | InlineKind::HardBreak => out.push(' '),
        InlineKind::Emph(c) | InlineKind::Strong(c) | InlineKind::Strikethrough(c) => {
            for child in c {
                append_text(child, out);
            }
        }
        InlineKind::Link { children, .. } => {
            for child in children {
                append_text(child, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use ast::Document;

    use super::*;

    fn scratch_dir(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("stele-sink-test-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_png(dir: &std::path::Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        std::fs::File::create(&path)
            .unwrap()
            .write_all(&bytes)
            .unwrap();
        path
    }

    /// Writes a `w` x `h` PNG for the residency benchmark, reusing one
    /// already on disk from an earlier run.
    ///
    /// Flat colour and the fastest encoder settings, deliberately: what
    /// DW-3.5 measures is the *rescale* of 36 megapixels and the transmit of
    /// the result, both of which are content-independent, so paying seconds
    /// to compress noise would buy the measurement nothing. The cached copy
    /// is written to a temporary name and renamed, so two test binaries
    /// racing here cannot leave a half-written PNG behind.
    fn write_huge_png(dir: &std::path::Path, name: &str, w: u32, h: u32) -> PathBuf {
        use image::ImageEncoder;

        let path = dir.join(name);
        if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > 0) {
            return path;
        }
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([9, 40, 120, 255]));
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new_with_quality(
            &mut png,
            image::codecs::png::CompressionType::Fast,
            image::codecs::png::FilterType::NoFilter,
        )
        .write_image(img.as_raw(), w, h, image::ExtendedColorType::Rgba8)
        .expect("encode the benchmark fixture");
        let staging = dir.join(format!("{name}.partial"));
        std::fs::write(&staging, &png).expect("write the benchmark fixture");
        std::fs::rename(&staging, &path).expect("publish the benchmark fixture");
        path
    }

    /// The `nth` image id this process's allocator hands out (`nth_id(1)` is
    /// the first).
    ///
    /// Tests name real ids rather than `1, 2, 3`: an id carries a pid-derived
    /// instance tag (see [`gfx::IdAllocator`]), so a hardcoded `1` would be
    /// asserting the very global-namespace collision the tag exists to
    /// prevent. Derived from the same allocator production uses, not from a
    /// re-implementation of its arithmetic.
    fn nth_id(nth: u32) -> u32 {
        let mut alloc = gfx::IdAllocator::for_this_process();
        for _ in 1..nth {
            alloc.allocate();
        }
        alloc.allocate().get()
    }

    fn creates(out: &[u8]) -> usize {
        String::from_utf8_lossy(out).matches("a=t,f=100").count()
    }
    fn deletes(out: &[u8]) -> usize {
        String::from_utf8_lossy(out).matches("a=d,d=I").count()
    }

    /// Every `(kind, id)` kitty command in `out`, in wire order — the only
    /// way to ask *which* image an escape names rather than how many there
    /// were. `kind` is `'t'` (transmit), `'p'` (place), `'u'` (unplace:
    /// `d=i`, placements dropped, raster kept) or `'d'` (delete: `d=I`,
    /// raster freed too).
    ///
    /// The two deletes are *not* folded together. They answer different
    /// questions — `'u'` is about what the viewer can see, `'d'` about what
    /// the terminal is still holding — and every frame boundary now emits
    /// `'u'` for each live placement, so a test that lumped them would read
    /// an unplace as an eviction and pass for the wrong reason.
    fn commands(out: &[u8]) -> Vec<(char, u32)> {
        let text = String::from_utf8_lossy(out).into_owned();
        let mut found = Vec::new();
        for token in text.split("\x1b_G").skip(1) {
            let body = token.split('\x1b').next().unwrap_or("");
            let kind = match body {
                b if b.starts_with("a=t") => 't',
                b if b.starts_with("a=p") => 'p',
                b if b.starts_with("a=d,d=i") => 'u',
                b if b.starts_with("a=d,d=I") => 'd',
                _ => continue,
            };
            let Some(id) = body
                .split(&[',', ';'][..])
                .find_map(|kv| kv.strip_prefix("i="))
                .and_then(|v| v.parse::<u32>().ok())
            else {
                continue;
            };
            found.push((kind, id));
        }
        found
    }

    /// A model of the *terminal's* graphics state, driven only by the bytes
    /// the sink emits. Two sets, because the protocol has two: what the
    /// terminal is storing, and what it is drawing.
    ///
    /// This exists because counting escapes cannot answer the question this
    /// module's bug was about. "32 creates and 8 deletes" was true while
    /// eight images were blank; "one delete per image that scrolled away"
    /// was true while the image was still on the screen. The only assertion
    /// that closes either is *which ids the terminal is showing when the
    /// frame ends* — so this replays the wire and asks that directly.
    #[derive(Default, Debug)]
    struct TerminalGfx {
        /// Image ids whose pixel data the terminal is holding.
        stored: std::collections::BTreeSet<u32>,
        /// Image ids the terminal is currently *drawing*.
        visible: std::collections::BTreeSet<u32>,
    }

    impl TerminalGfx {
        /// Replays one frame's bytes and returns the ids that frame placed.
        fn apply(&mut self, out: &[u8]) -> std::collections::BTreeSet<u32> {
            let mut placed = std::collections::BTreeSet::new();
            for (kind, id) in commands(out) {
                match kind {
                    't' => {
                        self.stored.insert(id);
                    }
                    'p' => {
                        assert!(
                            self.stored.contains(&id),
                            "placed image {id} whose pixel data the terminal does not have \
                             (freed by an earlier a=d,d=I) — the put draws nothing"
                        );
                        self.visible.insert(id);
                        placed.insert(id);
                    }
                    'u' => {
                        self.visible.remove(&id);
                    }
                    'd' => {
                        self.visible.remove(&id);
                        self.stored.remove(&id);
                    }
                    _ => unreachable!(),
                }
            }
            placed
        }
    }

    /// Kitty image ids live in a namespace that is **global to the terminal**,
    /// so an id this sink transmits is an id every other graphics client on the
    /// same screen can transmit too — and per the spec the second transmit
    /// deletes the first image and all of its placements. A second stele in a
    /// neighbouring pane would therefore have blanked this one's images, and
    /// been blanked in return.
    ///
    /// Asserted on the id that actually reaches the wire, parsed out of the
    /// `a=t` command, because that is the only place the guarantee is real: the
    /// id must be tagged for this process, i.e. above the whole naive counter
    /// range that an untagged allocator hands out.
    #[test]
    fn test_transmitted_image_ids_are_namespaced_to_this_process() {
        let dir = scratch_dir("id-namespace");
        write_png(&dir, "ns.png");
        let doc = Document::parse("![alt](./ns.png)\n");
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .map(|n| n.id())
            .unwrap();
        let mut sink = GfxMediaSink::new(doc, &dir);
        let mut out = Vec::new();
        sink.begin_frame(&mut out);
        sink.paint(
            &Reserved {
                node_id,
                cols: 4,
                rows: 1,
                row: 0,
            },
            CellRect {
                x: 0,
                y: 0,
                width: 4,
                height: 1,
            },
            &mut out,
        );
        let transmitted: Vec<u32> = commands(&out)
            .into_iter()
            .filter(|&(kind, _)| kind == 't')
            .map(|(_, id)| id)
            .collect();
        assert_eq!(
            transmitted,
            vec![nth_id(1)],
            "one transmit, at the first id"
        );
        // 1..=0xFFFFF is the low, untagged slice — what an allocator that
        // counted from 1 would emit, and what every other naive client emits.
        assert!(
            transmitted[0] > 0xFFFFF,
            "image id {} is in the range a colliding client would use",
            transmitted[0]
        );
    }

    /// The bug this module's frame boundary exists to prevent, stated as
    /// directly as the wire allows: after any frame, the images the terminal
    /// is drawing are exactly the images that frame placed.
    ///
    /// The old code deferred eviction by one frame of grace, so an image that
    /// scrolled off stayed *visible* for two more frames. `stele` paints only
    /// in response to input, so those frames arrive only when the user
    /// presses another key: a single Ctrl-d left the previous screen's
    /// formulas painted over the new text indefinitely.
    #[test]
    fn test_nothing_is_visible_that_was_not_placed_in_the_frame_you_are_looking_at() {
        let dir = scratch_dir("visible-set");
        for i in 0..4u32 {
            write_png(&dir, &format!("v{i}.png"));
        }
        let mut source = String::new();
        for i in 0..4u32 {
            source.push_str(&format!("![alt{i}](./v{i}.png)\n\n"));
        }
        let doc = Document::parse(&source);
        let nodes: Vec<NodeId> = doc
            .nodes()
            .filter(
                |n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })),
            )
            .map(|n| n.id())
            .collect();
        let mut sink = GfxMediaSink::new(doc, &dir);
        let mut term = TerminalGfx::default();

        // Each entry is one frame's worth of visible boxes — a scroll that
        // reveals, holds, drops and finally clears every image, including a
        // frame with no media at all (the Ctrl-d that lands on a screen of
        // plain text) and a scroll back up to an image seen before.
        let frames: [&[usize]; 6] = [&[0, 1], &[1, 2], &[3], &[], &[3], &[0]];
        for (n, visible_now) in frames.iter().enumerate() {
            let mut out = Vec::new();
            sink.begin_frame(&mut out);
            for (row, &idx) in visible_now.iter().enumerate() {
                sink.paint(
                    &Reserved {
                        node_id: nodes[idx],
                        cols: 10,
                        rows: 1,
                        row: 0,
                    },
                    CellRect {
                        x: 0,
                        y: row as u16,
                        width: 10,
                        height: 1,
                    },
                    &mut out,
                );
            }
            let placed = term.apply(&out);
            assert_eq!(
                term.visible, placed,
                "frame {n} (showing boxes {visible_now:?}) ends with the terminal drawing \
                 {:?}, but only {placed:?} were placed in it",
                term.visible
            );
            assert_eq!(
                term.visible.len(),
                visible_now.len(),
                "frame {n} must draw exactly one image per visible box"
            );
        }
    }

    /// The other half of the split: a box leaving the screen ends its
    /// *visibility* without ending its *residency*. The frame it leaves in
    /// emits `d=i` and no `d=I`, so the terminal stops drawing it while
    /// still holding its pixels — which is what makes coming back a re-place
    /// rather than a re-transmit, and what the LRU cap and the grace exist
    /// to protect.
    ///
    /// This asserts the separation itself, not a saved re-transmit: with
    /// `DATA_GRACE_FRAMES` at its current value the sweep at the top of a
    /// frame frees the raster of anything absent from the *previous* frame,
    /// before that frame's paints can claim it, so today the resident window
    /// closes one frame too early to ever be used. That is a separate
    /// finding about the grace's arithmetic, not about this seam, and it is
    /// deliberately not what this test pins — this test stays green whether
    /// or not the window is widened.
    #[test]
    fn test_a_box_that_leaves_the_screen_is_unplaced_but_keeps_its_raster() {
        let dir = scratch_dir("replace-not-retransmit");
        write_png(&dir, "a.png");
        write_png(&dir, "b.png");
        let doc = Document::parse("![alt-a](./a.png)\n\n![alt-b](./b.png)\n");
        let nodes: Vec<NodeId> = doc
            .nodes()
            .filter(
                |n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })),
            )
            .map(|n| n.id())
            .collect();
        let mut sink = GfxMediaSink::new(doc, &dir);
        let reserved = |id: NodeId| Reserved {
            node_id: id,
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

        let mut term = TerminalGfx::default();

        let mut f1 = Vec::new();
        sink.begin_frame(&mut f1);
        sink.paint(&reserved(nodes[0]), rect, &mut f1);
        assert_eq!(commands(&f1), vec![('t', nth_id(1)), ('p', nth_id(1))]);
        term.apply(&f1);

        // A scrolls off. Its placement must go — but its raster must not.
        let mut f2 = Vec::new();
        sink.begin_frame(&mut f2);
        sink.paint(&reserved(nodes[1]), rect, &mut f2);
        assert_eq!(
            commands(&f2),
            vec![('u', nth_id(1)), ('t', nth_id(2)), ('p', nth_id(2))],
            "A must be unplaced (d=i), never deleted (d=I), on the frame it leaves"
        );
        term.apply(&f2);
        assert_eq!(
            term.visible,
            std::collections::BTreeSet::from([nth_id(2)]),
            "only B is on the screen once A has scrolled off"
        );
        assert_eq!(
            term.stored,
            std::collections::BTreeSet::from([nth_id(1), nth_id(2)]),
            "A's pixel data must survive its placement — residency and \
             visibility are different lifetimes"
        );
    }

    /// The invariant that makes unplacing with lowercase `d=i` safe at all:
    /// an unplaced node keeps its **image id**, so the id still names the
    /// raster the terminal is still holding.
    ///
    /// [`gfx::Emitter::delete`]'s doc comment argues that lowercase would
    /// orphan one raster per eviction because an evicted node comes back
    /// under a *fresh* id. That argument is correct, and it is exactly why
    /// the frame boundary must not remove the node's record when it takes
    /// the placement off the screen. This asserts the consequence on the
    /// wire: a box that is on screen two frames running is unplaced and
    /// re-placed under the same id, with no PNG going back out.
    #[test]
    fn test_an_unplaced_box_keeps_its_image_id_so_no_raster_is_orphaned() {
        let dir = scratch_dir("stable-id");
        write_png(&dir, "a.png");
        let doc = Document::parse("![alt-a](./a.png)\n");
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        let mut sink = GfxMediaSink::new(doc, &dir);
        let reserved = Reserved {
            node_id,
            cols: 10,
            rows: 1,
            row: 0,
        };

        let mut f1 = Vec::new();
        sink.begin_frame(&mut f1);
        sink.paint(
            &reserved,
            CellRect {
                x: 0,
                y: 3,
                width: 10,
                height: 1,
            },
            &mut f1,
        );
        assert_eq!(commands(&f1), vec![('t', nth_id(1)), ('p', nth_id(1))]);

        // Same box, next frame, one row higher. The frame boundary unplaces
        // it and the paint puts it back — same id both times.
        let mut f2 = Vec::new();
        sink.begin_frame(&mut f2);
        sink.paint(
            &reserved,
            CellRect {
                x: 0,
                y: 2,
                width: 10,
                height: 1,
            },
            &mut f2,
        );
        assert_eq!(
            commands(&f2),
            vec![('u', nth_id(1)), ('p', nth_id(1))],
            "an unplaced node must keep its id and its raster: no fresh transmit, \
             and no id the earlier raster is no longer reachable through"
        );
        assert!(
            String::from_utf8_lossy(&f2).contains(&format!(
                "\x1b[3;1H\x1b_Ga=p,i={},p={},",
                nth_id(1),
                nth_id(1)
            )),
            "and it must be re-placed at its NEW row: {:?}",
            String::from_utf8_lossy(&f2)
        );
    }

    #[test]
    fn test_dw_6_1_cap_32_refuses_the_33rd_box_instead_of_evicting_a_live_one() {
        let dir = scratch_dir("cap");
        let mut out = Vec::new();
        for i in 0..40u32 {
            write_png(&dir, &format!("pic{i}.png"));
        }
        // One doc with all 40 images so `sink.doc.node(id)` resolves every
        // one of them, then paint each in turn at increasing y — simulates
        // a viewport tall enough to show 40 images simultaneously within a
        // single frame, isolating the CAP/LRU mechanism from the
        // grace-period sweep (which needs a *second* frame to trigger).
        let mut source = String::new();
        for i in 0..40u32 {
            source.push_str(&format!("![alt{i}](./pic{i}.png)\n\n"));
        }
        let doc = Document::parse(&source);
        let mut sink = GfxMediaSink::new(doc.clone(), &dir);
        let image_nodes: Vec<NodeId> = doc
            .nodes()
            .filter(
                |n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })),
            )
            .map(|n| n.id())
            .collect();
        assert_eq!(image_nodes.len(), 40);

        for (row, &node_id) in image_nodes.iter().enumerate() {
            let reserved = Reserved {
                node_id,
                cols: 10,
                rows: 1,
                row: 0,
            };
            let rect = CellRect {
                x: 0,
                y: row as u16,
                width: 10,
                height: 1,
            };
            sink.paint(&reserved, rect, &mut out);
        }

        // The cap still holds — but it is now held by *refusing* the 33rd
        // placement, not by deleting one of the 32 already painted this
        // frame. (This assertion used to read `creates == 40, deletes == 8`.
        // Both numbers were correct and eight images were nonetheless blank:
        // LRU order within a frame is paint order, so the deleted eight were
        // deterministically the boxes at the top of the viewport, erased
        // with nothing painted in their place. A count cannot see that.)
        assert_eq!(
            creates(&out),
            CAP,
            "exactly CAP boxes may be transmitted in a frame that shows more than CAP"
        );
        assert_eq!(
            deletes(&out),
            0,
            "no image may be deleted in the same frame it was placed"
        );

        // The invariant behind that count, stated directly: for every id, no
        // delete follows its create within this frame.
        let cmds = commands(&out);
        for (i, &(kind, id)) in cmds.iter().enumerate() {
            // Both flavours of delete count here: freeing the raster and
            // merely taking the placement off the screen leave the same hole
            // in the same cells.
            if kind != 'd' && kind != 'u' {
                continue;
            }
            let placed_earlier = cmds[..i].iter().any(|&(k, other)| k == 't' && other == id);
            assert!(
                !placed_earlier,
                "image {id} was deleted in the same frame it was transmitted: {cmds:?}"
            );
        }

        // ...and the eight boxes that could not get a placement are not
        // blank: each paints its own alt text, at its own row, in its own
        // reserved box. This is the ladder's contract — a rung that paints
        // nothing is worse than one that paints alt text.
        let text = String::from_utf8_lossy(&out);
        for row in CAP..40 {
            let expected = format!("\x1b[{};1Halt{row}", row + 1);
            assert!(
                text.contains(&expected),
                "over-cap box {row} must fall back to alt text at its own \
                 cell rect; expected {expected:?} in the frame"
            );
        }
        for row in 0..CAP {
            assert!(
                !text.contains(&format!("\x1b[{};1Halt{row}", row + 1)),
                "box {row} got a real placement and must not also paint alt text"
            );
        }
    }

    /// The cap is a *per-frame placement quota*, not a bound on how many
    /// rasters may exist. The complement of the test above: that one proves
    /// nothing on screen is evicted, this one proves a 33rd box still gets a
    /// real placement in the next frame — and, since DW-3.6, that it does so
    /// **without** costing any of the first 32 their pixel data.
    ///
    /// This used to assert the opposite of that last clause (`d=I` on the
    /// least-recently-painted node, "to make room"), which was the coupling
    /// DW-3.6 removes: room was needed only because the placement cap was
    /// checked against the map that also held rasters.
    #[test]
    fn test_dw_3_6_a_thirty_third_box_is_placed_without_costing_any_raster() {
        let dir = scratch_dir("cap-across-frames");
        for i in 0..33u32 {
            write_png(&dir, &format!("pic{i}.png"));
        }
        let mut source = String::new();
        for i in 0..33u32 {
            source.push_str(&format!("![alt{i}](./pic{i}.png)\n\n"));
        }
        let doc = Document::parse(&source);
        let nodes: Vec<NodeId> = doc
            .nodes()
            .filter(
                |n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })),
            )
            .map(|n| n.id())
            .collect();
        let mut sink = GfxMediaSink::new(doc, &dir);
        let rect = CellRect {
            x: 0,
            y: 0,
            width: 10,
            height: 1,
        };

        // Frame 1 fills the cap exactly.
        let mut f1 = Vec::new();
        sink.begin_frame(&mut f1);
        for (row, &node_id) in nodes[..CAP].iter().enumerate() {
            let reserved = Reserved {
                node_id,
                cols: 10,
                rows: 1,
                row: 0,
            };
            sink.paint(
                &reserved,
                CellRect {
                    y: row as u16,
                    ..rect
                },
                &mut f1,
            );
        }
        assert_eq!(creates(&f1), CAP);
        assert_eq!(deletes(&f1), 0);

        // Frame 2 shows only the 33rd. Nothing from frame 1 is on screen, so
        // the quota is free again and it must get a real graphics placement,
        // not a fallback.
        let mut f2 = Vec::new();
        sink.begin_frame(&mut f2);
        sink.paint(
            &Reserved {
                node_id: nodes[CAP],
                cols: 10,
                rows: 1,
                row: 0,
            },
            rect,
            &mut f2,
        );
        let cmds = commands(&f2);
        assert!(
            !cmds.iter().any(|&(k, _)| k == 'd'),
            "no raster may be freed to make room for the 33rd box — retention is \
             the byte budget's business, not the placement cap's; got {cmds:?}"
        );
        assert_eq!(
            cmds.iter().filter(|&&(k, _)| k == 'u').count(),
            CAP,
            "every placement frame 1 left on screen must be taken off it, got {cmds:?}"
        );
        assert_eq!(
            cmds.iter()
                .filter(|&&(k, _)| k == 'p')
                .map(|&(_, id)| id)
                .collect::<Vec<_>>(),
            vec![nth_id(33)],
            "the only image visible after frame 2 is the one frame 2 painted, got {cmds:?}"
        );
        assert_eq!(creates(&f2), 1, "the 33rd box must get a real placement");
        assert!(
            !String::from_utf8_lossy(&f2).contains("alt32"),
            "a box that got a placement must not also paint alt text"
        );
        assert_eq!(
            sink.resident_count(),
            CAP + 1,
            "all 33 rasters must still be resident: 33 > CAP, and the budget \
             is nowhere near spent"
        );

        // The consequence a reader would notice: scrolling back to the first
        // box re-places it and never re-transmits it.
        let mut f3 = Vec::new();
        sink.begin_frame(&mut f3);
        sink.paint(
            &Reserved {
                node_id: nodes[0],
                cols: 10,
                rows: 1,
                row: 0,
            },
            rect,
            &mut f3,
        );
        assert_eq!(
            commands(&f3),
            vec![('u', nth_id(33)), ('p', nth_id(1))],
            "scrolling back to box 0 must cost one re-place and no transmit"
        );
    }

    /// DW-3.4, stated as the wire fact it is: a box that leaves the screen
    /// and comes back is *re-placed*, with no PNG going out a second time.
    ///
    /// This is the case the pre-change grace arithmetic could never reach.
    /// It freed a raster at the top of the frame after the one that stopped
    /// painting it — i.e. exactly the frame that wanted it back — so the
    /// resident window, though documented as one frame wide, was empty. The
    /// baseline arm below runs that original behaviour and shows it.
    #[test]
    fn test_dw_3_4_a_box_scrolled_off_and_back_is_re_placed_without_re_transmitting() {
        let dir = scratch_dir("scroll-back");
        write_png(&dir, "a.png");
        write_png(&dir, "b.png");
        let doc = Document::parse("![alt-a](./a.png)\n\n![alt-b](./b.png)\n");
        let nodes: Vec<NodeId> = doc
            .nodes()
            .filter(
                |n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })),
            )
            .map(|n| n.id())
            .collect();
        let reserved = |id: NodeId| Reserved {
            node_id: id,
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

        // Frames: A, then B (A is off screen), then A again.
        let return_frame = |pre_change: bool| {
            let mut sink = GfxMediaSink::new(doc.clone(), &dir);
            sink.simulate_pre_change_residency_for_test(pre_change);
            let mut scratch = Vec::new();
            sink.begin_frame(&mut scratch);
            sink.paint(&reserved(nodes[0]), rect, &mut scratch);
            sink.begin_frame(&mut scratch);
            sink.paint(&reserved(nodes[1]), rect, &mut scratch);
            let mut back = Vec::new();
            sink.begin_frame(&mut back);
            sink.paint(&reserved(nodes[0]), rect, &mut back);
            back
        };

        let fixed = return_frame(false);
        assert_eq!(
            commands(&fixed)
                .iter()
                .filter(|&&(kind, _)| kind == 't')
                .count(),
            0,
            "the return frame must not re-transmit: {:?}",
            String::from_utf8_lossy(&fixed)
        );
        assert!(
            commands(&fixed)
                .iter()
                .any(|&(kind, id)| kind == 'p' && id == nth_id(1)),
            "...and must re-place A under its original id: {:?}",
            String::from_utf8_lossy(&fixed)
        );

        let baseline = return_frame(true);
        assert_eq!(
            commands(&baseline)
                .iter()
                .filter(|&&(kind, _)| kind == 't')
                .count(),
            1,
            "the fixture is broken: the pre-change window must re-transmit, or \
             this test cannot tell the two apart"
        );
    }

    /// The other side of DW-3.4: residency is a *budget*, so a raster the
    /// budget could not keep is re-transmitted when it comes back. Without
    /// this, "never re-transmits" would be indistinguishable from "never
    /// evicts", and an unbounded cache would pass.
    #[test]
    fn test_dw_3_4_a_raster_the_budget_evicted_is_re_transmitted_when_it_returns() {
        let dir = scratch_dir("budget-evicts");
        write_png(&dir, "a.png");
        write_png(&dir, "b.png");
        let doc = Document::parse("![alt-a](./a.png)\n\n![alt-b](./b.png)\n");
        let nodes: Vec<NodeId> = doc
            .nodes()
            .filter(
                |n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })),
            )
            .map(|n| n.id())
            .collect();
        // A budget one byte short of holding two rasters, measured rather
        // than guessed: paint A alone and read what it actually cost.
        let mut probe = GfxMediaSink::new(doc.clone(), &dir);
        let reserved = |id: NodeId| Reserved {
            node_id: id,
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
        let mut scratch = Vec::new();
        probe.begin_frame(&mut scratch);
        probe.paint(&reserved(nodes[0]), rect, &mut scratch);
        let one_raster = probe.resident_bytes();
        assert!(one_raster > 0, "a painted box must cost bytes");

        let mut sink = GfxMediaSink::new(doc, &dir).with_raster_budget(one_raster);
        let mut f1 = Vec::new();
        sink.begin_frame(&mut f1);
        sink.paint(&reserved(nodes[0]), rect, &mut f1);
        let mut f2 = Vec::new();
        sink.begin_frame(&mut f2);
        sink.paint(&reserved(nodes[1]), rect, &mut f2);
        assert!(
            commands(&f2)
                .iter()
                .any(|&(kind, id)| kind == 'd' && id == nth_id(1)),
            "B's raster must push A's out of a budget that fits only one: {:?}",
            commands(&f2)
        );
        assert_eq!(sink.resident_count(), 1);

        let mut f3 = Vec::new();
        sink.begin_frame(&mut f3);
        sink.paint(&reserved(nodes[0]), rect, &mut f3);
        assert!(
            commands(&f3).iter().any(|&(kind, _)| kind == 't'),
            "A lost its raster to the budget, so its return must re-transmit: {:?}",
            commands(&f3)
        );
    }

    /// DW-3.6: a document with more images than the placement cap keeps a
    /// raster for every one of them, because the budget — not the cap — is
    /// what governs retention.
    ///
    /// Painted one box per frame, so the placement quota is never the binding
    /// constraint and the only thing that could stop the 33rd..40th rasters
    /// existing is the coupling this DW removes.
    #[test]
    fn test_dw_3_6_forty_images_keep_their_rasters_although_only_32_may_be_placed() {
        let dir = scratch_dir("retention-past-cap");
        let count = 40usize;
        for i in 0..count {
            write_png(&dir, &format!("r{i}.png"));
        }
        let source: String = (0..count)
            .map(|i| format!("![alt{i}](./r{i}.png)\n\n"))
            .collect();
        let doc = Document::parse(&source);
        let nodes: Vec<NodeId> = doc
            .nodes()
            .filter(
                |n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })),
            )
            .map(|n| n.id())
            .collect();
        assert_eq!(nodes.len(), count);
        let mut sink = GfxMediaSink::new(doc, &dir);
        let rect = CellRect {
            x: 0,
            y: 0,
            width: 10,
            height: 1,
        };
        let reserved = |id: NodeId| Reserved {
            node_id: id,
            cols: 10,
            rows: 1,
            row: 0,
        };

        let mut out = Vec::new();
        for &node_id in &nodes {
            sink.begin_frame(&mut out);
            sink.paint(&reserved(node_id), rect, &mut out);
        }
        assert_eq!(
            sink.resident_count(),
            count,
            "all {count} rasters must be resident — CAP is {CAP} and bounds \
             placements, not retention"
        );
        assert_eq!(deletes(&out), 0, "nothing may be freed under the budget");
        assert!(sink.resident_bytes() < RASTER_BUDGET_BYTES);

        // The reader-visible consequence: every one of the 40 comes back as a
        // re-place. A cache bounded at 32 would re-transmit the first eight.
        let mut back = Vec::new();
        for &node_id in &nodes {
            sink.begin_frame(&mut back);
            sink.paint(&reserved(node_id), rect, &mut back);
        }
        assert_eq!(
            creates(&back),
            0,
            "revisiting every box must cost re-places only"
        );
    }

    /// The hole the residency split opens if the quota is only checked on the
    /// transmit path: 40 rasters can now be resident at once (that is DW-3.6's
    /// whole point), so a frame that brings all 40 boxes on screen sees 40
    /// cache *hits* — and a cache hit used to place unconditionally, because
    /// under the old shared map "resident" implied "within the cap".
    ///
    /// Seeded one box per frame so every raster is resident and nothing is
    /// blocked on the way in, then all 40 are painted in a single frame.
    #[test]
    fn test_the_placement_cap_holds_even_when_every_raster_is_already_resident() {
        let dir = scratch_dir("cap-with-warm-cache");
        let count = 40usize;
        for i in 0..count {
            write_png(&dir, &format!("w{i}.png"));
        }
        let source: String = (0..count)
            .map(|i| format!("![alt{i}](./w{i}.png)\n\n"))
            .collect();
        let doc = Document::parse(&source);
        let nodes: Vec<NodeId> = doc
            .nodes()
            .filter(
                |n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })),
            )
            .map(|n| n.id())
            .collect();
        let mut sink = GfxMediaSink::new(doc, &dir);
        let reserved = |id: NodeId| Reserved {
            node_id: id,
            cols: 10,
            rows: 1,
            row: 0,
        };

        let mut warm = Vec::new();
        for &node_id in &nodes {
            sink.begin_frame(&mut warm);
            sink.paint(
                &reserved(node_id),
                CellRect {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 1,
                },
                &mut warm,
            );
        }
        assert_eq!(
            sink.resident_count(),
            count,
            "every raster must be resident"
        );

        let mut all = Vec::new();
        sink.begin_frame(&mut all);
        for (row, &node_id) in nodes.iter().enumerate() {
            sink.paint(
                &reserved(node_id),
                CellRect {
                    x: 0,
                    y: row as u16,
                    width: 10,
                    height: 1,
                },
                &mut all,
            );
        }
        let placements = commands(&all).iter().filter(|&&(k, _)| k == 'p').count();
        assert_eq!(
            placements, CAP,
            "at most {CAP} placements may be live in one frame, warm cache or not"
        );
        assert_eq!(creates(&all), 0, "nothing needed re-transmitting");
        // The boxes that could not be placed are not blank: each paints its
        // own alt text, as the ladder requires.
        let text = String::from_utf8_lossy(&all);
        for row in CAP..count {
            assert!(
                text.contains(&format!("\x1b[{};1Halt{row}", row + 1)),
                "over-quota box {row} must fall back to alt text at its own row"
            );
        }
    }

    /// **DW-3.5, the committed speed claim.** The frame that brings a
    /// 6000x6000 image back on screen must be at least 10x faster than the
    /// same frame under the pre-change residency window, measured in this
    /// harness with that window switched back on.
    ///
    /// The baseline is the real old behaviour, not a description of it:
    /// [`GfxMediaSink::simulate_pre_change_residency_for_test`] reinstates the
    /// sweep that freed, at the top of frame *N*, every raster absent from
    /// frame *N-1* — so the return frame re-decodes, re-scales, re-encodes and
    /// re-transmits the whole image, exactly as it did before this phase.
    ///
    /// One sink and two timed frames, so the fixture is decoded twice rather
    /// than three times: frames 1-3 seed and measure the fixed return, then
    /// the switch goes on and frames 4-5 measure the pre-change return.
    ///
    /// A ratio rather than a latency budget, for the same reason DW-1.7 uses
    /// one: CI hosts vary, but "one ~40-byte escape" against "decode 36
    /// megapixels, rescale, PNG-encode and base64 a raster" is an
    /// architectural difference of three orders of magnitude, so a 10x floor
    /// has enormous margin against scheduling noise.
    #[test]
    fn test_dw_3_5_scroll_back_return_frame_is_at_least_10x_faster_than_the_pre_change_baseline() {
        let dir = scratch_dir("scroll-back-bench");
        let name = "huge.png";
        write_huge_png(&dir, name, 6000, 6000);
        let doc = Document::parse(&format!("![big](./{name})\n"));
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        // A 40x20 cell box at the fallback 24x48 cell geometry: a 960x960
        // raster target, i.e. a real full-width-ish image box, not a token one.
        let reserved = Reserved {
            node_id,
            cols: 40,
            rows: 20,
            row: 0,
        };
        let rect = CellRect {
            x: 0,
            y: 0,
            width: 40,
            height: 20,
        };
        let mut sink = GfxMediaSink::new(doc, &dir);
        let mut out = Vec::new();

        // Frame 1: on screen, transmitted. Frame 2: scrolled off.
        sink.begin_frame(&mut out);
        sink.paint(&reserved, rect, &mut out);
        assert_eq!(creates(&out), 1, "the fixture must transmit once to start");
        sink.begin_frame(&mut out);

        // Frame 3: back on screen, with the residency this phase ships.
        let mut fixed_frame = Vec::new();
        let start = std::time::Instant::now();
        sink.begin_frame(&mut fixed_frame);
        sink.paint(&reserved, rect, &mut fixed_frame);
        let fixed = start.elapsed();
        assert_eq!(
            creates(&fixed_frame),
            0,
            "the return frame must not re-transmit"
        );

        // Frames 4-5: the same scroll-off / scroll-back, with the pre-change
        // window back on. Frame 4 leaves the box off screen; frame 5's own
        // boundary is where the old sweep frees the raster the frame is about
        // to want, so the re-transmit is inside the timed region exactly as it
        // was before this phase.
        sink.simulate_pre_change_residency_for_test(true);
        sink.begin_frame(&mut out);
        let mut baseline_frame = Vec::new();
        let start = std::time::Instant::now();
        sink.begin_frame(&mut baseline_frame);
        sink.paint(&reserved, rect, &mut baseline_frame);
        let baseline = start.elapsed();
        assert_eq!(
            creates(&baseline_frame),
            1,
            "the fixture is broken: the pre-change window must re-transmit, or \
             there is no baseline to be faster than"
        );

        assert!(
            fixed.as_nanos() * 10 <= baseline.as_nanos(),
            "the scroll-back return frame must be at least 10x faster than the \
             pre-change baseline: fixed={fixed:?} baseline={baseline:?}"
        );
    }

    #[test]
    fn test_dw_3_4_100_scroll_cycles_transmit_each_image_exactly_once() {
        let dir = scratch_dir("cycles");
        for i in 0..5 {
            write_png(&dir, &format!("p{i}.png"));
        }
        let mut source = String::new();
        for i in 0..5 {
            source.push_str(&format!("![alt{i}](./p{i}.png)\n\n"));
        }
        let doc = Document::parse(&source);
        let nodes: Vec<NodeId> = doc
            .nodes()
            .filter(
                |n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })),
            )
            .map(|n| n.id())
            .collect();
        let mut sink = GfxMediaSink::new(doc, &dir);
        let mut out = Vec::new();

        // 100 "scroll cycles": each cycle shows exactly one of the 5
        // images (round robin) in its own frame, simulating scrolling past
        // and back to each repeatedly.
        for cycle in 0..100usize {
            let node_id = nodes[cycle % nodes.len()];
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
            sink.begin_frame(&mut out);
            sink.paint(&reserved, rect, &mut out);
        }

        // The claim this replaces was `creates - deletes == live` plus
        // `deletes > 0` — a balance, which stayed green while the resident
        // window was closed and every one of the 100 cycles paid for a full
        // re-transmit. The stronger statement a balance cannot make: five
        // images, five transmits, whatever the cycle count.
        assert_eq!(
            creates(&out),
            nodes.len(),
            "each image must go on the wire exactly once across 100 cycles"
        );
        assert_eq!(
            deletes(&out),
            0,
            "five small rasters are nowhere near the byte budget, so nothing \
             may be freed"
        );
        assert_eq!(sink.resident_count(), nodes.len());
        // The cap is about placements, and exactly one box is visible per
        // frame here — so the terminal is never asked to draw more than one.
        let mut term = TerminalGfx::default();
        term.apply(&out);
        assert_eq!(term.visible.len(), 1);
        assert!(sink.resident_bytes() < RASTER_BUDGET_BYTES);
    }

    #[test]
    fn test_dw_6_2_full_ladder_via_injected_failures() {
        let dir = scratch_dir("ladder");
        // A flat expression (no superscript/fraction vertical stacking) so
        // txm's grid is guaranteed to fit the single row this test's
        // `Reserved` reserves — a stacked expression like `x^2` can span
        // more than one txm row (the superscript raised above baseline),
        // which would need this test to paint every row of the box, not
        // just row 0.
        let doc = Document::parse("$a+b$\n");
        let mut sink = GfxMediaSink::new(doc.clone(), &dir);
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Math { .. })))
            .unwrap()
            .id();
        let reserved = Reserved {
            node_id,
            cols: 20,
            rows: 1,
            row: 0,
        };
        let rect = CellRect {
            x: 0,
            y: 0,
            width: 20,
            height: 1,
        };

        // Rung 1 (RaTeX): normal operation renders graphics, not text.
        let mut out = Vec::new();
        sink.begin_frame(&mut out);
        sink.paint(&reserved, rect, &mut out);
        assert!(
            creates(&out) == 1,
            "expected the RaTeX rung to emit a graphics placement"
        );

        // Rung 2 (txm): force RaTeX to fail -> must fall to the txm text
        // grid — no NEW graphics placement (a cleanup delete of rung 1's
        // now-stale placement is expected and correct, so this checks for
        // the absence of a create, not the absence of any kitty escape at
        // all; DW-6.4's stricter "zero escapes ever" is a separate case,
        // asserted in `sizer.rs`, where no placement ever existed to
        // clean up in the first place).
        sink.force_ratex_failure_for_test(true);
        let mut out2 = Vec::new();
        sink.begin_frame(&mut out2);
        sink.paint(&reserved, rect, &mut out2);
        assert_eq!(creates(&out2), 0, "RaTeX rung must be bypassed");
        assert!(
            String::from_utf8_lossy(&out2).contains('+'),
            "txm rung must render actual glyph content, got {:?}",
            String::from_utf8_lossy(&out2)
        );

        // Rung 3 (literal): force BOTH RaTeX and txm to fail -> the raw
        // TeX source must appear verbatim, and no new graphics placement.
        sink.force_txm_failure_for_test(true);
        let mut out3 = Vec::new();
        sink.begin_frame(&mut out3);
        sink.paint(&reserved, rect, &mut out3);
        assert_eq!(creates(&out3), 0);
        assert!(String::from_utf8_lossy(&out3).contains("a+b"));
    }

    #[test]
    fn test_dw_6_3_hostile_image_degrades_to_alt_text_no_crash() {
        let dir = scratch_dir("hostile");
        // A malformed "PNG": right magic bytes, garbage after.
        std::fs::write(dir.join("bad.png"), b"\x89PNGnotarealpngatall").unwrap();
        let doc = Document::parse("![fallback text](./bad.png)\n");
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        let mut sink = GfxMediaSink::new(doc, &dir);
        let reserved = Reserved {
            node_id,
            cols: 20,
            rows: 1,
            row: 0,
        };
        let rect = CellRect {
            x: 0,
            y: 0,
            width: 20,
            height: 1,
        };
        let mut out = Vec::new();
        sink.paint(&reserved, rect, &mut out);
        assert_eq!(creates(&out), 0);
        assert!(!String::from_utf8_lossy(&out).contains("\x1b_G"));
        assert!(String::from_utf8_lossy(&out).contains("fallback text"));
    }

    /// DW-6.3's memory clause at the sink: an image refused by the
    /// **allocation** cap degrades exactly like one refused by the dimension
    /// cap — alt text, no graphics, no crash.
    ///
    /// The fixture is a perfectly valid, fully-decodable PNG whose dimensions
    /// are far inside `max_dim`; only its RGBA8 byte size exceeds the budget.
    /// Every other hostile fixture in this file is malformed or over-dimension,
    /// so this is the only one that reaches the sink through
    /// `Limits::accepts`'s allocation branch — the branch
    /// `crates/gfx/src/decode.rs` shows is unreachable at the default limits.
    #[test]
    fn test_dw_6_3_an_image_over_the_allocation_cap_degrades_to_alt_text() {
        let dir = scratch_dir("alloc-cap-degrade");
        write_png(&dir, "big.png");
        let doc = Document::parse("![too big to decode](./big.png)\n");
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        let reserved = Reserved {
            node_id,
            cols: 20,
            rows: 1,
            row: 0,
        };
        let rect = CellRect {
            x: 0,
            y: 0,
            width: 20,
            height: 1,
        };

        // The same fixture, same code path, one number apart. Ample budget
        // first, so the assertion below is about the cap and not about the
        // fixture being unpaintable for some other reason.
        let mut ample = GfxMediaSink::new(doc.clone(), &dir).with_limits(gfx::Limits {
            max_dim: 8_192,
            max_alloc: 256 * 1024 * 1024,
        });
        let mut out_ample = Vec::new();
        ample.begin_frame(&mut out_ample);
        ample.paint(&reserved, rect, &mut out_ample);
        assert_eq!(
            creates(&out_ample),
            1,
            "with an ample allocation budget this fixture must paint as graphics"
        );

        let mut capped = GfxMediaSink::new(doc, &dir).with_limits(gfx::Limits {
            max_dim: 8_192,
            max_alloc: 8,
        });
        let mut out = Vec::new();
        capped.begin_frame(&mut out);
        capped.paint(&reserved, rect, &mut out);
        let text = String::from_utf8_lossy(&out);
        assert_eq!(
            creates(&out),
            0,
            "the allocation cap must refuse the decode"
        );
        assert!(
            !text.contains("\x1b_G"),
            "no graphics escape may reach the wire for a refused image: {text:?}"
        );
        assert!(
            text.contains("too big to decode"),
            "the reader must get the alt text instead of a blank box: {text:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Reassembles the PNG a `transmit` put on the wire and reports its pixel
    /// size, by concatenating every `a=t`/`m=` chunk's base64 payload and
    /// reading the IHDR back out.
    ///
    /// The wire is the only honest place to read this. `Placement.rendered_px`
    /// is the size the sink *asked* for, and a sink that stopped scaling would
    /// still record it.
    fn transmitted_raster_px(buf: &[u8]) -> Option<(u32, u32)> {
        use base64::Engine as _;
        let text = String::from_utf8_lossy(buf);
        let mut payload = String::new();
        for command in text.split("\x1b_G").skip(1) {
            // Not `?` on any of these: a `\x1b_G` command that is not a
            // transmit chunk (a put, a delete) simply has no payload — an
            // early return here would make the whole function answer `None`
            // for a wire that does carry a raster, just later in the buffer.
            let Some(body) = command.split("\x1b\\").next() else {
                continue;
            };
            let Some((keys, chunk)) = body.split_once(';') else {
                continue;
            };
            if !(keys.contains("a=t") || keys.starts_with("m=")) {
                continue;
            }
            payload.push_str(chunk);
        }
        if payload.is_empty() {
            return None;
        }
        let png = base64::engine::general_purpose::STANDARD
            .decode(payload.as_bytes())
            .ok()?;
        png_pixel_size(&png)
    }

    /// The `c=`/`r=` (cell box) of every `a=p` put in `buf`.
    fn placed_cell_boxes(buf: &[u8]) -> Vec<(u16, u16)> {
        let text = String::from_utf8_lossy(buf);
        let mut out = Vec::new();
        for command in text.split("\x1b_G").skip(1) {
            let Some(body) = command.split("\x1b\\").next() else {
                continue;
            };
            let keys = body.split(';').next().unwrap_or("");
            if !keys.starts_with("a=p") {
                continue;
            }
            let get = |name: &str| {
                keys.split(',')
                    .find_map(|kv| kv.strip_prefix(name)?.parse::<u16>().ok())
            };
            if let (Some(c), Some(r)) = (get("c="), get("r=")) {
                out.push((c, r));
            }
        }
        out
    }

    /// DW-6.5: *"cell-size change re-rasterizes without re-layout."*
    ///
    /// Both halves are asserted on the bytes the terminal would receive, which
    /// is the only place they are distinguishable:
    ///
    /// - **re-rasterizes** — the transmitted PNG's own pixel dimensions change.
    /// - **without re-layout** — the placement still claims the *same* `c=`/`r=`
    ///   cell box, so the reader's text does not move.
    ///
    /// This replaces an `assert_eq!(reserved.rows, 2)` on a local the test
    /// built itself and never passed by `&mut`. That assertion could not fail:
    /// it re-read a value nothing in the call had access to. It was standing in
    /// for a structural argument — *"`GfxMediaSink` has no `Document`/
    /// `WidthEngine` handle, so it is incapable of re-laying-out"* — which is
    /// false on its own terms; the type holds both (`doc`, `width_engine`).
    /// The guarantee is real, but it rests on call-graph discipline, so it has
    /// to be asserted from the output rather than argued from the fields.
    ///
    /// The node is an **image**, and its formula twin is
    /// `test_a_formula_raster_is_exactly_its_cell_box_at_every_cell_geometry`.
    /// Both rasterize to the requested pixel target exactly — the image through
    /// `gfx::decode::decode_and_scale`, the formula through
    /// `math::render_fitted` plus `gfx::decode::letterbox_png` — so in both
    /// cases "the raster followed the new cell size" is a statement about a
    /// number on the wire.
    #[test]
    fn test_dw_6_5_geometry_change_regenerates_the_raster_and_keeps_the_cell_box() {
        let dir = scratch_dir("geometry");
        write_png(&dir, "a.png");
        let doc = Document::parse("![alt](./a.png)\n");
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        let mut sink = GfxMediaSink::new(doc, &dir);
        let reserved = Reserved {
            node_id,
            cols: 20,
            rows: 2,
            row: 0,
        };
        let rect = CellRect {
            x: 0,
            y: 0,
            width: 20,
            height: 2,
        };

        let mut out1 = Vec::new();
        sink.begin_frame(&mut out1);
        sink.paint(&reserved, rect, &mut out1);
        assert_eq!(
            creates(&out1),
            1,
            "first paint must transmit a fresh raster"
        );
        let raster_before =
            transmitted_raster_px(&out1).expect("the first paint must put a PNG on the wire");
        let box_before = placed_cell_boxes(&out1);
        assert_eq!(box_before, vec![(20, 2)], "cell box on the first paint");

        // Repainting at the SAME geometry must reuse the raster: no new
        // transmit, only a re-place.
        let mut out_same = Vec::new();
        sink.begin_frame(&mut out_same);
        sink.paint(&reserved, rect, &mut out_same);
        assert_eq!(
            creates(&out_same),
            0,
            "unchanged geometry must not re-transmit"
        );
        assert_eq!(
            transmitted_raster_px(&out_same),
            None,
            "an unchanged geometry must put no raster on the wire at all"
        );
        assert_eq!(placed_cell_boxes(&out_same), box_before);

        // Cell-geometry change — what `terminal::query_cell_px` would report
        // after a font-size change.
        sink.set_cell_px((32, 64));
        let mut out2 = Vec::new();
        sink.begin_frame(&mut out2);
        sink.paint(&reserved, rect, &mut out2);
        assert_eq!(
            creates(&out2),
            1,
            "a geometry change must regenerate the raster"
        );
        let raster_after =
            transmitted_raster_px(&out2).expect("the regenerated raster must be transmitted");

        assert_eq!(
            raster_before,
            (20 * 24, 2 * 48),
            "the first raster must be the cell box at the fallback geometry"
        );
        assert_eq!(
            raster_after,
            (20 * 32, 2 * 64),
            "re-rasterize: the transmitted raster's pixel size must follow the \
             new cell geometry, not stay at {raster_before:?}"
        );
        assert_eq!(
            placed_cell_boxes(&out2),
            box_before,
            "no re-layout: the box must still claim exactly the cells layout \
             gave it. A different c=/r= here means the geometry change moved \
             the reader's text."
        );
    }

    /// **A formula on the wire is exactly its cell box, at every cell
    /// geometry** — the property that makes the kitty scale factor 1.0 and so
    /// makes stretching a formula structurally impossible rather than merely
    /// bounded.
    ///
    /// This test used to assert the opposite, and was right to:
    /// `math::render_fitted` rasterized at a fixed em and solved
    /// `ratex_render`'s single symmetric padding for the target's *aspect*.
    /// Symmetric padding only moves a raster's aspect toward 1:1, so it clamped
    /// at `math::PADDING` for any target wider than the ink and the raster
    /// stopped depending on the target at all — a 20x2 cell box is such a
    /// target at every plausible cell geometry, so a proportional cell change
    /// re-transmitted bit-identical pixels, and every box aspect the solve
    /// could not reach arrived on screen stretched by the residual (worst case
    /// over the shipped corpus: 1.47x vertically, on `x_1`).
    ///
    /// The sink now hands the formula's raster to `gfx::decode::letterbox_png`,
    /// the same letterbox an image takes, which centers it on a transparent
    /// canvas of exactly the target. So the assertions below are the ones the
    /// old version could not make: the raster *is* the target, and it tracks
    /// each axis of the cell geometry independently — the third case changes
    /// the cell's width alone.
    #[test]
    fn test_a_formula_raster_is_exactly_its_cell_box_at_every_cell_geometry() {
        let dir = scratch_dir("formula-geometry");
        let doc = Document::parse("$x+1$\n");
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Math { .. })))
            .unwrap()
            .id();
        let reserved = Reserved {
            node_id,
            cols: 20,
            rows: 2,
            row: 0,
        };
        let rect = CellRect {
            x: 0,
            y: 0,
            width: 20,
            height: 2,
        };
        let render_at = |cell_px: (u32, u32)| {
            let mut sink = GfxMediaSink::new(doc.clone(), &dir).with_cell_px(cell_px);
            let mut out = Vec::new();
            sink.begin_frame(&mut out);
            sink.paint(&reserved, rect, &mut out);
            (creates(&out), transmitted_raster_px(&out))
        };

        // Three cell geometries, including one that changes only the width:
        // the raster is the 20x2 cell box times the cell, every time.
        for cell in [(24u32, 48u32), (32, 64), (12, 48)] {
            let (creates_n, raster) = render_at(cell);
            assert_eq!(creates_n, 1, "cell {cell:?}: one raster on the wire");
            assert_eq!(
                raster,
                Some((20 * cell.0, 2 * cell.1)),
                "cell {cell:?}: a formula must be transmitted at exactly its cell \
                 box, or kitty stretches it by the difference"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The complement, and the reason the assertion above is about *pixels*
    /// rather than *bytes*: the raster target is exactly the cell box times
    /// the cell geometry, on both axes independently.
    ///
    /// Asserted for an image, whose `gfx::decode::decode_and_scale` rasterizes
    /// to the requested target exactly, so the number on the wire is the
    /// arithmetic under test. A formula now lands on its target exactly too
    /// (`test_a_formula_raster_is_exactly_its_cell_box_at_every_cell_geometry`),
    /// but by way of a second crate; the image is the shorter path from the
    /// arithmetic to the assertion.
    #[test]
    fn test_the_raster_target_is_the_cell_box_times_the_queried_cell_geometry() {
        let dir = scratch_dir("raster-target");
        write_png(&dir, "a.png");
        let doc = Document::parse("![alt](./a.png)\n");
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        let reserved = Reserved {
            node_id,
            cols: 10,
            rows: 3,
            row: 0,
        };
        let rect = CellRect {
            x: 0,
            y: 0,
            width: 10,
            height: 3,
        };
        // Deliberately non-square and non-proportional to the fallback, so a
        // transposed or single-axis cell geometry cannot produce this answer.
        for cell_px in [(24u32, 48u32), (10, 30), (48, 24)] {
            let mut sink = GfxMediaSink::new(doc.clone(), &dir).with_cell_px(cell_px);
            let mut out = Vec::new();
            sink.begin_frame(&mut out);
            sink.paint(&reserved, rect, &mut out);
            assert_eq!(
                transmitted_raster_px(&out),
                Some((10 * cell_px.0, 3 * cell_px.1)),
                "at {cell_px:?} the raster must be the 10x3 cell box in pixels"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A zero from a garbled reply must not reach `target_px`, where it makes
    /// every raster one pixel wide.
    #[test]
    fn test_set_cell_px_refuses_a_zero_axis() {
        let dir = scratch_dir("zero-cell-px");
        let doc = Document::parse("$x+1$\n");
        let mut sink = GfxMediaSink::new(doc, &dir);
        let before = sink.cell_px;
        sink.set_cell_px((0, 64));
        sink.set_cell_px((32, 0));
        sink.set_cell_px((0, 0));
        assert_eq!(sink.cell_px, before);
        sink.set_cell_px((32, 64));
        assert_eq!(sink.cell_px, (32, 64));
    }

    #[test]
    fn test_evict_trait_method_deletes_and_removes_tracking() {
        let dir = scratch_dir("evict-direct");
        write_png(&dir, "a.png");
        let doc = Document::parse("![alt](./a.png)\n");
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        let mut sink = GfxMediaSink::new(doc, &dir);
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
        let mut out = Vec::new();
        sink.paint(&reserved, rect, &mut out);
        assert_eq!(creates(&out), 1);
        sink.evict(node_id, &mut out);
        assert_eq!(deletes(&out), 1);
    }

    // --- Phase 6 gate-review regressions ---
    // The pre-existing frame tests all reset `rect.y` to 0 between frames,
    // which is exactly why the two bugs below shipped green.

    /// Scrolling UP moves every box's `y` DOWN (+1). The old code inferred a
    /// new frame from `y <= previous y`, so consecutive scroll-up frames
    /// merged: the box was never re-placed and the image drifted out of
    /// alignment with its text, permanently.
    #[test]
    fn test_scroll_up_increasing_y_still_starts_a_new_frame_and_replaces() {
        let dir = scratch_dir("scrollup");
        write_png(&dir, "a.png");
        let doc = Document::parse("![alt](./a.png)\n");
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        let mut sink = GfxMediaSink::new(doc, &dir);
        let reserved = Reserved {
            node_id,
            cols: 10,
            rows: 1,
            row: 0,
        };
        let at = |y: u16| CellRect {
            x: 0,
            y,
            width: 10,
            height: 1,
        };

        let mut f1 = Vec::new();
        sink.begin_frame(&mut f1);
        sink.paint(&reserved, at(5), &mut f1);
        assert_eq!(creates(&f1), 1);

        // Scroll up by one row: same box, strictly GREATER y.
        let mut f2 = Vec::new();
        sink.begin_frame(&mut f2);
        sink.paint(&reserved, at(6), &mut f2);
        let text = String::from_utf8_lossy(&f2);
        assert!(
            text.contains("a=p,i="),
            "a scroll-up frame must re-place the image at its new row, got {text:?}"
        );
        assert!(
            text.contains("\x1b[7;1H"),
            "the placement must be positioned at the NEW row (y=6 -> row 7), got {text:?}"
        );
    }

    /// Once a node scrolls entirely out of view, no `paint` ever fires for
    /// it again — so if the sweep only ran from `paint`, its placement was
    /// never taken down and the image stayed on the terminal forever.
    ///
    /// The sweep is the *unplace* (`d=i`), not the delete. It used to be
    /// asserted as a delete because residency and visibility shared a
    /// lifetime; keeping that assertion would now pin the very re-transmit
    /// DW-3.4 exists to stop, so this checks both halves of the split
    /// instead: the placement goes, the raster stays.
    #[test]
    fn test_placement_is_swept_on_media_free_frames_after_scrolling_off() {
        let dir = scratch_dir("sweep");
        write_png(&dir, "a.png");
        let doc = Document::parse("![alt](./a.png)\n");
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        let mut sink = GfxMediaSink::new(doc, &dir);
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

        let mut term = TerminalGfx::default();
        let mut f1 = Vec::new();
        sink.begin_frame(&mut f1);
        sink.paint(&reserved, rect, &mut f1);
        assert_eq!(creates(&f1), 1);
        assert_eq!(deletes(&f1), 0);
        term.apply(&f1);

        // The image scrolls off: subsequent frames contain no media at all,
        // so only `begin_frame` fires. The sweep must still reach it.
        let mut f2 = Vec::new();
        sink.begin_frame(&mut f2);
        sink.begin_frame(&mut f2);
        term.apply(&f2);
        assert!(
            term.visible.is_empty(),
            "a node scrolled fully out of view must stop being drawn"
        );
        assert_eq!(
            term.stored,
            std::collections::BTreeSet::from([nth_id(1)]),
            "...but the terminal must still hold its pixels, so coming back \
             costs a re-place and not a re-transmit"
        );
        assert_eq!(deletes(&f2), 0);
    }

    /// The crop is proportional to the raster's own pixel height, so it is
    /// correct for a raster that is *not* the target size. Every formula used
    /// to be one, back when `math::render_fitted` rasterized at a fixed em and
    /// only matched the target's aspect, and sizing the crop off the target
    /// mis-cropped each by the difference. Both producers now land on the
    /// target exactly, so the case is no longer *reached* in the product — the
    /// unit under test is the arithmetic, and it is asserted directly here
    /// rather than left to depend on that continuing to be true.
    #[test]
    fn test_crop_source_is_proportional_to_the_measured_raster_height() {
        // A 10-row box whose raster is 250 px tall: 25 px per cell row,
        // nothing to do with the 48 px cell the target was built from.
        let crop = crop_source((80, 250), 10, 4, 3).expect("a partial view must crop");
        assert_eq!(
            crop,
            gfx::SourceRect {
                x: 0,
                y: 100,
                width: 80,
                height: 75,
            }
        );
    }

    #[test]
    fn test_crop_source_is_none_when_the_whole_box_is_on_screen() {
        assert_eq!(crop_source((80, 240), 5, 0, 5), None);
        // A degenerate raster is never guessed at: kitty rejects an
        // out-of-range source rectangle outright, so an uncropped placement
        // is the safer degradation.
        assert_eq!(crop_source((0, 0), 5, 2, 3), None);
    }

    #[test]
    fn test_png_pixel_size_reads_the_ihdr_and_rejects_non_png_bytes() {
        let dir = scratch_dir("ihdr");
        let path = dir.join("m.png");
        let img = image::RgbaImage::from_pixel(37, 91, image::Rgba([0, 0, 0, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        std::fs::File::create(&path)
            .unwrap()
            .write_all(&bytes)
            .unwrap();
        assert_eq!(png_pixel_size(&bytes), Some((37, 91)));
        assert_eq!(png_pixel_size(b"not a png at all, truly"), None);
        assert_eq!(png_pixel_size(&[]), None);
    }

    /// Fallback text is clipped in display CELLS, not chars: double-width
    /// CJK must not overflow the reserved box (an overflow on the bottom
    /// viewport row wraps and scrolls the alt screen).
    #[test]
    fn test_fallback_text_clips_by_display_cells_not_chars() {
        let dir = scratch_dir("cjk");
        // No such file -> the alt-text rung, with double-width alt text.
        let doc = Document::parse("![日本語の代替テキスト](./missing.png)\n");
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        let mut sink = GfxMediaSink::new(doc, &dir);
        let reserved = Reserved {
            node_id,
            cols: 8,
            rows: 1,
            row: 0,
        };
        let rect = CellRect {
            x: 0,
            y: 0,
            width: 8,
            height: 1,
        };
        let mut out = Vec::new();
        sink.begin_frame(&mut out);
        sink.paint(&reserved, rect, &mut out);

        let engine = WidthEngine::new(width::WidthConfig::default());
        let painted = String::from_utf8_lossy(&out);
        // Strengthened alongside the box-origin CUP: assert the exact bytes
        // (position included), then measure the *glyphs* rather than the raw
        // stream, which now carries an escape sequence the width engine has
        // no business counting.
        let glyphs = painted
            .strip_prefix("\x1b[1;1H")
            .unwrap_or_else(|| panic!("fallback must be positioned at its rect: {painted:?}"));
        assert!(
            engine.display_width(glyphs) <= 8,
            "fallback text overflowed its 8-cell box: {glyphs:?} measures {}",
            engine.display_width(glyphs)
        );
        assert!(!glyphs.is_empty(), "some alt text should still render");
        assert!(
            glyphs.starts_with('日'),
            "the alt text must be clipped from its start, not shifted: {glyphs:?}"
        );
    }

    /// The seam's whole contract: the sink paints into the [`CellRect`] it is
    /// handed, not at wherever the caller's cursor happens to be. Nothing
    /// upstream can be trusted to have parked the cursor on the box —
    /// `Painter::paint_inline_box` blanks the box by *writing* spaces, so it
    /// leaves the cursor a full box-width to the right, and a box indented
    /// by a blockquote or list gutter never starts at column 0 at all.
    #[test]
    fn test_text_fallback_is_written_at_its_rect_origin_not_the_current_cursor() {
        let dir = scratch_dir("rect-origin");
        // No such file, so the alt-text rung runs with no placement in play.
        let doc = Document::parse("![alt here](./missing.png)\n");
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        let mut sink = GfxMediaSink::new(doc, &dir);
        let mut out = Vec::new();
        sink.begin_frame(&mut out);
        sink.paint(
            &Reserved {
                node_id,
                cols: 12,
                rows: 1,
                row: 0,
            },
            CellRect {
                x: 7,
                y: 4,
                width: 12,
                height: 1,
            },
            &mut out,
        );
        // CUP is 1-based: cell (x=7, y=4) is row 5, column 8.
        assert_eq!(
            String::from_utf8_lossy(&out),
            "\x1b[5;8Halt here",
            "the fallback must be preceded by a CUP to its own rect origin"
        );
    }

    /// Every row of a multi-row text fallback re-homes to the box's column,
    /// not just the first: a txm grid indented into a gutter would otherwise
    /// stagger, each row starting where the previous one ended.
    #[test]
    fn test_every_row_of_a_multi_row_text_fallback_re_homes_to_the_box_column() {
        let dir = scratch_dir("multirow-origin");
        let doc = Document::parse("$x^2$\n");
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Math { .. })))
            .unwrap()
            .id();
        let mut sink = GfxMediaSink::new(doc, &dir);
        // RaTeX off, txm on: the Unicode-grid rung, which is the multi-row one.
        sink.force_ratex_failure_for_test(true);
        let grid = math::render_text("x^2").expect("txm must render x^2");
        assert!(
            grid.rows.len() >= 2,
            "this test needs a genuinely multi-row grid, got {:?}",
            grid.rows
        );

        let mut out = Vec::new();
        sink.begin_frame(&mut out);
        for (i, _) in grid.rows.iter().enumerate() {
            sink.paint(
                &Reserved {
                    node_id,
                    cols: 10,
                    rows: grid.rows.len() as u16,
                    row: i as u16,
                },
                CellRect {
                    x: 5,
                    y: 2 + i as u16,
                    width: 10,
                    height: (grid.rows.len() - i) as u16,
                },
                &mut out,
            );
        }

        let painted = String::from_utf8_lossy(&out);
        let mut expected = String::new();
        for (i, row) in grid.rows.iter().enumerate() {
            expected.push_str(&format!("\x1b[{};6H{}", 3 + i, sanitize(row)));
        }
        assert_eq!(
            painted, expected,
            "each grid row must be preceded by a CUP to column 6 (x=5) on its own row"
        );
    }

    /// The text rungs are subject to the same scroll-truncation rule as the
    /// raster: box row *k* shows grid line *k*, whether or not lines 0..k are
    /// above the viewport.
    ///
    /// A multi-row txm grid is a picture drawn in characters — a fraction bar
    /// over its denominator, a superscript raised above its baseline. Showing
    /// its first lines on the box's first *visible* row re-anchors the whole
    /// picture as you scroll: the exponent slides down onto the baseline and
    /// the formula reads as a different formula. This is the defect
    /// [`Reserved::row`] was added to eliminate, fixed for the raster path and
    /// left standing on the fallback path.
    ///
    /// No test in the workspace constructed a `Reserved` with `row > 0`
    /// before this one, which is why it survived: the scroll sweeps all use
    /// image fixtures, and images never reach this code.
    #[test]
    fn test_a_top_truncated_txm_grid_shows_the_grid_rows_it_scrolled_to() {
        let dir = scratch_dir("truncated-txm");
        let doc = Document::parse("$x^2$\n");
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Math { .. })))
            .unwrap()
            .id();
        let grid = math::render_text("x^2").expect("txm must render x^2");
        assert!(
            grid.rows.len() >= 2,
            "this test needs a genuinely multi-row grid, got {:?}",
            grid.rows
        );
        let rows = grid.rows.len() as u16;

        // Scroll the box up one line at a time: its top `skipped` rows are
        // above the viewport, and its remaining rows start at screen row 0.
        for skipped in 1..rows {
            let mut sink = GfxMediaSink::new(doc.clone(), &dir);
            sink.force_ratex_failure_for_test(true);
            let mut out = Vec::new();
            sink.begin_frame(&mut out);
            for (screen_row, box_row) in (skipped..rows).enumerate() {
                sink.paint(
                    &Reserved {
                        node_id,
                        cols: 10,
                        rows,
                        row: box_row,
                    },
                    CellRect {
                        x: 0,
                        y: screen_row as u16,
                        width: 10,
                        height: rows - box_row,
                    },
                    &mut out,
                );
            }
            let mut expected = String::new();
            for (screen_row, box_row) in (skipped..rows).enumerate() {
                expected.push_str(&format!(
                    "\x1b[{};1H{}",
                    screen_row + 1,
                    sanitize(&grid.rows[box_row as usize])
                ));
            }
            assert_eq!(
                String::from_utf8_lossy(&out),
                expected,
                "with the box's top {skipped} row(s) scrolled off, screen row 1 must \
                 show grid line {skipped}, not grid line 0"
            );
        }
    }

    /// The complement, and the reason this is not one blanket rule: a
    /// *single-line* rung has one line and one chance to be seen. Indexing it
    /// by box row would make image alt text and the literal-TeX rung vanish
    /// entirely the moment the box is top-truncated — a strictly worse
    /// outcome than showing it one row low, and precisely the degraded path a
    /// user hits when an image will not decode.
    #[test]
    fn test_a_top_truncated_single_line_rung_still_shows_its_text() {
        let dir = scratch_dir("truncated-alt");
        // No such file: the alt-text rung, on a box three rows tall.
        let doc = Document::parse("![alt here](./missing.png)\n");
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        let mut sink = GfxMediaSink::new(doc, &dir);
        let mut out = Vec::new();
        sink.begin_frame(&mut out);
        // Box rows 0 and 1 are above the viewport; only row 2 is on screen.
        sink.paint(
            &Reserved {
                node_id,
                cols: 12,
                rows: 3,
                row: 2,
            },
            CellRect {
                x: 0,
                y: 0,
                width: 12,
                height: 1,
            },
            &mut out,
        );
        assert_eq!(
            String::from_utf8_lossy(&out),
            "\x1b[1;1Halt here",
            "a one-line rung must stay anchored to the box's first VISIBLE row"
        );
    }

    /// The complement of the cap test in `resolve_image`: a *formula* denied
    /// a placement must fall through to the txm rung rather than rasterize
    /// something it cannot put on screen.
    #[test]
    fn test_over_cap_math_box_falls_through_to_its_text_rung() {
        let dir = scratch_dir("cap-math");
        for i in 0..CAP as u32 {
            write_png(&dir, &format!("pic{i}.png"));
        }
        let mut source = String::new();
        for i in 0..CAP as u32 {
            source.push_str(&format!("![alt{i}](./pic{i}.png)\n\n"));
        }
        source.push_str("$a+b$\n");
        let doc = Document::parse(&source);
        let images: Vec<NodeId> = doc
            .nodes()
            .filter(
                |n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })),
            )
            .map(|n| n.id())
            .collect();
        let math_node = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Math { .. })))
            .unwrap()
            .id();
        let mut sink = GfxMediaSink::new(doc, &dir);

        let mut out = Vec::new();
        sink.begin_frame(&mut out);
        for (row, &node_id) in images.iter().enumerate() {
            sink.paint(
                &Reserved {
                    node_id,
                    cols: 10,
                    rows: 1,
                    row: 0,
                },
                CellRect {
                    x: 0,
                    y: row as u16,
                    width: 10,
                    height: 1,
                },
                &mut out,
            );
        }
        assert_eq!(creates(&out), CAP, "the cap is full of live placements");

        let mut math_out = Vec::new();
        sink.paint(
            &Reserved {
                node_id: math_node,
                cols: 10,
                rows: 1,
                row: 0,
            },
            CellRect {
                x: 3,
                y: 33,
                width: 10,
                height: 1,
            },
            &mut math_out,
        );
        assert_eq!(
            creates(&math_out),
            0,
            "no raster may be transmitted for a box that cannot be placed"
        );
        assert_eq!(deletes(&math_out), 0, "and nothing already painted evicted");
        let painted = String::from_utf8_lossy(&math_out);
        let glyphs = painted
            .strip_prefix("\x1b[34;4H")
            .unwrap_or_else(|| panic!("the txm rung must land in the box: {painted:?}"));
        assert!(
            glyphs.contains('+'),
            "the formula must degrade to visible glyphs, got {glyphs:?}"
        );
    }

    /// DW-2.6: the sink shares the caller's allocation instead of copying the
    /// AST into one of its own. `strong_count` is the oracle a type signature
    /// cannot be — a field declared `Rc<Document>` still copies if the
    /// constructor calls `Rc::new` on a clone.
    #[test]
    fn test_dw_2_6_the_sink_holds_the_caller_s_rc_not_a_copy() {
        let doc = std::rc::Rc::new(Document::parse("![alt](./a.png)\n\n$x+1$\n"));
        assert_eq!(std::rc::Rc::strong_count(&doc), 1);

        let sink = GfxMediaSink::new(std::rc::Rc::clone(&doc), ".");
        assert_eq!(std::rc::Rc::strong_count(&doc), 2);

        drop(sink);
        assert_eq!(std::rc::Rc::strong_count(&doc), 1);
    }

    /// The convenience overload must produce a sink that behaves like the
    /// sharing one — a bare `Document` is moved into a fresh `Rc`, and its
    /// nodes stay resolvable through it.
    ///
    /// Asserted on painted bytes rather than on "the document is non-empty",
    /// which was the earlier form of this test and which a sink holding an
    /// entirely different document would also have satisfied. Move-vs-copy
    /// itself is a memory property with no behavioural signature; the
    /// `strong_count` test above is where that claim is actually made.
    #[test]
    fn test_a_bare_document_argument_produces_the_same_sink_as_a_shared_one() {
        let dir = scratch_dir("bare-doc-equivalence");
        write_png(&dir, "a.png");
        let source = "![alt text](./a.png)\n";

        let paint = |sink: &mut GfxMediaSink| {
            let node_id = sink
                .doc
                .nodes()
                .find(|n| {
                    matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. }))
                })
                .map(|n| n.id())
                .expect("the fixture has an image node");
            let mut out = Vec::new();
            sink.begin_frame(&mut out);
            sink.paint(
                &Reserved {
                    node_id,
                    cols: 4,
                    rows: 1,
                    row: 0,
                },
                CellRect {
                    x: 0,
                    y: 0,
                    width: 4,
                    height: 1,
                },
                &mut out,
            );
            out
        };

        let mut from_bare = GfxMediaSink::new(Document::parse(source), &dir);
        let mut from_shared = GfxMediaSink::new(std::rc::Rc::new(Document::parse(source)), &dir);

        let bare_bytes = paint(&mut from_bare);
        assert!(
            !bare_bytes.is_empty(),
            "the moved document's node must actually resolve and paint"
        );
        assert_eq!(
            commands(&bare_bytes).len(),
            commands(&paint(&mut from_shared)).len(),
            "both constructor forms must drive the same protocol commands"
        );
    }

    /// A `--watch` reload renumbers every node, so the sink must swap
    /// documents **and** take its images off the terminal. Asserted on the
    /// modelled terminal state, not on a delete count: a delete that named
    /// the wrong id would still count.
    #[test]
    fn test_a_reload_takes_every_image_off_the_terminal_and_swaps_the_document() {
        let dir = scratch_dir("reload-clears");
        write_png(&dir, "r.png");
        let doc = std::rc::Rc::new(Document::parse("![alt](./r.png)\n"));
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .map(|n| n.id())
            .unwrap();
        let mut sink = GfxMediaSink::new(std::rc::Rc::clone(&doc), &dir);

        let mut screen = TerminalGfx::default();
        let mut out = Vec::new();
        sink.begin_frame(&mut out);
        sink.paint(
            &Reserved {
                node_id,
                cols: 4,
                rows: 1,
                row: 0,
            },
            CellRect {
                x: 0,
                y: 0,
                width: 4,
                height: 1,
            },
            &mut out,
        );
        let placed = screen.apply(&out);
        assert!(!placed.is_empty(), "the setup must really place an image");
        assert!(!screen.stored.is_empty());

        let replacement = std::rc::Rc::new(Document::parse("# no media here\n"));
        let mut reload_out = Vec::new();
        sink.reload_document(std::rc::Rc::clone(&replacement), &mut reload_out);
        screen.apply(&reload_out);

        assert!(
            screen.visible.is_empty(),
            "images from the old document must leave the screen: {screen:?}"
        );
        assert!(
            screen.stored.is_empty(),
            "their rasters must be freed too, or they leak for the session: {screen:?}"
        );
        assert!(
            std::rc::Rc::ptr_eq(&sink.doc, &replacement),
            "the sink must resolve nodes against the reloaded document"
        );
        // And the old `Rc` is no longer held by the sink.
        assert_eq!(std::rc::Rc::strong_count(&doc), 1);
    }

    /// The reload case the test above cannot see, and the one the residency
    /// split creates.
    ///
    /// That test places an image and reloads immediately, so at the moment of
    /// the swap the node is both **placed** and **resident** — a sweep that
    /// walked only the on-screen set would pass it. Since Phase 3 those are
    /// different sets, and the bigger one is the hazard: a raster deliberately
    /// outlives its placement, so a node the reader scrolled past is resident
    /// and invisible, holding pixels keyed to a document that is about to stop
    /// existing. Nothing on screen would show it; the next document to be
    /// numbered the same way would inherit it.
    ///
    /// Scrolls the image off first, so the reload lands on exactly that state.
    #[test]
    fn test_a_reload_frees_a_raster_that_outlived_its_placement() {
        let dir = scratch_dir("reload-unplaced-raster");
        write_png(&dir, "off.png");
        let doc = std::rc::Rc::new(Document::parse("![alt](./off.png)\n"));
        let node_id = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .map(|n| n.id())
            .unwrap();
        let mut sink = GfxMediaSink::new(std::rc::Rc::clone(&doc), &dir);
        let mut screen = TerminalGfx::default();

        // Frame 1: on screen.
        let mut f1 = Vec::new();
        sink.begin_frame(&mut f1);
        sink.paint(
            &Reserved {
                node_id,
                cols: 4,
                rows: 1,
                row: 0,
            },
            CellRect {
                x: 0,
                y: 0,
                width: 4,
                height: 1,
            },
            &mut f1,
        );
        screen.apply(&f1);

        // Frame 2: scrolled off. The placement goes, the raster stays — that
        // is the whole of DW-3.4, and it is what makes this state reachable.
        let mut f2 = Vec::new();
        sink.begin_frame(&mut f2);
        screen.apply(&f2);
        assert!(
            screen.visible.is_empty(),
            "the box is off screen, so nothing may be drawn"
        );
        assert_eq!(
            screen.stored.len(),
            1,
            "...and its raster must still be resident, or this test is \
             asserting nothing"
        );
        assert_eq!(sink.resident_count(), 1);

        let replacement = std::rc::Rc::new(Document::parse("# an entirely new document\n"));
        let mut reload_out = Vec::new();
        sink.reload_document(std::rc::Rc::clone(&replacement), &mut reload_out);
        screen.apply(&reload_out);

        assert!(
            screen.stored.is_empty(),
            "a raster keyed to a node of the old document must be freed even \
             though nothing was drawing it: {screen:?}"
        );
        assert_eq!(sink.resident_count(), 0);
        assert_eq!(sink.resident_bytes(), 0, "the byte budget must be refunded");
    }

    /// The other half of the same hazard: after a reload, a node of the *new*
    /// document must never be handed the previous document's pixels.
    ///
    /// `NodeId` is a dense index, so the first image node of the replacement
    /// document is very often numbered exactly as the old one was — which is
    /// what makes "the cache still has an entry for node 7" a wrong picture on
    /// screen rather than a leak. Asserted on the wire: the paint after the
    /// reload must transmit, and must transmit under an id the terminal is not
    /// already holding.
    #[test]
    fn test_after_a_reload_a_node_never_inherits_the_previous_documents_pixels() {
        let dir = scratch_dir("reload-no-inherit");
        write_png(&dir, "before.png");
        write_png(&dir, "after.png");
        let doc = std::rc::Rc::new(Document::parse("![alt](./before.png)\n"));
        let old_node = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .map(|n| n.id())
            .unwrap();
        let mut sink = GfxMediaSink::new(std::rc::Rc::clone(&doc), &dir);
        let rect = CellRect {
            x: 0,
            y: 0,
            width: 4,
            height: 1,
        };
        let reserved = |id: NodeId| Reserved {
            node_id: id,
            cols: 4,
            rows: 1,
            row: 0,
        };

        let mut screen = TerminalGfx::default();
        let mut f1 = Vec::new();
        sink.begin_frame(&mut f1);
        sink.paint(&reserved(old_node), rect, &mut f1);
        screen.apply(&f1);
        let old_ids = screen.stored.clone();
        assert_eq!(old_ids.len(), 1);

        // A different document that happens to number its image node the same
        // way — the collision this guards against, made certain rather than
        // hoped for.
        let replacement = std::rc::Rc::new(Document::parse("![alt](./after.png)\n"));
        let new_node = replacement
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .map(|n| n.id())
            .unwrap();
        assert_eq!(
            new_node, old_node,
            "the fixture must reuse the same NodeId, or the collision this \
             test is about never happens"
        );

        let mut reload_out = Vec::new();
        sink.reload_document(std::rc::Rc::clone(&replacement), &mut reload_out);
        screen.apply(&reload_out);

        let mut f2 = Vec::new();
        sink.begin_frame(&mut f2);
        sink.paint(&reserved(new_node), rect, &mut f2);
        let commands_after = commands(&f2);
        screen.apply(&f2);

        assert_eq!(
            commands_after
                .iter()
                .filter(|&&(kind, _)| kind == 't')
                .count(),
            1,
            "the new document's node must transmit its own pixels, not re-place \
             the old one's: {commands_after:?}"
        );
        let placed: Vec<u32> = commands_after
            .iter()
            .filter(|&&(kind, _)| kind == 'p')
            .map(|&(_, id)| id)
            .collect();
        assert_eq!(placed.len(), 1);
        assert!(
            !old_ids.contains(&placed[0]),
            "the placement reused image id {}, which named the previous \
             document's raster",
            placed[0]
        );
    }
}
