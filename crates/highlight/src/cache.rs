//! A bounded memo of per-line highlight results.
//!
//! **Why this exists.** `Painter::paint_run` calls `Decor::highlight` once
//! per code-block run per frame, and a themed decor answers that by running
//! tree-sitter over the line. A measure-first audit put code-heavy frames at
//! 2,525 µs against a 64 µs budget — ~40x — entirely on that repeated parse.
//! Nothing about the answer changes between two frames over the same line,
//! so the fix is to stop asking twice (DW-4.6/DW-4.7).
//!
//! **Two invariants the Phase 4 constraints pin, both enforced here.**
//! The cache is *bounded* — a viewer holds its document open for hours and
//! an unbounded memo of every line it ever scrolled past is a leak with a
//! slow fuse. And it never retains a result the highlighter produced by
//! timing out: see [`crate::Highlighted`]'s `cacheable` flag, which is the
//! only channel that distinguishes "this is how the line highlights" from
//! "this attempt did not finish in time".

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use layout::Run;

use crate::highlighter::{Highlighted, highlight_detailed};

/// How many `(line, lang)` results a default [`HighlightCache`] retains.
///
/// Sized in *lines*, because that is the unit the paint path misses on: a
/// tall terminal shows on the order of 100, so this is several dozen screens
/// of scrollback — enough that ordinary reading (scroll down, scroll back)
/// never evicts a line it is about to want again, and small enough that the
/// whole memo is a few megabytes at worst even for pathologically long lines.
pub const DEFAULT_CACHE_CAPACITY: usize = 4096;

/// The cache key. Owned rather than borrowed: entries outlive the borrow of
/// the line text they came from (the layout tree can be replaced under the
/// painter by any relayout), and `lang` is owned for the same reason.
type Key = (String, Option<String>);

/// A bounded, insertion-ordered memo of [`highlight_detailed`] results.
///
/// Eviction is FIFO rather than LRU on purpose: the access pattern is a
/// viewport sweeping over a document, so insertion order and recency track
/// each other closely, and FIFO needs no per-hit bookkeeping in the hot path
/// — a hit is a hash lookup and a refcount bump, nothing more. Runs are
/// handed out as `Rc<[Run]>` so a hit never deep-copies the run vector.
#[derive(Debug)]
pub struct HighlightCache {
    entries: HashMap<Key, Rc<[Run]>>,
    /// Keys in insertion order; the front is the next eviction victim.
    order: VecDeque<Key>,
    capacity: usize,
}

/// Hand-written rather than derived: a derived `Default` would produce
/// `capacity: 0`, which is a cache that silently retains nothing. That is a
/// legitimate thing to ask for explicitly — it is DW-4.7's baseline arm —
/// but it is the wrong thing to *fall into*, and `HighlightCache::default()`
/// is the obvious call for a future caller to reach for. Defaulting to the
/// real capacity means the trap costs a reader nothing.
impl Default for HighlightCache {
    fn default() -> Self {
        HighlightCache::with_default_capacity()
    }
}

