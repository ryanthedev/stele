//! The media hook seam (P6 registers a real implementation here). P5 paints
//! with the true no-op default: images and math degrade to alt-text/TeX
//! runs at layout time ([`layout::NullSizer`]), so no `Reserved` box is
//! ever produced in this phase's own binary — but the seam is fully wired
//! and testable independently of that fact.

use std::io::Write;
use std::rc::Rc;

use ast::{Document, NodeId};
use layout::Reserved;

use crate::painter::CellRect;

/// The content-addressed on-disk store a fetched remote image lands in.
pub mod cache;
/// The network seam: the [`fetch::Fetcher`] trait, its bounds, and the one
/// real HTTP implementation. Nothing else in this workspace opens a socket.
pub mod fetch;
/// `--fetch-remote`: rewriting `![alt](https://…)` to a cache path *before*
/// the parse, so the existing local-image pipeline draws it unchanged.
pub mod remote;
mod residency;
mod rung;
mod sink;
mod sizer;
mod text_sink;

pub use fetch::{FetchError, FetchLimits, Fetched, Fetcher};
pub use remote::RemoteImages;
pub use sink::GfxMediaSink;
pub use sizer::ImageSizer;
pub use text_sink::TextMediaSink;

/// How much of the media ladder this session can actually use.
///
/// This replaced a single `graphics_disabled: bool`, and the reason is that
/// the bool conflated a *request* with a *capability*. Three different
/// conditions set it — `--no-images`, `$TMUX`, and a `TERM_PROGRAM` that is
/// not Ghostty — and the first is a reader saying what they want while the
/// other two are the terminal saying what it can carry. Those deserve
/// different answers, and with one bool there was nowhere to put the
/// difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaMode {
    /// Ghostty, outside tmux, images wanted: the full ladder.
    Full,
    /// The terminal cannot carry a raster — `$TMUX` is set (tmux does not pass
    /// the kitty APC through) or `TERM_PROGRAM` is not Ghostty.
    ///
    /// Images still degrade to alt text, exactly as before, because an image
    /// has no second rung. Display math does: `math::render_text` draws it as
    /// a Unicode grid, which is legible where raw TeX source is a wall of
    /// backslashes. Inline `$…$` math stays TeX source deliberately — a grid
    /// is two or three rows tall the moment a formula has a superscript, and
    /// `layout::inline` can only keep a box on the text line if it is one row
    /// (`inline.rs`'s `size.rows <= 1` arm). Gridding inline math would break
    /// every sentence containing a formula into three fragments, which is a
    /// worse read than the TeX it replaced.
    ///
    /// **Known cost, stated rather than hidden:** a display formula becomes a
    /// `Line::Reserved`, and a reserved box carries no text, so `/` cannot
    /// find a symbol inside one and `y` cannot yank it (see
    /// `app::append_line_text`, whose offsets must stay byte-identical to what
    /// is painted — injecting the TeX there would make a match highlight cells
    /// that paint a grid). This is not a new class of loss: under
    /// [`MediaMode::Full`] display math has always been a box and has never
    /// been searchable, so this makes a non-Ghostty terminal behave like
    /// Ghostty rather than inventing a new gap. Inline math, which is where
    /// the overwhelming majority of formulas live, stays searchable text.
    TextOnly,
    /// The reader asked for no images (`--no-images`): alt text and literal
    /// TeX source, no boxes reserved at all.
    ///
    /// This is the documented contract of the flag — `--help` and the README
    /// both promise "alt text / TeX source" — so it is the one mode that must
    /// keep behaving exactly as it did before `MediaMode` existed.
    Suppressed,
}

/// P6's hook: paints media into a reserved cell region.
pub trait MediaSink {
    /// Called once at the start of every frame, before any `paint`, whether
    /// or not the frame contains media.
    ///
    /// This is the sink's only reliable frame boundary. Inferring one from
    /// `paint` call order does not work: on a scroll-*up* every box's `rect.y`
    /// increases, so consecutive frames are indistinguishable from one long
    /// frame — placements then go stale and misaligned. Being called on
    /// media-free frames too is what lets a sink sweep a placement whose node
    /// has scrolled entirely out of view (no `paint` would ever fire for it).
    ///
    /// It is called from inside [`crate::painter::Painter`]'s mode-2026
    /// synchronized-update block, so a sink may take images *off* the screen
    /// here and let this frame's `paint` calls put back whatever is still
    /// visible, without the gap between the two ever reaching the glass.
    fn begin_frame(&mut self, _out: &mut dyn Write) {}

