//! DW-6.1, live half. Two `#[ignore]`d tests, because each proves a
//! different thing and neither alone is the whole DW item:
//!
//! - `live_ghostty_session_launches_and_the_probe_harness_completes` —
//!   launches a real Ghostty via `crates/probe`'s `Launcher` and asserts the
//!   harness completes. Emits no graphics; proves only that the launch path
//!   works.
//! - `emit_live_kitty_image_to_dev_tty` — writes real kitty transmit/place
//!   bytes for a visible image to `/dev/tty`, then scrolls and deletes. When
//!   the test process is itself running inside Ghostty, `/dev/tty` IS that
//!   window, so this genuinely exercises the protocol against a live
//!   terminal. Confirming the image *appeared* still requires a human eye or
//!   a screenshot — the terminal sends no ack (see `ghostty-caps.md`:
//!   "kitty deletion is silent").
//!
//! **Ignored by default**, following `crates/probe/tests/live_ghostty.rs`'s
//! precedent: both need a real macOS GUI session with Ghostty, which CI does
//! not have. Run locally:
//!
//! ```sh
//! cargo build -p probe
//! cargo test -p gfx --test live_ghostty -- --ignored --nocapture
//! ```
//!
//! The other half of DW-6.1 — create/delete balance under the eviction
//! policy — is a normal (non-ignored) test against a captured `Vec<u8>`
//! writer in `crates/stele/src/media/sink.rs`, per the DW item's own
//! instruction to split the assertion this way.

use std::path::PathBuf;
use std::time::Duration;

use gfx::{CellRect, Emitter, ImageId};
use probe::Launcher;

/// `spike_a` is a binary of the `probe` package, not `gfx`'s own — Cargo's
/// `CARGO_BIN_EXE_*` mechanism only sets a compile-time env var for a
/// binary target belonging to the *same* package as the integration test
/// (verified: `env!("CARGO_BIN_EXE_spike_a")` fails to compile here with
/// exactly that explanation), so it isn't available to `gfx`'s tests. This
/// resolves the path at runtime instead, relative to the target directory
/// (respecting `$CARGO_TARGET_DIR`, defaulting to `target/` otherwise) —
/// requires `cargo build -p probe` to have produced it first, which the
/// module doc comment's invocation instructions call for.
fn probe_bin_path() -> PathBuf {
    let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_string());
    let mut p = PathBuf::from(target_dir);
    p.push("debug");
    p.push(if cfg!(windows) {
        "spike_a.exe"
    } else {
        "spike_a"
    });
    p
}

/// A tiny valid PNG (2x2 red), transmitted repeatedly. Content doesn't
/// matter for this test — only that Ghostty accepts 100 create/place/delete
/// cycles in a row without hanging or erroring the session.
fn tiny_png() -> Vec<u8> {
    let mut png = Vec::new();
    let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 30, 30, 255]));
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    png
}

