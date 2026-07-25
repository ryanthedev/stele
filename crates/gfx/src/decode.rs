//! Bounded local image decode: a header-only dimension probe (no full pixel
//! decode at layout time), a full `Limits`-bounded decode+scale for painting,
//! and an in-memory [`letterbox_png`] for a raster some other crate produced
//! (a `crates/math` formula). All three share one decode and one letterbox, so
//! the bounds and the "exactly the target box" guarantee are the same property
//! stated once, not three promises that can drift apart.
//!
//! Every input here is hostile until proven otherwise (DW-6.3): a malformed
//! file, a truncated file, or a valid-looking header claiming a
//! decompression-bomb-scale dimension must all fail cleanly — never panic,
//! never allocate unboundedly. The `image` crate's own PNG/JPEG decoders
//! already call `Limits::check_dimensions` inside `set_limits` (verified by
//! reading `image-0.25.10`'s `codecs/png.rs` / `codecs/jpeg/decoder.rs`
//! source directly, not assumed), which is why `reader.limits(..)` is set
//! *before* `into_dimensions`/`decode` below — that ordering is what makes
//! the library reject a bomb header itself. This module's own
//! [`Limits::accepts`] check is kept anyway as defense-in-depth on a
//! hostile-input path (the cc-defensive-programming barricade rule:
//! validate again at security-critical boundaries even inside a barricade).

use std::io::Cursor;
use std::path::Path;

use image::ImageReader;

/// Bounds on what this crate will decode. Applied to both the header-only
/// probe and the full decode, so a bomb-dimension header is rejected before
/// anything downstream (a layout box reservation, a pixel buffer
/// allocation) is ever sized from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// No decoded (or header-reported) dimension may exceed this on either
    /// axis.
    pub max_dim: u32,
    /// Upper bound on the decoded pixel buffer's byte size
    /// (`width * height * 4` for RGBA8).
    pub max_alloc: u64,
}

impl Default for Limits {
    fn default() -> Self {
        // Generous for any real terminal-viewport-sized image; even at the
        // cap a decode allocates at most ~256 MiB (RGBA8, before scaling
        // down to the much smaller cell-pixel target).
        Limits {
            max_dim: 8_192,
            max_alloc: 256 * 1024 * 1024,
        }
    }
}

impl Limits {
    fn to_image_limits(self) -> image::Limits {
        // `image::Limits` is `#[non_exhaustive]` — build from `default()`
        // and set fields rather than a struct literal.
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(self.max_dim);
        limits.max_image_height = Some(self.max_dim);
        limits.max_alloc = Some(self.max_alloc);
        limits
    }

    /// True if `(width, height)` fits within `max_dim` on both axes and the
    /// RGBA8-decoded byte size fits within `max_alloc`. Zero on either axis
    /// is rejected too (never a valid image to lay out or paint).
    fn accepts(self, width: u32, height: u32) -> bool {
        if width == 0 || height == 0 || width > self.max_dim || height > self.max_dim {
            return false;
        }
        let bytes = u64::from(width) * u64::from(height) * 4;
        bytes <= self.max_alloc
    }
}

/// Why decode or probing failed. Every variant is an anticipated, handled
/// outcome (hostile, missing, or oversized input) — never a panic.
#[derive(Debug)]
pub enum DecodeError {
    Io(std::io::Error),
    Malformed(String),
    ExceedsLimits { width: u32, height: u32 },
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Io(e) => write!(f, "could not read image file: {e}"),
            DecodeError::Malformed(msg) => write!(f, "malformed image: {msg}"),
            DecodeError::ExceedsLimits { width, height } => write!(
                f,
                "image dimensions {width}x{height} exceed the configured limits"
            ),
        }
    }
}

impl std::error::Error for DecodeError {}

/// A decoded image, scaled and letterboxed onto the caller's requested target
/// cell-pixel size, re-encoded as PNG.
///
/// The dimension fields are named for what they are — the **source file's**
/// dimensions, before any scaling — because the obvious reading of a bare
/// `width`/`height` next to a `png` field is "the raster's size", and that is
/// the one thing they are not. A caller that needs the transmitted raster's
/// pixel size must read it from `png` (or know it equals the `target_px` it
/// asked for).
#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub source_width: u32,
    pub source_height: u32,
    pub png: Vec<u8>,
}

