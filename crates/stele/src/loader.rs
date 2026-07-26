//! Document sourcing — the barricade's outside edge: everything between an
//! untrusted byte source (a file path, or a pipe on stdin) and a parsed
//! [`Document`] the rest of the viewer may assume is valid.
//!
//! The whole pipeline lives behind [`DocumentSource::load`] on purpose. It is
//! called twice — once at startup and once per `--watch` reload — and the two
//! must not be allowed to drift; a caller that reassembled `read` →
//! UTF-8-check → frontmatter → mermaid → parse for itself would also have to
//! keep the parse-once rule (DW-2.5) true in two places.

use std::cell::Cell;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Instant, SystemTime};

use ast::Document;

use crate::app::FileInfo;

/// Why loading failed. `Display` renders a clean, user-facing message —
/// never a raw `io::Error` debug dump.
#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    InvalidUtf8,
    /// The source is bigger than [`MAX_DOCUMENT_BYTES`]. Carries the limit
    /// rather than a pre-formatted string, so a caller can decide (per
    /// `docs/code-standards.md`'s error-enum rule).
    TooLarge {
        limit: u64,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "could not read file: {e}"),
            LoadError::InvalidUtf8 => write!(f, "file is not valid UTF-8"),
            LoadError::TooLarge { limit } => write!(
                f,
                "document is larger than the {} MiB stele will read",
                limit / (1024 * 1024)
            ),
        }
    }
}

impl std::error::Error for LoadError {}

/// The most bytes a document may be, for a file and for stdin alike.
///
/// A ceiling is required because this is the barricade and both sources are
/// untrusted: `read_to_end` on a pipe will grow a `Vec` until the machine
/// gives out, and a 400 MB pipe was measured at **1.61 GB peak RSS** — the
/// `Vec`'s doubling, the `String`, and the retained AST all at once — before
/// the viewer had even decided it could open a terminal.
///
/// 64 MiB rather than something tighter because the bound is meant to catch
/// *category* errors (a video piped in by mistake, `stele -` in a pipeline
/// that never terminates), not to police long documents. It is far above any
/// real markdown file and far below the point where the process is a problem
/// for the machine. stele is a whole-document viewer — it parses and lays out
/// everything up front — so a document near this ceiling is already the wrong
/// tool; the honest failure is a clear message, not a slow death.
pub const MAX_DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;

/// Reads `reader` to end, refusing anything past `limit` bytes.
///
/// `take(limit + 1)` is what makes the refusal *bounded*: the read itself
/// stops one byte past the ceiling, so an oversized source costs one extra
/// byte of memory rather than all of it. Checking a length after an unbounded
/// read would report the same error having already paid the cost.
///
/// `limit` is a parameter rather than a hardcoded [`MAX_DOCUMENT_BYTES`]
/// because the barricade has a second, tighter door: a target opened by
/// *following a link* is bounded by `link::MAX_LINK_FILE_BYTES`, not by the
/// ceiling for a file the user named on the command line. One function, two
/// budgets, so the `take(limit + 1)` discipline cannot drift between them.
pub(crate) fn read_bounded<R: Read>(reader: R, limit: u64) -> Result<Vec<u8>, LoadError> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(LoadError::Io)?;
    if bytes.len() as u64 > limit {
        return Err(LoadError::TooLarge { limit });
    }
    Ok(bytes)
}

/// Preprocess and parse already-validated UTF-8 source text into a
/// [`LoadedDocument`] named `name`.
///
/// The single seam between "some bytes we have decided are a document" and a
/// parsed AST. [`DocumentSource::load_with`] and `crate::link`'s
/// link-following path both come through here, so the parse-once rule
/// (DW-2.5) and the frontmatter/mermaid preprocessing order are stated once
/// rather than reimplemented per entry point. [`FileInfo`] describes the text
/// **as read** — before preprocessing — for the reason `load_with` documents.
pub fn document_from_text(text: &str, name: String, options: LoadOptions) -> LoadedDocument {
    let info = FileInfo {
        name,
        byte_size: text.len() as u64,
        line_count: text.lines().count(),
    };
    let prepared = crate::decor::frontmatter::apply(text, options.show_frontmatter);
    let doc = crate::decor::mermaid::parse(&prepared);
    LoadedDocument {
        doc: Rc::new(doc),
        info,
    }
}

