//! Decisive probe for the image-scroll "ghost trail" bug.
//!
//! The trail appears because stele's repaint-behind model re-issues a put
//! (`a=p`) for each visible image every frame. That only avoids stacking if
//! the terminal *replaces* a placement re-put with the same `(image id,
//! placement id)` instead of drawing a second one. The repo has never
//! measured whether Ghostty actually does that — this probe measures it, plus
//! the delete-placement backstop, in one run.
//!
//! Run it INSIDE a real Ghostty window (not through a pipe or `!`):
//!
//! ```sh
//! cargo run -p gfx --example place_probe --release
//! ```
//!
//! Then read off two counts and tell me:
//!   TEST A — how many RED squares?   (1 = Ghostty replaces; 2 = it stacks)
//!   TEST B — how many BLUE squares?  (1 = delete-placement works; 2 = ignored)

use std::io::{self, BufRead, Write};

use gfx::{CellRect, Emitter, ImageId};

/// A small solid-color PNG — one transmission chunk, so chunking is not a
/// variable here.
fn png(rgb: [u8; 3]) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(48, 48, image::Rgba([rgb[0], rgb[1], rgb[2], 255]));
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

fn label(out: &mut dyn Write, row: u16, text: &str) {
    let _ = write!(out, "\x1b[{row};1H\x1b[2K{text}");
}

fn main() {
    let mut out = io::stdout().lock();
    let mut em = Emitter::new();

    // Alt screen, clear, hide cursor.
    write!(out, "\x1b[?1049h\x1b[2J\x1b[?25l").unwrap();

    label(
        &mut out,
        1,
        "stele placement probe — count the colored squares in each test.",
    );

    // ── TEST A: replacement ────────────────────────────────────────────────
    // Same (i, p) put twice at two different rows. Spec says the second put
    // replaces (moves) the first → ONE square. If Ghostty stacks → TWO.
    label(
        &mut out,
        3,
        "TEST A  same (i,p) put at two rows  ->  EXPECT 1 red square (2 = Ghostty stacks, the bug):",
    );
    let a = ImageId::new(9001);
    em.transmit(a, &png([220, 40, 40]), &mut out);
    em.place(
        a,
        CellRect {
            x: 6,
            y: 4,
            width: 6,
            height: 3,
        },
        &mut out,
    ); // rows ~5-7
    em.place(
        a,
        CellRect {
            x: 6,
            y: 8,
            width: 6,
            height: 3,
        },
        &mut out,
    ); // rows ~9-11, SAME (i,p)

    // ── TEST B: delete-placement backstop ─────────────────────────────────
    // Place, then clear_placements (a=d,d=i), then place at a new row. If the
    // delete is honored → ONE square. If ignored → TWO.
    label(
        &mut out,
        13,
        "TEST B  put, delete-placement, put  ->  EXPECT 1 blue square (2 = a=d,d=i ignored):",
    );
    let b = ImageId::new(9002);
    em.transmit(b, &png([40, 120, 240]), &mut out);
    em.place(
        b,
        CellRect {
            x: 6,
            y: 14,
            width: 6,
            height: 3,
        },
        &mut out,
    ); // rows ~15-17
    em.clear_placements(b, &mut out);
    em.place(
        b,
        CellRect {
            x: 6,
            y: 18,
            width: 6,
            height: 3,
        },
        &mut out,
    ); // rows ~19-21

    label(
        &mut out,
        23,
        "Count RED (test A) and BLUE (test B) squares, then press Enter to quit.",
    );
    out.flush().unwrap();

    // Wait for Enter (line-buffered; no raw mode needed).
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).ok();

    // Restore: leave alt screen, show cursor.
    write!(out, "\x1b[?25h\x1b[?1049l").unwrap();
    out.flush().unwrap();
}
