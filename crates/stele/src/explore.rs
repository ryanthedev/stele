//! Turning a directory into rows a painter can draw — pure, no key handling,
//! no `Mode`, no `PendingAction`. `Listing::read` is the one function in this
//! crate that reads a directory; everything else here is deterministic
//! projection over what it found, exactly the way `Outline`/`toc_rows`
//! decide TOC content in code a test can call without a painter.
//!
//! The user's choice is "show everything, dim what cannot be opened" —
//! [`EntryKind::Unopenable`] is a listed classification, not an omission.
//! Openability here is the cheap, non-blocking test (an extension check);
//! the authoritative refusal is [`crate::link::Navigator::open_path`]'s
//! barricade at open time, and the two are allowed to disagree — this
//! listing is a hint, the barricade is the rule.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::link::is_markdown_path;

/// The most entries [`Listing::read`] will `stat`.
///
/// The plan's stated hazard is a `read_dir` plus 10⁵ `stat` calls landing
/// inside an unkillable raw-mode loop on a single keystroke — the same
/// hazard class as a blocking `open(2)`, reached slowly instead of
/// instantly. 256 is nowhere near that, comfortably more than a terminal
/// viewport can show in one screen (so truncation is rare against a real
/// directory), and keeps the cap-exceeded test's fixture (257 files) cheap
/// to build. The check happens *before* the per-entry `symlink_metadata`
/// call — a truncated entry costs no `stat` at all.
const MAX_LISTING_ENTRIES: usize = 256;

/// A directory read into classified, ordered rows.
///
/// Total by construction: [`Listing::read`] never panics and never returns
/// `Result` — an unreadable directory is a value ([`Listing::error`]), not an
/// error path the caller must thread through `?`.
#[derive(Debug)]
pub struct Listing {
    dir: PathBuf,
    entries: Vec<Entry>,
    truncated: bool,
    error: Option<io::Error>,
}

/// One entry in a [`Listing`]: what it is called, where it is, and what kind
/// it was classified as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: OsString,
    pub path: PathBuf,
    pub kind: EntryKind,
}

/// What an [`Entry`] is, decided by [`fs::symlink_metadata`] alone — the
/// link itself is never followed (see the module's symlink note on
/// [`Listing::read`]).
///
/// Exhaustive by design: a new variant is a compile error everywhere this is
/// matched, per `docs/code-standards.md`'s "no wildcard arms" rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// The `../` row. Present unless `dir` has no parent (the filesystem
    /// root).
    Parent,
    /// A real directory, not a symlink to one.
    Directory,
    /// A regular file whose extension [`is_markdown_path`] accepts.
    Document,
    /// Everything else on disk: a non-markdown regular file, a FIFO, a
    /// socket, a device, or any symlink regardless of what it points at.
    Unopenable,
}

/// One row of a rendered overlay: the text to paint and how to paint it.
///
/// Shared between the TOC overlay and the explorer overlay (this phase
/// swaps [`crate::app::AppState::toc_rows`] onto this type at every existing
/// call site) — one row type, one painter entry point, for both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayRow {
    pub text: String,
    pub style: RowStyle,
}

/// How an [`OverlayRow`] paints.
///
/// An enum rather than a `dimmed: bool` beside the TOC's old `selected: bool`
/// precisely so Phase 4 can add `Edited`/`New`/`PendingDelete` as variants
/// instead of accumulating a second and third meaningless-when-combined
/// bool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowStyle {
    /// Plain text, no attribute.
    Ordinary,
    /// The row under the cursor — reverse video, the one attribute legible
    /// against either theme (the overlay has no theme roles of its own).
    Selected,
    /// An [`EntryKind::Unopenable`] row not currently selected.
    Dimmed,
}

