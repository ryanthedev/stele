//! Bounded local image decode: a header-only dimension probe (no full pixel
//! decode at layout time) and a full, `Limits`-bounded decode+scale for
//! painting.
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

/// A decoded image, scaled and re-encoded as PNG at the caller's requested
/// target cell-pixel size. `width`/`height` are the *original* decoded
/// dimensions (before scaling) — useful for logging/diagnostics.
#[derive(Debug, Clone)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
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
    let probe_reader = opened(path, limits)?;
    let (width, height) = probe_reader.into_dimensions().map_err(|e| match e {
        image::ImageError::Limits(_) => DecodeError::ExceedsLimits {
            width: 0,
            height: 0,
        },
        other => DecodeError::Malformed(other.to_string()),
    })?;
    if !limits.accepts(width, height) {
        return Err(DecodeError::ExceedsLimits { width, height });
    }

    // `into_dimensions` consumed the probe reader; re-open for the actual
    // decode (limits are re-applied, and re-checked by the decoder as it
    // reads — the real backstop against a file whose body disagrees with
    // its own header).
    let decode_reader = opened(path, limits)?;
    let img = decode_reader.decode().map_err(|e| match e {
        image::ImageError::Limits(_) => DecodeError::ExceedsLimits { width, height },
        other => DecodeError::Malformed(other.to_string()),
    })?;

    let (target_w, target_h) = (target_px.0.max(1), target_px.1.max(1));
    let scaled = image::imageops::resize(
        &img.to_rgba8(),
        target_w,
        target_h,
        image::imageops::FilterType::Triangle,
    );
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(scaled)
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| DecodeError::Malformed(e.to_string()))?;

    Ok(DecodedImage {
        width: img.width(),
        height: img.height(),
        png,
    })
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

    #[test]
    fn test_missing_file_rejected_cleanly() {
        let result = probe_dimensions(std::path::Path::new("/no/such/file.png"), Limits::default());
        assert!(matches!(result, Err(DecodeError::Io(_))));
    }

    #[test]
    fn test_decode_and_scale_produces_png_at_target_pixel_size() {
        let path = write_file("scale", &valid_tiny_png());
        let decoded = decode_and_scale(&path, (24, 48), Limits::default()).unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 2));
        let (w, h) = probe_reencoded_dimensions(&decoded.png);
        assert_eq!((w, h), (24, 48));
        std::fs::remove_file(&path).ok();
    }

    fn probe_reencoded_dimensions(png: &[u8]) -> (u32, u32) {
        let img = image::load_from_memory(png).unwrap();
        (img.width(), img.height())
    }
}
