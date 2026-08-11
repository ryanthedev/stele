//! DW-2.10(a) fuzz target: `Listing::rows`/`first_selectable`/
//! `next_selectable`/`prev_selectable` must never panic, never return an
//! out-of-range index, and never return an index that names an
//! `EntryKind::Unopenable` row — over an arbitrary `(height, selected)` and
//! arbitrary entry names, including names that are not valid UTF-8.
//!
//! Drives `Listing::from_entries` (a pure, non-I/O constructor) rather than
//! `Listing::read`, so every iteration runs at libFuzzer's normal in-memory
//! throughput instead of paying for a real directory per input — and so a
//! non-UTF8 name is reachable at all, which a filesystem that enforces
//! UTF-8 filenames (APFS) would otherwise make impossible to fuzz on disk.
//! `Listing::read`'s I/O and classification are covered by
//! `crates/stele/tests/explore_listing.rs` instead.
//!
//! Committed minimum campaign: `-max_total_time=90 -timeout=2`, run via
//! `cargo fuzz run explore_listing corpus/explore_listing
//! seeds/explore_listing -- -max_total_time=90 -timeout=2` (the seed corpus
//! lives in `seeds/`, outside the gitignored `corpus/` working directory, so
//! it stays committed rather than merging into libFuzzer's discovered
//! inputs). The stable-toolchain twin of this target is
//! `test_dw_2_10_arbitrary_listings_never_panic_and_never_select_an_unselectable_row`
//! in `crates/stele/tests/explore_listing.rs`.

#![no_main]

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;
use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;
use stele::explore::{Entry, EntryKind, Listing};

/// The most entries built per input — the fuzzer explores byte layout, not
/// list length, so this bounds each iteration's cost without narrowing the
/// space of interest.
const MAX_FUZZ_ENTRIES: usize = 64;

fuzz_target!(|data: &[u8]| {
    // 2 bytes for height, 8 for selected — everything after that is entries.
    if data.len() < 10 {
        return;
    }
    let height = u16::from_le_bytes([data[0], data[1]]);
    let selected = u64::from_le_bytes(data[2..10].try_into().expect("exactly 8 bytes"))
        as usize;

    let mut entries = Vec::new();
    let mut cursor = 10;
    while cursor + 2 <= data.len() && entries.len() < MAX_FUZZ_ENTRIES {
        let kind = match data[cursor] % 4 {
            0 => EntryKind::Parent,
            1 => EntryKind::Directory,
            2 => EntryKind::Document,
            _ => EntryKind::Unopenable,
        };
        let name_len = usize::from(data[cursor + 1]) % 32;
        cursor += 2;
        let end = (cursor + name_len).min(data.len());
        let name = OsString::from_vec(data[cursor..end].to_vec());
        cursor = end;
        entries.push(Entry {
            path: PathBuf::from("fuzz-entry"),
            name,
            kind,
        });
    }

    let truncated = data[0].is_multiple_of(2);
    let listing = Listing::from_entries(PathBuf::from("/fuzz"), entries, truncated);

    let rows = listing.rows(height, selected);
    assert!(
        rows.len() <= usize::from(height),
        "rows exceeded the requested height"
    );

    for index in [
        listing.first_selectable(),
        listing.next_selectable(selected),
        listing.prev_selectable(selected),
    ]
    .into_iter()
    .flatten()
    {
        assert!(
            index < listing.entries().len(),
            "a movement method returned an out-of-range index"
        );
        assert_ne!(
            listing.entries()[index].kind,
            EntryKind::Unopenable,
            "a movement method returned an unselectable row"
        );
    }
});