fn opened(
    path: &Path,
    limits: Limits,
) -> Result<ImageReader<std::io::BufReader<std::fs::File>>, DecodeError> {
    let mut reader = ImageReader::open(path)
        .map_err(DecodeError::Io)?
        .with_guessed_format()
        .map_err(DecodeError::Io)?;
    reader.limits(limits.to_image_limits());
    Ok(reader)
}

/// The in-memory twin of [`opened`]. Same `limits`, applied the same way and
/// in the same order — before any pixel is decoded.
fn opened_bytes(bytes: &[u8], limits: Limits) -> Result<ImageReader<Cursor<&[u8]>>, DecodeError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(DecodeError::Io)?;
    reader.limits(limits.to_image_limits());
    Ok(reader)
}

/// Header-only dimension probe: reads just enough of `path` to learn its
/// pixel dimensions, without decoding any pixel data. Used by `ImageSizer`
/// at layout time, where a full decode of a potentially large/hostile image
/// would be wasted work (the image may never scroll into view) — this is
/// the "no full decode at layout time" the phase requires.
///
/// Rejects (returns `Err`, never panics) on: an unreadable path, an
/// unrecognized/malformed format, and a header that claims dimensions
/// `limits` would reject — a decompression-bomb *header* never gets a
/// chance to reserve a giant layout box.
pub fn probe_dimensions(path: &Path, limits: Limits) -> Result<(u32, u32), DecodeError> {
    let reader = opened(path, limits)?;
    let (width, height) = reader.into_dimensions().map_err(|e| match e {
        image::ImageError::Limits(_) => DecodeError::ExceedsLimits {
            width: 0,
            height: 0,
        },
        other => DecodeError::Malformed(other.to_string()),
    })?;
    if !limits.accepts(width, height) {
        return Err(DecodeError::ExceedsLimits { width, height });
    }
    Ok((width, height))
}

/// Full, `limits`-bounded decode of `path`, scaled to `target_px` (the
/// caller's resolved cell-pixel target) and re-encoded as PNG. Every
/// allocation this performs is bounded: `reader.limits(..)` is set before
/// decoding, so the underlying PNG/JPEG decoder itself rejects a
/// bomb-dimension header (verified against `image`'s own source; see the
/// module doc comment) before allocating a pixel buffer for it.
pub fn decode_and_scale(
    path: &Path,
    target_px: (u32, u32),
    limits: Limits,
) -> Result<DecodedImage, DecodeError> {
    let img = decode_within_limits(|| opened(path, limits), limits)?;
    let png = letterbox_onto(&img, target_px)?;
    Ok(DecodedImage {
        source_width: img.width(),
        source_height: img.height(),
        png,
    })
}

/// Letterboxes an **already-encoded** raster held in memory onto `target_px`,
/// returning a PNG that is exactly `target_px` with the source centered inside
/// it at its own aspect ratio.
///
/// This is the entry point `crates/math`'s rasters take. A formula comes out of
/// `math::render_fitted` at whatever size its em and its own ink demand, which
/// is not the cell box; kitty scales a placement to fill its `c=` x `r=` box
/// exactly, so anything that is not already the box arrives stretched. Sending
/// it through the same [`letterbox`] the image path uses makes the transmitted
/// raster equal to its box, which makes the stretch factor 1.0 by construction
/// rather than by a bound.
///
/// `limits` is honored exactly as on the file path, and for the same reason:
/// this seam does not get to assume its input is friendly just because the
/// bytes came from a sibling crate. Sharing the decode with
/// [`decode_and_scale`] is what makes that automatic rather than a promise.
pub fn letterbox_png(
    png: &[u8],
    target_px: (u32, u32),
    limits: Limits,
) -> Result<Vec<u8>, DecodeError> {
    let img = decode_within_limits(|| opened_bytes(png, limits), limits)?;
    letterbox_onto(&img, target_px)
}

