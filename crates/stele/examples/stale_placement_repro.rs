//! Repro: after a page jump, images from the old screen are still on it.
//!
//! stele paints only on input (`event::read()` blocks), so the sink's
//! one-frame grace period assumed a frame that never arrives. Press Ctrl-d
//! once and the frame that lands is the *only* frame — anything the grace
//! deferred stayed on the terminal until the next keystroke.
//!
//! Sweeps every single-Ctrl-d and single-Ctrl-u jump through the document,
//! each an independent "the user pressed a key once and stopped" scenario,
//! and rebuilds the terminal's live-placement set from the emitted bytes.
//! The verdict is on what the terminal is still *drawing*, not on whether a
//! delete was emitted: the weaker question is the one that let this ship.
//!
//! Run: `cargo run -p stele --example stale_placement_repro`

use std::collections::HashSet;

use layout::layout;
use stele::media::GfxMediaSink;
use stele::{ImageSizer, Painter, Size};
use width::{WidthConfig, WidthEngine};

const VIEWPORT_W: u16 = 90;
const VIEWPORT_H: u16 = 24;

fn main() {
    // Anchored to the crate, not the cwd: `cargo run` from anywhere in the
    // workspace must find the fixture.
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../testdocs/02-math.md")
            .to_string_lossy()
            .into_owned()
    });
    let src = std::fs::read_to_string(&path).expect("readable markdown");
    let base_dir = std::path::Path::new(&path)
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let doc = ast::Document::parse(&src);
    let engine = WidthEngine::new(WidthConfig::default());
    let config = layout::LayoutConfig {
        min_width: 24,
        max_width: VIEWPORT_W,
    };
    let sizer = ImageSizer::new(&base_dir);
    let tree = layout(&doc, VIEWPORT_W, &config, &engine, &sizer);

    let mut painter = Painter::new(WidthEngine::new(WidthConfig::default()));
    painter.register_media(Box::new(GfxMediaSink::new(doc, &base_dir)));

    // The terminal's own live-placement set, rebuilt from the wire in
    // emission order. "Was a delete emitted?" is the weaker question and the
    // one that let this ship: what matters is which images the terminal is
    // still *drawing* when the frame ends.
    let mut live: HashSet<String> = HashSet::new();
    let mut frame = |scroll: usize, live: &mut HashSet<String>| -> HashSet<String> {
        let mut buf = Vec::new();
        painter
            .frame(
                &tree,
                scroll,
                Size {
                    width: VIEWPORT_W,
                    height: VIEWPORT_H,
                },
                &mut buf,
            )
            .expect("paint");
        let text = String::from_utf8_lossy(&buf).to_string();
        let mut placed = HashSet::new();
        for token in text.split("\x1b_G").skip(1) {
            let body = token.split('\x1b').next().unwrap_or("");
            let Some(id) = body
                .split(&[',', ';'][..])
                .find_map(|kv| kv.strip_prefix("i="))
                .map(|v| {
                    v.chars()
                        .take_while(char::is_ascii_digit)
                        .collect::<String>()
                })
            else {
                continue;
            };
            if body.starts_with("a=p") {
                live.insert(id.clone());
                placed.insert(id);
            } else if body.starts_with("a=d") {
                live.remove(&id);
            }
        }
        placed
    };

    let half = (VIEWPORT_H / 2) as usize;
    let top = tree.line_count().saturating_sub(half);
    println!("viewport {VIEWPORT_W}x{VIEWPORT_H}, one Ctrl-d = +{half} rows\n");

    let mut down: Vec<usize> = (0..top).step_by(half).collect();
    let mut up = down.clone();
    up.reverse();

    let mut bad = 0;
    let mut blank_landings = 0;
    for (label, offsets) in [("Ctrl-d", &mut down), ("Ctrl-u", &mut up)] {
        println!("=== {label} ===");
        println!(
            "{:>12}  {:>24}  {:>24}  verdict",
            "jump", "left the screen", "still drawn after"
        );
        println!("{}", "-".repeat(86));
        for pair in offsets.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            let before = frame(from, &mut live);
            let after = frame(to, &mut live);

            let vanished: Vec<String> = sorted(&before.difference(&after).cloned().collect());
            if vanished.is_empty() {
                continue;
            }
            if after.is_empty() {
                blank_landings += 1;
            }
            // Anything the terminal is still drawing that this frame did not
            // place is painted on top of the new screen.
            let stale: Vec<String> = sorted(&live.difference(&after).cloned().collect());
            let verdict = if stale.is_empty() {
                "ok".to_string()
            } else {
                bad += 1;
                format!("STALE: {stale:?} drawn but not painted")
            };
            println!(
                "{:>12}  {:>24}  {:>24}  {verdict}",
                format!("{from}->{to}"),
                format!("{vanished:?}"),
                format!("{:?}", sorted(&live)),
            );
        }
        println!();
    }

    println!("jumps landing on a media-free screen: {blank_landings}");
    if bad == 0 {
        println!(
            "\nOK — after every jump, in both directions, the only images the terminal\n\
             is drawing are the ones that frame painted."
        );
    } else {
        println!(
            "\nBUG — {bad} jump(s) leave images drawn with nothing repainting them.\n\
             stele blocks on event::read(), so the frame the grace period defers\n\
             eviction to never arrives until the user presses another key."
        );
    }
}

fn sorted(set: &HashSet<String>) -> Vec<String> {
    let mut v: Vec<_> = set.iter().cloned().collect();
    v.sort_by_key(|s| s.parse::<u32>().unwrap_or(0));
    v
}
