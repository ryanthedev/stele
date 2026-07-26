//! Link following — the barricade between a destination written in an
//! untrusted document and the filesystem or the OS's URL opener.
//!
//! The user chose a **permissive** link policy: a relative markdown link, an
//! arbitrary local file, and an `http`/`https` URL are all followable, and a
//! `../` path or a symlink pointing outside the document's directory is a
//! legitimate target rather than an attack. There is no directory jail here,
//! and adding one would be a change of policy, not a hardening.
//!
//! Safety therefore comes from the *shape* of what is done with a target, not
//! from narrowing which targets are allowed:
//!
//! - **No shell, ever.** A URL reaches the OS opener as one argv element of a
//!   `std::process::Command`, never as text interpolated into `sh -c`. Shell
//!   metacharacters, `$(…)`, backticks and newlines are then not special to
//!   anything, because nothing on the path parses them.
//! - **Scheme allowlist for activation.** Only `http` and `https` are handed
//!   to the opener. Everything else — `file:`, `mailto:`, `javascript:`,
//!   `data:`, a scheme nobody has thought of yet — is refused with a message.
//!   This is deliberately *tighter* than
//!   [`highlight::sanitize_url`](highlight::sanitize_url), which governs
//!   painting an OSC 8 hyperlink: rendering a `mailto:` as clickable text and
//!   spawning a process for it are not the same act.
//! - **File type before file content.** A target's type is settled by `stat`
//!   before a single byte is read, because binary-detection-by-read on a FIFO
//!   or a character device does not return. `/dev/zero` would read forever; a
//!   FIFO with no writer blocks in `open` itself.
//! - **Bounded everything.** A size ceiling ([`MAX_LINK_FILE_BYTES`]) checked
//!   against the stat size *and* enforced during the read, a URL length
//!   ceiling, a NUL sniff over a bounded prefix, and a bounded document stack.
//!
//! ## Layering
//!
//! This module decides policy and owns no I/O device. [`UrlOpener`] is the
//! seam to the OS: declared here, where it is used, and implemented by
//! [`SystemOpener`] — so a test can assert the exact argv a URL produces
//! without a browser opening, and so nothing in the decision path depends on
//! `std::process`.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::loader::{DocumentSource, LoadError, LoadOptions, LoadedDocument, read_bounded};

/// The most bytes a file opened by *following a link* may be.
///
/// Deliberately tighter than [`crate::loader::MAX_DOCUMENT_BYTES`] (64 MiB).
/// Opening the file named on the command line is the reader's own explicit
/// choice; following a link is one keystroke against a destination someone
/// else wrote. The ceiling is checked twice — against the stat size before
/// anything is opened, so an oversized target costs no read at all, and again
/// during the read itself, so a file that grows between the two cannot slip
/// past.
pub const MAX_LINK_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// The longest URL handed to the OS opener. Long enough for any real link,
/// short enough that a pathological destination cannot be used to stuff a
/// process argument list.
pub const MAX_URL_BYTES: usize = 4096;

/// How much of a file's head is sniffed for a NUL byte before it is treated
/// as text. A NUL is legal UTF-8, so [`String::from_utf8`] alone would hand a
/// linked object file straight to the markdown parser.
const BINARY_SNIFF_BYTES: usize = 8192;

/// The deepest the document stack may go. A bound rather than a feature: each
/// entry is small, but an unbounded stack turns a document that links to
/// itself into a memory leak driven by held-down `Enter`.
pub const MAX_STACK_DEPTH: usize = 64;

/// Schemes that may be handed to the OS opener. An allowlist, not a denylist,
/// so a scheme nobody has enumerated is refused by default.
const ACTIVATABLE_SCHEMES: [&str; 2] = ["http", "https"];

/// The program that opens a URL, invoked with the URL as a single argv
/// element. Never a shell, and never a string a shell will parse — see
/// [`opener_argv`].
pub const OPENER_PROGRAM: &str = if cfg!(target_os = "macos") {
    "open"
} else {
    "xdg-open"
};

/// What a link destination turned out to be.
///
/// The same type names both an *unresolved* classification (what the href
/// looks like) and a *resolved* target (what it points at on this machine),
/// which is why [`LinkTarget::resolve`] takes and returns one: resolution
/// canonicalizes the path and settles the file's type without changing which
/// of the three kinds it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    /// A local path that looks like markdown (`.md` / `.markdown`).
    LocalDoc(PathBuf),
    /// Any other local path. Opened exactly like [`LinkTarget::LocalDoc`] —
    /// the distinction is what the destination *claims*, not what is allowed
    /// (DW-6.8).
    LocalFile(PathBuf),
    /// An `http`/`https` URL for the OS opener.
    Url(String),
}

/// Why a link could not be followed. Every variant reaches the reader as a
/// status-row message and leaves the current document on screen.
///
/// Variants carry the data a caller needs to decide rather than a
/// pre-formatted string, per `docs/code-standards.md`.
#[derive(Debug)]
pub enum LinkError {
    /// An empty destination, or one that is only whitespace.
    Empty,
    /// A `#fragment`-only destination. stele has no in-document anchors, so
    /// this is refused rather than silently doing nothing.
    Fragment,
    /// A URL scheme outside [`ACTIVATABLE_SCHEMES`] (DW-6.3).
    UnsupportedScheme(String),
    /// An `http(s)` URL longer than [`MAX_URL_BYTES`], or carrying any code
    /// point [`highlight::is_display_hazard`] names — a control byte, a
    /// newline, or a bidi character that would let the URL preview as one
    /// destination and open as another. Refused rather than sanitized:
    /// silently rewriting a URL and then opening the rewrite is worse than
    /// declining.
    MalformedUrl,
    /// The path does not exist, or cannot be canonicalized.
    Missing(PathBuf),
    /// The path exists but is not a regular file — a directory, a FIFO, a
    /// socket, a character or block device (DW-6.4). Settled by `stat`,
    /// before any read.
    NotAFile(PathBuf),
    /// Bigger than [`MAX_LINK_FILE_BYTES`].
    TooLarge { limit: u64 },
    /// A NUL byte in the file's head, or bytes that are not UTF-8.
    Binary(PathBuf),
    /// The file exists and is a regular file but could not be read.
    Io(std::io::Error),
    /// [`DocumentStack`] is already [`MAX_STACK_DEPTH`] deep.
    StackTooDeep { limit: usize },
    /// `Backspace` with nothing to go back to.
    AtRoot,
    /// `Backspace` to a document that arrived on stdin. A stream is read once
    /// to end and cannot be re-read — the same fact that makes `--watch -` a
    /// CLI-parse error (DW-2.3), surfacing here instead of quietly rendering
    /// the empty document a second `read_to_end` on a drained pipe returns.
    StreamNotRereadable,
    /// The OS opener could not be spawned, or exited non-zero.
    OpenerFailed(String),
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkError::Empty => write!(f, "link has no destination"),
            LinkError::Fragment => write!(f, "in-document anchors are not supported"),
            LinkError::UnsupportedScheme(scheme) => {
                write!(f, "refusing to open a `{scheme}:` link — http/https only")
            }
            LinkError::MalformedUrl => write!(f, "refusing to open a malformed URL"),
            LinkError::Missing(path) => write!(f, "no such file: {}", path.display()),
            LinkError::NotAFile(path) => {
                write!(f, "not a regular file: {}", path.display())
            }
            LinkError::TooLarge { limit } => write!(
                f,
                "linked file is larger than the {} MiB stele will open",
                limit / (1024 * 1024)
            ),
            LinkError::Binary(path) => {
                write!(f, "refusing to open binary content: {}", path.display())
            }
            LinkError::Io(e) => write!(f, "could not read linked file: {e}"),
            LinkError::StackTooDeep { limit } => {
                write!(f, "link history is {limit} deep — go back before going on")
            }
            LinkError::AtRoot => write!(f, "already at the first document"),
            LinkError::StreamNotRereadable => write!(
                f,
                "cannot go back to a document read from stdin — a stream is \
                 read once to end"
            ),
            LinkError::OpenerFailed(reason) => write!(f, "could not open the link: {reason}"),
        }
    }
}

impl std::error::Error for LinkError {}

impl From<LoadError> for LinkError {
    fn from(err: LoadError) -> Self {
        match err {
            LoadError::Io(e) => LinkError::Io(e),
            // A linked file that is not UTF-8 is, for a viewer's purposes,
            // binary: there is nothing to render and no way to say what it
            // would have said. The path is filled in by the caller, which is
            // the only place that still has it.
            LoadError::InvalidUtf8 => LinkError::Binary(PathBuf::new()),
            LoadError::TooLarge { limit } => LinkError::TooLarge { limit },
        }
    }
}

