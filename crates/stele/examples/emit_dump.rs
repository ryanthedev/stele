//! Drives the real layout+paint pipeline over a markdown file and dumps what
//! it emits — no pty, no tty, no terminal env to get wrong.
//!
//! This is the headless half of verifying inline media: it proves the boxes
//! are laid out inline and placed at the right columns. It cannot prove they
//! *look* right — that still needs a real Ghostty window.
//!
//! Run: `cargo run -p stele --example emit_dump -- /tmp/stele-demo/demo.md`

use std::io::Write;

use layout::{Line, LineItem, layout};
use stele::media::GfxMediaSink;
use stele::{ImageSizer, Painter, Size};
use width::{WidthConfig, WidthEngine};

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/stele-demo/demo.md".to_string());
    let src = std::fs::read_to_string(&path).expect("readable markdown file");
    let base_dir = std::path::Path::new(&path)
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let width: u16 = std::env::args()
        .nth(2)
        .and_then(|w| w.parse().ok())
        .unwrap_or(100);

    let doc = ast::Document::parse(&src);
    let engine = WidthEngine::new(WidthConfig::default());
    let config = layout::LayoutConfig {
        min_width: 24,
        max_width: width.max(24),
    };
    // The enabled sizer — exactly what main.rs builds when TERM_PROGRAM=ghostty.
    let sizer = ImageSizer::new(&base_dir);
    let tree = layout(&doc, width, &config, &engine, &sizer);

    // DW-4.3: no line may exceed the layout width. Re-measured through the
    // width engine rather than trusting the tree's own cached run widths, so a
    // mis-measured grapheme cluster is caught rather than confirmed.
    let mut overflow = Vec::new();
    for (i, line) in tree.lines(0..tree.line_count()).enumerate() {
        let measured: u16 = match line {
            Line::Items(items) => items.iter().fold(0u16, |acc, item| {
                let w = match item {
                    LineItem::Run(r) => engine.display_width(&r.text).min(u16::MAX as usize) as u16,
                    LineItem::Box(b) => b.cols,
                };
                acc.saturating_add(w)
            }),
            Line::Reserved(r) => r.cols,
        };
        if measured > width {
            overflow.push((i, measured));
        }
    }
    if overflow.is_empty() {
        println!("width bound at {width}: OK ({} lines)", tree.line_count());
    } else {
        println!(
            "width bound at {width}: {} LINE(S) OVERFLOW -> {:?}",
            overflow.len(),
            &overflow[..overflow.len().min(8)]
        );
    }

    // --- what layout decided -------------------------------------------
    let mut inline_rows = 0;
    let mut reserved_rows = 0;
    println!("=== layout ===");
    for (i, line) in tree.lines(0..tree.line_count()).enumerate() {
        match line {
            Line::Items(items) => {
                let boxes: Vec<_> = items
                    .iter()
                    .filter_map(|it| match it {
                        LineItem::Box(b) => Some(b),
                        LineItem::Run(_) => None,
                    })
                    .collect();
                if !boxes.is_empty() {
                    inline_rows += 1;
                    let text: String = items
                        .iter()
                        .map(|it| match it {
                            LineItem::Run(r) => r.text.clone(),
                            LineItem::Box(b) => format!("[BOX {}x{}]", b.cols, b.rows),
                        })
                        .collect();
                    println!("  line {i:>3}: {text}");
                }
            }
            Line::Reserved(r) => {
                reserved_rows += 1;
                println!(
                    "  line {i:>3}: <reserved {}x{} node={}>",
                    r.cols,
                    r.rows,
                    r.node_id.index()
                );
            }
        }
    }
    println!("  inline-box lines: {inline_rows}, reserved rows: {reserved_rows}");

    // --- what paint emits ----------------------------------------------
    let mut painter = Painter::new(WidthEngine::new(WidthConfig::default()));
    painter.register_media(Box::new(GfxMediaSink::new(doc, &base_dir)));

    let mut buf: Vec<u8> = Vec::new();
    // Paint enough frames to scroll the media region past the viewport.
    for scroll in 0..tree.line_count().saturating_sub(1).min(80) {
        let mut frame = Vec::new();
        painter
            .frame(
                &tree,
                scroll,
                Size {
                    width: 100,
                    height: 30,
                },
                &mut frame,
            )
            .expect("paint");
        buf.write_all(&frame).unwrap();
    }

    let text = String::from_utf8_lossy(&buf);
    let puts = text.matches("\x1b_Ga=p,").count();
    let transmits = text.matches("\x1b_Ga=t,").count();
    let with_c1 = text.matches(",C=1,").count();
    println!("\n=== emitted ===");
    println!("  transmits (a=t) : {transmits}");
    println!("  placements (a=p): {puts}");
    println!("  with C=1        : {with_c1}");

    // Placement columns: `CSI row;colH` immediately before each put.
    let re = regex_lite(&text);
    println!("  placements at column > 1 (inline): {}", re.1);
    for s in re.0.iter().take(10) {
        println!("    {s}");
    }
}

/// Tiny hand-rolled scan for `ESC [ row ; col H ESC _ G a=p,...` — avoids
/// pulling a regex dependency into the workspace for one example.
fn regex_lite(text: &str) -> (Vec<String>, usize) {
    let mut out = Vec::new();
    let mut inline = 0usize;
    let mut seen = std::collections::BTreeSet::new();
    for (idx, _) in text.match_indices("\x1b_Ga=p,") {
        // Walk back to the CSI ... H that precedes this put.
        let head = &text[..idx];
        let Some(csi) = head.rfind("\x1b[") else {
            continue;
        };
        let coords = &head[csi + 2..];
        let Some(hpos) = coords.find('H') else {
            continue;
        };
        let coords = &coords[..hpos];
        let Some((row, col)) = coords.split_once(';') else {
            continue;
        };
        let (Ok(row), Ok(col)) = (row.parse::<u32>(), col.parse::<u32>()) else {
            continue;
        };
        let tail = &text[idx..(idx + 60).min(text.len())];
        let put = tail.split('\u{1b}').next().unwrap_or("");
        if col > 1 {
            inline += 1;
        }
        if seen.insert((row, col, put.to_string())) {
            out.push(format!("row={row} col={col} {put}"));
        }
    }
    (out, inline)
}
