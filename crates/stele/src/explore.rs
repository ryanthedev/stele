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

/// What the filesystem says a thing *is*, independent of the name it was
/// reached through: device and inode.
///
/// Recorded by [`Listing::read`] at the instant the reader saw the directory
/// and compared again inside [`EditPlan::apply`], immediately before anything
/// irreversible. It is not atomicity — `std` has no atomic delete-this-inode
/// — but it is what turns the `w`-to-`y` window, which is *human*-scale
/// because the confirmation prompt sits inside it, into a syscall-scale one,
/// and it refuses outright the attack that window makes possible: swapping a
/// doomed name for a file the reader never saw and having that one deleted
/// instead.
///
/// Opaque: only equality is meaningful, and only against another id taken the
/// same way (`symlink_metadata` for an entry, `metadata` for the directory).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileId {
    dev: u64,
    ino: u64,
}

#[cfg(unix)]
fn file_id(meta: &fs::Metadata) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt as _;

    Some(FileId {
        dev: meta.dev(),
        ino: meta.ino(),
    })
}

/// Off Unix `std` exposes no cheap identity, so nothing is recorded and every
/// identity comparison below degrades to "not recorded" — a skipped check, not
/// a guessed answer.
#[cfg(not(unix))]
fn file_id(_meta: &fs::Metadata) -> Option<FileId> {
    None
}

/// The identity of whatever sits *at* `path`, following nothing — a symlink
/// answers with the link's own inode, which is what makes deleting a link and
/// deleting its target two different operations to this code.
fn link_id(path: &Path) -> Option<FileId> {
    fs::symlink_metadata(path).ok().as_ref().and_then(file_id)
}

/// The identity `path` *resolves* to. Used for the listed directory itself,
/// because `read_dir` follows symlinks too: recording the link's inode and
/// comparing it later would miss the swap it exists to catch.
fn resolved_id(path: &Path) -> Option<FileId> {
    fs::metadata(path).ok().as_ref().and_then(file_id)
}

/// Whether the filesystem under a listed directory treats two different byte
/// sequences as two different names.
///
/// **Probed at read time, never assumed from the platform** — a single macOS
/// machine routinely has both (`/` case-insensitive APFS, a case-sensitive
/// APFS volume mounted under `/Volumes`), and so does a Linux box with an
/// exFAT stick in it. [`Listing::read`] measures it with at most two extra
/// `stat` calls and no writes; see [`probe_equivalence`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameEquivalence {
    /// Distinct bytes are distinct names: ext4, XFS, ZFS, APFS formatted
    /// case-sensitive.
    Exact,
    /// Names differing only by case are one name: APFS and HFS+ as macOS
    /// ships them, NTFS, exFAT.
    ///
    /// Unicode *normalization* insensitivity travels with these filesystems
    /// too and this variant does not name it, for a reason stated where it
    /// bites: [`EditPlan::diff`].
    Folded,
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
    /// What [`Listing::read`] found `entries[i]` to *be*, in lockstep with
    /// `entries`: `None` for the parent row and for every entry of a
    /// synthetic listing.
    ///
    /// Beside [`Listing::entries`] rather than inside [`Entry`] because an
    /// `Entry` is also a value tests and the fuzz target *invent*, where an
    /// inode is meaningless and would only pollute the derived `PartialEq`
    /// every one of those comparisons relies on. Read only through
    /// [`Listing::identity`], which answers `None` out of range — so a short
    /// vector degrades into "nothing recorded", which skips a check, and
    /// never into a wrong identity, which would refuse the right file or
    /// permit the wrong one.
    identities: Vec<Option<FileId>>,
    /// What `dir` itself resolved to when it was read. `None` for a synthetic
    /// listing. See [`EditPlan::verify_dir`].
    dir_identity: Option<FileId>,
    equivalence: NameEquivalence,
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
    /// `2 * MAX_LISTING_ENTRIES + 3` `stat` calls and no `open(2)` at all.
    /// Following the symlink is bounded for the reason [`classify`] gives: a
    /// cycle returns `ELOOP`, it does not hang.
    ///
    /// The three are the constant part, and they are what makes this function
    /// able to answer questions [`EditPlan::diff`] must answer without
    /// touching a disk: one for the directory's own identity, and at most two
    /// for [`probe_equivalence`].
    ///
    /// Bounded in syscall *count*. Not in wall-clock time, and this call
    /// runs on the thread that reads keys — [`MAX_LISTING_ENTRIES`] records
    /// what that exposes, why the cap cannot close it, and what a real fix
    /// would have to look like.
    ///
    /// **What the read records beyond the rows.** A `(device, inode)` per
    /// entry, the same for `dir` itself, and whether the filesystem folds
    /// case. All three are observations of *the moment the reader was shown
    /// this directory*, which is the only moment at which they mean anything:
    /// taking them later — inside `apply`, after the reader has spent a
    /// minute at the confirmation prompt — would be comparing the filesystem
    /// against itself. [`EditPlan::apply`] says what each one refuses.
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
        // Before `read_dir`, so that what is recorded is the directory this
        // listing was actually taken from, and taken *resolved* — `read_dir`
        // follows symlinks, so recording the link would let the swap this
        // exists to catch go unnoticed.
        let dir_identity = resolved_id(dir);
        let mut entries = Vec::new();
        let mut identities = Vec::new();
        // [`parent_dir`], never `dir.parent()` — see that function for the
        // two paths `Path::parent` is wrong about, one of which (`.`) is
        // what `stele .` hands this function.
        if let Some(parent) = parent_dir(dir) {
            entries.push(Entry {
                name: OsString::from(".."),
                path: parent,
                kind: EntryKind::Parent,
            });
            // The parent row is not a child and nothing may act on it, so
            // there is no identity worth the `stat`.
            identities.push(None);
        }

        let mut read_dir = match fs::read_dir(dir) {
            Ok(read_dir) => read_dir,
            Err(error) => {
                let equivalence = probe_equivalence(dir, dir_identity, &entries, &identities);
                return Listing {
                    dir: dir.to_path_buf(),
                    entries,
                    identities,
                    dir_identity,
                    equivalence,
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
                Some(child) => children.push(child),
                None => dropped += 1,
            }
        }
        // One more `readdir` step and no `stat` at all: whether anything is
        // left is the whole question `truncated` answers.
        let truncated = read_dir.next().is_some();
        children.sort_by(|(a, _), (b, _)| a.name.cmp(&b.name));
        // Unzipped in one step from a vector of pairs, so `entries[i]` and
        // `identities[i]` cannot come to mean different entries: they are
        // sorted together and split once, rather than maintained as two
        // lists that a later edit could push to unevenly.
        for (entry, identity) in children {
            entries.push(entry);
            identities.push(identity);
        }

        let equivalence = probe_equivalence(dir, dir_identity, &entries, &identities);
        Listing {
            dir: dir.to_path_buf(),
            entries,
            identities,
            dir_identity,
            equivalence,
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
            // Nothing here was read off a disk, so there is nothing to
            // record: every identity check downstream sees `None` and skips,
            // which is the honest answer for a listing that was invented.
            identities: Vec::new(),
            dir_identity: None,
            // Byte identity, because a synthetic listing has no filesystem
            // to ask. `with_equivalence` is how a test says otherwise.
            equivalence: NameEquivalence::Exact,
            truncated,
            error,
            dropped,
        }
    }

    /// The same seam once more, for the one fact [`Listing::read`] measures
    /// and [`Listing::from_parts`] cannot invent.
    ///
    /// Exists so that the folding half of [`EditPlan::diff`]'s collision rules
    /// is reachable from a test on *any* filesystem, rather than only on
    /// whichever kind the machine running the suite happens to have — the
    /// failure mode that left the previous case-collision guard green and
    /// vacuous on macOS.
    #[doc(hidden)]
    pub fn with_equivalence(mut self, equivalence: NameEquivalence) -> Listing {
        self.equivalence = equivalence;
        self
    }

    /// How this listing's filesystem decides whether two names are one name.
    pub fn equivalence(&self) -> NameEquivalence {
        self.equivalence
    }

    /// What [`Listing::read`] found `entries[index]` to be, or `None` when
    /// nothing was recorded — the parent row, a synthetic listing, or an
    /// index this listing does not have.
    fn identity(&self, index: usize) -> Option<FileId> {
        self.identities.get(index).copied().flatten()
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
        let mut window = overlay_window(height, self.entries.len(), selected, self.notice());
        let selected = window.selected;
        let first = window.painted.start;
        window
            .rows
            .extend(
                self.entries[window.painted]
                    .iter()
                    .enumerate()
                    .map(|(offset, entry)| {
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
                    }),
            );
        window.rows
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

/// What [`overlay_window`] worked out: the notice rows, which list rows to
/// paint after them, and where the cursor actually is.
struct Window {
    /// The notice rows — zero or one — ready for the caller to extend with
    /// its own painted rows.
    rows: Vec<OverlayRow>,
    /// The half-open range of list indices to paint. Empty when nothing fits.
    painted: std::ops::Range<usize>,
    /// `selected`, clamped into the list. Compare painted indices against
    /// *this*, never against the caller's argument.
    selected: usize,
}

/// The windowing every overlay shares: which `len` rows a `height`-row
/// viewport shows with `selected` among them, after a notice row.
///
/// **One copy, because two was a defect.** [`Listing::rows`] and
/// [`EditBuffer::rows`] held this arithmetic twice, character for character,
/// and only the first copy was tested — so a mutation sweep killed
/// `Listing::rows`'s `-`, `||` and `/` and left all three of
/// `EditBuffer::rows`'s alive. Identical code, one pinned and one not, is a
/// test gap wearing the costume of an equivalent mutant. The callers differ
/// only in where their rows come from and how each one is styled, which is
/// what they still own.
///
/// Three details carry the weight:
///
/// - **`rows.len()` is the offset between two coordinate spaces.** List index
///   `i` paints at `rows.len() + (i - painted.start)`, while `selected` and
///   everything the movement methods return index the list alone.
/// - **The guard's `||` is load-bearing**, and this is the correction of an
///   earlier comment that called it an equivalent mutant — triaged as such by
///   three separate parties. `window` below is `budget.min(len)`, already zero
///   whenever either operand is, so the guard *looks* like a pure early exit.
///   It is not: the clamp subtracts one from `len`, which only the `len == 0`
///   half makes safe. With `&&`, an empty list carrying a notice row — nonzero
///   budget, no rows — falls through and underflows.
/// - **`selected` is clamped, not validated.** An index past the end (a list
///   that shrank under a stale selection) used to match no row and paint *no*
///   cursor. Landing on the last row is the honest answer.
fn overlay_window(height: u16, len: usize, selected: usize, notice: Option<String>) -> Window {
    let height = usize::from(height);
    let empty = Window {
        rows: Vec::new(),
        painted: 0..0,
        selected,
    };
    if height == 0 {
        return empty;
    }

    let mut rows = Vec::new();
    if let Some(text) = notice {
        rows.push(OverlayRow {
            text,
            style: RowStyle::Ordinary,
        });
    }

    let budget = height - rows.len();
    if budget == 0 || len == 0 {
        return Window {
            rows,
            painted: 0..0,
            selected,
        };
    }

    let window = budget.min(len);
    // The `- 1` is safe on the guard above, not on optimism: `len` is provably
    // nonzero here.
    let selected = selected.min(len - 1);
    let first = selected.saturating_sub(window / 2).min(len - window);
    Window {
        rows,
        painted: first..first + window,
        selected,
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
///
/// The identity comes out of the `symlink_metadata` this already pays for, so
/// recording it costs no extra syscall — and it is `symlink_metadata`, so a
/// symlink's identity is its own and never its target's.
fn entry_of(dir_entry: io::Result<fs::DirEntry>) -> Option<(Entry, Option<FileId>)> {
    let dir_entry = dir_entry.ok()?;
    let path = dir_entry.path();
    let meta = fs::symlink_metadata(&path).ok()?;
    let identity = file_id(&meta);
    Some((
        Entry {
            name: dir_entry.file_name(),
            kind: classify(&path, &meta),
            path,
        },
        identity,
    ))
}

/// Whether this filesystem folds case, measured against `dir` and its own
/// entries — **never inferred from `cfg!(target_os)`**, because one machine
/// routinely has both: macOS ships a case-insensitive `/` and mounts
/// case-sensitive APFS volumes under `/Volumes`, and a Linux box with an
/// exFAT stick in it is the same story the other way round.
///
/// Read-only and cheap. Take the last component of a path whose identity is
/// already known, flip its case, and `stat` that: if the flipped spelling
/// lands on the very same inode, the filesystem folded it. At most two `stat`
/// calls — `case_flipped` answers `None` with no I/O at all for a name with no
/// cased letter in it, so only the first entry that *has* one costs anything.
///
/// **An entry is asked before `dir` itself, and the order is a correctness
/// fix rather than a preference.** A name lives in the filesystem of the
/// directory that *holds* it, so probing `dir`'s own last component measures
/// `dir`'s **parent**. For a mount point those are different filesystems, and
/// the answer is then wrong: a case-sensitive volume mounted at
/// `/Volumes/Sensitive` has that name stored by `/Volumes`, which folds. An
/// entry's name is stored by `dir`, which is the filesystem every operation in
/// a plan will actually run against. `dir` stays as the fallback for a
/// directory with nothing in it to ask — where there is nothing to collide
/// either.
///
/// Asking the entry first also keeps the `stat` inside the directory being
/// read. Probing `dir` reaches into its parent, and a nonexistent sibling
/// under an autofs map is exactly the kind of path that triggers a mount and
/// blocks — the hazard [`MAX_LISTING_ENTRIES`] records at length. One fallback
/// `stat` there is a smaller door than one on every read.
///
/// **Undecidable answers fold**, which is the conservative direction: a
/// directory whose own name and whose every entry's name is free of cased
/// letters (`/`, or `2026/01/02`) cannot be probed at all, and the cost of
/// guessing `Folded` there is a refused edit, while the cost of guessing
/// `Exact` is [`EditPlan::diff`] waving through a collision it should have
/// caught.
fn probe_equivalence(
    dir: &Path,
    dir_identity: Option<FileId>,
    entries: &[Entry],
    identities: &[Option<FileId>],
) -> NameEquivalence {
    for (entry, identity) in entries.iter().zip(identities) {
        if entry.kind == EntryKind::Parent {
            continue;
        }
        if let Some(answer) = probe_one(&entry.path, *identity, link_id) {
            return answer;
        }
    }
    if let Some(answer) = probe_one(dir, dir_identity, resolved_id) {
        return answer;
    }
    NameEquivalence::Folded
}

/// One probe: `None` when `path` cannot answer the question at all — nothing
/// recorded for it, no final component, a component that is not UTF-8, or one
/// with no cased letter to flip.
///
/// `id` must be the same call the recorded identity came from, or a symlink
/// would compare its own inode against its target's and every probe would say
/// `Exact`.
fn probe_one(
    path: &Path,
    identity: Option<FileId>,
    id: fn(&Path) -> Option<FileId>,
) -> Option<NameEquivalence> {
    let identity = identity?;
    let name = path.file_name()?.to_str()?;
    let probe = path.with_file_name(case_flipped(name)?);
    Some(if id(&probe) == Some(identity) {
        NameEquivalence::Folded
    } else {
        NameEquivalence::Exact
    })
}

/// `name` with its case flipped, or `None` when there is no cased letter in it
/// and flipping is therefore not a different spelling.
fn case_flipped(name: &str) -> Option<String> {
    let upper = name.to_uppercase();
    if upper != name {
        return Some(upper);
    }
    let lower = name.to_lowercase();
    (lower != name).then_some(lower)
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

// =========================================================== editing (Phase 4)
//
// The first code in this crate that mutates the filesystem, and the only
// place a reader can lose data. Four rules shape everything below.
//
// **Identity is [`Entry::path`], captured when the directory was read — never
// row position.** A reordered buffer is not a rename, and the only way an
// edited line can name the entry it came from is to carry that entry's index
// in its own text. It does: the buffer's line grammar is `"{id}\t{text}"`,
// which is oil.nvim's design and, given the pinned
// [`EditPlan::diff`] signature (`edited: &[String]`), the only design that
// can tell a rename from a delete-plus-create. Matching lines to entries by
// *text* cannot: an edited name matches nothing, so every rename would read
// as "the old file vanished, a new one appeared" and apply as delete-then-
// create — the exact loss this module exists to prevent.
//
// **[`EditPlan::diff`] performs no I/O.** `AppState` builds the confirmation
// prompt, so anything `diff` touched, `AppState` would touch, and `app.rs`'s
// no-I/O invariant would be gone. What `diff` needs to know about the disk it
// reads off the [`Listing`], which took it at read time: each entry's
// `(device, inode)`, the directory's own, and whether the filesystem folds
// case. The check that genuinely needs the *current* disk — "is this name
// already taken, including by something that was never in the listing" — is a
// pre-flight inside [`EditPlan::apply`], which runs in `main.rs` behind a
// `PendingAction`, in the instant before the first mutation.
//
// **Nothing here is trusted.** Every name is text a reader typed, and it is
// validated twice on the way to a syscall: once as a name
// ([`validate_new_name`]) and once as a *path*, by proving the join landed
// directly inside the listed directory ([`child_path`]). The listing's own
// entries pass the same barricade in `OsStr` form, which is what makes "no
// operation targets a path outside the listed directory" a property of the
// code rather than a hope — an entry named `..` would otherwise turn into a
// `Delete` of the parent directory.
//
// **The filesystem is the authority on what a name means, and on what is
// still there.** This is the rule an adversarial review had to add, having
// destroyed files three ways without it. Two names this code compares as
// different can be one name on disk, so the guard that matters is the one the
// kernel applies: `link(2)` and `O_EXCL`, which refuse a taken name atomically
// and under whatever folding rules the filesystem actually has. And a path
// that named the reader's file when the directory was read may name a
// stranger's by the time they answer the confirmation, so what is irreversible
// is done only to an inode the read recorded. Every comparison this module
// makes in memory — [`folds_together`], the pre-flight — is an early, legible
// refusal layered *over* those, never in place of them.

/// The gutter each buffer row paints in front of its name.
///
/// Two columns of text rather than three new [`RowStyle`] variants. The
/// variants were the plan's suggestion and would have been fine, but
/// `painter.rs` holds the only other exhaustive `RowStyle` match and is
/// outside this phase's file scope; and a marker a test can read out of
/// [`OverlayRow::text`] is a stronger oracle than an SGR attribute, which a
/// test can only compare as an enum. `Dimmed` is reused for a removed row.
///
/// **A file genuinely called `- baz` reads much like a row marked for
/// removal**, and a review raised it. It is a legibility question and not a
/// safety one, deliberately: the marker is prepended at paint time by
/// [`EditRow::marker`] and never stored in [`EditRow::text`], so the round
/// trip through the buffer is unaffected however a row is spelled —
/// `test_the_two_column_gutter_cannot_be_confused_with_a_filename` drives
/// exactly `~ foo`, `+ bar`, `- baz` and `  quux` back through the diff and
/// gets an empty plan. What separates the two on screen is the fixed-width
/// column: every marker is two cells, so real names start at the same column
/// on every row and `  - baz` sits one gutter to the right of `- baz`. Making
/// the difference louder means picking a glyph no filename starts with, which
/// on a UTF-8 terminal is a font gamble and on a `LANG=C` one is mojibake in
/// the one place a reader is deciding what to delete. Left as is, and recorded
/// rather than silently dismissed.
const MARK_UNCHANGED: &str = "  ";
const MARK_EDITED: &str = "~ ";
const MARK_NEW: &str = "+ ";
const MARK_REMOVED: &str = "- ";

/// How many doomed names the confirmation prompt spells out before it stops
/// counting. Enough that an ordinary edit names every one of them; small
/// enough that "delete everything" cannot push the count itself off a
/// narrow status row, which is the number that matters most in that case.
const MAX_NAMED_DELETES: usize = 6;

/// One change to the filesystem.
///
/// Carries whole paths rather than names: the paths are built once, inside
/// the barricade, and nothing downstream re-derives them from text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    /// Move `from` to `to`. Both are direct children of the listed directory.
    ///
    /// **Never replaces what is at `to`.** `fs::rename` would — silently, on
    /// Unix — so [`rename_no_replace`] does not use it in the ordinary case:
    /// it creates `to` with `fs::hard_link`, which fails `EEXIST` in the
    /// kernel with no window at all, and only then unlinks `from`.
    Rename { from: PathBuf, to: PathBuf },
    /// Create an empty file at `path`, exclusively — never truncating
    /// whatever might be there.
    Create { path: PathBuf },
    /// Remove `path`. **The point of no return**: no trash, no undo.
    ///
    /// `identity` is what the directory read found under this name, and
    /// [`remove`] refuses when the name has come to hold something else since
    /// — the demonstrated attack on the human-scale `w`-to-`y` window. `None`
    /// where nothing was recorded (a synthetic listing, or a platform with no
    /// inode), which skips the check rather than inventing an answer.
    Delete {
        path: PathBuf,
        identity: Option<FileId>,
    },
}

impl Operation {
    /// The path this operation acts on — the destination for a rename, since
    /// that is the name a collision would be about.
    fn target(&self) -> &Path {
        match self {
            Operation::Rename { to, .. } => to,
            Operation::Create { path } => path,
            Operation::Delete { path, .. } => path,
        }
    }
}

impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Operation::Rename { from, to } => {
                write!(f, "rename {} to {}", display_name(from), display_name(to))
            }
            Operation::Create { path } => write!(f, "create {}", display_name(path)),
            Operation::Delete { path, .. } => write!(f, "delete {}", display_name(path)),
        }
    }
}

