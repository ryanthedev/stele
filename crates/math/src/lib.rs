//! `crates/math` — RaTeX-backed math rendering, with a `txm` text-grid
//! fallback.
//!
//! Two public entry points, per the phase's pinned contract:
//! [`render`] (RaTeX → PNG) and [`render_text`] (txm → a plain Unicode
//! grid). The third rung of the degradation ladder — showing the literal
//! TeX source — needs no code here: it's the document's own
//! `Semantic::MathTex` text, already laid out and painted by P4/P5 whenever
//! a `Reserved` box is never created for a formula (see
//! `stele::media::ImageSizer`).
//!
//! **Ink color, recorded per `docs/spikes/ratex.md`'s measured consequence:**
//! `LayoutOptions::default()` is `Color::BLACK`, which measured a 1.24:1
//! WCAG contrast ratio against Ghostty's `#1e1e1e`-class dark theme
//! (functionally invisible). [`render`] never uses the default — it always
//! sets an explicit light ink color and a transparent background.
//! **Documented limitation:** this is a static choice, not a live
//! OSC-10/11-resolved theme color — that integration lands with P7's theme
//! engine (the plan's own Decision Log: "P5 paints structural styles only;
//! theme engine lands in P7"). Still, it satisfies the spike's actual
//! requirement — never default black — honestly rather than only in
//! appearance.
//!
//! **Caching (DW-6.5).** A cell-geometry change means [`render`] gets
//! called again with the *same* `tex` and a *different* `px_height`. Two
//! bounded, capacity-256 caches make that cheap without ever re-laying-out
//! the document: a `DisplayList` cache keyed by the exact TeX source
//! (reused across a `px_height` change — no re-parse, no re-layout, only a
//! re-rasterization at the new size) and a final-PNG cache keyed by
//! `(tex, px_height, padding)` (skips rasterization entirely on a
//! byte-identical repeat request). [`cache_stats`] exposes hit/miss counters
//! so this is asserted deterministically rather than by timing.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use ratex_types::color::Color;
use unicode_width::UnicodeWidthStr;

/// Hostile-input bound: markdown math spans are ordinary inline/display
/// text, but nothing stops a hostile document from putting an enormous
/// string inside `$...$`. Bounded so a pathological formula cannot burn
/// unbounded parse/layout time (defense-in-depth alongside RaTeX's own
/// parser, which is not this crate's code to audit).
const MAX_TEX_LEN: usize = 4_096;

/// `RenderOptions.font_size`/`px_height` bounds: a terminal cell is tens of
/// pixels tall, never thousands — clamping here prevents a hostile or
/// simply wrong `px_height` from asking `ratex-render` to allocate an
/// enormous pixmap.
const MIN_PX_HEIGHT: u32 = 4;
const MAX_PX_HEIGHT: u32 = 512;

/// Default blank margin the rasterizer adds on every side, in pixels. Part
/// of the raster's final size, so [`render_fitted`] solves for it rather
/// than treating it as free.
const PADDING: f32 = 4.0;

/// Ceiling on the solved letterbox margin. A degenerate box aspect could
/// otherwise ask for an enormous transparent border, and padding counts
/// toward the pixmap budget like any other pixel.
const MAX_PADDING: f32 = 512.0;

const CACHE_CAP: usize = 256;

/// Explicit ink color for every RaTeX render: light gray-white, never the
/// library default `Color::BLACK` (measured 1.24:1 contrast on Ghostty's
/// dark theme — see the module doc comment).
const INK_COLOR: Color = Color::new(0.92, 0.92, 0.92, 1.0);
/// Fully transparent background — real alpha-0 RGBA, not a colored box
/// hiding the terminal's own background (per `ratex.md`'s transparency
/// check).
const TRANSPARENT: Color = Color::new(1.0, 1.0, 1.0, 0.0);

/// A rendered PNG's raw bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Png(pub Vec<u8>);

