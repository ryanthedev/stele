//! Measures what a formula's raster actually comes out as, versus the cell
//! box stele reserves for it.
//!
//! The question this answers: `sizer` picks a cell box from an em baseline,
//! while `sink` renders by handing `math::render` the box's *total pixel
//! height* — but that argument is the em/`font_size`, not a total. This prints
//! both sides so the mismatch is a measured number rather than an inference.
//!
//! Pass a cell geometry to see another terminal's answer:
//! `cargo run -p math --example raster_probe -- 12 28`
//!
//! Run: `cargo run -p math --example raster_probe`

/// Mirrors `stele::terminal::FALLBACK_CELL_PX`, used when no geometry is given
/// on the command line. stele asks the terminal with `CSI 16t` and only falls
/// back to this pair when nothing answers, so a live session may well be
/// measuring something else.
const FALLBACK_CELL_PX: (u32, u32) = (24, 48);

/// Mirrors `stele::media::sizer::math_baseline_px`: the em baseline **is** the
/// cell height, so one em is one row on every terminal.
///
/// It used to be a hardcoded 40 px here and there, and the two numbers were
/// the same only by coincidence of the fallback cell. That is exactly the
/// mismatch this probe was written to measure, so taking the cell as the
/// argument rather than restating a constant is the point.
fn math_baseline_px(cell_px: (u32, u32)) -> u32 {
    cell_px.1
}

/// Reads a PNG's IHDR width/height. The signature is 8 bytes, then an
/// 8-byte chunk header, then width and height as big-endian u32s.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let w = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
    let h = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
    Some((w, h))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let cell_px = match (
        args.next().and_then(|a| a.parse().ok()),
        args.next().and_then(|a| a.parse().ok()),
    ) {
        (Some(w), Some(h)) if w > 0 && h > 0 => (w, h),
        _ => FALLBACK_CELL_PX,
    };
    println!("cell geometry: {}x{}px", cell_px.0, cell_px.1);

    let cases = [
        ("inline (the reported bug)", r"e^{i\pi}+1=0"),
        (
            "display (renders fine today)",
            r"\int_0^\infty e^{-x^2} dx = \frac{\sqrt\pi}{2}",
        ),
        ("plain, no superscript", r"a+b=c"),
    ];

    for (label, tex) in cases {
        println!("\n=== {label} ===\n  tex: {tex}");
        let Some((em_w, em_h)) = math::intrinsic_em_size(tex) else {
            println!("  intrinsic_em_size declined");
            continue;
        };
        println!("  intrinsic em size: {em_w:.3}w x {em_h:.3}h");

        // --- what the sizer reserves -------------------------------------
        let baseline = math_baseline_px(cell_px);
        let px_w = (em_w * f64::from(baseline)).max(0.0) as u32;
        let px_h = (em_h * f64::from(baseline)).max(0.0) as u32;
        let cols = px_w.div_ceil(cell_px.0).max(1);
        let rows = px_h.div_ceil(cell_px.1).max(1);
        let box_px = (cols * cell_px.0, rows * cell_px.1);
        println!(
            "  sizer wants  : {px_w}x{px_h}px -> {cols}x{rows} cells = {}x{}px box",
            box_px.0, box_px.1
        );

        // --- what the sink actually renders ------------------------------
        // sink.rs: target_px() = (width_cells * cell_w, rows * cell_h), and
        // target.1 is passed to math::render as its `px_height` argument.
        let em_passed = baseline;
        match math::render_fitted(tex, em_passed, box_px.0, box_px.1) {
            Ok(png) => {
                let (rw, rh) = png_size(png.as_bytes()).expect("valid PNG header");
                println!("  sink renders : em={em_passed}px -> raster {rw}x{rh}px");
                let sx = rw as f64 / box_px.0 as f64;
                let sy = rh as f64 / box_px.1 as f64;
                println!("  scale into box: {sx:.2}x horizontal, {sy:.2}x vertical");
                println!(
                    "  aspect: raster {:.3}, box {:.3}  -> distortion {:.1}%",
                    rw as f64 / rh as f64,
                    box_px.0 as f64 / box_px.1 as f64,
                    ((rw as f64 / rh as f64) / (box_px.0 as f64 / box_px.1 as f64) - 1.0).abs()
                        * 100.0
                );
                // How many raster pixels survive per cell after downscaling.
                println!(
                    "  effective vertical resolution in the box: {:.0}px of raster -> {}px of box",
                    rh, box_px.1
                );
            }
            Err(e) => println!("  render failed: {e:?}"),
        }
    }
}
