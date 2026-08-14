//! DW-4.10(a) fuzz target: `EditPlan::diff` must never panic and must never
//! produce a plan that violates either safety invariant —
//!
//! 1. **no operation targets a path outside the listed directory**, and
//! 2. **no entry the reader left untouched appears in any operation.**
//!
//! Plus the three structural facts the rest of the module is built on: the
//! operations are laid out renames, then creates, then deletes;
//! `destructive()` is exactly the delete tail; and a truncated listing emits
//! no delete at all.
//!
//! **This target never calls `apply`.** That is not an oversight. `diff` is
//! the pure half and this is its contract; a fuzz target that called `apply`
//! would be a fuzz target that deleted files, over paths libFuzzer chose.
//! The apply half is covered by `crates/stele/tests/explore_edit.rs`, against
//! real fixture directories built and torn down per test.
//!
//! Drives `Listing::from_parts` and a synthesized `Vec<String>` rather than a
//! real directory, for the reasons `explore_listing.rs`'s target gives — and
//! one more that is specific to this phase: a non-UTF8 filename is
//! unreachable on APFS (`fs::write` returns `EILSEQ`, measured), so the
//! `NonUtf8Rename` refusal could not be fuzzed on disk on the platform this
//! is developed on at all.
//!
//! The edited buffer starts as the *honest* projection of the listing and is
//! then mutated by the fuzzer's own bytes. A purely arbitrary `Vec<String>`
//! would be rejected as malformed on nearly every input and would explore
//! almost none of the diff.
//!
//! Committed minimum campaign: `-max_total_time=90 -timeout=2`, run via
//! `cargo fuzz run edit_plan corpus/edit_plan seeds/edit_plan --
//! -max_total_time=90 -timeout=2`. The stable-toolchain twin is
//! `test_dw_4_10_arbitrary_edits_never_panic_and_never_touch_an_untouched_entry`
//! in `crates/stele/tests/explore_edit.rs`.

#![no_main]

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};

use libfuzzer_sys::fuzz_target;
use stele::explore::{
    EditBuffer, EditPlan, Entry, EntryKind, Listing, NameEquivalence, Operation,
};

/// The most entries built per input — the fuzzer explores byte layout, not
/// list length.
const MAX_FUZZ_ENTRIES: usize = 32;

/// The most buffer mutations applied per input.
const MAX_FUZZ_EDITS: usize = 16;

/// The directory every synthesized listing is read from.
const DIR: &str = "/listed/dir";

/// `Listing::rows`' private `row_text`, mirrored — without it, "the reader
/// left this row alone" cannot be computed independently of the code under
/// test, and invariant 2 would be asserting the implementation against
/// itself.
fn expected_row_text(entry: &Entry) -> String {
    match entry.kind {
        EntryKind::Parent => "../".to_string(),
        EntryKind::Directory => format!("{}/", entry.name.to_string_lossy()),
        EntryKind::Document | EntryKind::Unopenable => entry.name.to_string_lossy().into_owned(),
    }
}

/// Every path an operation names.
fn paths_of(op: &Operation) -> Vec<&Path> {
    match op {
        Operation::Rename { from, to } => vec![from.as_path(), to.as_path()],
        Operation::Create { path } => vec![path.as_path()],
        Operation::Delete { path, .. } => vec![path.as_path()],
    }
}

/// One of the line bodies worth trying: names, near-names, and things that
/// are not names at all.
fn text_for(byte: u8) -> String {
    match byte % 12 {
        0 => String::new(),
        1 => "   ".to_string(),
        2 => "..".to_string(),
        3 => ".".to_string(),
        4 => "../escape".to_string(),
        5 => "sub/nested".to_string(),
        6 => "shared".to_string(),
        7 => "with\ttab".to_string(),
        8 => "with\nbreak".to_string(),
        9 => "trailing/".to_string(),
        10 => "\u{0}nul".to_string(),
        _ => format!("gen-{byte}"),
    }
}