impl Png {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Why [`render`] failed. Every variant is a handled, anticipated outcome —
/// RaTeX rejecting the input, or (far rarer) the rasterizer itself failing
/// on a degenerate layout — never a panic.
#[derive(Debug, Clone)]
pub enum MathError {
    /// TeX source too long to attempt (see [`MAX_TEX_LEN`]).
    TooLarge { len: usize },
    /// `ratex_parser::parse` rejected the source.
    Parse(String),
    /// `ratex_render::render_to_png` failed after a successful parse+layout.
    Render(String),
    /// The formula's own em extent would rasterize to more than
    /// [`MAX_PIXMAP_PX`] pixels even at the smallest font size. Clamping
    /// `font_size` alone does not bound the pixmap: area scales with the
    /// *layout's* width and height too, so a wide construct (a large matrix,
    /// deeply stacked fractions) inside the `MAX_TEX_LEN` budget could
    /// otherwise ask the rasterizer for hundreds of megabytes.
    PixmapTooLarge { px: u64 },
}

impl std::fmt::Display for MathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MathError::TooLarge { len } => {
                write!(f, "TeX source too large ({len} bytes, max {MAX_TEX_LEN})")
            }
            MathError::Parse(msg) => write!(f, "RaTeX parse error: {msg}"),
            MathError::Render(msg) => write!(f, "RaTeX render error: {msg}"),
            MathError::PixmapTooLarge { px } => write!(
                f,
                "formula would rasterize to {px} pixels (max {MAX_PIXMAP_PX})"
            ),
        }
    }
}

impl std::error::Error for MathError {}

/// A plain (no embedded escape codes) Unicode text grid — the txm rung's
/// output. Rows may differ in length; [`TextGrid::cols`] is the widest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextGrid {
    pub rows: Vec<String>,
}

impl TextGrid {
    /// Display-cell width of the widest row (via `unicode-width`, not raw
    /// `chars().count()` — txm's glyphs include double-width math symbols).
    pub fn cols(&self) -> u16 {
        self.rows
            .iter()
            .map(|r| UnicodeWidthStr::width(r.as_str()))
            .max()
            .unwrap_or(0)
            .min(u16::MAX as usize) as u16
    }

    pub fn rows_count(&self) -> u16 {
        self.rows.len().min(u16::MAX as usize) as u16
    }
}

/// Renders `tex` to a PNG at `px_height` (the RaTeX `font_size`/em-pixel
/// size). Never relies on `LayoutOptions::default()`'s black ink (see the
/// module doc comment) — always explicit light ink over a transparent
/// background.
///
/// `px_height` is clamped to `[MIN_PX_HEIGHT, MAX_PX_HEIGHT]` before use —
/// a hostile or wrong caller-supplied size can never make the rasterizer
/// allocate an unbounded pixmap.
/// Largest raster [`render`] will ask for, in pixels (~32 MB at RGBA8).
/// Generous for a formula that has to fit in a few terminal cells, but a
/// hard ceiling against a hostile one.
const MAX_PIXMAP_PX: u64 = 8_000_000;

/// Returns the largest font size `<= requested` whose projected pixmap stays
/// within [`MAX_PIXMAP_PX`], or `PixmapTooLarge` if even [`MIN_PX_HEIGHT`]
/// does not fit. Degenerate (zero / non-finite) extents pass through
/// unchanged — there is nothing to bound, and the rasterizer handles them.
fn fit_font_size(
    list: &ratex_types::display_item::DisplayList,
    requested: u32,
) -> Result<u32, MathError> {
    let em_w = list.width;
    let em_h = list.height + list.depth;
    if !em_w.is_finite() || !em_h.is_finite() || em_w <= 0.0 || em_h <= 0.0 {
        return Ok(requested);
    }
    let px_at = |font: u32| -> u64 {
        let f = f64::from(font);
        let w = (em_w * f).ceil().max(1.0);
        let h = (em_h * f).ceil().max(1.0);
        // Saturate rather than wrap on an absurd extent.
        if w > u64::MAX as f64 || h > u64::MAX as f64 {
            return u64::MAX;
        }
        (w as u64).saturating_mul(h as u64)
    };
    if px_at(requested) <= MAX_PIXMAP_PX {
        return Ok(requested);
    }
    // area ~ font^2, so solve for the largest font that fits.
    let scale = (MAX_PIXMAP_PX as f64 / (em_w * em_h)).sqrt().floor();
    let fitted = if scale.is_finite() && scale >= 1.0 {
        (scale as u64).min(u64::from(requested)) as u32
    } else {
        0
    };
    if fitted >= MIN_PX_HEIGHT && px_at(fitted) <= MAX_PIXMAP_PX {
        Ok(fitted)
    } else {
        Err(MathError::PixmapTooLarge {
            px: px_at(MIN_PX_HEIGHT),
        })
    }
}

