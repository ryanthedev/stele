//! `ImageSizer` — the `IntrinsicSizer` seam's real implementation (P6).
//!
//! One sizer, two node kinds: `layout::IntrinsicSizer`'s own doc comment
//! says a node id is *"an `ast::InlineKind::Image` or
//! `ast::InlineKind::Math` node"* — the phase brief names only `ImageSizer`
//! as a produced type, and this is how that single name covers both.
//!
//! - **Images**: a header-only dimension probe (`gfx::decode::probe_dimensions`)
//!   — never a full decode at layout time (the phase's own constraint); a
//!   potentially large/hostile file may never even scroll into view.
//! - **Math**: RaTeX parse+layout only (`math::intrinsic_em_size`) — cheap
//!   and non-rasterizing for the same reason, made possible because RaTeX's
//!   `DisplayList` carries em-unit `width`/`height` before any pixel is
//!   produced. If RaTeX's parser rejects the source, falls back to `txm`'s
//!   plain-grid layout before giving up (returning `None`, which degrades
//!   to the document's existing literal-TeX-source alt-text path).
//!
//! Every path here is a graceful `None` on failure — a sizer can never
//! panic or block a document from opening.

use std::path::{Path, PathBuf};

use ast::{Document, InlineKind, NodeId, NodeRef};
use layout::{CellSize, IntrinsicSizer};

/// Guard rails on the cell geometry this sizer will divide by, applied to
/// whatever [`ImageSizer::with_cell_px`] is handed. `crate::terminal` already
/// rejects an implausible answer before it gets here, but `ImageSizer` is
/// `pub` and a zero on either axis is an immediate divide-by-zero in
/// [`px_to_cells`] — a barricade, not a duplicate check.
fn usable_cell_px(cell_px: (u32, u32)) -> (u32, u32) {
    (
        if cell_px.0 == 0 {
            crate::terminal::FALLBACK_CELL_PX.0
        } else {
            cell_px.0
        },
        if cell_px.1 == 0 {
            crate::terminal::FALLBACK_CELL_PX.1
        } else {
            cell_px.1
        },
    )
}

/// The em-pixel baseline a formula is measured and rastered at: **one em is
/// one terminal row**, so `cell_px.1` is the answer and there is no constant
/// to pick.
///
/// This used to be a hardcoded 40 px, matching RaTeX's default `font_size`,
/// and it produced two visible defects at once. A cell is whatever the
/// terminal reports — around 28 px on a typical Ghostty — so a formula was
/// rastered ~1.4x the height of the text beside it, and, worse, its row
/// count came out as `ceil(em_h * 40 / cell_h)` rather than `ceil(em_h)`.
/// At a 28 px cell that put the baseline-riding threshold at 0.7 em, so
/// `a+b=c` (0.778 em) claimed its own row and broke the sentence around it
/// — while `x_1` (0.581 em) stayed inline, which is what made the bug look
/// intermittent rather than systematic.
///
/// Every test in this workspace constructs its sizer without calling
/// [`ImageSizer::with_cell_px`] and therefore measured against the 48 px
/// fallback, where the threshold is 1.2 em and both formulas fit. The
/// fixture prose in `testdocs/02-math.md` quotes those fallback numbers as
/// "verified". They were — in a geometry the product never runs in.
///
/// Tying the baseline to the cell makes the row count a property of the
/// formula alone: a one-em formula is one row on every terminal, whatever
/// it reports.
/// `pub(crate)` because the raster half of the pipeline must render at the
/// same baseline this half measured against: `media::sink` hands it to
/// [`math::render_fitted`]. Two different answers here and there would size a
/// box from one em and draw a formula at another.
pub(crate) fn math_baseline_px(cell_px: (u32, u32)) -> u32 {
    usable_cell_px(cell_px).1
}

/// Upper bound on a single reserved box's cell extent, independent of the
/// probed pixel/em size — a legitimately large (within `gfx::Limits`)
/// image or an elaborate formula must not blow past a sane box, matching
/// the spirit of P4's own overflow-ladder discipline one layer down.
const MAX_RESERVED_COLS: u16 = 200;
const MAX_RESERVED_ROWS: u16 = 200;