/// The last component of `path`, for a message. Lossy for display only;
/// nothing acts on the result.
fn display_name(path: &Path) -> std::borrow::Cow<'_, str> {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
}

/// Why a name a reader typed cannot become a filename.
///
/// Exhaustive, no wildcard: a new fault is a compile error at every match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameFault {
    /// Empty, or nothing but whitespace. A file called `"   "` is legal on
    /// Unix and indistinguishable from a blank line on screen, which is the
    /// whole reason to refuse creating one. Entries that already exist under
    /// such a name are still listed and still deletable — [`entry_name_ok`]
    /// is the weaker check that governs those.
    Blank,
    /// Contains a path separator. This is the `..`-traversal refusal too:
    /// `../x` and `sub/x` both die here rather than in [`NameFault::Dot`].
    Separator,
    /// Exactly `.` or `..` — a name that is a directory reference, not a
    /// child.
    Dot,
    /// Contains a NUL, which no syscall can carry.
    Nul,
    /// Contains a newline or carriage return. Legal on Unix; refused because
    /// the buffer is line-based, so such a name cannot survive a round trip
    /// through it and would silently become two rows.
    Newline,
    /// The join did not land directly inside the listed directory. Nothing
    /// should ever reach this after the checks above — that is exactly why it
    /// is here. See [`child_path`].
    Escapes,
}

impl std::fmt::Display for NameFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            NameFault::Blank => "a name cannot be empty or only whitespace",
            NameFault::Separator => "a name cannot contain a path separator",
            NameFault::Dot => "`.` and `..` are not names",
            NameFault::Nul => "a name cannot contain a NUL byte",
            NameFault::Newline => "a name cannot contain a line break",
            NameFault::Escapes => "that name does not stay inside this directory",
        };
        f.write_str(text)
    }
}