impl Listing {
    /// Reads `dir` into a [`Listing`].
    ///
    /// **Total.** Never panics; an unreadable `dir` produces a `Listing`
    /// whose [`Listing::error`] is `Some` rather than a `Result` the caller
    /// must unwrap. Bounded to one `read_dir` plus at most
    /// [`MAX_LISTING_ENTRIES`] calls to [`fs::symlink_metadata`], one per
    /// entry — never a call that follows a symlink, so a broken link or a
    /// link cycle costs exactly one `stat` of the link itself, not a
    /// dereference (`symlink_metadata` never resolves the target, so
    /// `ELOOP` is structurally unreachable here, not merely caught).
    ///
    /// An entry that fails its own `read_dir` step, or has vanished by the
    /// time its `symlink_metadata` runs (the read-then-stat TOCTOU window),
    /// is silently dropped from the listing rather than surfaced as an
    /// error — the directory *as a whole* is still readable, so this is not
    /// [`Listing::error`]'s concern, and no bookkeeping exists to report a
    /// single vanished name usefully.
    pub fn read(dir: &Path) -> Listing {
        let mut entries = Vec::new();
        if let Some(parent) = dir.parent() {
            entries.push(Entry {
                name: OsString::from(".."),
                path: parent.to_path_buf(),
                kind: EntryKind::Parent,
            });
        }

        let read_dir = match fs::read_dir(dir) {
            Ok(read_dir) => read_dir,
            Err(error) => {
                return Listing {
                    dir: dir.to_path_buf(),
                    entries,
                    truncated: false,
                    error: Some(error),
                };
            }
        };

        let mut children = Vec::new();
        let mut truncated = false;
        for dir_entry in read_dir {
            let Ok(dir_entry) = dir_entry else {
                continue;
            };
            if children.len() >= MAX_LISTING_ENTRIES {
                truncated = true;
                break;
            }
            let path = dir_entry.path();
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            children.push(Entry {
                name: dir_entry.file_name(),
                kind: classify(&path, &meta),
                path,
            });
        }
        children.sort_by(|a, b| a.name.cmp(&b.name));
        entries.extend(children);

        Listing {
            dir: dir.to_path_buf(),
            entries,
            truncated,
            error: None,
        }
    }

    /// A pure, non-I/O `Listing`, built directly from `entries`.
    ///
    /// Hidden from the crate's ordinary API surface (it exposes no new
    /// concept — `Entry`/`EntryKind` are already public) but `pub` because
    /// it is the seam `crates/stele/fuzz`'s `explore_listing` target and
    /// this phase's stable-toolchain randomized test both drive: proving
    /// `rows`/`next_selectable`/`prev_selectable`/`first_selectable`'s
    /// invariants over arbitrary listings — including non-UTF8 names — at
    /// libFuzzer's normal in-memory throughput, rather than synthesizing a
    /// real directory tree per fuzz iteration (slow, and unable to reach a
    /// non-UTF8 name at all on a filesystem that rejects one, as APFS does).
    #[doc(hidden)]
    pub fn from_entries(dir: PathBuf, entries: Vec<Entry>, truncated: bool) -> Listing {
        Listing {
            dir,
            entries,
            truncated,
            error: None,
        }
    }

    /// The directory this listing was read from (or built for).
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Every entry, in the listing's stable order. `entries()[0]` is the
    /// parent row when one exists.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// `true` if [`Listing::read`] stopped at [`MAX_LISTING_ENTRIES`] rather
    /// than exhausting the directory.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// The error `read_dir` returned, if `dir` itself could not be read.
    pub fn error(&self) -> Option<&io::Error> {
        self.error.as_ref()
    }