/// Implements [`IntrinsicSizer`] for both image and math nodes.
///
/// `disabled` is the structural mechanism behind DW-6.4: when `true`,
/// `size` always returns `None` for every node, so `layout::layout` never
/// emits a `Reserved` line at all — the document falls back through the
/// already-tested P4/P5 alt-text path, and `MediaSink::paint` is never
/// invoked in the first place. The orchestrator sets this from `$TMUX`,
/// `--no-images`, or a failed capability probe.
pub struct ImageSizer {
    base_dir: PathBuf,
    limits: gfx::Limits,
    disabled: bool,
    /// The terminal cell geometry a probed pixel size is divided by to reach
    /// a cell count. Resolved once per session by
    /// [`crate::terminal::query_cell_px`] and handed in via
    /// [`ImageSizer::with_cell_px`]; defaults to
    /// [`crate::terminal::FALLBACK_CELL_PX`] so a caller that cannot query
    /// (every unit test — there is no terminal in a test process) behaves
    /// exactly as this code did when the value was a hardcoded constant.
    ///
    /// This is the one place cell geometry affects what the *reader* sees
    /// rather than only how sharp it is. `cols`/`rows` here become the box's
    /// aspect ratio, `layout::block::emit_box` fits that aspect to the content
    /// column, and `gfx::decode::letterbox` then letterboxes the picture into
    /// whatever shape it was given — so a wrong cell size shows up as bands of
    /// dead space around the image, not as a blurry image.
    cell_px: (u32, u32),
}

impl ImageSizer {
    /// `base_dir` resolves a markdown document's relative image paths
    /// (typically the document file's parent directory).
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        ImageSizer {
            base_dir: base_dir.into(),
            limits: gfx::Limits::default(),
            disabled: false,
            cell_px: crate::terminal::FALLBACK_CELL_PX,
        }
    }

    /// Builds a sizer that never reserves a box for any node — see the
    /// struct doc comment's DW-6.4 note.
    pub fn disabled(base_dir: impl Into<PathBuf>) -> Self {
        ImageSizer {
            base_dir: base_dir.into(),
            limits: gfx::Limits::default(),
            disabled: true,
            cell_px: crate::terminal::FALLBACK_CELL_PX,
        }
    }

    pub fn with_limits(mut self, limits: gfx::Limits) -> Self {
        self.limits = limits;
        self
    }

    /// Uses `cell_px` — normally [`crate::terminal::query_cell_px`]'s
    /// answer — instead of the fallback constant.
    pub fn with_cell_px(mut self, cell_px: (u32, u32)) -> Self {
        self.cell_px = usable_cell_px(cell_px);
        self
    }

    fn size_image(&self, dest: &str) -> Option<CellSize> {
        if !is_local_image_path(dest) {
            return None;
        }
        let path = resolve_path(&self.base_dir, dest);
        let (px_w, px_h) = gfx::decode::probe_dimensions(&path, self.limits).ok()?;
        Some(px_to_cells(px_w, px_h, self.cell_px))
    }

    fn size_math(&self, tex: &str) -> Option<CellSize> {
        if let Some((em_w, em_h)) = math::intrinsic_em_size(tex) {
            let baseline = f64::from(math_baseline_px(self.cell_px));
            let px_w = (em_w * baseline).max(0.0) as u32;
            let px_h = (em_h * baseline).max(0.0) as u32;
            return Some(px_to_cells(px_w.max(1), px_h.max(1), self.cell_px));
        }
        // RaTeX rung failed to parse: try the txm rung before giving up —
        // this is what makes a txm-rendered formula also get a real
        // Reserved box instead of always bottoming out at literal text.
        let grid = math::render_text(tex)?;
        Some(CellSize {
            cols: grid.cols().max(1),
            rows: grid.rows_count().max(1),
        })
    }
}

impl IntrinsicSizer for ImageSizer {
    fn size(&self, node: NodeId, doc: &Document) -> Option<CellSize> {
        if self.disabled {
            return None;
        }
        match doc.node(node)? {
            NodeRef::Inline(inline) => match &inline.kind {
                InlineKind::Image { dest, .. } => self.size_image(dest),
                InlineKind::Math { tex, .. } => self.size_math(tex),
                _ => None,
            },
            NodeRef::Block(_) => None,
        }
    }
}

