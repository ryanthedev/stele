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
    let prepared = preprocess(text, options);
    let doc = crate::decor::mermaid::parse(&prepared);
    LoadedDocument {
        doc: Rc::new(doc),
        info,
    }
}

/// Every source-text rewrite, in the one order they are allowed to run in.
///
/// `frontmatter` first because it can only ever act on the very start of the
/// file and the passes after it would otherwise have to know that a stripped
/// block used to be there. Then `codecogs`, whose input is an image link and
/// whose output is maths — it must run before anything that could edit the
/// line the link sits on. Then `d2l`, then `quarto`, which is the only pass
/// that changes a line's *block* structure, so it goes last and reads text
/// the others have already settled.
///
/// **A consequence worth stating.** `mermaid::parse` runs after all of them
/// and, by its own documented rule (`decor/mermaid.rs:13-15`), only renders
/// *top-level* mermaid fences. A ` ```mermaid ` fence that began at the top
/// level and ends up inside a callout that `quarto` turned into a blockquote
/// is no longer top level, so it renders as a plain code block rather than
/// as a diagram. That is the same fallback a mermaid fence in a list or a
/// blockquote already gets, reached by a new route.
///
/// **The remote-image pass runs last, and outside the `--no-rewrite` gate.**
/// Last because it must not fetch anything the passes above would have
/// deleted — `codecogs` turns a `latex.codecogs.com` image into `$…$` maths,
/// and running the network before it would have downloaded an equation
/// picture stele was about to replace with the equation. Outside the gate
/// because `--no-rewrite` is about what the document *says* and this pass
/// changes none of it; `media::remote`'s module doc argues that at length,
/// including the one behaviour it implies (under `--no-rewrite` a CodeCogs
/// image stays an image and so is fetched).
///
/// This is also the only pass here that can block: it is the reason
/// [`FetchLimits::document_budget`](crate::media::FetchLimits::document_budget)
/// exists, and it runs before the terminal is entered so a slow host delays
/// the first frame rather than freezing a live one.
fn preprocess(text: &str, options: LoadOptions) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;

    let mut current = crate::decor::frontmatter::apply(text, options.show_frontmatter);
    if options.rewrite_source {
        let passes: [for<'a> fn(&'a str) -> Cow<'a, str>; 3] = [
            crate::decor::codecogs::apply,
            crate::decor::d2l::apply,
            crate::decor::quarto::apply,
        ];
        for pass in passes {
            // The `match` (rather than `if let`) is what ends the borrow of
            // `current` before the reassignment: a pass that changed nothing
            // hands back a slice of the very string being replaced.
            let rewritten = match pass(&current) {
                Cow::Borrowed(_) => None,
                Cow::Owned(text) => Some(text),
            };
            if let Some(text) = rewritten {
                current = Cow::Owned(text);
            }
        }
    }
    // `None` — the default, and what every run without `--fetch-remote` gets —
    // does not reach this pass at all. That is the whole privacy claim: not a
    // network call that declines, but no call site.
    if let Some(remote) = options.remote {
        let rewritten = match remote.rewrite(&current) {
            Cow::Borrowed(_) => None,
            Cow::Owned(text) => Some(text),
        };
        if let Some(text) = rewritten {
            current = Cow::Owned(text);
        }
    }
    current
}

/// Source-text preprocessing policy, fixed for a session by the CLI.
#[derive(Debug, Clone, Copy)]
pub struct LoadOptions {
    /// `--frontmatter`: show a leading YAML block as ordinary content
    /// instead of hiding it. Default (`false`) hides it.
    pub show_frontmatter: bool,
    /// Whether the d2l, Quarto and CodeCogs source rewrites run.
    /// `--no-rewrite` turns them off; see [`Default`] below for why the
    /// default is `true` and the field is not named for the flag.
    pub rewrite_source: bool,
    /// `--fetch-remote`: how a `![alt](https://…)` image is resolved, and
    /// whether it is resolved at all. **`None` — the default — never touches
    /// the network**, and does so by never calling the pass rather than by
    /// calling a pass that declines.
    ///
    /// `&'static` rather than owned or `Rc` so this struct stays `Copy`: it is
    /// passed by value into `crate::link`'s `Navigator` and back out of the
    /// `--watch` reload, and there is exactly one of these per process anyway
    /// (`media::remote::production`).
    pub remote: Option<&'static crate::media::RemoteImages>,
}

