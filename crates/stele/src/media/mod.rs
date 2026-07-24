//! The media hook seam (P6 registers a real implementation here). P5 paints
//! with the true no-op default: images and math degrade to alt-text/TeX
//! runs at layout time ([`layout::NullSizer`]), so no `Reserved` box is
//! ever produced in this phase's own binary — but the seam is fully wired
//! and testable independently of that fact.

use std::io::Write;

use ast::NodeId;
use layout::Reserved;

use crate::painter::CellRect;

mod sink;
mod sizer;

pub use sink::GfxMediaSink;
pub use sizer::ImageSizer;

/// P6's hook: paints media into a reserved cell region, and evicts a
/// placement when its node scrolls out of view.
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
    fn begin_frame(&mut self, _out: &mut dyn Write) {}

    fn paint(&mut self, reserved: &Reserved, rect: CellRect, out: &mut dyn Write);
    fn evict(&mut self, node_id: NodeId, out: &mut dyn Write);
}

/// The no-media default: a true no-op on every call.
#[derive(Debug, Default)]
pub struct NoopMediaSink;

impl MediaSink for NoopMediaSink {
    fn paint(&mut self, _reserved: &Reserved, _rect: CellRect, _out: &mut dyn Write) {}
    fn evict(&mut self, _node_id: NodeId, _out: &mut dyn Write) {}
}
