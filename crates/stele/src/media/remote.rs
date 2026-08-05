//! `![alt](https://host/pic.png)` → `![alt](</cache/…/<key>>)`, before the
//! parse, and only when the reader asked for it.
//!
//! ## Why this is a source-text preprocessor and not a sizer
//!
//! `crates/layout` is pure by contract — its own module doc opens with "no
//! I/O, no clock, no global or hidden state" (`crates/layout/src/lib.rs:1-11`)
//! — and `IntrinsicSizer::size` is called *during* layout, once per image, on
//! whatever thread is laying out. A network request cannot live there without
//! making layout non-deterministic, non-reproducible and slow, which would
//! cost far more than remote images are worth.
//!
//! So the resolution happens one stage earlier, in `loader::preprocess`, and
//! its output is an ordinary local path. Everything downstream — the
//! `is_local_image_path` check in `media::sizer`, `gfx::decode`'s format
//! sniffing (PNG, JPEG, GIF, WebP, BMP, ICO, TIFF *and* SVG), the reserved
//! box, the kitty transmission — is untouched and does not know a network
//! exists. A remote image and a local one differ only in how the file got
//! there.
//!
//! ## Default off, and what that means precisely
//!
//! [`crate::loader::LoadOptions::remote`] is `None` unless `--fetch-remote`
//! was typed, and `None` means this module's [`RemoteImages::rewrite`] is
//! never called at all — not called-and-declining. A markdown file that
//! fetches on open is a tracking vector: the URL, the timing and the IP are
//! all the author's to read, and the reader consented to none of it by opening
//! a document. Off is not a conservative default here, it is the correct one.
//!
//! ## How it composes with `--no-rewrite`
//!
//! **It does not disable it.** `--no-rewrite` turns off the d2l, Quarto and
//! CodeCogs passes, and every one of those *changes what the document says* —
//! it deletes Sphinx roles, reshapes callouts, and turns an equation image
//! into `$…$` maths. The flag exists for the reader who wants the file as
//! written. This pass changes nothing about what the document says: an image
//! stays an image with the same alt text, and only its destination moves from
//! a URL to the local copy of the bytes that URL served. Suppressing an
//! explicitly typed `--fetch-remote` because an unrelated flag is present
//! would be a surprise, and the two are not in contradiction the way `--watch`
//! and `-` are.
//!
//! One consequence follows and is deliberate: under `--no-rewrite` the
//! CodeCogs pass has not run, so a `latex.codecogs.com` image is still an
//! image and *is* fetched. That is coherent — the reader asked for the
//! document as written and for its images to be drawn, and what is written
//! there is an image of an equation.
//!
//! ## Failure is the ordinary case
//!
//! Every way a fetch can go wrong — a timeout, a 404, a body over the ceiling,
//! a redirect loop, a `file://` redirect target, bytes that are not an image —
//! ends with the URL left exactly as the author wrote it. `media::sizer`'s
//! `is_local_image_path` then rejects it for containing `://`, layout emits no
//! box, and the reader sees the alt text: byte for byte what they see today
//! with no flag at all. There is no failure path here that can stop a document
//! opening.

use std::borrow::Cow;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::decor::fences::{Scan, strip_eol};
use crate::media::cache::Cache;
use crate::media::fetch::{FetchError, FetchLimits, Fetched, Fetcher};

/// The two schemes a document may send this viewer to.
///
/// A list rather than "anything with a `://`" because the interesting attacks
/// are all the other schemes: `file:` reads the reader's disk, `data:` smuggles
/// bytes past every bound here, and `ftp:`/`gopher:` are code paths nobody
/// audits. Checked on the original URL *and* on every redirect target, which
/// is the hop a client that follows redirects for you cannot check.
const ALLOWED_SCHEMES: [&str; 2] = ["http", "https"];

/// The policy object: a fetcher, a cache, and the bounds both answer to.
///
/// Held behind a `&'static` in [`crate::loader::LoadOptions`] so that struct
/// stays `Copy` — it is passed by value through `link.rs`'s navigator and the
/// `--watch` reload, and making it `Clone`-only would ripple into files this
/// change has no business touching. A process has exactly one of these and it
/// lives as long as the process does, so `'static` costs nothing real.
pub struct RemoteImages {
    fetcher: Box<dyn Fetcher>,
    cache: Cache,
    limits: FetchLimits,
    /// The ceiling a downloaded image's *header* answers to, handed to
    /// `gfx::decode::probe_dimensions` when the cache validates it. Kept here
    /// rather than defaulted at the call site so a test can prove a
    /// bomb-dimension image is refused without generating one.
    decode_limits: gfx::Limits,
}

/// Hand-written because `Box<dyn Fetcher>` is not `Debug` and requiring it on
/// the trait would tax every fake for the sake of one derive.
impl fmt::Debug for RemoteImages {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteImages")
            .field("fetcher", &"<dyn Fetcher>")
            .field("cache", &self.cache)
            .field("limits", &self.limits)
            .finish()
    }
}

impl RemoteImages {
    pub fn new(fetcher: Box<dyn Fetcher>, cache: Cache, limits: FetchLimits) -> Self {
        RemoteImages {
            fetcher,
            cache,
            limits,
            decode_limits: gfx::Limits::default(),
        }
    }

