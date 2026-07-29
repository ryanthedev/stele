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
//! never allocate unboundedly. Every codec this crate enables calls
//! `Limits::check_dimensions` inside `set_limits`, which is why
//! `reader.limits(..)` is set *before* `into_dimensions`/`decode` below —
//! that ordering is what makes the library reject a bomb header itself.
//!
//! That claim is verified against `image-0.25.10`'s own source rather than
//! assumed, and it holds two different ways. PNG, JPEG, GIF and TIFF override
//! `set_limits` and call `check_dimensions` themselves (`codecs/png.rs:55`,
//! `codecs/jpeg/decoder.rs:171`, `codecs/gif.rs:106`, `codecs/tiff.rs:353`).
//! WebP, BMP and ICO do not override it at all, and inherit the
//! `ImageDecoder` trait's default — which checks dimensions too
//! (`io/decoder.rs:118`). Either way the header is refused before a pixel
//! buffer is sized from it. `test_a_bomb_header_is_refused_in_every_format`
//! is what stops that going stale when a codec is added or `image` is bumped.
//!
//! This module's own [`Limits::accepts`] check is kept anyway as
//! defense-in-depth on a hostile-input path (the cc-defensive-programming
//! barricade rule: validate again at security-critical boundaries even
//! inside a barricade).

use std::io::Cursor;
use std::path::Path;

use image::ImageReader;