pub fn render(tex: &str, px_height: u32) -> Result<Png, MathError> {
    render_padded(tex, px_height, PADDING)
}

/// [`render`] with an explicit transparent margin. Used by
/// [`render_fitted`] to letterbox a formula onto its cell box's aspect ratio
/// without touching the em size.
fn render_padded(tex: &str, px_height: u32, padding: f32) -> Result<Png, MathError> {
    if tex.len() > MAX_TEX_LEN {
        return Err(MathError::TooLarge { len: tex.len() });
    }
    let px_height = px_height.clamp(MIN_PX_HEIGHT, MAX_PX_HEIGHT);
    // Padding is part of the raster's identity, so it belongs in the cache
    // key — the same formula at the same em but a different letterbox is a
    // different image. Quantized to whole pixels so f32 noise cannot spawn
    // unbounded near-duplicate cache entries.
    let padding = if padding.is_finite() {
        padding.clamp(0.0, MAX_PADDING).round()
    } else {
        PADDING
    };
    let cache_key = (tex.to_string(), px_height, padding as u32);

    if let Some(png) = png_cache().lock().unwrap().get(&cache_key) {
        record_png_hit();
        return Ok(png.clone());
    }
    record_png_miss();

    let display_list = cached_display_list(tex)?;

    // Bound the *pixmap*, not just the font size. Area scales with the
    // layout's em extent as well, so a wide/tall construct within the
    // MAX_TEX_LEN budget can demand an enormous raster at a legal font size.
    // Shrink the font to fit the pixel budget; only if even MIN_PX_HEIGHT
    // overflows it do we fail, and then the caller falls to the txm rung.
    let px_height = fit_font_size(&display_list, px_height)?;

    let render_options = ratex_render::RenderOptions {
        font_size: px_height as f32,
        padding,
        background_color: TRANSPARENT,
        font_dir: String::new(),
        device_pixel_ratio: 1.0,
    };
    let bytes =
        ratex_render::render_to_png(&display_list, &render_options).map_err(MathError::Render)?;
    let png = Png(bytes);

    insert_bounded(
        &mut png_cache().lock().unwrap(),
        (tex.to_string(), px_height, padding as u32),
        png.clone(),
    );
    Ok(png)
}

/// Renders `tex` as a plain Unicode text grid via `txm` — the second rung
/// of the degradation ladder, tried when [`render`] fails. Strips txm's own
/// embedded ANSI SGR styling before returning: this crate's output must
/// never carry raw escape bytes of its own (the caller is responsible for
/// the single sanitized path text reaches the terminal through — see
/// `stele::painter`'s barricade — and third-party escape sequences are not
/// exempt from that discipline).
///
/// Returns `None` on any txm parse failure — DW-6.2's third rung (literal
/// TeX source) is what a caller falls back to next.
pub fn render_text(tex: &str) -> Option<TextGrid> {
    if tex.len() > MAX_TEX_LEN {
        return None;
    }
    let styled = txm::render(tex).ok()?;
    let rows = styled.lines().map(strip_ansi_sgr).collect();
    Some(TextGrid { rows })
}