/// Why an edited buffer could not become a plan.
///
/// Every variant is a *refusal before anything runs*. There is no partial
/// diff: either the whole buffer becomes a plan or none of it does, because a
/// reader who mistyped one row should not have the other nine applied out
/// from under them.
///
/// Hand-rolled with a manual `Display`, per `docs/code-standards.md`; the
/// variants carry the data a caller needs to point at the offending row
/// rather than a pre-formatted string. `line` is the 0-based index into the
/// buffer's lines; `Display` shows it 1-based, which is what a reader counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// A line with no `id` separator at all. `AppState` cannot produce one;
    /// a fuzzer can.
    MalformedRow { line: usize },
    /// A line naming an entry index this listing does not have.
    UnknownRow { line: usize, id: usize },
    /// Two lines claiming the same entry — which of them is the rename?
    DuplicateRow { line: usize, id: usize },
    /// A line naming the `../` row, which is not a child of this directory
    /// and cannot be renamed, created or deleted from here.
    ParentRow { line: usize },
    /// The listing itself is not something this module will edit. Four
    /// shapes, all impossible from `Listing::read` and all reachable through
    /// [`Listing::from_entries`]: an entry whose name is not a legal single
    /// path component (an entry named `..` would otherwise become a `Delete`
    /// of the parent directory), an entry whose recorded path is not the
    /// listed directory joined with its own name, two entries sharing a name,
    /// and a `../` row pointing back into the directory it heads. The last
    /// two were found by the randomized suite rather than by inspection.
    InconsistentListing { name: String },
    /// The reader typed something that is not a filename.
    IllegalName {
        line: usize,
        name: String,
        fault: NameFault,
    },
    /// The new name is already the name of an entry in this listing.
    ///
    /// This one refusal covers four of the adversarial cases at once: a
    /// rename onto a file that is staying; a rename onto a row the reader
    /// removed (the delete runs *last*, so the rename would overwrite it); a
    /// create colliding with a pending delete; and a rename cycle or chain
    /// (`a`→`b`, `b`→`a`), where every target is an occupied name. Refusing
    /// the cycle rather than shuffling through a temporary name is
    /// deliberate — see [`EditPlan::diff`].
    ///
    /// `existing` is the entry's spelling, which is `name` itself in the
    /// ordinary case and differs when the filesystem folded the two together.
    /// Saying "`target.md` already exists" when what exists is `Target.md`
    /// would send a reader looking for a file that is not there under that
    /// name.
    NameTaken {
        line: usize,
        name: String,
        existing: String,
    },
    /// Two edits produce the same name — byte for byte, or as far as this
    /// filesystem is concerned. `other` is the earlier spelling; see
    /// [`EditError::NameTaken`] for why both are carried.
    DuplicateName { name: String, other: String },
    /// A rename of an entry whose name is not valid UTF-8. Its row shows a
    /// lossy rendering, so any edit of that row is a guess about bytes the
    /// reader never actually saw. Leaving the row alone and removing it both
    /// still work: neither touches the text.
    NonUtf8Rename { line: usize, shown: String },
    /// A new row ending in `/`. This phase creates files, not directories,
    /// and creating a file named without the slash would answer a different
    /// question than the one asked.
    DirectoryCreate { line: usize, name: String },
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::MalformedRow { line } => {
                write!(f, "line {}: not a buffer row", line + 1)
            }
            EditError::UnknownRow { line, id } => {
                write!(f, "line {}: no entry {id} in this listing", line + 1)
            }
            EditError::DuplicateRow { line, id } => {
                write!(f, "line {}: entry {id} is named twice", line + 1)
            }
            EditError::ParentRow { line } => {
                write!(f, "line {}: the `../` row cannot be edited", line + 1)
            }
            EditError::InconsistentListing { name } => write!(
                f,
                "`{name}`: this is not a plain reading of one directory and cannot be edited"
            ),
            EditError::IllegalName { line, name, fault } => {
                write!(f, "line {}: `{name}` — {fault}", line + 1)
            }
            EditError::NameTaken {
                line,
                name,
                existing,
            } if name == existing => write!(
                f,
                "line {}: `{name}` already exists here — move it away in a separate write first",
                line + 1
            ),
            EditError::NameTaken {
                line,
                name,
                existing,
            } => write!(
                f,
                "line {}: `{name}` and `{existing}`, which already exists here, are one name on \
                 this filesystem",
                line + 1
            ),
            EditError::DuplicateName { name, other } if name == other => {
                write!(f, "two rows both name `{name}`")
            }
            EditError::DuplicateName { name, other } => write!(
                f,
                "`{name}` and `{other}` are one name on this filesystem, and two rows ask for both"
            ),
            EditError::NonUtf8Rename { line, shown } => write!(
                f,
                "line {}: `{shown}` is not valid text on disk and cannot be renamed from here",
                line + 1
            ),
            EditError::DirectoryCreate { line, name } => write!(
                f,
                "line {}: `{name}` — this phase creates files, not directories",
                line + 1
            ),
        }
    }
}

impl std::error::Error for EditError {}

/// What an edited buffer asks the filesystem to do, in the order it will be
/// done.
///
/// **Storage order is apply order** — renames, then creates, then deletes,
/// in one vector, with [`EditPlan::first_delete`] marking where the
/// irreversible part starts. One vector rather than three fields precisely so
/// the order and the destructive-operation slice cannot drift apart: both are
/// read off the same layout, and [`EditPlan::destructive`] is a subslice, not
/// a filtered copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditPlan {
    dir: PathBuf,
    /// What `dir` resolved to when the listing was read, so [`EditPlan::apply`]
    /// can refuse when the directory under the plan is no longer the one the
    /// reader was looking at. `None` for a plan built over a synthetic
    /// listing.
    dir_identity: Option<FileId>,
    ops: Vec<Operation>,
    /// Index of the first [`Operation::Delete`]; `ops.len()` when there is
    /// none.
    first_delete: usize,
    /// Rows the reader removed that produced no delete, because the listing
    /// was truncated and absence therefore cannot mean removal. Reported, not
    /// swallowed — see [`EditPlan::diff`].
    suppressed_deletes: usize,
}

impl EditPlan {
    /// The difference between `read` as it was listed and `edited` as the
    /// reader left it.
    ///
    /// **Pure.** Touches no filesystem, so `AppState` may call it.
    ///
    /// `edited` is one string per surviving buffer row, in the grammar
    /// [`EditBuffer::lines`] emits: `"{id}\t{text}"` for a row seeded from
    /// `read.entries()[id]`, `"\t{text}"` for a row the reader added. A row
    /// the reader removed is simply *absent*, which is the single inference
    /// in this whole module and the only thing truncation can poison.
    ///
    /// **Precondition:** `edited` must have come from an [`EditBuffer`] built
    /// from *this* `read`. `AppState` guarantees it structurally —
    /// [`crate::app::AppState::install_listing`] drops the buffer whenever it
    /// installs a listing, so a buffer cannot outlive the indices it names.
    /// A mismatched pair is still *safe* (every path is derived from `read`
    /// itself, so no operation can leave the listed directory) but it would
    /// name the wrong entries, which is why the coupling is enforced by
    /// construction rather than documented and hoped for.
    ///
    /// ## Refusals, and why they are refusals rather than repairs
    ///
    /// After every line is classified, two rules run over the names the edit
    /// produces:
    ///
    /// - **A produced name may not be the name of any entry in the listing.**
    ///   Blunt on purpose. It refuses a rename onto a file that is staying, a
    ///   rename onto a row the reader removed (deletes run last, so the
    ///   rename would land on a file that still exists), a create colliding
    ///   with a pending delete, and a rename cycle `a`→`b`, `b`→`a`. It also
    ///   refuses an *orderable* chain (`a`→`b`, `b`→`c`), which a topological
    ///   sort could have applied. The trade is deliberate: the only way to
    ///   apply a true cycle is to shuffle through a temporary name, and a
    ///   partial failure mid-shuffle strands a `stele-tmp-…` file holding the
    ///   reader's data under a name they never chose. DW-4.6 permits refusal;
    ///   refusal leaves the directory exactly as the reader last saw it,
    ///   which is the outcome that cannot surprise anyone.
    /// - **Two edits may not produce the same name**, which is the two-rows-
    ///   renamed-to-`z` case. Without it the second rename would land on the
    ///   first one's result.
    ///
    /// ## "The same name" is the filesystem's question, not `==`
    ///
    /// Both rules used to compare `String`s byte for byte, and an adversarial
    /// review destroyed a file with that: on the case-folding filesystem macOS
    /// ships, `Target.md` and `target.md` are two different byte strings and
    /// one directory entry, so two renames both passed every rule here, the
    /// pre-flight found neither target on disk, and the second `rename`
    /// replaced the first one's result. `destructive()` was empty and the
    /// summary said "applied 2 operations".
    ///
    /// So the comparison folds when [`Listing::equivalence`] — measured
    /// against the real filesystem at read time, never inferred from the
    /// platform — says the filesystem folds. Two things follow, and both are
    /// deliberate:
    ///
    /// - On a case-sensitive filesystem nothing changes. `README` and `readme`
    ///   are two files there, and refusing to make both would be a bug.
    /// - **Unicode normalization is not folded here, and cannot be.** NFC
    ///   `café.md` and NFD `café.md` are one name on APFS and are visually
    ///   identical, so a reader cannot even see that collision — it is the
    ///   nastier half of the same finding. Deciding it needs a Unicode
    ///   normalization table, which is not in `std` and which the plan's
    ///   no-new-dependency constraint forbids adding. It is therefore closed
    ///   one layer down instead, atomically and for every folding rule any
    ///   filesystem might have, by [`rename_no_replace`] and `create_new`;
    ///   this rule is the early, legible refusal, not the load-bearing one.
    ///   `test_f1_two_renames_colliding_by_unicode_normalization_never_lose_a_file`
    ///   is the proof that the layer below holds without this one.
    ///
    /// ## Truncation
    ///
    /// When `read` was capped by [`Listing::read`]'s entry limit, the reader
    /// saw the first 256 names `readdir` happened to yield, sorted only
    /// afterwards — so the names they did *not* see are scattered arbitrarily
    /// through the alphabet. Absence from the buffer can no longer mean "the
    /// reader removed it", so **no delete is emitted at all**. The count of
    /// rows that would have become deletes is kept in
    /// [`EditPlan::suppressed_deletes`] and named by
    /// [`EditPlan::confirmation`], so a reader who removed a row learns it was
    /// ignored instead of believing it happened. Renames and creates are
    /// unaffected: both are named explicitly, never inferred from absence.
    ///
    /// A nonzero [`Listing::dropped`] deliberately does *not* suppress
    /// deletes. A dropped entry has no row, so no row of it can be removed;
    /// its name can still collide with a create or a rename target, and the
    /// apply-time pre-flight is what catches that.
    pub fn diff(read: &Listing, edited: &[String]) -> Result<EditPlan, EditError> {
        let dir = read.dir().to_path_buf();
        let entries = read.entries();

        // The barricade over the listing itself, before a single line is
        // read. Every non-parent entry must be a plain child of `dir`, by
        // name *and* by the path it recorded. `Listing::read` cannot produce
        // anything else — `dir_entry.path()` is literally `dir.join(name)`
        // and `readdir` never yields `.` or `..` — but `Listing::from_entries`
        // can, and this is what turns "no operation targets a path outside
        // the listed directory" into a property that holds for every input
        // the fuzz target can invent, not just for the ones a real
        // filesystem produces.
        for (index, entry) in entries.iter().enumerate() {
            if entry.kind == EntryKind::Parent {
                // A parent row points *out* of the listed directory — that is
                // the whole of what `../` means. One that points back *into*
                // it would alias a name an edit can legitimately produce, and
                // the randomized suite found exactly that: a synthetic parent
                // row recorded at `dir/shared` while a rename targeted
                // `dir/shared`, so an operation's path collided with an entry
                // nothing had touched. Unreachable from `Listing::read`, whose
                // parent row is always the lexical parent; refused here so it
                // is unreachable from anywhere.
                if entry.path.parent() == Some(dir.as_path()) {
                    return Err(EditError::InconsistentListing {
                        name: entry.path.to_string_lossy().into_owned(),
                    });
                }
                continue;
            }
            if !entry_name_ok(&entry.name) || entry.path != dir.join(&entry.name) {
                return Err(EditError::InconsistentListing {
                    name: entry.name.to_string_lossy().into_owned(),
                });
            }
            // `readdir` yields each name exactly once, so two entries sharing
            // one is not a directory — it is a listing that was built rather
            // than read. Refused because it makes the reader's edit
            // ambiguous: with two identical rows, removing one and keeping
            // the other is both a delete and a keep of the same file, and
            // the randomized suite duly produced a plan that deleted a path
            // an untouched row still named.
            if entries[..index]
                .iter()
                .any(|earlier| earlier.kind != EntryKind::Parent && earlier.name == entry.name)
            {
                return Err(EditError::InconsistentListing {
                    name: entry.name.to_string_lossy().into_owned(),
                });
            }
        }

        let mut present = vec![false; entries.len()];
        let mut renames = Vec::new();
        let mut creates = Vec::new();
        // Every name this edit brings into existence: the line that asked
        // for it, the entry it is *renaming away from* (`None` for a create),
        // and the name itself.
        //
        // **The origin is not bookkeeping.** Once the collision rules fold,
        // an entry collides with itself: `readme.md` → `README.md` produces a
        // name that folds against `readme.md`, which is the very row being
        // renamed, and refusing it would make the commonest rename in the
        // world impossible on macOS. Byte comparison never had to say this
        // out loud because the two spellings differed.
        let mut produced: Vec<(usize, Option<usize>, String)> = Vec::new();

        for (line, text) in edited.iter().enumerate() {
            let Some((prefix, text)) = text.split_once('\t') else {
                return Err(EditError::MalformedRow { line });
            };

            if prefix.is_empty() {
                // A row the reader added.
                if text.ends_with('/') {
                    return Err(EditError::DirectoryCreate {
                        line,
                        name: text.to_string(),
                    });
                }
                let path = child_path(&dir, text).map_err(|fault| EditError::IllegalName {
                    line,
                    name: text.to_string(),
                    fault,
                })?;
                produced.push((line, None, text.to_string()));
                creates.push(Operation::Create { path });
                continue;
            }

            let Ok(id) = prefix.parse::<usize>() else {
                return Err(EditError::MalformedRow { line });
            };
            let Some(entry) = entries.get(id) else {
                return Err(EditError::UnknownRow { line, id });
            };
            if entry.kind == EntryKind::Parent {
                return Err(EditError::ParentRow { line });
            }
            if present[id] {
                return Err(EditError::DuplicateRow { line, id });
            }
            // Marked before anything else can go wrong with this line: an
            // entry the reader *kept* must never be delete-inferred, whatever
            // else its row says.
            present[id] = true;

            if text == row_text(entry) {
                continue;
            }

            // The row changed — but a non-UTF8 name only ever *showed* as a
            // lossy rendering, so the comparison above is the only honest
            // question that can be asked about it. Anything else is a guess.
            let Some(current) = entry.name.to_str() else {
                return Err(EditError::NonUtf8Rename {
                    line,
                    shown: entry.name.to_string_lossy().into_owned(),
                });
            };
            // A directory row paints as `name/`. Dropping that slash is not a
            // rename, and adding one is not a request to change a file into a
            // directory — the trailing slash is display sugar in both
            // directions, and stripping at most one leaves `foo//` to die on
            // the separator check rather than quietly becoming `foo/`.
            let name = text.strip_suffix('/').unwrap_or(text);
            if name == current {
                continue;
            }
            let to = child_path(&dir, name).map_err(|fault| EditError::IllegalName {
                line,
                name: name.to_string(),
                fault,
            })?;
            produced.push((line, Some(id), name.to_string()));
            renames.push(Operation::Rename {
                from: entry.path.clone(),
                to,
            });
        }

        let equivalence = read.equivalence();
        for (line, origin, name) in &produced {
            let collision = entries.iter().enumerate().find(|(index, entry)| {
                Some(*index) != *origin
                    && entry.kind != EntryKind::Parent
                    && folds_together(equivalence, &entry.name, name)
            });
            if let Some((_, entry)) = collision {
                return Err(EditError::NameTaken {
                    line: *line,
                    name: name.clone(),
                    existing: entry.name.to_string_lossy().into_owned(),
                });
            }
        }
        for (index, (_, _, name)) in produced.iter().enumerate() {
            let earlier = produced[..index]
                .iter()
                .find(|(_, _, seen)| folds_together(equivalence, OsStr::new(seen), name));
            if let Some((_, _, other)) = earlier {
                return Err(EditError::DuplicateName {
                    name: name.clone(),
                    other: other.clone(),
                });
            }
        }

        let mut deletes = Vec::new();
        let mut suppressed_deletes = 0;
        for (index, entry) in entries.iter().enumerate() {
            if present[index] || entry.kind == EntryKind::Parent {
                continue;
            }
            if read.truncated() {
                suppressed_deletes += 1;
                continue;
            }
            deletes.push(Operation::Delete {
                path: entry.path.clone(),
                identity: read.identity(index),
            });
        }

        let first_delete = renames.len() + creates.len();
        let mut ops = renames;
        ops.extend(creates);
        ops.extend(deletes);
        Ok(EditPlan {
            dir,
            dir_identity: read.dir_identity,
            ops,
            first_delete,
            suppressed_deletes,
        })
    }

