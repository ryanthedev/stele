//! SVG: the one *vector* format on the media path, and the only one whose
//! input is a program rather than a picture.
//!
//! Everything in [`crate::decode`] answers to the same contract — a hostile
//! file must fail cleanly, and nothing may allocate unboundedly — so this
//! module answers to it too. It just has to earn it differently, because the
//! threats are different. A raster bomb is a header claiming a huge size, and
//! `image` refuses it. An SVG has three separate ways to be hostile:
//!
//!   1. **Entity expansion** (billion laughs). Not bounded by `roxmltree`, and
//!      an earlier version of this file was wrong to say it was. Its
//!      `LoopDetector` caps *depth* at 10 and references at 255 **per root
//!      reference** — and explicitly allows an unlimited number of root
//!      references (`roxmltree-0.21.1/src/parse.rs:531`, "Allow infinite
//!      amount of references at zero depth"). Worse, `nodes_limit` never
//!      fires on it: consecutive references become one text node plus an
//!      unbounded `after_text` vector that is finally `join`ed into a single
//!      `String` (`parse.rs:599`). Measured: 76 KiB of source declaring one
//!      64 KiB entity and referencing it 4096 times expands to 256 MiB and
//!      takes **4.7 seconds**, parsing successfully. Both figures scale with
//!      the source, so the real growth is quadratic, and at [`MAX_SVG_BYTES`]
//!      it is terabytes. [`refuse_internal_subset`] is what actually stops it.
//!   2. **Node count.** [`MAX_XML_NODES`] bounds elements. `usvg` has caps of
//!      its own on this path — 1,000,000 nodes
//!      (`usvg-0.47.0/src/parser/svgtree/parse.rs:392`) and `<use>` depth 1024
//!      (`svgtree/parse.rs:182`), both raising
//!      `roxmltree::Error::NodesLimitReached`, which `usvg` imports there
//!      rather than using an error of its own (`svgtree/parse.rs:6`) — so this
//!      is a tightening rather than the only bound, five times lower and
//!      applied a stage earlier. The reason to keep parsing the XML here
//!      rather than calling `usvg::Tree::from_data` is that `from_data`
//!      (`usvg-0.47.0/src/parser/mod.rs:98`) hands off to `from_str`
//!      (`mod.rs:146`), whose `ParsingOptions` leaves `nodes_limit` at its
//!      `u32::MAX` default with no way to change it (`mod.rs:147`,
//!      `roxmltree-0.21.1/src/parse.rs:375`), and `from_xmltree` is the seam
//!      that lets a caller set one.
//!   3. **Canvas size.** A `viewBox` can claim any dimensions it likes. The
//!      declared size is checked against `Limits` like a raster header, and
//!      then ignored for allocation purposes anyway — see [`rasterize`],
//!      which never makes a pixmap bigger than the caller's target box.
//!   4. **Wall time**, which none of the above bounds. `<use>` expansion is
//!      exponential in the source and stops only at `usvg`'s 1,000,000
//!      elements: measured, 808 bytes of nested `<use>` costs 60 ms, and each
//!      two levels of nesting doubles it. Node and byte caps cannot see this
//!      because the source really is tiny. [`within_time_cap`] is the answer,
//!      and it is the one `crates/highlight` already reached for — see
//!      `HIGHLIGHT_TIME_CAP`. It matters here for the same reason it matters
//!      there and one more: this runs at *layout* time, and `ImageSizer` does
//!      not cache the probe, so an expensive drawing is re-paid on every
//!      relayout — every fold, resize and theme swap.
//!
//! What this does *not* accept, deliberately: `.svgz`. Decompressing it is
//! `usvg::Tree::from_data`'s job, and that entry point is the one that parses
//! with no node limit. Keeping the hardened seam means keeping the text one.

use std::sync::{Arc, OnceLock, mpsc};
use std::thread;
use std::time::Duration;

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
/// megabytes is far past any real diagram.
///
/// It also bounds the text one parse can *hold*, and that is a fact about
/// [`refuse_internal_subset`] rather than about `roxmltree`. Custom entities
/// are the only construct that makes an XML document grow, and the only way
/// one reaches a parse here is a DOCTYPE's internal subset — that is what is
/// refused, and an external subset is never fetched to begin with. Every
/// substitution still legal — `&amp;`, `&#x2014;` — is shorter than the source
/// it replaces. So peak text stays at roughly the source's own size and this
/// one number caps both. What `roxmltree` does to entity expansion is measured
/// in the module doc, and it is not a bound.
pub(crate) const MAX_SVG_BYTES: usize = 4 * 1024 * 1024;

/// How much of a file is read to decide whether it is SVG.
///
/// One constant, used both to size the read and to bound the search, because
/// two independent 1024s — one in the reader, one in the matcher — is a
/// change that looks applied and is not: widen the buffer alone and
/// [`looks_like_svg`] silently clamps back.
const SNIFF_BYTES: usize = 1024;

