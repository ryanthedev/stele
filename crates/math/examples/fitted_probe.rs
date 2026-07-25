//! Probe: what does `render_fitted` actually rasterize for the hostile
//! formulas in `testdocs/02-math.md`, and does the result stay inside the
//! crate's own `MAX_PIXMAP_PX` budget?
//!
//! The raster reported here is the *math* raster — ink at the em
//! `em_capped_to_box` allows, at the ink's own aspect. It is not what goes on
//! the wire: `stele::media::sink` runs it through `gfx::decode::letterbox_png`
//! first, which centers it on a transparent canvas of exactly the cell box.
//!
//! Run: `cargo run -p math --example fitted_probe`

fn png_dims(bytes: &[u8]) -> (u32, u32) {
    (
        u32::from_be_bytes(bytes[16..20].try_into().unwrap()),
        u32::from_be_bytes(bytes[20..24].try_into().unwrap()),
    )
}

const MAX_PIXMAP_PX: u64 = 8_000_000;

fn main() {
    let doc = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("testdocs/02-math.md"),
    )
    .unwrap();
    // The longest formula, from the "absurdly long formula" section: 3423
    // characters between the `$$`, not the 4095 that section's prose claims.
    let long = doc
        .lines()
        .find(|l| l.starts_with("$$x + 1 + 1"))
        .expect("the long-formula line")
        .trim_start_matches("$$")
        .trim_end_matches("$$")
        .to_string();

    let cases: Vec<(&str, String)> = vec![
        ("empty", String::new()),
        ("space-only", "   ".to_string()),
        ("simple", "a+b=c".to_string()),
        (
            "tall cfrac",
            r"\cfrac{1}{1+\cfrac{1}{1+\cfrac{1}{1+\cfrac{1}{1+x}}}}".to_string(),
        ),
        ("3423-char", long),
    ];

    println!(
        "{:<12} {:>7} {:>9} {:>9} {:>14} {:>12} {:>7} {:>10} {:>9}",
        "case", "tex len", "em w", "em h", "raster", "megapixels", "budget", "png bytes", "render"
    );
    for (name, tex) in &cases {
        let em = math::intrinsic_em_size(tex);
        // The box the sizer+layout land on at a 100-column content width.
        let (target_w, target_h) = match em {
            Some((w, h)) => {
                let cols = (((w * 40.0) as u32).div_ceil(24)).clamp(1, 200).min(100);
                let rows = (((h * 40.0) as u32).div_ceil(48)).clamp(1, 200);
                (cols * 24, rows * 48)
            }
            None => (100 * 24, 48),
        };
        let started = std::time::Instant::now();
        let rendered = math::render_fitted(tex, 40, target_w, target_h);
        let elapsed = started.elapsed();
        match rendered {
            Ok(png) => {
                let (w, h) = png_dims(png.as_bytes());
                let mp = u64::from(w) * u64::from(h);
                println!(
                    "{:<12} {:>7} {:>9.2} {:>9.2} {:>7}x{:<6} {:>12.2} {:>7} {:>10} {:>8.0?}",
                    name,
                    tex.len(),
                    em.map(|e| e.0).unwrap_or(-1.0),
                    em.map(|e| e.1).unwrap_or(-1.0),
                    w,
                    h,
                    mp as f64 / 1e6,
                    if mp > MAX_PIXMAP_PX { "OVER" } else { "ok" },
                    png.as_bytes().len(),
                    elapsed
                );
            }
            Err(e) => println!(
                "{:<12} {:>7} {:>9.2} {:>9.2}  Err: {e}",
                name,
                tex.len(),
                em.map(|e| e.0).unwrap_or(-1.0),
                em.map(|e| e.1).unwrap_or(-1.0)
            ),
        }
    }
}