    /// The directory every operation in this plan acts inside.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Every operation, in apply order.
    pub fn operations(&self) -> &[Operation] {
        &self.ops
    }

    /// The operations that cannot be taken back: the deletes.
    ///
    /// A subslice, not a filter — deletes are stored last, so this is the
    /// same fact as "deletes run last" read from the other end.
    ///
    /// **Renames are excluded because a rename here cannot destroy anything,
    /// and that is now enforced rather than argued.** The old argument was
    /// that a diff-time check plus a batch pre-flight covered it, and an
    /// adversarial review broke both: two renames whose targets the filesystem
    /// folds together passed diff and pre-flight alike, this method returned
    /// an empty slice, the confirmation therefore promised nothing
    /// irreversible, and a file was destroyed. What holds the claim up now is
    /// [`rename_no_replace`], which cannot replace an existing name at all —
    /// the kernel decides, against the filesystem's own folding rules. A
    /// rename that would land on something fails and is reported; the data
    /// stays where the reader last saw it.
    pub fn destructive(&self) -> &[Operation] {
        &self.ops[self.first_delete..]
    }

    /// How many rows the reader removed that this plan will *not* act on,
    /// because the listing was truncated.
    pub fn suppressed_deletes(&self) -> usize {
        self.suppressed_deletes
    }

    /// Whether this plan would do nothing at all.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// How many operations this plan holds.
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// The sentence a reader must accept before anything runs.
    ///
    /// Names every destructive operation (up to [`MAX_NAMED_DELETES`], then
    /// counts the rest) and counts the others, which is the whole of this
    /// phase's rollback story: there is no undo, so the last chance to
    /// notice is here.
    pub fn confirmation(&self) -> String {
        let deletes = self.destructive();
        let renames = self.ops[..self.first_delete]
            .iter()
            .filter(|op| matches!(op, Operation::Rename { .. }))
            .count();
        let creates = self.first_delete - renames;

        let mut parts = Vec::new();
        if renames > 0 {
            parts.push(count_of(renames, "rename"));
        }
        if creates > 0 {
            parts.push(count_of(creates, "create"));
        }
        if !deletes.is_empty() {
            let named: Vec<String> = deletes
                .iter()
                .take(MAX_NAMED_DELETES)
                .map(|op| display_name(op.target()).into_owned())
                .collect();
            let rest = deletes.len() - named.len();
            let tail = if rest > 0 {
                format!(" and {rest} more")
            } else {
                String::new()
            };
            parts.push(format!(
                "DELETE {} ({}{})",
                deletes.len(),
                named.join(", "),
                tail
            ));
        }

        let mut text = if parts.is_empty() {
            "apply nothing? [y/n]".to_string()
        } else {
            format!("apply {}? [y/n]", parts.join(", "))
        };
        if self.suppressed_deletes > 0 {
            text.push_str(&format!(
                "  ({} removed {} ignored: the listing was capped, so absence cannot mean removal)",
                self.suppressed_deletes,
                if self.suppressed_deletes == 1 {
                    "row"
                } else {
                    "rows"
                }
            ));
        }
        text
    }

    /// Carries the plan out, reporting each operation's own outcome.
    ///
    /// **The only function in this crate that destroys anything.** Its order
    /// is the plan's storage order and the argument for it is short: if a
    /// delete ran before a create and the create then failed — out of disk,
    /// the named edge case — the file would be gone and its replacement would
    /// not exist. In this order the disk-full create fails while everything it
    /// might have replaced is still there.
    ///
    /// ## Pre-flight
    ///
    /// The listed directory is checked to still *be* the directory the reader
    /// was looking at, and then every rename target and create target is
    /// `stat`ed. If anything is already there, **no operation runs at all**
    /// and every one of them comes back paired with the reason. That is
    /// DW-4.5's "refused before any operation runs", and it catches the
    /// collider that was never in the listing — a dotfile, or a name another
    /// process created since the read.
    ///
    /// One exception: a rename whose target differs from its source by case
    /// alone and which the filesystem folded onto the source itself. That is a
    /// case-only rename on a case-insensitive filesystem, which is macOS's
    /// default; without the exception `readme.md` → `README.md` would be
    /// impossible. [`is_case_only_rename`] is the test, and both of its halves
    /// are load-bearing.
    ///
    /// ## Failure policy
    ///
    /// Two phases, with different rules, for one reason:
    ///
    /// - A failure anywhere in the renames-and-creates **aborts the rest of
    ///   that phase and skips every delete**. Deletes are irreversible;
    ///   running the irreversible half of a plan whose reversible half did not
    ///   land is the worst available choice. Skipped operations report
    ///   `Err(… "not attempted …")` — accurate, since they did not succeed,
    ///   and unambiguous, since the message says which.
    /// - Deletes are attempted **independently of one another**. Each was
    ///   named and confirmed on its own, and one refused non-empty directory
    ///   is no reason to spare the rest.
    ///
    /// ## Races: what is closed, what is narrowed, what remains
    ///
    /// An earlier version of this section called the rename window
    /// theoretical, said `renameat2` was the only race-free fix, and claimed a
    /// delete's worst case was a failed remove. **All three were wrong**, and
    /// an adversarial review destroyed files on disk three ways to prove it:
    /// an in-process racer won 1296 of 3000 attempts at the rename window, and
    /// swapping a doomed name between `w` and `y` — a window a *human* sits
    /// inside, because the confirmation prompt is in it — got a file the
    /// reader had never seen deleted and reported as applied.
    ///
    /// - **Renames of files: closed, atomically, no `unsafe` and no new
    ///   dependency.** [`rename_no_replace`] creates the target with
    ///   `fs::hard_link`, which fails `EEXIST` in the kernel — including
    ///   against a name the filesystem folds onto an existing one, which is
    ///   what also closes the case and normalization collisions — and only
    ///   then unlinks the source.
    /// - **Renames of directories: narrowed to a syscall.** `link(2)` answers
    ///   `EPERM` for a directory, so there is no atomic primitive to reach
    ///   for; the target is re-`stat`ed immediately before `fs::rename`
    ///   instead. Deterministic against a collision the plan itself creates,
    ///   which is the demonstrated attack; a one-syscall window against
    ///   another process.
    /// - **Creates: closed.** `create_new` is `O_CREAT|O_EXCL`, atomic in the
    ///   kernel, and it refuses a symlink rather than following it.
    /// - **Deletes: the human-scale window is gone.** Each delete carries the
    ///   `(device, inode)` the directory read found under that name and
    ///   [`remove`] re-`stat`s and compares before unlinking, so a name that
    ///   has come to hold something else is refused outright. What is left is
    ///   the syscall between that `stat` and the `unlink`.
    /// - **The directory itself: narrowed to a syscall, per operation.**
    ///   Replacing the listed directory with a symlink carried every operation
    ///   out of it. [`EditPlan::verify_dir`] runs in the pre-flight *and*
    ///   before each individual operation, so the whole plan no longer rides
    ///   on one check taken at the start.
    pub fn apply(&self) -> Vec<(Operation, Result<(), io::Error>)> {
        if let Some((kind, reason)) = self.preflight() {
            return self
                .ops
                .iter()
                .map(|op| (op.clone(), Err(io::Error::new(kind, reason.clone()))))
                .collect();
        }

        let mut out = Vec::with_capacity(self.ops.len());
        let mut aborted = false;

        // The reversible half: stop at the first failure, and let that
        // failure travel into the loop below, where it skips every delete.
        for op in &self.ops[..self.first_delete] {
            if aborted {
                out.push((op.clone(), Err(not_attempted())));
                continue;
            }
            let result = self.run(op);
            aborted = result.is_err();
            out.push((op.clone(), result));
        }

        // The irreversible half: skipped entirely if anything above failed,
        // and otherwise attempted operation by operation, because each delete
        // was named and confirmed on its own.
        for op in &self.ops[self.first_delete..] {
            let result = if aborted {
                Err(not_attempted())
            } else {
                self.run(op)
            };
            out.push((op.clone(), result));
        }
        out
    }

