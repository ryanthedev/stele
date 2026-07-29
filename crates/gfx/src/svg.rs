//! SVG: the one *vector* format on the media path, and the only one whose
//! input is a program rather than a picture.
//!
//! Everything in [`crate::decode`] answers to the same contract — a hostile
//! file must fail cleanly, and nothing may allocate unboundedly — so this
//! module answers to it too. It just has to earn it differently, because the
//! threats are different. A raster bomb is a header claiming a huge size, and
//! `image` refuses it. An SVG has three separate ways to be hostile:
//!
//!   1. **Entity expansion** (billion laughs). Bounded, but not by us:
//!      `roxmltree`'s `LoopDetector` caps reference *depth* at 10 and total
//!      references resolved per root reference at 255, so expansion is linear
//!      in the source — about 255x — rather than exponential. Read from
//!      `roxmltree-0.21.1/src/parse.rs:500`, not assumed. Paired with
//!      [`MAX_SVG_BYTES`] that makes the expanded document bounded outright.
//!   2. **Node count.** `usvg` parses with `nodes_limit` at its default of
//!      `u32::MAX` — no limit at all (`usvg-0.47.0/src/parser/mod.rs:147`).
//!      That is why this module does not call `usvg::Tree::from_data`: it
//!      parses the XML itself with [`MAX_XML_NODES`] set and hands the
//!      finished document to `Tree::from_xmltree`, which is the seam `usvg`
//!      exposes for exactly this.
//!   3. **Canvas size.** A `viewBox` can claim any dimensions it likes. The
//!      declared size is checked against `Limits` like a raster header, and
//!      then ignored for allocation purposes anyway — see [`rasterize`],
//!      which never makes a pixmap bigger than the caller's target box.
//!
//! What this does *not* accept, deliberately: `.svgz`. Decompressing it is
//! `usvg::Tree::from_data`'s job, and that entry point is the one that parses
//! with no node limit. Keeping the hardened seam means keeping the text one.

use std::sync::{Arc, OnceLock};

// `usvg` and `tiny_skia` are reached through `resvg` rather than depended on
// directly, so their versions cannot drift out of step with it — `from_xmltree`
// takes *resvg's* `roxmltree::Document`, and two copies of that crate in the
// tree would be two incompatible types with one name.
use resvg::tiny_skia;
use resvg::usvg;
use resvg::usvg::roxmltree;

use crate::decode::{DecodeError, Limits};

/// The largest SVG source this will parse.
///
/// An SVG is text, and a hand-written or tool-exported one is kilobytes. Four
/// megabytes is far past any real diagram and is the number that turns
/// `roxmltree`'s "expansion is linear in the source" into an absolute ceiling
/// rather than a relative one.
pub(crate) const MAX_SVG_BYTES: usize = 4 * 1024 * 1024;

/// The largest parsed XML node count.
///
/// `usvg` sets no limit of its own, so this is the whole of that bound. A
/// complex real diagram — a full architecture drawing, a rendered Mermaid
/// graph — lands in the low thousands; two hundred thousand is generous
/// enough that no honest document meets it and small enough that a
/// pathological one stops early.
const MAX_XML_NODES: u32 = 200_000;

/// Whether `prefix` looks like SVG source.
///
/// Content sniffing, like every other format here — the extension is never
/// consulted. An SVG document may open with a BOM, an XML declaration, a
/// DOCTYPE, comments or whitespace before the root element, so this looks for
/// the `<svg` tag itself within the prefix rather than trying to model the
/// prologue. Anything claiming to be SVG but lacking an `<svg` element inside
/// the first kilobyte is not one worth rendering.
pub(crate) fn looks_like_svg(prefix: &[u8]) -> bool {
    let head = &prefix[..prefix.len().min(1024)];
    // `<svg` must be followed by a delimiter, or `<svgfoo` would match.
    head.windows(4)
        .enumerate()
        .filter(|(_, w)| w.eq_ignore_ascii_case(b"<svg"))
        .any(|(i, _)| {
            head.get(i + 4)
                .is_none_or(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/')
        })
}

/// The font set SVG text is shaped with, loaded once for the life of the
/// process.
///
/// Text in an SVG is converted to paths during parsing, so a `usvg::Options`
/// with an empty database renders a diagram with every label missing — which
/// for the documents SVG is actually used for (architecture drawings, flow
/// charts, anything exported from a diagramming tool) is most of the picture.
/// So the system fonts are loaded — but only for a drawing that actually has
/// text in it, and only once for the process.
///
/// Both halves of that are load-bearing, because the cost is not small.
/// Measured on this machine: 516 ms for the first text-bearing SVG against
/// 8 ms for every render after it. That half second lands during *layout*, on
/// the thread running the event loop, so paying it for a logo or an icon —
/// which is most of the SVG in real documents, and has its text converted to
/// paths by the exporting tool anyway — would be a visible stall bought for
/// nothing. [`needs_fonts`] is what makes those free.
fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    Arc::clone(FONTS.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    }))
}

