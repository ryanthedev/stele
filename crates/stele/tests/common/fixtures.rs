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
/// tests in the same process cannot collide — and **emptied first**, so a
/// previous process cannot collide with this one either.
///
/// The path is unique per `(tag, pid)` and nothing has ever removed it, so
/// the temp dir accumulates one directory per test per run — 5,479 of them
/// on the machine this was found on, a dozen still unreadable. That is untidy on its own, and it is a
/// test bug as soon as the OS hands out a pid twice, which it does: this
/// name then resolves to a directory a previous run built, and the test
/// inherits its contents and its permission bits. `fixture`'s
/// `create_dir("sub")` and DW-3.16's `create_dir("locked")` both fail
/// outright with `AlreadyExists`, and a directory left execute-only by an
/// aborted run of `test_dw_3_16_*` comes back unreadable under a test that
/// expects to read it.
///
/// Reproduced rather than reasoned: planting exactly that state for the pids
/// a test binary was about to be handed turned an intermittent failure into
/// one that fires on demand.
///
/// Emptying on the way *in* rather than on the way out is deliberate. A
/// cleanup at the end of a test cannot run when the test panics, which is
/// the case that leaves the unreadable directory behind in the first place,
/// and it would take the evidence with it whenever a failure needs looking
/// at. This runs before the test can see anything, and leaves the last run's
/// tree on disk to inspect.
pub fn scratch_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("stele-test-{tag}-{}", std::process::id()));
    remove_stale(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Removes `p` and everything under it, if it is there at all.
///
/// `remove_dir_all` alone is not enough: DW-3.16 leaves directories at mode
/// `0o100`, and one of those cannot be enumerated, so the removal fails on
/// exactly the case that most needs it. Every directory is made traversable
/// on the way down first.
fn remove_stale(p: &Path) {
    if std::fs::symlink_metadata(p).is_err() {
        return;
    }
    #[cfg(unix)]
    make_removable(p);
    let _ = std::fs::remove_dir_all(p);
}

/// Restores `0o700` on `p` and every directory beneath it.
///
/// Walks `symlink_metadata` rather than `is_dir` so a symlink is never
/// followed: this recurses while chmod-ing, and a link pointing out of the
/// temp directory would take both operations with it.
#[cfg(unix)]
fn make_removable(p: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let Ok(meta) = std::fs::symlink_metadata(p) else {
        return;
    };
    if !meta.file_type().is_dir() {
        return;
    }
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700));
    let Ok(entries) = std::fs::read_dir(p) else {
        return;
    };
    for entry in entries.flatten() {
        make_removable(&entry.path());
    }
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
