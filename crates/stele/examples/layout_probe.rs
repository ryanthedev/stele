//! Bug-bash probe: dump every laid-out line with its *re-measured* width, so a
//! run whose cached `Run.width` disagrees with the width engine shows up.
//!
//! Run: `cargo run -p stele --example layout_probe -- <file.md> <width> [minw] [--media]`

use layout::{Line, LineItem, NullSizer, layout};
use stele::ImageSizer;
use width::{WidthConfig, WidthEngine};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().cloned().expect("path");
    let width: u16 = args.get(1).and_then(|w| w.parse().ok()).unwrap_or(80);
    let minw: u16 = args.get(2).and_then(|w| w.parse().ok()).unwrap_or(1);
    let media = args.iter().any(|a| a == "--media");
    let ambiguous_wide = args.iter().any(|a| a == "--ambiguous-wide");

    let src = std::fs::read_to_string(&path).expect("readable");
    let base_dir = std::path::Path::new(&path)
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let doc = ast::Document::parse(&src);
    let engine = WidthEngine::new(WidthConfig { ambiguous_wide });
    let config = layout::LayoutConfig {
        min_width: minw,
        max_width: width.max(minw),
    };
    let image_sizer = ImageSizer::new(&base_dir);
    let null = NullSizer;
    let sizer: &dyn layout::IntrinsicSizer = if media { &image_sizer } else { &null };
    let tree = layout(&doc, width, &config, &engine, sizer);

    println!(
        "tree width = {} lines = {}",
        tree.width(),
        tree.line_count()
    );
    let mut bad = 0;
    for (i, line) in tree.lines(0..tree.line_count()).enumerate() {
        let (cached, measured, text) = match line {
            Line::Items(items) => {
                let cached: u16 = items
                    .iter()
                    .fold(0u16, |a, it| a.saturating_add(it.width()));
                let measured: u16 = items.iter().fold(0u16, |a, it| {
                    let w = match it {
                        LineItem::Run(r) => {
                            engine.display_width(&r.text).min(u16::MAX as usize) as u16
                        }
                        LineItem::Box(b) => b.cols,
                    };
                    a.saturating_add(w)
                });
                let text: String = items
                    .iter()
                    .map(|it| match it {
                        LineItem::Run(r) => r.text.clone(),
                        LineItem::Box(b) => format!("«BOX {}x{}»", b.cols, b.rows),
                    })
                    .collect();
                (cached, measured, text)
            }
            // The container prefix counts toward the line's width and toward
            // where the box starts, so it has to be reported here too —
            // reporting `cols` alone under-states a quoted or listed box by
            // the whole gutter and hides the column the box is drawn at.
            Line::Reserved(r) => {
                let cached: u16 = r.prefix_width().saturating_add(r.boxed.cols);
                let measured: u16 = r
                    .prefix
                    .iter()
                    .fold(0u16, |a, run| {
                        a.saturating_add(
                            engine.display_width(&run.text).min(u16::MAX as usize) as u16
                        )
                    })
                    .saturating_add(r.boxed.cols);
                let prefix_text: String = r.prefix.iter().map(|run| run.text.as_str()).collect();
                (
                    cached,
                    measured,
                    format!(
                        "{prefix_text}«RESERVED {}x{} node={} col={}»",
                        r.boxed.cols,
                        r.boxed.rows,
                        r.boxed.node_id.index(),
                        cached - r.boxed.cols,
                    ),
                )
            }
        };
        let flag = if measured > tree.width() {
            bad += 1;
            "OVER"
        } else if measured != cached {
            bad += 1;
            "MISM"
        } else {
            "    "
        };
        println!("{flag} {i:>4} c={cached:<4} m={measured:<4} |{text}|");
    }
    println!("problem lines: {bad}");
}