use crate::svg;

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
    ///
    /// Crate-visible rather than private so [`crate::svg`] answers to the same
    /// arithmetic as the raster path. A vector drawing's declared canvas is
    /// its header, and it earns the same refusal.
    pub(crate) fn accepts(self, width: u32, height: u32) -> bool {
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
    ExceedsLimits {
        width: u32,
        height: u32,
    },
    /// The path exists but is not a regular file — a directory, a FIFO, a
    /// socket, a character or block device. Settled by `stat`, before
    /// anything opens it.
    NotAFile(std::path::PathBuf),
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
            DecodeError::NotAFile(path) => {
                write!(f, "not a regular file: {}", path.display())
            }
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

/// Opens `path` as an image reader — **after** settling, by `stat`, that it
/// is a regular file.
///
/// The type check lives here rather than in the callers, and that placement
/// is the whole point. `ImageReader::open` is the only place in this
/// workspace that turns a document-supplied path into an open file, and a
/// document-supplied path is untrusted: `![x](pipe.png)` where `pipe.png` is
/// a FIFO with no writer makes `open(2)` block forever. Both of this
/// module's entry points reach it, both of `crates/stele`'s media call sites
/// reach those, and every one of them runs inside the event loop with the
/// terminal in raw mode — where Ctrl-C is an ordinary keystroke, not a
/// signal, so a blocked `open` leaves a viewer that cannot be quit from the
/// keyboard at all. It was reproduced exactly that way: 801 of 801 stack
/// samples inside `open`, process killed from outside.
///
/// The same defect had already been found three times on the *document*
/// paths (`Navigator::back`, the `--watch` reload, and the link target
/// itself), each time fixed at a call site. Fixing it at the call sites once
/// more would leave the next caller of this module exposed — so it is fixed
/// at the one function that owns "a path becomes pixels", which is what
/// makes a fifth instance impossible rather than merely fixing the two that
/// are visible today.
///
/// `stat` rather than an opened-descriptor `fstat`, because by the time a
/// descriptor exists the block has already happened. That leaves the same
/// narrow race the document barricade documents — a path swapped between the
/// `stat` and the `open` — and it is bounded the same way: an attacker who
/// can win it can already write into the document's directory at the instant
/// of a repaint.
fn opened(
    path: &Path,
    limits: Limits,
) -> Result<ImageReader<std::io::BufReader<std::fs::File>>, DecodeError> {
    ensure_regular_file(path)?;
    let mut reader = ImageReader::open(path)
        .map_err(DecodeError::Io)?
        .with_guessed_format()
        .map_err(DecodeError::Io)?;
    reader.limits(limits.to_image_limits());
    Ok(reader)
}

/// The `stat` half of [`opened`]'s barricade, on its own so the SVG path
/// answers to it too.
///
/// Extracted rather than duplicated, and that is not tidiness. [`opened`]'s
/// doc explains at length that this check earns its keep by living at the one
/// function that owns "a path becomes pixels" — the FIFO that hangs `open(2)`
/// forever had already been fixed four times at four call sites before it was
/// fixed here. A second reader of document-supplied paths is exactly the
/// fifth call site that argument predicted, so it gets the same guard rather
/// than its own copy of one.
fn ensure_regular_file(path: &Path) -> Result<(), DecodeError> {
    let meta = std::fs::metadata(path).map_err(DecodeError::Io)?;
    if !meta.is_file() {
        return Err(DecodeError::NotAFile(path.to_path_buf()));
    }
    Ok(())
}

/// Reads `path` as SVG source if that is what it is.
///
/// `Ok(None)` means "this is not an SVG, carry on with the raster path" — the
/// sniff is on content, never on the extension, so a `.svg` holding a PNG
/// decodes as a PNG and a `diagram.txt` holding SVG renders as a drawing.
///
/// Bounded twice over: the file is refused before reading if it is larger
/// than [`svg::MAX_SVG_BYTES`], so a huge file is never pulled into memory to
/// discover it was too big, and the read is capped anyway in case the file
/// grew between the two.
fn read_svg_source(path: &Path) -> Result<Option<String>, DecodeError> {
    use std::io::Read as _;

    ensure_regular_file(path)?;
    let mut file = std::fs::File::open(path).map_err(DecodeError::Io)?;

    let mut prefix = [0u8; 1024];
    let read = file.read(&mut prefix).map_err(DecodeError::Io)?;
    if !svg::looks_like_svg(&prefix[..read]) {
        return Ok(None);
    }

    let len = file.metadata().map_err(DecodeError::Io)?.len();
    if len > svg::MAX_SVG_BYTES as u64 {
        return Err(DecodeError::Malformed(format!(
            "svg: {len} bytes, over the {}-byte limit",
            svg::MAX_SVG_BYTES
        )));
    }
    let mut source = String::from_utf8_lossy(&prefix[..read]).into_owned();
    file.take(svg::MAX_SVG_BYTES as u64)
        .read_to_string(&mut source)
        .map_err(|_| DecodeError::Malformed("svg: source is not valid UTF-8".to_string()))?;
    Ok(Some(source))
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
    if let Some(source) = read_svg_source(path)? {
        return svg::probe(&source, limits);
    }
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
/// decoding, so the underlying decoder itself rejects a bomb-dimension
/// header (verified against `image`'s own source; see the module doc
/// comment) before allocating a pixel buffer for it.
///
/// The output is PNG whatever went in — the kitty protocol wants one format
/// and this is where every input becomes it, so a GIF and a TIFF reach
/// `protocol.rs` indistinguishable from a PNG. An animated GIF arrives as its
/// first frame; nothing here holds a frame clock.
pub fn decode_and_scale(
    path: &Path,
    target_px: (u32, u32),
    limits: Limits,
) -> Result<DecodedImage, DecodeError> {
    // Vector and raster meet here and nowhere else: both produce a
    // `DynamicImage`, and `letterbox_onto` is the single place that decides
    // what a transmitted raster's pixel dimensions are. An SVG that
    // letterboxed itself would be a second answer to that question.
    // The reported source size is the *input's* own, never the buffer's —
    // they are the same number for a raster and deliberately different for a
    // vector, which is fitted to the target before a pixel is allocated.
    let (source_width, source_height, img) = match read_svg_source(path)? {
        Some(source) => {
            let out = svg::rasterize(&source, target_px, limits)?;
            (out.source.0, out.source.1, out.image)
        }
        None => {
            let img = decode_within_limits(|| opened(path, limits), limits)?;
            (img.width(), img.height(), img)
        }
    };
    let png = letterbox_onto(&img, target_px)?;
    Ok(DecodedImage {
        source_width,
        source_height,
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
///
/// **One resize, not two, and that is measured.** Phase 3 was chartered to add
/// a two-stage downscale — repeated halving down to within 2x of the target,
/// then one Triangle pass — on the strength of an audit that measured it at a
/// 500x400 target. At the target a real full-width box actually asks for
/// (~2400 px: 100 cells at the fallback 24 px cell width), it does not pay:
/// `cargo run --release -p gfx --example downscale_probe` measures a 6000x6000
/// source at 445 ms one-stage against 417 ms two-stage — **6% end to end**, of
/// which the 273 ms decode is untouched either way. A second resize path and a
/// hand-rolled halving loop is not worth 6%, so the two-stage step was
/// measured and declined rather than shipped as a no-op. Re-run the probe
/// before reviving it.
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

    // ------------------- untrusted paths that must never be opened --------

    /// **The security review's fourth blocker.** A document-supplied image
    /// path pointing at a FIFO with no writer blocks `open(2)` forever, and
    /// this runs inside the event loop under raw mode where Ctrl-C is a
    /// keystroke rather than a signal — a viewer that cannot be quit from the
    /// keyboard.
    ///
    /// Both public entry points are checked, because both reach `opened`, and
    /// each is behind a watchdog: a regression **hangs**, so a bare call
    /// would wedge the suite instead of failing it.
    #[test]
    #[cfg(unix)]
    fn test_a_fifo_image_path_is_refused_by_type_and_never_opened() {
        use std::sync::mpsc;
        use std::time::Duration;

        let path = scratch_path("fifo-image");
        let _ = std::fs::remove_file(&path);
        let made = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .expect("run mkfifo");
        assert!(made.success(), "mkfifo failed: {made:?}");

        for (label, probe) in [("probe_dimensions", 0usize), ("decode_and_scale", 1usize)] {
            let (tx, rx) = mpsc::channel();
            let path_for_thread = path.clone();
            std::thread::spawn(move || {
                let limits = Limits::default();
                let answer = if probe == 0 {
                    probe_dimensions(&path_for_thread, limits).err()
                } else {
                    decode_and_scale(&path_for_thread, (16, 16), limits).err()
                };
                let _ = tx.send(match answer {
                    Some(err) => format!("{err:?}"),
                    None => "opened".to_string(),
                });
            });
            let answer = rx.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| {
                panic!(
                    "{label} did not return within 5s on a FIFO — this is the \
                     blocker: `open(2)` blocks and the viewer can no longer be \
                     quit from the keyboard"
                )
            });
            assert!(
                answer.starts_with("NotAFile"),
                "{label} must refuse by type before opening, got {answer}"
            );
        }
        let _ = std::fs::remove_file(&path);
    }

    /// A character device would read forever rather than block; the same
    /// `stat` settles it.
    #[test]
    #[cfg(unix)]
    fn test_a_character_device_image_path_is_refused_by_type() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let err = probe_dimensions(Path::new("/dev/zero"), Limits::default()).err();
            let _ = tx.send(match err {
                Some(err) => format!("{err:?}"),
                None => "opened".to_string(),
            });
        });
        let answer = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("probing /dev/zero must return promptly, not read forever");
        assert!(answer.starts_with("NotAFile"), "got {answer}");
    }

    #[test]
    fn test_a_directory_image_path_is_refused_by_type() {
        let dir = scratch_path("dir-image");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        let err =
            probe_dimensions(&dir, Limits::default()).expect_err("a directory is not an image");
        assert!(matches!(err, DecodeError::NotAFile(_)), "{err:?}");
        assert!(err.to_string().starts_with("not a regular file:"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The guard must not have narrowed the ordinary cases: a real PNG still
    /// decodes, and a missing path is still the `Io` error callers already
    /// degrade on.
    #[test]
    fn test_the_type_guard_leaves_real_images_and_missing_paths_alone() {
        let png = write_file("guard-ok.png", &valid_tiny_png());
        assert!(probe_dimensions(&png, Limits::default()).is_ok());
        assert!(decode_and_scale(&png, (8, 8), Limits::default()).is_ok());
        let _ = std::fs::remove_file(&png);

        let missing = scratch_path("guard-missing.png");
        let _ = std::fs::remove_file(&missing);
        let err = probe_dimensions(&missing, Limits::default()).expect_err("no such file");
        assert!(
            matches!(err, DecodeError::Io(_)),
            "a missing file must stay the Io error callers already handle: {err:?}"
        );
    }

    /// Every format this crate enables, encoded by `image` itself and read
    /// back through the *real* entry points rather than through a decoder
    /// picked by hand.
    ///
    /// The point is the round trip, not the encoder: `probe_dimensions` and
    /// `decode_and_scale` both begin with `with_guessed_format`, so a format
    /// that is compiled in but not sniffable — or sniffable but not compiled
    /// in — fails here. That is the whole failure mode of adding a format by
    /// Cargo feature, and it is invisible to a test that constructs a decoder
    /// directly.
    fn encoded(format: image::ImageFormat, w: u32, h: u32) -> Vec<u8> {
        let img = image::DynamicImage::new_rgba8(w, h);
        let mut bytes = Cursor::new(Vec::new());
        img.write_to(&mut bytes, format).expect("encode");
        bytes.into_inner()
    }

    /// The formats `Cargo.toml` claims, and the extension each is written
    /// with. The extension is cosmetic — detection is by content — but a real
    /// document names its files this way, so the fixtures do too.
    const FORMATS: &[(image::ImageFormat, &str)] = &[
        (image::ImageFormat::Png, "png"),
        (image::ImageFormat::Jpeg, "jpg"),
        (image::ImageFormat::Gif, "gif"),
        (image::ImageFormat::WebP, "webp"),
        (image::ImageFormat::Bmp, "bmp"),
        (image::ImageFormat::Ico, "ico"),
        (image::ImageFormat::Tiff, "tiff"),
    ];

    #[test]
    fn test_every_enabled_format_probes_and_decodes() {
        for (format, ext) in FORMATS {
            // ICO caps a directory entry at 256 px per side, so every fixture
            // stays inside the smallest format's ceiling.
            let bytes = encoded(*format, 32, 16);
            let path = write_file(&format!("fmt-ok.{ext}"), &bytes);

            let dims = probe_dimensions(&path, Limits::default())
                .unwrap_or_else(|e| panic!("{format:?} did not probe: {e:?}"));
            assert_eq!(dims, (32, 16), "{format:?} probed the wrong size");

            let decoded = decode_and_scale(&path, (8, 8), Limits::default())
                .unwrap_or_else(|e| panic!("{format:?} did not decode: {e:?}"));
            assert_eq!(
                (decoded.source_width, decoded.source_height),
                (32, 16),
                "{format:?} reported the wrong source size"
            );
            // The emitted raster is always PNG and always exactly the target
            // box, whatever went in — that is the guarantee `protocol.rs`
            // downstream is written against.
            let raster = image::load_from_memory(&decoded.png)
                .unwrap_or_else(|e| panic!("{format:?} emitted an unreadable png: {e:?}"));
            assert_eq!(
                (raster.width(), raster.height()),
                (8, 8),
                "{format:?} did not land on the target box"
            );
            let _ = std::fs::remove_file(&path);
        }
    }

    /// The bomb guard, per format.
    ///
    /// `decode.rs` claims every enabled codec refuses an over-limit header
    /// before sizing a buffer from it — some by overriding `set_limits`, the
    /// rest by inheriting the trait default. That is a claim about a
    /// dependency's internals, so it is worth an assertion rather than a
    /// comment: bump `image`, or add a format whose decoder ignores limits,
    /// and this is what says so.
    ///
    /// A real bomb file is not needed and would be the wrong test anyway —
    /// what must hold is that a *header* claiming more than `max_dim` is
    /// refused, which a small header with a tight limit states exactly.
    #[test]
    fn test_a_bomb_header_is_refused_in_every_format() {
        let tight = Limits {
            max_dim: 8,
            max_alloc: 1024,
        };
        for (format, ext) in FORMATS {
            let bytes = encoded(*format, 64, 64);
            let path = write_file(&format!("fmt-bomb.{ext}"), &bytes);

            let err = probe_dimensions(&path, tight)
                .expect_err("a header over the limit must be refused");
            assert!(
                matches!(err, DecodeError::ExceedsLimits { .. }),
                "{format:?} refused a bomb header for the wrong reason: {err:?}"
            );
            let err = decode_and_scale(&path, (4, 4), tight)
                .expect_err("a decode over the limit must be refused");
            assert!(
                matches!(err, DecodeError::ExceedsLimits { .. }),
                "{format:?} decoded past its limit: {err:?}"
            );
            let _ = std::fs::remove_file(&path);
        }
    }

    /// An animated GIF is its first frame, and that is a decision rather than
    /// an accident — nothing in stele holds a frame clock, and a silently
    /// still animation is a better surprise than a document that repaints
    /// forever. Asserted on a two-frame GIF whose frames differ, so a change
    /// that started returning the last frame would fail rather than pass by
    /// looking similar.
    #[test]
    fn test_an_animated_gif_decodes_to_its_first_frame() {
        use image::codecs::gif::GifEncoder;
        use image::{Delay, Frame, Rgba, RgbaImage};

        let red = RgbaImage::from_pixel(8, 8, Rgba([255, 0, 0, 255]));
        let blue = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 255, 255]));
        let mut bytes = Vec::new();
        {
            let mut enc = GifEncoder::new(&mut bytes);
            for frame in [red, blue] {
                enc.encode_frame(Frame::from_parts(
                    frame,
                    0,
                    0,
                    Delay::from_numer_denom_ms(100, 1),
                ))
                .expect("encode frame");
            }
        }
        let path = write_file("animated.gif", &bytes);

        let decoded = decode_and_scale(&path, (8, 8), Limits::default()).expect("decodes");
        let img = image::load_from_memory(&decoded.png).expect("re-read the emitted png");
        let px = img.to_rgba8();
        let pixel = px.get_pixel(4, 4).0;
        assert!(
            pixel[0] > 200 && pixel[2] < 60,
            "expected the first (red) frame, got {pixel:?}"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// SVG through the public entry points, which is the half `svg`'s own
    /// tests cannot reach: the sniff, the byte cap, and the shared letterbox.
    ///
    /// The extension is deliberately wrong on two of these. Detection is by
    /// content everywhere in this module, and an SVG named `.png` is the
    /// cheapest way to state that a reader can trust the picture over the
    /// filename.
    #[test]
    fn test_svg_probes_and_decodes_through_the_public_path() {
        const DRAWING: &[u8] =
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="60">
            <rect width="120" height="60" fill="#3a78c8"/></svg>"##;

        for name in ["drawing.svg", "lying.png", "no-extension"] {
            let path = write_file(name, DRAWING);
            assert_eq!(
                probe_dimensions(&path, Limits::default()).expect("probes"),
                (120, 60),
                "{name} did not probe as SVG"
            );
            let decoded = decode_and_scale(&path, (40, 40), Limits::default()).expect("decodes");
            assert_eq!((decoded.source_width, decoded.source_height), (120, 60));
            let raster = image::load_from_memory(&decoded.png).expect("a readable png");
            assert_eq!(
                (raster.width(), raster.height()),
                (40, 40),
                "the shared letterbox must still hand back exactly the target box"
            );
            let _ = std::fs::remove_file(&path);
        }
    }

    /// A `.svg` holding a PNG is a PNG. The sniff runs both ways or it is not
    /// a sniff.
    #[test]
    fn test_a_raster_named_svg_still_decodes_as_a_raster() {
        let path = write_file("raster-named.svg", &valid_tiny_png());
        assert!(probe_dimensions(&path, Limits::default()).is_ok());
        assert!(decode_and_scale(&path, (8, 8), Limits::default()).is_ok());
        let _ = std::fs::remove_file(&path);
    }

    /// The byte cap is checked from the file's own length before the source is
    /// read, so an oversized drawing never reaches memory to be measured.
    #[test]
    fn test_an_oversized_svg_is_refused_without_being_read() {
        let mut source = Vec::from(
            &b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\">"[..],
        );
        source.resize(crate::svg::MAX_SVG_BYTES + 1_024, b' ');
        source.extend_from_slice(b"</svg>");
        let path = write_file("huge.svg", &source);

        let err = probe_dimensions(&path, Limits::default()).expect_err("too large");
        assert!(
            matches!(&err, DecodeError::Malformed(m) if m.contains("over the")),
            "{err:?}"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// The barricade covers the vector path too — with the same watchdog the
    /// raster FIFO test uses, because a regression here *hangs* rather than
    /// failing, and a bare call would wedge the suite.
    ///
    /// This is the test the `ensure_regular_file` extraction exists for. The
    /// whole argument in `opened`'s doc is that fixing the block at call sites
    /// leaves the next reader of a document-supplied path exposed; the SVG
    /// path is that next reader, and a `.svg` FIFO is how it would have shown
    /// up.
    #[cfg(unix)]
    #[test]
    fn test_a_fifo_named_svg_is_refused_by_type_and_never_opened() {
        use std::sync::mpsc;
        use std::time::Duration;

        let path = scratch_path("svg-fifo.svg");
        let _ = std::fs::remove_file(&path);
        let made = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .expect("run mkfifo");
        assert!(made.success(), "mkfifo failed: {made:?}");

        let (tx, rx) = mpsc::channel();
        let path_for_thread = path.clone();
        std::thread::spawn(move || {
            let _ = tx.send(probe_dimensions(&path_for_thread, Limits::default()).err());
        });
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Some(err)) => assert!(matches!(err, DecodeError::NotAFile(_)), "{err:?}"),
            Ok(None) => panic!("a fifo must not probe as an image"),
            Err(_) => panic!("the svg path blocked on a fifo — the stat guard was bypassed"),
        }
        let _ = std::fs::remove_file(&path);
    }
}