    /// Uses `limits` for the downloaded image's dimension check instead of
    /// `gfx::Limits::default()`.
    pub fn with_decode_limits(mut self, limits: gfx::Limits) -> Self {
        self.decode_limits = limits;
        self
    }

    /// Every remote image destination in `source` replaced by the local file
    /// its bytes were cached to. Untouched — and borrowed, not copied — when
    /// there is nothing to replace, or when nothing could be resolved.
    ///
    /// Declines the whole document when a fence is left open, exactly as the
    /// other source passes do (`decor::fences`' R10 rule): a file caught
    /// mid-save cannot be said to have a code block anywhere in particular,
    /// and under `--watch` that state is reached on every editor write.
    /// Declining here has a second edge the other passes do not: guessing
    /// wrong would fetch a URL out of a code sample the author was only
    /// *quoting*.
    pub fn rewrite<'a>(&self, source: &'a str) -> Cow<'a, str> {
        let scan = Scan::of(source);
        if !scan.balanced() {
            return Cow::Borrowed(source);
        }
        // One deadline for the whole document, taken before the first request.
        // See `FetchLimits::document_budget`: without this, the worst case is
        // per-image rather than per-document, and a page of forty dead links
        // is forty timeouts long.
        let deadline = Instant::now() + self.limits.document_budget;

        let mut out = String::new();
        let mut rewrote = false;
        for (index, raw) in source.split_inclusive('\n').enumerate() {
            if scan.is_protected(index) {
                out.push_str(raw);
                continue;
            }
            let line = strip_eol(raw);
            match self.rewrite_line(line, deadline) {
                Some(rewritten) => {
                    rewrote = true;
                    out.push_str(&rewritten);
                    out.push_str(&raw[line.len()..]);
                }
                None => out.push_str(raw),
            }
        }

        if rewrote {
            Cow::Owned(out)
        } else {
            Cow::Borrowed(source)
        }
    }

    /// One line with its resolvable remote images repointed at the cache, or
    /// `None` when nothing on it changed.
    fn rewrite_line(&self, line: &str, deadline: Instant) -> Option<String> {
        let bytes = line.as_bytes();
        let mut out = String::new();
        let mut copied = 0usize;
        let mut cursor = 0usize;
        let mut rewrote = false;

        while cursor + 1 < bytes.len() {
            if bytes[cursor] != b'!' || bytes[cursor + 1] != b'[' {
                cursor += 1;
                continue;
            }
            let Some(image) = image_at(line, cursor) else {
                cursor += 1;
                continue;
            };
            let end = image.end;
            // Not a URL we fetch, or a fetch that failed: leave the image
            // exactly as the author wrote it and let it become alt text, which
            // is what it would have been anyway.
            let Ok(path) = self.resolve(image.dest, deadline) else {
                cursor = end;
                continue;
            };
            let Some(destination) = markdown_destination(&path) else {
                cursor = end;
                continue;
            };
            out.push_str(&line[copied..cursor]);
            out.push_str(&line[cursor..image.dest_start]);
            out.push_str(&destination);
            out.push_str(&line[image.dest_end..end]);
            copied = end;
            cursor = end;
            rewrote = true;
        }

        if !rewrote {
            return None;
        }
        out.push_str(&line[copied..]);
        Some(out)
    }

    /// `url`'s bytes as a local file: from the cache if they are there, from
    /// the network if they are not, and never at all if the scheme, the
    /// redirect chain, the clock or the bytes themselves say no.
    ///
    /// The redirect loop is here rather than inside the client for the reason
    /// `media::fetch`'s module doc gives: it is the only place the scheme
    /// check can be applied to **every hop**, and the only place a fake can
    /// drive a loop without a socket.
    fn resolve(&self, url: &str, deadline: Instant) -> Result<PathBuf, FetchError> {
        check_scheme(url)?;
        // The cache is consulted before the clock, so an already-cached
        // document opens instantly even after the budget would have been spent
        // — a second open of the same file must cost nothing, including when
        // the first one was slow.
        if let Some(path) = self.cache.lookup(url) {
            return Ok(path);
        }

        let mut current = url.to_string();
        // `max_redirects` hops means `max_redirects + 1` requests.
        for _ in 0..=u32::from(self.limits.max_redirects) {
            if Instant::now() >= deadline {
                return Err(FetchError::BudgetSpent);
            }
            match self.fetcher.fetch(&current, &self.limits)? {
                Fetched::Body(bytes) => {
                    // Filed under the URL the document named, not the one the
                    // chain ended at. The next open starts from the same place
                    // the document points, so it hits the cache rather than
                    // walking the redirects again.
                    return self.cache.store(url, &bytes, self.decode_limits);
                }
                Fetched::Redirect(location) => {
                    let next = join(&current, &location).ok_or(FetchError::UnusableRedirect)?;
                    check_scheme(&next)?;
                    current = next;
                }
            }
        }
        Err(FetchError::TooManyRedirects {
            cap: self.limits.max_redirects,
        })
    }
}

/// One `![alt](dest …)` found in a line: where its destination sits, and where
/// the whole image ends.
struct ImageSpan<'a> {
    dest: &'a str,
    /// Byte offset of the first character of the destination.
    dest_start: usize,
    /// Byte offset just past the destination, before any title.
    dest_end: usize,
    /// Byte offset just past the closing `)`.
    end: usize,
}