    /// Paints one row of a reserved media box into `rect`.
    ///
    /// Called once per row of the box that is on screen, in top-to-bottom
    /// order. Every subtle obligation of this seam is on this method, so they
    /// are written here rather than left to be reassembled from
    /// [`crate::painter`]'s call sites:
    ///
    /// - **`rect` is the position, and the only one.** The caller's cursor is
    ///   not on the box — an inline box's blanking spaces leave it a whole box
    ///   width to the right — so a sink that writes at the cursor writes
    ///   outside the box. Everything it emits must be positioned absolutely
    ///   from `rect`.
    /// - **`rect.height` is the box's visible remainder from this row down**,
    ///   not 1. On the box's first painted row that is its whole on-screen
    ///   extent, which is the one thing a sink cannot work out for itself: it
    ///   sees neither the viewport height nor how many rows scrolled off the
    ///   top. (For an inline box `rows == 1`, so the remainder *is* 1 — the
    ///   contract is uniform, not a special case.)
    /// - **`rect.x` is the box's real column**, after any container prefix; the
    ///   caller has already painted the prefix runs and does not expect them
    ///   back. [`Reserved`] deliberately carries no prefix data (it lives on
    ///   [`layout::ReservedLine`]), so there is nothing here to misread.
    /// - **`reserved.row` says which row *of the box* this is**, which differs
    ///   from the call's ordinal within the frame whenever the box's top has
    ///   scrolled above the viewport. It is what makes a top-truncated box
    ///   paintable instead of re-anchored to the first visible row.
    /// - **An inline box's cells are already blanked** by the caller, so a sink
    ///   that draws a transparent raster does not show the previous frame's
    ///   glyphs through it.
    /// - **The cursor may be left anywhere.** The caller re-homes it after an
    ///   inline box and does not depend on where a block box leaves it.
    fn paint(&mut self, reserved: &Reserved, rect: CellRect, out: &mut dyn Write);

    /// Drops `node_id`'s media entirely, on demand.
    ///
    /// **Not** the scroll-out path, despite the name: nothing in the viewer
    /// calls this. A node that scrolls out of view is handled by the frame
    /// boundary — it simply stops being painted, and `begin_frame` takes it
    /// off the screen. This remains as an explicit "forget this node" hook
    /// (a document reload, a node that no longer exists) and is exercised by
    /// tests only. The trait doc used to describe it as the scroll-out
    /// mechanism, which stopped being true when the sweep moved into
    /// `begin_frame`.
    ///
    /// Defaulted to a no-op for that reason: with no caller, requiring every
    /// implementor to write an empty body only spreads the dead weight.
    fn evict(&mut self, _node_id: NodeId, _out: &mut dyn Write) {}

    /// The document was reloaded (`--watch`); every `NodeId` the sink is
    /// holding now names a node in a document that no longer exists.
    ///
    /// A sink that resolves nodes against its own copy of the AST must swap
    /// to `doc` here **and** drop whatever it cached per node — a re-parse
    /// renumbers nodes, so a retained cache silently starts answering with
    /// the wrong node's content. `out` is the same writer a frame is painted
    /// through, so a sink holding terminal-side state (a kitty placement) can
    /// take it off the screen on the way past; this is called between frames,
    /// outside any synchronized-update block.
    ///
    /// Defaulted to a no-op: a sink that keeps no per-node state has nothing
    /// to do, and [`NoopMediaSink`] should not have to say so.
    fn reload_document(&mut self, _doc: Rc<Document>, _out: &mut dyn Write) {}
}

/// The no-media default: a true no-op on every call.
#[derive(Debug, Default)]
pub struct NoopMediaSink;

impl MediaSink for NoopMediaSink {
    fn paint(&mut self, _reserved: &Reserved, _rect: CellRect, _out: &mut dyn Write) {}
}
