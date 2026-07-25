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

/// Projected pixmap area, in pixels, for a layout of em extent
/// `(em_w, em_h)` rendered at `font` with `padding` px of margin on every
/// side. **The padding term is not optional:** [`render_fitted`] solves for a
/// letterbox margin up to [`MAX_PADDING`] on each side, which adds up to
/// `2 * MAX_PADDING` to *both* axes. Counting only `em * font` let a wide
/// formula's solved margin blow the budget by 8x — measured 63.5 megapixels
/// against a stated 8-megapixel ceiling on `testdocs/02-math.md`'s own
/// 3423-character formula (`cargo run -p math --example fitted_probe`).
fn projected_px(em_w: f64, em_h: f64, font: u32, padding: f64) -> u64 {
    let f = f64::from(font);
    let margin = 2.0 * padding.max(0.0);
    let w = (em_w * f + margin).ceil().max(1.0);
    let h = (em_h * f + margin).ceil().max(1.0);
    // Saturate rather than wrap on an absurd extent.
    if w > u64::MAX as f64 || h > u64::MAX as f64 {
        return u64::MAX;
    }
    (w as u64).saturating_mul(h as u64)
}

/// Returns the largest font size `<= requested` whose projected pixmap —
/// **including `padding`** — stays within [`MAX_PIXMAP_PX`], or
/// `PixmapTooLarge` if even [`MIN_PX_HEIGHT`] does not fit. Degenerate (zero /
/// non-finite) extents pass through unchanged — there is nothing to bound, and
/// the rasterizer handles them.
fn fit_font_size(
    list: &ratex_types::display_item::DisplayList,
    requested: u32,
    padding: f64,
) -> Result<u32, MathError> {
    let em_w = list.width;
    let em_h = list.height + list.depth;
    if !em_w.is_finite() || !em_h.is_finite() || em_w <= 0.0 || em_h <= 0.0 {
        return Ok(requested);
    }
    let px_at = |font: u32| projected_px(em_w, em_h, font, padding);
    if px_at(requested) <= MAX_PIXMAP_PX {
        return Ok(requested);
    }
    // `px_at` is monotone non-decreasing in `font`, so binary-search the
    // largest fitting size rather than solving a closed form. The closed form
    // this replaces (`sqrt(MAX / (em_w*em_h))`) silently assumed area scales
    // as `font^2`, which stops being true the moment a constant padding term
    // is in the product.
    let (mut lo, mut hi) = (MIN_PX_HEIGHT, requested.max(MIN_PX_HEIGHT));
    if px_at(lo) > MAX_PIXMAP_PX {
        return Err(MathError::PixmapTooLarge { px: px_at(lo) });
    }
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if px_at(mid) <= MAX_PIXMAP_PX {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    Ok(lo)
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
    // layout's em extent and with the letterbox padding as well, so a
    // wide/tall construct within the MAX_TEX_LEN budget can demand an enormous
    // raster at a legal font size. Shrink the font to fit the pixel budget;
    // only if even MIN_PX_HEIGHT overflows it do we fail, and then the caller
    // falls to the txm rung.
    let px_height = fit_font_size(&display_list, px_height, f64::from(padding))?;

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

    // Insert under the key the lookup above *used*, not under the
    // pixmap-fitted font size. `fit_font_size` may have shrunk `px_height`,
    // and keying the insert on the shrunk value meant a formula big enough to
    // need shrinking could never hit its own cache entry: every repaint
    // re-rasterized it from scratch.
    insert_bounded(&mut png_cache().lock().unwrap(), cache_key, png.clone());
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
///
/// **The em is a ceiling, not a promise.** When the ink at `em_px` is *larger*
/// than the box (layout scaled a wide formula down to the content column), the
/// terminal is going to shrink the raster to fit regardless — rasterizing at
/// the full em only buys a bigger pixmap, a bigger PNG on the wire, and a
/// round of terminal resampling. `testdocs/02-math.md`'s longest formula (3423
/// characters — its own prose calls it 4095, which is wrong) measured
/// a 59984x1058 raster (63.5 megapixels, a 2 MB PNG) for a box 2400x48 px
/// wide. [`em_capped_to_box`] shrinks the em to whatever actually fits, which
/// lands the same glyph size on screen at 1:1 instead of via a 25x downscale.
/// Every formula that *does* fit its box is untouched, so the fixed-em promise
/// holds exactly where it is observable.
pub fn render_fitted(
    tex: &str,
    em_px: u32,
    target_w: u32,
    target_h: u32,
) -> Result<Png, MathError> {
    let (em_w, em_h) = intrinsic_em_size(tex).ok_or(MathError::TooLarge { len: tex.len() })?;
    let (tw, th) = (f64::from(target_w.max(1)), f64::from(target_h.max(1)));
    let em_px = em_capped_to_box(em_w, em_h, em_px, tw, th);
    let ink_w = em_w * f64::from(em_px);
    let ink_h = em_h * f64::from(em_px);

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

/// The largest em `<= em_px` whose ink fits inside a `tw` x `th` pixel box,
/// floored at [`MIN_PX_HEIGHT`]. Returns `em_px` unchanged when the ink
/// already fits (the common case — the sizer reserves a box from the same em)
/// or when the extent is degenerate.
fn em_capped_to_box(em_w: f64, em_h: f64, em_px: u32, tw: f64, th: f64) -> u32 {
    if !em_w.is_finite() || !em_h.is_finite() || em_w <= 0.0 || em_h <= 0.0 {
        return em_px;
    }
    let (ink_w, ink_h) = (em_w * f64::from(em_px), em_h * f64::from(em_px));
    if ink_w <= tw && ink_h <= th {
        return em_px;
    }
    let scale = (tw / ink_w).min(th / ink_h);
    if !scale.is_finite() || scale <= 0.0 {
        return MIN_PX_HEIGHT;
    }
    ((f64::from(em_px) * scale)
        .floor()
        .max(f64::from(MIN_PX_HEIGHT)) as u32)
        .min(em_px)
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

    /// Four inline formulas, asserting "the PNG is non-empty and its IHDR
    /// dimensions are nonzero". Kept as a smoke test of the bare [`render`]
    /// entry point (which the sink never calls — it calls
    /// [`render_fitted`]), **not** as DW-6.2's evidence: a renderer that
    /// emitted a 1x1 fully transparent pixel for every formula in the corpus
    /// passes every line of it. What it cannot see is whether any ink landed,
    /// whether the raster's size has anything to do with the formula, and
    /// whether the letterbox is transparent padding or an opaque bar. The
    /// gate's "math fixture set renders" half is discharged by
    /// [`test_dw_6_2_every_fixture_formula_rasterizes_real_ink_into_its_letterbox`]
    /// against the real corpus on disk.
    #[test]
    fn test_render_produces_a_nonempty_sized_png_for_a_few_formulas() {
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

    /// A formula short enough to put in a panic message. The fixture set
    /// includes a 3423-character one; printing it whole buries the assertion
    /// that failed under three screens of `+ 1`.
    fn short(tex: &str) -> String {
        let head: String = tex.chars().take(48).collect();
        if head.chars().count() < tex.chars().count() {
            format!("{head:?}… ({} chars)", tex.chars().count())
        } else {
            format!("{head:?}")
        }
    }

    /// One formula lifted out of a real fixture document, with the cell box
    /// the shipped pipeline would hand it.
    struct FixtureFormula {
        /// Repo-relative path of the document it came from — in the failure
        /// message, so a regression names a file a human can open.
        file: &'static str,
        tex: String,
    }

    /// Every distinct math span in DW-6.2's fixture set, extracted with the
    /// **real parser** rather than a `$`-scanner of the test's own: both
    /// documents deliberately contain prose dollar signs (`I paid $5 and $10`)
    /// that are not math, and a hand-rolled extractor that disagreed with
    /// `ast` would test formulas the product never sees while missing ones it
    /// does.
    ///
    /// De-duplicated by TeX source: `testdocs/02-math.md` repeats `a+b=c` in
    /// nine positions to exercise P4's wrapping, which is not this crate's
    /// concern — 49 spans collapse to 33 distinct formulas.
    fn fixture_formulas() -> Vec<FixtureFormula> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("crate lives two levels under the repo root");
        let mut seen = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for file in ["fixtures/math.md", "testdocs/02-math.md"] {
            let path = root.join(file);
            let src = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("DW-6.2's fixture set must exist at {path:?}: {e}"));
            let doc = ast::Document::parse(&src);
            let before = out.len();
            for node in doc.nodes() {
                if let ast::NodeRef::Inline(inline) = node
                    && let ast::InlineKind::Math { tex, .. } = &inline.kind
                    && seen.insert(tex.clone())
                {
                    out.push(FixtureFormula {
                        file,
                        tex: tex.clone(),
                    });
                }
            }
            assert!(
                out.len() > before,
                "{file} contributed no math spans — the fixture set went missing or the \
                 parser stopped recognizing `$…$`, and every per-formula assertion below \
                 would pass vacuously"
            );
        }
        out
    }

    /// The cell box the shipped pipeline lands on for a formula of em extent
    /// `(em_w, em_h)` at a `content_cols`-wide content column, mirroring
    /// `stele::media::sizer`'s `size_math` + `px_to_cells` (the 200-cell cap
    /// rescales *both* axes) and `layout::block`'s `emit_box` (a box wider
    /// than the content column is scaled down preserving aspect). Returned in
    /// pixels, which is what `stele::media::sink` passes to
    /// [`render_fitted`]: `rect.width * cell_px.0` by `reserved.rows *
    /// cell_px.1`, at the sink's 24x48 cell.
    ///
    /// Mirrored rather than imported because `stele` depends on `math`, not
    /// the other way round. The point of mirroring it at all is that
    /// [`render_fitted`]'s letterbox is only correct *relative to the box it
    /// is given*, so testing it against boxes the product never produces
    /// would prove nothing about what a reader sees.
    fn pipeline_target_px(em_w: f64, em_h: f64, content_cols: u64) -> (u32, u32) {
        const CELL: (u32, u32) = (24, 48);
        const MAX_RESERVED: u64 = 200;
        let px_w = ((em_w * 40.0).max(0.0) as u32).max(1);
        let px_h = ((em_h * 40.0).max(0.0) as u32).max(1);
        let mut cols = u64::from(px_w.div_ceil(CELL.0)).max(1);
        let mut rows = u64::from(px_h.div_ceil(CELL.1)).max(1);
        if cols > MAX_RESERVED {
            rows = (rows * MAX_RESERVED).div_ceil(cols).max(1);
            cols = MAX_RESERVED;
        }
        if rows > MAX_RESERVED {
            cols = (cols * MAX_RESERVED).div_ceil(rows).max(1);
            rows = MAX_RESERVED;
        }
        if cols > content_cols {
            rows = (rows * content_cols).div_ceil(cols).max(1);
            cols = content_cols;
        }
        (cols as u32 * CELL.0, rows as u32 * CELL.1)
    }

    /// What a rendered raster's ink actually looks like.
    struct Ink {
        /// Size of the tightest axis-aligned box containing every pixel that
        /// carries any alpha at all — the ink's real extent, as opposed to the
        /// raster's, which includes the letterbox.
        bbox: (u32, u32),
        /// How many pixels carry any alpha.
        count: u64,
        /// RGBA of the most opaque ink pixel: the ink color, undiluted by
        /// antialiasing.
        strongest: (u8, u8, u8, u8),
    }

    /// Measures [`Ink`] over a decoded RGBA raster `w` px wide. `None` when
    /// not one pixel carries alpha — a blank canvas.
    fn ink_extent(w: u32, px: &[(u8, u8, u8, u8)]) -> Option<Ink> {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        let mut count = 0u64;
        let mut strongest = (0u8, 0u8, 0u8, 0u8);
        for (i, p) in px.iter().enumerate() {
            if p.3 == 0 {
                continue;
            }
            count += 1;
            let i = u32::try_from(i).expect("raster indices fit u32 under MAX_PIXMAP_PX");
            let (x, y) = (i % w, i / w);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            if p.3 > strongest.3 {
                strongest = *p;
            }
        }
        if count == 0 {
            return None;
        }
        Some(Ink {
            bbox: (x1 - x0 + 1, y1 - y0 + 1),
            count,
            strongest,
        })
    }

    /// DW-6.2, render half: *"math fixture set renders"*, asserted on the
    /// pixels of every formula in the fixture set on disk
    /// (`fixtures/math.md` + `testdocs/02-math.md`, 33 distinct formulas),
    /// through [`render_fitted`] — the entry point the sink actually calls —
    /// at the cell box the shipped sizer/layout would hand each one.
    ///
    /// Four properties, each chosen because a renderer that got it wrong
    /// would put something wrong on the reader's screen while the old
    /// four-formula "non-empty PNG, nonzero IHDR" check stayed green:
    ///
    /// 1. **Ink is there.** At least one pixel carries alpha, the ink spans
    ///    more than one pixel on each axis, it covers ≥0.5% of the raster,
    ///    and the most opaque ink pixel is light rather than the library
    ///    default black. A blank or all-but-blank letterbox — the actual
    ///    failure mode when a rung silently produces an empty canvas — fails
    ///    here.
    /// 2. **The ink never exceeds the extent the layout declared** for it:
    ///    `ink_bbox <= em_extent * effective_em + 3px` (the slack is
    ///    antialiasing spilling past the em box). This is the tight half of
    ///    "dimensions track the formula's extent"; the loose half is the 0.65
    ///    floor below, which only has to exclude a shrunken or vestigial
    ///    raster.
    /// 3. **The raster's aspect is the one the letterbox contract predicts.**
    ///    Symmetric padding can only move a raster's aspect *toward* 1:1, so
    ///    the reachable set is the interval between 1 and the ink's own
    ///    aspect at minimum padding; the contract is "land on the box's
    ///    aspect if it is in that interval, otherwise sit at the interval's
    ///    edge". Predicted from the em extent and the box, then checked
    ///    against the decoded IHDR — so it fails if the solve, the
    ///    [`MAX_PADDING`] clamp, [`em_capped_to_box`], or the rasterizer's
    ///    own padding handling breaks. Worst observed error over the corpus:
    ///    2.5%, asserted at 8% to leave room for ±1px rounding on the
    ///    smallest rasters.
    /// 4. **The letterbox is padding, not a bar.** The entire outermost ring
    ///    of pixels is alpha 0. `test_render_never_uses_default_black_ink…`
    ///    checks one corner of one formula; an opaque background that
    ///    happened to leave a transparent corner, or a bar on one edge only,
    ///    survives that and not this.
    ///
    /// **What this cannot catch:** whether the glyphs are the *right* glyphs.
    /// Nothing here would notice RaTeX rendering `b` where the source said
    /// `a`, or transposing a matrix — that needs a golden-image corpus, and a
    /// third-party rasterizer's antialiasing makes byte-exact goldens a
    /// maintenance trap. It also cannot catch a wrong-but-plausible letterbox
    /// *contract* (property 3 mirrors the contract rather than deriving it
    /// from first principles); the residual-stretch bound in
    /// [`test_dw_6_2_letterbox_stretch_on_a_square_box_is_bounded_and_unfixed`]
    /// is where that gap is stated in terms of what the reader sees.
    ///
    /// **One measured result worth knowing, which these thresholds accept.**
    /// `testdocs/02-math.md`'s 3423-character formula is ~1474 em wide against
    /// a 2400 px box, so [`em_capped_to_box`] takes its em all the way to the
    /// [`MIN_PX_HEIGHT`] floor of 4. Its strokes then go sub-pixel: the raster
    /// is 6012x120 with ink in 1.0% of it, an ink bbox 4 px tall, and a peak
    /// alpha of 128 — i.e. it passes "ink is there" while being, on screen,
    /// a faint smear the terminal then downscales 2.5x further. That is the
    /// honest outcome for a formula thirty times wider than the viewport, not
    /// a bug this crate can fix by itself, but it is the reason the ink
    /// threshold here is `alpha > 0` rather than "solid ink".
    #[test]
    fn test_dw_6_2_every_fixture_formula_rasterizes_real_ink_into_its_letterbox() {
        let _g = test_guard();
        reset_caches_for_test();
        let corpus = fixture_formulas();
        assert!(
            corpus.len() >= 25,
            "DW-6.2's fixture set shrank to {} formulas; it is supposed to cover the whole \
             ladder plus matrices, big operators, and the near-pixmap-bound case",
            corpus.len()
        );

        let mut rendered = 0usize;
        for FixtureFormula { file, tex } in &corpus {
            // Formulas RaTeX rejects belong to the txm/literal rungs, which
            // produce no raster at all; which formulas those are is pinned
            // exactly by `test_dw_6_2_the_fixture_corpus_lands_on_the_rung_each_document_promises`.
            let Some((em_w, em_h)) = intrinsic_em_size(tex) else {
                continue;
            };
            rendered += 1;
            let (target_w, target_h) = pipeline_target_px(em_w, em_h, 100);
            let png = render_fitted(tex, 40, target_w, target_h)
                .unwrap_or_else(|e| panic!("{file}: {} failed to render: {e}", short(tex)));
            let (w, h) = decode_png_dims(png.as_bytes());
            let px = png_pixels_via_manual_decode(png.as_bytes());
            assert_eq!(
                px.len() as u64,
                u64::from(w) * u64::from(h),
                "{file}: {} IHDR says {w}x{h} but decoded {} pixels",
                short(tex),
                px.len()
            );
            // Slack on today's corpus, and labelled as such: the largest
            // fixture raster is 6012x120 = 0.72 Mpx against an 8 Mpx ceiling,
            // so this cannot fail as things stand — mutating the padding term
            // out of `projected_px` leaves it green, and only
            // `test_pixmap_budget_counts_the_letterbox_padding` notices. It is
            // here as a ceiling on what the *corpus* may grow into, not as
            // evidence the budget works.
            let area = u64::from(w) * u64::from(h);
            assert!(
                area <= MAX_PIXMAP_PX,
                "{file}: {} rasterized {w}x{h} = {area} px, past the stated \
                 {MAX_PIXMAP_PX} px ceiling",
                short(tex)
            );

            // (1) Ink is there, and it is light.
            let ink = ink_extent(w, &px).unwrap_or_else(|| {
                panic!(
                    "{file}: {} rasterized {w}x{h} with not one pixel of ink",
                    short(tex)
                )
            });
            let (ink_w, ink_h) = ink.bbox;
            let strongest = ink.strongest;
            assert!(
                ink_w >= 2 && ink_h >= 2,
                "{file}: {} ink collapsed to {ink_w}x{ink_h} px in a {w}x{h} raster",
                short(tex)
            );
            let coverage = ink.count as f64 / area as f64;
            assert!(
                coverage >= 0.005,
                "{file}: {} ink covers {:.4}% of its {w}x{h} raster — effectively a \
                 blank letterbox",
                short(tex),
                coverage * 100.0
            );
            let luma = u16::from(strongest.0) + u16::from(strongest.1) + u16::from(strongest.2);
            assert!(
                luma > 300,
                "{file}: {} strongest ink pixel is {strongest:?} — dark ink on a \
                 transparent background is the 1.24:1-contrast bug `ratex.md` measured",
                short(tex)
            );

            // (2)+(3) The raster's size and shape follow the layout's em
            // extent and the box, per the letterbox contract.
            let em_px = em_capped_to_box(em_w, em_h, 40, f64::from(target_w), f64::from(target_h));
            let fits_at_the_fixed_em =
                em_w * 40.0 <= f64::from(target_w) && em_h * 40.0 <= f64::from(target_h);
            assert_eq!(
                em_px == 40,
                fits_at_the_fixed_em,
                "{file}: {} em was capped to {em_px} but its ink {:.0}x{:.0} px \
                 {} inside the {target_w}x{target_h} box — the fixed-em promise applies \
                 exactly when the ink fits",
                short(tex),
                em_w * 40.0,
                em_h * 40.0,
                if fits_at_the_fixed_em {
                    "does sit"
                } else {
                    "does not sit"
                },
            );
            let (ink_px_w, ink_px_h) = (em_w * f64::from(em_px), em_h * f64::from(em_px));
            // 3 px of slack, from measurement: antialiased strokes spill past
            // the typographic em box, worst observed +2.44 px (on the height
            // axis, `a \leq b \neq c \approx d`) and +0.05 px on the width.
            assert!(
                f64::from(ink_w) <= ink_px_w + 3.0 && f64::from(ink_h) <= ink_px_h + 3.0,
                "{file}: {} inked {ink_w}x{ink_h} px, past the {ink_px_w:.1}x{ink_px_h:.1} \
                 px extent its own layout declared at em {em_px}",
                short(tex)
            );
            assert!(
                f64::from(ink_w) >= 0.65 * ink_px_w && f64::from(ink_h) >= 0.65 * ink_px_h,
                "{file}: {} inked only {ink_w}x{ink_h} px of a declared \
                 {ink_px_w:.0}x{ink_px_h:.0} px extent at em {em_px}",
                short(tex)
            );

            let ink_aspect_at_min_padding =
                (ink_px_w + 2.0 * f64::from(PADDING)) / (ink_px_h + 2.0 * f64::from(PADDING));
            let box_aspect = f64::from(target_w) / f64::from(target_h);
            let expected = expected_raster_aspect(box_aspect, ink_aspect_at_min_padding);
            let actual = f64::from(w) / f64::from(h);
            let err = (actual / expected - 1.0).abs();
            assert!(
                err < 0.08,
                "{file}: {} in a {target_w}x{target_h} box rasterized {w}x{h} \
                 (aspect {actual:.3}); the letterbox contract predicts {expected:.3} \
                 ({:.1}% off)",
                short(tex),
                err * 100.0
            );

            // (4) The letterbox is transparent padding on every edge.
            let opaque_border = (0..w)
                .flat_map(|x| [(x, 0), (x, h - 1)])
                .chain((0..h).flat_map(|y| [(0, y), (w - 1, y)]))
                .filter(|&(x, y)| px[(y * w + x) as usize].3 != 0)
                .count();
            assert_eq!(
                opaque_border,
                0,
                "{file}: {} has {opaque_border} non-transparent pixels on the edge of \
                 its {w}x{h} raster — the letterbox must be padding, not an opaque bar the \
                 terminal background cannot show through",
                short(tex)
            );
        }
        assert!(
            rendered >= 25,
            "only {rendered} of {} fixture formulas reached the RaTeX rung",
            corpus.len()
        );
    }

    /// The aspect [`render_fitted`]'s contract predicts for a raster, given
    /// the target box's aspect and the ink's own aspect at minimum padding.
    ///
    /// Padding is symmetric, so it can only ever move a raster's aspect
    /// *toward* 1:1: the reachable set is the interval bounded by 1 and
    /// `ink_aspect`. A box aspect inside that interval is hit exactly; one
    /// outside it clamps to the interval's ink-side edge, since padding
    /// smaller than [`PADDING`] is not on offer. A square box is the one case
    /// with no solution at all (`w == h` forces `ink_w == ink_h`), and
    /// [`render_fitted`] takes the default margin there.
    fn expected_raster_aspect(box_aspect: f64, ink_aspect: f64) -> f64 {
        if (box_aspect - 1.0).abs() < 1e-9 {
            return ink_aspect;
        }
        if ink_aspect >= 1.0 {
            box_aspect.clamp(1.0, ink_aspect)
        } else {
            box_aspect.clamp(ink_aspect, 1.0)
        }
    }

    /// DW-6.2's rung selection, on the fixture set rather than on injected
    /// failures (the injected-failure ladder lives in
    /// `stele::media::sink::test_dw_6_2_full_ladder_via_injected_failures`).
    ///
    /// `testdocs/02-math.md` documents, per section, exactly which rung each
    /// of its formulas is supposed to land on. Two of them are there to
    /// exercise the ladder: `\dv{y}{x}` (physics-package notation RaTeX's
    /// KaTeX-derived parser does not implement, but txm does) and
    /// `\notarealcommand{` (rejected by both, so P4/P5 shows the literal
    /// source). Pinning the reject set to *exactly* those two is what keeps
    /// the render test above honest: it skips formulas the RaTeX rung
    /// declines, so a RaTeX regression that started rejecting half the corpus
    /// would otherwise quietly shrink its coverage instead of failing.
    #[test]
    fn test_dw_6_2_the_fixture_corpus_lands_on_the_rung_each_document_promises() {
        let _g = test_guard();
        let corpus = fixture_formulas();
        // Compared in `short` form so a regression that declines the 3423-char
        // formula prints a diff a human can read.
        let declined: Vec<String> = corpus
            .iter()
            .filter(|f| intrinsic_em_size(&f.tex).is_none())
            .map(|f| short(&f.tex))
            .collect();
        assert_eq!(
            declined,
            vec![short(r"\dv{y}{x}"), short(r"\notarealcommand{")],
            "the RaTeX rung must decline exactly the two formulas the fixture set puts there \
             on purpose, in document order"
        );

        // Rung 2: txm renders what RaTeX would not.
        let grid = render_text(r"\dv{y}{x}")
            .expect("`\\dv{y}{x}` is in the corpus to prove the txm rung catches a RaTeX reject");
        assert!(grid.cols() > 0 && grid.rows_count() > 0);
        assert!(
            grid.rows.iter().any(|r| !r.trim().is_empty()),
            "the txm rung produced a grid of blank rows: {grid:?}"
        );
        // Rung 3: both rungs decline, so nothing but the literal source is left.
        assert!(render_text(r"\notarealcommand{").is_none());
    }

    /// **A defect this crate cannot currently fix, bounded so it cannot get
    /// worse.**
    ///
    /// [`render_fitted`] letterboxes with `ratex_render`'s single symmetric
    /// `padding`, which moves a raster's aspect toward 1:1 and no other way.
    /// A target box whose aspect sits *outside* the interval between 1 and the
    /// ink's own aspect is therefore unreachable — most importantly every
    /// square box, where matching would require `ink_w == ink_h`. The kitty
    /// protocol scales a placement to fill its `c=` x `r=` cell box exactly,
    /// so the residual is a visible stretch: `x_1` reserves 2 cols x 1 row
    /// (48x48 px), rasterizes 47x32, and lands on screen stretched 1.47x
    /// vertically.
    ///
    /// [`test_render_fitted_matches_the_target_box_aspect_without_changing_em`]
    /// misses this entirely because all three of its boxes are wide enough for
    /// the solve to succeed. Fixing it needs per-axis padding, which means
    /// composing the raster onto a larger transparent canvas here (a new
    /// runtime image dependency and a two-axis cache key) or letting the sink
    /// place a box the raster's own aspect fits — neither is a change this
    /// test's scope can make. So this records the measured worst case instead:
    /// the numbers below are today's residual, not a target.
    #[test]
    fn test_dw_6_2_letterbox_stretch_on_a_square_box_is_bounded_and_unfixed() {
        let _g = test_guard();
        reset_caches_for_test();
        let mut worst: (f64, String) = (0.0, String::new());
        let mut stretched = 0usize;
        for FixtureFormula { tex, .. } in &fixture_formulas() {
            let Some((em_w, em_h)) = intrinsic_em_size(tex) else {
                continue;
            };
            let (target_w, target_h) = pipeline_target_px(em_w, em_h, 100);
            let png = render_fitted(tex, 40, target_w, target_h).expect("renders");
            let (w, h) = decode_png_dims(png.as_bytes());
            let box_aspect = f64::from(target_w) / f64::from(target_h);
            let stretch = ((f64::from(w) / f64::from(h)) / box_aspect - 1.0).abs();
            if stretch > 0.10 {
                stretched += 1;
            }
            if stretch > worst.0 {
                worst = (stretch, tex.clone());
            }
        }
        assert!(
            worst.0 < 0.50,
            "worst letterbox residual grew to {:.1}% on {}; the terminal stretches the \
             raster by exactly that much",
            worst.0 * 100.0,
            short(&worst.1)
        );
        assert!(
            stretched <= 8,
            "{stretched} fixture formulas now miss their box aspect by more than 10% (was 8: \
             the single-cell-wide and square-box cases). A new one means the letterbox got \
             worse, not that this bound is wrong."
        );
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
    ///
    /// **Scope, stated because the name overclaims:** all three boxes here are
    /// proportionally narrower than their formula's ink, which is exactly the
    /// case where the padding solve has a solution. Symmetric padding cannot
    /// reach a box aspect outside the interval between 1 and the ink's own
    /// aspect, and 8 of the 33 real fixture formulas sit outside it — see
    /// [`test_dw_6_2_letterbox_stretch_on_a_square_box_is_bounded_and_unfixed`].
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

    /// A formula whose ink is far wider than its cell box used to rasterize at
    /// the full fixed em anyway and let the terminal scale it down. Measured on
    /// `testdocs/02-math.md`'s own longest (3423-character) formula: a 59984x1058
    /// raster — 63.5
    /// megapixels against this crate's stated 8-megapixel ceiling, a 2 MB PNG
    /// pushed down the wire, 345 ms to render (release) — for a box 2400x48 px.
    ///
    /// Why the suite missed it: `MAX_PIXMAP_PX` *was* enforced, by
    /// `fit_font_size`, which computed area as `em * font` and never counted
    /// the up-to-`2 * MAX_PADDING` the letterbox adds to each axis. The bound
    /// was asserted at the wrong level: on the layout's em extent rather than
    /// on the pixmap actually allocated.
    #[test]
    fn test_render_fitted_stays_inside_the_pixmap_budget_and_its_own_box() {
        let _g = test_guard();
        reset_caches_for_test();
        // ~1470 em wide, well under MAX_TEX_LEN — the shape `02-math.md`'s
        // "absurdly long formula near the pixmap bound" section exercises.
        let wide = format!("x{}= y", " + 1".repeat(950));
        assert!(wide.len() < MAX_TEX_LEN);
        // The box layout lands on: 100 content columns, one row.
        let (target_w, target_h) = (100 * 24, 48);
        let png = render_fitted(&wide, 40, target_w, target_h).expect("must still render");
        let (w, h) = decode_png_dims(png.as_bytes());
        let px = u64::from(w) * u64::from(h);
        assert!(
            px <= MAX_PIXMAP_PX,
            "raster {w}x{h} = {px} px exceeds the stated {MAX_PIXMAP_PX} px ceiling"
        );
        // And the em must actually have been capped toward the box. The only
        // thing keeping this raster above `target_w` at all is the
        // MIN_PX_HEIGHT floor (~1470 em of glyphs cannot fit 2400 px at a
        // legible em), so that floor — plus one letterbox margin per side — is
        // the real upper bound, and it is ~9x tighter than the uncapped em's
        // 59984 px.
        let (em_w, em_h) = intrinsic_em_size(&wide).unwrap();
        let floor_w = (em_w * f64::from(MIN_PX_HEIGHT)).ceil() as u32 + 2 * MAX_PADDING as u32;
        assert!(
            w <= floor_w,
            "raster width {w} exceeds the MIN_PX_HEIGHT floor bound {floor_w} \
             (uncapped em would have rasterized {}px wide)",
            (em_w * 40.0) as u32
        );
        // The width bound above is necessary but not sufficient: with
        // `em_capped_to_box` neutered, `fit_font_size` shrinks the font on its
        // own to stay inside MAX_PIXMAP_PX and the raster still lands under
        // `floor_w` — so the bound alone cannot tell "capped to the box" from
        // "clipped by the pixel budget". Assert the cap directly.
        assert!(
            em_capped_to_box(em_w, em_h, 40, f64::from(target_w), f64::from(target_h)) < 40,
            "a formula {em_w:.0} em wide must have its em capped down from 40 for a \
             {target_w}x{target_h} px box"
        );
    }

    /// The pixmap ceiling must be computed on the pixmap, padding included.
    #[test]
    fn test_pixmap_budget_counts_the_letterbox_padding() {
        let list = ratex_types::display_item::DisplayList {
            width: 100.0,
            height: 1.0,
            depth: 0.0,
            ..Default::default()
        };
        // Without padding, font 40 projects 4000 x 40 = 160k px — trivially
        // inside the budget. With a full MAX_PADDING letterbox it projects
        // 5024 x 1064 = 5.3M, and at font 300 it projects 31024 x 1324 = 41M,
        // which must be rejected down to something that fits.
        let unpadded = fit_font_size(&list, 300, 0.0).unwrap();
        let padded = fit_font_size(&list, 300, f64::from(MAX_PADDING)).unwrap();
        assert!(
            padded < unpadded,
            "padding must tighten the fitted font size: {padded} vs {unpadded}"
        );
        assert!(projected_px(100.0, 1.0, padded, f64::from(MAX_PADDING)) <= MAX_PIXMAP_PX);
    }

    /// A formula big enough for `fit_font_size` to shrink used to be inserted
    /// into the PNG cache under the *shrunk* font size while every lookup used
    /// the *requested* one — so it could never hit its own entry and was
    /// re-rasterized on every single repaint.
    #[test]
    fn test_png_cache_hits_even_when_the_pixmap_budget_shrank_the_font() {
        let _g = test_guard();
        reset_caches_for_test();
        let tex = r"\cfrac{1}{1+\cfrac{1}{1+\cfrac{1}{1+\cfrac{1}{1+x}}}}";
        // 512 px em on this layout projects past MAX_PIXMAP_PX once padding
        // is counted, so `fit_font_size` genuinely shrinks it.
        render(tex, MAX_PX_HEIGHT).unwrap();
        assert_eq!(cache_stats().png_misses, 1);
        render(tex, MAX_PX_HEIGHT).unwrap();
        assert_eq!(
            cache_stats().png_hits,
            1,
            "a pixmap-shrunk render must still populate its own cache key"
        );
        assert_eq!(cache_stats().png_misses, 1);
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