impl LinkTarget {
    /// Classifies a raw href, without touching the filesystem.
    ///
    /// A destination with an RFC 3986 scheme (`ALPHA *( ALPHA / DIGIT / "+" /
    /// "-" / "." ) ":"`) is a URL and is answered by the allowlist. Anything
    /// else is a local path — including an absolute `/etc/hosts` and a `../`
    /// traversal, both of which the chosen policy allows.
    ///
    /// The scheme test is written out rather than "contains a colon" because
    /// a colon is a perfectly ordinary character in a filename; `notes:2026.md`
    /// is a relative path, not a `notes:` URL, only if the part before the
    /// colon fails the scheme grammar — which it does not, so it is treated as
    /// a URL and refused. That is the safe direction: a refusal a reader can
    /// see beats resolving an ambiguous string against the filesystem.
    pub fn classify(href: &str) -> Result<LinkTarget, LinkError> {
        let href = href.trim();
        if href.is_empty() {
            return Err(LinkError::Empty);
        }
        if href.starts_with('#') {
            return Err(LinkError::Fragment);
        }
        if let Some(scheme) = url_scheme(href) {
            if ACTIVATABLE_SCHEMES
                .iter()
                .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
            {
                return Ok(LinkTarget::Url(href.to_string()));
            }
            return Err(LinkError::UnsupportedScheme(scheme.to_ascii_lowercase()));
        }
        // A `?query` or `#fragment` suffix is meaningless on a local path and
        // would become part of the filename; drop it before the path is built.
        let path = href
            .split_once(['?', '#'])
            .map_or(href, |(before, _)| before);
        if path.is_empty() {
            return Err(LinkError::Fragment);
        }
        let path = PathBuf::from(percent_decoded(path));
        if is_markdown_path(&path) {
            Ok(LinkTarget::LocalDoc(path))
        } else {
            Ok(LinkTarget::LocalFile(path))
        }
    }

    /// Resolves this target against the open document's directory, returning
    /// a target that is safe to act on — **without reading one byte of it**.
    ///
    /// For a URL: re-validates the scheme and refuses anything past
    /// [`MAX_URL_BYTES`] or carrying a [`highlight::is_display_hazard`] code
    /// point — see [`validated_url`] for why the bidi half is not redundant
    /// with the OSC 8 sanitizer.
    ///
    /// For a path: joins `base`, canonicalizes (which resolves `..` and every
    /// symlink, and is also the existence check), then `stat`s and requires a
    /// **regular file** within [`MAX_LINK_FILE_BYTES`]. The type check is here,
    /// ahead of every read in this module, because the read is what cannot be
    /// taken back: `open` on a FIFO with no writer blocks, and a read of
    /// `/dev/zero` never ends. `fs::metadata` and `fs::canonicalize` both
    /// `stat` rather than open, so neither can block on either.
    pub fn resolve(&self, base: &Path) -> Result<LinkTarget, LinkError> {
        match self {
            LinkTarget::Url(url) => Ok(LinkTarget::Url(validated_url(url)?)),
            LinkTarget::LocalDoc(path) => Ok(LinkTarget::LocalDoc(resolve_regular_file(
                path,
                base,
                MAX_LINK_FILE_BYTES,
            )?)),
            LinkTarget::LocalFile(path) => Ok(LinkTarget::LocalFile(resolve_regular_file(
                path,
                base,
                MAX_LINK_FILE_BYTES,
            )?)),
        }
    }
}

/// The scheme of `href`, if it has one by RFC 3986's grammar.
fn url_scheme(href: &str) -> Option<&str> {
    let (scheme, _) = href.split_once(':')?;
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if chars.any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))) {
        return None;
    }
    Some(scheme)
}

/// Whether `path`'s extension names markdown. Case-insensitive: `README.MD`
/// is markdown.
fn is_markdown_path(path: &Path) -> bool {
    path.extension().is_some_and(|ext| {
        let ext = ext.to_string_lossy().to_ascii_lowercase();
        ext == "md" || ext == "markdown"
    })
}

/// Decodes `%XX` escapes in a link path, leaving anything malformed alone.
///
/// Markdown writers percent-encode spaces (`my%20notes.md`) and CommonMark
/// does not decode them, so without this the path would be looked up with a
/// literal `%20` in it and reported missing. Only well-formed two-hex-digit
/// escapes are decoded, and a decoded NUL is dropped rather than embedded —
/// a NUL inside a path would truncate it at the syscall boundary.
fn percent_decoded(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let decoded = (bytes[i] == b'%' && i + 2 < bytes.len())
            .then(|| {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
                u8::from_str_radix(hex, 16).ok()
            })
            .flatten();
        match decoded {
            Some(0) => i += 3,
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| path.to_string())
}

/// The URL-activation half of the barricade: scheme allowlist, length
/// ceiling, and a refusal of every code point
/// [`highlight::is_display_hazard`] names — the C0/DEL/C1 controls (which is
/// what a newline, a carriage return and a NUL all are) plus the bidi
/// reordering class.
///
/// **The bidi half is not redundant with the OSC 8 sanitizer, and the
/// difference is the whole point.** That sanitizer *strips* hazards from the
/// URI it prints, so the destination a reader inspects on hover is clean.
/// This function validates the destination that actually *opens*, and it
/// reads the raw dest straight off the AST. If it only refused ASCII
/// controls, a link could preview as `safe.md` — cleaned — and activate
/// `dm.exe`, the spoof surviving into the one moment it matters. What is
/// shown and what is opened have to agree about the same set.
///
/// Refused rather than stripped, matching this function's existing contract:
/// it already rejects a URL containing a newline instead of quietly removing
/// it, because silently opening *a different URL than the document asked
/// for* is its own surprise.
fn validated_url(url: &str) -> Result<String, LinkError> {
    if url.len() > MAX_URL_BYTES {
        return Err(LinkError::MalformedUrl);
    }
    if url.chars().any(highlight::is_display_hazard) {
        return Err(LinkError::MalformedUrl);
    }
    let scheme = url_scheme(url).ok_or(LinkError::MalformedUrl)?;
    if !ACTIVATABLE_SCHEMES
        .iter()
        .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
    {
        return Err(LinkError::UnsupportedScheme(scheme.to_ascii_lowercase()));
    }
    // A URL that is nothing but its scheme has no target to open.
    if url.len() <= scheme.len() + 1 {
        return Err(LinkError::MalformedUrl);
    }
    Ok(url.to_string())
}

/// Joins, canonicalizes and type-checks a local target against a size
/// ceiling. No read.
///
/// `limit` is a parameter because the ceiling is a property of *provenance*,
/// not of this function: a file the reader named on the command line is
/// admitted at [`crate::loader::MAX_DOCUMENT_BYTES`], one reached by
/// following a link at the tighter [`MAX_LINK_FILE_BYTES`]. Hardcoding the
/// link ceiling here would refuse to go **back** to a 20 MiB document the
/// reader opened deliberately.
fn resolve_regular_file(path: &Path, base: &Path, limit: u64) -> Result<PathBuf, LinkError> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    // `canonicalize` resolves `..` and every symlink on the way, and fails
    // for a path that does not exist — deliberately *not* a containment
    // check. Where it lands is where the reader asked to go.
    let resolved = std::fs::canonicalize(&joined).map_err(|_| LinkError::Missing(joined))?;
    let meta = std::fs::metadata(&resolved).map_err(LinkError::Io)?;
    if !meta.is_file() {
        return Err(LinkError::NotAFile(resolved));
    }
    if meta.len() > limit {
        return Err(LinkError::TooLarge { limit });
    }
    Ok(resolved)
}

/// Reads an already-[`resolve`](LinkTarget::resolve)d path as document text.
///
/// Repeats the regular-file check twice more, and neither is redundant:
///
/// 1. `fs::metadata` before `File::open`, because `open` itself is the call
///    that blocks on a FIFO — by the time a descriptor exists it is too late
///    to decline.
/// 2. `File::metadata` on the **open descriptor**, which is an `fstat` on the
///    object we are about to read rather than a second lookup of a name that
///    may now mean something else. This is what closes the
///    check-then-read race.
///
/// The one window that remains is a path replaced between (1) and the `open`.
/// Closing it needs `O_NONBLOCK` through `OpenOptionsExt::custom_flags`, whose
/// value is a platform-specific constant; the cost of that is a portability
/// matrix, and the exposure is an attacker who can already write into the
/// document's directory at the instant of a keystroke. Documented rather than
/// silently accepted.
pub fn read_text_target(path: &Path, limit: u64) -> Result<String, LinkError> {
    let meta = std::fs::metadata(path).map_err(LinkError::Io)?;
    if !meta.is_file() {
        return Err(LinkError::NotAFile(path.to_path_buf()));
    }
    let file = std::fs::File::open(path).map_err(LinkError::Io)?;
    let opened = file.metadata().map_err(LinkError::Io)?;
    if !opened.is_file() {
        return Err(LinkError::NotAFile(path.to_path_buf()));
    }
    if opened.len() > limit {
        return Err(LinkError::TooLarge { limit });
    }
    let bytes = read_bounded(file, limit)?;
    if is_binary(&bytes) {
        return Err(LinkError::Binary(path.to_path_buf()));
    }
    String::from_utf8(bytes).map_err(|_| LinkError::Binary(path.to_path_buf()))
}

