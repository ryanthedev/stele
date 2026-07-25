//! Bug-bash probe: paint ONE frame and print the wire with every escape
//! sequence made visible, so "which column did that land in" is readable.
//!
//! Run: `cargo run -p stele --example wire_dump -- <file.md> <width> <height> [scroll]`

use std::fmt::Write as _;

use stele::media::GfxMediaSink;
use stele::{ImageSizer, Painter, Size};
use width::{WidthConfig, WidthEngine};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().cloned().expect("path");
    let width: u16 = args.get(1).and_then(|w| w.parse().ok()).unwrap_or(60);
    let height: u16 = args.get(2).and_then(|w| w.parse().ok()).unwrap_or(24);
    let scroll: usize = args.get(3).and_then(|w| w.parse().ok()).unwrap_or(0);

    let src = std::fs::read_to_string(&path).expect("readable");
    let base_dir = std::path::Path::new(&path)
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let doc = ast::Document::parse(&src);
    let engine = WidthEngine::new(WidthConfig::default());
    let config = layout::LayoutConfig {
        min_width: 1,
        max_width: width.max(1),
    };
    let sizer = ImageSizer::new(&base_dir);
    let tree = layout::layout(&doc, width, &config, &engine, &sizer);

    let mut painter = Painter::new(WidthEngine::new(WidthConfig::default()));
    painter.register_media(Box::new(GfxMediaSink::new(doc, &base_dir)));
    let mut frame: Vec<u8> = Vec::new();
    painter
        .frame(&tree, scroll, Size { width, height }, &mut frame)
        .expect("paint");

    let text = String::from_utf8_lossy(&frame);
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                out.push_str("<CSI ");
                for c in chars.by_ref().skip(1) {
                    out.push(c);
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
                out.push('>');
                if out.ends_with("H>") {
                    // a cursor move starts a new visual line in the dump
                    out.push('\n');
                }
            }
            Some('_') => {
                // Kitty APC — keep only the key/value head, drop the payload.
                let mut head = String::new();
                let mut prev = '\0';
                for c in chars.by_ref() {
                    if prev == '\u{1b}' && c == '\\' {
                        break;
                    }
                    if c != '\u{1b}' && head.len() < 60 {
                        head.push(c);
                    }
                    prev = c;
                }
                let _ = write!(out, "<APC {head}…>");
            }
            Some(']') => {
                let mut head = String::new();
                let mut prev = '\0';
                for c in chars.by_ref() {
                    if (prev == '\u{1b}' && c == '\\') || c == '\u{7}' {
                        break;
                    }
                    if c != '\u{1b}' && head.len() < 60 {
                        head.push(c);
                    }
                    prev = c;
                }
                let _ = write!(out, "<OSC {head}>");
            }
            _ => out.push_str("<ESC>"),
        }
    }
    println!("{out}");
}
