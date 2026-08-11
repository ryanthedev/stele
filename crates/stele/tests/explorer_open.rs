//! DW-1.2 and DW-1.5 at the level the explorer will actually use: a path
//! handed to [`Navigator::open_path`] from outside the crate, with no href and
//! no document author anywhere in the story.
//!
//! Two things are proved here that a unit test inside `link.rs` cannot say as
//! well. The first is **liveness**: every refusal of a non-regular file runs
//! on its own thread behind a `recv_timeout`, so a barricade that opened
//! before it looked fails this file in five seconds instead of wedging the
//! suite. `open(2)` on a FIFO with no writer blocks in the kernel and a read of
//! a character device never ends — under raw mode, where Ctrl-C is an ordinary
//! keystroke, either one is an unkillable viewer, so "it returns at all" is the
//! claim, not a detail of how. The second is the **round trip**: the size
//! ceiling a document was admitted under has to survive being pushed and popped
//! by a caller that only ever sees the public API.
//!
//! Fixtures are hand-rolled from `std::env::temp_dir()` and `process::id()`,
//! the idiom at `link_interaction.rs:47-68` — there is no `tempfile` in this
//! workspace and adding one is out of scope.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use stele::link::{
    Followed, LinkError, MAX_LINK_FILE_BYTES, MAX_STACK_DEPTH, Navigator, UrlOpener,
};
use stele::loader::{DocumentSource, LoadOptions, MAX_DOCUMENT_BYTES};

/// Long enough that a slow machine is not mistaken for a hang, short enough
/// that a real hang is reported rather than waited out.
const WATCHDOG: Duration = Duration::from_secs(5);

/// A [`UrlOpener`] that fails the test if it is ever reached.
///
/// `open_path` has no URL branch at all, so this is not a stub standing in for
/// behaviour — it is the assertion that no such branch grows later.
#[derive(Debug, Default)]
struct NeverOpens;

impl UrlOpener for NeverOpens {
    fn open(&mut self, url: &str) -> Result<(), LinkError> {
        panic!("open_path must never reach the URL opener, but it was handed {url:?}");
    }
}

fn navigator() -> Navigator {
    Navigator::new(Box::new(NeverOpens), LoadOptions::default())
}

/// A scratch directory unique to `tag`, so concurrently running tests cannot
/// see each other's fixtures.
fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("stele-open-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("write fixture");
    path
}

/// What `open_path` answered for `path`, or a failure if it did not answer
/// within [`WATCHDOG`].
///
/// The call runs on its own thread for one reason: a blocking `open(2)` cannot
/// be interrupted, so the only way to *assert* that it did not happen is to
/// outlive it. libtest exits the process on failure rather than joining, so a
/// genuinely wedged thread does not take the suite with it.
fn answer_within_the_watchdog(path: PathBuf, current: DocumentSource) -> String {
    let (tx, rx) = mpsc::channel();
    let named = path.display().to_string();
    std::thread::spawn(move || {
        let mut nav = navigator();
        let outcome = nav.open_path(&path, &current, 0);
        let _ = tx.send(match outcome {
            Ok(Followed::Opened { .. }) => "opened".to_string(),
            Ok(Followed::Handed) => "handed".to_string(),
            Err(err) => format!("{err:?}"),
        });
    });
    rx.recv_timeout(WATCHDOG).unwrap_or_else(|_| {
        panic!(
            "open_path on {named} did not return within {WATCHDOG:?} — the refusal happened \
             after open(2), which is the defect this test exists for"
        )
    })
}

// -------------------------------------------------------------------- DW-1.2