/// Source-text preprocessing policy, fixed for a session by the CLI.
#[derive(Debug, Clone, Copy, Default)]
pub struct LoadOptions {
    /// `--frontmatter`: show a leading YAML block as ordinary content
    /// instead of hiding it. Default (`false`) hides it.
    pub show_frontmatter: bool,
}

/// One load's product: the parsed document, shared rather than copied, plus
/// the raw-file facts `Ctrl-G` and the status row report.
#[derive(Debug)]
pub struct LoadedDocument {
    /// `Rc` so the app and [`crate::media::GfxMediaSink`] hold the *same*
    /// AST (DW-2.6) — the sink resolves media by `NodeId` against it, which
    /// only needs read access, never a copy.
    pub doc: Rc<Document>,
    pub info: FileInfo,
}

/// Where the document's bytes come from.
///
/// Two kinds, both known at compile time, so this is an enum rather than a
/// trait: the exhaustive matches below turn a third source kind into a
/// compile error instead of a silently unhandled case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentSource {
    Path(PathBuf),
    Stdin,
}

impl DocumentSource {
    /// Loads under the default policy. See [`DocumentSource::load_with`].
    pub fn load(&self) -> Result<LoadedDocument, LoadError> {
        self.load_with(LoadOptions::default())
    }

    /// Reads, validates, preprocesses and parses the document.
    ///
    /// [`LoadedDocument::info`] describes the bytes **as read**, before
    /// frontmatter stripping or mermaid rendering touch them — that is what a
    /// reader means by "this file" when they press `Ctrl-G`. `line_count`
    /// uses [`str::lines`], which is invariant to a trailing newline.
    ///
    /// Reading [`DocumentSource::Stdin`] consumes the pipe, so a second call
    /// on that variant yields an empty document. Nothing calls it twice: the
    /// only repeat caller is the `--watch` reload, and `--watch -` is
    /// rejected at CLI parse (DW-2.3) precisely because a stream cannot be
    /// re-read.
    pub fn load_with(&self, options: LoadOptions) -> Result<LoadedDocument, LoadError> {
        // Both arms go through `read_bounded`: the ceiling is a property of
        // "a document stele will read", not of one source. `read_to_end`
        // already retries on `ErrorKind::Interrupted` (EINTR), so no retry
        // loop is needed on either.
        let bytes = match self {
            DocumentSource::Path(path) => read_bounded(
                std::fs::File::open(path).map_err(LoadError::Io)?,
                MAX_DOCUMENT_BYTES,
            )?,
            DocumentSource::Stdin => read_bounded(std::io::stdin().lock(), MAX_DOCUMENT_BYTES)?,
        };
        let text = String::from_utf8(bytes).map_err(|_| LoadError::InvalidUtf8)?;
        Ok(document_from_text(&text, self.display_name(), options))
    }

    /// Whether the source's contents may have changed since `since`.
    ///
    /// `since` is monotonic and a file's mtime is wall-clock, so the two
    /// cannot be compared directly. `SystemTime::now() - since.elapsed()`
    /// reconstructs the wall clock reading at `since`, which is comparable.
    /// Two limits follow, and both are the harmless direction: a wall-clock
    /// jump between the two calls shifts the comparison by the jump, and a
    /// file *restored* with an older mtime (`git checkout`, `touch -t`) is
    /// not seen as changed. Neither can produce a torn frame — the worst
    /// case is a reload that does not happen until the next real write.
    ///
    /// **A file that cannot be stat'ed reports `true`.** Unable to prove it
    /// unchanged, this says "changed" so the caller's `load` runs, fails, and
    /// reports the real error on the status row (DW-2.4). Returning `false`
    /// would turn a deleted file into silence.
    pub fn changed_since(&self, since: Instant) -> bool {
        let path = match self {
            DocumentSource::Path(path) => path,
            // A consumed stream has no "later version" to poll for.
            DocumentSource::Stdin => return false,
        };
        let Ok(mtime) = std::fs::metadata(path).and_then(|meta| meta.modified()) else {
            return true;
        };
        match SystemTime::now().checked_sub(since.elapsed()) {
            Some(wall_clock_at_since) => mtime > wall_clock_at_since,
            // Only reachable if the wall clock sits before the epoch; treat
            // an unanswerable comparison the same as an unreadable file.
            None => true,
        }
    }