    /// Carries out one operation, after proving the directory under it is
    /// still the one that was read.
    ///
    /// **The directory check is here rather than only in the pre-flight** so
    /// that the plan does not ride on a single observation taken before the
    /// first syscall: a swap that happens between operation three and
    /// operation four is caught before operation four, not after the plan
    /// finishes.
    ///
    /// **One function for all three kinds, rather than one per phase**, and
    /// that is a correction. The first cut had the rename/create loop answer
    /// "internal error: a delete was ordered before the creates" for a
    /// `Delete`, and the delete loop answer its mirror image — tripwires for
    /// an ordering that [`EditPlan::diff`] establishes by construction. They
    /// were unreachable, so no test could kill a mutation of either, and the
    /// ordering they guarded is already pinned where it is decided: by
    /// `test_dw_4_2_the_plan_orders_renames_and_creates_before_every_delete`
    /// on the plan's own layout, by the abort test on the phase boundary, and
    /// by both randomized suites. A tripwire nothing can trip is not a safety
    /// feature; it is code that has never run.
    fn run(&self, op: &Operation) -> io::Result<()> {
        self.verify_dir()?;
        match op {
            Operation::Rename { from, to } => rename_no_replace(from, to),
            Operation::Create { path } => create_exclusively(path),
            Operation::Delete { path, identity } => remove(path, *identity),
        }
    }

    /// `Ok(())` while the listed directory is still the directory the reader
    /// was shown.
    ///
    /// Replacing `dir` with a symlink between the diff and the apply pointed
    /// every path in the plan somewhere else — a delete of `dir/doomed.md`
    /// reported `Ok` while `elsewhere/doomed.md` stopped existing, which is
    /// the "no operation targets a path outside the listed directory"
    /// invariant failing under TOCTOU. Comparing the resolved identity is what
    /// refuses it. A plan built over a synthetic listing recorded nothing and
    /// is not checked; there is no directory under it to swap.
    fn verify_dir(&self) -> io::Result<()> {
        let Some(recorded) = self.dir_identity else {
            return Ok(());
        };
        if resolved_id(&self.dir) == Some(recorded) {
            return Ok(());
        }
        Err(io::Error::other(
            "the listed directory is not the one that was read",
        ))
    }

    /// `Some((kind, reason))` when the whole plan must be refused before a
    /// single operation runs — see [`EditPlan::apply`].
    fn preflight(&self) -> Option<(io::ErrorKind, String)> {
        if let Err(error) = self.verify_dir() {
            return Some((error.kind(), format!("{error} — no operation ran")));
        }
        for op in &self.ops {
            let target = match op {
                Operation::Rename { to, .. } => to,
                Operation::Create { path } => path,
                // A delete has no target to collide with: whatever is at the
                // path is exactly what it is there to remove. Whether it is
                // still the *right* thing is `remove`'s identity check, which
                // has to be taken next to the unlink to be worth anything.
                Operation::Delete { .. } => continue,
            };
            if fs::symlink_metadata(target).is_err() {
                continue;
            }
            if let Operation::Rename { from, .. } = op
                && is_case_only_rename(from, target).unwrap_or(false)
            {
                continue;
            }
            return Some((
                io::ErrorKind::AlreadyExists,
                format!("{} already exists — no operation ran", display_name(target)),
            ));
        }
        None
    }
}

/// The error an operation the plan never reached reports.
fn not_attempted() -> io::Error {
    io::Error::other("not attempted: an earlier operation failed")
}

/// Creates an empty file at `path`, and **never truncates** whatever might
/// already be there.
///
/// `create_new` is `O_CREAT|O_EXCL`: atomic in the kernel, so a file that
/// appears a nanosecond before this call is refused by the kernel rather than
/// by a check this code could lose a race to, and it fails on a symlink rather
/// than following it into a file the reader never named.
///
/// **A named function rather than three chained calls inside `run`**, and the
/// reason is a mutation that survived: replacing `.create_new(true)` with
/// `.create(true).truncate(true)` — which is exactly the "destroy a file the
/// reader only asked to add" defect — passed the entire suite, because the
/// only test aimed at it was satisfied by [`EditPlan::preflight`] and never
/// reached the open at all. A guarantee reachable only through a batch that
/// refuses first is a guarantee nothing can test. This is the seam
/// `test_create_exclusively_refuses_an_existing_file_and_keeps_its_bytes`
/// pushes on directly.
fn create_exclusively(path: &Path) -> io::Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(|_| ())
}

/// Moves `from` to `to`, and **never replaces anything that is already
/// there**.
///
/// `fs::rename` alone cannot promise that: on Unix it silently replaces its
/// destination. Neither can the batch pre-flight in [`EditPlan::apply`], and
/// two demonstrated failures say why.
///
/// **A fold no byte comparison can see.** `a.md` → `Target.md` and `b.md` →
/// `target.md` are two different byte strings, so the pre-flight finds neither
/// target on disk. On a case-folding filesystem the first rename *creates* the
/// second's target and the second replaces it: the directory is left holding
/// one file and the summary says two operations applied. Normalization is
/// worse — NFC `café.md` and NFD `café.md` are *visually identical*, so no
/// case fold catches them and a reader cannot see the collision even in
/// principle. [`EditPlan::diff`] cannot decide this: it is pure, and only the
/// filesystem knows which spellings it folds together. So the real guard is
/// here, against the disk.
///
/// **A window a pre-flight cannot close.** Between the pre-flight `stat` and
/// the rename another process can create the target. Measured, not
/// theoretical: an in-process racer creating the target with `O_EXCL` won the
/// name and was then overwritten in 1296 of 3000 attempts.
///
/// Both close for a **regular file**, with no `unsafe`, no `libc`, no
/// `renameat2` and no new dependency, because `link(2)` is *defined* to fail
/// `EEXIST` rather than replace — and the kernel applies that against the
/// filesystem's own folding rules, whatever they are, which is what makes it
/// cover the normalization case `to_lowercase` cannot reach. Link the new
/// name, then unlink the old one. If the unlink is the half that fails, the
/// file has two names and no bytes are lost; that is the right way round for
/// a failure to land, and [`unlink_source`] puts even that back.
///
/// **Everything that is not a regular file takes [`checked_rename`]** and its
/// residual one-syscall window: a directory, for which `link(2)` answers
/// `EPERM`, and a symlink, which POSIX leaves implementation-defined. Rust's
/// `fs::hard_link` uses `linkat(…, 0)` on every target this ships to, and a
/// direct measurement on macOS confirms it hard-links the *link* rather than
/// its target — but "measured on the machine I had" is not a guarantee worth
/// resting a symlink's identity on, and the checked path costs a symlink
/// nothing but a window it shares with directories.
fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    if !fs::symlink_metadata(from)?.is_file() {
        return checked_rename(from, to);
    }
    match classify_link(fs::hard_link(from, to)) {
        LinkOutcome::Linked => unlink_source(from, to),
        LinkOutcome::Taken(error) => finish_case_only_rename(from, to, error),
        LinkOutcome::Unavailable => checked_rename(from, to),
        LinkOutcome::Failed(error) => Err(error),
    }
}

/// What a `link(2)` outcome means for the rename that asked for it.
#[derive(Debug)]
enum LinkOutcome {
    /// The new name is in place; all that is left is to drop the old one.
    Linked,
    /// The kernel says the name is taken — the refusal this whole primitive
    /// exists to get. Only a fold onto the source itself is not a collision.
    Taken(io::Error),
    /// Hard links are not usable here. **Not** a collision, so refusing would
    /// reject a perfectly legal rename; fall back to the checked path.
    Unavailable,
    /// Anything else is the reader's problem and is reported as it came.
    Failed(io::Error),
}

/// Which of those four a `link(2)` result is.
///
/// **A pure function over the result rather than three guards inside
/// [`rename_no_replace`]'s `match`, and that is a fix.** As guards, all three
/// survived the mutation sweep: nothing in the suite can make a real
/// `hard_link` fail with anything but `EEXIST`, so `→ true` on either guard —
/// which silently routes *every* failure to the racy stat-then-rename path,
/// the one measured losing 178 of 181 contested rounds — was untestable
/// without fault injection. "Unreachable on this filesystem" is a claim about
/// reachability, and the cost of it being wrong is swapping the atomic
/// primitive for a racy one with nothing to say so.
///
/// Over a `Result` it needs no filesystem at all: a test hands it a synthetic
/// `EPERM`, `EMLINK` and `Unsupported` and watches them route to the fallback,
/// an `AlreadyExists` and watches it route to the collision path, and anything
/// else and watches it propagate. See
/// `test_classify_link_routes_every_link_outcome`.
///
/// Order matters and the test pins it: `AlreadyExists` is checked first, so a
/// collision can never be mistaken for "hard links are unavailable" and fall
/// through to a path that would replace the file.
fn classify_link(result: io::Result<()>) -> LinkOutcome {
    match result {
        Ok(()) => LinkOutcome::Linked,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => LinkOutcome::Taken(error),
        Err(error) if hard_links_unavailable(&error) => LinkOutcome::Unavailable,
        Err(error) => LinkOutcome::Failed(error),
    }
}

/// `EMLINK`, spelled the same on Linux and the BSDs. `io::ErrorKind` has no
/// name for it, so the raw number is the only way to ask.
const EMLINK: i32 = 31;

/// Whether `link(2)` failing means "not available here" rather than "that
/// name is taken".
fn hard_links_unavailable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
    ) || error.raw_os_error() == Some(EMLINK)
}

/// The second half of a linked rename: drop the old name.
///
/// On failure the new name is removed again. It is a second link to the very
/// same inode, so removing it destroys nothing, and it leaves the directory
/// exactly as the reader last saw it instead of holding one file under two
/// names they did not ask for.
///
/// **The rollback checks identity first**, because a rollback in the only
/// function that destroys anything is still a destroy. The window is narrow —
/// between a `link` this code just made and a `remove_file` that just failed —
/// but it is the same window every other finding in this module lived in, and
/// the answer is the same one: touch the inode we linked, or touch nothing. A
/// source that has itself been swapped leaves the new name in place, which is
/// a stray duplicate rather than a loss.
fn unlink_source(from: &Path, to: &Path) -> io::Result<()> {
    match fs::remove_file(from) {
        Ok(()) => Ok(()),
        Err(error) => {
            roll_back_link(from, to);
            Err(error)
        }
    }
}

/// Undoes a [`fs::hard_link`] whose unlink half failed — **only** if `to` is
/// still a second name for the very file `from` names.
///
/// A named function rather than four lines inside [`unlink_source`], for the
/// same reason [`create_exclusively`] is one: its caller can only be reached by
/// a `remove_file` that fails on a file this process just linked in a directory
/// it just wrote to, which no test can arrange without fault injection. Left
/// inline, the identity guard would be a guard nothing could exercise — the
/// F9 pattern exactly. Out here,
/// `test_roll_back_link_removes_only_a_second_name_for_the_same_file` drives
/// both branches directly.
///
/// Best-effort by construction: this runs while reporting somebody else's
/// failure, and a rollback that fails too has nothing useful to add.
fn roll_back_link(from: &Path, to: &Path) {
    let linked = link_id(to);
    if linked.is_some() && linked == link_id(from) {
        let _ = fs::remove_file(to);
    }
}

/// `fs::rename`, refused if anything is at `to` — for the sources `link(2)`
/// cannot serve.
///
/// The window between the `stat` and the `rename` is real and is one syscall
/// wide. Against the collision a plan makes *for itself* — two renames whose
/// targets the filesystem folds together — it is not a window at all: the
/// first rename has already committed its result to disk by the time the
/// second one looks, so the refusal is deterministic.
fn checked_rename(from: &Path, to: &Path) -> io::Result<()> {
    match fs::symlink_metadata(to) {
        Err(_) => fs::rename(from, to),
        Ok(_) => finish_case_only_rename(from, to, already_exists(to)),
    }
}

/// The one rename whose target may legitimately already exist: `readme.md` →
/// `README.md` where the filesystem folded the two spellings onto one entry,
/// so what "already exists" *is* the source. Anything else gets `refusal`.
fn finish_case_only_rename(from: &Path, to: &Path, refusal: io::Error) -> io::Result<()> {
    if is_case_only_rename(from, to)? {
        return fs::rename(from, to);
    }
    Err(refusal)
}

/// `{name} already exists`, as the refusal a caller hands to
/// [`finish_case_only_rename`].
fn already_exists(to: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!("{} already exists", display_name(to)),
    )
}

