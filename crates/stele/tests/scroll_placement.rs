//! Regression: a media box truncated by **scrolling** must be placed on the
//! rows it actually occupies on screen, with the source raster cropped to
//! match.
//!
//! Asserted on the exact bytes `Painter::frame` puts on the wire, at every
//! scroll offset, through the real layout → painter → sink → emitter path.
//! That level is the point of this file. The suite already had DW-6.1's
//! create/delete *balance* checks over 100 simulated scroll cycles, and they
//! were green throughout: the placements were balanced, capped, and swept
//! exactly as intended — they were simply at the wrong rows, at full box
//! height, painting the image down over the document text below it. A count
//! and a balance cannot see that. A row number and a `r=` can.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use layout::{LayoutConfig, Line, layout};
use stele::media::GfxMediaSink;
use stele::{ImageSizer, Painter, Size};
use width::{WidthConfig, WidthEngine};

/// The sink's assumed cell geometry (`GfxMediaSink::cell_px`), which both
/// the raster target and — because `ImageSizer` assumes the same — the box's
/// cell extent derive from. Fixing it here is what makes the expected source
/// rectangle exact rather than approximate.
const CELL_PX: (u32, u32) = (24, 48);

const VIEWPORT: Size = Size {
    width: 60,
    height: 10,
};