    /// The rows this listing paints into a viewport `height` rows tall,
    /// scrolled so `selected` is always among them — the same windowing
    /// [`crate::app::AppState::toc_rows`] does, so a listing longer than the
    /// screen behaves exactly like a table of contents longer than the
    /// screen.
    ///
    /// Empty for a zero-row viewport. When [`Listing::error`] is `Some`, the
    /// first row names the failure and the remaining budget (if any) still
    /// windows the entries that are available (typically just the parent
    /// row) — an unreadable directory still shows the way back up.
    ///
    /// `selected` is never validated against selectability here: painting
    /// highlights whatever index the caller is pointing at, even a
    /// momentarily-unselectable one. Only the `*_selectable` movement
    /// methods enforce "never lands on `Unopenable`".
    pub fn rows(&self, height: u16, selected: usize) -> Vec<OverlayRow> {
        let height = usize::from(height);
        if height == 0 {
            return Vec::new();
        }

        let mut out = Vec::new();
        if let Some(error) = &self.error {
            out.push(OverlayRow {
                text: format!("cannot read directory: {error}"),
                style: RowStyle::Ordinary,
            });
        }

        let budget = height - out.len();
        // An early exit, not a correctness requirement: `window` below is
        // `budget.min(self.entries.len())`, which is already 0 whenever
        // either operand here is — so replacing `||` with `&&` changes no
        // observable output (`cargo mutants` confirms this survives; see
        // the Phase 2 discovery file's DW-2.11 triage). Kept for the reader,
        // who should not have to re-derive that from the `.min()` below.
        if budget == 0 || self.entries.is_empty() {
            return out;
        }

        let window = budget.min(self.entries.len());
        let first = selected
            .saturating_sub(window / 2)
            .min(self.entries.len() - window);
        out.extend(self.entries[first..first + window].iter().enumerate().map(
            |(offset, entry)| {
                let index = first + offset;
                let style = if index == selected {
                    RowStyle::Selected
                } else if entry.kind == EntryKind::Unopenable {
                    RowStyle::Dimmed
                } else {
                    RowStyle::Ordinary
                };
                OverlayRow {
                    text: row_text(entry),
                    style,
                }
            },
        ));
        out
    }

    /// The first selectable index, or `None` if nothing in this listing is
    /// — an empty directory whose parent is also absent (the filesystem
    /// root with nothing readable) is the only way to reach `None`.
    pub fn first_selectable(&self) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.kind != EntryKind::Unopenable)
    }

    /// The nearest selectable index strictly after `from`, or `None` if
    /// there is none — the caller's cue that moving down is a no-op.
    pub fn next_selectable(&self, from: usize) -> Option<usize> {
        self.entries
            .iter()
            .enumerate()
            .skip(from.saturating_add(1))
            .find(|(_, entry)| entry.kind != EntryKind::Unopenable)
            .map(|(index, _)| index)
    }

    /// The nearest selectable index strictly before `from`, or `None` if
    /// there is none — the caller's cue that moving up is a no-op.
    pub fn prev_selectable(&self, from: usize) -> Option<usize> {
        let before = &self.entries[..from.min(self.entries.len())];
        before
            .iter()
            .enumerate()
            .rev()
            .find(|(_, entry)| entry.kind != EntryKind::Unopenable)
            .map(|(index, _)| index)
    }
}

/// Classifies one directory entry from its own (never-followed) metadata.
///
/// A symlink is `Unopenable` regardless of what it points at: telling
/// "symlink to a directory" from "symlink to a FIFO" needs a *following*
/// `stat`, which is exactly the call this module's doc promises never to
/// make. See the module doc and [`Listing::read`] for why.
fn classify(path: &Path, meta: &fs::Metadata) -> EntryKind {
    if meta.file_type().is_symlink() {
        return EntryKind::Unopenable;
    }
    if meta.is_dir() {
        return EntryKind::Directory;
    }
    if meta.is_file() && is_markdown_path(path) {
        return EntryKind::Document;
    }
    EntryKind::Unopenable
}

/// The text an [`Entry`] paints as. Non-UTF8 bytes degrade lossily for
/// *display* only — [`Entry::path`] carries the exact bytes, unchanged, for
/// anything that actually opens the entry.
fn row_text(entry: &Entry) -> String {
    match entry.kind {
        EntryKind::Parent => "../".to_string(),
        EntryKind::Directory => format!("{}/", entry.name.to_string_lossy()),
        EntryKind::Document | EntryKind::Unopenable => entry.name.to_string_lossy().into_owned(),
    }
}