impl Default for LoadOptions {
    /// The default *policy*, not the zero value — which is why this is
    /// written out rather than derived. The rewrites are on by default
    /// because a d2l or Quarto document is unreadable without them; the
    /// escape hatch exists for the reader who wants to see the file as it
    /// was written, exactly as `--frontmatter` exists for the reader who
    /// wants to see the block stele hides.
    ///
    /// Remote fetching is the one policy here that defaults *off*, and it is
    /// the only one whose default is a stance rather than a convenience: a
    /// markdown file that makes a network request the moment it is opened is
    /// a tracking vector, and opening a document is not consent to be counted.
    fn default() -> Self {
        LoadOptions {
            show_frontmatter: false,
            rewrite_source: true,
            remote: None,
        }
    }
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
    use crate::media::{GfxMediaSink, RemoteImages};

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
                ..LoadOptions::default()
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

    /// The source rewrites reach the parse, and reach it in the documented
    /// order: `codecogs` turns an image into maths, `d2l` strips the roles
    /// around it, `quarto` turns the callout it all sits in into an alert.
    /// A pass running out of turn — `quarto` before `d2l`, say — would quote
    /// the `:label:` line instead of dropping it, and the alert would come
    /// out with a line of Sphinx in it.
    #[test]
    fn test_the_source_rewrites_reach_the_parse_in_the_documented_order() {
        let raw = concat!(
            "::: {.callout-note}\n",
            ":label:`sec_x`\n",
            "See :numref:`fig_y`, and ",
            "![e](http://latex.codecogs.com/svg.latex?x%5E2).\n",
            ":::\n",
        );
        let path = scratch("rewrites", raw);
        let loaded = DocumentSource::Path(path.clone()).load().unwrap();
        std::fs::remove_file(&path).ok();

        let blocks = loaded.doc.blocks();
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        let ast::BlockKind::Alert { kind, children } = &blocks[0].kind else {
            panic!("expected an alert, got {:?}", blocks[0].kind);
        };
        assert_eq!(*kind, ast::AlertKind::Note);
        let dumped = format!("{children:?}");
        assert!(!dumped.contains(":label:"), "{dumped}");
        assert!(!dumped.contains(":numref:"), "{dumped}");
        assert!(!dumped.contains("codecogs"), "{dumped}");
        assert!(dumped.contains("x^2"), "the maths must survive: {dumped}");
    }

    /// R4's escape hatch. The same document, read as written: every
    /// construct still there, and — because nothing was rewritten — the same
    /// number of source lines the reader would see in an editor.
    #[test]
    fn test_no_rewrite_shows_the_document_exactly_as_written() {
        let raw = "::: {.callout-note}\n:label:`sec_x`\nBody.\n:::\n";
        let path = scratch("no-rewrite", raw);
        let source = DocumentSource::Path(path.clone());
        let as_written = source
            .load_with(LoadOptions {
                rewrite_source: false,
                ..LoadOptions::default()
            })
            .unwrap();
        let rewritten = source.load().unwrap();
        std::fs::remove_file(&path).ok();

        let dumped = format!("{:?}", as_written.doc.blocks());
        assert!(dumped.contains(":::"), "{dumped}");
        assert!(dumped.contains(":label:"), "{dumped}");
        assert!(
            !matches!(
                as_written.doc.blocks()[0].kind,
                ast::BlockKind::Alert { .. }
            ),
            "nothing may have been rewritten"
        );
        // And the flag really is the only difference.
        assert!(matches!(
            rewritten.doc.blocks()[0].kind,
            ast::BlockKind::Alert { .. }
        ));
    }

