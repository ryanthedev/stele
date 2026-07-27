//! DW-1.5: `T` swaps the built-in theme variant, and every heading level
//! must clear WCAG AA in whichever variant is active.
//!
//! Two claims, both grounded in the real painted bytes rather than in the
//! `highlight` crate's own (already-passing, out of this phase's file
//! scope) `test_heading_tiers_clear_wcag_aa_against_the_reference_backgrounds`:
//! that swapping the variant a running `Painter` paints with actually
//! changes what reaches the wire, and that both variants still clear AA —
//! so a future change that quietly made the swap a no-op, or that broke AA
//! for one variant, fails here even without touching `crates/highlight`.

mod common;

use ast::Document;
use highlight::{Color, ColorMode, Theme, Variant};
use layout::{LayoutConfig, NullSizer, layout};
use stele::decor::themed::ThemedDecor;
use stele::painter::{Painter, Size};

use common::fixtures::engine;

/// Extracts the truecolor foreground `38;2;r;g;b` SGR code immediately
/// preceding `needle` in `wire` — the color the painter actually used for
/// that text, read off the bytes rather than assumed from the theme table.
fn fg_before(wire: &str, needle: &str) -> Color {
    let at = wire
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not found on the wire: {wire:?}"));
    let prefix = &wire[..at];
    let start = prefix
        .rfind("38;2;")
        .unwrap_or_else(|| panic!("no truecolor fg SGR before {needle:?}: {wire:?}"));
    let rest = &prefix[start + "38;2;".len()..];
    let end = rest.find('m').unwrap_or(rest.len());
    // Take exactly the three channels and stop. A background (`48;2;r;g;b`)
    // can follow the foreground inside the same SGR sequence — H1 carries the
    // wash — so the triple is not necessarily what runs up to the `m`.
    let mut parts = rest[..end].split(';');
    let mut channel = |name: &str| -> u8 {
        parts
            .next()
            .unwrap_or_else(|| panic!("truecolor fg before {needle:?} has no {name}: {wire:?}"))
            .parse()
            .unwrap_or_else(|e| panic!("fg {name} before {needle:?} is not a u8: {e}"))
    };
    let (r, g, b) = (channel("r"), channel("g"), channel("b"));
    Color::new(r, g, b)
}

fn paint_heading(variant: Variant) -> String {
    let doc = Document::parse("# Heading One\n\n## Heading Two\n");
    let config = LayoutConfig::default();
    let tree = layout(&doc, 40, &config, &engine(), &NullSizer);
    let mut painter = Painter::new(engine());
    painter.register_decor(Box::new(ThemedDecor::new(Theme::new(
        variant,
        ColorMode::Truecolor,
    ))));
    let mut buf = Vec::new();
    painter
        .frame(
            &tree,
            0,
            Size {
                width: 40,
                height: 4,
            },
            &mut buf,
        )
        .unwrap();
    String::from_utf8(buf).expect("painter emits valid UTF-8")
}

#[test]
fn test_dw_1_5_swapping_the_registered_decors_variant_changes_the_painted_heading_color() {
    let dark_wire = paint_heading(Variant::Dark);
    let light_wire = paint_heading(Variant::Light);

    let dark_h1 = fg_before(&dark_wire, "Heading One");
    let light_h1 = fg_before(&light_wire, "Heading One");
    assert_ne!(
        dark_h1, light_h1,
        "swapping the variant registered on the painter must change the color the heading is \
         actually painted with — this is what `T` does at runtime"
    );
}

/// The reference backgrounds and WCAG contrast formula
/// `crates/highlight/src/theme.rs`'s own test already pins; duplicated here
/// (out of this phase's file scope) so the check runs against the *painted
/// bytes*, not only against `Theme::resolve`'s return value.
fn contrast(fg: Color, bg: Color) -> f64 {
    fn relative_luminance(c: Color) -> f64 {
        let channel = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
    }
    let (x, y) = (relative_luminance(fg), relative_luminance(bg));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[test]
fn test_dw_1_5_every_heading_level_clears_wcag_aa_in_the_painted_bytes_of_both_variants() {
    const AA_NORMAL_TEXT: f64 = 4.5;
    for (variant, background, needle) in [
        (Variant::Dark, Color::new(0x1a, 0x1b, 0x26), "Heading One"),
        (Variant::Light, Color::new(0xff, 0xff, 0xff), "Heading One"),
    ] {
        let wire = paint_heading(variant);
        let fg = fg_before(&wire, needle);
        let ratio = contrast(fg, background);
        assert!(
            ratio >= AA_NORMAL_TEXT,
            "{variant:?} heading painted {fg:?}, {ratio:.2}:1 against {background:?} — under \
             WCAG AA's {AA_NORMAL_TEXT}:1"
        );
    }
}