/// Whether `bytes` look like binary content: a NUL anywhere in the first
/// [`BINARY_SNIFF_BYTES`]. The same heuristic `git` uses, and for the same
/// reason — a NUL is valid UTF-8, so the decode below cannot catch it.
fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(BINARY_SNIFF_BYTES).any(|&byte| byte == 0)
}

/// Re-reads a document this session **already has open** — the one
/// `Backspace` returns to, and the one a `--watch` tick reloads.
///
/// **This function exists because two entry points had diverged, which is the
/// shape of the defect it fixes.** `Navigator::follow` put every target
/// through `resolve_regular_file` + [`read_text_target`]; `Navigator::back`
/// called the command-line loader, which goes straight to `File::open`. A
/// security review reproduced the consequence on the real binary: open
/// `index.md`, follow a link to `second.md`, replace `index.md` with a FIFO,
/// press `Backspace` — and the event loop never returns. No key is read, no
/// frame painted, `q` and Ctrl-C inert, every stack sample inside `open()`.
/// The process had to be killed from outside. A viewer may refuse, and may
/// fail loudly, but it must never become unkillable from the keyboard.
///
/// So the way back is judged by the same rules as the way in — file type
/// before any read, a size ceiling, a NUL sniff — with one deliberate
/// difference: `limit` is the ceiling that document's *provenance* earned
/// (see [`StackedDocument::limit`]), not a constant. Re-resolving rather than
/// merely re-reading is the point: the path was canonicalized when it was
/// pushed, but the **object at it** may have been replaced since, and that
/// replacement is exactly the attack.
///
/// A relative path is resolved against the process's working directory,
/// which is where the command line's own path was relative to. Nothing here
/// ever changes directory.
pub fn reread_document(
    source: &DocumentSource,
    limit: u64,
    options: LoadOptions,
) -> Result<LoadedDocument, LinkError> {
    let path = match source {
        DocumentSource::Path(path) => path,
        // A drained pipe would `read_to_end` into an empty document and
        // render a blank screen with no explanation. Refusing says the true
        // thing instead, and matches `--watch -`'s CLI-parse refusal.
        DocumentSource::Stdin => return Err(LinkError::StreamNotRereadable),
    };
    let resolved = resolve_regular_file(path, Path::new("."), limit)?;
    let text = read_text_target(&resolved, limit)?;
    Ok(crate::loader::document_from_text(
        &text,
        source.display_name(),
        options,
    ))
}

/// Refuses a source that is not a regular file, **without opening it**.
///
/// The guard the `--watch` reload needs, and the narrowest thing that closes
/// the same hang [`reread_document`] documents. A reload runs *inside* the
/// event loop with the terminal in raw mode, where Ctrl-C is an ordinary
/// keystroke rather than a signal — so a `File::open` that blocks there is
/// exactly as unescapable as the one on the `Backspace` path, and reachable
/// by replacing a watched file with a FIFO.
///
/// Deliberately *only* the type check, so every message the reload path
/// already produces for a missing, unreadable or oversized file is unchanged.
/// The startup load is left alone on purpose: it happens before raw mode, so
/// Ctrl-C still works there, and `stele <(curl …)` — a process substitution,
/// which really is a FIFO — is a legitimate way to open a document that this
/// guard would otherwise take away.
pub fn refuse_unless_regular_file(source: &DocumentSource) -> Result<(), LinkError> {
    let path = match source {
        DocumentSource::Path(path) => path,
        DocumentSource::Stdin => return Ok(()),
    };
    // A path that cannot be stat'ed is *not* refused here: it is almost
    // certainly a deleted file, and the caller's own load reports that with
    // the message it has always used.
    match std::fs::metadata(path) {
        Ok(meta) if !meta.is_file() => Err(LinkError::NotAFile(path.clone())),
        Ok(_) | Err(_) => Ok(()),
    }
}

/// One document the reader came from, and where they were in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackedDocument {
    pub source: DocumentSource,
    /// The scroll offset to restore on the way back (DW-6.2).
    pub scroll: usize,
    /// The size ceiling this document was admitted under, carried so the way
    /// back is judged by the same rule as the way in.
    ///
    /// Entry 0 is the document the reader named on the command line and is
    /// worth [`crate::loader::MAX_DOCUMENT_BYTES`]; everything above it was
    /// reached by following a link and is worth [`MAX_LINK_FILE_BYTES`].
    /// Without this, `Backspace` out of a link would refuse to reopen a
    /// 20 MiB document the reader had opened deliberately a moment earlier.
    limit: u64,
}

/// The reader's link history: `Enter` pushes, `Backspace` pops.
///
/// Bounded at [`MAX_STACK_DEPTH`]. The bound refuses the *push* rather than
/// dropping the oldest entry, because dropping would silently break the one
/// promise the stack makes — that `Backspace` gets you back where you were.
#[derive(Debug, Default)]
pub struct DocumentStack {
    entries: Vec<StackedDocument>,
}

impl DocumentStack {
    pub fn new() -> Self {
        DocumentStack {
            entries: Vec::new(),
        }
    }

    /// Records `source` at `scroll` as the document to return to.
    ///
    /// The size ceiling is derived rather than passed: the first entry pushed
    /// is by construction the document the session started on — the one named
    /// on the command line — and every later one was reached by following a
    /// link from it.
    pub fn push(&mut self, source: DocumentSource, scroll: usize) -> Result<(), LinkError> {
        if self.entries.len() >= MAX_STACK_DEPTH {
            return Err(LinkError::StackTooDeep {
                limit: MAX_STACK_DEPTH,
            });
        }
        let limit = if self.entries.is_empty() {
            crate::loader::MAX_DOCUMENT_BYTES
        } else {
            MAX_LINK_FILE_BYTES
        };
        self.entries.push(StackedDocument {
            source,
            scroll,
            limit,
        });
        Ok(())
    }

    /// The most recent entry, or `None` at the root document.
    pub fn pop(&mut self) -> Option<StackedDocument> {
        self.entries.pop()
    }

    pub fn depth(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The seam to the OS's URL handler.
///
/// Declared here — in the layer that decides *whether* a URL may be opened —
/// and implemented by [`SystemOpener`] in the layer that knows *how*. That
/// inversion is what lets DW-6.3 be proved on the exact argv a URL produces
/// rather than on a browser window nobody can assert about.
pub trait UrlOpener {
    /// Hands `url` to the OS. `url` has already passed [`validated_url`].
    fn open(&mut self, url: &str) -> Result<(), LinkError>;
}

/// The program and argument vector `url` is opened with.
///
/// One element, the URL itself, and no shell anywhere: the returned pair is
/// spawned through `std::process::Command`, which `execvp`s directly. Shell
/// metacharacters, command substitution and embedded newlines are therefore
/// not merely escaped — nothing on this path has a parser that would give them
/// meaning (DW-6.5).
///
/// A leading `-` cannot turn the URL into an option to `open`/`xdg-open`,
/// because [`validated_url`] has already required an `http`/`https` scheme.
pub fn opener_argv(url: &str) -> (&'static str, [String; 1]) {
    (OPENER_PROGRAM, [url.to_string()])
}

/// Opens URLs with the platform's own handler.
#[derive(Debug, Default)]
pub struct SystemOpener;

impl UrlOpener for SystemOpener {
    fn open(&mut self, url: &str) -> Result<(), LinkError> {
        let (program, args) = opener_argv(url);
        // stdio is nulled out on all three: the opener shares this process's
        // terminal, and a helper that prints a warning would paint it over the
        // alternate screen mid-frame.
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| LinkError::OpenerFailed(format!("{program}: {e}")))
    }
}

/// What following a link did.
#[derive(Debug)]
pub enum Followed {
    /// A local target was loaded and the previous document pushed. The caller
    /// installs `loaded` and switches to `source`.
    Opened {
        source: DocumentSource,
        loaded: LoadedDocument,
    },
    /// A URL was handed to the OS opener; nothing on screen changes.
    Handed,
}

/// Link activation and the document stack, above the terminal and below the
/// key table.
///
/// Owns the [`UrlOpener`] and the [`DocumentStack`], and nothing else — it has
/// no view of the viewport, the painter, or the terminal. `main.rs` calls it
/// with the document currently open and installs whatever comes back.
pub struct Navigator {
    stack: DocumentStack,
    opener: Box<dyn UrlOpener>,
    options: LoadOptions,
}

impl Navigator {
    pub fn new(opener: Box<dyn UrlOpener>, options: LoadOptions) -> Self {
        Navigator {
            stack: DocumentStack::new(),
            opener,
            options,
        }
    }

