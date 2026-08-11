//! DW-2.1 through DW-2.10 at the level a caller outside the crate uses
//! [`explore::Listing`]: real fixture directories for classification and
//! I/O edge cases, and `Listing::from_entries` — a pure, non-I/O
//! constructor built from already-public types — for the selection and
//! windowing invariants that must hold over listings no real filesystem
//! needs to produce.
//!
//! Fixtures are hand-rolled from `std::env::temp_dir()` and `process::id()`,
//! the idiom at `link_interaction.rs:47-68` and `explorer_open.rs:51-57` —
//! there is no `tempfile` in this workspace.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use stele::explore::{Entry, EntryKind, Listing, OverlayRow, RowStyle};
use stele::{Painter, Size};
use width::{WidthConfig, WidthEngine};

/// A scratch directory unique to `tag`.
fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("stele-explore-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

fn entry(name: &str, kind: EntryKind) -> Entry {
    Entry {
        name: OsString::from(name),
        path: PathBuf::from(name),
        kind,
    }
}

// -------------------------------------------------------------------- DW-2.1

/// A directory holding one of every kind: a subdirectory, a markdown file,
/// a non-markdown regular file, and a socket — against a real directory, as
/// the DW item requires, not a mock.
#[test]
#[cfg(unix)]
fn test_dw_2_1_a_mixed_directory_classifies_every_entry_into_exactly_one_kind() {
    use std::os::unix::net::UnixListener;

    let dir = scratch_dir("mixed");
    std::fs::create_dir(dir.join("sub")).expect("create subdirectory");
    write(&dir, "readme.md", "# hi\n");
    write(&dir, "notes.txt", "plain text\n");
    let sock_path = dir.join("sock");
    let _listener = UnixListener::bind(&sock_path).expect("bind unix socket");

    let listing = Listing::read(&dir);
    assert!(
        listing.error().is_none(),
        "a readable directory has no error"
    );

    let kind_of = |name: &str| {
        listing
            .entries()
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("{name} must be listed: {:?}", listing.entries()))
            .kind
    };
    assert_eq!(kind_of("sub"), EntryKind::Directory);
    assert_eq!(kind_of("readme.md"), EntryKind::Document);
    assert_eq!(kind_of("notes.txt"), EntryKind::Unopenable);
    assert_eq!(kind_of("sock"), EntryKind::Unopenable);
    assert_eq!(
        listing.entries()[0].kind,
        EntryKind::Parent,
        "the parent row leads, since this directory has one"
    );
    assert_eq!(
        listing.entries().len(),
        5,
        "parent + 4 real entries, no more and no fewer: {:?}",
        listing.entries()
    );
    assert!(
        !listing.truncated(),
        "four entries is nowhere near the cap; `truncated()` must answer false"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Past the DW floor: an entry that vanished between `read_dir` naming it
/// and `symlink_metadata` stat-ing it must not panic or otherwise wedge the
/// read — it is silently absent from the listing instead. A genuine race is
/// not reproducible on demand, so this races a background thread against
/// many reads of the same directory, which reliably wins the window often
/// enough across iterations to exercise the code path while asserting only
/// what is actually guaranteed: `Listing::read` never panics regardless of
/// which side of the race it lands on.
#[test]
fn test_an_entry_deleted_between_read_dir_and_symlink_metadata_never_panics() {
    let dir = scratch_dir("vanishing");
    for i in 0..20 {
        write(&dir, &format!("stable{i}.md"), "# stable\n");
    }
    let racer_path = dir.join("racer.md");

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let racer_dir = dir.clone();
    let racer_stop = stop.clone();
    let racer = std::thread::spawn(move || {
        while !racer_stop.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = std::fs::write(&racer_path, "# racer\n");
            let _ = std::fs::remove_file(&racer_path);
        }
        let _ = racer_dir;
    });

    for _ in 0..500 {
        let listing = Listing::read(&dir);
        assert!(
            listing.entries().len() <= 22,
            "never more than parent + stable + racer: {:?}",
            listing.entries()
        );
    }

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    racer.join().expect("racer thread must not panic either");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Past the DW floor: an empty directory lists only its parent row.
#[test]
fn test_an_empty_directory_lists_only_the_parent_row() {
    let dir = scratch_dir("empty");
    let listing = Listing::read(&dir);
    assert_eq!(listing.entries().len(), 1);
    assert_eq!(listing.entries()[0].kind, EntryKind::Parent);
    let _ = std::fs::remove_dir_all(&dir);
}

// -------------------------------------------------------------------- DW-2.2

/// Unopenable rows appear first, last, and in consecutive runs (by name
/// sort: `0-`/`1-` lead, `3-`/`4-` run together in the middle, `6-` trails).
/// Movement in both directions must skip every one and never land on one.
#[test]
fn test_dw_2_2_selection_movement_skips_every_unopenable_row_in_both_directions() {
    let dir = scratch_dir("skip-unopenable");
    write(&dir, "0-bad.bin", "x");
    write(&dir, "1-bad.bin", "x");
    write(&dir, "2-doc.md", "# 2\n");
    write(&dir, "3-bad.bin", "x");
    write(&dir, "4-bad.bin", "x");
    write(&dir, "5-doc.md", "# 5\n");
    write(&dir, "6-bad.bin", "x");

    let listing = Listing::read(&dir);
    let names: Vec<String> = listing
        .entries()
        .iter()
        .map(|e| e.name.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec![
            "..",
            "0-bad.bin",
            "1-bad.bin",
            "2-doc.md",
            "3-bad.bin",
            "4-bad.bin",
            "5-doc.md",
            "6-bad.bin"
        ],
        "sorted order must put the runs where this test expects them"
    );

    // index: 0=Parent 1=bad 2=bad 3=doc 4=bad 5=bad 6=doc 7=bad
    assert_eq!(listing.first_selectable(), Some(0));
    assert_eq!(listing.next_selectable(0), Some(3), "skips the leading run");
    assert_eq!(listing.next_selectable(3), Some(6), "skips the middle run");
    assert_eq!(
        listing.next_selectable(6),
        None,
        "the trailing run has nothing after it"
    );

    assert_eq!(listing.prev_selectable(7), Some(6));
    assert_eq!(listing.prev_selectable(6), Some(3));
    assert_eq!(listing.prev_selectable(3), Some(0));
    assert_eq!(listing.prev_selectable(0), None);

    for i in 0..listing.entries().len() {
        if let Some(next) = listing.next_selectable(i) {
            assert_ne!(
                listing.entries()[next].kind,
                EntryKind::Unopenable,
                "next_selectable must never return an unopenable index"
            );
        }
        if let Some(prev) = listing.prev_selectable(i) {
            assert_ne!(
                listing.entries()[prev].kind,
                EntryKind::Unopenable,
                "prev_selectable must never return an unopenable index"
            );
        }
    }

    let _ = std::fs::remove_dir_all(&dir);
}

// -------------------------------------------------------------------- DW-2.3

/// A directory with only unopenable children still has its parent row, and
/// the parent row is where selection lands.
#[test]
fn test_dw_2_3_a_directory_with_nothing_selectable_leaves_the_parent_row_selected() {
    let dir = scratch_dir("nothing-selectable");
    write(&dir, "a.bin", "x");
    write(&dir, "b.bin", "x");

    let listing = Listing::read(&dir);
    assert_eq!(listing.entries()[0].kind, EntryKind::Parent);
    assert_eq!(
        listing.first_selectable(),
        Some(0),
        "the parent row is the landing spot"
    );
    assert_eq!(
        listing.next_selectable(0),
        None,
        "nothing else is reachable"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The filesystem-root shape: no parent row, and — simulated here since a
/// live `/` cannot be made to have "nothing selectable" — nothing else
/// selectable either. `first_selectable` is `None` and movement is a no-op
/// (`next_selectable`/`prev_selectable` both answer `None` rather than
/// wrapping onto the one unopenable row or panicking on an empty entries
/// list).
#[test]
fn test_dw_2_3_a_root_like_directory_with_no_parent_and_nothing_selectable_has_no_selectable_index()
{
    let with_unopenables = Listing::from_entries(
        PathBuf::from("/"),
        vec![
            entry("dev", EntryKind::Unopenable),
            entry("proc", EntryKind::Unopenable),
        ],
        false,
    );
    assert_eq!(with_unopenables.first_selectable(), None);
    assert_eq!(with_unopenables.next_selectable(0), None);
    assert_eq!(with_unopenables.prev_selectable(1), None);

    let truly_empty = Listing::from_entries(PathBuf::from("/"), Vec::new(), false);
    assert_eq!(truly_empty.first_selectable(), None);
    assert_eq!(truly_empty.next_selectable(0), None);
    assert_eq!(truly_empty.prev_selectable(0), None);
}

/// Past the DW floor, but the real-world half of the root shape: an actual
/// `/` read has no parent row, whatever else it contains.
#[test]
#[cfg(unix)]
fn test_reading_the_real_filesystem_root_has_no_parent_row() {
    let listing = Listing::read(Path::new("/"));
    assert!(
        listing
            .entries()
            .iter()
            .all(|e| e.kind != EntryKind::Parent),
        "the root has no parent to list: {:?}",
        listing.entries()
    );
}

// -------------------------------------------------------------------- DW-2.4

/// An unreadable directory (EACCES) reports the failure through `rows`,
/// verified by calling the function — never by expecting a panic. Skips
/// under root, where `chmod 000` does not actually restrict access, rather
/// than passing vacuously.
#[test]
#[cfg(unix)]
fn test_dw_2_4_an_unreadable_directory_reports_the_failure_in_its_rows() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = scratch_dir("unreadable");
    write(&dir, "secret.md", "# secret\n");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).expect("chmod 000");

    if std::fs::read_dir(&dir).is_ok() {
        // Running as root defeats the fixture; skip rather than assert a
        // falsehood about the code.
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        let _ = std::fs::remove_dir_all(&dir);
        eprintln!("skipping: running as root, chmod 000 does not restrict this process");
        return;
    }

    let listing = Listing::read(&dir);
    assert!(listing.error().is_some(), "an EACCES read must be reported");

    let rows = listing.rows(10, 0);
    assert!(
        !rows.is_empty() && rows[0].text.contains("cannot read directory"),
        "rows must name the failure: {rows:?}"
    );

    // The error row alone already spends the whole budget at `height == 1`:
    // this directory's listing carries the parent row too (`entries().len()
    // == 1`), so a budget computed as `height + out.len()` instead of
    // `height - out.len()` would wrongly find room to paint it as a second
    // row here.
    assert_eq!(
        listing.entries().len(),
        1,
        "only the parent row survives an unreadable dir"
    );
    assert_eq!(
        listing.rows(1, 0).len(),
        1,
        "a height of exactly 1 must show only the error row, not also the parent"
    );

    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    let _ = std::fs::remove_dir_all(&dir);
}

// -------------------------------------------------------------------- DW-2.5

/// Reading the same directory twice, including two names differing only by
/// case, produces the same order both times.
#[test]
fn test_dw_2_5_reading_the_same_directory_twice_produces_the_same_order() {
    let dir = scratch_dir("stable-order");
    for name in [
        "Banana.md",
        "apple.md",
        "banana.md",
        "Cherry.bin",
        "0-first.md",
    ] {
        write(&dir, name, "content\n");
    }
    std::fs::create_dir(dir.join("Zdir")).expect("create dir");

    let first = Listing::read(&dir);
    let second = Listing::read(&dir);
    assert_eq!(
        first.entries(),
        second.entries(),
        "two reads of an unchanged directory must agree exactly"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// -------------------------------------------------------------------- DW-2.6

/// A listing longer than the viewport windows around the selection exactly
/// as `AppState::toc_rows` does: walking one row at a time keeps the
/// selection visible in a fixed-size window at every step, including both
/// ends of the list.
#[test]
fn test_dw_2_6_a_listing_longer_than_the_viewport_windows_around_the_selection() {
    let entries: Vec<Entry> = (0..30)
        .map(|i| entry(&format!("doc-{i:02}.md"), EntryKind::Document))
        .collect();
    let listing = Listing::from_entries(PathBuf::from("/many"), entries, false);

    for selected in 0..30usize {
        let rows = listing.rows(5, selected);
        assert_eq!(
            rows.len(),
            5,
            "selected {selected}: window must fill the screen"
        );
        let selected_rows: Vec<&OverlayRow> = rows
            .iter()
            .filter(|r| r.style == RowStyle::Selected)
            .collect();
        assert_eq!(
            selected_rows.len(),
            1,
            "selected {selected}: exactly one row highlighted"
        );
        assert!(
            selected_rows[0]
                .text
                .contains(&format!("doc-{selected:02}.md")),
            "selected {selected}: the highlighted row must be the selected entry: {:?}",
            selected_rows[0].text
        );
    }

    // Both ends explicitly.
    let first_rows = listing.rows(5, 0);
    assert!(first_rows[0].style == RowStyle::Selected);
    let last_rows = listing.rows(5, 29);
    assert!(last_rows.last().unwrap().style == RowStyle::Selected);
}

/// Past the DW floor: a viewport of exactly one row still shows exactly the
/// selected row, and a viewport of zero rows shows nothing rather than
/// panicking on the window arithmetic.
#[test]
fn test_a_one_row_viewport_shows_only_the_selection_and_a_zero_row_viewport_shows_nothing() {
    let entries: Vec<Entry> = (0..10)
        .map(|i| entry(&format!("doc-{i}.md"), EntryKind::Document))
        .collect();
    let listing = Listing::from_entries(PathBuf::from("/ten"), entries, false);

    assert!(listing.rows(0, 3).is_empty());
    let rows = listing.rows(1, 3);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].style == RowStyle::Selected);
    assert!(rows[0].text.contains("doc-3.md"));
}

/// Past the DW floor, and the direct counterpart to DW-2.9's painter-level
/// byte test: `Listing::rows` itself — not a hand-built `OverlayRow` list —
/// must classify every row correctly. The exact index carrying `Selected`
/// is pinned, and so is every other row: `Unopenable` rows dim, everything
/// else stays `Ordinary`. A window with the whole listing in view (`height`
/// covers all six entries) makes every index checkable in one call.
#[test]
fn test_rows_marks_exactly_the_selected_index_dims_only_unopenable_rows_and_leaves_the_rest_ordinary()
 {
    let entries = vec![
        entry("a-dir", EntryKind::Directory),
        entry("b-bad", EntryKind::Unopenable),
        entry("c-doc", EntryKind::Document),
        entry("d-bad", EntryKind::Unopenable),
        entry("e-doc", EntryKind::Document),
        entry("f-bad", EntryKind::Unopenable),
    ];
    let listing = Listing::from_entries(PathBuf::from("/styles"), entries, false);

    // Select index 2 (an openable `Document` row) and check every row.
    let rows = listing.rows(6, 2);
    assert_eq!(rows.len(), 6);
    let expected = [
        RowStyle::Ordinary, // a-dir: openable, not selected
        RowStyle::Dimmed,   // b-bad: unopenable, not selected
        RowStyle::Selected, // c-doc: selected
        RowStyle::Dimmed,   // d-bad: unopenable, not selected
        RowStyle::Ordinary, // e-doc: openable, not selected
        RowStyle::Dimmed,   // f-bad: unopenable, not selected
    ];
    for (i, (row, want)) in rows.iter().zip(expected.iter()).enumerate() {
        assert_eq!(row.style, *want, "row {i} ({:?}): wrong style", row.text);
    }

    // Selecting an `Unopenable` row itself must still highlight it as
    // `Selected`, not `Dimmed` — painting honors the caller's selection even
    // when it points at a row the movement methods would never choose.
    let rows = listing.rows(6, 1);
    assert_eq!(rows[1].style, RowStyle::Selected);
}

// -------------------------------------------------------------------- DW-2.7

/// A broken symlink, a symlink loop, and (where the filesystem allows it) a
/// non-UTF8 name all list without panicking, and none of the three is ever
/// classified `Document`.
#[test]
#[cfg(unix)]
fn test_dw_2_7_a_broken_symlink_a_symlink_loop_and_a_non_utf8_name_list_without_panicking() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;

    let dir = scratch_dir("symlinks-and-bytes");
    std::os::unix::fs::symlink(dir.join("does-not-exist.md"), dir.join("broken.md"))
        .expect("create broken symlink");
    std::os::unix::fs::symlink(dir.join("loop.md"), dir.join("loop.md"))
        .expect("create self-loop symlink");

    let non_utf8_name = OsString::from_vec(vec![b'x', 0xff, b'.', b'm', b'd']);
    let wrote_non_utf8 = std::fs::write(dir.join(&non_utf8_name), "# bytes\n").is_ok();

    let listing = Listing::read(&dir);
    assert!(listing.error().is_none());

    let kind_of = |name: &std::ffi::OsStr| {
        listing
            .entries()
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.kind)
    };

    assert_eq!(
        kind_of(std::ffi::OsStr::new("broken.md")),
        Some(EntryKind::Unopenable),
        "a broken symlink must not be Document"
    );
    assert_eq!(
        kind_of(std::ffi::OsStr::new("loop.md")),
        Some(EntryKind::Unopenable),
        "a symlink loop must not be Document, and reading it must not hang"
    );
    if wrote_non_utf8 {
        assert_eq!(
            kind_of(&non_utf8_name),
            Some(EntryKind::Unopenable),
            "a non-UTF8-named file with a non-markdown-looking extension test still classifies without panicking"
        );
    } else {
        eprintln!(
            "skipping the non-UTF8 name assertion: this filesystem refused to create the \
             fixture (APFS enforces UTF-8 filenames) — the same reasoning as \
             link.rs's test_a_filename_that_is_not_utf8_opens_because_the_command_line_opens_it"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}

// -------------------------------------------------------------------- DW-2.8

/// A directory past the cap lists exactly the cap's worth, reports
/// `truncated()`, and — since the cap is enforced before any `stat` beyond
/// it — returns quickly rather than paying for every entry on disk.
#[test]
fn test_dw_2_8_a_directory_past_the_cap_truncates_and_stays_within_a_bounded_time() {
    let dir = scratch_dir("past-cap");
    // One past whatever the cap is; the exact cap value is an implementation
    // detail this test does not hardcode. 300 is comfortably past any
    // reasonable cap while staying cheap to create.
    for i in 0..300 {
        write(&dir, &format!("f{i:04}.md"), "x");
    }

    let start = Instant::now();
    let listing = Listing::read(&dir);
    let elapsed = start.elapsed();

    assert!(listing.truncated(), "300 entries must exceed the cap");
    assert!(
        listing.entries().len() < 300,
        "a truncated listing must not contain every entry: {}",
        listing.entries().len()
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "a bounded stat count must keep this fast even on a slow machine: {elapsed:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// -------------------------------------------------------------------- DW-2.9

/// The overlay painter's byte contract for all three `RowStyle` variants:
/// `Selected` still opens with the reverse-video SGR the TOC has always
/// used, `Dimmed` (new this phase, for Phase 3's unopenable rows) opens
/// with the dim SGR, and `Ordinary` opens with neither.
#[test]
fn test_dw_2_9_the_selected_row_still_paints_reverse_video_and_dimmed_paints_dim() {
    let mut painter = Painter::new(WidthEngine::new(WidthConfig::default()));
    let size = Size {
        width: 20,
        height: 3,
    };
    let rows = vec![
        OverlayRow {
            text: "plain".to_string(),
            style: RowStyle::Ordinary,
        },
        OverlayRow {
            text: "picked".to_string(),
            style: RowStyle::Selected,
        },
        OverlayRow {
            text: "faded".to_string(),
            style: RowStyle::Dimmed,
        },
    ];
    let mut buf = Vec::new();
    painter
        .frame_overlay(&rows, size, &stele::app::StatusLine::default(), &mut buf)
        .expect("paint");
    let frame = String::from_utf8_lossy(&buf);

    assert!(
        frame.contains("\x1b[7mpicked"),
        "the selected row must open with reverse video: {frame:?}"
    );
    assert!(
        frame.contains("\x1b[2mfaded"),
        "the dimmed row must open with the dim SGR: {frame:?}"
    );
    assert!(
        !frame.contains("\x1b[7mplain") && !frame.contains("\x1b[2mplain"),
        "the ordinary row must open with neither attribute: {frame:?}"
    );
}

// ------------------------------------------------------------------- DW-2.10

/// A tiny, dependency-free xorshift64 PRNG — `crates/width` is the only
/// place `proptest` is used, and adding it here for one test file is out of
/// scope (`docs/code-standards.md`: no new dependency).
struct Xorshift64(u64);

impl Xorshift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_usize(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next() % bound as u64) as usize
        }
    }
}

/// Builds one arbitrary `Listing` from `rng`, including entries whose names
/// carry non-UTF8 bytes, entries with no name at all, and an occasional
/// out-of-range `selected`/`height` — the same shape DW-2.10's fuzz target
/// explores, driven here through the stable toolchain so the invariant
/// cannot silently rot between anyone running `cargo fuzz` by hand.
#[cfg(unix)]
fn arbitrary_listing(rng: &mut Xorshift64) -> (Listing, u16, usize) {
    use std::os::unix::ffi::OsStringExt as _;

    let entry_count = rng.next_usize(40);
    let entries: Vec<Entry> = (0..entry_count)
        .map(|i| {
            let kind = match rng.next() % 4 {
                0 => EntryKind::Parent,
                1 => EntryKind::Directory,
                2 => EntryKind::Document,
                _ => EntryKind::Unopenable,
            };
            let name_len = rng.next_usize(12);
            let name_bytes: Vec<u8> = (0..name_len).map(|_| (rng.next() % 256) as u8).collect();
            let name = OsString::from_vec(name_bytes);
            Entry {
                path: PathBuf::from(format!("entry-{i}")),
                name,
                kind,
            }
        })
        .collect();
    let listing = Listing::from_entries(
        PathBuf::from("/fuzz"),
        entries,
        rng.next().is_multiple_of(2),
    );
    // Skew toward small heights/selections (the common case) but reach
    // pathological values (0, near-`usize::MAX`) often enough to matter.
    let height = if rng.next().is_multiple_of(8) {
        (rng.next() % u64::from(u16::MAX)) as u16
    } else {
        rng.next_usize(20) as u16
    };
    let selected = if rng.next().is_multiple_of(8) {
        usize::MAX - rng.next_usize(4)
    } else {
        rng.next_usize(50)
    };
    (listing, height, selected)
}

/// DW-2.10(b): the stable-toolchain half of the two-way proof. 20,000
/// arbitrary listings, each checked for the three named invariants; the
/// seed is printed on any failure so a counterexample replays exactly.
#[test]
#[cfg(unix)]
fn test_dw_2_10_arbitrary_listings_never_panic_and_never_select_an_unselectable_row() {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9e3779b97f4a7c15)
        | 1; // xorshift64 cannot start at zero

    let mut rng = Xorshift64(seed);
    for iteration in 0..20_000u32 {
        let (listing, height, selected) = arbitrary_listing(&mut rng);
        let assertion_context = || format!("seed {seed:#x}, iteration {iteration}");

        // No panic: reaching the next line already proves it for this
        // iteration. No out-of-range index, and no landing on a
        // non-selectable row.
        let rows = listing.rows(height, selected);
        assert!(
            rows.len() <= usize::from(height),
            "rows exceeded the requested height ({}): {}",
            assertion_context(),
            rows.len()
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
                "out-of-range selectable index: {}",
                assertion_context()
            );
            assert_ne!(
                listing.entries()[index].kind,
                EntryKind::Unopenable,
                "a movement method returned an unselectable row: {}",
                assertion_context()
            );
        }
    }
}