/// Refuses a document that declares its own XML entities.
///
/// This is the whole defence against entity expansion, because `roxmltree`
/// is not one — see the module doc for the measurement. Custom entities can
/// only be declared in a DOCTYPE's *internal subset*, the `[ ... ]` between
/// `<!DOCTYPE svg` and its closing `>`, so refusing that construct refuses
/// the entire class.
///
/// Finding that `[` takes a quote-aware walk rather than a `find('>')`, and
/// the difference is a bypass rather than a nicety. XML's `SystemLiteral` is
/// `('"' [^"]* '"') | ("'" [^']* "'")` (production 11), so it may hold a `>`
/// — and `<!DOCTYPE svg SYSTEM "x>y" [ <!ENTITY a "…"> ]>` does not end at
/// that one. Measured through [`crate::decode::probe_dimensions`] against the
/// pre-fix scan, on the fixture
/// `test_dw_1b_6_the_reproduced_bypass_is_refused_through_probe_dimensions`
/// still holds and its twin one byte shorter: `SYSTEM "x>y"` returned
/// `Ok((10, 10))` in 12 ms having expanded 256 KiB of entity text, while
/// `SYSTEM "xy"` was refused in 124 µs.
///
/// So this walks the declaration once, treating a quoted run as opaque. One
/// pass is exact rather than approximate, because production 28 is
/// `'<!DOCTYPE' S Name (S ExternalID)? S? ('[' intSubset ']' S?)? '>'` and
/// between `<!DOCTYPE` and the subset's `[` it admits only `Name` and
/// `ExternalID` (75): a name carries no `>` or quote, and an external id's
/// only free-form regions are its literals. No unquoted `>` can precede the
/// `[`, so whichever of the two turns up first outside a quote settles it.
///
/// Both quote characters delimit, and both of `PUBLIC`'s literals are walked
/// the same way. `PubidChar` (13) lists neither `>` nor `[`, so a *legal*
/// `PubidLiteral` could not hide either — but a hostile document is under no
/// obligation to be legal, and that asymmetry is not worth the branch it would
/// save.
///
/// A declaration that never closes — an unterminated literal, or a `<!DOCTYPE`
/// cut off before its `>` — is refused rather than accepted, since at that
/// point this cannot say what the document declares. It gets a message of its
/// own: calling it a document that "declares its own XML entities" would
/// assert something the walk never read.
///
/// A DOCTYPE **without** an internal subset stays legal, which is the reason
/// this is a scan and not `allow_dtd: false`. Illustrator and older Inkscape
/// emit `<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" ...>` on files that
/// are otherwise perfectly ordinary, and those carry no threat: an external
/// DTD is never fetched, since `ParsingOptions::default()` leaves
/// `entity_resolver` at `None` (`roxmltree-0.21.1/src/parse.rs:371`).
///
/// Scanned only up to the root element, which is where a DOCTYPE must appear
/// anyway. A `<!DOCTYPE` inside a comment in the prologue would be a false
/// positive; that costs one unusual file a render, where a false negative
/// costs the reader their terminal.
fn refuse_internal_subset(source: &str) -> Result<(), DecodeError> {
    let prologue = match source.find("<svg").or_else(|| source.find("<SVG")) {
        Some(root) => &source[..root],
        None => source,
    };
    let Some(doctype) = prologue.find("<!DOCTYPE") else {
        return Ok(());
    };
    // Walked as bytes: every delimiter here is ASCII and no UTF-8
    // continuation byte is, so this cannot land inside a character.
    let mut quote: Option<u8> = None;
    for &byte in &prologue.as_bytes()[doctype..] {
        match (quote, byte) {
            (Some(open), b) if b == open => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(byte),
            // The internal subset opens here, before the declaration's `>`.
            (None, b'[') => {
                return Err(DecodeError::Malformed(
                    "svg: declares its own XML entities, which this refuses to expand".to_string(),
                ));
            }
            (None, b'>') => return Ok(()),
            (None, _) => {}
        }
    }
    Err(DecodeError::Malformed(
        "svg: DOCTYPE does not close before the root element".to_string(),
    ))
}

/// Wall-clock budget for turning one drawing into pixels.
///
/// Deliberately the same number as `highlight::HIGHLIGHT_TIME_CAP`, and for
/// the same reason: it must never trip on a real document, only on input
/// engineered to be slow. A real diagram parses and renders in single-digit
/// milliseconds; the measured pathological case is 60 ms at eight levels of
/// `<use>` nesting and doubles from there, so this refuses at roughly ten
/// levels and passes everything a person would ever draw.
///
/// The font load is kept outside this budget — see [`rasterize`] — because it
/// is a fixed process-wide cost, not work a document can inflate.
const RENDER_TIME_CAP: Duration = Duration::from_millis(250);