/// The image starting at `start` (which must be `![`).
///
/// Bracket and parenthesis depth are both tracked because alt text legitimately
/// contains balanced brackets and a URL legitimately contains balanced
/// parentheses — a Wikipedia image is the standard example. This is the same
/// shape `decor::codecogs` uses and is deliberately a second copy rather than a
/// shared helper: that one returns the destination *including* any title so it
/// can decline the whole image, and this one has to split the title off and
/// keep it. Merging them would mean one function with a flag, and the flag
/// would be the difference between "decline" and "rewrite in place".
fn image_at(line: &str, start: usize) -> Option<ImageSpan<'_>> {
    let bytes = line.as_bytes();
    let mut cursor = start + 2;
    let mut depth = 1usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 1,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    if depth != 0 || bytes.get(cursor) != Some(&b']') || bytes.get(cursor + 1) != Some(&b'(') {
        return None;
    }

    let inner_start = cursor + 2;
    let mut cursor = inner_start;
    let mut depth = 1usize;
    let mut end = None;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => cursor += 1,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(cursor + 1);
                    break;
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    let end = end?;

    // Split the destination off the optional title the same way CommonMark
    // does: leading whitespace, then either a `<…>` form or a bare run up to
    // the next whitespace.
    let inner = &line[inner_start..end - 1];
    let lead = inner.len() - inner.trim_start().len();
    let dest_start = inner_start + lead;
    let rest = &line[dest_start..end - 1];
    let dest_len = if rest.starts_with('<') {
        rest.find('>').map(|close| close + 1)?
    } else {
        rest.find(char::is_whitespace).unwrap_or(rest.len())
    };
    let dest_end = dest_start + dest_len;
    let dest = &line[dest_start..dest_end];
    // A `<…>`-delimited destination is handed back without its delimiters, so
    // the scheme check sees a URL rather than a `<`.
    let dest = dest
        .strip_prefix('<')
        .and_then(|d| d.strip_suffix('>'))
        .unwrap_or(dest);
    Some(ImageSpan {
        dest,
        dest_start,
        dest_end,
        end,
    })
}

/// `path` written so CommonMark reads it back as exactly this path, or `None`
/// when no such spelling exists.
///
/// Always the `<…>` form, unconditionally, because the cache directory comes
/// from `$XDG_CACHE_HOME` or `$HOME` and both can contain spaces — and a space
/// in a bare destination *ends* the destination, silently turning the rest of
/// the path into a title. `/Users/Jane Doe/.cache/stele/ab…` written bare
/// parses as the destination `/Users/Jane` and it would have been a quiet,
/// machine-specific bug.
///
/// Backslashes are escaped because `scan::unescape_string` will unescape them
/// on the way back in. `<`, `>` and either newline have no spelling inside the
/// pointy form at all (`crates/ast/src/parser/scan.rs:152-163` ends the
/// destination at the first of them), so a cache path containing one is
/// refused — the image keeps its URL and becomes alt text, which is the same
/// answer every other failure gets.
fn markdown_destination(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    if text.contains(['<', '>', '\n', '\r']) {
        return None;
    }
    Some(format!("<{}>", text.replace('\\', "\\\\")))
}

/// Accepts `http` and `https`, case-insensitively, and refuses everything
/// else by name.
///
/// A destination with no `://` at all is refused too, with an empty scheme:
/// this pass only ever handles remote images, and a relative path is already
/// the local-image pipeline's business.
fn check_scheme(url: &str) -> Result<(), FetchError> {
    let scheme = url.split("://").next().unwrap_or_default();
    let has_separator = url.contains("://");
    if has_separator
        && ALLOWED_SCHEMES
            .iter()
            .any(|a| scheme.eq_ignore_ascii_case(a))
    {
        return Ok(());
    }
    // `data:` and `javascript:` have no `//`, so the scheme has to be read off
    // the colon as well or they would be reported as "no scheme" — accurate
    // but useless in a message and untestable as a distinct case.
    let named = if has_separator {
        scheme
    } else {
        url.split(':').next().unwrap_or_default()
    };
    Err(FetchError::UnsupportedScheme(named.to_string()))
}

/// A `Location` header resolved against the URL it came from.
///
/// Three forms, and deliberately only three: an absolute URL, a root-relative
/// path (`/other/pic.png`), and a protocol-relative one (`//host/pic.png`).
/// Anything else — a path-relative `../pic.png`, a bare `pic.png` — returns
/// `None` and the image falls back to its alt text.
///
/// That is a real gap and it is chosen rather than overlooked: resolving a
/// path-relative redirect correctly means implementing RFC 3986's merge and
/// remove-dot-segments algorithms, which is where URL joiners grow their
/// traversal bugs. The three forms here cover http→https upgrades, CDN host
/// bounces and shorteners, which is what redirects in the wild actually are;
/// the alternative was a `url` crate dependency for one function, on top of
/// the seventeen crates the client already costs.
fn join(base: &str, location: &str) -> Option<String> {
    let location = location.trim();
    if location.is_empty() {
        return None;
    }
    if location.contains("://") {
        return Some(location.to_string());
    }
    let (scheme, after) = base.split_once("://")?;
    if let Some(host_relative) = location.strip_prefix("//") {
        return Some(format!("{scheme}://{host_relative}"));
    }
    if location.starts_with('/') {
        let authority = after.split(['/', '?', '#']).next().unwrap_or(after);
        return Some(format!("{scheme}://{authority}{location}"));
    }
    None
}

