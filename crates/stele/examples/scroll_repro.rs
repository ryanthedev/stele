//! Debug repro for the top-truncated placement defect.
//!
//! Paints one tall reserved box at a sequence of scroll offsets and reports,
//! per frame, where the kitty placement actually landed (the CUP row that
//! precedes it) versus where the box's visible rows actually are.
//!
//! Run: `cargo run -p stele --example scroll_repro`

use std::io::Write;

use layout::{Line, layout};
use stele::media::GfxMediaSink;
use stele::{ImageSizer, Painter, Size};
use width::{WidthConfig, WidthEngine};

const VIEWPORT_H: u16 = 12;
const VIEWPORT_W: u16 = 60;

fn main() {
    let base_dir = std::path::PathBuf::from("testdocs");
    let src = "\
# Before

filler one
filler two

![tall banner](img/tall.png)

AFTER-A
AFTER-B
AFTER-C
AFTER-D
AFTER-E
";

    let doc = ast::Document::parse(src);
    let engine = WidthEngine::new(WidthConfig::default());
    let config = layout::LayoutConfig {
        min_width: 24,
        max_width: VIEWPORT_W,
    };
    let sizer = ImageSizer::new(&base_dir);
    let tree = layout(&doc, VIEWPORT_W, &config, &engine, &sizer);

    // Where do the box's reserved rows actually live in the tree?
    let mut reserved_rows = Vec::new();
    for (i, line) in tree.lines(0..tree.line_count()).enumerate() {
        if let Line::Reserved(r) = line {
            reserved_rows.push((i, r.rows, r.cols));
        }
    }
    let first = reserved_rows.first().map(|r| r.0).unwrap_or(0);
    let last = reserved_rows.last().map(|r| r.0).unwrap_or(0);
    let box_rows = reserved_rows.first().map(|r| r.1).unwrap_or(0);
    println!(
        "tree: {} lines; box occupies tree lines {}..={} ({} rows tall)\n",
        tree.line_count(),
        first,
        last,
        box_rows
    );

    let mut painter = Painter::new(WidthEngine::new(WidthConfig::default()));
    painter.register_media(Box::new(GfxMediaSink::new(doc, &base_dir)));

    println!(
        "{:>6}  {:>14}  {:>14}  {:>10}  verdict",
        "scroll", "box rows vis", "placed at row", "placed r="
    );
    println!("{}", "-".repeat(74));

    for scroll in 0..=(last + 2) {
        let mut frame = Vec::new();
        painter
            .frame(
                &tree,
                scroll,
                Size {
                    width: VIEWPORT_W,
                    height: VIEWPORT_H,
                },
                &mut frame,
            )
            .expect("paint");
        let text = String::from_utf8_lossy(&frame).to_string();

        // Which screen rows does the box legitimately occupy this frame?
        let vis: Vec<u16> = (0..VIEWPORT_H)
            .filter(|row| {
                let idx = scroll + *row as usize;
                idx >= first && idx <= last
            })
            .collect();
        let vis_desc = match (vis.first(), vis.last()) {
            (Some(a), Some(b)) => format!("{a}..={b}"),
            _ => "none".to_string(),
        };

        // The CUP row in effect when the placement was emitted, and its r=.
        let placed = find_placement(&text);
        let (placed_row, placed_r) = match &placed {
            Some((row, r)) => (row.to_string(), r.clone()),
            None => ("-".to_string(), "-".to_string()),
        };

        let verdict = match (&placed, vis.first(), vis.last()) {
            (None, _, _) if vis.is_empty() => "ok (no box on screen)".to_string(),
            (None, Some(_), _) => "MISSING".to_string(),
            (Some((row, r)), Some(top), Some(bot)) => {
                let rows_visible = bot - top + 1;
                let r_num: u16 = r.parse().unwrap_or(0);
                if row != top {
                    format!("WRONG ROW (want {top}, got {row})")
                } else if r_num > rows_visible {
                    format!("OVERFLOWS {} rows past box", r_num - rows_visible)
                } else {
                    "ok".to_string()
                }
            }
            _ => "n/a".to_string(),
        };

        println!("{scroll:>6}  {vis_desc:>14}  {placed_row:>14}  {placed_r:>10}  {verdict}");
        let _ = std::io::stdout().flush();
    }
}

/// Returns (cup_row, r_value) for the first `a=p` placement in the frame.
/// `cup_row` is the row from the most recent `ESC [ <row> ; <col> H` before it.
fn find_placement(text: &str) -> Option<(u16, String)> {
    let put = text.find("\x1b_Ga=p,")?;
    let before = &text[..put];
    let cup_start = before.rfind("\x1b[")?;
    let cup = &before[cup_start + 2..];
    let row: u16 = cup
        .split([';', 'H'])
        .next()?
        .parse()
        .ok()
        .map(|r: u16| r.saturating_sub(1))?; // CUP is 1-based; report 0-based

    let tail = &text[put..];
    let end = tail.find('\x1b').map(|_| tail.len()).unwrap_or(tail.len());
    let params = &tail[..end.min(200)];
    let r = params
        .split(',')
        .find_map(|kv| kv.strip_prefix("r="))
        .map(|v| {
            v.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
        })
        .unwrap_or_else(|| "?".to_string());
    Some((row, r))
}
