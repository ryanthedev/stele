//! `RasterCache` — which decoded rasters the terminal is still holding, and
//! how many bytes of them.
//!
//! **This is residency, and residency only.** Whether an image is *drawn* is
//! [`super::sink::GfxMediaSink`]'s `placed` list and nothing in this file can
//! see it; whether an image's pixel data is still in the terminal is this
//! file and nothing in the sink decides it. The two used to be one
//! `HashMap<NodeId, Placement>`, and the cost of that was not theoretical:
//! the kitty placement cap (32) was checked against `placements.len()`, so
//! the *raster* cache was silently capped at 32 entries too, and a document
//! with more images than that could not keep a raster no matter how small it
//! was. Splitting the map is what makes it impossible to re-derive one bound
//! from the other by accident — after the split there is no expression
//! anywhere comparing a placement cap to a count of rasters.
//!
//! **The budget is in bytes, not frames.** The mechanism this replaced freed
//! a raster once its node had gone unpainted for a frame, as an approximation
//! of a one-viewport scroll margin the [`super::MediaSink`] trait cannot
//! observe. The arithmetic was off by one in the direction that made the
//! window always empty — a node absent from frame *N-1* lost its raster at
//! the top of frame *N*, i.e. before the frame that wanted it could ask — so
//! a scroll-back always re-transmitted. Bytes are the honest unit anyway: what
//! is actually scarce is the memory the terminal spends holding rasters, and
//! that is measurable here, unlike scroll distance.
//!
//! **Pinning.** Eviction is told which nodes have a live placement this frame
//! and never takes one of those: freeing the pixel data under a placement
//! blanks a box the reader is looking at. When *every* resident is pinned the
//! cache goes over budget rather than blank anything — bounded, because at
//! most `CAP` boxes can be placed in a frame.

use std::collections::{HashMap, HashSet};

use ast::NodeId;

/// One transmitted raster the terminal is holding.
pub(crate) struct Resident {
    pub id: gfx::ImageId,
    /// The `(width_px, height_px)` this raster was rendered *for*. A repaint
    /// at the same target reuses it; a different target (a resize, a cell-size
    /// change) is a miss and re-renders.
    pub rendered_px: (u32, u32),
    /// The raster's *actual* `(width_px, height_px)`, read back from the
    /// transmitted PNG rather than assumed to equal `rendered_px` — source
    /// rectangle cropping is denominated in raster pixels, so this is the
    /// unit that has to be observed rather than inferred.
    pub raster_px: (u32, u32),
    /// The transmitted PNG's byte length: what the budget is spent in.
    bytes: u64,
}

/// The resident set, ordered least-recently-used first and bounded by a byte
/// budget.
pub(crate) struct RasterCache {
    resident: HashMap<NodeId, Resident>,
    /// LRU order: front evicts first, back is most recently used. A `Vec`
    /// rather than a linked structure — it is bounded by the budget divided
    /// by a raster's size, in practice tens of entries, and a `Vec` gives the
    /// deterministic iteration order the emitted byte stream is asserted on.
    order: Vec<NodeId>,
    bytes: u64,
    budget: u64,
}

impl RasterCache {
    pub(crate) fn new(budget: u64) -> Self {
        RasterCache {
            resident: HashMap::new(),
            order: Vec::new(),
            bytes: 0,
            budget,
        }
    }

    /// The resident raster for `node` if it was rendered for `target`,
    /// marking it most-recently-used.
    ///
    /// A record rendered for a *different* target is deliberately left alone
    /// rather than dropped: the caller is about to replace it, and dropping
    /// it here would emit a delete the replacing transmit makes redundant.
    pub(crate) fn get(&mut self, node: NodeId, target: (u32, u32)) -> Option<&Resident> {
        if self.resident.get(&node)?.rendered_px != target {
            return None;
        }
        self.touch(node);
        self.resident.get(&node)
    }

    /// The resident record for `node`, whatever it was rendered for and
    /// without disturbing LRU order — for the two questions that are not
    /// cache lookups: which image id to place, and which ids are in use.
    pub(crate) fn peek(&self, node: NodeId) -> Option<&Resident> {
        self.resident.get(&node)
    }

    pub(crate) fn holds_id(&self, id: gfx::ImageId) -> bool {
        self.resident.values().any(|r| r.id == id)
    }

    pub(crate) fn bytes_resident(&self) -> u64 {
        self.bytes
    }

    pub(crate) fn len(&self) -> usize {
        self.resident.len()
    }

    /// Every node with a resident raster, least-recently-used first. Ordered
    /// rather than hash order, so a caller that emits one escape per node
    /// produces a deterministic byte stream.
    pub(crate) fn resident_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.order.iter().copied()
    }

    /// Records `node`'s newly transmitted raster and returns the image ids
    /// evicted to stay inside the budget, least-recently-used first.
    ///
    /// `pinned` are the nodes with a live placement in the current frame;
    /// none of them is ever a victim, nor is `node` itself. Returning the ids
    /// rather than emitting the deletes keeps the wire out of this module —
    /// the caller owns the emitter, and this owns the accounting.
    pub(crate) fn insert(
        &mut self,
        node: NodeId,
        record: Resident,
        pinned: &HashSet<NodeId>,
    ) -> Vec<gfx::ImageId> {
        if let Some(previous) = self.resident.remove(&node) {
            self.bytes -= previous.bytes;
        }
        self.bytes += record.bytes;
        self.resident.insert(node, record);
        self.touch(node);

        let mut freed = Vec::new();
        while self.bytes > self.budget {
            let Some(victim) = self
                .order
                .iter()
                .copied()
                .find(|id| *id != node && !pinned.contains(id))
            else {
                // Every resident is on the screen right now. Going over
                // budget beats blanking a box the reader is looking at, and
                // the overshoot is bounded: only a placed box can pin, and at
                // most `CAP` boxes are placed per frame.
                break;
            };
            if let Some(id) = self.remove(victim) {
                freed.push(id);
            }
        }
        freed
    }

    /// Forgets `node` and returns the image id whose raster the caller must
    /// now free on the wire.
    pub(crate) fn remove(&mut self, node: NodeId) -> Option<gfx::ImageId> {
        let record = self.resident.remove(&node)?;
        self.bytes -= record.bytes;
        self.order.retain(|&id| id != node);
        Some(record.id)
    }

    fn touch(&mut self, node: NodeId) {
        self.order.retain(|&id| id != node);
        self.order.push(node);
    }
}