/// The process's one real [`RemoteImages`], built on first use.
///
/// A `OnceLock` rather than a leaked `Box` so the allocation is named and
/// finite, and `&'static` so [`crate::loader::LoadOptions`] stays `Copy`. The
/// agent inside holds a connection pool, so sharing one across a document's
/// images (and across a `--watch` reload) is also what keeps a page of
/// diagrams from opening a fresh TLS session per picture.
#[cfg(feature = "remote-images")]
pub fn production() -> &'static RemoteImages {
    use std::sync::OnceLock;

    static REMOTE: OnceLock<RemoteImages> = OnceLock::new();
    REMOTE.get_or_init(|| {
        let limits = FetchLimits::default();
        RemoteImages::new(
            Box::new(crate::media::fetch::HttpFetcher::new(&limits)),
            Cache::user(),
            limits,
        )
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ast::{InlineKind, NodeRef};

    use super::*;
    use crate::media::cache::DEFAULT_CACHE_BYTES;
    use crate::media::fetch::fake::{Fake, Log, Reply, png};

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "stele-remote-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A `RemoteImages` over `fake`, caching into `dir`, with the production
    /// bounds unless a test overrides them.
    fn remote_with(dir: &Path, fake: Fake, limits: FetchLimits) -> RemoteImages {
        RemoteImages::new(Box::new(fake), Cache::at(dir, DEFAULT_CACHE_BYTES), limits)
    }

    fn remote(dir: &Path, fake: Fake) -> RemoteImages {
        remote_with(dir, fake, FetchLimits::default())
    }

    /// Every image destination the *parser* finds in `source` — the oracle for
    /// every rewrite assertion below.
    ///
    /// Asserted through `ast::Document::parse` rather than on the text,
    /// because a rewrite that produces plausible-looking markdown the parser
    /// reads some other way is exactly the failure this pass can have: the
    /// destination it writes goes through `scan_link_destination` and
    /// `unescape_string` before anything downstream sees it.
    fn parsed_destinations(source: &str) -> Vec<String> {
        let doc = ast::Document::parse(source);
        doc.nodes()
            .filter_map(|node| match node {
                NodeRef::Inline(inline) => match &inline.kind {
                    InlineKind::Image { dest, .. } => Some(dest.clone()),
                    _ => None,
                },
                NodeRef::Block(_) => None,
            })
            .collect()
    }

    const DOC: &str = "![a diagram](https://example.com/pic.png)\n";

    /// The happy path, end to end through the parser: the destination the
    /// document ends up with is a real file inside the cache, and the bytes in
    /// it are the ones the fake served.
    #[test]
    fn test_a_fetched_image_resolves_to_a_cache_path_that_holds_the_served_bytes() {
        let dir = scratch("happy");
        let log = Log::default();
        let bytes = png(64, 32);
        let remote = remote(&dir, Fake::serving(&log, bytes.clone()));

        let rewritten = remote.rewrite(DOC);
        assert!(matches!(rewritten, Cow::Owned(_)));
        assert_eq!(log.urls(), vec!["https://example.com/pic.png".to_string()]);

        let dests = parsed_destinations(&rewritten);
        assert_eq!(dests.len(), 1, "{rewritten:?}");
        let path = Path::new(&dests[0]);
        assert_eq!(path.parent(), Some(dir.as_path()), "{path:?}");
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        // The oracle for "and it renders": `gfx` reads the same dimensions
        // back out of the file that the PNG was built with, which is exactly
        // what `media::sizer` asks it at layout time.
        assert_eq!(
            gfx::decode::probe_dimensions(path, gfx::Limits::default()).unwrap(),
            (64, 32)
        );
        assert!(rewritten.contains("![a diagram]"), "{rewritten}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The rewritten destination has to reach `media::sizer` as a *local*
    /// path, or the exercise resolves a URL into something the next stage
    /// still refuses. Asserted through the real sizer and the real layout
    /// engine: a reserved box on the page is what the reader gets.
    #[test]
    fn test_a_fetched_image_reserves_a_real_box_in_the_layout_tree() {
        use layout::{IntrinsicSizer, LayoutConfig};
        use width::{WidthConfig, WidthEngine};

        let dir = scratch("reserves");
        let log = Log::default();
        let remote = remote(&dir, Fake::serving(&log, png(240, 480)));
        let rewritten = remote.rewrite(DOC);

        let doc = ast::Document::parse(&rewritten);
        // No base directory is needed: the rewritten destination is absolute.
        let sizer = crate::media::ImageSizer::new(".");
        let engine = WidthEngine::new(WidthConfig::default());
        let tree = layout::layout(&doc, 80, &LayoutConfig::default(), &engine, &sizer);
        let reserved = tree
            .lines(0..tree.line_count())
            .filter(|line| matches!(line, layout::Line::Reserved(_)))
            .count();
        assert!(reserved > 0, "the fetched image reserved no box at all");

        // The same document *without* the rewrite reserves nothing, which is
        // what makes the assertion above about this pass rather than about the
        // sizer.
        let untouched = ast::Document::parse(DOC);
        let image = untouched
            .nodes()
            .find(|n| matches!(n, NodeRef::Inline(i) if matches!(i.kind, InlineKind::Image { .. })))
            .unwrap()
            .id();
        assert!(sizer.size(image, &untouched).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Two loads, one fetch. The oracle is the fake's own call log, which is
    /// independent of anything the cache reports about itself.
    #[test]
    fn test_a_second_load_of_the_same_document_costs_no_second_fetch() {
        let dir = scratch("cache-hit");
        let log = Log::default();
        let remote = remote(&dir, Fake::serving(&log, png(32, 32)));

        let first = remote.rewrite(DOC).into_owned();
        let second = remote.rewrite(DOC).into_owned();

        assert_eq!(
            log.calls(),
            1,
            "the second load refetched: {:?}",
            log.urls()
        );
        assert_eq!(first, second, "the same URL must resolve to the same path");
        assert_eq!(parsed_destinations(&first).len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The same claim across two independent `RemoteImages` over one cache
    /// directory, which is what a second *process* looks like. A per-instance
    /// memo would pass the test above and fail this one.
    #[test]
    fn test_a_fresh_viewer_reading_the_same_cache_makes_no_request_at_all() {
        let dir = scratch("cache-across-instances");
        let first_log = Log::default();
        remote(&dir, Fake::serving(&first_log, png(32, 32))).rewrite(DOC);
        assert_eq!(first_log.calls(), 1);

        let second_log = Log::default();
        // A fake with an *empty* script: any request at all is an error, so
        // the assertion cannot pass by accidentally refetching successfully.
        let second = remote(&dir, Fake::new(&second_log, Vec::new()));
        let rewritten = second.rewrite(DOC);

        assert_eq!(second_log.calls(), 0, "a warm cache still made a request");
        assert!(
            matches!(rewritten, Cow::Owned(_)),
            "the cached path must still be substituted"
        );
        assert_eq!(parsed_destinations(&rewritten).len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// One document, the same URL three times: one fetch, three rewrites.
    #[test]
    fn test_the_same_url_repeated_in_one_document_is_fetched_once() {
        let dir = scratch("repeat");
        let log = Log::default();
        let source = format!("{DOC}\ntext\n\n{DOC}\n{DOC}");
        let remote = remote(&dir, Fake::serving(&log, png(16, 16)));

        let rewritten = remote.rewrite(&source);
        let dests = parsed_destinations(&rewritten);

        assert_eq!(log.calls(), 1, "{:?}", log.urls());
        assert_eq!(dests.len(), 3, "{rewritten:?}");
        assert!(dests.windows(2).all(|w| w[0] == w[1]));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Every failure mode, one table, one claim: the document comes back
    /// **byte for byte** and the image is still the URL the author wrote —
    /// which is what makes it alt text on screen.
    ///
    /// `Cow::Borrowed` is the strong form of that claim. An equality check
    /// would pass for a pass that rebuilt the string identically; borrowing
    /// proves nothing was rewritten at all.
    #[test]
    fn test_every_failure_mode_leaves_the_image_as_the_url_the_author_wrote() {
        // (name, url, script, how many requests it should have cost)
        let cases: [(&str, &str, Vec<Reply>, usize); 8] = [
            (
                "timeout",
                "https://example.com/pic.png",
                vec![Reply::Timeout],
                1,
            ),
            (
                "oversized response",
                "https://example.com/pic.png",
                vec![Reply::TooLarge],
                1,
            ),
            (
                "non-2xx",
                "https://example.com/pic.png",
                vec![Reply::Status(404)],
                1,
            ),
            (
                "transport failure",
                "https://example.com/pic.png",
                vec![Reply::Transport("dns error")],
                1,
            ),
            (
                "redirect loop",
                "https://example.com/pic.png",
                vec![Reply::RedirectTo("https://example.com/pic.png".to_string())],
                usize::from(FetchLimits::default().max_redirects) + 1,
            ),
            (
                "redirect to a refused scheme",
                "https://example.com/pic.png",
                vec![Reply::RedirectTo("file:///etc/passwd".to_string())],
                1,
            ),
            // Refused before any request is made at all — the zero is the
            // assertion: a scheme check that ran after the fetch would leak
            // the request it was meant to prevent.
            (
                "non-http scheme",
                "file:///etc/passwd",
                vec![Reply::Bytes(png(8, 8))],
                0,
            ),
            (
                "garbage bytes",
                "https://example.com/pic.png",
                vec![Reply::Bytes(b"<!doctype html><h1>404</h1>".to_vec())],
                1,
            ),
        ];

        for (name, url, script, requests) in cases {
            let dir = scratch(&format!("fail-{}", name.replace(' ', "-")));
            let log = Log::default();
            let source = format!("![alt text]({url})\n");
            let remote = remote(&dir, Fake::new(&log, script));

            let rewritten = remote.rewrite(&source);

            assert!(
                matches!(rewritten, Cow::Borrowed(_)),
                "{name}: the document was rewritten after a failure"
            );
            assert_eq!(
                parsed_destinations(&rewritten),
                vec![url.to_string()],
                "{name}"
            );
            assert_eq!(log.calls(), requests, "{name}: asked for {:?}", log.urls());
            // Nothing was left in the cache either — a failed fetch must not
            // cost disk any more than it costs a rewrite.
            let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
            assert!(
                left.is_empty(),
                "{name}: left {} file(s) behind",
                left.len()
            );
            std::fs::remove_dir_all(&dir).ok();
        }
    }

    /// The redirect cap bites at the documented hop and not one later. The
    /// oracle is the fake's URL log: `max_redirects` hops is `max_redirects + 1`
    /// requests, and an off-by-one in either direction changes its length.
    #[test]
    fn test_a_redirect_chain_is_followed_to_the_cap_and_no_further() {
        for cap in [0u8, 1, 3] {
            let dir = scratch(&format!("redirect-cap-{cap}"));
            let log = Log::default();
            let limits = FetchLimits {
                max_redirects: cap,
                ..FetchLimits::default()
            };
            let fake = Fake::new(
                &log,
                vec![Reply::RedirectTo("https://example.com/next".to_string())],
            );
            let remote = remote_with(&dir, fake, limits);

            assert!(matches!(remote.rewrite(DOC), Cow::Borrowed(_)), "cap {cap}");
            assert_eq!(
                log.calls(),
                usize::from(cap) + 1,
                "cap {cap} made {:?}",
                log.urls()
            );
            std::fs::remove_dir_all(&dir).ok();
        }
    }

    /// A chain *within* the cap resolves, and the bytes are filed under the
    /// URL the document named rather than the one the chain ended at — so the
    /// next open hits the cache instead of walking the redirects again.
    #[test]
    fn test_a_redirect_within_the_cap_resolves_and_caches_under_the_original_url() {
        let dir = scratch("redirect-ok");
        let log = Log::default();
        let bytes = png(20, 20);
        let fake = Fake::new(
            &log,
            vec![
                Reply::RedirectTo("https://cdn.example.com/pic.png".to_string()),
                Reply::Bytes(bytes.clone()),
            ],
        );
        let remote = remote(&dir, fake);
        let rewritten = remote.rewrite(DOC);

        assert_eq!(
            log.urls(),
            vec![
                "https://example.com/pic.png".to_string(),
                "https://cdn.example.com/pic.png".to_string()
            ]
        );
        let dests = parsed_destinations(&rewritten);
        assert_eq!(dests.len(), 1);
        let cache = Cache::at(&dir, DEFAULT_CACHE_BYTES);
        assert_eq!(
            Path::new(&dests[0]),
            cache.path_for("https://example.com/pic.png"),
            "the entry must be keyed by the document's URL, not the redirect target"
        );
        assert_eq!(std::fs::read(&dests[0]).unwrap(), bytes);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A root-relative and a protocol-relative `Location` are resolved against
    /// the URL they came from; a path-relative one is refused. The second half
    /// asserts on the URL the fake was *asked for*, which is the only way to
    /// see where a redirect actually went.
    #[test]
    fn test_a_relative_redirect_is_joined_against_its_own_request_or_refused() {
        assert_eq!(
            join("https://example.com/a/b.png?v=1", "/c/d.png").as_deref(),
            Some("https://example.com/c/d.png")
        );
        assert_eq!(
            join("https://example.com/a/b.png", "//cdn.example.net/d.png").as_deref(),
            Some("https://cdn.example.net/d.png")
        );
        assert_eq!(
            join("http://example.com/a/b.png", "https://example.com/d.png").as_deref(),
            Some("https://example.com/d.png")
        );
        // The gap this file documents rather than papers over.
        assert_eq!(join("https://example.com/a/b.png", "../d.png"), None);
        assert_eq!(join("https://example.com/a/b.png", "d.png"), None);
        assert_eq!(join("https://example.com/a/b.png", "   "), None);
        assert_eq!(join("https://example.com/a/b.png", ""), None);

        let dir = scratch("redirect-relative");
        let log = Log::default();
        let fake = Fake::new(
            &log,
            vec![
                Reply::RedirectTo("/moved/pic.png".to_string()),
                Reply::Bytes(png(12, 12)),
            ],
        );
        remote(&dir, fake).rewrite(DOC);
        assert_eq!(
            log.urls(),
            vec![
                "https://example.com/pic.png".to_string(),
                "https://example.com/moved/pic.png".to_string()
            ]
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Schemes, by name. The list is the point: `file:` and `data:` are the
    /// two an attacker reaches for, and neither has a `//` in the shape that
    /// matters, so a naive "split on `://`" check reports them as having no
    /// scheme at all.
    #[test]
    fn test_only_http_and_https_are_fetched_and_the_rest_are_named() {
        assert!(check_scheme("http://example.com/a.png").is_ok());
        assert!(check_scheme("https://example.com/a.png").is_ok());
        assert!(check_scheme("HTTPS://example.com/a.png").is_ok());
        for (url, scheme) in [
            ("file:///etc/passwd", "file"),
            ("ftp://example.com/a.png", "ftp"),
            ("data:image/png;base64,AAAA", "data"),
            ("javascript:alert(1)", "javascript"),
            ("gopher://example.com/a", "gopher"),
            ("./local.png", "./local.png"),
        ] {
            let error = check_scheme(url).unwrap_err();
            let FetchError::UnsupportedScheme(named) = error else {
                panic!("{url} was not refused as a scheme: {error:?}");
            };
            assert_eq!(named, scheme, "{url}");
        }
    }

    /// The whole-document budget, which is what bounds a page of forty dead
    /// images. The oracle is the fake's call count: with a budget shorter than
    /// two slow replies, the third image is never requested at all.
    ///
    /// Timing is the mechanism under test, so it is measured rather than
    /// assumed — but the assertion is on the *count*, which is exact, with the
    /// elapsed time only checked against a ceiling far above the noise.
    #[test]
    fn test_a_spent_document_budget_stops_further_requests_rather_than_waiting() {
        let dir = scratch("budget");
        let log = Log::default();
        let limits = FetchLimits {
            document_budget: Duration::from_millis(40),
            ..FetchLimits::default()
        };
        let source = concat!(
            "![one](https://example.com/1.png)\n",
            "![two](https://example.com/2.png)\n",
            "![three](https://example.com/3.png)\n",
            "![four](https://example.com/4.png)\n",
            "![five](https://example.com/5.png)\n",
        );
        let fake = Fake::new(&log, vec![Reply::Timeout]).slow(Duration::from_millis(50));
        let remote = remote_with(&dir, fake, limits);

        let started = Instant::now();
        let rewritten = remote.rewrite(source);
        let spent = started.elapsed();

        assert!(matches!(rewritten, Cow::Borrowed(_)));
        assert!(
            log.calls() < 5,
            "the budget stopped nothing: all {} images were requested",
            log.calls()
        );
        assert!(
            spent < Duration::from_millis(250),
            "five images cost {spent:?} against a 40 ms budget"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A URL inside a fenced code block is quoted, not requested. This is the
    /// rule every other source pass follows, and here it has teeth: a tutorial
    /// *about* remote images would otherwise fetch every URL it documents.
    #[test]
    fn test_a_url_inside_a_fence_or_an_unclosed_document_is_never_requested() {
        let dir = scratch("fenced");
        let log = Log::default();
        let remote = remote(&dir, Fake::serving(&log, png(8, 8)));

        let fenced = "```markdown\n![x](https://example.com/pic.png)\n```\n";
        assert!(matches!(remote.rewrite(fenced), Cow::Borrowed(_)));

        // Mid-save: an unclosed fence declines the whole document (R10).
        let half = "```\nhalf a save\n\n![x](https://example.com/pic.png)\n";
        assert!(matches!(remote.rewrite(half), Cow::Borrowed(_)));

        assert_eq!(log.calls(), 0, "asked for {:?}", log.urls());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A cache directory with a space in it — `/Users/Jane Doe/.cache/…` is an
    /// entirely ordinary macOS home — must still produce a destination the
    /// parser reads as one path. Written bare it would parse as `/Users/Jane`
    /// with the rest becoming a title, and the image would silently fail to
    /// load on exactly the machines whose owners have a space in their name.
    #[test]
    fn test_a_cache_directory_containing_a_space_still_parses_as_one_destination() {
        let root = scratch("spaced");
        let dir = root.join("Jane Doe").join(".cache");
        std::fs::create_dir_all(&dir).unwrap();
        let log = Log::default();
        let remote = remote(&dir, Fake::serving(&log, png(16, 16)));

        let rewritten = remote.rewrite(DOC);
        let dests = parsed_destinations(&rewritten);

        assert_eq!(dests.len(), 1, "{rewritten:?}");
        assert!(
            Path::new(&dests[0]).is_file(),
            "the parsed destination {:?} is not the file that was written",
            dests[0]
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// An image with a title keeps it, and the alt text survives unchanged
    /// around the swapped destination.
    #[test]
    fn test_a_title_and_alt_text_survive_the_destination_swap() {
        let dir = scratch("title");
        let log = Log::default();
        let remote = remote(&dir, Fake::serving(&log, png(16, 16)));
        let source = "![the alt](https://example.com/pic.png \"the title\")\n";
        let rewritten = remote.rewrite(source);

        let doc = ast::Document::parse(&rewritten);
        let image = doc
            .nodes()
            .find_map(|node| match node {
                NodeRef::Inline(inline) => match &inline.kind {
                    InlineKind::Image {
                        dest,
                        title,
                        children,
                    } => Some((dest.clone(), title.clone(), format!("{children:?}"))),
                    _ => None,
                },
                NodeRef::Block(_) => None,
            })
            .expect("the rewritten line must still hold an image");
        assert!(Path::new(&image.0).is_file(), "{:?}", image.0);
        assert_eq!(image.1, "the title");
        assert!(image.2.contains("the alt"), "{}", image.2);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Several images on one line, of which only one is remote: the local one
    /// is left alone and the prose between them survives.
    #[test]
    fn test_a_mixed_line_swaps_only_the_remote_images() {
        let dir = scratch("mixed");
        let log = Log::default();
        let remote = remote(&dir, Fake::serving(&log, png(16, 16)));
        let source = "before ![local](./a.png) middle ![remote](https://example.com/b.png) after\n";
        let rewritten = remote.rewrite(source);

        let dests = parsed_destinations(&rewritten);
        assert_eq!(log.urls(), vec!["https://example.com/b.png".to_string()]);
        assert_eq!(dests.len(), 2, "{rewritten:?}");
        assert_eq!(dests[0], "./a.png", "the local image must be untouched");
        assert!(Path::new(&dests[1]).is_file(), "{:?}", dests[1]);
        assert!(rewritten.contains("before "), "{rewritten}");
        assert!(rewritten.contains(" middle "), "{rewritten}");
        assert!(rewritten.ends_with(" after\n"), "{rewritten}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A link to an image is not an image. `[text](https://…/pic.png)` must
    /// never be fetched — following it is `crate::link`'s job and the reader's
    /// decision, not a side effect of opening the page.
    #[test]
    fn test_a_plain_link_to_an_image_url_is_not_fetched() {
        let dir = scratch("link-not-image");
        let log = Log::default();
        let source = "[a picture](https://example.com/pic.png) and <https://example.com/b.png>\n";
        let remote = remote(&dir, Fake::serving(&log, png(8, 8)));

        assert!(matches!(remote.rewrite(source), Cow::Borrowed(_)));
        assert_eq!(log.calls(), 0, "asked for {:?}", log.urls());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// CRLF endings survive, as they must for every pass in this chain — the
    /// corpus has a fixture whose whole purpose is that they do.
    #[test]
    fn test_crlf_line_endings_survive_the_rewrite() {
        let dir = scratch("crlf");
        let log = Log::default();
        let remote = remote(&dir, Fake::serving(&log, png(8, 8)));
        let rewritten = remote.rewrite("![a](https://example.com/pic.png)\r\nnext\r\n");
        assert!(rewritten.contains("\r\nnext\r\n"), "{rewritten:?}");
        assert_eq!(parsed_destinations(&rewritten).len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// An image whose declared dimensions exceed `gfx::Limits` is refused at
    /// the same gate a local one is. Exercised with a lowered limit rather
    /// than an 8193-pixel PNG so the test costs bytes rather than megabytes —
    /// which is what `with_decode_limits` is for.
    #[test]
    fn test_a_downloaded_image_over_the_decoder_limits_is_refused_like_a_local_one() {
        let dir = scratch("bomb");
        let log = Log::default();
        let remote =
            remote(&dir, Fake::serving(&log, png(64, 64))).with_decode_limits(gfx::Limits {
                max_dim: 32,
                max_alloc: 1024,
            });

        assert!(matches!(remote.rewrite(DOC), Cow::Borrowed(_)));
        assert_eq!(
            log.calls(),
            1,
            "the bytes still had to be fetched to be judged"
        );
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert!(
            left.is_empty(),
            "a refused image left {} file(s)",
            left.len()
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The image scanner, on the shapes that break naive ones.
    #[test]
    fn test_the_image_scanner_finds_the_destination_through_brackets_and_parens() {
        let cases = [
            ("![a](https://h/p.png)", "https://h/p.png"),
            ("![a [b] c](https://h/p.png)", "https://h/p.png"),
            ("![a](https://h/p_(1).png)", "https://h/p_(1).png"),
            ("![a](https://h/p.png \"t\")", "https://h/p.png"),
            ("![a](<https://h/p.png>)", "https://h/p.png"),
            ("![a](  https://h/p.png  )", "https://h/p.png"),
        ];
        for (line, expected) in cases {
            let span = image_at(line, 0).unwrap_or_else(|| panic!("no image in {line}"));
            assert_eq!(span.dest, expected, "{line}");
            assert_eq!(span.end, line.len(), "{line}");
        }
        // Not images at all.
        assert!(image_at("![a] (https://h/p.png)", 0).is_none());
        assert!(image_at("![a](https://h/p.png", 0).is_none());
        assert!(image_at("![a", 0).is_none());
    }

    /// A destination that cannot be spelled in CommonMark's pointy form is
    /// declined rather than emitted broken, and one that can survives the
    /// round trip through the parser's own unescaping.
    #[test]
    fn test_a_path_with_no_valid_markdown_spelling_is_declined() {
        assert_eq!(
            markdown_destination(Path::new("/tmp/cache/abc")).as_deref(),
            Some("</tmp/cache/abc>")
        );
        assert_eq!(
            markdown_destination(Path::new("/tmp/Jane Doe/abc")).as_deref(),
            Some("</tmp/Jane Doe/abc>")
        );
        assert_eq!(markdown_destination(Path::new("/tmp/a<b/abc")), None);
        assert_eq!(markdown_destination(Path::new("/tmp/a>b/abc")), None);
        assert_eq!(markdown_destination(Path::new("/tmp/a\nb/abc")), None);

        for path in ["/tmp/a\\b/abc", "/tmp/Jane Doe/abc", "/tmp/a(1)/abc"] {
            let spelled = markdown_destination(Path::new(path)).unwrap();
            assert_eq!(
                parsed_destinations(&format!("![x]({spelled})\n")),
                vec![path.to_string()],
                "{path} did not survive the round trip as {spelled}"
            );
        }
    }
}