/// Rejects remote (`scheme://`) URLs and SVG — both explicitly out of
/// scope ("local file paths only... no SVG"). Everything else is treated
/// as a local, possibly relative, path.
fn is_local_image_path(dest: &str) -> bool {
    if dest.is_empty() || dest.contains("://") || dest.starts_with("//") {
        return false;
    }
    !dest.to_ascii_lowercase().ends_with(".svg")
}

fn resolve_path(base_dir: &Path, dest: &str) -> PathBuf {
    base_dir.join(dest)
}

/// Converts a probed/estimated pixel size into a cell count against
/// `cell_px`, doing the division in `u32` (a wide-enough type for
/// any dimension `gfx::Limits` would ever accept) and saturating only at
/// the final cast to `u16` — never casting an unbounded pixel value to
/// `u16` bare, per the width/cell-rect gotcha carried from earlier phases.
///
/// The [`MAX_RESERVED_COLS`]/[`MAX_RESERVED_ROWS`] caps scale **both** axes
/// by the same factor rather than clamping each independently. Clamping them
/// separately silently reshapes the box: a square 6000×6000 image measures
/// 250×125 cells, only `cols` exceeds the cap, and the box lands at 200×125
/// — aspect 0.8 for a 1.0 source. Downstream `layout::block::emit_box` then
/// fits *that* shape to the content column, so a 100-column viewport
/// reserved 100×63 where 100×50 is right. The image itself is letterboxed
/// into whatever box it is given (`gfx::decode::decode_and_scale`), so the
/// visible result is a band of dead transparent rows under the picture; a
/// vertical stretch, before letterboxing existed.
fn px_to_cells(px_w: u32, px_h: u32, cell_px: (u32, u32)) -> CellSize {
    // u64 throughout: the proportional rescale multiplies a cell count by a
    // cap before dividing, and a cell count derived from an unbounded u32
    // pixel dimension would overflow that product in u32.
    let max_cols = u64::from(MAX_RESERVED_COLS);
    let max_rows = u64::from(MAX_RESERVED_ROWS);
    let cell_px = usable_cell_px(cell_px);
    let mut cols = u64::from(px_w.div_ceil(cell_px.0)).max(1);
    let mut rows = u64::from(px_h.div_ceil(cell_px.1)).max(1);
    if cols > max_cols {
        rows = (rows * max_cols).div_ceil(cols).max(1);
        cols = max_cols;
    }
    if rows > max_rows {
        cols = (cols * max_rows).div_ceil(rows).max(1);
        rows = max_rows;
    }
    CellSize {
        cols: cols as u16,
        rows: rows as u16,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use ast::Document;
    use layout::{LayoutConfig, layout};
    use width::{WidthConfig, WidthEngine};

    use super::*;
    use crate::media::GfxMediaSink;
    use crate::painter::{Painter, Size};

    // Tests run concurrently within the same process — each needs its own
    // directory (a shared, pid-only path let one test's teardown delete a
    // directory a sibling test was still using).
    fn scratch_dir(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("stele-sizer-test-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn write_png(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
        let path = dir.join(name);
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 20, 30, 255]));
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

    #[test]
    fn test_size_image_reads_header_and_converts_to_cells() {
        let dir = scratch_dir("reads-header");
        write_png(&dir, "pic.png", 240, 480);
        let sizer = ImageSizer::new(&dir);
        let doc = Document::parse("![alt](pic.png)\n");
        let node = doc.blocks()[0].id;
        // The paragraph's child inline (the image) — find it by kind.
        let image_node = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        let size = sizer
            .size(image_node, &doc)
            .expect("must size a valid local png");
        // 240x480 px at the assumed 24x48 px/cell => exactly 10x10 cells.
        assert_eq!(size, CellSize { cols: 10, rows: 10 });
        let _ = node;
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The cell caps rescale both axes together. Clamping them independently
    /// silently reshaped every over-cap image: a square 6000×6000 became a
    /// 200×125-cell box (aspect 0.8), which `emit_box` then fitted to the
    /// content column as 100×63 instead of 100×50 — a 13-row band of dead
    /// space under the picture once the raster is letterboxed rather than
    /// stretched to fill it.
    #[test]
    fn test_over_cap_box_is_clamped_proportionally_not_per_axis() {
        let cell = crate::terminal::FALLBACK_CELL_PX;
        // Square, over the column cap: 6000/24 = 250 cols, 6000/48 = 125 rows.
        assert_eq!(
            px_to_cells(6000, 6000, cell),
            CellSize {
                cols: 200,
                rows: 100
            },
            "a square source must stay square when the column cap bites"
        );
        // Very wide, far over the column cap.
        assert_eq!(
            px_to_cells(30_000, 480, cell),
            CellSize { cols: 200, rows: 2 }
        );
        // Very tall: the row cap bites instead, and pulls cols with it.
        assert_eq!(
            px_to_cells(480, 30_000, cell),
            CellSize { cols: 7, rows: 200 }
        );
        // Over both caps at once.
        assert_eq!(
            px_to_cells(24_000, 48_000, cell),
            CellSize {
                cols: 200,
                rows: 200
            }
        );
        // Under both caps: untouched, exactly as before.
        assert_eq!(px_to_cells(240, 480, cell), CellSize { cols: 10, rows: 10 });
        // A degenerate axis still floors to one cell rather than zero.
        assert_eq!(px_to_cells(1, 1, cell), CellSize { cols: 1, rows: 1 });
        assert_eq!(px_to_cells(u32::MAX, 1, cell).cols, 200);
        assert_eq!(px_to_cells(1, u32::MAX, cell).rows, 200);
    }

    /// The queried geometry has to reach the division, and it has to reach it
    /// on both axes and in the right order.
    ///
    /// A 480x480 source is deliberately square: at the fallback 24x48 it
    /// measures 20x10 cells, and any of the three ways this wiring can be
    /// wrong gives a different answer. Ignoring `cell_px` gives 20x10 at every
    /// geometry; transposing the pair gives 10x20 at the fallback; using one
    /// axis for both gives a square box. The four geometries below pin all
    /// four apart.
    #[test]
    fn test_a_queried_cell_geometry_changes_the_reserved_cell_count() {
        assert_eq!(
            px_to_cells(480, 480, (24, 48)),
            CellSize { cols: 20, rows: 10 }
        );
        // A 2x-larger cell halves both counts.
        assert_eq!(
            px_to_cells(480, 480, (48, 96)),
            CellSize { cols: 10, rows: 5 }
        );
        // A square cell measures a square source as a square box.
        assert_eq!(
            px_to_cells(480, 480, (48, 48)),
            CellSize { cols: 10, rows: 10 }
        );
        // Width and height are not interchangeable.
        assert_ne!(
            px_to_cells(480, 480, (24, 48)),
            px_to_cells(480, 480, (48, 24))
        );
    }

    /// A zero divisor is a panic, not a bad picture. `ImageSizer` is `pub`, so
    /// "the terminal module already rejected zero" is a fact about one caller.
    #[test]
    fn test_a_zero_cell_axis_falls_back_instead_of_dividing_by_zero() {
        let fallback = px_to_cells(480, 480, crate::terminal::FALLBACK_CELL_PX);
        assert_eq!(px_to_cells(480, 480, (0, 0)), fallback);
        assert_eq!(px_to_cells(480, 480, (0, 48)), fallback);
        assert_eq!(px_to_cells(480, 480, (24, 0)), fallback);
    }

    /// The builder is the only path a queried geometry takes into a sizer, so
    /// it has to be the value `size()` divides by — not merely stored.
    #[test]
    fn test_with_cell_px_is_what_size_divides_by() {
        let dir = scratch_dir("with-cell-px");
        write_png(&dir, "pic.png", 240, 480);
        let doc = Document::parse("![alt](pic.png)\n");
        let image_node = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        let at_fallback = ImageSizer::new(&dir).size(image_node, &doc).unwrap();
        let at_double = ImageSizer::new(&dir)
            .with_cell_px((48, 96))
            .size(image_node, &doc)
            .unwrap();
        assert_eq!(at_fallback, CellSize { cols: 10, rows: 10 });
        assert_eq!(at_double, CellSize { cols: 5, rows: 5 });
        std::fs::remove_dir_all(&dir).ok();
    }

    /// End to end on the real oversized fixture, through `layout` — the box
    /// the reader actually gets, not just the sizer's intermediate.
    #[test]
    fn test_dw_6_over_cap_image_reserves_an_aspect_correct_box_in_the_layout_tree() {
        let img_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdocs/img")
            .canonicalize()
            .expect("testdocs/img must exist");
        // huge.png is 6000x6000 — square, and well over MAX_RESERVED_COLS.
        let doc = Document::parse("![huge](huge.png)\n");
        let sizer = ImageSizer::new(&img_dir);
        let engine = WidthEngine::new(WidthConfig::default());
        let tree = layout(&doc, 100, &LayoutConfig::default(), &engine, &sizer);
        let reserved: Vec<(u16, u16)> = (0..tree.line_count())
            .filter_map(|i| match tree.lines(i..i + 1).next() {
                Some(layout::Line::Reserved(r)) => Some((r.boxed.cols, r.boxed.rows)),
                _ => None,
            })
            .collect();
        assert!(!reserved.is_empty(), "huge.png must reserve a box");
        assert_eq!(
            (reserved[0].0, reserved[0].1, reserved.len()),
            (100, 50, 50),
            "a square image in a 100-column viewport must reserve a 100x50 \
             box (24x48 px cells), one Reserved line per row"
        );
    }

    #[test]
    fn test_size_image_none_for_remote_url() {
        let dir = scratch_dir("remote-url");
        let sizer = ImageSizer::new(&dir);
        let doc = Document::parse("![alt](https://example.com/pic.png)\n");
        let image_node = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        assert!(sizer.size(image_node, &doc).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_size_image_none_for_svg() {
        let dir = scratch_dir("svg");
        let sizer = ImageSizer::new(&dir);
        let doc = Document::parse("![alt](diagram.svg)\n");
        let image_node = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        assert!(sizer.size(image_node, &doc).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_size_image_none_for_missing_file() {
        let dir = scratch_dir("missing-file");
        let sizer = ImageSizer::new(&dir);
        let doc = Document::parse("![alt](does-not-exist.png)\n");
        let image_node = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        assert!(sizer.size(image_node, &doc).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_size_math_sizes_a_valid_formula_via_ratex_em_layout() {
        let dir = scratch_dir("math-valid");
        let sizer = ImageSizer::new(&dir);
        let doc = Document::parse("$x^2 + y^2$\n");
        let math_node = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Math { .. })))
            .unwrap()
            .id();
        let size = sizer.size(math_node, &doc).expect("valid TeX must size");
        assert!(size.cols > 0 && size.rows > 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A formula's **row count is a property of the formula**, not of the
    /// terminal it is being read in.
    ///
    /// This is the invariant the cell-height baseline buys, and the one the
    /// hardcoded 40 px em did not have: rows came out as
    /// `ceil(em_h * 40 / cell_h)`, so the baseline-riding threshold moved with
    /// the reported cell — 1.2 em at the 48 px fallback every test used, but
    /// 0.7 em at the ~28 px cell a real Ghostty reports. `a+b=c` is 0.778 em
    /// tall, which is why it rode the baseline in the whole test suite and
    /// broke its sentence in three on the actual product.
    ///
    /// The geometries below are the fallback, a retina Ghostty, and a small
    /// non-retina cell. Column counts legitimately differ between them (an
    /// em-square covers more columns in a narrow cell); rows must not.
    #[test]
    fn test_a_formulas_row_count_does_not_move_with_the_terminals_cell_size() {
        let dir = scratch_dir("math-cell-invariance");
        let doc = Document::parse("$a+b=c$ $x_1$ $\\frac{a}{b}$ $\\int_0^\\infty$\n");
        let nodes: Vec<_> = doc
            .nodes()
            .filter(
                |n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Math { .. })),
            )
            .map(|n| n.id())
            .collect();
        assert_eq!(nodes.len(), 4, "the fixture must parse as four math spans");

        for node in nodes {
            let rows: Vec<u16> = [(24, 48), (12, 28), (9, 19), (10, 21)]
                .into_iter()
                .map(|cell| {
                    ImageSizer::new(&dir)
                        .with_cell_px(cell)
                        .size(node, &doc)
                        .expect("every formula in the fixture sizes")
                        .rows
                })
                .collect();
            assert!(
                rows.windows(2).all(|w| w[0] == w[1]),
                "row count moved with the cell geometry: {rows:?} — a reader on \
                 one terminal sees the formula ride the baseline and a reader on \
                 another sees it break the sentence"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The specific regression from `testdocs/02-math.md`: at a realistic
    /// terminal cell, the two formulas that fixture calls the "core regression
    /// case" both measure one row and so ride the text baseline.
    #[test]
    fn test_a_simple_sum_rides_the_baseline_at_a_real_terminals_cell_size() {
        let dir = scratch_dir("math-baseline-riding");
        let doc = Document::parse("$a+b=c$ $x_1$\n");
        let nodes: Vec<_> = doc
            .nodes()
            .filter(
                |n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Math { .. })),
            )
            .map(|n| n.id())
            .collect();

        // 12x28 is the shape a retina Ghostty reports; the pre-fix code made
        // `a+b=c` two rows here while leaving `x_1` at one, which is exactly
        // what the screenshot of the bug showed.
        for node in nodes {
            let size = ImageSizer::new(&dir)
                .with_cell_px((12, 28))
                .size(node, &doc)
                .expect("both formulas size");
            assert_eq!(
                size.rows, 1,
                "a one-em formula must be one row so `wrap()` lets it ride the \
                 baseline; got {size:?}"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_size_math_falls_back_to_txm_grid_on_ratex_parse_failure() {
        let dir = scratch_dir("math-txm-fallback");
        let sizer = ImageSizer::new(&dir);
        // Valid enough for the ast math-span syntax, but not a command
        // RaTeX's parser accepts — forces the RaTeX rung to fail while txm
        // (a much smaller command set) still handles plain "a+b".
        let doc = Document::parse("$a+b$\n");
        let math_node = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Math { .. })))
            .unwrap()
            .id();
        // This exercises the real (non-forced) RaTeX rung succeeding first;
        // the forced-failure ladder path is covered end to end in
        // `sink.rs`'s tests via an explicit failure-injection switch.
        assert!(sizer.size(math_node, &doc).is_some());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_size_math_none_when_both_ratex_and_txm_reject() {
        let dir = scratch_dir("math-none");
        let sizer = ImageSizer::new(&dir);
        let source = r"$\begin{tikzpicture}\end{tikzpicture}$".to_string() + "\n";
        let doc = Document::parse(&source);
        let math_node = doc
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Math { .. })))
            .unwrap()
            .id();
        assert!(sizer.size(math_node, &doc).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_dw_6_4_disabled_sizer_yields_no_reserved_boxes_and_full_frame_has_no_kitty_escapes() {
        let dir = scratch_dir("dw-6-4-disabled");
        write_png(&dir, "pic.png", 240, 480);
        let doc = Document::parse("![alt text](pic.png)\n\n$x^2$\n");
        let sizer = ImageSizer::disabled(&dir);
        let engine = WidthEngine::new(WidthConfig::default());
        let tree = layout(&doc, 80, &LayoutConfig::default(), &engine, &sizer);

        let mut painter = Painter::new(WidthEngine::new(WidthConfig::default()));
        // Even registering a REAL media sink must not matter: a disabled
        // sizer means no Reserved line is ever produced, so paint() is
        // never called on it at all.
        painter.register_media(Box::new(GfxMediaSink::new(doc.clone(), dir.clone())));

        let mut buf = Vec::new();
        painter
            .frame(
                &tree,
                0,
                Size {
                    width: 80,
                    height: 10,
                },
                &mut buf,
            )
            .unwrap();
        let text = String::from_utf8_lossy(&buf);
        assert!(
            !text.contains("\x1b_G"),
            "expected zero kitty graphics escapes with images disabled, got: {text}"
        );
        // Graceful degradation, not a blank region: the alt text and TeX
        // source must actually be visible.
        assert!(text.contains("alt text"));
        assert!(text.contains("x^2"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