    /// The documented consequence of running `mermaid::parse` last: a fence
    /// that *was* top level is not top level any more once `quarto` has
    /// wrapped it in a blockquote, and `decor/mermaid.rs:13-15` only renders
    /// top-level fences. It falls back to a plain code block — the same
    /// fallback a mermaid fence in a list already gets, reached by a new
    /// route. Pinned so the fallback is a decision rather than a surprise.
    #[test]
    fn test_a_mermaid_fence_inside_a_rewritten_callout_falls_back_to_a_code_block() {
        assert!(
            mermaid::render("graph TD\n  A-->B\n").is_ok(),
            "graph TD must render for this test to mean anything"
        );
        let raw = "::: {.callout-note}\n```mermaid\ngraph TD\n  A-->B\n```\n:::\n";
        let path = scratch("mermaid-in-callout", raw);
        let loaded = DocumentSource::Path(path.clone()).load().unwrap();
        std::fs::remove_file(&path).ok();

        let ast::BlockKind::Alert { children, .. } = &loaded.doc.blocks()[0].kind else {
            panic!("expected an alert, got {:?}", loaded.doc.blocks()[0].kind);
        };
        let ast::BlockKind::CodeBlock { info, literal, .. } = &children[0].kind else {
            panic!("expected a code block, got {:?}", children[0].kind);
        };
        assert_eq!(
            info.as_deref(),
            Some("mermaid"),
            "the fence keeps its unrendered mermaid tag"
        );
        assert_eq!(literal, "graph TD\n  A-->B\n");
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

    /// A document with several remote images in it, plus prose either side so
    /// a rewrite that eats a line is visible.
    const REMOTE_DOC: &str = concat!(
        "# Diagrams\n",
        "\n",
        "![the first](https://example.com/one.png)\n",
        "\n",
        "Some prose between them.\n",
        "\n",
        "![the second](https://example.com/two.png) inline with ",
        "![the third](https://example.com/three.png)\n",
    );

    /// A `RemoteImages` over a counting fake, leaked to `'static` because
    /// [`LoadOptions`] holds it that way — see the field's doc for why that
    /// struct must stay `Copy`. A leak in a test is bounded by the test binary.
    fn leaked_remote(dir: &Path, log: &crate::media::fetch::fake::Log) -> &'static RemoteImages {
        use crate::media::cache::{Cache, DEFAULT_CACHE_BYTES};
        use crate::media::fetch::fake::{Fake, png};

        Box::leak(Box::new(RemoteImages::new(
            Box::new(Fake::serving(log, png(48, 48))),
            Cache::at(dir, DEFAULT_CACHE_BYTES),
            crate::media::FetchLimits::default(),
        )))
    }

    /// One painted frame of `doc`, byte for byte as the viewer would emit it.
    fn frame_of(doc: &Document, base_dir: &Path) -> Vec<u8> {
        use layout::{LayoutConfig, layout};
        use width::{WidthConfig, WidthEngine};

        use crate::painter::{Painter, Size};

        let engine = WidthEngine::new(WidthConfig::default());
        let sizer = crate::media::ImageSizer::new(base_dir);
        let tree = layout(doc, 80, &LayoutConfig::default(), &engine, &sizer);
        let mut painter = Painter::new(WidthEngine::new(WidthConfig::default()));
        let mut buf = Vec::new();
        painter
            .frame(
                &tree,
                0,
                Size {
                    width: 80,
                    height: 24,
                },
                &mut buf,
            )
            .unwrap();
        buf
    }