    pub fn depth(&self) -> usize {
        self.stack.depth()
    }

    /// Follows `href`, written in the document `current` at `current_scroll`.
    ///
    /// **Nothing is pushed until the target has loaded.** A refusal therefore
    /// leaves the stack and the open document exactly as they were, which is
    /// DW-6.4's "leaves the current document rendered" at the level it is
    /// actually decided.
    pub fn follow(
        &mut self,
        href: &str,
        current: &DocumentSource,
        current_scroll: usize,
    ) -> Result<Followed, LinkError> {
        let base = current.base_dir();
        match LinkTarget::classify(href)?.resolve(&base)? {
            LinkTarget::Url(url) => {
                self.opener.open(&url)?;
                Ok(Followed::Handed)
            }
            LinkTarget::LocalDoc(path) | LinkTarget::LocalFile(path) => {
                // Depth is checked before the read, so a stack at its ceiling
                // does not cost 8 MiB of I/O to say no.
                if self.stack.depth() >= MAX_STACK_DEPTH {
                    return Err(LinkError::StackTooDeep {
                        limit: MAX_STACK_DEPTH,
                    });
                }
                let text = read_text_target(&path, MAX_LINK_FILE_BYTES)?;
                let source = DocumentSource::Path(path);
                let loaded =
                    crate::loader::document_from_text(&text, source.display_name(), self.options);
                self.stack.push(current.clone(), current_scroll)?;
                Ok(Followed::Opened { source, loaded })
            }
        }
    }