/// The empty database a text-free drawing parses against.
fn no_fonts() -> Arc<usvg::fontdb::Database> {
    static EMPTY: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    Arc::clone(EMPTY.get_or_init(|| Arc::new(usvg::fontdb::Database::new())))
}

/// Whether `source` can contain text that needs shaping.
///
/// SVG has exactly one way to render glyphs — a `<text>` element — so the
/// absence of that substring is proof the font database will never be read.
/// A `<tspan>` and a `<textPath>` are both only legal inside one, but they are
/// checked anyway: the cost of being wrong in this direction is one
/// unnecessary font load, and the cost of being wrong in the other is a
/// diagram with no labels.
fn needs_fonts(source: &str) -> bool {
    source.contains("<text") || source.contains("<tspan")
}

/// Parses `source` into a `usvg` tree under this module's limits.
///
/// The two-step parse — `roxmltree` first, `usvg::Tree::from_xmltree` second
/// — is the whole point of this function and the reason
/// `usvg::Tree::from_str` is not called. `from_str` builds its own
/// `ParsingOptions` with `nodes_limit` unset, and there is no way to reach in
/// and change it.
fn parse(source: &str) -> Result<usvg::Tree, DecodeError> {
    let xml_options = roxmltree::ParsingOptions {
        // A DOCTYPE is left allowed on purpose. Refusing it would harden
        // nothing this does not already bound — expansion is capped by
        // `roxmltree`'s own loop detector — and would reject the many real
        // SVGs that older versions of Illustrator and Inkscape emit with one.
        allow_dtd: true,
        nodes_limit: MAX_XML_NODES,
        ..Default::default()
    };
    let document = roxmltree::Document::parse_with_options(source, xml_options)
        .map_err(|e| DecodeError::Malformed(format!("svg: {e}")))?;

    let options = usvg::Options {
        fontdb: if needs_fonts(source) {
            fonts()
        } else {
            no_fonts()
        },
        ..Default::default()
    };
    usvg::Tree::from_xmltree(&document, &options)
        .map_err(|e| DecodeError::Malformed(format!("svg: {e}")))
}

/// The SVG's declared pixel size, for layout to reserve a box from.
///
/// This is the vector counterpart of the raster header probe, and it is
/// honestly more expensive: an SVG has no header to read, so learning its size
/// means parsing it. Parsing is still far cheaper than rasterizing, which is
/// the distinction the layout-time probe actually cares about.
pub(crate) fn probe(source: &str, limits: Limits) -> Result<(u32, u32), DecodeError> {
    let tree = parse(source)?;
    dimensions(&tree, limits)
}

/// A tree's size as whole pixels, checked against `limits`.
fn dimensions(tree: &usvg::Tree, limits: Limits) -> Result<(u32, u32), DecodeError> {
    let size = tree.size();
    // `ceil`, so a 10.2-pixel drawing reserves 11 and is never clipped by the
    // rounding. `to_u32` saturates rather than wrapping on a NaN or a huge
    // float, and the `accepts` check below refuses whatever comes out.
    let (width, height) = (size.width().ceil() as u32, size.height().ceil() as u32);
    if !limits.accepts(width, height) {
        return Err(DecodeError::ExceedsLimits { width, height });
    }
    Ok((width, height))
}