/// The largest parsed XML node count.
///
/// A tightening, not the only bound. `usvg` caps this path too — 1,000,000
/// nodes (`usvg-0.47.0/src/parser/svgtree/parse.rs:392`) and 1024 levels of
/// nesting, which `<use>` expansion also spends (`svgtree/parse.rs:182`) — but
/// both fire a stage later, once `roxmltree` has already built the document
/// this bounds. Five times lower and one stage earlier is the whole of what
/// this adds.
///
/// A complex real diagram — a full architecture drawing, a rendered Mermaid
/// graph — lands in the low thousands; two hundred thousand is generous
/// enough that no honest document meets it and small enough that a
/// pathological one stops early.
const MAX_XML_NODES: u32 = 200_000;

/// Reads `path` as SVG source, if that is what it holds.
///
/// `Ok(None)` means "not SVG, carry on with the raster path". The sniff is on
/// content, never on the extension, so a `.svg` holding a PNG decodes as a
/// PNG and a `diagram.txt` holding SVG renders as a drawing.
///
/// Ingestion lives here rather than in [`crate::decode`] so that everything
/// deciding what an SVG *is* — the sniff window, the byte ceiling, the
/// encoding — sits next to the code that parses one. `decode`'s SVG knowledge
/// is then a single call.
///
/// The prefix is sniffed as **bytes** and the source is read separately from
/// the start, rather than the prefix being decoded and the remainder appended
/// to it. That is not tidiness: `SNIFF_BYTES` is a byte offset, and an SVG
/// with a multi-byte character straddling it would have had that character
/// cut in half — the first part lossily replaced with U+FFFD and the
/// continuation bytes failing the UTF-8 check on the remainder. A label in
/// any non-ASCII script could land there.
pub(crate) fn read_source(path: &std::path::Path) -> Result<Option<String>, DecodeError> {
    use std::io::{Read as _, Seek as _};

    crate::decode::ensure_regular_file(path)?;
    let mut file = std::fs::File::open(path).map_err(DecodeError::Io)?;

    // Filled in a loop rather than with one `read`. `File::read` is allowed
    // to return short, and std does not retry `EINTR`, so a single call could
    // hand back a fraction of the prologue — and a prologue cut before its
    // `<svg` sniffs as "not an SVG" and degrades the drawing to alt text with
    // no error anywhere. Rare, silent, and cheap to rule out.
    let mut prefix = [0u8; SNIFF_BYTES];
    let mut filled = 0;
    while filled < prefix.len() {
        match file.read(&mut prefix[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(DecodeError::Io(e)),
        }
    }
    if !looks_like_svg(&prefix[..filled]) {
        return Ok(None);
    }

    // Checked from the file's own length before reading, so an oversized
    // drawing is never pulled into memory to discover it was too big. The
    // `take` is not redundant with it: the file may grow between the two.
    let len = file.metadata().map_err(DecodeError::Io)?.len();
    if len > MAX_SVG_BYTES as u64 {
        return Err(DecodeError::Malformed(format!(
            "svg: {len} bytes, over the {MAX_SVG_BYTES}-byte limit"
        )));
    }
    file.rewind().map_err(DecodeError::Io)?;
    let mut source = String::new();
    file.take(MAX_SVG_BYTES as u64)
        .read_to_string(&mut source)
        .map_err(|_| DecodeError::Malformed("svg: source is not valid UTF-8".to_string()))?;
    Ok(Some(source))
}

/// Whether `prefix` looks like SVG source.
///
/// Content sniffing, like every other format here — the extension is never
/// consulted. An SVG document may open with a BOM, an XML declaration, a
/// DOCTYPE, comments or whitespace before the root element, so this looks for
/// the `<svg` tag itself within the prefix rather than trying to model the
/// prologue. Anything claiming to be SVG but lacking an `<svg` element inside
/// the first kilobyte is not one worth rendering.
fn looks_like_svg(prefix: &[u8]) -> bool {
    contains_element(&prefix[..prefix.len().min(SNIFF_BYTES)], b"svg")
}

/// Whether `haystack` opens an element whose *local* name is `local`, with or
/// without a namespace prefix.
///
/// The prefix is the point. SVG is a namespaced format and `usvg` resolves
/// elements by namespace plus local name, never by literal spelling
/// (`usvg-0.47.0/src/parser/svgtree/parse.rs:139`) — so `<svg:svg>` and
/// `<s:text>` are an ordinary drawing and an ordinary label to it, while a
/// plain `contains("<svg")` sees neither. Both used to fail here: a prefixed
/// root degraded silently to alt text, and a prefixed `<text>` lost every
/// glyph because the font database was never loaded for it.
///
/// The name must end at a delimiter, so `<svgfoo` does not match `svg`. What
/// this deliberately does not do is check that the prefix is *bound* to the
/// SVG namespace — that needs a parse, and this runs before one. Being wrong
/// that way costs a non-SVG XML file one refused parse; being wrong the other
/// way cost a real drawing its labels.
fn contains_element(haystack: &[u8], local: &[u8]) -> bool {
    let matches_at = |rest: &[u8]| {
        rest.len() >= local.len()
            && rest[..local.len()].eq_ignore_ascii_case(local)
            && rest
                .get(local.len())
                .is_none_or(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/')
    };
    haystack.iter().enumerate().any(|(open, byte)| {
        if *byte != b'<' {
            return false;
        }
        let after = &haystack[open + 1..];
        if matches_at(after) {
            return true;
        }
        // `prefix:local`. An NCName is short and has no `<` or space in it,
        // so a colon further out belongs to something else entirely.
        match after.iter().position(|b| *b == b':') {
            Some(colon)
                if (1..=64).contains(&colon)
                    && after[..colon]
                        .iter()
                        .all(|b| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_') =>
            {
                matches_at(&after[colon + 1..])
            }
            _ => false,
        }
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
///
/// Those numbers are perishable. Re-measure with
/// `cargo test -p gfx --test svg_cost -- --ignored --nocapture` after bumping
/// `resvg` or touching this strategy.
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
/// Matched through [`contains_element`], so a namespace-prefixed `<s:text>`
/// counts. It did not before, and a prefixed drawing lost every label to a
/// `log::warn` nobody sees.
///
/// Erring toward loading is deliberate: the cost of a false positive is one
/// unnecessary font load, and the cost of a false negative is a diagram with
/// no labels.
fn needs_fonts(source: &str) -> bool {
    let bytes = source.as_bytes();
    // `<textPath>` is only legal inside a `<text>`, so matching `text` covers
    // it — but it is listed because the coverage is incidental, and narrowing
    // this later would drop it silently.
    contains_element(bytes, b"text")
        || contains_element(bytes, b"tspan")
        || contains_element(bytes, b"textPath")
}

/// Parses `source` into a `usvg` tree under this module's limits.
///
/// The two-step parse — `roxmltree` first, `usvg::Tree::from_xmltree` second
/// — is the whole point of this function and the reason
/// `usvg::Tree::from_str` is not called. `from_str` builds its own
/// `ParsingOptions` with `nodes_limit` unset, and there is no way to reach in
/// and change it.
/// Whether a parse needs glyphs, which only rasterizing does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fonts {
    /// Shape text into paths if the drawing has any.
    IfPresent,
    /// Never load the font database.
    ///
    /// [`probe`] passes this, and it is not an optimisation — it is a
    /// statement about where a size comes from. `usvg::Tree::size()` is
    /// derived from the root element's `width`/`height`/`viewBox` attributes
    /// and never from text extents, so glyphs cannot move the answer. Loading
    /// them to compute it would spend ~480 ms, on the layout thread, on a
    /// number already sitting in an attribute.
    Never,
}

fn parse(source: &str, fonts_needed: Fonts) -> Result<usvg::Tree, DecodeError> {
    refuse_internal_subset(source)?;
    let xml_options = roxmltree::ParsingOptions {
        // A plain DOCTYPE is left allowed so the many real SVGs that ship one
        // still render; what makes that safe is `refuse_internal_subset`
        // above, not anything `roxmltree` does.
        allow_dtd: true,
        nodes_limit: MAX_XML_NODES,
        ..Default::default()
    };
    let document = roxmltree::Document::parse_with_options(source, xml_options)
        .map_err(|e| DecodeError::Malformed(format!("svg: {e}")))?;

    let want_fonts = fonts_needed == Fonts::IfPresent && needs_fonts(source);
    let options = usvg::Options {
        fontdb: if want_fonts { fonts() } else { no_fonts() },
        ..Default::default()
    };
    usvg::Tree::from_xmltree(&document, &options)
        .map_err(|e| DecodeError::Malformed(format!("svg: {e}")))
}

/// Runs `work` on a worker thread and gives up on it after
/// [`RENDER_TIME_CAP`].
///
/// The abandoned thread finishes and drops its result harmlessly — nothing
/// waits on it. That is the same trade `highlight::highlight_with_timeout`
/// makes, and the same reason: short of signal-based preemption it is the
/// only way to bound wall time regardless of what a dependency does inside
/// its own call stack. `usvg` and `resvg` offer no cancellation hook.
///
/// Unlike the highlighter there is no size threshold below which this runs
/// inline, because there is no size below which an SVG is cheap: the whole
/// point of the `<use>` bomb is that it is 808 bytes long.
fn within_time_cap<T, F>(work: F) -> Result<T, DecodeError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, DecodeError> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(work());
    });
    match rx.recv_timeout(RENDER_TIME_CAP) {
        Ok(result) => result,
        Err(_) => Err(DecodeError::Malformed(format!(
            "svg: gave up after {} ms — too complex to draw",
            RENDER_TIME_CAP.as_millis()
        ))),
    }
}

/// The SVG's declared pixel size, for layout to reserve a box from.
///
/// This is the vector counterpart of the raster header probe, and it is
/// honestly more expensive: an SVG has no header to read, so learning its size
/// means parsing it. Parsing is still far cheaper than rasterizing, which is
/// the distinction the layout-time probe actually cares about.
pub(crate) fn probe(source: &str, limits: Limits) -> Result<(u32, u32), DecodeError> {
    let owned = source.to_string();
    within_time_cap(move || dimensions(&parse(&owned, Fonts::Never)?, limits))
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
) -> Result<image::DynamicImage, DecodeError> {
    // Warmed *outside* the cap, deliberately. The font load is a one-time,
    // process-wide, ~480 ms cost bounded by how many fonts are installed —
    // nothing a document controls — so counting it against a per-drawing
    // budget would refuse the first labelled diagram of every session and
    // then render it fine on the second. The cap exists to bound work an
    // attacker chooses, and this is not that work.
    //
    // Found by `test_text_in_a_drawing_is_actually_drawn`, which failed with
    // "gave up after 250 ms" the moment the cap went in — while the comment
    // on the cap claimed it wrapped the render and not the font load.
    if needs_fonts(source) {
        let _ = fonts();
    }
    let owned = source.to_string();
    within_time_cap(move || rasterize_now(&owned, target_px, limits))
}

fn rasterize_now(
    source: &str,
    target_px: (u32, u32),
    limits: Limits,
) -> Result<image::DynamicImage, DecodeError> {
    let tree = parse(source, Fonts::IfPresent)?;
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
    Ok(image::DynamicImage::ImageRgba8(buffer))
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
        // Namespace-prefixed spellings are what `usvg` actually resolves, and
        // what some Batik and older-editor toolchains emit. A prefixed root
        // used to sniff as "not an SVG" and degrade silently to alt text.
        assert!(looks_like_svg(
            br#"<svg:svg xmlns:svg="http://www.w3.org/2000/svg">"#
        ));
        assert!(looks_like_svg(
            br#"<ns0:svg xmlns:ns0="http://www.w3.org/2000/svg"/>"#
        ));
        assert!(
            !looks_like_svg(b"<a:notsvg>"),
            "the local name still has to match"
        );
        assert!(!looks_like_svg(b"\x89PNG\r\n\x1a\n"));
        assert!(!looks_like_svg(b""));
        // An `<svg` past the sniff window is not one this will find, and that
        // is the documented behaviour rather than an accident.
        let buried = format!("{}<svg>", " ".repeat(2000));
        assert!(!looks_like_svg(buried.as_bytes()));
    }

    /// A multi-byte character sitting across the sniff boundary survives.
    ///
    /// Regression: the source used to be assembled as "lossily decode the
    /// sniff prefix, then append the rest", which cut whatever character
    /// straddled byte 1024 in half — the leading bytes became U+FFFD and the
    /// continuation bytes failed the UTF-8 check on the remainder, so a
    /// perfectly valid drawing was refused as "not valid UTF-8". Any
    /// non-ASCII label long enough to reach the boundary could land there.
    #[test]
    fn test_a_character_straddling_the_sniff_boundary_survives() {
        // Pad so a 3-byte character starts at byte SNIFF_BYTES - 1.
        let head = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><desc>"##;
        let pad = "a".repeat(SNIFF_BYTES - 1 - head.len());
        let source = format!("{head}{pad}\u{4e16}</desc></svg>");
        assert_eq!(
            source.as_bytes()[SNIFF_BYTES - 1],
            "\u{4e16}".as_bytes()[0],
            "the fixture must actually straddle the boundary"
        );

        let dir = std::env::temp_dir().join(format!("stele-svg-straddle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("straddle.svg");
        std::fs::write(&path, &source).expect("write");

        let read = read_source(&path)
            .expect("reads")
            .expect("is recognised as svg");
        assert_eq!(read, source, "the source came back altered");
        assert_eq!(probe(&read, Limits::default()).expect("probes"), (10, 10));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_a_drawing_probes_at_its_declared_size() {
        assert_eq!(probe(CIRCLE, Limits::default()).expect("probes"), (120, 60));
    }

    #[test]
    fn test_rasterizing_fits_the_target_box_and_keeps_the_aspect_ratio() {
        let img = rasterize(CIRCLE, (60, 60), Limits::default()).expect("renders");
        // 120x60 into a 60x60 box is 60x30, not 60x60 — the letterbox in
        // `decode` adds the empty rows, and it is the only thing that should.
        assert_eq!((img.width(), img.height()), (60, 30));
    }

    #[test]
    fn test_the_pixmap_never_exceeds_the_target_however_large_the_drawing() {
        let huge = r##"<svg xmlns="http://www.w3.org/2000/svg" width="8000" height="8000"><rect width="8000" height="8000" fill="red"/></svg>"##;
        let img = rasterize(huge, (32, 32), Limits::default()).expect("renders");
        assert_eq!((img.width(), img.height()), (32, 32));
    }

    #[test]
    fn test_a_canvas_past_the_limits_is_refused_like_a_bomb_header() {
        let bomb = r##"<svg xmlns="http://www.w3.org/2000/svg" width="99999" height="99999"><rect width="10" height="10"/></svg>"##;
        let err = probe(bomb, Limits::default()).expect_err("a huge canvas is refused");
        assert!(matches!(err, DecodeError::ExceedsLimits { .. }), "{err:?}");
    }

    /// The node-count bound. Not the only one — `usvg` caps the same path at
    /// 1,000,000 nodes (`usvg-0.47.0/src/parser/svgtree/parse.rs:392`) — but
    /// [`MAX_XML_NODES`] is five times lower and applied a stage earlier, so
    /// it is the one a document meets first, and the one this pins. Built as
    /// a wide, shallow document so it is the *count* under test rather than
    /// nesting depth.
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

    /// Billion laughs, in its classic shape: entities nested ten to a level so
    /// each tier multiplies the last.
    ///
    /// [`refuse_internal_subset`] is what stops this, not `roxmltree`. The
    /// fixture declares an internal subset, so it is refused on [`parse`]'s
    /// first line, before any entity is resolved at all. What this test pins
    /// is therefore the *shape* and not the bound: `Malformed(_)` is broad
    /// enough to pass on any refusal, and `test_a_wide_entity_bomb_is_refused`
    /// is the one that checks the guard by its message. The module doc has the
    /// measurement showing why the guard, rather than the parser, has to be
    /// the thing that stops it.
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
        let img = rasterize(labelled, (200, 60), Limits::default()).expect("renders");
        let rgba = img.to_rgba8();
        let inked = rgba.pixels().filter(|p| p.0[3] > 0).count();
        assert!(
            inked > 200,
            "expected glyph coverage, found {inked} inked pixels — fonts are not reaching text"
        );
    }

    /// The two input caps must stay in a stated relationship, not merely each
    /// be a number: every node costs bytes, so a node cap the byte cap could
    /// never let a document reach would be decoration.
    ///
    /// `decode` has
    /// `test_dw_6_3_no_input_the_dimension_cap_admits_can_exceed_the_allocation_cap`
    /// for exactly this on the raster side: it pins the *conjunction*, so
    /// raising one cap alone fails loudly. This is the vector counterpart.
    ///
    /// No expansion factor enters the arithmetic, and that is deliberate.
    /// [`refuse_internal_subset`] means a source cannot declare entities, so
    /// bytes of source are bytes of text — see [`MAX_SVG_BYTES`], which is
    /// where that argument is made.
    ///
    /// Worth knowing how loose the pin is: `<g/>` is the shortest element that
    /// makes a node, so [`MAX_XML_NODES`] needs 800,000 bytes of source
    /// against [`MAX_SVG_BYTES`]'s 4,194,304 — 5.24x of headroom. Raising the
    /// node cap past 1,048,576 is what trips this.
    #[test]
    fn test_the_node_cap_is_reachable_within_the_byte_cap() {
        let smallest_node_bytes = "<g/>".len() as u64;
        assert!(
            MAX_SVG_BYTES as u64 >= u64::from(MAX_XML_NODES) * smallest_node_bytes,
            "MAX_XML_NODES is unreachable within MAX_SVG_BYTES — one of the two \
             caps is doing nothing"
        );
    }

    /// The wide entity bomb, which the earlier defence did not stop.
    ///
    /// This is the shape the old `test_an_entity_bomb_is_refused_rather_than_expanded`
    /// missed: that fixture nests entities inside *one* root reference, which
    /// `roxmltree`'s 255-per-root cap does catch. Repeat the reference instead
    /// and the cap never applies, because it is explicitly disabled at depth
    /// zero. Measured before the fix: 76 KiB of source, 256 MiB of expansion,
    /// 4.7 seconds, and a successful parse.
    ///
    /// Kept small enough to be harmless if the guard ever regresses — 16 MiB
    /// and a fraction of a second rather than the numbers above.
    #[test]
    fn test_a_wide_entity_bomb_is_refused() {
        let value = "A".repeat(16 * 1024);
        let mut source =
            format!("<?xml version=\"1.0\"?>\n<!DOCTYPE svg [ <!ENTITY a \"{value}\"> ]>\n");
        source.push_str(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"><desc>",
        );
        for _ in 0..1024 {
            source.push_str("&a;");
        }
        source.push_str("</desc></svg>");

        let err = probe(&source, Limits::default()).expect_err("a wide entity bomb is refused");
        assert!(
            matches!(&err, DecodeError::Malformed(m) if m.contains("own XML entities")),
            "{err:?}"
        );
    }

    /// A DOCTYPE with no internal subset still renders.
    ///
    /// The entity guard is a scan rather than `allow_dtd: false` precisely so
    /// this keeps working: Illustrator and older Inkscape put one on files
    /// that are otherwise ordinary, and an external DTD is never fetched.
    #[test]
    fn test_a_plain_doctype_is_still_accepted() {
        let source = concat!(
            "<?xml version=\"1.0\"?>\n",
            "<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" ",
            "\"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n",
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40\" height=\"20\"/>"
        );
        assert_eq!(probe(source, Limits::default()).expect("parses"), (40, 20));
    }

    /// Builds a drawing whose DOCTYPE carries `external_id` and an internal
    /// subset. Small on purpose: `read_source` only sniffs
    /// [`SNIFF_BYTES`] for the root element, so a fixture whose prologue
    /// outgrew that would stop being recognised as SVG and would prove
    /// nothing about this guard.
    fn subset_behind(external_id: &str) -> String {
        format!(
            "<?xml version=\"1.0\"?>\n\
             <!DOCTYPE svg {external_id} [ <!ENTITY a \"AAAAAAAAAAAAAAAA\"> ]>\n\
             <svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\">\
             <desc>&a;</desc></svg>"
        )
    }

    fn refusal_of(source: &str) -> DecodeError {
        probe(source, Limits::default()).expect_err("an internal subset is refused")
    }

    /// The bypass the Phase 1 batch review found by execution.
    ///
    /// A `>` inside a `SystemLiteral` is legal (XML production 11 is
    /// `('"' [^"]* '"') | ("'" [^']* "'")`), and the old scan ended the
    /// declaration at the first `>` it saw — so the `[` two characters later
    /// was never examined. Measured through `probe_dimensions` before the fix:
    /// `Ok((10, 10))` in 23 ms, a megabyte of entity text expanded. The
    /// unquoted twin differs by one byte and was refused in 127 µs; this
    /// asserts the two now produce the *same* error, which is the whole claim.
    #[test]
    fn test_dw_1b_1_a_subset_behind_a_quoted_gt_is_refused_like_the_unquoted_twin() {
        let hidden = refusal_of(&subset_behind(r#"SYSTEM "x>y""#));
        let plain = refusal_of(&subset_behind(r#"SYSTEM "xy""#));
        assert!(
            matches!(&hidden, DecodeError::Malformed(m) if m.contains("own XML entities")),
            "{hidden:?}"
        );
        assert_eq!(
            format!("{hidden:?}"),
            format!("{plain:?}"),
            "one byte of quoting must not change the answer"
        );
    }

    /// `'` delimits a literal exactly as `"` does, so a scan that knew only
    /// about double quotes would be the same defect with an extra step.
    #[test]
    fn test_dw_1b_2_a_single_quoted_literal_hides_no_subset_either() {
        let err = refusal_of(&subset_behind(r#"SYSTEM 'x>y'"#));
        assert!(
            matches!(&err, DecodeError::Malformed(m) if m.contains("own XML entities")),
            "{err:?}"
        );
    }

    /// `PUBLIC` carries **two** literals (production 75: `'PUBLIC' S
    /// PubidLiteral S SystemLiteral`), so both have to be walked. A legal
    /// `PubidLiteral` could not in fact hold a `>` — `PubidChar` (13) does not
    /// list one — but nothing obliges a hostile document to be legal, and the
    /// guard does not lean on that.
    #[test]
    fn test_dw_1b_2_both_public_literals_are_walked_for_a_hidden_subset() {
        let err = refusal_of(&subset_behind(r#"PUBLIC "-//a>b//EN" "http://c>d/x.dtd""#));
        assert!(
            matches!(&err, DecodeError::Malformed(m) if m.contains("own XML entities")),
            "{err:?}"
        );
    }

    /// The inverse error, and the reason the walk cannot simply refuse on any
    /// `[`: a bracket inside a literal is an ordinary character, not a subset.
    #[test]
    fn test_dw_1b_3_a_bracket_inside_a_literal_is_not_an_internal_subset() {
        let source = concat!(
            "<?xml version=\"1.0\"?>\n",
            "<!DOCTYPE svg SYSTEM \"a[b\">\n",
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40\" height=\"20\"/>"
        );
        assert_eq!(probe(source, Limits::default()).expect("parses"), (40, 20));
    }

    /// A literal that never closes leaves the walk unable to say what the
    /// document declares, so it refuses — the safe direction in front of a
    /// parser whose failure mode is unbounded memory on the layout thread.
    /// The message is the DOCTYPE one rather than the entity one, because no
    /// subset was ever read.
    #[test]
    fn test_dw_1b_4_an_unterminated_literal_is_refused_rather_than_walked_past() {
        let source = concat!(
            "<?xml version=\"1.0\"?>\n",
            "<!DOCTYPE svg SYSTEM \"x\n",
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40\" height=\"20\"/>"
        );
        let err = probe(source, Limits::default()).expect_err("an unclosed DOCTYPE is refused");
        assert!(
            matches!(&err, DecodeError::Malformed(m) if m.contains("does not close")),
            "{err:?}"
        );
    }

    /// Every truncation of a hostile DOCTYPE, since the walk indexes bytes and
    /// a file can end anywhere. Nothing may panic; each prefix must land on
    /// one of the two answers rather than out of bounds.
    #[test]
    fn test_dw_1b_4_no_prefix_of_a_hostile_doctype_walks_out_of_bounds() {
        let hostile = subset_behind(r#"PUBLIC 'p>[q' "s>[t""#);
        assert!(
            hostile.is_ascii(),
            "prefixes must split on character bounds"
        );
        for end in 0..=hostile.len() {
            let _ = refuse_internal_subset(&hostile[..end]);
        }
    }

    /// The reproduction, inverted, through the real public entry point.
    ///
    /// `decode::probe_dimensions` is what layout calls, and it is where the
    /// bypass was demonstrated: this file's shape returned `Ok((10, 10))` with
    /// a megabyte of entity text expanded. Driving the same bytes off disk
    /// rather than calling [`refuse_internal_subset`] directly is the point —
    /// it pins the guard where it is actually reached, past the sniff and the
    /// byte cap.
    #[test]
    fn test_dw_1b_6_the_reproduced_bypass_is_refused_through_probe_dimensions() {
        let mut source = String::from(
            "<?xml version=\"1.0\"?>\n\
             <!DOCTYPE svg SYSTEM \"x>y\" [ <!ENTITY a \"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"> ]>\n\
             <svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"><desc>",
        );
        for _ in 0..8_192 {
            source.push_str("&a;");
        }
        source.push_str("</desc></svg>");

        let dir = std::env::temp_dir().join(format!("stele-svg-bypass-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("evil.svg");
        std::fs::write(&path, &source).expect("write");

        let err = crate::decode::probe_dimensions(&path, Limits::default())
            .expect_err("the hidden subset is refused on the public path");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            matches!(&err, DecodeError::Malformed(m) if m.contains("own XML entities")),
            "{err:?}"
        );
    }

    /// The time cap, on the one axis no byte or node count can see.
    ///
    /// Nested `<use>` is exponential in the source: measured, 808 bytes at
    /// eight levels costs 60 ms and every two further levels doubles it, with
    /// nothing stopping it before `usvg`'s 1,000,000 elements. Sixteen levels
    /// is ~65k leaves — comfortably past the budget, comfortably inside every
    /// other limit, and about a kilobyte of source.
    #[test]
    fn test_an_expansion_bomb_is_refused_on_time_rather_than_size() {
        let mut source = String::from(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"64\" height=\"64\"><defs>\
             <g id=\"l0\"><rect width=\"4\" height=\"4\"/></g>",
        );
        for i in 1..=16 {
            source.push_str(&format!(
                "<g id=\"l{i}\"><use href=\"#l{}\"/><use href=\"#l{}\"/></g>",
                i - 1,
                i - 1
            ));
        }
        source.push_str("</defs><use href=\"#l16\"/></svg>");
        assert!(
            source.len() < 2_048,
            "the fixture must stay tiny — that is the whole point"
        );

        let started = std::time::Instant::now();
        let err = rasterize(&source, (32, 32), Limits::default())
            .expect_err("an expansion bomb is refused");
        assert!(
            matches!(&err, DecodeError::Malformed(m) if m.contains("too complex")),
            "{err:?}"
        );
        assert!(
            started.elapsed() < RENDER_TIME_CAP * 8,
            "the cap did not actually bound the wait: {:?}",
            started.elapsed()
        );
    }

    /// `probe` must never load fonts — a drawing's size comes from its root
    /// attributes, not from text extents. Asserted by timing rather than by
    /// inspection: the font load is ~480 ms, so a probe that stayed under the
    /// render cap cannot have done one.
    #[test]
    fn test_probing_a_labelled_drawing_does_not_load_fonts() {
        let labelled = r##"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="80">
            <text x="10" y="40" font-size="20">a label that would need shaping</text>
        </svg>"##;
        let started = std::time::Instant::now();
        assert_eq!(
            probe(labelled, Limits::default()).expect("probes"),
            (300, 80)
        );
        assert!(
            started.elapsed() < RENDER_TIME_CAP,
            "probing took {:?} — it loaded the font database for a size it reads \
             off an attribute",
            started.elapsed()
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