/// Whether `to` already existing is the harmless kind: the only difference
/// between the two names is case, **and** the directory holds no entry spelled
/// exactly `to` — so whatever `to` resolved to can only be `from` itself,
/// under a fold.
///
/// `from` and `to` are never the same name: [`EditPlan::diff`] treats a rename
/// to the entry's own name as no edit at all. So the first clause asks only
/// whether the difference is *case*.
///
/// **The second clause replaced an inode comparison**, and that is a fix
/// rather than a rewrite. The old test was "`to` resolves to the same file as
/// `from`", which is true of two hard links spelled `readme.md` and
/// `README.md` — and POSIX `rename` between two links to one file succeeds
/// and does nothing, so the plan reported "applied 1 operation" for a rename
/// that never happened. It was also untestable on the machine it shipped from:
/// a case-folding filesystem cannot hold two such names, so the one test
/// covering it returned early and a mutation replacing the whole comparison
/// with `true` survived the entire suite. Asking `readdir` for the byte-exact
/// spelling answers both problems — it distinguishes a fold from a hard-linked
/// pair, which the inode never could, and both of its branches are reachable
/// on every filesystem, which is what
/// `test_directory_holds_name_sees_only_the_byte_exact_spelling` exercises.
///
/// The scan is the one unbounded read in this module. It runs only for a
/// rename whose target already exists *and* differs by case alone, which is
/// rare, and no cheaper question distinguishes the two cases: `exists` answers
/// yes to both.
fn is_case_only_rename(from: &Path, to: &Path) -> io::Result<bool> {
    let (Some(before), Some(after)) = (
        from.file_name().and_then(OsStr::to_str),
        to.file_name().and_then(OsStr::to_str),
    ) else {
        return Ok(false);
    };
    if before.to_lowercase() != after.to_lowercase() {
        return Ok(false);
    }
    Ok(!directory_holds_name(to)?)
}

/// Whether `path`'s directory holds an entry spelled exactly like `path`'s
/// last component, byte for byte.
///
/// **Not `Path::exists`**, and the difference is the whole reason this is
/// here: on a folding filesystem `dir/README.md` exists whenever `dir/readme.md`
/// does. Only `readdir` reports the spelling actually stored.
fn directory_holds_name(path: &Path) -> io::Result<bool> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Ok(false);
    };
    for entry in fs::read_dir(parent)? {
        if entry?.file_name() == name {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Removes one entry, deciding *from the disk* rather than from the listing's
/// classification, which was taken earlier and followed symlinks.
///
/// **`recorded` is what closes the finding that a delete's worst case is a
/// failed remove.** It is not: between the reader pressing `w` and pressing
/// `y` — a window a human sits inside, because the confirmation prompt is in
/// it — the name can come to hold a file they never saw, and the old code
/// removed that one and reported "applied 1 operation". So the `(device,
/// inode)` the directory read found under this name is compared against what
/// is there now, and a disagreement refuses. That is not atomic; it turns a
/// human-scale window into the syscall between this `stat` and the unlink, and
/// it refuses the demonstrated attack outright.
///
/// A symlink is unlinked, never followed — deleting a row called `latest`
/// must not delete whatever it points at. That is also why the identity above
/// is taken with `symlink_metadata` at both ends: a symlink's own inode is
/// never its target's, so following at either end turns every symlink delete
/// into a refusal.
///
/// A directory goes through `remove_dir`, which the kernel refuses for a
/// non-empty one; that is DW-4.8's refusal, enforced race-free by the syscall
/// rather than by a `read_dir` this code would have to trust a moment later.
/// **Never `remove_dir_all`** — recursive delete is out of scope for this
/// phase, and its absence here is the enforcement.
fn remove(path: &Path, recorded: Option<FileId>) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if let Some(recorded) = recorded
        && file_id(&meta) != Some(recorded)
    {
        return Err(io::Error::other(
            "this is not the file that was listed under that name — nothing was removed",
        ));
    }
    if meta.file_type().is_symlink() || !meta.is_dir() {
        fs::remove_file(path)
    } else {
        fs::remove_dir(path)
    }
}

/// `count` of `noun`, pluralised.
fn count_of(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// One sentence saying what an apply actually did.
///
/// **Never reports success when anything failed**, which is the whole of
/// DW-4.7's status-row half: the count of what landed, the count of what was
/// attempted, and the first failure spelled out. A summary that said
/// "applied" and stopped would be the exact lie this phase must not tell.
pub fn apply_summary(results: &[(Operation, Result<(), io::Error>)]) -> String {
    let mut done = 0;
    let mut first_failure = None;
    for (op, result) in results {
        match result {
            Ok(()) => done += 1,
            Err(error) => {
                if first_failure.is_none() {
                    first_failure = Some(format!("{op}: {error}"));
                }
            }
        }
    }
    match first_failure {
        None => match done {
            0 => "nothing to write".to_string(),
            landed => format!("applied {}", count_of(landed, "operation")),
        },
        Some(reason) => format!(
            "applied {done} of {}, {} failed — {reason}",
            results.len(),
            results.len() - done
        ),
    }
}

/// `dir` joined with `name`, proven to be a direct child of `dir`.
///
/// The second half of the barricade, and it is not redundant with
/// [`validate_new_name`]: that one asks "is this a name?", this one asks "did
/// joining it land where we said it would?". A `.` that somehow slipped past
/// the first check joins to `dir/.`, whose `file_name` is `dir`'s own last
/// component rather than `.` — caught here. Nothing should ever reach
/// [`NameFault::Escapes`]; it exists so that if something does, it becomes a
/// refusal instead of a syscall.
fn child_path(dir: &Path, name: &str) -> Result<PathBuf, NameFault> {
    validate_new_name(name)?;
    let path = dir.join(name);
    if path.parent() != Some(dir) || path.file_name() != Some(OsStr::new(name)) {
        return Err(NameFault::Escapes);
    }
    Ok(path)
}

/// Whether `name`, typed by a reader, may become a filename in this
/// directory. See [`NameFault`] for what each refusal is protecting.
fn validate_new_name(name: &str) -> Result<(), NameFault> {
    if name.trim().is_empty() {
        return Err(NameFault::Blank);
    }
    if name.contains('\0') {
        return Err(NameFault::Nul);
    }
    if name.contains('\n') || name.contains('\r') {
        return Err(NameFault::Newline);
    }
    if name.chars().any(std::path::is_separator) {
        return Err(NameFault::Separator);
    }
    if name == "." || name == ".." {
        return Err(NameFault::Dot);
    }
    Ok(())
}

/// Whether a name that is *already on disk* is a plain child name.
///
/// Weaker than [`validate_new_name`] on purpose, and the difference is the
/// point: a file called `"   "` or `"a\nb"` is legal on Unix and may really
/// be sitting there. Refusing to *create* one is a kindness; refusing to
/// *list or delete* one would make it unreachable. What this does refuse is a
/// name that is not a single path component at all — empty, `.`, `..`, or
/// anything containing a separator — because such a name cannot come from
/// `readdir` and, joined to the directory, would point somewhere else
/// entirely.
///
/// Expressed through [`Path::components`] rather than a byte scan, which
/// makes the three refusals one question: a legal child name is exactly one
/// `Normal` component equal to itself.
/// Whether `left` and `right` are the same name as far as `equivalence` is
/// concerned.
///
/// The early, legible half of the collision rules — [`EditPlan::diff`] says
/// what the other half is and why this one cannot be the load-bearing one. A
/// name that is not UTF-8 has no case to fold, so it compares byte for byte;
/// that is not a gap, because [`EditPlan::diff`] refuses to rename such an
/// entry at all.
fn folds_together(equivalence: NameEquivalence, left: &OsStr, right: &str) -> bool {
    if left == OsStr::new(right) {
        return true;
    }
    match (equivalence, left.to_str()) {
        (NameEquivalence::Folded, Some(left)) => left.to_lowercase() == right.to_lowercase(),
        (NameEquivalence::Folded, None) | (NameEquivalence::Exact, _) => false,
    }
}

fn entry_name_ok(name: &OsStr) -> bool {
    let mut components = Path::new(name).components();
    let single = matches!(components.next(), Some(Component::Normal(only)) if only == name);
    single && components.next().is_none() && !name.as_encoded_bytes().contains(&0)
}

/// A [`Listing`] the reader can type into.
///
/// Holds one row per listed entry — including the `../` row, so a selection
/// index means the same thing before and after editing starts — plus whatever
/// rows the reader adds. The parent row is never editable, never removable,
/// and never emitted by [`EditBuffer::lines`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditBuffer {
    /// Copied from the listing, because it changes what a removed row means
    /// and the reader needs telling. See [`EditPlan::diff`].
    truncated: bool,
    rows: Vec<EditRow>,
}

/// One line of an [`EditBuffer`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct EditRow {
    /// Index into the listing's entries, or `None` for a row the reader
    /// added. This is the identity that travels in the line text.
    origin: Option<usize>,
    /// What the listing classified `origin` as; `None` for a new row. Only
    /// [`EntryKind::Parent`] is acted on — it is what makes a row
    /// uneditable.
    kind: Option<EntryKind>,
    /// The text this row was seeded with. Two jobs: `Esc` reverts to it, and
    /// the diff's "untouched" test is a comparison against it, which is what
    /// lets a non-UTF8 name be left alone safely despite showing lossily.
    seeded: String,
    text: String,
    removed: bool,
}

impl EditRow {
    /// The gutter this row paints in.
    fn marker(&self) -> &'static str {
        if self.removed {
            MARK_REMOVED
        } else if self.origin.is_none() {
            MARK_NEW
        } else if self.text == self.seeded {
            MARK_UNCHANGED
        } else {
            MARK_EDITED
        }
    }

    /// Whether the reader may type into or remove this row.
    fn editable(&self) -> bool {
        self.kind != Some(EntryKind::Parent)
    }
}