    /// The name shown in the status row and by `Ctrl-G`.
    pub fn display_name(&self) -> String {
        match self {
            DocumentSource::Path(path) => path.display().to_string(),
            DocumentSource::Stdin => STDIN_DISPLAY_NAME.to_string(),
        }
    }

    /// The directory relative image paths resolve against: the document's own
    /// directory, or the working directory for a stream that has none.
    pub fn base_dir(&self) -> PathBuf {
        match self {
            DocumentSource::Path(path) => path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
            DocumentSource::Stdin => PathBuf::from("."),
        }
    }
}

/// What the status row and `Ctrl-G` call a document that arrived on a pipe.
pub const STDIN_DISPLAY_NAME: &str = "(stdin)";

thread_local! {
    /// Thread-local rather than a global atomic so the DW-2.5 assertion is an
    /// exact count: `cargo test` runs unit tests on many threads at once, and
    /// a shared counter would make the delta depend on what else happened to
    /// be parsing. The load path itself is single-threaded.
    static PARSE_COUNT: Cell<u64> = const { Cell::new(0) };
}

/// How many times this thread has parsed a document through
/// [`counted_parse`] — the instrumentation DW-2.5 is asserted against, in
/// place of a timing measurement.
pub fn parse_count() -> u64 {
    PARSE_COUNT.with(Cell::get)
}

