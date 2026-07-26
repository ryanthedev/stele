//! Measures the cost of `decode::decode_and_scale` on a large source image at
//! the *real* cell-pixel target a full-width box asks for (~2400 px), and
//! compares the single-stage Triangle resize against a two-stage
//! halve-then-Triangle.
//!
//! The plan's assumption for Phase 3 was that a two-stage downscale still helps
//! at the real target and not only at the 500x400 case the original audit
//! measured. That is a claim about wall-clock time on this machine, so it is
//! measured here rather than argued: run
//!
//! ```text
//! cargo run --release -p gfx --example downscale_probe
//! ```
//!
//! and read the two timings. Whatever ships in `decode.rs` is whichever one
//! this says is faster — and if the difference is inside the noise, the
//! two-stage step does not ship at all.

use std::time::Instant;

fn main() {
    let source = build_source(6000, 6000);
    let targets = [(2400u32, 2400u32), (1000, 800), (500, 400)];

    println!("source: 6000x6000 RGBA, {} PNG bytes", source.len());
    for target in targets {
        let one = time(|| {
            let img = decode(&source);
            let _ = std::hint::black_box(one_stage(&img.to_rgba8(), target.0, target.1));
        });
        let two = time(|| {
            let img = decode(&source);
            let _ = std::hint::black_box(two_stage(&img.to_rgba8(), target.0, target.1));
        });
        let decode_only = time(|| {
            let _ = std::hint::black_box(decode(&source));
        });
        println!(
            "target {}x{}: decode {:?} | decode+one-stage {:?} | decode+two-stage {:?}",
            target.0, target.1, decode_only, one, two
        );
    }
}

fn time(mut f: impl FnMut()) -> std::time::Duration {
    // Three runs, best of: this is a coarse A/B, and the minimum is the least
    // scheduler-contaminated estimate of the work itself.
    let mut best = std::time::Duration::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        f();
        best = best.min(start.elapsed());
    }
    best
}

fn decode(png: &[u8]) -> image::DynamicImage {
    image::ImageReader::new(std::io::Cursor::new(png))
        .with_guessed_format()
        .expect("format")
        .decode()
        .expect("decode")
}

/// What `decode::letterbox` does today: one Triangle resize straight onto the
/// fitted size.
fn one_stage(src: &image::RgbaImage, target_w: u32, target_h: u32) -> image::RgbaImage {
    let (fit_w, fit_h) = fit(src, target_w, target_h);
    image::imageops::resize(src, fit_w, fit_h, image::imageops::FilterType::Triangle)
}

/// Repeated exact halving (a box filter over 2x2 blocks, which is what
/// `resize`-to-half with Triangle degenerates to, but without the general
/// filter machinery) down to within 2x of the target, then one Triangle resize
/// onto the fitted size.
fn two_stage(src: &image::RgbaImage, target_w: u32, target_h: u32) -> image::RgbaImage {
    let (fit_w, fit_h) = fit(src, target_w, target_h);
    let mut current = std::borrow::Cow::Borrowed(src);
    while current.width() / 2 >= fit_w.max(1) && current.height() / 2 >= fit_h.max(1) {
        let halved = halve(&current);
        current = std::borrow::Cow::Owned(halved);
    }
    image::imageops::resize(
        current.as_ref(),
        fit_w,
        fit_h,
        image::imageops::FilterType::Triangle,
    )
}

fn halve(src: &image::RgbaImage) -> image::RgbaImage {
    let (w, h) = (src.width() / 2, src.height() / 2);
    image::RgbaImage::from_fn(w.max(1), h.max(1), |x, y| {
        let (sx, sy) = (x * 2, y * 2);
        let mut acc = [0u32; 4];
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let p = src.get_pixel(
                (sx + dx).min(src.width() - 1),
                (sy + dy).min(src.height() - 1),
            );
            for (channel, sum) in acc.iter_mut().enumerate() {
                *sum += u32::from(p.0[channel]);
            }
        }
        image::Rgba([
            (acc[0] / 4) as u8,
            (acc[1] / 4) as u8,
            (acc[2] / 4) as u8,
            (acc[3] / 4) as u8,
        ])
    })
}

fn fit(src: &image::RgbaImage, target_w: u32, target_h: u32) -> (u32, u32) {
    let (sw, sh) = (src.width().max(1), src.height().max(1));
    let scale = (f64::from(target_w) / f64::from(sw)).min(f64::from(target_h) / f64::from(sh));
    (
        ((f64::from(sw) * scale).round() as u32).clamp(1, target_w),
        ((f64::from(sh) * scale).round() as u32).clamp(1, target_h),
    )
}

/// A deterministic non-uniform source: a flat colour would compress and decode
/// far faster than any real photo and would flatter the decode stage.
fn build_source(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(w, h, |x, y| {
        let n = (x ^ y) as u8;
        image::Rgba([n, n.wrapping_mul(3), n.wrapping_add(x as u8), 255])
    });
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("encode");
    png
}