impl EditBuffer {
    /// One row per entry, seeded with exactly the text [`Listing::rows`]
    /// paints, so entering edit mode changes the gutter and nothing else.
    pub fn new(listing: &Listing) -> EditBuffer {
        EditBuffer {
            truncated: listing.truncated(),
            rows: listing
                .entries()
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    let seeded = row_text(entry);
                    EditRow {
                        origin: Some(index),
                        kind: Some(entry.kind),
                        text: seeded.clone(),
                        seeded,
                        removed: false,
                    }
                })
                .collect(),
        }
    }

    /// The buffer as [`EditPlan::diff`] reads it: `"{id}\t{text}"`, one line
    /// per surviving row.
    ///
    /// Three kinds of row are left out, and each omission is a decision.
    /// A **removed** row: its absence is what the diff turns into a delete.
    /// The **parent** row: `../` is not a child of this directory and cannot
    /// be renamed, created or deleted from here, so the diff must never see
    /// it — and must not read its absence as a delete either, which is why
    /// `diff` skips [`EntryKind::Parent`] when inferring. An **empty new**
    /// row: pressing `o` and then writing without typing anything is not a
    /// request to create a file called nothing. A *whitespace* new row is
    /// still emitted, and still refused by the diff, because that one is a
    /// mistake worth naming out loud.
    pub fn lines(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|row| row.editable() && !row.removed)
            .filter_map(|row| match row.origin {
                Some(id) => Some(format!("{id}\t{}", row.text)),
                None => (!row.text.is_empty()).then(|| format!("\t{}", row.text)),
            })
            .collect()
    }

    /// The rows this buffer paints into a viewport `height` rows tall,
    /// windowed by the very same [`overlay_window`] [`Listing::rows`] uses —
    /// literally the same code now, not merely the same shape — so the overlay
    /// cannot jump when editing begins.
    pub fn rows(&self, height: u16, selected: usize) -> Vec<OverlayRow> {
        let mut window = overlay_window(height, self.rows.len(), selected, self.notice());
        let selected = window.selected;
        let first = window.painted.start;
        window.rows.extend(
            self.rows[window.painted]
                .iter()
                .enumerate()
                .map(|(offset, row)| {
                    let index = first + offset;
                    // Dim beats the cursor for a removed row, the same way it
                    // does for an unopenable one in `Listing::rows`: reverse
                    // video on a row that is about to be deleted reads as
                    // "this is where you are", which is true, over "this is
                    // going away", which is what matters more.
                    let style = if row.removed {
                        RowStyle::Dimmed
                    } else if index == selected {
                        RowStyle::Selected
                    } else {
                        RowStyle::Ordinary
                    };
                    OverlayRow {
                        text: format!("{}{}", row.marker(), row.text),
                        style,
                    }
                }),
        );
        window.rows
    }

    /// The one row that precedes the entries when this buffer has something
    /// to say about itself.
    fn notice(&self) -> Option<String> {
        self.truncated.then(|| {
            "listing capped: names you never saw are missing, so removed rows will be ignored"
                .to_string()
        })
    }

    /// How many rows this buffer holds — the space `selected` indexes while
    /// editing.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether this buffer holds no rows at all.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Whether the reader has changed anything. A new row counts even while
    /// it is still empty — they pressed a key, and the indicator should say
    /// so.
    pub fn is_dirty(&self) -> bool {
        self.rows
            .iter()
            .any(|row| row.removed || row.origin.is_none() || row.text != row.seeded)
    }

    /// Renamed, added, removed.
    fn tally(&self) -> (usize, usize, usize) {
        let mut renamed = 0;
        let mut added = 0;
        let mut removed = 0;
        for row in &self.rows {
            if row.removed {
                removed += 1;
            } else if row.origin.is_none() {
                added += 1;
            } else if row.text != row.seeded {
                renamed += 1;
            }
        }
        (renamed, added, removed)
    }

    /// The dirty-buffer indicator for the status row.
    pub fn indicator(&self) -> String {
        let (renamed, added, removed) = self.tally();
        if !self.is_dirty() {
            return "[ ] editing — w writes, Esc discards".to_string();
        }
        format!("[+] {renamed} renamed, {added} new, {removed} removed — w writes, Esc discards")
    }

    /// Whether the reader may type into or remove row `row`.
    pub fn is_editable(&self, row: usize) -> bool {
        self.rows.get(row).is_some_and(EditRow::editable)
    }

    /// Row `row`'s current text, without its gutter.
    pub fn text(&self, row: usize) -> Option<&str> {
        self.rows.get(row).map(|row| row.text.as_str())
    }

    /// Row `row`, if it exists and the reader is allowed to change it.
    fn editable_mut(&mut self, row: usize) -> Option<&mut EditRow> {
        self.rows.get_mut(row).filter(|row| row.editable())
    }

    /// Appends one character to row `row`.
    pub fn push_char(&mut self, row: usize, ch: char) {
        if let Some(row) = self.editable_mut(row) {
            row.text.push(ch);
        }
    }

    /// Drops the last character of row `row`.
    pub fn pop_char(&mut self, row: usize) {
        if let Some(row) = self.editable_mut(row) {
            row.text.pop();
        }
    }

    /// Puts row `row` back to the text it was seeded with.
    pub fn revert(&mut self, row: usize) {
        if let Some(row) = self.editable_mut(row) {
            row.text = row.seeded.clone();
        }
    }

    /// Marks row `row` for removal, or unmarks it.
    pub fn toggle_removed(&mut self, row: usize) {
        if let Some(row) = self.editable_mut(row) {
            row.removed = !row.removed;
        }
    }

    /// Inserts an empty new row directly after `after` and answers where it
    /// landed.
    pub fn insert_new(&mut self, after: usize) -> usize {
        let at = after.saturating_add(1).min(self.rows.len());
        self.rows.insert(
            at,
            EditRow {
                origin: None,
                kind: None,
                seeded: String::new(),
                text: String::new(),
                removed: false,
            },
        );
        at
    }
}