    /// `Backspace`: the document one level up, at the scroll offset it was
    /// left at.
    ///
    /// A failed reload puts the entry **back** on the stack rather than
    /// swallowing it: the file may have been mid-write, and a reader who
    /// presses `Backspace` again should get another go rather than find their
    /// history one shorter for no visible reason.
    pub fn back(&mut self) -> Result<(DocumentSource, LoadedDocument, usize), LinkError> {
        let entry = self.stack.pop().ok_or(LinkError::AtRoot)?;
        match reread_document(&entry.source, entry.limit, self.options) {
            Ok(loaded) => Ok((entry.source, loaded, entry.scroll)),
            Err(err) => {
                // Restored field-by-field rather than through `push`, which
                // would re-derive `limit` from the stack's *current* depth —
                // and the entry has already been popped, so the root would
                // come back as a link-provenance entry.
                self.stack.entries.push(entry);
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    /// A [`UrlOpener`] that records instead of spawning.
    ///
    /// The log is behind an `Rc<RefCell<_>>` so the test keeps a handle after
    /// the opener has been moved into a [`Navigator`] — rather than putting a
    /// test-only accessor on the production trait, which would make every real
    /// implementation carry a method that exists for one assertion.
    #[derive(Debug, Default)]
    struct RecordingOpener {
        opened: Rc<RefCell<Vec<String>>>,
    }

    impl UrlOpener for RecordingOpener {
        fn open(&mut self, url: &str) -> Result<(), LinkError> {
            self.opened.borrow_mut().push(url.to_string());
            Ok(())
        }
    }

    /// A scratch directory unique to `tag`, so concurrently running tests
    /// cannot see each other's fixtures.
    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("stele-link-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).expect("write fixture");
        path
    }

    fn navigator() -> Navigator {
        Navigator::new(Box::new(RecordingOpener::default()), LoadOptions::default())
    }

    /// A navigator plus a live handle on everything its opener was handed.
    fn navigator_watching_the_opener() -> (Navigator, Rc<RefCell<Vec<String>>>) {
        let opener = RecordingOpener::default();
        let log = Rc::clone(&opener.opened);
        (
            Navigator::new(Box::new(opener), LoadOptions::default()),
            log,
        )
    }

    // ---------------------------------------------------------------- DW-6.3

    #[test]
    fn test_dw_6_3_an_http_link_reaches_the_opener_as_a_single_argv_element() {
        let url = "https://example.com/a b?q=1&r=2";
        let (program, args) = opener_argv(url);
        assert_eq!(args.len(), 1, "the URL must be exactly one argv element");
        assert_eq!(args[0], url, "the URL must reach the opener unmodified");
        assert_eq!(program, OPENER_PROGRAM);
    }

    #[test]
    fn test_dw_6_3_the_opener_argv_names_a_real_program_and_never_a_shell() {
        let (program, _) = opener_argv("http://example.com");
        for shell in ["sh", "bash", "zsh", "cmd", "cmd.exe", "powershell"] {
            assert_ne!(program, shell, "the opener must never be a shell");
        }
        assert!(
            program == "open" || program == "xdg-open",
            "unexpected opener program {program:?}"
        );
    }

    #[test]
    fn test_dw_6_3_a_non_http_scheme_is_refused_before_the_opener_is_touched() {
        let dir = scratch_dir("scheme-refusal");
        let doc = write(&dir, "index.md", "# hi\n");
        let source = DocumentSource::Path(doc);
        for href in [
            "mailto:someone@example.com",
            "file:///etc/hosts",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "ftp://example.com/x",
            "vbscript:evil()",
            "about:blank",
        ] {
            let mut nav = navigator();
            let err = nav
                .follow(href, &source, 0)
                .expect_err("non-http(s) schemes must be refused");
            assert!(
                matches!(err, LinkError::UnsupportedScheme(_)),
                "{href} produced {err:?}"
            );
            assert!(
                err.to_string().contains("http/https only"),
                "the reader must be told why: {err}"
            );
            assert_eq!(nav.depth(), 0, "a refusal must not touch the stack");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dw_6_3_http_and_https_are_the_only_schemes_that_reach_the_opener() {
        let dir = scratch_dir("scheme-allow");
        let doc = write(&dir, "index.md", "# hi\n");
        let source = DocumentSource::Path(doc);
        for href in ["http://example.com/x", "HTTPS://Example.COM/y"] {
            let mut nav = navigator();
            assert!(
                matches!(nav.follow(href, &source, 0), Ok(Followed::Handed)),
                "{href} must be handed to the opener"
            );
            assert_eq!(nav.depth(), 0, "a URL does not join the document stack");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The claim in its strongest form: a `Navigator` holding a real opener
    /// seam must have handed it **nothing at all** after a batch of hostile
    /// destinations, and exactly the URL — byte for byte — after a good one.
    ///
    /// The tests above assert that classification refuses; this one asserts
    /// that the refusal happens *upstream of the opener*, which is the part a
    /// reordering could break while every other test stayed green.
    #[test]
    fn test_dw_6_5_no_hostile_destination_ever_reaches_the_opener_seam() {
        let dir = scratch_dir("opener-seam");
        let doc = write(&dir, "index.md", "# hi\n");
        let source = DocumentSource::Path(doc);

        let (mut nav, opened) = navigator_watching_the_opener();

        for href in [
            "javascript:alert(1)",
            "data:text/html,evil",
            "mailto:a@example.com",
            "file:///etc/passwd",
            "https://example.com/\nrm -rf /",
            "https://example.com/\x00",
            "; rm -rf /",
            "$(reboot)",
            "`id`",
            "../../etc/passwd",
            "#fragment",
            "",
        ] {
            assert!(
                nav.follow(href, &source, 0).is_err(),
                "{href:?} must be refused"
            );
        }
        assert!(
            opened.borrow().is_empty(),
            "not one hostile destination may reach the opener, got {:?}",
            opened.borrow()
        );

        assert!(matches!(
            nav.follow("https://example.com/ok?a=1&b=2", &source, 0),
            Ok(Followed::Handed)
        ));
        assert_eq!(
            opened.borrow().as_slice(),
            ["https://example.com/ok?a=1&b=2".to_string()],
            "and a good URL must arrive unmodified — no stripping, no escaping"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---------------------------------------------------------------- DW-6.5

    #[test]
    fn test_dw_6_5_shell_metacharacters_in_a_target_never_reach_a_process() {
        let dir = scratch_dir("metachars");
        let doc = write(&dir, "index.md", "# hi\n");
        let source = DocumentSource::Path(doc);
        for href in [
            "; rm -rf /",
            "$(rm -rf /)",
            "`rm -rf /`",
            "| tee /tmp/pwned",
            "&& curl evil.example",
            "notes.md; rm -rf /",
            "../../$(whoami).md",
            "'; DROP TABLE docs; --",
        ] {
            let mut nav = navigator();
            let outcome = nav.follow(href, &source, 0);
            // Every one of these classifies as a *local path*, which is never
            // handed to a process at all — it is looked up on the filesystem
            // and reported missing. The claim is not "it was escaped"; it is
            // "no process was ever involved".
            let err = outcome.expect_err("none of these name a real file");
            assert!(
                matches!(err, LinkError::Missing(_)),
                "{href:?} must be refused as a missing path, got {err:?}"
            );
            assert_eq!(nav.depth(), 0);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dw_6_5_a_newline_inside_a_url_is_refused_rather_than_passed_to_the_opener() {
        for hostile in [
            "https://example.com/\nrm -rf /",
            "https://example.com/\r\nSet-Cookie: x",
            "http://example.com/\x00",
            "https://example.com/\x1b]52;c;cGF5bG9hZA==\x07",
        ] {
            let err = LinkTarget::classify(hostile)
                .expect("the scheme is allowed; the body is what is hostile")
                .resolve(Path::new("."))
                .expect_err("a control byte in a URL must be refused, not stripped");
            assert!(
                matches!(err, LinkError::MalformedUrl),
                "{hostile:?} produced {err:?}"
            );
        }
    }

    /// A URL whose *preview* was cleaned must not still open dirty.
    ///
    /// The OSC 8 sanitizer strips bidi controls from the URI it prints, so
    /// the destination on hover reads honestly. This path validates the raw
    /// dest off the AST instead, and if it accepted what the printer cleaned,
    /// a link would preview as one destination and activate another — the
    /// spoof surviving into the only moment that has consequences.
    #[test]
    fn test_a_bidi_spoofed_url_is_refused_so_preview_and_activation_agree() {
        for hostile in [
            // Override: everything after it renders reversed.
            "https://example.com/\u{202e}exe.dm",
            // Isolate pair: hides a span from the surrounding direction.
            "https://example.com/\u{2066}safe.md\u{2069}/evil.exe",
            // Deprecated format control: invisible, and no browser wants it.
            "https://example.com/\u{206b}payload",
        ] {
            let err = LinkTarget::classify(hostile)
                .expect("the scheme is allowed; the body is what is hostile")
                .resolve(Path::new("."))
                .expect_err("a bidi control in a URL must be refused, not opened");
            assert!(
                matches!(err, LinkError::MalformedUrl),
                "{hostile:?} produced {err:?}"
            );
        }
    }

    /// The refusal above must not have been bought by rejecting RTL URLs
    /// wholesale. Directional *marks* are legitimate in a URL that names an
    /// Arabic or Hebrew resource, and they cannot mount the spoof.
    #[test]
    fn test_a_url_carrying_a_directional_mark_still_opens() {
        let url = "https://example.com/\u{200f}מסמך.md";
        let resolved = LinkTarget::classify(url)
            .expect("scheme is fine")
            .resolve(Path::new("."))
            .expect("a directional mark is not a spoof and must not be refused");
        assert!(
            matches!(&resolved, LinkTarget::Url(u) if u.contains('\u{200f}')),
            "the mark must survive validation intact, got {resolved:?}"
        );
    }

    #[test]
    fn test_dw_6_5_a_url_past_the_length_ceiling_is_refused() {
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL_BYTES));
        let err = LinkTarget::classify(&long)
            .expect("scheme is fine")
            .resolve(Path::new("."))
            .expect_err("an over-long URL must be refused");
        assert!(matches!(err, LinkError::MalformedUrl));
    }

    #[test]
    fn test_dw_6_5_a_traversal_path_that_resolves_to_a_readable_file_opens() {
        // The chosen policy: `../` is a legitimate way to reach a sibling
        // directory, not an attack to refuse. This test fails if someone
        // "hardens" the resolver into a directory jail.
        let dir = scratch_dir("traversal");
        let inner = dir.join("chapters");
        std::fs::create_dir_all(&inner).expect("create nested dir");
        write(&dir, "outside.md", "# Outside\n\nreachable\n");
        let doc = write(&inner, "index.md", "# Inner\n");

        let mut nav = navigator();
        let source = DocumentSource::Path(doc);
        let followed = nav
            .follow("../outside.md", &source, 7)
            .expect("a `../` target that resolves to a readable file must open");
        match followed {
            Followed::Opened { source, loaded } => {
                assert!(source.display_name().ends_with("outside.md"));
                assert_eq!(loaded.doc.blocks().len(), 2);
            }
            Followed::Handed => panic!("a local path must not reach the opener"),
        }
        assert_eq!(nav.depth(), 1, "the traversal target joins the stack");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn test_dw_6_5_a_symlink_pointing_outside_the_base_directory_opens() {
        let dir = scratch_dir("symlink");
        let target_dir = dir.join("elsewhere");
        let base_dir = dir.join("docs");
        std::fs::create_dir_all(&target_dir).expect("create target dir");
        std::fs::create_dir_all(&base_dir).expect("create base dir");
        write(&target_dir, "real.md", "# Real\n\nbody\n");
        let doc = write(&base_dir, "index.md", "# Index\n");
        std::os::unix::fs::symlink(target_dir.join("real.md"), base_dir.join("link.md"))
            .expect("create symlink");

        let mut nav = navigator();
        let followed = nav
            .follow("link.md", &DocumentSource::Path(doc), 0)
            .expect("a symlink out of the base directory must open, per the chosen policy");
        match followed {
            Followed::Opened { source, .. } => assert!(
                source.display_name().contains("elsewhere"),
                "the canonical path must be the symlink's target: {}",
                source.display_name()
            ),
            Followed::Handed => panic!("a local path must not reach the opener"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---------------------------------------------------------------- DW-6.4

    #[test]
    fn test_dw_6_4_a_missing_target_is_refused() {
        let dir = scratch_dir("missing");
        let doc = write(&dir, "index.md", "# hi\n");
        let mut nav = navigator();
        let err = nav
            .follow("nope.md", &DocumentSource::Path(doc), 0)
            .expect_err("a missing target must be refused");
        assert!(matches!(err, LinkError::Missing(_)), "{err:?}");
        assert!(err.to_string().starts_with("no such file:"));
        assert_eq!(nav.depth(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dw_6_4_a_directory_target_is_refused() {
        let dir = scratch_dir("directory");
        std::fs::create_dir_all(dir.join("chapters")).expect("create dir");
        let doc = write(&dir, "index.md", "# hi\n");
        let mut nav = navigator();
        let err = nav
            .follow("chapters", &DocumentSource::Path(doc), 0)
            .expect_err("a directory must be refused");
        assert!(matches!(err, LinkError::NotAFile(_)), "{err:?}");
        assert_eq!(nav.depth(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The FIFO case, and the reason the type check comes before the read:
    /// `File::open` on a FIFO with no writer blocks forever, so a resolver
    /// that opened first and asked questions later would wedge the viewer on
    /// a keystroke. The watchdog is the assertion — a hang fails this test
    /// instead of hanging the suite.
    #[test]
    #[cfg(unix)]
    fn test_dw_6_4_a_fifo_target_is_refused_without_ever_opening_it() {
        use std::os::unix::fs::FileTypeExt as _;

        let dir = scratch_dir("fifo");
        let fifo = dir.join("pipe");
        let made = Command::new("mkfifo")
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
        let doc = write(&dir, "index.md", "# hi\n");

        let (tx, rx) = mpsc::channel();
        let source = DocumentSource::Path(doc);
        std::thread::spawn(move || {
            let mut nav = navigator();
            let outcome = nav.follow("pipe", &source, 0);
            let _ = tx.send(match outcome {
                Ok(_) => "opened".to_string(),
                Err(err) => format!("{err:?}"),
            });
        });
        let answer = rx.recv_timeout(Duration::from_secs(5)).expect(
            "following a FIFO must return promptly — a blocking open is the bug this test exists \
             for",
        );
        assert!(
            answer.starts_with("NotAFile"),
            "a FIFO must be refused by type, got {answer}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A character device is refused for the same reason and by the same
    /// check: reading `/dev/zero` returns bytes forever, so only a `stat` can
    /// answer this question in finite time.
    #[test]
    #[cfg(unix)]
    fn test_dw_6_4_a_character_device_is_refused_by_type_not_by_read() {
        let dir = scratch_dir("chardev");
        let doc = write(&dir, "index.md", "# hi\n");
        let (tx, rx) = mpsc::channel();
        let source = DocumentSource::Path(doc);
        std::thread::spawn(move || {
            let mut nav = navigator();
            let outcome = nav.follow("/dev/zero", &source, 0);
            let _ = tx.send(match outcome {
                Ok(_) => "opened".to_string(),
                Err(err) => format!("{err:?}"),
            });
        });
        let answer = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("following /dev/zero must return promptly, not read forever");
        assert!(
            answer.starts_with("NotAFile"),
            "a character device must be refused by type, got {answer}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn test_dw_6_4_an_unreadable_file_is_refused() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = scratch_dir("unreadable");
        let secret = write(&dir, "secret.md", "# secret\n");
        std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o000))
            .expect("chmod 000");
        let doc = write(&dir, "index.md", "# hi\n");

        // Running as root defeats the fixture; skip rather than assert a
        // falsehood about the code.
        if std::fs::read(&secret).is_ok() {
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }

        let mut nav = navigator();
        let err = nav
            .follow("secret.md", &DocumentSource::Path(doc), 0)
            .expect_err("an unreadable file must be refused");
        assert!(matches!(err, LinkError::Io(_)), "{err:?}");
        assert!(err.to_string().starts_with("could not read linked file:"));
        assert_eq!(nav.depth(), 0);
        let _ = std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dw_6_4_an_oversized_file_is_refused_by_its_stat_size() {
        let dir = scratch_dir("oversize");
        let big = dir.join("big.md");
        let file = std::fs::File::create(&big).expect("create sparse file");
        file.set_len(MAX_LINK_FILE_BYTES + 1).expect("set_len");
        drop(file);
        let doc = write(&dir, "index.md", "# hi\n");

        let mut nav = navigator();
        let err = nav
            .follow("big.md", &DocumentSource::Path(doc), 0)
            .expect_err("an oversized target must be refused");
        assert!(
            matches!(err, LinkError::TooLarge { limit } if limit == MAX_LINK_FILE_BYTES),
            "{err:?}"
        );
        assert!(err.to_string().contains("8 MiB"), "message was: {err}");
        assert_eq!(nav.depth(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dw_6_4_a_file_exactly_at_the_link_ceiling_still_opens() {
        let dir = scratch_dir("atlimit");
        let big = dir.join("big.md");
        let file = std::fs::File::create(&big).expect("create sparse file");
        file.set_len(MAX_LINK_FILE_BYTES).expect("set_len");
        drop(file);
        let doc = write(&dir, "index.md", "# hi\n");

        // A sparse file of NULs is binary by the sniff, which is the *other*
        // refusal — so the size check must be what passes here, and the error
        // must be the binary one rather than TooLarge.
        let mut nav = navigator();
        let err = nav
            .follow("big.md", &DocumentSource::Path(doc), 0)
            .expect_err("a file of NUL bytes is binary");
        assert!(
            matches!(err, LinkError::Binary(_)),
            "exactly at the ceiling must pass the size gate and be refused for its content \
             instead, got {err:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dw_6_4_a_binary_file_is_refused_before_it_is_parsed() {
        let dir = scratch_dir("binary");
        std::fs::write(
            dir.join("blob.md"),
            [0x7f, 0x45, 0x4c, 0x46, 0x00, 0x01, 0x02],
        )
        .expect("write binary fixture");
        let doc = write(&dir, "index.md", "# hi\n");

        let mut nav = navigator();
        let err = nav
            .follow("blob.md", &DocumentSource::Path(doc), 0)
            .expect_err("binary content must be refused");
        assert!(matches!(err, LinkError::Binary(_)), "{err:?}");
        assert!(err.to_string().starts_with("refusing to open binary"));
        assert_eq!(nav.depth(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dw_6_4_invalid_utf8_that_carries_no_nul_is_still_refused() {
        let dir = scratch_dir("badutf8");
        std::fs::write(dir.join("bad.txt"), [0x68, 0x69, 0xff, 0xfe]).expect("write fixture");
        let doc = write(&dir, "index.md", "# hi\n");

        let mut nav = navigator();
        let err = nav
            .follow("bad.txt", &DocumentSource::Path(doc), 0)
            .expect_err("non-UTF-8 content must be refused");
        assert!(matches!(err, LinkError::Binary(_)), "{err:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dw_6_4_a_refused_target_leaves_the_stack_and_the_document_untouched() {
        let dir = scratch_dir("refusal-stack");
        let first = write(&dir, "one.md", "# One\n");
        write(&dir, "two.md", "# Two\n");
        let mut nav = navigator();
        let source = DocumentSource::Path(first);

        // One good hop, so there is a stack to be damaged.
        assert!(matches!(
            nav.follow("two.md", &source, 4),
            Ok(Followed::Opened { .. })
        ));
        assert_eq!(nav.depth(), 1);

        for href in ["nope.md", "javascript:alert(1)", "#anchor", "   "] {
            let second = DocumentSource::Path(dir.join("two.md"));
            assert!(
                nav.follow(href, &second, 9).is_err(),
                "{href} must be refused"
            );
            assert_eq!(nav.depth(), 1, "{href} disturbed the stack");
        }

        let (back, _, scroll) = nav.back().expect("the one good hop is still poppable");
        assert_eq!(scroll, 4, "the original scroll offset must survive");
        assert!(back.display_name().ends_with("one.md"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---------------------------------------------------------------- DW-6.2

    #[test]
    fn test_dw_6_2_following_a_relative_markdown_link_pushes_the_current_document() {
        let dir = scratch_dir("relative-md");
        let first = write(&dir, "one.md", "# One\n\n[two](two.md)\n");
        write(&dir, "two.md", "# Two\n\nsecond document\n");

        let mut nav = navigator();
        let followed = nav
            .follow("two.md", &DocumentSource::Path(first), 12)
            .expect("a relative markdown link must open");
        match followed {
            Followed::Opened { source, loaded } => {
                assert!(source.display_name().ends_with("two.md"));
                assert_eq!(loaded.doc.blocks().len(), 2);
                assert!(loaded.info.name.ends_with("two.md"));
            }
            Followed::Handed => panic!("a local markdown link must not reach the opener"),
        }
        assert_eq!(nav.depth(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dw_6_2_back_pops_to_the_previous_source_and_its_scroll() {
        let dir = scratch_dir("back");
        let first = write(&dir, "one.md", "# One\n");
        write(&dir, "two.md", "# Two\n");

        let mut nav = navigator();
        nav.follow("two.md", &DocumentSource::Path(first.clone()), 31)
            .expect("hop");
        let (source, loaded, scroll) = nav.back().expect("back must return the previous document");
        assert_eq!(source, DocumentSource::Path(first));
        assert_eq!(scroll, 31, "DW-6.2: the previous scroll position");
        assert_eq!(loaded.doc.blocks().len(), 1);
        assert!(nav.stack.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dw_6_2_back_at_the_root_reports_instead_of_popping() {
        let mut nav = navigator();
        let err = nav.back().expect_err("nothing to go back to");
        assert!(matches!(err, LinkError::AtRoot));
        assert_eq!(err.to_string(), "already at the first document");
        assert_eq!(nav.depth(), 0);
    }

    #[test]
    fn test_dw_6_2_a_link_to_the_open_document_opens_it_again_and_back_returns() {
        let dir = scratch_dir("self-link");
        let doc = write(&dir, "self.md", "# Self\n\n[me](self.md)\n");
        let source = DocumentSource::Path(doc);

        let mut nav = navigator();
        for depth in 1..=3 {
            assert!(matches!(
                nav.follow("self.md", &source, depth),
                Ok(Followed::Opened { .. })
            ));
            assert_eq!(nav.depth(), depth);
        }
        for depth in (1..=3).rev() {
            let (_, _, scroll) = nav.back().expect("each self-hop must be poppable");
            assert_eq!(scroll, depth);
        }
        assert!(nav.back().is_err(), "and then the root");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dw_6_2_a_stack_at_its_ceiling_refuses_the_next_hop_and_stays_poppable() {
        let dir = scratch_dir("deep-stack");
        let doc = write(&dir, "self.md", "# Self\n");
        let source = DocumentSource::Path(doc);

        let mut nav = navigator();
        for _ in 0..MAX_STACK_DEPTH {
            assert!(nav.follow("self.md", &source, 0).is_ok());
        }
        assert_eq!(nav.depth(), MAX_STACK_DEPTH);
        let err = nav
            .follow("self.md", &source, 0)
            .expect_err("the ceiling must refuse");
        assert!(
            matches!(err, LinkError::StackTooDeep { limit } if limit == MAX_STACK_DEPTH),
            "{err:?}"
        );
        assert_eq!(nav.depth(), MAX_STACK_DEPTH, "and drop nothing");
        assert!(nav.back().is_ok(), "the history must still work");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_a_back_whose_file_vanished_keeps_the_entry_for_another_try() {
        let dir = scratch_dir("back-vanished");
        let first = write(&dir, "one.md", "# One\n");
        write(&dir, "two.md", "# Two\n");

        let mut nav = navigator();
        nav.follow("two.md", &DocumentSource::Path(first.clone()), 5)
            .expect("hop");
        std::fs::remove_file(&first).expect("delete the document behind us");

        let err = nav.back().expect_err("the previous document is gone");
        // `Missing`, not `Io`: the way back now re-*resolves* the path rather
        // than re-reading it blind, so a deleted parent is reported as the
        // missing file it is.
        assert!(matches!(err, LinkError::Missing(_)), "{err:?}");
        assert_eq!(nav.depth(), 1, "the entry must stay for another attempt");

        std::fs::write(&first, "# One again\n").expect("restore");
        let (_, _, scroll) = nav.back().expect("and the retry works");
        assert_eq!(scroll, 5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ----------------- the way back is the way in (security review) -------

    /// Sets up `index.md` → `second.md`, follows the link, then hands the
    /// parent path to `sabotage` before `back()` is called. Returns whatever
    /// `back()` produced, **through a watchdog** — a hang is a test failure
    /// rather than a wedged suite, which is the whole point.
    fn back_after_sabotage(
        tag: &str,
        sabotage: impl FnOnce(&Path) + Send + 'static,
    ) -> Result<String, String> {
        let dir = scratch_dir(tag);
        let parent = write(&dir, "index.md", "# Index\n\n[two](second.md)\n");
        write(&dir, "second.md", "# Second\n\nchild body.\n");

        let (tx, rx) = mpsc::channel();
        let dir_for_thread = dir.clone();
        std::thread::spawn(move || {
            let mut nav = navigator();
            let source = DocumentSource::Path(dir_for_thread.join("index.md"));
            nav.follow("second.md", &source, 11)
                .expect("the child must open before the parent is sabotaged");
            sabotage(&parent);
            let answer = match nav.back() {
                Ok((_, loaded, scroll)) => Ok(format!(
                    "opened {} blocks at {scroll}",
                    loaded.doc.blocks().len()
                )),
                Err(err) => Err(format!("{err:?}")),
            };
            let _ = tx.send(answer);
        });

        let answer = rx.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| {
            panic!(
                "`back()` did not return within 5s — this is the security review's \
                 blocker: the event loop never comes back, no key is read, and the \
                 process has to be killed from outside"
            )
        });
        let _ = std::fs::remove_dir_all(&dir);
        answer
    }

    /// **The blocker, pinned.** A parent replaced by a FIFO between `follow`
    /// and `back` must be refused, not opened — `File::open` on a FIFO with
    /// no writer blocks forever, and this runs inside the event loop where
    /// Ctrl-C is a keystroke rather than a signal.
    #[test]
    #[cfg(unix)]
    fn test_dw_6_4_a_parent_replaced_by_a_fifo_between_follow_and_back_is_refused_not_opened() {
        let answer = back_after_sabotage("back-fifo", |parent| {
            std::fs::remove_file(parent).expect("remove the parent");
            let made = Command::new("mkfifo")
                .arg(parent)
                .status()
                .expect("run mkfifo");
            assert!(made.success(), "mkfifo failed: {made:?}");
        });
        let err = answer.expect_err("a FIFO parent must be refused");
        assert!(
            err.starts_with("NotAFile"),
            "the way back must type-check before it opens, got {err}"
        );
    }

    /// The same seam, the device case: a parent symlinked to `/dev/zero`
    /// would read to the ceiling before deciding anything.
    #[test]
    #[cfg(unix)]
    fn test_dw_6_4_a_parent_replaced_by_a_device_between_follow_and_back_is_refused_by_type() {
        let answer = back_after_sabotage("back-device", |parent| {
            std::fs::remove_file(parent).expect("remove the parent");
            std::os::unix::fs::symlink("/dev/zero", parent).expect("symlink to /dev/zero");
        });
        let err = answer.expect_err("a device parent must be refused");
        assert!(err.starts_with("NotAFile"), "got {err}");
    }

    /// Binary content on the way back is refused exactly as it is on the way
    /// in — the way back used to hand it straight to the markdown parser.
    #[test]
    fn test_dw_6_4_a_parent_that_turned_binary_between_follow_and_back_is_refused() {
        let answer = back_after_sabotage("back-binary", |parent| {
            std::fs::write(parent, [0x00, 0x01, 0x02, 0x03]).expect("write binary");
        });
        let err = answer.expect_err("binary content must be refused");
        assert!(err.starts_with("Binary"), "got {err}");
    }

    /// ...and the size ceiling applies both ways. The parent here is entry 0,
    /// so it is judged at the command line's 64 MiB rather than a link's
    /// 8 MiB — the next test is the other half of that rule.
    #[test]
    fn test_dw_6_4_a_parent_that_outgrew_the_ceiling_between_follow_and_back_is_refused() {
        let answer = back_after_sabotage("back-oversize", |parent| {
            let file = std::fs::File::create(parent).expect("recreate the parent");
            file.set_len(crate::loader::MAX_DOCUMENT_BYTES + 1)
                .expect("set_len");
        });
        let err = answer.expect_err("an oversized parent must be refused");
        assert!(err.starts_with("TooLarge"), "got {err}");
    }

    /// **The provenance rule.** Entry 0 is the document the reader named on
    /// the command line, so going back to it is judged at
    /// `MAX_DOCUMENT_BYTES` — not the tighter link ceiling. Without this a
    /// reader who opened a 20 MiB document, followed a link, and pressed
    /// `Backspace` would be refused their own file.
    #[test]
    fn test_the_command_line_document_is_still_reachable_above_the_link_ceiling() {
        let dir = scratch_dir("back-provenance");
        let parent = dir.join("index.md");
        // Comfortably past the 8 MiB link ceiling, well under 64 MiB, and
        // real text rather than a sparse hole so the NUL sniff has to pass.
        let body = "lorem ipsum dolor sit amet\n".repeat(400_000);
        assert!(body.len() as u64 > MAX_LINK_FILE_BYTES);
        std::fs::write(&parent, &body).expect("write a large parent");
        write(&dir, "second.md", "# Second\n");

        let mut nav = navigator();
        let source = DocumentSource::Path(parent);
        nav.follow("second.md", &source, 5).expect("hop");
        let (_, loaded, scroll) = nav
            .back()
            .expect("the reader's own command-line document must still open");
        assert_eq!(scroll, 5);
        // Consecutive non-blank lines are one paragraph, so the block count
        // is 1; the byte size is what makes the point.
        assert!(
            loaded.info.byte_size > MAX_LINK_FILE_BYTES,
            "the reopened document must really be past the link ceiling, was {}",
            loaded.info.byte_size
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The other half: a document reached *by a link* keeps the link ceiling
    /// on the way back too, so the tighter bound is not lost after one hop.
    #[test]
    fn test_a_linked_document_keeps_the_link_ceiling_on_the_way_back() {
        let dir = scratch_dir("back-link-ceiling");
        let root = write(&dir, "root.md", "# Root\n");
        write(&dir, "mid.md", "# Mid\n");
        write(&dir, "leaf.md", "# Leaf\n");

        let mut nav = navigator();
        nav.follow("mid.md", &DocumentSource::Path(root), 0)
            .expect("root -> mid");
        let mid = DocumentSource::Path(dir.join("mid.md"));
        nav.follow("leaf.md", &mid, 3).expect("mid -> leaf");
        assert_eq!(nav.depth(), 2);

        // `mid.md` was reached by a link, so it is admitted at the link
        // ceiling — grow it past that and going back must refuse.
        let file = std::fs::File::create(dir.join("mid.md")).expect("recreate mid");
        file.set_len(MAX_LINK_FILE_BYTES + 1).expect("set_len");
        drop(file);

        let err = nav.back().expect_err("mid outgrew the link ceiling");
        assert!(
            matches!(err, LinkError::TooLarge { limit } if limit == MAX_LINK_FILE_BYTES),
            "a linked document must keep the link ceiling on the way back: {err:?}"
        );
        assert_eq!(nav.depth(), 2, "and the entry stays for another try");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A stdin root cannot be re-read — the pipe is drained. Refusing says so
    /// instead of silently rendering the empty document a second
    /// `read_to_end` returns.
    #[test]
    fn test_going_back_to_a_stdin_document_reports_rather_than_rendering_nothing() {
        let dir = scratch_dir("back-stdin");
        write(&dir, "second.md", "# Second\n");
        let mut nav = navigator();
        // `base_dir()` for stdin is the working directory, so follow an
        // absolute path to reach the fixture.
        let target = dir.join("second.md");
        nav.follow(
            target.to_str().expect("utf-8 path"),
            &DocumentSource::Stdin,
            2,
        )
        .expect("a stdin document may still follow links");
        assert_eq!(nav.depth(), 1);

        let err = nav.back().expect_err("a drained pipe cannot be re-read");
        assert!(matches!(err, LinkError::StreamNotRereadable), "{err:?}");
        assert!(err.to_string().contains("read once to end"));
        assert_eq!(
            nav.depth(),
            1,
            "and the entry is kept, not silently dropped"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The `--watch` guard, which is the same hang reached by the other
    /// in-loop door: a watched file replaced by a FIFO must be refused before
    /// anything opens it.
    #[test]
    #[cfg(unix)]
    fn test_dw_6_4_the_reload_guard_refuses_a_watched_path_replaced_by_a_fifo() {
        let dir = scratch_dir("watch-fifo");
        let path = dir.join("doc.md");
        let made = Command::new("mkfifo").arg(&path).status().expect("mkfifo");
        assert!(made.success());

        let source = DocumentSource::Path(path);
        let err = refuse_unless_regular_file(&source).expect_err("a FIFO must be refused");
        assert!(matches!(err, LinkError::NotAFile(_)), "{err:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ...and the guard is deliberately narrow: it must not start refusing
    /// the ordinary cases whose messages the reload path already owns.
    #[test]
    fn test_the_reload_guard_passes_regular_files_and_leaves_missing_ones_to_the_loader() {
        let dir = scratch_dir("watch-guard-narrow");
        let path = write(&dir, "doc.md", "# hi\n");
        assert!(refuse_unless_regular_file(&DocumentSource::Path(path.clone())).is_ok());

        std::fs::remove_file(&path).expect("delete it");
        assert!(
            refuse_unless_regular_file(&DocumentSource::Path(path)).is_ok(),
            "a deleted file is the loader's error to report, with the message \
             `--watch` has always used"
        );
        assert!(refuse_unless_regular_file(&DocumentSource::Stdin).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---------------------------------------------------------------- DW-6.8

    #[test]
    fn test_dw_6_8_a_txt_target_opens_in_place_and_joins_the_document_stack() {
        let dir = scratch_dir("txt-target");
        let doc = write(&dir, "index.md", "# Index\n");
        write(&dir, "notes.txt", "plain notes\n\nsecond paragraph\n");

        let mut nav = navigator();
        let followed = nav
            .follow("notes.txt", &DocumentSource::Path(doc), 3)
            .expect("a .txt target must open — the permissive policy's third leg");
        match followed {
            Followed::Opened { source, loaded } => {
                assert!(source.display_name().ends_with("notes.txt"));
                assert_eq!(loaded.doc.blocks().len(), 2);
            }
            Followed::Handed => panic!("a local file must not reach the opener"),
        }
        assert_eq!(nav.depth(), 1, "DW-6.8: it joins the stack like markdown");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dw_6_8_a_rs_target_opens_in_place_and_joins_the_document_stack() {
        let dir = scratch_dir("rs-target");
        let doc = write(&dir, "index.md", "# Index\n");
        write(&dir, "main.rs", "fn main() {\n    println!(\"hi\");\n}\n");

        let mut nav = navigator();
        assert!(matches!(
            nav.follow("main.rs", &DocumentSource::Path(doc), 0),
            Ok(Followed::Opened { .. })
        ));
        assert_eq!(nav.depth(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dw_6_8_an_extensionless_readable_file_opens() {
        let dir = scratch_dir("no-extension");
        let doc = write(&dir, "index.md", "# Index\n");
        write(&dir, "LICENSE", "MIT License\n");

        let mut nav = navigator();
        assert!(matches!(
            nav.follow("LICENSE", &DocumentSource::Path(doc), 0),
            Ok(Followed::Opened { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dw_6_8_back_from_a_non_markdown_document_returns_to_the_markdown_one() {
        let dir = scratch_dir("txt-back");
        let doc = write(&dir, "index.md", "# Index\n");
        write(&dir, "notes.txt", "plain notes\n");

        let mut nav = navigator();
        nav.follow("notes.txt", &DocumentSource::Path(doc.clone()), 17)
            .expect("hop into the .txt");
        let (source, loaded, scroll) = nav.back().expect("back out of it");
        assert_eq!(source, DocumentSource::Path(doc));
        assert_eq!(scroll, 17);
        assert_eq!(loaded.doc.blocks().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ------------------------------------------------------- classification

    #[test]
    fn test_classification_splits_markdown_other_files_and_urls() {
        assert_eq!(
            LinkTarget::classify("notes.md").unwrap(),
            LinkTarget::LocalDoc(PathBuf::from("notes.md"))
        );
        assert_eq!(
            LinkTarget::classify("READ.MARKDOWN").unwrap(),
            LinkTarget::LocalDoc(PathBuf::from("READ.MARKDOWN"))
        );
        assert_eq!(
            LinkTarget::classify("src/main.rs").unwrap(),
            LinkTarget::LocalFile(PathBuf::from("src/main.rs"))
        );
        assert_eq!(
            LinkTarget::classify("/etc/hosts").unwrap(),
            LinkTarget::LocalFile(PathBuf::from("/etc/hosts"))
        );
        assert_eq!(
            LinkTarget::classify("https://example.com").unwrap(),
            LinkTarget::Url("https://example.com".to_string())
        );
    }

    #[test]
    fn test_an_empty_or_fragment_only_destination_is_refused() {
        assert!(matches!(LinkTarget::classify(""), Err(LinkError::Empty)));
        assert!(matches!(LinkTarget::classify("   "), Err(LinkError::Empty)));
        assert!(matches!(
            LinkTarget::classify("#section"),
            Err(LinkError::Fragment)
        ));
        assert!(matches!(
            LinkTarget::classify("?q=1"),
            Err(LinkError::Fragment)
        ));
    }

    #[test]
    fn test_a_query_or_fragment_suffix_is_dropped_from_a_local_path() {
        assert_eq!(
            LinkTarget::classify("notes.md#heading").unwrap(),
            LinkTarget::LocalDoc(PathBuf::from("notes.md"))
        );
        assert_eq!(
            LinkTarget::classify("notes.md?v=2").unwrap(),
            LinkTarget::LocalDoc(PathBuf::from("notes.md"))
        );
    }

    #[test]
    fn test_percent_escapes_in_a_local_path_are_decoded_but_a_nul_is_dropped() {
        assert_eq!(percent_decoded("my%20notes.md"), "my notes.md");
        assert_eq!(percent_decoded("100%"), "100%");
        assert_eq!(percent_decoded("a%zzb"), "a%zzb");
        assert_eq!(
            percent_decoded("evil%00.md"),
            "evil.md",
            "a decoded NUL would truncate the path at the syscall boundary"
        );
    }

    #[test]
    fn test_a_scheme_like_prefix_that_is_not_a_scheme_stays_a_path() {
        // The grammar rejects a leading digit and an underscore, so these are
        // paths rather than URLs — and are looked up, not refused.
        assert_eq!(
            LinkTarget::classify("2026:notes.md").unwrap(),
            LinkTarget::LocalDoc(PathBuf::from("2026:notes.md"))
        );
        assert_eq!(
            LinkTarget::classify("my_scheme:x").unwrap(),
            LinkTarget::LocalFile(PathBuf::from("my_scheme:x"))
        );
    }

    #[test]
    fn test_a_bare_scheme_with_no_target_is_refused() {
        let err = LinkTarget::classify("https:")
            .expect("classified as a URL")
            .resolve(Path::new("."))
            .expect_err("a scheme with nothing after it opens nothing");
        assert!(matches!(err, LinkError::MalformedUrl));
    }

    #[test]
    fn test_the_binary_sniff_looks_only_at_a_bounded_prefix() {
        let mut head = vec![b'a'; BINARY_SNIFF_BYTES];
        head.push(0);
        assert!(
            !is_binary(&head),
            "a NUL past the sniff window is not what this heuristic claims to catch"
        );
        let mut early = vec![b'a'; 16];
        early.push(0);
        assert!(is_binary(&early));
        assert!(!is_binary(b"# ordinary markdown\n"));
    }

    #[test]
    fn test_the_document_stack_bounds_itself_and_pops_in_order() {
        let mut stack = DocumentStack::new();
        assert!(stack.is_empty());
        for i in 0..MAX_STACK_DEPTH {
            stack
                .push(DocumentSource::Path(PathBuf::from(format!("{i}.md"))), i)
                .expect("within the ceiling");
        }
        assert!(matches!(
            stack.push(DocumentSource::Stdin, 0),
            Err(LinkError::StackTooDeep { .. })
        ));
        for i in (0..MAX_STACK_DEPTH).rev() {
            let entry = stack.pop().expect("in reverse order");
            assert_eq!(entry.scroll, i);
        }
        assert!(stack.pop().is_none());
    }
}