impl Resident {
    pub(crate) fn new(
        id: gfx::ImageId,
        rendered_px: (u32, u32),
        raster_px: (u32, u32),
        bytes: u64,
    ) -> Self {
        Resident {
            id,
            rendered_px,
            raster_px,
            bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `NodeId` has no public constructor — by design, since only a parsed
    /// document may mint one. Fifty one-word paragraphs give this module the
    /// distinct, real ids it compares.
    fn nodes() -> Vec<NodeId> {
        let doc: ast::Document =
            ast::Document::parse(&(0..50).map(|i| format!("p{i}\n\n")).collect::<String>());
        doc.blocks().iter().map(|b| b.id).collect()
    }

    fn id(n: u32) -> gfx::ImageId {
        gfx::ImageId::new(n)
    }

    fn record(n: u32, bytes: u64) -> Resident {
        Resident::new(id(n), (10, 10), (10, 10), bytes)
    }

    /// The property DW-3.6 is about, at the level the accounting lives: how
    /// many rasters fit is a function of their size and the budget, and of
    /// nothing else. Forty entries survive here, well past the 32-placement
    /// cap that used to bound the same map.
    #[test]
    fn test_a_budget_that_fits_forty_rasters_keeps_forty_rasters() {
        let node = nodes();
        let mut cache = RasterCache::new(40 * 100);
        let pinned = HashSet::new();
        for n in 0..40u32 {
            assert!(
                cache
                    .insert(node[n as usize], record(n + 1, 100), &pinned)
                    .is_empty(),
                "nothing may be evicted while the budget still fits {n}"
            );
        }
        assert_eq!(cache.len(), 40);
        assert_eq!(cache.bytes_resident(), 4_000);
    }

    #[test]
    fn test_the_least_recently_used_raster_is_the_one_the_budget_frees() {
        let node = nodes();
        let mut cache = RasterCache::new(250);
        let pinned = HashSet::new();
        cache.insert(node[0], record(1, 100), &pinned);
        cache.insert(node[1], record(2, 100), &pinned);
        // Touching 0 makes 1 the least recently used.
        assert!(cache.get(node[0], (10, 10)).is_some());

        let freed = cache.insert(node[2], record(3, 100), &pinned);
        assert_eq!(freed, vec![id(2)], "node 1 was the least recently used");
        assert!(cache.peek(node[1]).is_none());
        assert!(cache.peek(node[0]).is_some());
        assert_eq!(cache.bytes_resident(), 200);
    }

    /// The invariant that keeps an eviction from blanking the screen: a node
    /// with a live placement is never a victim, even when that means the
    /// budget is exceeded.
    #[test]
    fn test_a_pinned_raster_is_never_evicted_even_over_budget() {
        let node = nodes();
        let mut cache = RasterCache::new(150);
        let mut pinned = HashSet::new();
        pinned.insert(node[0]);
        pinned.insert(node[1]);
        cache.insert(node[0], record(1, 100), &pinned);
        let freed = cache.insert(node[1], record(2, 100), &pinned);
        assert!(freed.is_empty(), "a placed box must not lose its raster");
        assert_eq!(cache.bytes_resident(), 200, "over budget, deliberately");
        assert_eq!(cache.len(), 2);
    }

    /// Re-transmitting the same node at a new size replaces its bytes rather
    /// than adding to them — the accounting bug that would slowly starve the
    /// budget across a session of resizes.
    #[test]
    fn test_re_inserting_a_node_replaces_its_bytes_instead_of_adding_them() {
        let node = nodes();
        let mut cache = RasterCache::new(10_000);
        let pinned = HashSet::new();
        cache.insert(node[0], record(1, 100), &pinned);
        cache.insert(node[0], record(1, 700), &pinned);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.bytes_resident(), 700);
    }

    #[test]
    fn test_a_cache_hit_requires_the_same_render_target() {
        let node = nodes();
        let mut cache = RasterCache::new(10_000);
        cache.insert(node[0], record(1, 100), &HashSet::new());
        assert!(cache.get(node[0], (10, 10)).is_some());
        assert!(
            cache.get(node[0], (20, 10)).is_none(),
            "a raster rendered for another target is not a hit"
        );
        assert!(
            cache.peek(node[0]).is_some(),
            "...and the miss must not drop it: the caller is about to replace it"
        );
    }

    #[test]
    fn test_removing_a_node_returns_its_id_and_refunds_its_bytes() {
        let node = nodes();
        let mut cache = RasterCache::new(10_000);
        cache.insert(node[0], record(7, 100), &HashSet::new());
        assert_eq!(cache.remove(node[0]), Some(id(7)));
        assert_eq!(cache.bytes_resident(), 0);
        assert_eq!(cache.remove(node[0]), None);
    }
}
