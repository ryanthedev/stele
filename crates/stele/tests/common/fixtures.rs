//! The three helpers every media/paint test needs: a width engine, a scratch
//! directory, and a PNG whose pixel size is exactly what the caller asked for.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use width::{WidthConfig, WidthEngine};

/// The default width engine. Every test wants the same one: the corrections
/// under test are the *default* ones a real reader gets.
pub fn engine() -> WidthEngine {
    WidthEngine::new(WidthConfig::default())
}

/// A per-test scratch directory under the system temp dir, tagged so two
/// tests in the same process cannot collide.
pub fn scratch_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("stele-test-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Writes a `w`x`h` pixel PNG into `dir`.
///
/// The pixel size is the *point*: at the sink's assumed 24x48 px cell the
/// box's cell extent is exactly `(w / 24, h / 48)`, which is what lets the
/// placement tests assert exact `c=`/`r=`/`w=`/`h=` bytes instead of a
/// tolerance. The fill colour is opaque and uniform so a letterbox or a crop
/// is visible as transparency rather than as more of the same pixels.
pub fn write_png(dir: &Path, name: &str, w: u32, h: u32) {
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba([7, 8, 9, 255]));
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    std::fs::File::create(dir.join(name))
        .unwrap()
        .write_all(&bytes)
        .unwrap();
}