/// Strips `ESC [ ... m` SGR sequences (txm's own styling) from one line,
/// leaving the plain visible characters. Deliberately narrow (SGR only,
/// the only escape kind txm's terminal backend emits) rather than a
/// general-purpose ANSI stripper.
fn strip_ansi_sgr(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next(); // consume '['
            for next in chars.by_ref() {
                if next == 'm' {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Introspection for DW-6.5: proves a `px_height`-only change on the same
/// `tex` hits the `DisplayList` cache (no re-parse, no re-layout) rather
/// than relying on timing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub display_list_hits: u64,
    pub display_list_misses: u64,
    pub png_hits: u64,
    pub png_misses: u64,
}

pub fn cache_stats() -> CacheStats {
    stats().lock().unwrap().clone_stats()
}

/// Test-only: clears every cache and counter so tests don't observe stale
/// state from an earlier test in the same process.
#[doc(hidden)]
pub fn reset_caches_for_test() {
    display_list_cache().lock().unwrap().clear();
    png_cache().lock().unwrap().clear();
    *stats().lock().unwrap() = Stats::default();
}

#[derive(Debug, Clone, Default)]
struct Stats {
    display_list_hits: u64,
    display_list_misses: u64,
    png_hits: u64,
    png_misses: u64,
}

impl Stats {
    fn clone_stats(&self) -> CacheStats {
        CacheStats {
            display_list_hits: self.display_list_hits,
            display_list_misses: self.display_list_misses,
            png_hits: self.png_hits,
            png_misses: self.png_misses,
        }
    }
}

fn stats() -> &'static Mutex<Stats> {
    static STATS: OnceLock<Mutex<Stats>> = OnceLock::new();
    STATS.get_or_init(|| Mutex::new(Stats::default()))
}

fn record_png_hit() {
    stats().lock().unwrap().png_hits += 1;
}
fn record_png_miss() {
    stats().lock().unwrap().png_misses += 1;
}

/// Bounded map with FIFO-ish eviction (evicts an arbitrary oldest-inserted
/// entry once at capacity) — not a true LRU, but enough to guarantee
/// bounded memory growth over a long session with many distinct formulas,
/// which a strictly-unbounded cache would not.
type BoundedMap<K, V> = HashMap<K, V>;

fn insert_bounded<K: std::hash::Hash + Eq + Clone, V>(map: &mut HashMap<K, V>, key: K, value: V) {
    if map.len() >= CACHE_CAP
        && !map.contains_key(&key)
        && let Some(evict_key) = map.keys().next().cloned()
    {
        map.remove(&evict_key);
    }
    map.insert(key, value);
}

fn display_list_cache() -> &'static Mutex<BoundedMap<String, ratex_types::display_item::DisplayList>>
{
    static CACHE: OnceLock<Mutex<BoundedMap<String, ratex_types::display_item::DisplayList>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A rendered PNG's full identity: `(tex, em size, letterbox padding)`.
/// Padding belongs here because [`render_fitted`] varies it per target box —
/// the same formula at the same em but a different letterbox is a different
/// raster, and keying without it would serve the wrong one.
type PngKey = (String, u32, u32);

fn png_cache() -> &'static Mutex<BoundedMap<PngKey, Png>> {
    static CACHE: OnceLock<Mutex<BoundedMap<PngKey, Png>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Parses + lays out `tex` into a `DisplayList` in em units (no
/// rasterization — this is the cheap, non-pixel-producing step that makes
/// [`stele::media::ImageSizer`]'s math sizing possible without a full
/// render, and that DW-6.5's cache-hit proof is about), reusing a cached
/// result for the exact same source string.
/// Deepest brace/group nesting [`render`] will hand to the TeX parser.
///
/// `MAX_TEX_LEN` alone does not bound this: ~2000 nested `{` fit inside 4 KiB,
/// and a recursive-descent parser on that can exhaust the stack. A stack
/// overflow **aborts** the process — `catch_unwind` cannot recover it, and
/// wrapping the parser in `catch_unwind` would be worse than useless here
/// anyway, because the panic hook (which restores the terminal) runs before
/// unwinding, so a "recovered" panic would still tear down the alt screen
/// mid-session. Rejecting the input up front is the only mitigation that
/// actually holds, and it keeps the failure on the degradation ladder: the
/// caller falls through to the txm rung, then to literal source.
const MAX_TEX_DEPTH: usize = 64;

/// Rejects TeX whose group nesting exceeds [`MAX_TEX_DEPTH`], before any
/// third-party parser sees it.
fn reject_pathological_nesting(tex: &str) -> Result<(), MathError> {
    let mut depth = 0usize;
    let mut escaped = false;
    for ch in tex.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '{' => {
                depth += 1;
                if depth > MAX_TEX_DEPTH {
                    return Err(MathError::Parse(format!(
                        "group nesting deeper than {MAX_TEX_DEPTH}"
                    )));
                }
            }
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

fn cached_display_list(tex: &str) -> Result<ratex_types::display_item::DisplayList, MathError> {
    let cached = display_list_cache().lock().unwrap().get(tex).cloned();
    if let Some(list) = cached {
        record_display_list_hit();
        return Ok(list);
    }
    record_display_list_miss();

    reject_pathological_nesting(tex)?;
    let nodes = ratex_parser::parse(tex).map_err(|e| MathError::Parse(e.to_string()))?;
    let layout_options = ratex_layout::LayoutOptions {
        color: INK_COLOR,
        ..Default::default()
    };
    let layout_box = ratex_layout::layout(&nodes, &layout_options);
    let display_list = ratex_layout::to_display_list(&layout_box);

    insert_bounded(
        &mut display_list_cache().lock().unwrap(),
        tex.to_string(),
        display_list.clone(),
    );
    Ok(display_list)
}

fn record_display_list_hit() {
    stats().lock().unwrap().display_list_hits += 1;
}
fn record_display_list_miss() {
    stats().lock().unwrap().display_list_misses += 1;
}

/// Cheap, non-rasterizing intrinsic size of `tex` in em units:
/// `(width, height + depth)`, straight from the cached [`DisplayList`] —
/// used by `stele::media::ImageSizer` to reserve a layout box without ever
/// producing a pixel. Returns `None` on a RaTeX parse failure (the caller
/// tries the txm rung next).
pub fn intrinsic_em_size(tex: &str) -> Option<(f64, f64)> {
    if tex.len() > MAX_TEX_LEN {
        return None;
    }
    let list = cached_display_list(tex).ok()?;
    Some((list.width, list.height + list.depth))
}

/// Renders `tex` at a fixed `em_px`, padded so the raster's aspect ratio
/// matches a `target_w` x `target_h` pixel box.
///
/// Both halves matter, and they pull against each other:
///
/// - **Fixed em.** The em size is what makes a formula's glyphs look like the
///   body text around them. It must not be derived from the box, or `a+b=c`
///   (0.78 em tall) and `e^{i\pi}+1=0` (0.96 em tall) would render at
///   noticeably different glyph sizes just because one has a superscript.
/// - **Matched aspect.** The kitty graphics protocol scales a placement to
///   fill its `c=` x `r=` cell box *exactly*, so any mismatch between the
///   raster's proportions and the box's shows up as stretched glyphs. Cell
///   quantization guarantees a mismatch: a box is a whole number of cells,
///   the ink is not.
///
/// Padding reconciles them. Growing the transparent margin moves the raster's
/// aspect toward 1:1 without touching glyph size, so there is a padding that
/// lands exactly on the box's aspect — solve
/// `(ink_w + 2p) / (ink_h + 2p) = target_w / target_h` for `p`. The result is
/// letterboxed, not stretched: the formula keeps its shape and sits centered
/// in its cells.
///
/// When the box is proportionally *wider* than the ink, the required `p` is
/// negative — padding can only ever move aspect toward square. There `p`
/// clamps to [`PADDING`] and the residual mismatch stands; it is bounded by
/// one cell of rounding.
pub fn render_fitted(
    tex: &str,
    em_px: u32,
    target_w: u32,
    target_h: u32,
) -> Result<Png, MathError> {
    let (em_w, em_h) = intrinsic_em_size(tex).ok_or(MathError::TooLarge { len: tex.len() })?;
    let ink_w = em_w * f64::from(em_px);
    let ink_h = em_h * f64::from(em_px);
    let (tw, th) = (f64::from(target_w.max(1)), f64::from(target_h.max(1)));

    // p = (ink_h*tw - ink_w*th) / (2*(th - tw)); guard the degenerate square
    // box, where the equation has no unique solution.
    let denom = 2.0 * (th - tw);
    let pad = if denom.abs() < f64::EPSILON {
        f64::from(PADDING)
    } else {
        ((ink_h * tw - ink_w * th) / denom).max(f64::from(PADDING))
    };
    let pad = if pad.is_finite() { pad as f32 } else { PADDING };
    render_padded(tex, em_px, pad)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    // `render`/`render_text` share process-global caches; serialize tests
    // that assert on cache *counts* so they don't observe each other's
    // increments.
    static TEST_LOCK: StdMutex<()> = StdMutex::new(());

    /// Acquires [`TEST_LOCK`], ignoring poisoning.
    ///
    /// A bare `.lock().unwrap()` turns the *first* failing cache-count
    /// assertion into a cascade: that test unwinds while holding the lock,
    /// poisoning it, and every other serialized test then panics on the
    /// unwrap instead of running. Seven "failures" for one real defect, and
    /// the real one is not obviously the real one. The lock guards nothing
    /// but test ordering, so a poisoned guard is still perfectly usable.
    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn decode_png_dims(bytes: &[u8]) -> (u32, u32) {
        image_dims_via_png_crate(bytes)
    }

    // Minimal PNG IHDR reader — avoids pulling in the `image` crate just
    // for a test assertion; reads width/height directly from the 8-byte
    // signature + IHDR chunk every PNG starts with.
    fn image_dims_via_png_crate(bytes: &[u8]) -> (u32, u32) {
        assert!(bytes.len() >= 24, "too short to be a PNG");
        let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        (width, height)
    }

    #[test]
    fn test_dw_6_2_ratex_rung_renders_a_fixture_set() {
        let _g = test_guard();
        reset_caches_for_test();
        for tex in [
            "x^2 + y^2 = z^2",
            r"\frac{a}{b}",
            r"\begin{matrix} a & b \\ c & d \end{matrix}",
            r"\begin{cases} x^2 & x \geq 0 \\ -x^2 & x < 0 \end{cases}",
        ] {
            let png = render(tex, 40).unwrap_or_else(|e| panic!("{tex:?} failed to render: {e}"));
            assert!(!png.as_bytes().is_empty());
            let (w, h) = decode_png_dims(png.as_bytes());
            assert!(w > 0 && h > 0, "{tex:?} rendered a zero-sized image");
        }
    }

    #[test]
    fn test_dw_6_2_ratex_failure_falls_to_txm_rung() {
        let _g = test_guard();
        // Genuinely invalid TeX: an unknown control sequence RaTeX rejects.
        let hostile = r"\thisisnotarealcommand{x}";
        let ratex_result = render(hostile, 40);
        assert!(
            matches!(ratex_result, Err(MathError::Parse(_))),
            "expected the RaTeX rung to fail on invalid input, got {ratex_result:?}"
        );
        // The ladder's next rung: a plain expression txm CAN handle, proving
        // the fallback path itself works (this test simulates "RaTeX rung
        // failed" by using input that fails RaTeX but is valid for txm,
        // since txm and RaTeX don't share a failure mode to inject into a
        // single string — the ladder is exercised end to end in
        // `stele::media::sink`'s integration test with an explicit
        // failure-injection switch).
        let grid = render_text("a+b").expect("txm should render a simple expression");
        assert!(!grid.rows.is_empty());
        assert!(grid.cols() > 0);
    }

    #[test]
    fn test_dw_6_2_txm_failure_falls_to_literal_rung() {
        // txm's own grammar is a strict subset of LaTeX; a construct it
        // doesn't recognize returns `None` here, which is exactly the
        // signal a caller uses to fall through to the literal-TeX-source
        // rung (no further code in this crate — see the module doc
        // comment).
        let unsupported = r"\begin{tikzpicture}\end{tikzpicture}";
        assert!(render_text(unsupported).is_none());
    }

    #[test]
    fn test_render_never_uses_default_black_ink_on_transparent_background() {
        let _g = test_guard();
        reset_caches_for_test();
        let png = render("x", 40).unwrap();
        let decoded = png_pixels_via_manual_decode(png.as_bytes());
        // At least one solid (alpha > 200) ink pixel must be far from
        // black — proves LayoutOptions.color was actually set, not left at
        // the library default that measured 1.24:1 on a dark background.
        let has_light_ink = decoded
            .iter()
            .any(|&(r, g, b, a)| a > 200 && (r as u16 + g as u16 + b as u16) > 300);
        assert!(
            has_light_ink,
            "expected a light (non-black) solid ink pixel; got none in {} pixels",
            decoded.len()
        );
        // Real transparency (per `ratex.md`'s own check b methodology): the
        // corner pixels, which are always outside the glyph ink, must be
        // alpha 0 — not a white RGB box masquerading as transparent.
        let corner = decoded.first().copied().expect("non-empty image");
        assert_eq!(
            corner.3, 0,
            "expected a transparent corner pixel, got {corner:?}"
        );
    }

    /// Full RGBA decode via the `png` crate transitively pulled in by
    /// `ratex-render` — reused here rather than adding a fresh dependency
    /// just for this one assertion.
    fn png_pixels_via_manual_decode(bytes: &[u8]) -> Vec<(u8, u8, u8, u8)> {
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().expect("known buffer size")];
        let info = reader.next_frame(&mut buf).unwrap();
        let bytes = &buf[..info.buffer_size()];
        bytes.chunks(4).map(|c| (c[0], c[1], c[2], c[3])).collect()
    }

    #[test]
    fn test_dw_6_5_display_list_cache_hit_on_px_height_change() {
        let _g = test_guard();
        reset_caches_for_test();
        let tex = r"\sqrt{x+1}";
        render(tex, 40).unwrap();
        let after_first = cache_stats();
        assert_eq!(after_first.display_list_misses, 1);
        assert_eq!(after_first.display_list_hits, 0);

        // Simulated cell-geometry change: same formula, different px size.
        render(tex, 80).unwrap();
        let after_second = cache_stats();
        assert_eq!(
            after_second.display_list_misses, 1,
            "a px_height-only change must not re-parse/re-layout"
        );
        assert_eq!(after_second.display_list_hits, 1);
        // The PNG cache, keyed by (tex, px_height), correctly treats this
        // as a distinct entry (a genuinely different raster is needed).
        assert_eq!(after_second.png_misses, 2);
    }

    #[test]
    fn test_png_cache_hits_on_byte_identical_repeat_request() {
        let _g = test_guard();
        reset_caches_for_test();
        let tex = "y = mx + b";
        render(tex, 40).unwrap();
        render(tex, 40).unwrap();
        let stats = cache_stats();
        assert_eq!(stats.png_misses, 1);
        assert_eq!(stats.png_hits, 1);
        // And the display-list layer was only ever computed once too.
        assert_eq!(stats.display_list_misses, 1);
    }

    #[test]
    fn test_oversized_tex_source_rejected_before_parsing() {
        let huge = "x".repeat(MAX_TEX_LEN + 1);
        let result = render(&huge, 40);
        assert!(matches!(result, Err(MathError::TooLarge { .. })));
        assert!(render_text(&huge).is_none());
    }

    #[test]
    fn test_px_height_is_clamped_not_trusted_bare() {
        let _g = test_guard();
        reset_caches_for_test();
        // A hostile/huge px_height must not make the rasterizer attempt an
        // unbounded pixmap allocation; clamped to MAX_PX_HEIGHT internally.
        let png = render("x", u32::MAX).expect("must clamp, not panic or OOM");
        let (_, h) = decode_png_dims(png.as_bytes());
        assert!(
            h < MAX_PX_HEIGHT * 4,
            "rendered height {h} suggests px_height was not clamped"
        );
    }

    #[test]
    fn test_intrinsic_em_size_is_cheap_and_non_rasterizing() {
        let _g = test_guard();
        reset_caches_for_test();
        let (w, h) = intrinsic_em_size("x+1").expect("valid TeX must size");
        assert!(w > 0.0 && h > 0.0);
        // Sizing populated the DisplayList cache; a subsequent render() of
        // the exact same source hits it rather than re-parsing.
        render("x+1", 40).unwrap();
        assert_eq!(cache_stats().display_list_misses, 1);
        assert_eq!(cache_stats().display_list_hits, 1);
    }

    #[test]
    fn test_intrinsic_em_size_none_on_parse_failure() {
        // Takes the lock despite asserting no counts: it still walks the
        // shared display-list cache, so without it this races the tests that
        // DO assert on miss counts.
        let _g = test_guard();
        assert!(intrinsic_em_size(r"\notarealcommand").is_none());
    }

    #[test]
    fn test_strip_ansi_sgr_removes_only_sgr_sequences() {
        let styled = "\x1b[1mbold\x1b[0m plain";
        assert_eq!(strip_ansi_sgr(styled), "bold plain");
    }

    /// The kitty protocol stretches a placement to fill its `c=` x `r=` cell
    /// box exactly, so a raster whose proportions disagree with the box shows
    /// up as visibly stretched glyphs. `render_fitted` letterboxes instead:
    /// same em (so glyph size stays consistent between formulas), padding
    /// solved so the aspect lands on the box's.
    #[test]
    fn test_render_fitted_matches_the_target_box_aspect_without_changing_em() {
        let _g = test_guard();
        reset_caches_for_test();
        // A formula with a superscript and one without: their em heights
        // differ a lot (0.96 vs 0.78), which is exactly what used to make a
        // box-derived em render them at different glyph sizes.
        for tex in [r"e^{i\pi}+1=0", "a+b=c"] {
            for (tw, th) in [(192u32, 48u32), (168, 48), (312, 96)] {
                let png = render_fitted(tex, 40, tw, th).expect("renders");
                let (rw, rh) = decode_png_dims(png.as_bytes());
                let raster_aspect = f64::from(rw) / f64::from(rh);
                let box_aspect = f64::from(tw) / f64::from(th);
                let err = (raster_aspect / box_aspect - 1.0).abs();
                assert!(
                    err < 0.10,
                    "{tex} into {tw}x{th}: raster {rw}x{rh} aspect {raster_aspect:.3} \
                     vs box {box_aspect:.3} ({:.1}% off)",
                    err * 100.0
                );
            }
        }
    }

    /// Padding can only push a raster's aspect toward square, so a box that is
    /// proportionally wider than the ink has no solution. That must clamp to
    /// the default margin and still produce a usable raster, never a negative
    /// padding or a panic.
    #[test]
    fn test_render_fitted_clamps_when_the_box_is_wider_than_the_ink() {
        let _g = test_guard();
        reset_caches_for_test();
        let png = render_fitted("a+b=c", 40, 4_000, 48).expect("still renders");
        let (rw, rh) = decode_png_dims(png.as_bytes());
        assert!(rw > 0 && rh > 0);
        // Unsolvable, so it falls back to the plain default-padding raster.
        let plain = render("a+b=c", 40).expect("renders");
        assert_eq!(decode_png_dims(plain.as_bytes()), (rw, rh));
    }
}