/// Renders `source` into an image no larger than `target_px`.
///
/// The pixmap is allocated at the **fitted** size — the SVG's aspect ratio
/// scaled to sit inside the caller's box — never at the drawing's declared
/// size. That is a stronger bound than the raster path manages: a raster is
/// decoded at its natural size and scaled afterwards, so a legal 8000x8000
/// PNG really does allocate 256 MiB on the way through, while an 8000x8000
/// SVG allocates only the target box. Being able to rasterize straight to the
/// size wanted is the one thing vector input makes easier rather than harder.
///
/// The result still goes through the shared letterbox in [`crate::decode`],
/// so "exactly the target box, aspect preserved, centred" stays one
/// guarantee stated in one place rather than two implementations that agree
/// today.
pub(crate) fn rasterize(
    source: &str,
    target_px: (u32, u32),
    limits: Limits,
) -> Result<Rasterized, DecodeError> {
    let tree = parse(source)?;
    let (width, height) = dimensions(&tree, limits)?;
    let (fit_w, fit_h) = fit(width, height, target_px.0.max(1), target_px.1.max(1));

    let mut pixmap = tiny_skia::Pixmap::new(fit_w, fit_h).ok_or(DecodeError::ExceedsLimits {
        width: fit_w,
        height: fit_h,
    })?;
    // One uniform scale for both axes: `fit` preserved the aspect ratio, so a
    // per-axis scale here would be the same number twice, and any difference
    // between them would be a distortion nobody asked for.
    let scale = f32::min(
        fit_w as f32 / width.max(1) as f32,
        fit_h as f32 / height.max(1) as f32,
    );
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    let buffer = image::RgbaImage::from_raw(fit_w, fit_h, pixmap.take()).ok_or_else(|| {
        DecodeError::Malformed("svg: rasterized buffer did not match its own size".to_string())
    })?;
    Ok(Rasterized {
        source: (width, height),
        image: image::DynamicImage::ImageRgba8(buffer),
    })
}

/// A rendered drawing and the size it was drawn *at*.
///
/// The two are reported separately because they are different facts and the
/// caller needs both: `DecodedImage::source_width` is documented as the
/// source's own dimensions before any scaling, and for a vector drawing the
/// rasterized buffer is never those — it is the fitted box. Returning only
/// the image would silently make a 120x60 drawing report itself as 40x20,
/// which is what happened before this type existed.
pub(crate) struct Rasterized {
    /// The drawing's declared canvas, before fitting.
    pub source: (u32, u32),
    pub image: image::DynamicImage,
}