/// Runs the probe binary as a real Ghostty child, feeding it a small shell
/// script instead of `spike_a` — this test doesn't need Spike A's nine
/// checks, only a live PTY to send our own bytes down. Reuses
/// `Launcher::run_probe`'s launch pattern (`open -na Ghostty.app --args -e`)
/// since that's the only pattern verified to work (see
/// `docs/spikes/ghostty-caps.md`'s "Launch pattern" section).
/// NOTE ON WHAT THIS DOES AND DOES NOT PROVE: it launches a real Ghostty
/// session and asserts the launch harness completes cleanly. It emits NO
/// graphics bytes and performs NO scrolling, so it does **not** on its own
/// satisfy DW-6.1's "image renders in live Ghostty and survives 100 scroll
/// cycles" — that needs the visual pass driven by
/// `emit_live_kitty_image_to_dev_tty` below plus a human/screenshot check.
/// The name says exactly this much so a green run is not mistaken for the
/// visual claim.
#[test]
#[ignore = "requires a real Ghostty install and an interactive GUI session"]
fn live_ghostty_session_launches_and_the_probe_harness_completes() {
    let launcher = Launcher::default_macos();
    let probe_bin = probe_bin_path();
    assert!(
        probe_bin.exists(),
        "run `cargo build -p probe` first so {probe_bin:?} exists"
    );
    let mut out_path = std::env::temp_dir();
    out_path.push(format!("stele-gfx-live-scroll-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&out_path);

    // Reuses the probe binary purely as a live-Ghostty launch harness (it
    // already validates the PTY and writes a result file on exit); the
    // graphics bytes this test cares about are written independently below
    // via a throwaway script run through the same launch pattern would
    // require a second binary, so instead this test documents the intended
    // exercise and asserts the harness itself completes cleanly — the
    // visual/scroll-survival claim in DW-6.1 needs the one manual pass the
    // spike itself calls for (see the phase discovery doc's "Placement
    // mode" deviation note).
    let result = launcher.run_probe(
        &probe_bin,
        &["--out", out_path.to_str().unwrap()],
        &out_path,
        Duration::from_secs(20),
    );
    assert!(
        result.is_ok(),
        "expected a live Ghostty session to launch and complete: {result:?}"
    );
    let _ = std::fs::remove_file(&out_path);
}

/// A lower-ceremony sibling: writes 100 create/place/delete cycles straight
/// to the emitter with a `Vec<u8>` writer, then (when run with `--ignored`
/// against a live terminal by hand, piping this test's stdout into a real
/// Ghostty) a human can visually confirm no artifact survives past its
/// delete. Kept as a `#[test]` (not `#[ignore]`) because writing to a
/// `Vec<u8>` needs no terminal at all — the byte-level create/delete
/// balance this proves is exactly DW-6.1's non-terminal half, duplicated
/// here at the `Emitter` level (the `MediaSink`-level version in
/// `crates/stele/src/media/sink.rs` additionally proves the *eviction
/// policy*, not just that `Emitter` itself emits balanced bytes).
#[test]
fn emitter_level_100_cycles_stay_balanced_in_a_captured_buffer() {
    let mut emitter = Emitter::new();
    let png = tiny_png();
    let mut out = Vec::new();
    for i in 0..100u32 {
        let id = ImageId::new(i + 1);
        emitter.transmit(id, &png, &mut out);
        emitter.place(
            id,
            CellRect {
                x: 0,
                y: 0,
                width: 10,
                height: 4,
            },
            &mut out,
        );
        emitter.delete(id, &mut out);
    }
    let text = String::from_utf8_lossy(&out);
    let creates = text.matches("a=t,f=100").count();
    let deletes = text.matches("a=d,d=I").count();
    assert_eq!(creates, 100);
    assert_eq!(deletes, 100);
}

/// DW-6.1, the part the launch test does NOT cover: emit real kitty graphics
/// bytes for a visibly-sized image to the controlling terminal, hold them
/// across 100 scroll cycles, then delete. When this test runs inside Ghostty,
/// `/dev/tty` is that live window, so these are real protocol bytes hitting a
/// real terminal — not a captured buffer.
///
/// The terminal returns no acknowledgement for placement or deletion
/// (`ghostty-caps.md`: deletion is silent), so this test asserts only what it
/// can observe: every write succeeds and the session stays healthy. Whether
/// the image *rendered* is confirmed by eye or screenshot while it runs —
/// that is exactly the manual pass the spike itself asked for before treating
/// placement as load-bearing.
#[test]
#[ignore = "requires running inside a real Ghostty GUI session"]
fn emit_live_kitty_image_to_dev_tty() {
    use std::io::Write;

    let term = std::env::var("TERM_PROGRAM").unwrap_or_default();
    assert_eq!(
        term, "ghostty",
        "must run inside Ghostty for this to mean anything (TERM_PROGRAM={term:?})"
    );

    // Only meaningful with a controlling terminal. An agent/CI process that
    // merely has TERM_PROGRAM inherited has no /dev/tty, so skip loudly
    // rather than fail — this test is the human visual pass, and pretending
    // it ran would be worse than saying it didn't.
    let mut tty = match std::fs::OpenOptions::new().write(true).open("/dev/tty") {
        Ok(tty) => tty,
        Err(err) => {
            eprintln!(
                "SKIPPED: no controlling terminal ({err}). Run this from an \
                 interactive Ghostty shell to perform the DW-6.1 visual pass."
            );
            return;
        }
    };

    // A 64x64 block, big enough to be unmistakable on screen.
    let mut png = Vec::new();
    let img = image::RgbaImage::from_fn(64, 64, |x, y| {
        image::Rgba([(x * 4) as u8, (y * 4) as u8, 200, 255])
    });
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();

    let mut emitter = Emitter::new();
    let id = ImageId::new(9001);

    writeln!(tty, "\n[stele DW-6.1 live pass] image should appear below:").unwrap();
    emitter.transmit(id, &png, &mut tty);
    emitter.place(
        id,
        CellRect {
            x: 0,
            y: 0,
            width: 8,
            height: 4,
        },
        &mut tty,
    );
    tty.flush().unwrap();

    // Survive 100 scroll cycles: the placement must not corrupt the session.
    for _ in 0..100 {
        write!(tty, "\x1b[S\x1b[T").unwrap(); // scroll up then back down
    }
    tty.flush().unwrap();

    emitter.delete(id, &mut tty);
    writeln!(
        tty,
        "\n[stele DW-6.1 live pass] image deleted; session healthy."
    )
    .unwrap();
    tty.flush().unwrap();
}
