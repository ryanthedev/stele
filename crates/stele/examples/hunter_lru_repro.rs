//! Repro: when more than `CAP` (32) media boxes are visible in ONE frame, the
//! LRU evictor deletes the ones already painted *this* frame — so the top of
//! the viewport goes blank, with no alt text, while the bottom renders.
//!
//! A badge row (`![build](a.png) ![cov](b.png) ...`) puts dozens of one-row
//! inline images on two or three lines, so "33+ boxes visible at once" is an
//! ordinary README, not a stress case.
//!
//! DW-6.1's own test (`sink.rs::test_dw_6_1_cap_32_evicts_least_recently_used_within_a_single_frame`)
//! paints exactly this shape and asserts `creates == 40, deletes == 8`. Both
//! numbers are right. The picture is still wrong.
//!
//! Run: `cargo run -p stele --example hunter_lru_repro`

use std::io::Write as _;

use layout::{LayoutConfig, layout};
use stele::media::GfxMediaSink;
use stele::{ImageSizer, Painter, Size};
use width::{WidthConfig, WidthEngine};

const COUNT: usize = 40;

fn main() {
    let dir = std::env::temp_dir().join(format!("stele-hunter-lru-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // 40x24 px == exactly 2x1 cells at the assumed 24x48 geometry, so every
    // badge is a one-row inline box and many share a line.
    let img = image::RgbaImage::from_pixel(40, 24, image::Rgba([10, 120, 220, 255]));
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let mut source = String::from("# Badges\n\n");
    for i in 0..COUNT {
        std::fs::File::create(dir.join(format!("b{i}.png")))
            .unwrap()
            .write_all(&png)
            .unwrap();
        source.push_str(&format!("![badge{i}](b{i}.png) "));
    }
    source.push('\n');

    let doc = ast::Document::parse(&source);
    let engine = WidthEngine::new(WidthConfig::default());
    let sizer = ImageSizer::new(&dir);
    let tree = layout(&doc, 100, &LayoutConfig::default(), &engine, &sizer);

    let mut painter = Painter::new(WidthEngine::new(WidthConfig::default()));
    painter.register_media(Box::new(GfxMediaSink::new(doc, &dir)));
    let mut frame = Vec::new();
    painter
        .frame(
            &tree,
            0,
            Size {
                width: 100,
                height: 40,
            },
            &mut frame,
        )
        .unwrap();
    std::fs::remove_dir_all(&dir).ok();

    // Replay the frame's kitty commands in order and report which image ids
    // were transmitted and then deleted again before the frame ended.
    let text = String::from_utf8_lossy(&frame);
    let mut transmitted: Vec<u32> = Vec::new();
    let mut deleted: Vec<u32> = Vec::new();
    for cmd in text.split("\u{1b}_G").skip(1) {
        let body = cmd.split('\u{1b}').next().unwrap_or("");
        let id = body
            .split(',')
            .find_map(|kv| kv.strip_prefix("i="))
            .and_then(|v| v.split(';').next().unwrap_or("").parse::<u32>().ok());
        let Some(id) = id else { continue };
        if body.starts_with("a=t,f=100") {
            transmitted.push(id);
        } else if body.starts_with("a=d,d=I") {
            deleted.push(id);
        }
    }

    println!("boxes in the document : {COUNT}");
    println!("transmitted this frame: {}", transmitted.len());
    println!(
        "deleted   this frame  : {}  -> ids {deleted:?}",
        deleted.len()
    );
    println!(
        "still live at frame end: {}",
        transmitted.len() - deleted.len()
    );

    // Was anything painted where the over-budget boxes are? Each badge is a
    // 2-cell box, so its alt text ("badge37") is clipped to two cells —
    // grepping for the whole word can never find it however well the sink
    // behaves. What is observable is the sink's own box-origin CUP followed
    // by glyphs: `write_text_row` re-homes to `rect.x`/`rect.y` before
    // writing, so every text fallback on the wire looks like `ESC[r;cH` + the
    // clipped alt text. The graphics rung writes no text at all.
    let clipped_alt = &"badge"[..2];
    let text_rungs = text
        .match_indices(clipped_alt)
        .filter(|(i, _)| text[..*i].ends_with('H'))
        .count();
    println!("boxes that painted a text fallback in the box: {text_rungs}");

    let same_frame_blank: Vec<u32> = deleted
        .iter()
        .copied()
        .filter(|id| transmitted.contains(id))
        .collect();
    if !same_frame_blank.is_empty() && text_rungs == 0 {
        println!(
            "\nBLANK: {} images were transmitted, placed, then deleted inside the same \
             frame with nothing painted in their place — the first {} boxes of the row \
             render as empty cells.",
            same_frame_blank.len(),
            same_frame_blank.len()
        );
    } else {
        println!(
            "\nOK: no image was deleted in the frame it was placed in, and the {} boxes \
             over the {}-placement cap each painted their alt text in their own cells \
             instead of leaving a hole.",
            text_rungs,
            transmitted.len()
        );
    }
}