#[cfg(test)]
mod tests {
    //! The four filesystem primitives, driven **directly**.
    //!
    //! `tests/explore_edit.rs` reaches them only through [`EditPlan::apply`],
    //! which pre-flights first — and a pre-flight that refuses is exactly what
    //! makes the last line of defence unreachable from a test. A mutation
    //! sweep proved it: `create_new` could be swapped for a truncating open
    //! and the whole suite stayed green, because the one test named for that
    //! guarantee never reached the `open`. These are the tests that fail when
    //! a primitive is weakened, and nothing else here can be.
    //!
    //! Every one of them runs on a folding filesystem and on a byte-exact one,
    //! which is the second half of the same lesson: the guard that shipped
    //! untested was untested precisely because its only test returned early
    //! on macOS.

    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fresh empty directory, the idiom the integration tests already use —
    /// `temp_dir` plus the process id, with a counter so two tests in one
    /// process cannot collide.
    fn scratch(tag: &str) -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "stele-explore-unit-{tag}-{}-{unique}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    /// **The F6 test.** Weakening `create_new` to a truncating open must fail
    /// here, so the assertion is on the *bytes*, not only on the error.
    #[test]
    fn test_create_exclusively_refuses_an_existing_file_and_keeps_its_bytes() {
        let dir = scratch("create-excl");
        let path = dir.join("already-here.md");
        fs::write(&path, "PRECIOUS\n").expect("write the existing file");

        let error = create_exclusively(&path).expect_err("an occupied name must be refused");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read_to_string(&path).expect("read back"),
            "PRECIOUS\n",
            "a create truncated a file it was only supposed to add"
        );
    }

    /// `O_EXCL` refuses a symlink rather than following it — otherwise a
    /// create of `latest` would open, and a truncating variant would empty,
    /// whatever `latest` points at.
    #[test]
    #[cfg(unix)]
    fn test_create_exclusively_refuses_a_symlink_rather_than_following_it() {
        let dir = scratch("create-symlink");
        let real = dir.join("real.md");
        let link = dir.join("link.md");
        fs::write(&real, "TARGET\n").expect("write target");
        std::os::unix::fs::symlink(&real, &link).expect("create symlink");

        assert!(create_exclusively(&link).is_err());
        assert_eq!(fs::read_to_string(&real).expect("read back"), "TARGET\n");
    }

    /// A create of a name nothing occupies still has to work.
    #[test]
    fn test_create_exclusively_makes_an_empty_file() {
        let dir = scratch("create-ok");
        let path = dir.join("new.md");
        create_exclusively(&path).expect("a free name must be creatable");
        assert_eq!(fs::read_to_string(&path).expect("read back"), "");
    }

    /// The predicate that replaced the untestable inode clause, on both
    /// branches — and the second assertion is the one `Path::exists` cannot
    /// make, because on a folding filesystem the flipped spelling exists too.
    #[test]
    fn test_directory_holds_name_sees_only_the_byte_exact_spelling() {
        let dir = scratch("holds-name");
        fs::write(dir.join("readme.md"), "R\n").expect("write readme");

        assert!(
            directory_holds_name(&dir.join("readme.md")).expect("scan"),
            "the spelling that is stored must be found"
        );
        assert!(
            !directory_holds_name(&dir.join("README.md")).expect("scan"),
            "a spelling that is not stored must not be, however the filesystem resolves it"
        );
        assert!(
            !directory_holds_name(&dir.join("absent.md")).expect("scan"),
            "a name nothing resolves to is not held either"
        );
    }

    /// The rename primitive's whole promise: an occupied target is refused and
    /// **both** files keep their bytes.
    #[test]
    fn test_rename_no_replace_refuses_an_occupied_target_and_keeps_both_files() {
        let dir = scratch("rename-occupied");
        let from = dir.join("source.md");
        let to = dir.join("victim.md");
        fs::write(&from, "SOURCE\n").expect("write source");
        fs::write(&to, "VICTIM\n").expect("write victim");

        let error = rename_no_replace(&from, &to).expect_err("an occupied target must be refused");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&from).expect("read source"), "SOURCE\n");
        assert_eq!(fs::read_to_string(&to).expect("read victim"), "VICTIM\n");
    }

    /// The same promise for a **directory** source, which takes the other
    /// branch entirely: `link(2)` answers `EPERM` for a directory, so this is
    /// [`checked_rename`] rather than the atomic path.
    #[test]
    fn test_rename_no_replace_refuses_an_occupied_target_for_a_directory() {
        let dir = scratch("rename-dir-occupied");
        let from = dir.join("source");
        let to = dir.join("victim");
        fs::create_dir(&from).expect("create source");
        fs::create_dir(&to).expect("create victim");
        fs::write(to.join("inside"), "INSIDE\n").expect("write inside victim");

        let error = rename_no_replace(&from, &to).expect_err("an occupied target must be refused");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert!(from.is_dir(), "the source directory must survive");
        assert_eq!(
            fs::read_to_string(to.join("inside")).expect("read inside"),
            "INSIDE\n"
        );
    }

    /// A free target renames, and the bytes travel.
    #[test]
    fn test_rename_no_replace_moves_a_file_to_a_free_name() {
        let dir = scratch("rename-free");
        let from = dir.join("before.md");
        let to = dir.join("after.md");
        fs::write(&from, "BYTES\n").expect("write source");

        rename_no_replace(&from, &to).expect("a free target must be renameable");
        assert!(!from.exists(), "the old name must be gone");
        assert_eq!(fs::read_to_string(&to).expect("read target"), "BYTES\n");
    }

    /// Renaming a symlink row moves the **link**, never its target — the
    /// property that decides whether the source takes the `link(2)` path or
    /// the checked one.
    #[test]
    #[cfg(unix)]
    fn test_rename_no_replace_keeps_a_symlink_a_symlink() {
        let dir = scratch("rename-symlink");
        fs::write(dir.join("real.md"), "TARGET\n").expect("write target");
        let from = dir.join("link.md");
        let to = dir.join("moved.md");
        std::os::unix::fs::symlink("real.md", &from).expect("create symlink");

        rename_no_replace(&from, &to).expect("a symlink must be renameable");
        let meta = fs::symlink_metadata(&to).expect("stat the moved link");
        assert!(
            meta.file_type().is_symlink(),
            "a renamed symlink must still be a symlink, not a hard link to its target"
        );
        assert_eq!(
            fs::read_link(&to).expect("read link"),
            PathBuf::from("real.md")
        );
        assert_eq!(
            fs::read_to_string(dir.join("real.md")).expect("read target"),
            "TARGET\n"
        );
    }

    /// The rollback in the linked-rename path, both branches. A rollback
    /// inside the only function that destroys anything is still a destroy, and
    /// the second assertion is the one that says so.
    #[test]
    fn test_roll_back_link_removes_only_a_second_name_for_the_same_file() {
        let dir = scratch("rollback");
        let from = dir.join("source.md");
        let linked = dir.join("linked.md");
        fs::write(&from, "SOURCE\n").expect("write source");
        fs::hard_link(&from, &linked).expect("link");

        roll_back_link(&from, &linked);
        assert!(!linked.exists(), "our own second name must be dropped");
        assert_eq!(
            fs::read_to_string(&from).expect("read source"),
            "SOURCE\n",
            "and the file itself must survive"
        );

        // A name that is *not* our link — what a swap in the window between
        // the `link` and the failed unlink would leave behind.
        let stranger = dir.join("stranger.md");
        fs::write(&stranger, "NOT OURS\n").expect("write stranger");
        roll_back_link(&from, &stranger);
        assert_eq!(
            fs::read_to_string(&stranger).expect("read stranger"),
            "NOT OURS\n",
            "the rollback removed a file it never created"
        );

        // And a name that is not there at all is not an error.
        roll_back_link(&from, &dir.join("absent.md"));
    }

    /// Every arm of the `link(2)` routing, on any filesystem, with no fault
    /// injection — the three guards that survived the sweep as guards.
    ///
    /// The `Unavailable` cases are the dangerous direction: each of them sends
    /// the rename to the stat-then-rename path, so an error wrongly classified
    /// there swaps the atomic primitive for the racy one silently.
    #[test]
    fn test_classify_link_routes_every_link_outcome() {
        assert!(matches!(classify_link(Ok(())), LinkOutcome::Linked));

        // The refusal the whole primitive exists to get.
        let taken = io::Error::new(io::ErrorKind::AlreadyExists, "File exists");
        assert!(matches!(classify_link(Err(taken)), LinkOutcome::Taken(_)));

        // Not collisions: hard links simply are not usable for this file.
        for unusable in [
            io::Error::new(io::ErrorKind::PermissionDenied, "protected_hardlinks"),
            io::Error::new(io::ErrorKind::Unsupported, "no hard links here"),
            io::Error::from_raw_os_error(EMLINK),
        ] {
            let described = format!("{unusable:?}");
            assert!(
                matches!(classify_link(Err(unusable)), LinkOutcome::Unavailable),
                "{described} must fall back, not refuse a legal rename"
            );
        }

        // Everything else is reported as it came, never quietly downgraded to
        // the racy path.
        for propagated in [
            io::Error::new(io::ErrorKind::NotFound, "gone"),
            io::Error::other("something nobody anticipated"),
        ] {
            let described = format!("{propagated:?}");
            assert!(
                matches!(classify_link(Err(propagated)), LinkOutcome::Failed(_)),
                "{described} must propagate rather than fall back"
            );
        }
    }

    /// The order of the two guards, pinned on its own: a collision must never
    /// be read as "hard links are unavailable" and sent to a path that would
    /// replace the file.
    ///
    /// `PermissionDenied` is what `hard_links_unavailable` accepts, so an
    /// `AlreadyExists` carrying that raw code is the input that tells the two
    /// checks apart.
    #[test]
    fn test_classify_link_checks_for_a_collision_before_anything_else() {
        let both = io::Error::new(io::ErrorKind::AlreadyExists, "File exists");
        assert!(hard_links_unavailable(&io::Error::new(
            io::ErrorKind::PermissionDenied,
            "x"
        )));
        assert!(
            matches!(classify_link(Err(both)), LinkOutcome::Taken(_)),
            "a taken name must win over the unavailability check"
        );
    }

    /// The windowing both overlays now share, driven directly.
    ///
    /// Each assertion here is aimed at one operator that survived the sweep
    /// inside `EditBuffer`'s former copy of this arithmetic.
    #[test]
    fn test_overlay_window_arithmetic() {
        let notice = || Some("notice".to_string());

        // `height - rows.len()`: a notice row in a one-row viewport leaves no
        // budget at all, so the notice is the whole frame. With `+` there
        // would be room for entries.
        let window = overlay_window(1, 10, 0, notice());
        assert_eq!(window.rows.len(), 1);
        assert!(window.painted.is_empty(), "{:?}", window.painted);

        // `budget == 0 || len == 0`: an empty list carrying a notice has a
        // nonzero budget, and only the emptiness half stops the clamp below
        // subtracting one from zero.
        let window = overlay_window(10, 0, 0, notice());
        assert_eq!(window.rows.len(), 1);
        assert!(window.painted.is_empty());

        // `window / 2`: the selection is centred, not merely visible. With
        // `%` a window of six would put `first` at `selected` itself.
        let window = overlay_window(6, 20, 10, None);
        assert_eq!(window.painted, 7..13, "the cursor must sit mid-window");
        assert_eq!(window.selected, 10);

        // Clamped, not validated: an index past the end lands on the last row.
        let window = overlay_window(4, 3, 99, None);
        assert_eq!(window.selected, 2);
        assert_eq!(window.painted, 0..3);

        // A zero-row viewport paints nothing at all, notice or not.
        assert!(overlay_window(0, 5, 0, notice()).rows.is_empty());

        // And the window never runs off the end of the list.
        let window = overlay_window(4, 10, 9, None);
        assert_eq!(window.painted, 6..10);
    }

    /// **The F3 primitive.** A name that has come to hold a different file is
    /// refused, and the impostor survives untouched — a delete that removed it
    /// would be deleting a file the reader never saw.
    #[test]
    fn test_remove_refuses_a_name_that_no_longer_holds_the_recorded_file() {
        let dir = scratch("remove-identity");
        let path = dir.join("doomed.md");
        fs::write(&path, "ORIGINAL\n").expect("write original");
        let recorded = link_id(&path).expect("record an identity");

        fs::remove_file(&path).expect("remove original");
        fs::write(&path, "IMPOSTOR\n").expect("write impostor");

        remove(&path, Some(recorded)).expect_err("a swapped name must be refused");
        assert_eq!(
            fs::read_to_string(&path).expect("read back"),
            "IMPOSTOR\n",
            "a file that was never listed was removed"
        );
    }

    /// And the ordinary case still removes: the identity check must refuse the
    /// impostor without also refusing the file it was recorded from.
    #[test]
    fn test_remove_takes_the_file_whose_identity_was_recorded() {
        let dir = scratch("remove-ok");
        let path = dir.join("doomed.md");
        fs::write(&path, "GONE\n").expect("write doomed");
        let recorded = link_id(&path).expect("record an identity");

        remove(&path, Some(recorded)).expect("the recorded file must be removable");
        assert!(!path.exists());
    }

    /// A symlink's identity is the link's own, so a symlink delete has to pass
    /// the check *and* unlink the link rather than its target. This is the
    /// test that fails if `remove` ever resolves the path it stats.
    #[test]
    #[cfg(unix)]
    fn test_remove_unlinks_a_symlink_and_never_its_target() {
        let dir = scratch("remove-symlink");
        let real = dir.join("real.md");
        let link = dir.join("link.md");
        fs::write(&real, "TARGET\n").expect("write target");
        std::os::unix::fs::symlink(&real, &link).expect("create symlink");
        let recorded = link_id(&link).expect("record the link's own identity");

        remove(&link, Some(recorded)).expect("a symlink row must be removable");
        assert!(!link.exists(), "the link must be gone");
        assert_eq!(
            fs::read_to_string(&real).expect("read target"),
            "TARGET\n",
            "the target must survive"
        );
    }

    /// The case probe against ground truth: whatever it answers, the
    /// filesystem must agree, measured by writing one file and asking whether
    /// the flipped spelling reaches it.
    ///
    /// Not an assertion about macOS or Linux — an assertion that the probe and
    /// the disk cannot disagree, which is the only claim that means anything
    /// on a machine that has both kinds of filesystem mounted.
    #[test]
    fn test_probe_equivalence_agrees_with_the_filesystem() {
        let dir = scratch("probe-truth");
        fs::write(dir.join("probe.md"), "P\n").expect("write probe file");
        let folds_in_fact = fs::symlink_metadata(dir.join("PROBE.MD")).is_ok();

        let listing = Listing::read(&dir);
        let expected = if folds_in_fact {
            NameEquivalence::Folded
        } else {
            NameEquivalence::Exact
        };
        assert_eq!(listing.equivalence(), expected);
    }

    /// The probe's `Exact` branch, which a case-folding filesystem cannot
    /// otherwise produce — and *not* reaching it was a real gap: with the disk
    /// as the only input, "always answer `Folded`" is indistinguishable from a
    /// working probe on this machine, so a revert of the whole thing survived
    /// [`probe_equivalence`]'s own test.
    ///
    /// Driving [`probe_one`] directly closes it without a case-sensitive
    /// volume to mount: hand it an identity that is *not* what the flipped
    /// spelling resolves to, which is exactly the shape a case-sensitive
    /// filesystem presents, and the answer must be `Exact`.
    #[test]
    fn test_probe_one_answers_exact_when_the_flipped_spelling_is_another_file() {
        let dir = scratch("probe-exact");
        fs::write(dir.join("a.md"), "A\n").expect("write a");
        fs::write(dir.join("other.md"), "O\n").expect("write other");
        let other = link_id(&dir.join("other.md")).expect("identity of the other file");

        assert_eq!(
            probe_one(&dir.join("a.md"), Some(other), link_id),
            Some(NameEquivalence::Exact),
            "the flipped spelling reaches a.md, which is not the recorded file"
        );
        let a = link_id(&dir.join("a.md")).expect("identity of a");
        let folds_in_fact = fs::symlink_metadata(dir.join("A.MD")).is_ok();
        assert_eq!(
            probe_one(&dir.join("a.md"), Some(a), link_id),
            Some(if folds_in_fact {
                NameEquivalence::Folded
            } else {
                NameEquivalence::Exact
            })
        );
        assert_eq!(
            probe_one(&dir.join("a.md"), None, link_id),
            None,
            "nothing recorded is not an answer"
        );
    }

    /// An entry decides, and the directory is only the fallback — because a
    /// name is stored by the directory that holds it, so `dir`'s own name
    /// measures `dir`'s *parent*, which at a mount point is a different
    /// filesystem.
    ///
    /// Pinned by making the two sources disagree: the entry carries a
    /// deliberately wrong identity, so it answers `Exact` whatever this
    /// filesystem is, while `dir` answers honestly. Which one comes out *is*
    /// the ordering.
    #[test]
    fn test_probe_equivalence_asks_an_entry_before_the_directory_itself() {
        let dir = scratch("probe-order");
        fs::write(dir.join("a.md"), "A\n").expect("write a");
        fs::write(dir.join("other.md"), "O\n").expect("write other");
        let dir_identity = resolved_id(&dir).expect("directory identity");
        let wrong = link_id(&dir.join("other.md")).expect("a deliberately wrong identity");
        let entries = vec![Entry {
            name: OsString::from("a.md"),
            path: dir.join("a.md"),
            kind: EntryKind::Document,
        }];

        assert_eq!(
            probe_equivalence(&dir, Some(dir_identity), &entries, &[Some(wrong)]),
            NameEquivalence::Exact,
            "the entry's answer must win over the directory's"
        );
        let from_dir = probe_one(&dir, Some(dir_identity), resolved_id)
            .expect("the scratch directory's name has letters in it");
        assert_eq!(
            probe_equivalence(&dir, Some(dir_identity), &entries, &[None]),
            from_dir,
            "an entry with nothing recorded cannot decide, so the directory does"
        );
    }

    /// The directory fallback itself, reached with an empty listing — and
    /// again with a deliberately wrong identity, which is the only way to
    /// observe its `Exact` branch on a filesystem that folds everything.
    #[test]
    fn test_probe_equivalence_falls_back_to_the_directory_for_an_empty_listing() {
        let dir = scratch("probe-empty");
        fs::write(dir.join("other.md"), "O\n").expect("write other");
        let wrong = link_id(&dir.join("other.md")).expect("a deliberately wrong identity");

        assert_eq!(
            probe_equivalence(&dir, Some(wrong), &[], &[]),
            NameEquivalence::Exact,
            "the flipped directory name does not reach the recorded file"
        );
        let dir_identity = resolved_id(&dir).expect("directory identity");
        assert_eq!(
            probe_equivalence(&dir, Some(dir_identity), &[], &[]),
            probe_one(&dir, Some(dir_identity), resolved_id).expect("decidable"),
        );
    }

    /// Nothing to probe at all folds, rather than guessing the permissive
    /// answer.
    #[test]
    fn test_probe_equivalence_folds_when_nothing_can_be_probed() {
        let dir = scratch("probe-undecidable");
        assert_eq!(
            probe_equivalence(&dir, None, &[], &[]),
            NameEquivalence::Folded
        );
    }

    /// A name with no cased letter cannot be probed, and the undecidable
    /// answer folds — the conservative direction, because guessing `Exact`
    /// would wave a collision through.
    #[test]
    fn test_case_flipped_declines_a_name_with_no_letters() {
        assert_eq!(case_flipped("readme"), Some("README".to_string()));
        assert_eq!(case_flipped("README"), Some("readme".to_string()));
        assert_eq!(case_flipped("2026-01-02"), None);
        assert_eq!(case_flipped(""), None);
    }

    /// Folding is the filesystem's question, so the predicate must answer it
    /// differently for the two kinds — and must never fold a name that is not
    /// text, which has no case to fold.
    #[test]
    fn test_folds_together_follows_the_equivalence_it_is_given() {
        let exact = NameEquivalence::Exact;
        let folded = NameEquivalence::Folded;
        assert!(folds_together(exact, OsStr::new("a.md"), "a.md"));
        assert!(!folds_together(exact, OsStr::new("A.md"), "a.md"));
        assert!(folds_together(folded, OsStr::new("A.md"), "a.md"));
        assert!(!folds_together(folded, OsStr::new("b.md"), "a.md"));
        // NFC against NFD: one name on APFS, and neither equivalence can say
        // so without a Unicode table. `EditPlan::diff` records why, and
        // `rename_no_replace` is what actually catches it.
        assert!(!folds_together(
            folded,
            OsStr::new("caf\u{e9}.md"),
            "cafe\u{301}.md"
        ));
    }

    /// `hard_links_unavailable` must say "no" to the one answer that means a
    /// collision, or a taken name would fall through to the weaker path.
    #[test]
    fn test_hard_links_unavailable_never_swallows_a_collision() {
        let taken = io::Error::new(io::ErrorKind::AlreadyExists, "exists");
        assert!(!hard_links_unavailable(&taken));
        assert!(!hard_links_unavailable(&io::Error::other("something else")));
        assert!(hard_links_unavailable(&io::Error::new(
            io::ErrorKind::PermissionDenied,
            "protected_hardlinks"
        )));
        assert!(hard_links_unavailable(&io::Error::new(
            io::ErrorKind::Unsupported,
            "no hard links here"
        )));
        assert!(hard_links_unavailable(&io::Error::from_raw_os_error(
            EMLINK
        )));
    }
}
