//! Diagnostic: emit three kitty-graphics placement variants to stdout so a
//! human in a real Ghostty window can report which (if any) actually draws a
//! square. The automated tests only prove the bytes are well-formed and
//! balanced — never that a pixel lands on screen. Run:
//!
//! ```sh
//! cargo run -p gfx --example kitty_probe
//! ```
//!
//! Each variant places a small solid square. Tell me which letters (A/B/C)
//! show a colored square beside them.

use std::io::{self, Write};

use base64::Engine as _;
use gfx::{CellRect, Emitter, ImageId};

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

/// A small solid-color PNG (20x20) — small enough to fit one transmission
/// chunk, so chunking can't be the variable under test.
fn png(rgb: [u8; 3]) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(20, 20, image::Rgba([rgb[0], rgb[1], rgb[2], 255]));
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

/// Raw 20x20 RGBA bytes for the f=32 variant (no PNG decode involved).
fn rgba(rgb: [u8; 3]) -> Vec<u8> {
    let mut v = Vec::with_capacity(20 * 20 * 4);
    for _ in 0..20 * 20 {
        v.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
    }
    v
}

fn main() {
    let mut out = io::stdout().lock();

    writeln!(
        out,
        "\nkitty graphics probe — which variants show a square?\n"
    )
    .unwrap();

    // Variant A: the PRODUCT's own Emitter — a=t transmit, then a=p place.
    write!(out, "A (product Emitter: a=t then a=p, PNG): ").unwrap();
    let mut em = Emitter::new();
    let id = ImageId::new(7001);
    em.transmit(id, &png([220, 40, 40]), &mut out);
    em.place(
        id,
        CellRect {
            x: 40,
            y: 0,
            width: 4,
            height: 2,
        },
        &mut out,
    );
    writeln!(out).unwrap();

    // Variant B: a=T (transmit AND display in one command), PNG, at cursor.
    write!(out, "B (a=T combined, PNG, at cursor):        ").unwrap();
    let payload = B64.encode(png([40, 200, 40]));
    write!(out, "\x1b_Ga=T,f=100,i=7002,q=2;{payload}\x1b\\").unwrap();
    writeln!(out).unwrap();

    // Variant C: a=T, raw RGBA f=32 with explicit source dimensions — the
    // exact shape the P1 spike sent and Ghostty accepted.
    write!(out, "C (a=T, raw RGBA f=32, s/v set):         ").unwrap();
    let payload = B64.encode(rgba([60, 120, 240]));
    write!(out, "\x1b_Ga=T,f=32,s=20,v=20,i=7003,q=2;{payload}\x1b\\").unwrap();
    writeln!(out, "\n").unwrap();

    out.flush().unwrap();
}
