//! The media hook seam (P6 registers a real implementation here). P5 paints
//! with the true no-op default: images and math degrade to alt-text/TeX
//! runs at layout time ([`layout::NullSizer`]), so no `Reserved` box is
//! ever produced in this phase's own binary — but the seam is fully wired
//! and testable independently of that fact.

use std::io::Write;

use ast::NodeId;
use layout::Reserved;

use crate::painter::CellRect;

/// P6's hook: paints media into a reserved cell region, and evicts a
/// placement when its node scrolls out of view.
pub trait MediaSink {
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
