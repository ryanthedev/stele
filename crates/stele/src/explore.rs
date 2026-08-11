//! Turning a directory into rows a painter can draw — pure, no key handling,
//! no `Mode`, no `PendingAction`. `Listing::read` is the one function in this
//! crate that reads a directory; everything else here is deterministic
//! projection over what it found, exactly the way `Outline`/`toc_rows`
//! decide TOC content in code a test can call without a painter.
//!
//! The user's choice is "show everything, dim what cannot be opened" —
//! [`EntryKind::Unopenable`] is a listed classification, not an omission.
//!
//! **What `Unopenable` means, and the one direction the disagreement may
//! run.** The plan's hard constraint is that the explorer is never more
//! restrictive than the command line: any path `stele <path>` opens must be
//! openable from here. An `Unopenable` row is *unselectable* — the three
//! movement methods skip it, so the cursor can never land on one — which
//! makes a wrong `Unopenable` not a dimmed row but an unreachable file. It
//! is therefore reserved for what the barricade refuses **before reading a
//! byte**: a FIFO, a socket, a character or block device, or a symlink whose
//! target will not resolve. Every regular file is [`EntryKind::Document`]
//! whatever its extension, because whether stele can *render* it depends on
//! its content — valid UTF-8, no NUL — which this module cannot know without
//! reading, and reading is [`crate::link::Navigator::open_path`]'s job. A
//! binary regular file is therefore listed and selectable; `Enter` attempts
//! it and the barricade supplies the reason.
//!
//! The permitted disagreement runs **one way only**: this listing may offer
//! something the barricade then refuses. It must never refuse something the
//! command line accepts.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// The most directory entries [`Listing::read`] will consider.
///
/// The plan's stated hazard is a `read_dir` plus 10⁵ `stat` calls landing
/// inside an unkillable raw-mode loop on a single keystroke — the same
/// hazard class as a blocking `open(2)`, reached slowly instead of
/// instantly. 256 is nowhere near that, comfortably more than a terminal
/// viewport can show in one screen (so truncation is rare against a real
/// directory), and keeps the cap-exceeded test's fixture cheap to build.
///
/// **Counted against loop iterations, never against entries successfully
/// classified.** That distinction is a fix, not a detail: the cap used to be
/// checked against the classified count, so a directory where every child
/// `stat` fails — read permission without search permission is enough, and
/// `readdir` still yields every name — never reached the cap at all and paid
/// one `stat` per entry on disk. A review measured 2 000 entries at 3.24 ms
/// and 20 000 at 32.35 ms against the real function: 9.99× for 10×, which is
/// no bound whatsoever, and precisely the hazard this constant exists to
/// close.
///
/// **What it does not bound: wall-clock time — and no value of it could.**
/// An earlier version of this comment claimed the cap closed the hazard
/// outright. It closes the *count*. [`Listing::read`] runs synchronously on
/// the event-loop thread (`main.rs`'s `perform_action`) inside raw mode,
/// where `ISIG` is off and `Ctrl-C` is therefore a keystroke rather than a
/// signal — see `link.rs`'s barricade notes, which record the same fact.
/// One `fs::metadata` against a hung NFS or SMB mount, or against an autofs
/// mount point that triggers a mount when it is `stat`ed, blocks for the
/// server or mount timeout; 512 bounded calls are 512 unbounded waits, and
/// `Enter` on a `../` row that ascends into a stale mount reaches it. A
/// FIFO cannot do this to `fs::metadata` — that is what the barricade's
/// `open(2)` guards are for — but a dead server can, through a door those
/// guards do not cover.
///
/// **Deliberately not fixed, and this is the reasoning rather than a
/// silence.** The only real fix is to stop reading the directory on the
/// thread that reads keys: hand the path to a worker, paint a cancellable
/// "reading…" frame, and install the listing if and when it arrives — a
/// feature with its own state machine, cancellation semantics and tests,
/// not a hardening tweak, and one that would have to be threaded through
/// [`Listing::read`]'s totality contract. It is also not a hazard this
/// module introduces: the same event loop already blocks the same way on
/// `Navigator::open_path` reading a document off that mount, and on
/// `--watch`'s quarter-second `stat` of the watched file. Closing this one
/// door while those two stand open would buy a reader nothing. When it is
/// fixed it should be fixed for all three at once.
const MAX_LISTING_ENTRIES: usize = 256;