fuzz_target!(|data: &[u8]| {
    // One byte for the listing's flags, then two bytes per entry, then the
    // edit script.
    if data.len() < 3 {
        return;
    }
    let dir = PathBuf::from(DIR);
    let flags = data[0];
    let entry_count = usize::from(data[1]) % (MAX_FUZZ_ENTRIES + 1);

    let mut cursor = 2;
    let mut entries = Vec::new();
    while entries.len() < entry_count && cursor + 2 <= data.len() {
        let shape = data[cursor];
        let name_len = usize::from(data[cursor + 1]) % 8;
        cursor += 2;
        let end = (cursor + name_len).min(data.len());
        let index = entries.len();

        let kind = match shape % 5 {
            0 => EntryKind::Parent,
            1 => EntryKind::Directory,
            2 | 3 => EntryKind::Document,
            _ => EntryKind::Unopenable,
        };
        // Weighted exactly as the stable-toolchain twin's generator is, and
        // for the reason recorded there: names no `read_dir` could produce
        // must appear often enough to prove the barricade refuses them and
        // rarely enough that most listings still reach the diff.
        let name = match shape % 64 {
            0 => OsString::from(".."),
            1 => OsString::from("."),
            2 => OsString::new(),
            3 => OsString::from(format!("with/slash-{index}")),
            // Not index-prefixed: two of these is the duplicate-name case.
            4 | 5 => OsString::from("shared"),
            6..=11 => OsString::from_vec({
                let mut bytes = format!("raw-{index}-").into_bytes();
                bytes.extend_from_slice(&data[cursor..end]);
                bytes
            }),
            _ => OsString::from(format!("name-{index}")),
        };
        cursor = end;

        // A parent row's honest path is the directory above; every other
        // entry's is a child. Both are occasionally wrong on purpose.
        let astray = shape % 24 == 0;
        let path = match (kind, astray) {
            (EntryKind::Parent, false) => PathBuf::from("/listed"),
            (EntryKind::Parent, true) => dir.join(&name),
            (EntryKind::Directory | EntryKind::Document | EntryKind::Unopenable, false) => {
                dir.join(&name)
            }
            (EntryKind::Directory | EntryKind::Document | EntryKind::Unopenable, true) => {
                PathBuf::from("/elsewhere").join(&name)
            }
        };
        entries.push(Entry { name, path, kind });
    }

    let truncated = flags % 4 == 0;
    // The collision rules fold when the filesystem folds, so both answers have
    // to be fuzzed: `Folded` is the one that added a case comparison and an
    // origin-entry exemption to the `NameTaken` loop, and an exemption is
    // exactly the shape of thing that lets a collision through when it is one
    // index too wide.
    let equivalence = if flags % 2 == 0 {
        NameEquivalence::Exact
    } else {
        NameEquivalence::Folded
    };
    let listing = Listing::from_parts(
        dir.clone(),
        entries,
        truncated,
        None,
        usize::from(flags % 3),
    )
    .with_equivalence(equivalence);

    let mut lines = EditBuffer::new(&listing).lines();
    let mut edits = 0;
    while cursor < data.len() && edits < MAX_FUZZ_EDITS {
        let op = data[cursor];
        cursor += 1;
        edits += 1;
        if lines.is_empty() || op % 6 == 1 {
            lines.push(match op % 3 {
                0 => format!("\t{}", text_for(op)),
                1 => text_for(op),
                _ => format!("{}\t{}", usize::from(op) % 40, text_for(op)),
            });
            continue;
        }
        let at = usize::from(op) % lines.len();
        match op % 6 {
            0 => {
                lines.remove(at);
            }
            2 => {
                let id = lines[at]
                    .split_once('\t')
                    .map(|(id, _)| id.to_string())
                    .unwrap_or_default();
                lines[at] = format!("{id}\t{}", text_for(op));
            }
            3 => {
                let other = usize::from(op.rotate_left(3)) % lines.len();
                lines.swap(at, other);
            }
            4 => {
                let copy = lines[at].clone();
                lines.push(copy);
            }
            _ => lines.reverse(),
        }
    }

    // Reaching this at all is the no-panic half.
    let Ok(plan) = EditPlan::diff(&listing, &lines) else {
        return;
    };

    for op in plan.operations() {
        for path in paths_of(op) {
            assert_eq!(
                path.parent(),
                Some(dir.as_path()),
                "an operation left the listed directory: {op:?}"
            );
            assert!(
                path.file_name().is_some(),
                "an operation has no final component: {op:?}"
            );
        }
    }

    for (index, entry) in listing.entries().iter().enumerate() {
        let untouched = lines
            .iter()
            .any(|line| *line == format!("{index}\t{}", expected_row_text(entry)));
        if !untouched && entry.kind != EntryKind::Parent {
            continue;
        }
        for op in plan.operations() {
            for path in paths_of(op) {
                assert_ne!(
                    path, entry.path,
                    "an untouched entry appeared in an operation: {op:?}"
                );
            }
        }
    }

    let ops = plan.operations();
    let first_delete = ops.len() - plan.destructive().len();
    assert!(
        ops[..first_delete]
            .iter()
            .all(|op| !matches!(op, Operation::Delete { .. })),
        "a delete was ordered before the creates"
    );
    assert!(
        plan.destructive()
            .iter()
            .all(|op| matches!(op, Operation::Delete { .. })),
        "destructive() must be exactly the deletes"
    );
    if truncated {
        assert!(
            plan.destructive().is_empty(),
            "a truncated listing must produce no deletes: {ops:?}"
        );
    } else {
        assert_eq!(
            plan.suppressed_deletes(),
            0,
            "only truncation suppresses deletes"
        );
    }
});