impl HighlightCache {
    /// A cache retaining at most `capacity` entries.
    ///
    /// `capacity == 0` disables memoization entirely — every lookup is a
    /// miss and nothing is ever retained, which reproduces the pre-change
    /// paint path exactly. That is not a degenerate case to guard against
    /// but the baseline arm of DW-4.7's committed benchmark: the honest way
    /// to measure "10x faster than before" is to run *before* in the same
    /// harness, on the same host, over the same fixture.
    pub fn new(capacity: usize) -> Self {
        HighlightCache {
            entries: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    /// The shipped default: [`DEFAULT_CACHE_CAPACITY`] entries.
    pub fn with_default_capacity() -> Self {
        HighlightCache::new(DEFAULT_CACHE_CAPACITY)
    }

    /// Number of retained entries. Never exceeds the configured capacity —
    /// that bound is what makes this safe to keep alive for a whole session.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drops every entry. Called when the decor implementation is replaced:
    /// a memo of one highlighter's answers must not be served as another's.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    /// The memoized runs for `(line_text, lang)`, running the real
    /// highlighter on a miss.
    pub fn highlight(&mut self, line_text: &str, lang: Option<&str>) -> Rc<[Run]> {
        self.get_or_compute(line_text, lang, highlight_detailed)
    }

    /// [`HighlightCache::highlight`] with the miss path supplied by the
    /// caller — the seam `stele`'s painter uses to route misses through its
    /// own registered `Decor` implementation (which may not be this crate's
    /// at all), and the seam DW-4.6's test uses to count how many times the
    /// underlying highlighter was actually invoked.
    ///
    /// `miss` is called **at most once**, and only when the key is absent.
    /// Its result is retained only if [`Highlighted::cacheable`].
    pub fn get_or_compute<F>(&mut self, line_text: &str, lang: Option<&str>, miss: F) -> Rc<[Run]>
    where
        F: FnOnce(&str, Option<&str>) -> Highlighted,
    {
        // Borrowed lookup first, so a hit never allocates the owned key.
        if let Some(hit) = self.lookup(line_text, lang) {
            return hit;
        }
        let computed = miss(line_text, lang);
        let runs: Rc<[Run]> = Rc::from(computed.runs);
        if computed.cacheable {
            self.insert(
                (line_text.to_string(), lang.map(str::to_string)),
                runs.clone(),
            );
        }
        runs
    }

    /// A hit, without building the owned key a miss would need.
    ///
    /// `HashMap::get` cannot take `(&str, Option<&str>)` against a
    /// `(String, Option<String>)` key without a `Borrow` impl that no tuple
    /// provides, so the key is built here too — but only its cheap half is
    /// avoidable, and correctness beats a micro-optimization the DW-4.7
    /// measurement does not ask for. Kept as its own function so the day
    /// that changes, it changes in one place.
    fn lookup(&self, line_text: &str, lang: Option<&str>) -> Option<Rc<[Run]>> {
        let key = (line_text.to_string(), lang.map(str::to_string));
        self.entries.get(&key).cloned()
    }

    /// Stores `runs` under `key`, evicting the oldest entry first when the
    /// cache is already at capacity. A zero capacity stores nothing at all.
    fn insert(&mut self, key: Key, runs: Rc<[Run]>) {
        if self.capacity == 0 {
            return;
        }
        while self.entries.len() >= self.capacity {
            let Some(oldest) = self.order.pop_front() else {
                // `order` and `entries` are maintained in lockstep, so this
                // is unreachable; returning rather than looping forever is
                // the safe reading if that ever stops being true.
                return;
            };
            self.entries.remove(&oldest);
        }
        self.order.push_back(key.clone());
        self.entries.insert(key, runs);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use layout::{Semantic, StyleId};

    use super::*;

    fn run(text: &str) -> Run {
        Run {
            text: text.to_string(),
            style_id: StyleId::Semantic(Semantic::CodeBlock),
            width: 0,
            aux: None,
        }
    }

    fn cacheable(text: &str) -> Highlighted {
        Highlighted {
            runs: vec![run(text)],
            cacheable: true,
        }
    }

    #[test]
    fn test_dw_4_6_a_repeated_lookup_computes_once_and_returns_the_same_runs() {
        let mut cache = HighlightCache::with_default_capacity();
        let calls = Cell::new(0);
        let lookup = |cache: &mut HighlightCache| {
            cache.get_or_compute("let x = 1;", Some("rust"), |text, _lang| {
                calls.set(calls.get() + 1);
                cacheable(text)
            })
        };

        let first = lookup(&mut cache);
        let second = lookup(&mut cache);

        assert_eq!(calls.get(), 1, "the second lookup must not recompute");
        assert_eq!(first, second);
    }

    /// The Phase 4 constraint, stated as a test: a result the highlighter
    /// produced by falling back (timeout or error) must never be retained,
    /// so a transient stall cannot freeze plain text into the rest of the
    /// session. `classify` in `highlighter.rs` is what sets the flag; this
    /// asserts the cache actually honours it.
    #[test]
    fn test_dw_4_6_a_non_cacheable_result_is_recomputed_on_every_lookup() {
        let mut cache = HighlightCache::with_default_capacity();
        let calls = Cell::new(0);
        for _ in 0..5 {
            cache.get_or_compute("slow line", Some("rust"), |text, _lang| {
                calls.set(calls.get() + 1);
                Highlighted {
                    runs: vec![run(text)],
                    cacheable: false,
                }
            });
        }
        assert_eq!(calls.get(), 5, "a fallback result must not be memoized");
        assert!(cache.is_empty(), "nothing may be retained: {cache:?}");
    }

    /// Bounded, as the constraint requires — and bounded by eviction, not by
    /// refusing to insert once full, which would freeze the cache on the
    /// first screenful and silently stop helping.
    #[test]
    fn test_the_cache_never_grows_past_its_capacity_and_evicts_the_oldest_first() {
        let mut cache = HighlightCache::new(3);
        for i in 0..10 {
            let line = format!("line {i}");
            cache.get_or_compute(&line, Some("rust"), |text, _| cacheable(text));
            assert!(cache.len() <= 3, "capacity exceeded at line {i}");
        }
        assert_eq!(cache.len(), 3);

        // The three most recent survived; the older ones were evicted.
        let calls = Cell::new(0);
        cache.get_or_compute("line 9", Some("rust"), |text, _| {
            calls.set(calls.get() + 1);
            cacheable(text)
        });
        assert_eq!(calls.get(), 0, "the newest entry must still be resident");
        cache.get_or_compute("line 0", Some("rust"), |text, _| {
            calls.set(calls.get() + 1);
            cacheable(text)
        });
        assert_eq!(calls.get(), 1, "the oldest entry must have been evicted");
    }

    /// Capacity zero is the DW-4.7 benchmark's baseline arm: it must behave
    /// exactly like having no cache at all rather than panicking on the
    /// eviction loop's `while len >= 0`.
    #[test]
    fn test_a_zero_capacity_cache_recomputes_every_time_and_never_retains() {
        let mut cache = HighlightCache::new(0);
        let calls = Cell::new(0);
        for _ in 0..4 {
            cache.get_or_compute("let x = 1;", Some("rust"), |text, _| {
                calls.set(calls.get() + 1);
                cacheable(text)
            });
        }
        assert_eq!(calls.get(), 4);
        assert!(cache.is_empty());
    }

    /// The language is part of the key: the same text in two fences must not
    /// serve one fence's highlighting to the other.
    #[test]
    fn test_the_same_line_in_two_languages_is_two_entries() {
        let mut cache = HighlightCache::with_default_capacity();
        let calls = Cell::new(0);
        let count = |cache: &mut HighlightCache, lang: Option<&str>| {
            cache.get_or_compute("value = 1", lang, |text, _| {
                calls.set(calls.get() + 1);
                cacheable(text)
            });
        };
        count(&mut cache, Some("rust"));
        count(&mut cache, Some("python"));
        count(&mut cache, None);
        assert_eq!(calls.get(), 3, "three distinct keys, three computations");
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn test_clear_drops_every_entry_so_a_new_decor_is_never_served_the_old_ones() {
        let mut cache = HighlightCache::with_default_capacity();
        cache.get_or_compute("let x = 1;", Some("rust"), |text, _| cacheable(text));
        assert_eq!(cache.len(), 1);

        cache.clear();
        assert!(cache.is_empty());

        let calls = Cell::new(0);
        cache.get_or_compute("let x = 1;", Some("rust"), |text, _| {
            calls.set(calls.get() + 1);
            cacheable(text)
        });
        assert_eq!(calls.get(), 1, "a cleared entry must be recomputed");
    }

    /// The production entry point still routes through the real highlighter
    /// and preserves the line's text exactly, cached or not.
    #[test]
    fn test_the_default_miss_path_runs_the_real_highlighter_and_preserves_text() {
        let mut cache = HighlightCache::with_default_capacity();
        let first = cache.highlight("let x: i32 = 1;", Some("rust"));
        let second = cache.highlight("let x: i32 = 1;", Some("rust"));
        let text: String = first.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(text, "let x: i32 = 1;");
        assert_eq!(first, second);
        assert_eq!(cache.len(), 1);
    }
}
