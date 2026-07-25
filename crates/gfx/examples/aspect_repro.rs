//! Repro: does `decode_and_scale` preserve the source image's aspect ratio
//! when it fills a cell-quantized target box?
//!
//! The kitty protocol stretches a placement to fill its `c=` x `r=` cell box
//! exactly, so the raster's proportions ARE the on-screen proportions. This
//! walks the real `testdocs/img` fixtures through the real sizer/layout math
//! and reports source aspect vs. rastered aspect.
//!
//! Run: `cargo run -p gfx --example aspect_repro`

const ASSUMED_CELL_PX: (u32, u32) = (24, 48);
const MAX_RESERVED_COLS: u16 = 200;
const MAX_RESERVED_ROWS: u16 = 200;

/// Mirror of `stele::media::sizer::px_to_cells`.
fn px_to_cells(px_w: u32, px_h: u32) -> (u16, u16) {
    let cols = px_w.div_ceil(ASSUMED_CELL_PX.0);
    let rows = px_h.div_ceil(ASSUMED_CELL_PX.1);
    (
        cols.min(MAX_RESERVED_COLS as u32).max(1) as u16,
        rows.min(MAX_RESERVED_ROWS as u32).max(1) as u16,
    )
}

/// Mirror of `layout::block::emit_box`'s scale-to-content-width.
fn fit_to_width(cols: u16, rows: u16, cw: u16) -> (u16, u16) {
    if cols <= cw {
        (cols.max(1), rows.max(1))
    } else {
        let rows = (u32::from(rows.max(1)) * u32::from(cw)).div_ceil(u32::from(cols));
        (cw, rows.clamp(1, u32::from(u16::MAX)) as u16)
    }
}

fn png_dims(bytes: &[u8]) -> (u32, u32) {
    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    (w, h)
}

/// Bounding-box extent of the non-transparent pixels — the *ink*, which is
/// what the viewer actually judges. Identical to the raster extent when the
/// image is stretched to fill; smaller when it is letterboxed.
fn ink_dims(bytes: &[u8]) -> (u32, u32) {
    let img = image::load_from_memory(bytes).unwrap().to_rgba8();
    let (mut min_x, mut min_y) = (u32::MAX, u32::MAX);
    let (mut max_x, mut max_y) = (0u32, 0u32);
    for (x, y, px) in img.enumerate_pixels() {
        if px.0[3] > 0 {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    if min_x == u32::MAX {
        return (0, 0);
    }
    (max_x - min_x + 1, max_y - min_y + 1)
}

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("testdocs/img");
    let content_width: u16 = std::env::args()
        .nth(1)
        .and_then(|w| w.parse().ok())
        .unwrap_or(100);

    println!(
        "{:<16} {:>11} {:>9} {:>13} {:>13} {:>9} {:>9} {:>8}",
        "file", "source px", "aspect", "raster px", "ink px", "ink aspect", "box", "stretch"
    );
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("testdocs/img")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    files.sort();
    for path in files {
        let Ok((sw, sh)) = gfx::decode::probe_dimensions(&path, gfx::Limits::default()) else {
            continue;
        };
        let (cols, rows) = px_to_cells(sw, sh);
        let (cols, rows) = fit_to_width(cols, rows, content_width);
        let target = (
            u32::from(cols) * ASSUMED_CELL_PX.0,
            u32::from(rows) * ASSUMED_CELL_PX.1,
        );
        let decoded =
            gfx::decode::decode_and_scale(&path, target, gfx::Limits::default()).expect("decodes");
        let (rw, rh) = png_dims(&decoded.png);
        let (iw, ih) = ink_dims(&decoded.png);
        let src_aspect = f64::from(sw) / f64::from(sh);
        let ink_aspect = f64::from(iw) / f64::from(ih);
        let stretch = (ink_aspect / src_aspect - 1.0) * 100.0;
        println!(
            "{:<16} {:>5}x{:<5} {:>9.3} {:>6}x{:<6} {:>6}x{:<6} {:>10.3} {:>4}x{:<4} {:>+7.1}%",
            path.file_name().unwrap().to_string_lossy(),
            sw,
            sh,
            src_aspect,
            rw,
            rh,
            iw,
            ih,
            ink_aspect,
            cols,
            rows,
            stretch
        );
    }
}