/// The directory path the explorer works in: `dir` made absolute against the
/// current directory, with every `.` dropped and every `..` cancelled.
///
/// **The barricade every path crossing into the explorer passes through**,
/// and it exists because of [`Path::parent`]: for a single-component
/// relative path that answers `Some("")`, not `None`.
/// `Path::new(".").parent()` and `Path::new("docs").parent()` are both the
/// empty path, which no `read_dir` can open. A `stele .` whose start
/// directory reached [`Listing::read`] verbatim therefore built a `../` row
/// aimed at `""`, and one press of `-` landed the reader in a listing with
/// no entries, no parent and no key that could leave it.
/// `loader.rs`'s `DocumentSource::base_dir` has always guarded that same
/// trap for image paths; this is the explorer's half of the same guard.
///
/// Lexical, and deliberately **not** [`fs::canonicalize`]: no `stat`, no
/// symlink resolution. A directory reached through a symlink keeps the name
/// the reader typed and ascends back the way they came rather than jumping
/// to the link's target's parent, a directory that cannot be read still
/// normalizes (its unreadability is [`Listing::read`]'s to report, not this
/// function's), and nothing here can block on a dead mount. Cancelling
/// `x/..` without consulting the filesystem is what `cd ..` does in a shell
/// and what a `../` row means to a reader.
///
/// Falls back to the lexically-normalized `dir` when the current directory
/// cannot be read at all — a process whose cwd was deleted out from under
/// it. That leaves a relative `dir` relative, which [`Listing::read`] still
/// handles correctly; a degraded answer, not a broken one.
pub fn absolute_dir(dir: &Path) -> PathBuf {
    match std::env::current_dir() {
        Ok(cwd) => lexical(&cwd.join(dir)),
        Err(_) => lexical(dir),
    }
}

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
    dropped: usize,
}

/// One entry in a [`Listing`]: what it is called, where it is, and what kind
/// it was classified as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: OsString,
    pub path: PathBuf,
    pub kind: EntryKind,
}

/// What an [`Entry`] is.
///
/// Decided from the entry's own [`fs::symlink_metadata`], except for a
/// symlink, which is decided from a *following* [`fs::metadata`] of whatever
/// it resolves to — see [`classify`] for why following is both safe and
/// required.
///
/// Exhaustive by design: a new variant is a compile error everywhere this is
/// matched, per `docs/code-standards.md`'s "no wildcard arms" rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// The `../` row. Present unless `dir` is its own parent — the
    /// filesystem root, and nothing else (see [`absolute_dir`] for why a
    /// relative `dir` is not the second case it once was).
    Parent,
    /// A directory, or a symlink resolving to one.
    Directory,
    /// A regular file, or a symlink resolving to one — **whatever its
    /// extension**. Selectable. Whether it renders is decided later, by
    /// reading it.
    Document,
    /// What the barricade refuses before reading a byte: a FIFO, a socket, a
    /// character or block device, or a symlink whose target will not resolve
    /// (a broken link, or a cycle reported as `ELOOP`).
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
    /// An [`EntryKind::Unopenable`] row — selected or not. Dim wins over
    /// the cursor; [`Listing::rows`] says why.
    Dimmed,
}

