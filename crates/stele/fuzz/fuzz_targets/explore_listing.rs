//! DW-2.10(a) fuzz target: `Listing::rows`/`first_selectable`/
//! `next_selectable`/`prev_selectable` must never panic, never return an
//! out-of-range index, and never return an index that names an
//! `EntryKind::Unopenable` row — over an arbitrary `(height, selected)` and
//! arbitrary entry names, including names that are not valid UTF-8.
//!
//! Plus, added after a batch review: **a valid selection must actually
//! appear in the returned window.** The classic off-by-one victim was
//! checked only against one hand-built fixture, and this target could not
//! have caught it anyway — it hardcoded `error: None`, so the notice-row
//! branch, which holds the only subtraction in the windowing math, was never
//! reached. Both the notice states and the cursor invariant are covered
//! below.
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
use stele::explore::{Entry, EntryKind, Listing, RowStyle};

/// The most entries built per input — the fuzzer explores byte layout, not
/// list length, so this bounds each iteration's cost without narrowing the
/// space of interest.
const MAX_FUZZ_ENTRIES: usize = 64;

/// The text `Listing::rows` must paint for `entry`. `row_text` is private;
/// mirroring it here is what turns "some row is selected" into "the *right*
/// row is selected", which is the only form of the assertion an off-by-one
/// cannot pass.
fn expected_row_text(entry: &Entry) -> String {
    match entry.kind {
        EntryKind::Parent => "../".to_string(),
        EntryKind::Directory => format!("{}/", entry.name.to_string_lossy()),
        EntryKind::Document | EntryKind::Unopenable => entry.name.to_string_lossy().into_owned(),
    }
}

fuzz_target!(|data: &[u8]| {
    // 2 bytes for height, 8 for selected, 1 to pick the notice state —
    // everything after that is entries.
    if data.len() < 11 {
        return;
    }
    let height = u16::from_le_bytes([data[0], data[1]]);
    let selected = u64::from_le_bytes(data[2..10].try_into().expect("exactly 8 bytes"))
        as usize;
    // 0 => the directory itself was unreadable, 1 => some entries were
    // dropped, anything else => neither. Both of the first two produce the
    // one notice row that offsets every entry row.
    let (error, dropped) = match data[10] % 4 {
        0 => (
            Some(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            0,
        ),
        1 => (None, 1 + usize::from(data[10]) * 7),
        _ => (None, 0),
    };

    let mut entries = Vec::new();
    let mut cursor = 11;
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
        // Index-prefixed so no two entries can share a name: the cursor
        // assertion below identifies a row by its text, and the bytes after
        // the prefix are still arbitrary, non-UTF8 included.
        let mut name_bytes = format!("{}-", entries.len()).into_bytes();
        name_bytes.extend_from_slice(&data[cursor..end]);
        let name = OsString::from_vec(name_bytes);
        cursor = end;
        entries.push(Entry {
            path: PathBuf::from("fuzz-entry"),
            name,
            kind,
        });
    }

    let truncated = data[0].is_multiple_of(2);
    let notice_rows = usize::from(error.is_some() || dropped > 0);
    let listing = Listing::from_parts(PathBuf::from("/fuzz"), entries, truncated, error, dropped);

    let rows = listing.rows(height, selected);
    assert!(
        rows.len() <= usize::from(height),
        "rows exceeded the requested height"
    );

    // Whenever an entry row was painted at all, exactly one carries the
    // cursor, and it is the row for the (clamped) selected entry.
    if rows.len() > notice_rows {
        let picked: Vec<&stele::explore::OverlayRow> = rows
            .iter()
            .filter(|row| row.style == RowStyle::Selected)
            .collect();
        assert_eq!(picked.len(), 1, "exactly one row must carry the cursor");
        let clamped = selected.min(listing.entries().len() - 1);
        assert_eq!(
            picked[0].text,
            expected_row_text(&listing.entries()[clamped]),
            "the cursor landed on the wrong row"
        );
    }
    if notice_rows == 1 && !rows.is_empty() {
        assert_eq!(
            rows[0].style,
            RowStyle::Ordinary,
            "the notice row leads and is never styled as an entry"
        );
    }

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