/// Every non-regular file the explorer can list, refused **by type**, in
/// bounded time, through the new entry point.
///
/// The symlinks are half the point: `canonicalize` follows them, so a symlink
/// to a FIFO must be refused as the FIFO it resolves to rather than admitted
/// as the harmless-looking link it is spelled as. A containment check would
/// have refused these for the wrong reason; there is none, so the type check
/// is the only thing standing between a keystroke and a blocked `open`.
#[test]
#[cfg(unix)]
fn test_dw_1_2_a_non_regular_file_is_refused_by_type_in_bounded_time() {
    use std::os::unix::fs::FileTypeExt as _;
    use std::os::unix::net::UnixListener;

    let dir = scratch_dir("nonregular");
    let current = DocumentSource::Path(write(&dir, "index.md", "# Index\n"));

    let fifo = dir.join("fifo");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("run mkfifo");
    assert!(made.success(), "mkfifo failed: {made:?}");
    assert!(
        std::fs::symlink_metadata(&fifo)
            .expect("stat the fifo")
            .file_type()
            .is_fifo(),
        "the fixture must really be a FIFO for this test to mean anything"
    );

    // Bound to a short name: `sun_path` is 104 bytes on macOS and the scratch
    // directory under `/var/folders/...` already spends most of it.
    let sock = dir.join("s");
    let listener = UnixListener::bind(&sock).expect("bind a unix socket");
    assert!(
        std::fs::symlink_metadata(&sock)
            .expect("stat the socket")
            .file_type()
            .is_socket(),
        "the fixture must really be a socket"
    );

    let subdir = dir.join("subdir");
    std::fs::create_dir(&subdir).expect("create a subdirectory");

    // A character device that exists on every unix, and the reason a read
    // cannot be the type test: `/dev/zero` returns bytes forever.
    let chardev = PathBuf::from("/dev/zero");

    let mut cases = vec![
        ("a FIFO with no writer", fifo.clone()),
        ("a unix socket", sock.clone()),
        ("a directory", subdir.clone()),
        ("a character device", chardev.clone()),
    ];
    for (what, target) in [
        ("a symlink to a FIFO", fifo),
        ("a symlink to a socket", sock),
        ("a symlink to a directory", subdir),
        ("a symlink to a character device", chardev),
    ] {
        let link = dir.join(what.replace(' ', "-"));
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");
        cases.push((what, link));
    }

    for (what, path) in cases {
        let answer = answer_within_the_watchdog(path, current.clone());
        assert!(
            answer.starts_with("NotAFile"),
            "{what} must be refused by type, got {answer}"
        );
    }

    drop(listener);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The refusal must also leave the reader where they were — a hostile entry in
/// a listing cannot cost them their history.
#[test]
#[cfg(unix)]
fn test_dw_1_2_a_refused_non_regular_file_leaves_the_stack_and_the_document_alone() {
    let dir = scratch_dir("nonregular-stack");
    let root = write(&dir, "root.md", "# Root\n");
    let second = write(&dir, "second.md", "# Second\n");
    let subdir = dir.join("subdir");
    std::fs::create_dir(&subdir).expect("create a subdirectory");

    let mut nav = navigator();
    nav.open_path(&second, &DocumentSource::Path(root.clone()), 6)
        .expect("one good hop");
    assert_eq!(nav.depth(), 1);

    let err = nav
        .open_path(&subdir, &DocumentSource::Path(second), 13)
        .expect_err("a directory must be refused");
    assert!(matches!(err, LinkError::NotAFile(_)), "{err:?}");
    assert!(err.to_string().starts_with("not a regular file:"));
    assert_eq!(nav.depth(), 1, "the refusal disturbed the stack");

    let (source, _, scroll) = nav.back().expect("the good hop is still poppable");
    assert_eq!(source, DocumentSource::Path(root));
    assert_eq!(scroll, 6);
    let _ = std::fs::remove_dir_all(&dir);
}

// -------------------------------------------------------------------- DW-1.5

/// The round trip, not just the push.
///
/// A document between the two ceilings, opened by path at depth 1 and then
/// left for another, must come back **intact** — same bytes, same scroll
/// offset. Before `DocumentStack::push` carried the ceiling its entry was
/// admitted under, this failed at the last step: the entry was recorded as
/// link-provenance because it sat above depth 0, and `back()` refused the
/// reader a document they had opened themselves a moment earlier.
#[test]
fn test_dw_1_5_a_large_document_opened_by_path_at_depth_survives_the_back_round_trip() {
    let dir = scratch_dir("round-trip");
    let root = write(&dir, "root.md", "# Root\n");
    write(&dir, "second.md", "# Second\n");
    let third = write(&dir, "third.md", "# Third\n");

    // Real text, not a sparse hole: a hole is all NULs and would be refused by
    // the binary sniff instead, which would make the size gate untested.
    let body = "lorem ipsum dolor sit amet\n".repeat(400_000);
    let size = body.len() as u64;
    assert!(
        size > MAX_LINK_FILE_BYTES && size <= MAX_DOCUMENT_BYTES,
        "the fixture must sit strictly between the ceilings for this to say anything"
    );
    let big = dir.join("big.md");
    std::fs::write(&big, &body).expect("write a large fixture");

    let mut nav = navigator();
    // root -> second by link, so the path-open below happens at depth 1 and
    // the old depth derivation would get it wrong.
    nav.follow("second.md", &DocumentSource::Path(root), 5)
        .expect("root -> second");
    let second = DocumentSource::Path(dir.join("second.md"));

    match nav
        .open_path(&big, &second, 7)
        .expect("a document under the command-line ceiling opens by path")
    {
        Followed::Opened { source, loaded } => {
            assert!(source.display_name().ends_with("big.md"));
            assert_eq!(loaded.info.byte_size, size);
        }
        Followed::Handed => panic!("open_path must never hand a path to an opener"),
    }
    assert_eq!(nav.depth(), 2);

    // Move off it, which is what pushes it.
    let big_source = DocumentSource::Path(dir.join("big.md"));
    nav.open_path(&third, &big_source, 11)
        .expect("big -> third");
    assert_eq!(nav.depth(), 3);

    let (source, loaded, scroll) = nav
        .back()
        .expect("a document the reader opened themselves must still be reachable");
    assert!(source.display_name().ends_with("big.md"));
    assert_eq!(scroll, 11, "the scroll offset must survive the round trip");
    assert_eq!(
        loaded.info.byte_size, size,
        "the document must come back intact, not truncated to a smaller ceiling"
    );

    // And the rest of the history is still in order underneath it.
    let (source, _, scroll) = nav.back().expect("back to second");
    assert!(source.display_name().ends_with("second.md"));
    assert_eq!(scroll, 7);
    let (source, _, scroll) = nav.back().expect("back to root");
    assert!(source.display_name().ends_with("root.md"));
    assert_eq!(scroll, 5);
    assert!(matches!(nav.back(), Err(LinkError::AtRoot)));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The mirror case, so the fix is a fix and not a blanket loosening: a
/// document reached by a **link** keeps the tighter link ceiling on the way
/// back even after an `open_path` hop has used the wider one.
#[test]
fn test_a_link_opened_document_keeps_the_link_ceiling_after_an_open_path_hop() {
    let dir = scratch_dir("mirror-ceiling");
    let root = write(&dir, "root.md", "# Root\n");
    write(&dir, "mid.md", "# Mid\n");
    let leaf = write(&dir, "leaf.md", "# Leaf\n");

    let mut nav = navigator();
    nav.follow("mid.md", &DocumentSource::Path(root), 0)
        .expect("root -> mid");
    let mid = DocumentSource::Path(dir.join("mid.md"));
    nav.open_path(&leaf, &mid, 3).expect("mid -> leaf");
    assert_eq!(nav.depth(), 2);

    // `mid.md` arrived by link, so it is worth 8 MiB. Grow it past that and
    // the way back must refuse — if `open_path` had raised the ceiling for
    // everything, this would silently open.
    let file = std::fs::File::create(dir.join("mid.md")).expect("recreate mid");
    file.set_len(MAX_LINK_FILE_BYTES + 1).expect("set_len");
    drop(file);

    let err = nav
        .back()
        .expect_err("a link-provenance document keeps the link ceiling");
    assert!(
        matches!(err, LinkError::TooLarge { limit } if limit == MAX_LINK_FILE_BYTES),
        "{err:?}"
    );
    // A failed reload puts the entry back rather than swallowing it.
    assert_eq!(nav.depth(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// At the ceiling the stack **refuses** rather than evicting, and the oldest
/// entry is still there afterwards to prove nothing was dropped.
#[test]
fn test_dw_1_5_open_path_at_the_stack_ceiling_refuses_instead_of_evicting() {
    let dir = scratch_dir("stack-ceiling");
    let doc = write(&dir, "self.md", "# Self\n");
    let source = DocumentSource::Path(doc.clone());

    let mut nav = navigator();
    for depth in 0..MAX_STACK_DEPTH {
        nav.open_path(&doc, &source, depth)
            .expect("within the ceiling");
    }
    assert_eq!(nav.depth(), MAX_STACK_DEPTH);

    let err = nav
        .open_path(&doc, &source, 999)
        .expect_err("the ceiling must refuse");
    assert!(
        matches!(err, LinkError::StackTooDeep { limit } if limit == MAX_STACK_DEPTH),
        "{err:?}"
    );
    assert_eq!(
        nav.depth(),
        MAX_STACK_DEPTH,
        "the refusal must not have evicted anything"
    );

    // Every entry, oldest included, is still poppable in order — which is the
    // claim "refuses rather than evicts" actually makes.
    for depth in (0..MAX_STACK_DEPTH).rev() {
        let (_, _, scroll) = nav.back().expect("each entry survived");
        assert_eq!(scroll, depth);
    }
    assert!(matches!(nav.back(), Err(LinkError::AtRoot)));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The bound is checked **before** any I/O, so a full stack costs a `stat` at
/// most — asserted by refusing at the ceiling with a path that would otherwise
/// block forever.
#[test]
#[cfg(unix)]
fn test_a_full_stack_refuses_before_it_touches_the_path_at_all() {
    let dir = scratch_dir("ceiling-before-io");
    let doc = write(&dir, "self.md", "# Self\n");
    let source = DocumentSource::Path(doc.clone());

    let mut nav = navigator();
    for _ in 0..MAX_STACK_DEPTH {
        nav.open_path(&doc, &source, 0).expect("within the ceiling");
    }

    let err = nav
        .open_path(Path::new("/dev/zero"), &source, 0)
        .expect_err("the ceiling must refuse");
    assert!(
        matches!(err, LinkError::StackTooDeep { .. }),
        "the depth check must come first, so the path is never examined; got {err:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