/// `(width, height)` scaled to fit inside `(max_w, max_h)` without changing
/// its aspect ratio, never scaled *up* past its own size and never zero on
/// either axis.
fn fit(width: u32, height: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let (w, h) = (width.max(1), height.max(1));
    let scale = f64::min(
        f64::from(max_w) / f64::from(w),
        f64::from(max_h) / f64::from(h),
    );
    // Vector input can be enlarged without going soft, but the target box is
    // the caller's statement of how many pixels the cell grid can show — past
    // it, every extra pixel is thrown away by the letterbox.
    let scale = scale.min(1.0);
    (
        ((f64::from(w) * scale).round() as u32).max(1),
        ((f64::from(h) * scale).round() as u32).max(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const CIRCLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="60">
        <rect width="120" height="60" fill="#3a78c8"/>
        <circle cx="60" cy="30" r="24" fill="#fac83c"/>
    </svg>"##;

    #[test]
    fn test_sniffing_finds_svg_after_a_prologue_and_rejects_near_misses() {
        assert!(looks_like_svg(CIRCLE.as_bytes()));
        assert!(looks_like_svg(
            b"<?xml version=\"1.0\"?>\n<!-- a comment -->\n<svg xmlns=\"x\">"
        ));
        assert!(looks_like_svg(b"\xef\xbb\xbf<svg>"));
        assert!(
            looks_like_svg(b"<SVG WIDTH=\"1\">"),
            "tags are case-insensitive"
        );

        assert!(!looks_like_svg(b"<svgfoo>"), "`<svg` needs a delimiter");
        assert!(!looks_like_svg(b"\x89PNG\r\n\x1a\n"));
        assert!(!looks_like_svg(b""));
        // An `<svg` past the sniff window is not one this will find, and that
        // is the documented behaviour rather than an accident.
        let buried = format!("{}<svg>", " ".repeat(2000));
        assert!(!looks_like_svg(buried.as_bytes()));
    }

    #[test]
    fn test_a_drawing_probes_at_its_declared_size() {
        assert_eq!(probe(CIRCLE, Limits::default()).expect("probes"), (120, 60));
    }

    #[test]
    fn test_rasterizing_fits_the_target_box_and_keeps_the_aspect_ratio() {
        let out = rasterize(CIRCLE, (60, 60), Limits::default()).expect("renders");
        // 120x60 into a 60x60 box is 60x30, not 60x60 — the letterbox in
        // `decode` adds the empty rows, and it is the only thing that should.
        assert_eq!((out.image.width(), out.image.height()), (60, 30));
        assert_eq!(
            out.source,
            (120, 60),
            "the declared canvas is reported as-is"
        );
    }

    #[test]
    fn test_the_pixmap_never_exceeds_the_target_however_large_the_drawing() {
        let huge = r##"<svg xmlns="http://www.w3.org/2000/svg" width="8000" height="8000"><rect width="8000" height="8000" fill="red"/></svg>"##;
        let out = rasterize(huge, (32, 32), Limits::default()).expect("renders");
        assert_eq!((out.image.width(), out.image.height()), (32, 32));
        assert_eq!(out.source, (8000, 8000));
    }

    #[test]
    fn test_a_canvas_past_the_limits_is_refused_like_a_bomb_header() {
        let bomb = r##"<svg xmlns="http://www.w3.org/2000/svg" width="99999" height="99999"><rect width="10" height="10"/></svg>"##;
        let err = probe(bomb, Limits::default()).expect_err("a huge canvas is refused");
        assert!(matches!(err, DecodeError::ExceedsLimits { .. }), "{err:?}");
    }

    /// The node-count bound, which is entirely this module's — `usvg` sets
    /// none. Built as a wide, shallow document so it is the *count* under
    /// test rather than recursion depth.
    #[test]
    fn test_a_document_past_the_node_limit_is_refused() {
        let mut source =
            String::from(r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">"#);
        for _ in 0..(MAX_XML_NODES + 1_000) {
            source.push_str("<g/>");
        }
        source.push_str("</svg>");
        let err = probe(&source, Limits::default()).expect_err("too many nodes");
        assert!(matches!(err, DecodeError::Malformed(_)), "{err:?}");
    }

    /// Billion laughs. `roxmltree` is what bounds this, and the bound is
    /// asserted rather than trusted: a document whose entities would expand
    /// exponentially must come back as an error, not as gigabytes.
    #[test]
    fn test_an_entity_bomb_is_refused_rather_than_expanded() {
        let bomb = r##"<?xml version="1.0"?>
<!DOCTYPE svg [
  <!ENTITY a "aaaaaaaaaa">
  <!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;">
  <!ENTITY c "&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;">
  <!ENTITY d "&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;">
  <!ENTITY e "&d;&d;&d;&d;&d;&d;&d;&d;&d;&d;">
  <!ENTITY f "&e;&e;&e;&e;&e;&e;&e;&e;&e;&e;">
  <!ENTITY g "&f;&f;&f;&f;&f;&f;&f;&f;&f;&f;">
]>
<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><text>&g;</text></svg>"##;
        let err = probe(bomb, Limits::default()).expect_err("an entity bomb is refused");
        assert!(matches!(err, DecodeError::Malformed(_)), "{err:?}");
    }

    #[test]
    fn test_only_a_drawing_with_text_asks_for_fonts() {
        assert!(!needs_fonts(CIRCLE), "shapes alone need no font database");
        assert!(needs_fonts(r##"<svg><text x="0" y="0">hi</text></svg>"##));
        assert!(needs_fonts(
            r##"<svg><text><tspan>hi</tspan></text></svg>"##
        ));
        assert!(needs_fonts(
            r##"<svg><textPath href="#p">hi</textPath></svg>"##
        ));
    }

    /// Text still renders. The check above is an optimisation, and an
    /// optimisation that silently dropped every label would be the worst
    /// possible outcome for the documents SVG is actually used for — so this
    /// asserts ink lands where a glyph should be.
    #[test]
    fn test_text_in_a_drawing_is_actually_drawn() {
        let labelled = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="60">
            <text x="4" y="40" font-size="40" fill="#ffffff">HHHH</text>
        </svg>"##;
        let out = rasterize(labelled, (200, 60), Limits::default()).expect("renders");
        let rgba = out.image.to_rgba8();
        let inked = rgba.pixels().filter(|p| p.0[3] > 0).count();
        assert!(
            inked > 200,
            "expected glyph coverage, found {inked} inked pixels — fonts are not reaching text"
        );
    }

    #[test]
    fn test_malformed_source_is_an_error_and_never_a_panic() {
        for source in [
            "<svg",
            "<svg></rect>",
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"-5\" height=\"x\"></svg>",
            "",
        ] {
            assert!(
                probe(source, Limits::default()).is_err(),
                "{source:?} should not parse"
            );
        }
    }
}