/// Probe-then-decode under `limits`, shared by both entry points above.
///
/// `open` is called twice on purpose: `into_dimensions` consumes a reader, so
/// the header check and the pixel decode each need their own. The second one
/// re-applies the limits and the decoder re-checks them as it reads — the real
/// backstop against input whose body disagrees with its own header.
fn decode_within_limits<R, F>(open: F, limits: Limits) -> Result<image::DynamicImage, DecodeError>
where
    R: std::io::BufRead + std::io::Seek,
    F: Fn() -> Result<ImageReader<R>, DecodeError>,
{
    let (width, height) = open()?.into_dimensions().map_err(|e| match e {
        image::ImageError::Limits(_) => DecodeError::ExceedsLimits {
            width: 0,
            height: 0,
        },
        other => DecodeError::Malformed(other.to_string()),
    })?;
    if !limits.accepts(width, height) {
        return Err(DecodeError::ExceedsLimits { width, height });
    }
    open()?.decode().map_err(|e| match e {
        image::ImageError::Limits(_) => DecodeError::ExceedsLimits { width, height },
        other => DecodeError::Malformed(other.to_string()),
    })
}

/// [`letterbox`] plus the PNG re-encode — the tail both entry points share, so
/// there is exactly one place that decides what a transmitted raster's pixel
/// dimensions are.
fn letterbox_onto(
    img: &image::DynamicImage,
    target_px: (u32, u32),
) -> Result<Vec<u8>, DecodeError> {
    let (target_w, target_h) = (target_px.0.max(1), target_px.1.max(1));
    let letterboxed = letterbox(&img.to_rgba8(), target_w, target_h);
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(letterboxed)
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| DecodeError::Malformed(e.to_string()))?;
    Ok(png)
}