fn scratch_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "stele-scroll-placement-{tag}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// Writes a `w`x`h` pixel PNG so the box's cell extent is exactly
/// `(w / 24, h / 48)`.
fn write_png(dir: &Path, name: &str, w: u32, h: u32) {
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

/// The full `a=p` put command in `frame`, and the row the cursor was moved
/// to immediately before it (0-indexed, as `CellRect::y`).
/// The first image id this process's sink will allocate.
///
/// Not the literal `1`: an id carries a pid-derived instance tag so a second
/// stele on the same screen cannot transmit over this one's images (see
/// `gfx::IdAllocator`). Taken from the same allocator the sink uses, so the
/// byte-exact assertions below stay byte-exact.
fn first_id() -> u32 {
    gfx::IdAllocator::for_this_process().allocate().get()
}

fn put_and_row(frame: &str) -> Option<(String, u16)> {
    assert!(
        frame.matches("\x1b_Ga=p").count() <= 1,
        "a single-image frame must emit at most one placement"
    );
    let start = frame.find("\x1b_Ga=p")?;
    let end = frame[start..].find("\x1b\\")? + start + 2;
    let cup_start = frame[..start].rfind("\x1b[")?;
    let cup = &frame[cup_start + 2..start];
    let row: u16 = cup.split(';').next()?.parse().ok()?;
    Some((frame[start..end].to_string(), row.saturating_sub(1)))
}

struct Fixture {
    painter: Painter,
    tree: layout::LayoutTree,
    /// Tree line index of the box's first row.
    first: usize,
    /// Height of the box in cells.
    rows: u16,
    /// Width of the box in cells.
    cols: u16,
}

fn fixture(tag: &str, png_px: (u32, u32)) -> Fixture {
    let dir = scratch_dir(tag);
    write_png(&dir, "tall.png", png_px.0, png_px.1);
    // Text on both sides of the box: the defect this guards painted the
    // image over the trailing lines, so they have to exist.
    let src = "lead one\n\nlead two\n\n![tall](tall.png)\n\ntail one\n\ntail two\n\ntail three\n";
    let doc = ast::Document::parse(src);
    let engine = WidthEngine::new(WidthConfig::default());
    let config = LayoutConfig {
        min_width: 24,
        max_width: VIEWPORT.width,
    };
    let sizer = ImageSizer::new(&dir);
    let tree = layout(&doc, VIEWPORT.width, &config, &engine, &sizer);

    let mut first = None;
    let mut rows = 0;
    let mut cols = 0;
    for (i, line) in tree.lines(0..tree.line_count()).enumerate() {
        if let Line::Reserved(r) = line {
            first.get_or_insert(i);
            rows = r.boxed.rows;
            cols = r.boxed.cols;
        }
    }
    let mut painter = Painter::new(WidthEngine::new(WidthConfig::default()));
    painter.register_media(Box::new(GfxMediaSink::new(doc, &dir)));
    Fixture {
        painter,
        tree,
        first: first.expect("the image must reserve lines"),
        rows,
        cols,
    }
}

impl Fixture {
    fn frame(&mut self, scroll: usize) -> String {
        let mut buf = Vec::new();
        self.painter
            .frame(&self.tree, scroll, VIEWPORT, &mut buf)
            .expect("paint");
        String::from_utf8(buf).expect("painter output is utf-8")
    }
}

/// The whole defect in one assertion, swept across every scroll offset at
/// which the box is on screen.
///
/// At each offset the placement must sit on the box's first *visible* row
/// and claim exactly the rows the box actually occupies there — never the
/// full box height from wherever it happens to start. Where that is a
/// partial view, the source rectangle must crop the raster by the same
/// fraction, so the visible pixels stay on the cells they belong to.
#[test]
fn test_scroll_truncated_box_is_placed_and_cropped_to_its_visible_rows() {
    // 96x960 px at 24x48 per cell -> a 4-col, 20-row box: taller than the
    // 10-row viewport, so it is truncated at one edge or both at every
    // offset.
    let mut fx = fixture("truncated", (96, 960));
    assert_eq!((fx.cols, fx.rows), (4, 20), "fixture box extent");
    let last = fx.first + usize::from(fx.rows) - 1;
    let raster_h = u32::from(fx.rows) * CELL_PX.1;
    let raster_w = u32::from(fx.cols) * CELL_PX.0;

    for scroll in 0..=last {
        let frame = fx.frame(scroll);
        let top_row = fx.first.saturating_sub(scroll) as u16;
        // Rows of the box that scrolled above the viewport.
        let above = scroll.saturating_sub(fx.first) as u16;
        let visible = (fx.rows - above).min(VIEWPORT.height - top_row);

        let (put, row) = put_and_row(&frame)
            .unwrap_or_else(|| panic!("scroll {scroll}: no placement emitted at all"));

        assert_eq!(
            row, top_row,
            "scroll {scroll}: placement must sit on the box's first visible row"
        );
        // Proportional crop: the raster covers the whole `rows`-cell box, so
        // `above/rows` of its height is exactly `above` cell rows. No cell-
        // pixel query from the terminal is involved.
        let y = u32::from(above) * raster_h / u32::from(fx.rows);
        let h = u32::from(visible) * raster_h / u32::from(fx.rows);
        assert_eq!(
            put,
            format!(
                "\x1b_Ga=p,i={id},p={id},c={},r={visible},x=0,y={y},w={raster_w},h={h},C=1,q=2\x1b\\",
                fx.cols,
                id = first_id()
            ),
            "scroll {scroll}: wrong placement bytes"
        );
        // The specific regression, spelled out: the pre-fix sink emitted the
        // full box height at every offset, which is how the image ended up
        // painted over `tail one`/`tail two`/`tail three`.
        assert!(
            !put.contains(&format!("r={},", fx.rows)) || visible == fx.rows,
            "scroll {scroll}: placement claims the full box height while only \
             {visible} of {} rows are on screen: {put:?}",
            fx.rows
        );
    }
}

/// The complement, so the crop is not applied unconditionally: a box that
/// fits entirely on screen displays the whole raster, and the placement
/// omits the source keys altogether (kitty's defaults already mean "all of
/// it") — the exact bytes the emitter has always produced.
#[test]
fn test_fully_visible_box_places_the_whole_raster_uncropped() {
    // 48x96 px -> a 2-col, 2-row box, well inside the 10-row viewport.
    let mut fx = fixture("whole", (48, 96));
    assert_eq!((fx.cols, fx.rows), (2, 2), "fixture box extent");

    let frame = fx.frame(0);
    let (put, row) = put_and_row(&frame).expect("placement emitted");
    assert_eq!(row, fx.first as u16);
    assert_eq!(
        put,
        format!(
            "\x1b_Ga=p,i={id},p={id},c=2,r=2,C=1,q=2\x1b\\",
            id = first_id()
        )
    );
}

/// Scrolling a fully-visible box off the top edge one row at a time must
/// walk the crop down the raster in lock step — the transition case between
/// the two tests above, and the one the old code got wrong the instant the
/// box's first row left the viewport.
#[test]
fn test_crop_advances_one_raster_row_band_per_scrolled_off_row() {
    let mut fx = fixture("bands", (48, 96));
    let raster_band = 96 / u32::from(fx.rows); // pixels per cell row

    // Scroll until the box's top row is exactly at the viewport top, then
    // one further: the box is now top-truncated by one row.
    let at_top = fx.frame(fx.first);
    let (put_at_top, row_at_top) = put_and_row(&at_top).expect("placement emitted");
    assert_eq!(row_at_top, 0);
    assert_eq!(
        put_at_top,
        format!(
            "\x1b_Ga=p,i={id},p={id},c=2,r=2,C=1,q=2\x1b\\",
            id = first_id()
        )
    );

    let one_off = fx.frame(fx.first + 1);
    let (put_one_off, row_one_off) = put_and_row(&one_off).expect("placement emitted");
    assert_eq!(row_one_off, 0, "the surviving row is still at the top");
    assert_eq!(
        put_one_off,
        format!(
            "\x1b_Ga=p,i={id},p={id},c=2,r=1,x=0,y={raster_band},w=48,h={raster_band},C=1,q=2\x1b\\",
            id = first_id()
        ),
        "one scrolled-off row must skip exactly one raster band and claim one cell row"
    );
}