impl Listing {
    /// Reads `dir` into a [`Listing`].
    ///
    /// **Total.** Never panics; an unreadable `dir` produces a `Listing`
    /// whose [`Listing::error`] is `Some` rather than a `Result` the caller
    /// must unwrap.
    ///
    /// **Bounded.** One `read_dir`, then at most [`MAX_LISTING_ENTRIES`]
    /// iterations of the loop below — counted per iteration, so an entry
    /// whose `stat` fails still spends budget. Each iteration costs one
    /// [`fs::symlink_metadata`], plus one following [`fs::metadata`] when
    /// that first call reports a symlink, so the whole read is at most
    /// `2 * MAX_LISTING_ENTRIES` `stat` calls and no `open(2)` at all.
    /// Following the symlink is bounded for the reason [`classify`] gives: a
    /// cycle returns `ELOOP`, it does not hang.
    ///
    /// Bounded in syscall *count*. Not in wall-clock time, and this call
    /// runs on the thread that reads keys — [`MAX_LISTING_ENTRIES`] records
    /// what that exposes, why the cap cannot close it, and what a real fix
    /// would have to look like.
    ///
    /// **A partial read is reported, never hidden.** An entry whose
    /// `read_dir` step or `symlink_metadata` fails is left out — there is no
    /// honest row to paint for a name whose type is unknown — but it is
    /// counted in [`Listing::dropped`] and named by a row in
    /// [`Listing::rows`]. Dropping it *silently* was a defect: a directory
    /// with read but not search permission yields every name from `readdir`
    /// and `EACCES` from every child `stat`, so a directory of 200 000 files
    /// painted as empty with no error row and no truncation notice. Five
    /// files are enough to trigger it, and it is that directory's permanent
    /// state — not the read-then-stat vanishing race the silent drop was
    /// originally justified by, which is real but rare and is now simply
    /// reported the same way.
    pub fn read(dir: &Path) -> Listing {
        let mut entries = Vec::new();
        // [`parent_dir`], never `dir.parent()` — see that function for the
        // two paths `Path::parent` is wrong about, one of which (`.`) is
        // what `stele .` hands this function.
        if let Some(parent) = parent_dir(dir) {
            entries.push(Entry {
                name: OsString::from(".."),
                path: parent,
                kind: EntryKind::Parent,
            });
        }

        let mut read_dir = match fs::read_dir(dir) {
            Ok(read_dir) => read_dir,
            Err(error) => {
                return Listing {
                    dir: dir.to_path_buf(),
                    entries,
                    truncated: false,
                    error: Some(error),
                    dropped: 0,
                };
            }
        };

        let mut children = Vec::new();
        let mut dropped = 0;
        // `take` **is** the bound, rather than a counter compared against it
        // inside the loop — a counter is what the earlier version had, and it
        // counted the wrong thing. Nothing in the body can spend more than
        // one iteration or fewer than one, so the `stat` count follows from
        // the iterator rather than from reading the body correctly.
        for dir_entry in read_dir.by_ref().take(MAX_LISTING_ENTRIES) {
            match entry_of(dir_entry) {
                Some(entry) => children.push(entry),
                None => dropped += 1,
            }
        }
        // One more `readdir` step and no `stat` at all: whether anything is
        // left is the whole question `truncated` answers.
        let truncated = read_dir.next().is_some();
        children.sort_by(|a, b| a.name.cmp(&b.name));
        entries.extend(children);

        Listing {
            dir: dir.to_path_buf(),
            entries,
            truncated,
            error: None,
            dropped,
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
        Listing::from_parts(dir, entries, truncated, None, 0)
    }

    /// [`Listing::from_entries`] with the two notice states reachable too.
    ///
    /// The same test-and-fuzz seam, widened for one reason: both randomized
    /// suites used to hardcode "no error, nothing dropped", so the notice-row
    /// branch of [`Listing::rows`] — which holds the only subtraction in the
    /// windowing math — was never randomized at all. An off-by-one there was
    /// checkable only against one hand-built fixture.
    #[doc(hidden)]
    pub fn from_parts(
        dir: PathBuf,
        entries: Vec<Entry>,
        truncated: bool,
        error: Option<io::Error>,
        dropped: usize,
    ) -> Listing {
        Listing {
            dir,
            entries,
            truncated,
            error,
            dropped,
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

    /// Where `-` ascends to: the `../` row's own recorded target, or `None`
    /// at the filesystem root.
    ///
    /// Read off the row rather than recomputed from [`Listing::dir`], so the
    /// key and the row cannot disagree about where "up" is. They did:
    /// `AppState::ascend_explore` asked `dir().parent()` itself, which meant
    /// the two answers were only ever equal by coincidence and were both
    /// wrong for a relative directory — see [`parent_dir`].
    pub fn parent(&self) -> Option<&Path> {
        self.entries
            .first()
            .filter(|entry| entry.kind == EntryKind::Parent)
            .map(|entry| entry.path.as_path())
    }

    /// `true` if [`Listing::read`] stopped at [`MAX_LISTING_ENTRIES`] rather
    /// than exhausting the directory.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// How many entries `read_dir` named that this listing could not
    /// classify, and therefore does not contain.
    ///
    /// Separate from [`Listing::truncated`] because the two say different
    /// things: truncation means "there is more beyond the cap", dropping
    /// means "some of what is here could not be read". A caller that
    /// conflated them would tell a reader a directory is longer than it
    /// looks when in fact it is unreadable.
    pub fn dropped(&self) -> usize {
        self.dropped
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
    /// Empty for a zero-row viewport. When this listing has something to say
    /// about itself — see [`Listing::notice`] — the first row says it, and
    /// the remaining budget (if any) still windows the entries that are
    /// available (typically just the parent row), so an unreadable directory
    /// still shows the way back up.
    ///
    /// `selected` is **clamped**, not validated: an index past the end lands
    /// on the last row rather than painting no cursor at all.
    ///
    /// The one selection this does not honor is an [`EntryKind::Unopenable`]
    /// row, which paints [`RowStyle::Dimmed`] even when it is the selected
    /// index. Reverse video is a promise that `Enter` opens this row, and
    /// `Enter` refuses that one — so the promise must not be made. Nothing
    /// *seats* the cursor there any more either
    /// ([`Listing::seat`] checks the kind, the three movement methods always
    /// did), which leaves exactly one listing where the difference shows: one
    /// with no selectable row at all, where the honest frame has no cursor
    /// in it because there is nowhere for a cursor to be.
    pub fn rows(&self, height: u16, selected: usize) -> Vec<OverlayRow> {
        let height = usize::from(height);
        if height == 0 {
            return Vec::new();
        }

        let mut out = Vec::new();
        if let Some(text) = self.notice() {
            out.push(OverlayRow {
                text,
                style: RowStyle::Ordinary,
            });
        }

        // `out` now holds exactly the notice rows — zero or one — that
        // precede every entry row. **The two indices are different
        // coordinate spaces**, and `out.len()` here is the whole offset
        // between them: entry `index` paints at `out.len() + (index -
        // first)`, while `selected` and everything the movement methods
        // return index `self.entries` alone. Read that off this line rather
        // than re-deriving it from the loop below.
        let budget = height - out.len();
        // **A guard, and the `||` is load-bearing.** This used to be a pure
        // early exit — `window` below is `budget.min(self.entries.len())`,
        // already 0 whenever either operand here is — and `cargo mutants`
        // duly reported `||` → `&&` as an unkillable equivalent mutant,
        // triaged as such by three separate parties.
        //
        // It is not equivalent any more, and this comment is the correction.
        // The `selected` clamp below subtracts one from `self.entries.len()`,
        // which only the `is_empty()` half of this condition makes safe: with
        // `&&`, an empty listing carrying a notice row (a nonzero budget, no
        // entries) falls through and underflows. Two tests now fail on that
        // mutation and the sweep kills it, so the invariant "entries is
        // non-empty below this line" is enforced rather than merely intended.
        if budget == 0 || self.entries.is_empty() {
            return out;
        }

        let window = budget.min(self.entries.len());
        // Clamped rather than trusted. `first` was already clamped and
        // `selected` was not, so an index past the end (a listing that
        // shrank under a stale selection — the shape that broke
        // viewer-features Phases 2 and 5) matched no row and the frame
        // painted *no* cursor. Landing on the last row is the honest
        // answer; painting nothing is not.
        //
        // The `- 1` is safe on the guard above, not on optimism: `entries`
        // is provably non-empty here.
        let selected = selected.min(self.entries.len() - 1);
        let first = selected
            .saturating_sub(window / 2)
            .min(self.entries.len() - window);
        out.extend(self.entries[first..first + window].iter().enumerate().map(
            |(offset, entry)| {
                let index = first + offset;
                // Dim beats the cursor, deliberately — see this method's
                // doc. The two arms used to be the other way round, which
                // let a listing with nothing selectable paint reverse video
                // on a row `Enter` would refuse.
                let style = if entry.kind == EntryKind::Unopenable {
                    RowStyle::Dimmed
                } else if index == selected {
                    RowStyle::Selected
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

    /// The one row that precedes the entries when this listing has something
    /// to say about itself, or `None` when it does not.
    ///
    /// At most one row, and the error wins: a failed `read_dir` returns
    /// before any entry is considered, so [`Listing::read`] never produces
    /// both. That is what keeps the notice offset in [`Listing::rows`] a
    /// zero-or-one, and it is stated here rather than left to be inferred.
    ///
    /// **Truncation says so here, and the claim that it did not need to was
    /// false.** This comment used to argue that truncation could stay silent
    /// because [`Listing::truncated`] had "a designed consumer already —
    /// the status row". It had none: `explore_status_text` returns the
    /// directory path and nothing else, and nothing else in the crate read
    /// the flag. So a directory over [`MAX_LISTING_ENTRIES`] painted a
    /// listing that looked complete and was not — and worse than a missing
    /// tail, because the survivors are the first `MAX_LISTING_ENTRIES` in
    /// `readdir` order and only *then* sorted, so the missing names are
    /// scattered arbitrarily through the alphabet with nothing to mark the
    /// gaps. That is precisely the module doc's "must never refuse
    /// something the command line accepts", failing quietly.
    ///
    /// The cap itself stays. Paging is a feature, not a fix; a bigger cap
    /// moves the same cliff without removing it and buys more of the
    /// wall-clock exposure [`MAX_LISTING_ENTRIES`] documents; and reading
    /// every name before sorting so the omission is a clean alphabetical
    /// tail would make the `readdir` walk itself unbounded, which is the
    /// same trade in a different currency. What was actually wrong was the
    /// silence, so the silence is what changed.
    ///
    /// Still at most one row, which is what keeps the notice offset in
    /// [`Listing::rows`] a zero-or-one: an error returns before any entry is
    /// considered, so [`Listing::read`] never produces both, and truncation
    /// and dropped entries — which *can* co-occur — share one line.
    fn notice(&self) -> Option<String> {
        if let Some(error) = &self.error {
            return Some(format!("cannot read directory: {error}"));
        }
        let mut parts = Vec::new();
        if self.truncated {
            // "some names are missing", not "the rest are below": the
            // omitted entries are scattered through the sort order, and a
            // reader who is told the tail was cut would trust the alphabet.
            parts.push(format!(
                "listing capped at {MAX_LISTING_ENTRIES} entries: some names are missing"
            ));
        }
        match self.dropped {
            0 => {}
            1 => parts.push("1 entry could not be read".to_string()),
            dropped => parts.push(format!("{dropped} entries could not be read")),
        }
        (!parts.is_empty()).then(|| parts.join("; "))
    }

    /// Where the cursor lands in a freshly-read listing: the row named by
    /// `seed`, or [`Listing::default_selection`] when there is no seed, no
    /// row by that name, or a row by that name that cannot be selected.
    ///
    /// One method for three callers with three different ideas of `seed` —
    /// the open document's own name (`_`), the directory just left
    /// (`Enter`/`-`), or none at all (the initial rooted launch) — so the
    /// fallback rule cannot drift between them.
    ///
    /// **The kind check on the matched row is not decoration.** A name can
    /// change type between two listings — the directory just left, replaced
    /// by a FIFO of the same name — and seating on it would put the cursor
    /// exactly where the three movement methods refuse to go and where
    /// `Enter` refuses to act.
    pub fn seat(&self, seed: Option<&OsStr>) -> usize {
        seed.and_then(|name| {
            self.entries
                .iter()
                .position(|entry| entry.name == name && entry.kind != EntryKind::Unopenable)
        })
        .unwrap_or_else(|| self.default_selection())
    }

    /// Where a fresh listing lands with no seed to reseat by: the first
    /// selectable row that **is not** the `../` row, when one exists.
    ///
    /// [`Listing::first_selectable`] documents, correctly, that `../` itself
    /// counts as selectable — DW-2.3 pins exactly that, as the landing spot
    /// for a directory with nothing else in it. But `../` is always entry
    /// `0` when it exists, so a caller that used `first_selectable` as its
    /// *only* answer would land every fresh listing with real content on
    /// `../` instead of on the first real entry — including the very first
    /// frame a rooted `stele <dir>` paints. This tries "the first real
    /// entry" before falling back to `first_selectable`'s own answer
    /// (`../`, or `None`), so DW-2.3's fallback still holds for the
    /// directory it was written for: one with nothing selectable past the
    /// parent row.
    ///
    /// `0` when nothing at all is selectable — a parentless directory whose
    /// every child is a FIFO, socket, device or broken symlink. There is no
    /// honest cursor position in such a listing, and row `0` is therefore
    /// painted [`RowStyle::Dimmed`] rather than [`RowStyle::Selected`] (see
    /// [`Listing::rows`]) so the frame does not promise an `Enter` that
    /// would do nothing.
    fn default_selection(&self) -> usize {
        let starts_with_parent = matches!(
            self.entries.first(),
            Some(entry) if entry.kind == EntryKind::Parent
        );
        let skip_parent = starts_with_parent
            .then(|| self.next_selectable(0))
            .flatten();
        skip_parent.or_else(|| self.first_selectable()).unwrap_or(0)
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

/// The directory a listing of `dir` ascends to, or `None` when `dir` is its
/// own parent — the filesystem root, where there is nowhere above to go.
///
/// [`Path::parent`] is not this function, for the reason [`absolute_dir`]
/// gives and for a second one it does not: `parent` is also wrong for any
/// path whose last component is `..`. `Path::new("../..").parent()` is
/// `".."`, a step *down*, so `-` from `../..` would bounce between two
/// directories forever instead of ascending. Appending `..` and normalizing
/// gives `../../..`, which is what ascending means.
fn parent_dir(dir: &Path) -> Option<PathBuf> {
    let here = lexical(dir);
    let up = lexical(&here.join(".."));
    (up != here).then_some(up)
}

/// `path` with every `.` dropped and every `..` cancelled against the
/// component before it, touching no filesystem.
///
/// Two rules carry the weight, and together they are why [`parent_dir`]
/// always terminates. A leading `..` survives, because there is no
/// component in front of it to cancel — so ascending from a relative path
/// grows a `../` prefix forever and never cycles. And `/..` is `/`, because
/// the root is its own parent — so ascending from an absolute path stops
/// there. An empty result is `.`: "no components" names the current
/// directory, not the empty path that started all this.
fn lexical(path: &Path) -> PathBuf {
    let mut stack: Vec<Component> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match stack.last() {
                // Cancels the name in front of it.
                Some(Component::Normal(_)) => {
                    stack.pop();
                }
                // The root's parent is the root. A bare Windows prefix
                // (`C:..`, drive-relative) has no name to cancel and so
                // falls to the arm below, which keeps the `..`.
                Some(Component::RootDir) => {}
                _ => stack.push(component),
            },
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                stack.push(component);
            }
        }
    }
    let out: PathBuf = stack.iter().collect();
    if out.as_os_str().is_empty() {
        return PathBuf::from(".");
    }
    out
}

/// One `read_dir` item turned into an [`Entry`], or `None` when it cannot be
/// classified — which is the caller's cue to count it as dropped.
///
/// Two different failures fold into that one `None`: the directory read
/// failing for this particular item, and its [`fs::symlink_metadata`]
/// failing. They fold deliberately. Kept apart, each needed its own
/// `dropped += 1`, and the first of the two sits in a branch no test can
/// reach without fault injection — a mutation sweep duly found the
/// unreachable increment and survived two mutations of it. One increment,
/// on a path the unsearchable-directory tests do exercise, is both the
/// smaller code and the honest one: a reader cannot act differently on the
/// two cases, so the listing does not distinguish them.
fn entry_of(dir_entry: io::Result<fs::DirEntry>) -> Option<Entry> {
    let dir_entry = dir_entry.ok()?;
    let path = dir_entry.path();
    let meta = fs::symlink_metadata(&path).ok()?;
    Some(Entry {
        name: dir_entry.file_name(),
        kind: classify(&path, &meta),
        path,
    })
}

/// Classifies one directory entry.
///
/// A symlink is classified **by its target**, with a following
/// [`fs::metadata`], and is [`EntryKind::Unopenable`] only when that call
/// errors — a broken link, or a cycle, which the kernel reports as `ELOOP`
/// after `MAXSYMLINKS` hops.
///
/// **An earlier version returned `Unopenable` for every symlink, and its
/// stated justification was wrong twice over.** It claimed following would
/// risk a hang: it does not, the follow is bounded and an erroring `stat` is
/// already the shape this code handles. And because `Unopenable` rows are
/// unselectable, the rule did not dim those entries, it made them
/// unreachable — including a `README.md -> ../README.md` symlink that
/// `stele README.md` opens and that [`crate::link::Navigator::open_path`]
/// opens (its `canonicalize` resolves the link before the type check). On
/// macOS it also made `/tmp` and `/var`, both symlinks, impossible to enter
/// from a listing of `/`.
fn classify(path: &Path, meta: &fs::Metadata) -> EntryKind {
    if meta.file_type().is_symlink() {
        return match fs::metadata(path) {
            Ok(target) => kind_of(&target),
            Err(_) => EntryKind::Unopenable,
        };
    }
    kind_of(meta)
}

/// The kind `meta` names, with any symlink already resolved by the caller.
///
/// Every regular file is a [`EntryKind::Document`]: see the module doc for
/// why the extension is not consulted and why a binary file must still be
/// selectable.
fn kind_of(meta: &fs::Metadata) -> EntryKind {
    if meta.is_dir() {
        EntryKind::Directory
    } else if meta.is_file() {
        EntryKind::Document
    } else {
        EntryKind::Unopenable
    }
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