    /// **The privacy claim, at the level it is claimed.** A document full of
    /// remote images, loaded under the default policy, with a working fetcher
    /// sitting right there in the process: zero requests, and a frame that is
    /// byte-identical to today's alt-text path.
    ///
    /// The fake is deliberately *constructed and never installed*. A test that
    /// simply omitted it would assert nothing — it would pass against a build
    /// with no fetching code at all — so the fetcher here is real, ready, and
    /// asked for nothing.
    ///
    /// The byte-identity oracle is independent of this file: the comparison
    /// frame is painted from `Document::parse(REMOTE_DOC)` directly, with the
    /// whole preprocessing chain bypassed. If any pass had touched the
    /// document — the remote one included — the two frames would differ.
    #[test]
    fn test_the_default_policy_fetches_nothing_and_paints_todays_alt_text_frame() {
        let dir = std::env::temp_dir().join(format!("stele-loader-off-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = crate::media::fetch::fake::Log::default();
        let _ready_but_uninstalled = leaked_remote(&dir, &log);

        let path = scratch("remote-off", REMOTE_DOC);
        let options = LoadOptions::default();
        assert!(
            options.remote.is_none(),
            "the default policy names a fetcher"
        );
        let loaded = DocumentSource::Path(path.clone())
            .load_with(options)
            .unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(
            log.calls(),
            0,
            "the default policy fetched {:?}",
            log.urls()
        );
        assert_eq!(
            frame_of(&loaded.doc, &dir),
            frame_of(&Document::parse(REMOTE_DOC), &dir),
            "the flag-off frame differs from an untouched parse of the same source"
        );
        // And what that frame contains is the alt text, not a path.
        let painted = String::from_utf8_lossy(&frame_of(&loaded.doc, &dir)).into_owned();
        for alt in ["the first", "the second", "the third"] {
            assert!(painted.contains(alt), "{alt} is missing from the frame");
        }
        assert!(
            !painted.contains(dir.to_str().unwrap()),
            "a cache path reached the screen"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The same document with the policy installed: three requests, three
    /// destinations inside the cache, and a frame that is no longer the
    /// alt-text one. Without this the test above would pass equally well
    /// against a `--fetch-remote` that is wired to nothing.
    #[test]
    fn test_the_installed_policy_resolves_every_remote_image_through_the_loader() {
        let dir = std::env::temp_dir().join(format!("stele-loader-on-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let log = crate::media::fetch::fake::Log::default();
        let remote = leaked_remote(&dir, &log);

        let path = scratch("remote-on", REMOTE_DOC);
        let source = DocumentSource::Path(path.clone());
        let options = LoadOptions {
            remote: Some(remote),
            ..LoadOptions::default()
        };
        let loaded = source.load_with(options).unwrap();

        assert_eq!(
            log.urls(),
            vec![
                "https://example.com/one.png".to_string(),
                "https://example.com/two.png".to_string(),
                "https://example.com/three.png".to_string(),
            ]
        );
        let dests: Vec<String> = loaded
            .doc
            .nodes()
            .filter_map(|node| match node {
                ast::NodeRef::Inline(inline) => match &inline.kind {
                    ast::InlineKind::Image { dest, .. } => Some(dest.clone()),
                    _ => None,
                },
                ast::NodeRef::Block(_) => None,
            })
            .collect();
        assert_eq!(dests.len(), 3, "{dests:?}");
        for dest in &dests {
            assert_eq!(
                Path::new(dest).parent(),
                Some(dir.as_path()),
                "{dest} is not in the cache"
            );
            assert!(Path::new(dest).is_file(), "{dest} was not written");
        }
        assert_ne!(
            frame_of(&loaded.doc, &dir),
            frame_of(&Document::parse(REMOTE_DOC), &dir),
            "the resolved document painted the same frame as the untouched one"
        );

        // A second load of the same document costs nothing more: the whole
        // point of the cache, asserted through the loader rather than through
        // the pass.
        source.load_with(options).unwrap();
        assert_eq!(log.calls(), 3, "the reload refetched: {:?}", log.urls());

        std::fs::remove_file(&path).ok();
        std::fs::remove_dir_all(&dir).ok();
    }

    /// `--no-rewrite` does not cancel `--fetch-remote`. The two answer
    /// different questions — one is about what the document *says*, the other
    /// about whether its pictures are drawn — and `media::remote`'s module doc
    /// argues the case. Pinned here because "these flags compose" is the kind
    /// of decision a later refactor folds into one gate by accident.
    #[test]
    fn test_no_rewrite_leaves_remote_fetching_alone() {
        let dir = std::env::temp_dir().join(format!("stele-loader-both-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let log = crate::media::fetch::fake::Log::default();
        let remote = leaked_remote(&dir, &log);

        // A Quarto callout the rewrite would normally consume, with a remote
        // image inside it: one flag must leave the callout alone and the other
        // must still resolve the image.
        let raw = concat!(
            "::: {.callout-note}\n",
            "![a diagram](https://example.com/one.png)\n",
            ":::\n",
        );
        let path = scratch("no-rewrite-remote", raw);
        let loaded = DocumentSource::Path(path.clone())
            .load_with(LoadOptions {
                rewrite_source: false,
                remote: Some(remote),
                ..LoadOptions::default()
            })
            .unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(log.calls(), 1, "--no-rewrite suppressed the fetch");
        let dumped = format!("{:?}", loaded.doc.blocks());
        assert!(
            dumped.contains(":::"),
            "the callout was rewritten: {dumped}"
        );
        assert!(
            !dumped.contains("https://"),
            "the image kept its URL: {dumped}"
        );
        std::fs::remove_dir_all(&dir).ok();
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