/// Scales `src` down/up to fit *inside* `target_w` x `target_h` preserving its
/// aspect ratio, then centers it on a fully transparent canvas of exactly
/// `target_w` x `target_h`.
///
/// **Why the padding is not optional.** The kitty graphics protocol scales a
/// placement to fill its `c=` x `r=` cell box *exactly*, so the raster's
/// proportions are the on-screen proportions. A cell box is a whole number of
/// cells and an image is not, so resizing straight onto the target — what this
/// used to do — stretches every image whose pixel dimensions don't happen to
/// land on the cell grid. Measured against the shipped fixtures
/// (`cargo run -p gfx --example aspect_repro`) that was a 40x24 icon rendering
/// as a square (-40%) and a 3000x100 banner rendering 80% too tall (-44%).
///
/// **A photo and a formula take this same code.** They used to only claim to:
/// `math::render_fitted` letterboxed by solving `ratex_render`'s single
/// symmetric `padding` for the box's aspect, and symmetric padding can only
/// move a raster's aspect *toward* 1:1 — so every box aspect outside the
/// interval between 1 and the ink's own was unreachable, and 8 of the 33
/// shipped fixture formulas missed their box by more than 10% (`x_1` reserved
/// 2 cols x 1 row = 48x48 px, rasterized 47x32, and landed on screen stretched
/// 1.47x vertically). It is now literally one implementation: `math` renders
/// ink at the right em and its own aspect, and [`letterbox_png`] puts that
/// raster on the box here.
///
/// The returned raster is *exactly* `target_w` x `target_h`, so every caller
/// that reasons about the raster's pixel geometry (placement sizing, and any
/// source-rect crop over it) is unaffected — and the kitty scale factor is
/// 1.0 on both axes for both kinds of content.
fn letterbox(src: &image::RgbaImage, target_w: u32, target_h: u32) -> image::RgbaImage {
    // Fit: the smaller of the two axis ratios, in a wide-enough type that a
    // 8192-px source against a 1-px target cannot round to zero.
    let (sw, sh) = (src.width().max(1), src.height().max(1));
    let scale = f64::from(target_w) / f64::from(sw);
    let scale = scale.min(f64::from(target_h) / f64::from(sh));
    let fit_w = ((f64::from(sw) * scale).round() as u32).clamp(1, target_w);
    let fit_h = ((f64::from(sh) * scale).round() as u32).clamp(1, target_h);

    let resized = image::imageops::resize(src, fit_w, fit_h, image::imageops::FilterType::Triangle);
    if fit_w == target_w && fit_h == target_h {
        // Already exactly on the box: no canvas copy, no wasted allocation.
        return resized;
    }
    // Transparent, not a colored box — the terminal's own background must show
    // through the margin, the same guarantee `crates/math` makes for formulas.
    let mut canvas = image::RgbaImage::from_pixel(target_w, target_h, image::Rgba([0, 0, 0, 0]));
    let (dx, dy) = ((target_w - fit_w) / 2, (target_h - fit_h) / 2);
    image::imageops::replace(&mut canvas, &resized, i64::from(dx), i64::from(dy));
    canvas
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn scratch_path(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "stele-gfx-decode-test-{name}-{}",
            std::process::id()
        ));
        p
    }

    fn write_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = scratch_path(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(bytes).unwrap();
        path
    }

    /// A real, valid, tiny (2x2) PNG — the positive-path fixture the
    /// hostile fixtures below are contrasted against.
    fn valid_tiny_png() -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        png
    }

    /// Hand-built PNG signature + `IHDR` claiming a 50000x50000 image (a
    /// decompression-bomb-scale dimension) with no further chunks — DW-6.3's
    /// "decompression-bomb dimensions" fixture, authored here rather than as
    /// a committed binary file (out of this phase's file scope).
    fn bomb_dimension_png() -> Vec<u8> {
        let mut bytes = vec![0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let width: u32 = 50_000;
        let height: u32 = 50_000;
        let mut ihdr_data = Vec::new();
        ihdr_data.extend_from_slice(&width.to_be_bytes());
        ihdr_data.extend_from_slice(&height.to_be_bytes());
        ihdr_data.extend_from_slice(&[8, 6, 0, 0, 0]); // bit depth 8, RGBA, no interlace
        let mut chunk = Vec::new();
        chunk.extend_from_slice(&(ihdr_data.len() as u32).to_be_bytes());
        chunk.extend_from_slice(b"IHDR");
        chunk.extend_from_slice(&ihdr_data);
        let crc = crc32(&chunk[4..]);
        chunk.extend_from_slice(&crc.to_be_bytes());
        bytes.extend_from_slice(&chunk);
        bytes
    }

    /// Minimal CRC-32 (PNG's exact polynomial) so the hand-built `IHDR`
    /// chunk above has a syntactically valid CRC and the `png` decoder
    /// parses far enough to reach the dimension check rather than bailing
    /// out on a checksum mismatch first.
    fn crc32(data: &[u8]) -> u32 {
        let mut crc: u32 = 0xFFFF_FFFF;
        for &byte in data {
            crc ^= byte as u32;
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        !crc
    }

    #[test]
    fn test_probe_dimensions_reads_a_valid_png_header_only() {
        let path = write_file("valid", &valid_tiny_png());
        let (w, h) = probe_dimensions(&path, Limits::default()).unwrap();
        assert_eq!((w, h), (2, 2));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_dw_6_3_malformed_png_header_rejected_no_panic() {
        let path = write_file("malformed", b"\x89PNGnotarealpngatall");
        let result = probe_dimensions(&path, Limits::default());
        assert!(result.is_err());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_dw_6_3_truncated_jpeg_rejected_no_panic() {
        // A real JPEG magic number (SOI + APP0) followed by nothing: a
        // truncated file that must fail cleanly, not panic mid-scan.
        let path = write_file("truncated", &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10]);
        let result = probe_dimensions(&path, Limits::default());
        assert!(result.is_err());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_dw_6_3_bomb_dimension_header_rejected_before_decode() {
        let path = write_file("bomb", &bomb_dimension_png());
        let limits = Limits::default(); // max_dim 8192 < the bomb's 50000
        let result = probe_dimensions(&path, limits);
        assert!(
            matches!(result, Err(DecodeError::ExceedsLimits { .. })),
            "expected ExceedsLimits, got {result:?}"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_dw_6_3_bomb_dimension_rejected_by_full_decode_too_not_just_probe() {
        let path = write_file("bomb-decode", &bomb_dimension_png());
        let result = decode_and_scale(&path, (24, 48), Limits::default());
        assert!(result.is_err(), "decode must also reject, not just probe");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_dw_6_3_oversized_but_otherwise_valid_png_rejected_by_limits() {
        // A genuinely valid, fully-decodable PNG whose real dimensions
        // exceed a caller-tightened Limits — proves the cap is enforced on
        // real content, not only on a hand-crafted malformed header.
        let img = image::RgbaImage::from_pixel(100, 100, image::Rgba([1, 2, 3, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let path = write_file("oversized-valid", &png);
        let tight = Limits {
            max_dim: 50,
            max_alloc: 256 * 1024 * 1024,
        };
        let result = probe_dimensions(&path, tight);
        assert!(matches!(result, Err(DecodeError::ExceedsLimits { .. })));
        std::fs::remove_file(&path).ok();
    }

    /// DW-6.3's *"memory stays under cap"*, half one: the **allocation** cap is
    /// enforced, and it is enforced independently of the dimension cap.
    ///
    /// Every other DW-6.3 fixture trips `max_dim`, so before this test
    /// `max_alloc` had never been the thing that refused an input — the check
    /// could have been deleted with the suite still green. Here both dimensions
    /// pass `max_dim` comfortably (100 < 8192) and only the RGBA8 byte size
    /// (100·100·4 = 40 000) exceeds the budget, so the only line that can
    /// reject this file is `Limits::accepts`'s `bytes <= self.max_alloc`.
    ///
    /// Asserted on both entry points: the layout-time probe and the paint-time
    /// full decode. The probe is the one that matters most — it is what runs on
    /// a document being opened, before anything has been allocated.
    #[test]
    fn test_dw_6_3_allocation_cap_rejects_an_image_the_dimension_cap_admits() {
        let img = image::RgbaImage::from_pixel(100, 100, image::Rgba([9, 9, 9, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let path = write_file("alloc-cap", &png);

        let tight_alloc = Limits {
            max_dim: 8_192,
            max_alloc: 20_000,
        };
        assert!(
            100 <= tight_alloc.max_dim,
            "the fixture must pass the dimension cap, or this tests max_dim again"
        );
        let probed = probe_dimensions(&path, tight_alloc);
        assert!(
            matches!(
                probed,
                Err(DecodeError::ExceedsLimits {
                    width: 100,
                    height: 100
                })
            ),
            "the allocation cap must reject a 100x100 RGBA8 image against a \
             20 000-byte budget, reporting the real dimensions; got {probed:?}"
        );
        let decoded = decode_and_scale(&path, (24, 48), tight_alloc);
        assert!(
            matches!(decoded, Err(DecodeError::ExceedsLimits { .. })),
            "the full decode must reject it too; got {decoded:?}"
        );

        // The same file, the same dimension cap, a budget one byte over what it
        // needs: accepted. Without this the test would also pass if `accepts`
        // rejected everything.
        let ample = Limits {
            max_dim: 8_192,
            max_alloc: 100 * 100 * 4,
        };
        assert_eq!(probe_dimensions(&path, ample).unwrap(), (100, 100));
        std::fs::remove_file(&path).ok();
    }

    /// DW-6.3's *"memory stays under cap"*, half two — and the half that is
    /// actually about memory rather than about one fixture.
    ///
    /// `Limits` has two caps and only one of them bounds a decode's peak
    /// allocation. The bound that matters is the *conjunction*: no input the
    /// dimension cap admits may be able to exceed the allocation cap. At the
    /// shipped defaults that holds exactly — 8192·8192·4 = 268 435 456 =
    /// 256 MiB = `max_alloc` — which is why no default-limits fixture can ever
    /// exercise `max_alloc`, and why the assertion above has to tighten it by
    /// hand.
    ///
    /// This is the assertion that fails if `max_dim` is ever raised on its own.
    /// Doubling it to 16 384 admits a 1 GiB decode against an unchanged 256 MiB
    /// budget, and nothing else in the suite would notice: every existing test
    /// would still pass, and the cap would silently stop capping.
    #[test]
    fn test_dw_6_3_no_input_the_dimension_cap_admits_can_exceed_the_allocation_cap() {
        let limits = Limits::default();
        let worst_case_bytes = u64::from(limits.max_dim) * u64::from(limits.max_dim) * 4;
        assert!(
            worst_case_bytes <= limits.max_alloc,
            "the largest image `max_dim` ({}) admits decodes to {worst_case_bytes} \
             bytes of RGBA8, which exceeds `max_alloc` ({}). Raise `max_alloc` \
             alongside `max_dim`, or the allocation cap is no longer a cap.",
            limits.max_dim,
            limits.max_alloc
        );
        // And the pair is tight, not merely satisfied by a wildly slack
        // `max_alloc` — the two caps are meant to describe the same ceiling
        // from two directions.
        assert!(
            worst_case_bytes * 2 > limits.max_alloc,
            "`max_alloc` is more than twice the worst case `max_dim` admits, so \
             it is documenting a ceiling nothing can reach"
        );
    }

    #[test]
    fn test_missing_file_rejected_cleanly() {
        let result = probe_dimensions(std::path::Path::new("/no/such/file.png"), Limits::default());
        assert!(matches!(result, Err(DecodeError::Io(_))));
    }

    #[test]
    fn test_decode_and_scale_produces_png_at_target_pixel_size() {
        let path = write_file("scale", &valid_tiny_png());
        let decoded = decode_and_scale(&path, (24, 48), Limits::default()).unwrap();
        assert_eq!((decoded.source_width, decoded.source_height), (2, 2));
        let (w, h) = probe_reencoded_dimensions(&decoded.png);
        assert_eq!((w, h), (24, 48));
        std::fs::remove_file(&path).ok();
    }

    fn probe_reencoded_dimensions(png: &[u8]) -> (u32, u32) {
        let img = image::load_from_memory(png).unwrap();
        (img.width(), img.height())
    }

    /// Regression: kitty stretches a placement to fill its `c=` x `r=` cell box
    /// exactly, so the raster's proportions are what the viewer sees. Filling
    /// the box by resizing straight onto it distorted every image whose pixel
    /// size didn't land on the cell grid — a 40x24 icon rendered square, a
    /// 3000x100 banner rendered 80% too tall.
    ///
    /// Asserting the raster is the target size (the test above) is exactly the
    /// assertion that let that ship: it is true both before and after. This
    /// asserts on the *ink*, which is the thing the viewer actually judges.
    #[test]
    fn test_decode_and_scale_letterboxes_instead_of_stretching_the_source() {
        // Source aspect / target box / expected ink extent inside the box.
        // Each case is a real `testdocs/img` geometry paired with the box
        // `ImageSizer` + `layout::block::emit_box` actually reserve for it.
        /// `(source px, target box px, expected ink extent inside the box)`.
        struct Case {
            source: (u32, u32),
            target: (u32, u32),
            ink: (u32, u32),
        }
        let case = |source, target, ink| Case {
            source,
            target,
            ink,
        };
        let cases = [
            // small.png: 40x24 into a 2x1 cell box (48x48 px) -> 48x29 ink.
            case((40, 24), (48, 48), (48, 29)),
            // wide.png: 3000x100 into a 100x3 cell box (2400x144 px).
            case((3000, 100), (2400, 144), (2400, 80)),
            // tall.png: 100x800 into a 5x17 cell box (120x816 px).
            case((100, 800), (120, 816), (102, 816)),
            // huge.png (square): 6000x6000 into the capped 100x63 box.
            case((6000, 6000), (2400, 3024), (2400, 2400)),
        ];
        for &Case {
            source: (sw, sh),
            target,
            ink: (ink_w, ink_h),
        } in &cases
        {
            let img = image::RgbaImage::from_pixel(sw, sh, image::Rgba([200, 30, 30, 255]));
            let mut bytes = Vec::new();
            image::DynamicImage::ImageRgba8(img)
                .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
                .unwrap();
            let path = write_file(&format!("letterbox-{sw}x{sh}"), &bytes);
            let decoded = decode_and_scale(&path, target, Limits::default()).unwrap();
            std::fs::remove_file(&path).ok();

            // The raster still exactly fills the reserved box, so placement
            // sizing (and any source-rect crop over it) is unchanged.
            assert_eq!(
                probe_reencoded_dimensions(&decoded.png),
                target,
                "{sw}x{sh} must still rasterize to exactly its {target:?} box"
            );

            // The ink inside it keeps the source's aspect ratio.
            let raster = image::load_from_memory(&decoded.png).unwrap().to_rgba8();
            let (measured_w, measured_h) = opaque_extent(&raster);
            assert_eq!(
                (measured_w, measured_h),
                (ink_w, ink_h),
                "{sw}x{sh} into {target:?}: ink {measured_w}x{measured_h}, expected {ink_w}x{ink_h}"
            );
            let src_aspect = f64::from(sw) / f64::from(sh);
            let ink_aspect = f64::from(measured_w) / f64::from(measured_h);
            assert!(
                (ink_aspect / src_aspect - 1.0).abs() < 0.05,
                "{sw}x{sh} into {target:?}: ink aspect {ink_aspect:.3} vs source {src_aspect:.3}"
            );
        }
    }

    /// Bounding-box extent of the non-transparent pixels — the letterboxed
    /// image's actual ink, ignoring the transparent margin around it.
    fn opaque_extent(img: &image::RgbaImage) -> (u32, u32) {
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

    /// [`letterbox_png`] is the entry point a `crates/math` formula takes, and
    /// it has to give the same two guarantees the file path does: the result is
    /// *exactly* the target, and the ink inside it keeps the source's aspect.
    ///
    /// The 40x24-into-48x48 case is the formula shape that motivated the whole
    /// change — `x_1` reserves a 2 col x 1 row box and rasterizes wider than it
    /// is tall, and kitty stretches whatever it is handed to fill the box. A
    /// resize-onto-the-target here would report ink of 48x48.
    #[test]
    fn test_letterbox_png_puts_an_in_memory_raster_exactly_on_the_target() {
        for (sw, sh, target, ink) in [
            (40u32, 24u32, (48u32, 48u32), (48u32, 29u32)),
            (47, 32, (48, 48), (48, 33)),
            (5904, 12, (2400, 48), (2400, 5)),
            (24, 48, (24, 48), (24, 48)),
        ] {
            let img = image::RgbaImage::from_pixel(sw, sh, image::Rgba([200, 30, 30, 255]));
            let mut bytes = Vec::new();
            image::DynamicImage::ImageRgba8(img)
                .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
                .unwrap();
            let out = letterbox_png(&bytes, target, Limits::default())
                .unwrap_or_else(|e| panic!("{sw}x{sh} -> {target:?}: {e}"));
            assert_eq!(
                probe_reencoded_dimensions(&out),
                target,
                "{sw}x{sh} must land exactly on {target:?}"
            );
            let raster = image::load_from_memory(&out).unwrap().to_rgba8();
            assert_eq!(
                opaque_extent(&raster),
                ink,
                "{sw}x{sh} into {target:?}: ink must keep the source's aspect"
            );
        }
    }

    /// The math raster is untrusted at this seam like anything else: the same
    /// `limits` the file path honors are honored here, on the same two caps.
    /// Sharing one decode with [`decode_and_scale`] is what makes that true
    /// without a second implementation to keep in step.
    #[test]
    fn test_letterbox_png_enforces_the_same_limits_as_the_file_path() {
        let img = image::RgbaImage::from_pixel(100, 100, image::Rgba([9, 9, 9, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();

        let tight_dim = Limits {
            max_dim: 50,
            max_alloc: 256 * 1024 * 1024,
        };
        // Reported as `{0, 0}`, exactly as on the file path: `image`'s own
        // `Limits` rejects inside `into_dimensions`, so this crate never learns
        // the real numbers — which is the point of setting the limits before
        // reading rather than after.
        assert!(matches!(
            letterbox_png(&bytes, (24, 48), tight_dim),
            Err(DecodeError::ExceedsLimits { .. })
        ));

        // The allocation cap is this crate's own check, past the decoder's, so
        // here the real dimensions do come back.
        let tight_alloc = Limits {
            max_dim: 8_192,
            max_alloc: 20_000,
        };
        assert!(matches!(
            letterbox_png(&bytes, (24, 48), tight_alloc),
            Err(DecodeError::ExceedsLimits {
                width: 100,
                height: 100
            })
        ));

        // And it is not simply refusing everything.
        assert!(letterbox_png(&bytes, (24, 48), Limits::default()).is_ok());
        // A bomb header never reaches a pixel buffer here either.
        assert!(letterbox_png(&bomb_dimension_png(), (24, 48), Limits::default()).is_err());
        // Nor does a raster that is not a raster.
        assert!(letterbox_png(b"\x89PNGnotarealpngatall", (24, 48), Limits::default()).is_err());
    }

    /// The letterbox margin must be genuinely transparent, not an opaque black
    /// bar — the terminal's own background has to show through it, the same
    /// guarantee `crates/math` makes for a formula's padding.
    #[test]
    fn test_letterbox_margin_is_transparent_not_an_opaque_bar() {
        let img = image::RgbaImage::from_pixel(40, 24, image::Rgba([200, 30, 30, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        let path = write_file("letterbox-alpha", &bytes);
        let decoded = decode_and_scale(&path, (48, 48), Limits::default()).unwrap();
        std::fs::remove_file(&path).ok();
        let raster = image::load_from_memory(&decoded.png).unwrap().to_rgba8();
        assert_eq!(
            raster.get_pixel(0, 0).0[3],
            0,
            "top-left margin pixel must be fully transparent"
        );
        assert_eq!(raster.get_pixel(0, 47).0[3], 0);
    }
}