/// The load path's single entry to [`Document::parse`].
///
/// Every parse between raw bytes and a laid-out document goes through here so
/// [`parse_count`] is the whole truth about how many times a document was
/// parsed. `tests/hardening.rs` asserts by inspecting the sources that no
/// load-path file calls `Document::parse` directly — without that guard the
/// counter could be bypassed and DW-2.5's test would pass while parsing
/// twice.
pub(crate) fn counted_parse(source: &str) -> Document {
    PARSE_COUNT.with(|count| count.set(count.get().saturating_add(1)));
    Document::parse(source)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::media::GfxMediaSink;

    /// A scratch file unique to `tag`, so tests running concurrently in the
    /// same process cannot see each other's writes.
    fn scratch(tag: &str, contents: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("stele-loader-{tag}-{}.md", std::process::id()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn test_dw_5_4_missing_file_produces_clean_error() {
        let source = DocumentSource::Path(PathBuf::from("/nonexistent/does-not-exist.md"));
        let err = source.load().unwrap_err();
        assert!(matches!(err, LoadError::Io(_)));
        let message = err.to_string();
        assert!(message.starts_with("could not read file:"));
        // The message should be readable prose, not a raw Debug dump.
        assert!(!message.contains("Os {"));
    }

    #[test]
    fn test_dw_5_4_invalid_utf8_produces_clean_error() {
        let path =
            std::env::temp_dir().join(format!("stele-invalid-utf8-{}.md", std::process::id()));
        std::fs::write(&path, [0x48, 0x65, 0x6c, 0x6c, 0x6f, 0xff, 0xfe]).unwrap();
        let err = DocumentSource::Path(path.clone()).load().unwrap_err();
        std::fs::remove_file(&path).ok();
        assert!(matches!(err, LoadError::InvalidUtf8));
        assert_eq!(err.to_string(), "file is not valid UTF-8");
    }

    #[test]
    fn test_a_valid_file_loads_with_its_raw_size_and_line_count() {
        let path = scratch("valid", "# Hello\n\nWorld.\n");
        let loaded = DocumentSource::Path(path.clone()).load().unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(loaded.info.byte_size, 16);
        assert_eq!(loaded.info.line_count, 3);
        assert_eq!(loaded.info.name, path.display().to_string());
        assert_eq!(loaded.doc.blocks().len(), 2);
    }

    /// The reported size and line count describe the bytes on disk, not the
    /// preprocessed text the layout engine sees — a stripped frontmatter block
    /// still counts toward both.
    #[test]
    fn test_file_info_measures_the_raw_bytes_not_the_preprocessed_text() {
        let raw = "---\ntitle: t\n---\n# Hello\n";
        let path = scratch("frontmatter-info", raw);
        let source = DocumentSource::Path(path.clone());

        let hidden = source.load().unwrap();
        let shown = source
            .load_with(LoadOptions {
                show_frontmatter: true,
            })
            .unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(hidden.info.byte_size, raw.len() as u64);
        assert_eq!(hidden.info.byte_size, shown.info.byte_size);
        assert_eq!(hidden.info.line_count, shown.info.line_count);
        // The policy still reaches the parse: hiding the block leaves fewer
        // blocks than showing it does.
        assert!(hidden.doc.blocks().len() < shown.doc.blocks().len());
    }

    /// DW-2.5: the load path parses once, not twice. Asserted on an
    /// instrumented count rather than on elapsed time, which would only ever
    /// be a proxy — and a flaky one on a shared CI box.
    #[test]
    fn test_dw_2_5_a_document_without_mermaid_is_parsed_exactly_once() {
        let path = scratch(
            "parse-once",
            "# Title\n\nA paragraph.\n\n```rust\nfn main() {}\n```\n",
        );
        let source = DocumentSource::Path(path.clone());

        let before = parse_count();
        let loaded = source.load().unwrap();
        let spent = parse_count() - before;
        std::fs::remove_file(&path).ok();

        assert_eq!(spent, 1, "a mermaid-free document must cost one parse");
        assert_eq!(loaded.doc.blocks().len(), 3);
    }

    /// The other half of the same rule: a fence that really renders *does*
    /// cost a second parse, because the spliced text is different text. Pinned
    /// so "parse once" is never mistaken for "never parse twice".
    #[test]
    fn test_dw_2_5_a_renderable_mermaid_fence_costs_exactly_one_extra_parse() {
        assert!(
            mermaid::render("graph TD\n  A-->B\n").is_ok(),
            "graph TD must render for this test to exercise the splice"
        );
        let path = scratch(
            "parse-twice",
            "# Title\n\n```mermaid\ngraph TD\n  A-->B\n```\n",
        );
        let source = DocumentSource::Path(path.clone());

        let before = parse_count();
        source.load().unwrap();
        let spent = parse_count() - before;
        std::fs::remove_file(&path).ok();

        assert_eq!(spent, 2);
    }

    /// DW-2.6, in the shape startup actually uses: the app holds the loaded
    /// `Rc`, hands a handle to the sink, and the AST is allocated once. A
    /// deep copy would leave the count at 1 with two documents in memory.
    #[test]
    fn test_dw_2_6_the_sink_shares_the_loaded_document_rather_than_cloning_it() {
        let path = scratch("shared-doc", "# Title\n\n![alt](./a.png)\n");
        let loaded = DocumentSource::Path(path.clone()).load().unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(
            Rc::strong_count(&loaded.doc),
            1,
            "only the app holds it yet"
        );
        let sink = GfxMediaSink::new(Rc::clone(&loaded.doc), ".");
        assert_eq!(
            Rc::strong_count(&loaded.doc),
            2,
            "the sink must hold the same allocation, not a copy of the AST"
        );
        drop(sink);
        assert_eq!(Rc::strong_count(&loaded.doc), 1);
    }

    #[test]
    fn test_an_untouched_file_reports_unchanged() {
        let path = scratch("unchanged", "# Title\n");
        let source = DocumentSource::Path(path.clone());
        let loaded_at = Instant::now();
        source.load().unwrap();
        std::thread::sleep(Duration::from_millis(60));

        let changed = source.changed_since(loaded_at);
        std::fs::remove_file(&path).ok();
        assert!(!changed, "nothing wrote to the file; nothing changed");
    }

    #[test]
    fn test_dw_2_2_a_rewritten_file_reports_changed_and_reloads_the_new_text() {
        let path = scratch("rewritten", "# First\n");
        let source = DocumentSource::Path(path.clone());
        let loaded_at = Instant::now();
        let first = source.load().unwrap();

        std::thread::sleep(Duration::from_millis(60));
        std::fs::write(&path, "# First\n\n# Second\n").unwrap();

        let changed = source.changed_since(loaded_at);
        let second = source.load().unwrap();
        std::fs::remove_file(&path).ok();

        assert!(changed);
        assert_eq!(first.doc.blocks().len(), 1);
        assert_eq!(second.doc.blocks().len(), 2);
    }

    /// DW-2.4: a file that cannot be stat'ed must report *changed*, so the
    /// caller's next `load` surfaces the real error. Reporting "unchanged"
    /// would turn a deleted file into silence.
    #[test]
    fn test_dw_2_4_a_missing_path_reports_changed_so_the_failure_is_seen() {
        let source = DocumentSource::Path(PathBuf::from("/nonexistent/vanished.md"));
        assert!(source.changed_since(Instant::now()));
        assert!(matches!(source.load(), Err(LoadError::Io(_))));
    }

    /// DW-2.4's other half: deleting a file mid-session is exactly the
    /// sequence above, on a path that *was* loadable a moment ago.
    #[test]
    fn test_dw_2_4_a_file_deleted_after_a_good_load_reports_changed_then_fails() {
        let path = scratch("deleted", "# Title\n");
        let source = DocumentSource::Path(path.clone());
        let loaded_at = Instant::now();
        assert!(source.load().is_ok());

        std::fs::remove_file(&path).unwrap();

        assert!(source.changed_since(loaded_at));
        let err = source.load().unwrap_err();
        assert!(err.to_string().starts_with("could not read file:"));
    }

    /// DW-2.1: stdin names itself for the status row, and has no "later
    /// version" to poll for — a stream is read once to end, which is also why
    /// `--watch -` is rejected (DW-2.3).
    #[test]
    fn test_dw_2_1_stdin_source_names_itself_and_reports_no_change() {
        let source = DocumentSource::Stdin;
        assert_eq!(source.display_name(), "(stdin)");
        assert!(!source.changed_since(Instant::now()));
        assert_eq!(source.base_dir(), PathBuf::from("."));
    }

    /// A file truncated to nothing must load as an empty document, not as an
    /// error — `--watch` sees this whenever an editor writes in two steps.
    #[test]
    fn test_an_empty_file_loads_as_an_empty_document() {
        let path = scratch("empty", "");
        let loaded = DocumentSource::Path(path.clone()).load().unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(loaded.doc.blocks().len(), 0);
        assert_eq!(loaded.info.byte_size, 0);
        assert_eq!(loaded.info.line_count, 0);
    }

    /// Relative image paths resolve against the document's directory — and a
    /// bare filename has an empty parent, which must become `.` rather than
    /// the filesystem root's neighbour.
    #[test]
    fn test_base_dir_is_the_documents_directory_and_never_empty() {
        assert_eq!(
            DocumentSource::Path(PathBuf::from("/tmp/notes/a.md")).base_dir(),
            PathBuf::from("/tmp/notes")
        );
        assert_eq!(
            DocumentSource::Path(PathBuf::from("a.md")).base_dir(),
            PathBuf::from(".")
        );
    }

    /// The barricade must bound what it reads, not just validate it. A file
    /// one byte past the ceiling is refused with a message naming the limit;
    /// one byte under it still loads, so the bound is exact rather than
    /// approximately somewhere near 64 MiB.
    ///
    /// Exercised against a *lowered* ceiling would be a different function;
    /// this uses the real constant, and writes a sparse file so the test
    /// costs disk-that-is-not-allocated rather than 64 MiB of RAM.
    #[test]
    fn test_a_document_past_the_size_ceiling_is_refused_by_the_barricade() {
        let path = std::env::temp_dir().join(format!("stele-oversize-{}.md", std::process::id()));
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_DOCUMENT_BYTES + 1).unwrap();
        drop(file);

        let err = DocumentSource::Path(path.clone()).load().unwrap_err();
        std::fs::remove_file(&path).ok();

        assert!(matches!(err, LoadError::TooLarge { limit } if limit == MAX_DOCUMENT_BYTES));
        let message = err.to_string();
        assert!(message.contains("64 MiB"), "message was: {message}");
        assert!(!message.contains("Os {"), "message was: {message}");
    }

    /// The other side of the boundary: exactly at the ceiling is fine. Without
    /// this, `read_bounded` could refuse everything and still pass the test
    /// above.
    #[test]
    fn test_a_document_exactly_at_the_size_ceiling_still_loads() {
        // A real allocation of 64 MiB would dominate the suite's memory; the
        // read path is identical for a sparse file, and `read_to_end` really
        // does produce `MAX_DOCUMENT_BYTES` bytes from it.
        let path = std::env::temp_dir().join(format!("stele-atlimit-{}.md", std::process::id()));
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_DOCUMENT_BYTES).unwrap();
        drop(file);

        let loaded = DocumentSource::Path(path.clone()).load();
        std::fs::remove_file(&path).ok();

        assert!(
            loaded.is_ok(),
            "exactly at the limit must be accepted, got {:?}",
            loaded.err()
        );
    }
}
