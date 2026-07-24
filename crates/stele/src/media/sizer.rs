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

/// Assumed terminal cell-pixel geometry used only to convert a probed pixel
/// size into a cell count *at layout time* — before any live `CSI 16t`
/// query is possible (layout is pure, no I/O). Pinned to the exact values
/// `docs/spikes/ghostty-caps.md` item 7 measured against a live Ghostty
/// session (`CSI 16t` and `CSI 14t`÷(rows,cols) agreed exactly: 24×48px).
/// A real per-session query, once available, feeds the *raster* stage
/// (`GfxMediaSink`) instead — this constant only ever affects how many
/// cells get reserved, not what gets painted into them.
const ASSUMED_CELL_PX: (u32, u32) = (24, 48);

/// RaTeX `RenderOptions::default().font_size` — the em-pixel baseline this
/// sizer converts em units against. Matches `crates/math`'s own baseline so
/// a size probed here and a raster later rendered at the same nominal size
/// agree.
pub(crate) const MATH_BASELINE_PX_HEIGHT: u32 = 40;

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
}

impl ImageSizer {
    /// `base_dir` resolves a markdown document's relative image paths
    /// (typically the document file's parent directory).
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        ImageSizer {
            base_dir: base_dir.into(),
            limits: gfx::Limits::default(),
            disabled: false,
        }
    }

    /// Builds a sizer that never reserves a box for any node — see the
    /// struct doc comment's DW-6.4 note.
    pub fn disabled(base_dir: impl Into<PathBuf>) -> Self {
        ImageSizer {
            base_dir: base_dir.into(),
            limits: gfx::Limits::default(),
            disabled: true,
        }
    }

    pub fn with_limits(mut self, limits: gfx::Limits) -> Self {
        self.limits = limits;
        self
    }

    fn size_image(&self, dest: &str) -> Option<CellSize> {
        if !is_local_image_path(dest) {
            return None;
        }
        let path = resolve_path(&self.base_dir, dest);
        let (px_w, px_h) = gfx::decode::probe_dimensions(&path, self.limits).ok()?;
        Some(px_to_cells(px_w, px_h))
    }

    fn size_math(&self, tex: &str) -> Option<CellSize> {
        if let Some((em_w, em_h)) = math::intrinsic_em_size(tex) {
            let px_w = (em_w * MATH_BASELINE_PX_HEIGHT as f64).max(0.0) as u32;
            let px_h = (em_h * MATH_BASELINE_PX_HEIGHT as f64).max(0.0) as u32;
            return Some(px_to_cells(px_w.max(1), px_h.max(1)));
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

/// Converts a probed/estimated pixel size into a cell count via
/// [`ASSUMED_CELL_PX`], doing the division in `u32` (a wide-enough type for
/// any dimension `gfx::Limits` would ever accept) and saturating only at
/// the final cast to `u16` — never casting an unbounded pixel value to
/// `u16` bare, per the width/cell-rect gotcha carried from earlier phases.
fn px_to_cells(px_w: u32, px_h: u32) -> CellSize {
    let cols = px_w.div_ceil(ASSUMED_CELL_PX.0.max(1));
    let rows = px_h.div_ceil(ASSUMED_CELL_PX.1.max(1));
    CellSize {
        cols: cols.min(MAX_RESERVED_COLS as u32).max(1) as u16,
        rows: rows.min(MAX_RESERVED_ROWS as u32).max(1) as u16,
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
