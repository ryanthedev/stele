//! The on-disk store a fetched image lands in, content-addressed by its URL.
//!
//! Two jobs, and they are the same job seen from either end: make the second
//! open of a document cost no requests, and make a filename that a hostile URL
//! cannot steer. Both fall out of the same rule — **a cache entry's name is 32
//! hex digits and nothing else**. Not a sanitized URL, not a percent-encoded
//! path, not the last segment with the slashes taken out. A name built out of
//! the alphabet `0-9a-f` cannot contain a `/`, a `..`, a NUL or a drive
//! letter, so `join`ing it onto the cache directory cannot leave the cache
//! directory. There is no traversal check anywhere in this file because there
//! is nothing left for one to check.
//!
//! ## The hash, and what it is not
//!
//! [`key`] is FNV-1a/128 over the URL's bytes. It is **not** cryptographic and
//! is not claimed to be: an attacker who controls two URLs in a document you
//! open can, with effort, make them collide, and the second image would then
//! be served the first's bytes. That is the honest bound on this choice. It
//! was taken over adding a SHA-2 dependency, or hand-rolling one, because the
//! property this file actually needs from a hash is *stability and a safe
//! alphabet*, and the consequence of a collision is a wrong picture in a
//! document whose author already chose both URLs.
//!
//! `std::hash::DefaultHasher` was the other candidate and is unusable here for
//! a reason worth writing down: its algorithm is explicitly not guaranteed
//! across Rust releases, so a toolchain upgrade would silently rename every
//! entry and the cache would refill itself from the network. A cache key must
//! outlive the compiler that wrote it.
//!
//! ## Eviction
//!
//! Total-size cap, least-recently-used first. "Recently used" is the file's
//! mtime, and a cache *hit* touches it ([`Cache::lookup`]) — without that
//! touch the rule would be first-in-first-out wearing an LRU label, and a
//! diagram you read every day would be evicted by images you saw once. The
//! touch is best-effort: a read-only cache directory degrades to FIFO rather
//! than to a failure.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::media::fetch::FetchError;

/// The FNV-1a 128-bit offset basis and prime, as specified. Written out rather
/// than computed so a reader can check them against the specification without
/// running anything.
const FNV_OFFSET_BASIS: u128 = 144_066_263_297_769_815_596_495_629_667_062_367_629;
const FNV_PRIME: u128 = 309_485_009_821_345_068_724_781_371;

/// How many bytes of fetched images stele will keep on disk.
///
/// 128 MiB, and the number is a judgement rather than a measurement, so here
/// is the judgement: a d2l chapter's diagrams run 20–200 KiB each, so this
/// holds on the order of a thousand of them — every remote image a reader is
/// plausibly revisiting — while staying small enough that finding it in
/// `~/.cache` is a shrug rather than an incident. It is deliberately well under
/// [`crate::media::fetch::FetchLimits::max_response_bytes`]: a single response
/// at that ceiling would evict the entire cache to store itself, which is the
/// correct behavior for a cache and a good reason not to size the two alike.
pub const DEFAULT_CACHE_BYTES: u64 = 128 * 1024 * 1024;

/// A content-addressed image cache rooted at one directory.
///
/// `dir` is a parameter rather than something this type reads from the
/// environment on demand, and that is what makes it testable: `std::env::set_var`
/// is `unsafe` under edition 2024 and `crates/stele` is `deny(unsafe_code)`, so
/// a cache that resolved `$XDG_CACHE_HOME` internally could not be pointed at a
/// scratch directory by any test in this crate. [`Cache::user`] does the
/// environment lookup once, at the edge.
#[derive(Debug, Clone)]
pub struct Cache {
    dir: PathBuf,
    max_bytes: u64,
}

impl Cache {
    /// A cache at an explicit directory. The directory is created lazily, on
    /// the first successful store — opening a document that fetches nothing
    /// must not leave a directory behind.
    pub fn at(dir: impl Into<PathBuf>, max_bytes: u64) -> Self {
        Cache {
            dir: dir.into(),
            max_bytes,
        }
    }

    /// The reader's own cache directory: `$XDG_CACHE_HOME/stele`, else
    /// `$HOME/.cache/stele`, else a per-user directory under the system
    /// temporary directory.
    ///
    /// `$XDG_CACHE_HOME` is ignored unless it is absolute, which the XDG base
    /// directory specification requires and which also happens to be the
    /// difference between a cache and a directory called `stele` appearing
    /// wherever the reader happened to be standing.
    ///
    /// The last fallback is not the specification's — nothing in XDG says what
    /// to do with neither variable set — and it is chosen over refusing to
    /// cache because a cache that vanishes on reboot is still a cache, while
    /// no cache at all turns every reopen into a fresh set of requests.
    pub fn user() -> Self {
        let base = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .map(|home| home.join(".cache"))
            })
            .unwrap_or_else(std::env::temp_dir);
        Cache::at(base.join("stele"), DEFAULT_CACHE_BYTES)
    }

    /// Where `url`'s bytes would live. Says nothing about whether they do.
    pub fn path_for(&self, url: &str) -> PathBuf {
        self.dir.join(key(url))
    }

    /// The cached file for `url`, if one is there — and a touch on the way
    /// past, which is what makes [`Cache::store`]'s eviction least-recently-*used*
    /// rather than oldest-first.
    ///
    /// Checked with `metadata`, not `exists`, because the answer has to be
    /// "a regular file with bytes in it": a directory or a leftover zero-byte
    /// entry would otherwise be reported as a hit and then fail to decode,
    /// turning a transient into a permanent alt text that no refetch clears.
    pub fn lookup(&self, url: &str) -> Option<PathBuf> {
        let path = self.path_for(url);
        let meta = fs::metadata(&path).ok()?;
        if !meta.is_file() || meta.len() == 0 {
            return None;
        }
        touch(&path);
        Some(path)
    }

    /// Validates `bytes` as an image and, if they are one, files them under
    /// `url`'s key.
    ///
    /// **Written to a temporary name and probed before being renamed**, in
    /// that order, for two independent reasons:
    ///
    /// - `gfx::decode::probe_dimensions` takes a path, and reusing it rather
    ///   than a bytes-shaped twin is what makes a fetched image answer to
    ///   *exactly* the checks a local one does — format sniffing, the SVG
    ///   path, and `gfx::Limits` on the declared dimensions. A second
    ///   validation path would be a second place for the two to disagree.
    /// - `rename(2)` within a directory is atomic, so a second stele reading
    ///   this cache never sees a half-written file. Without the temporary,
    ///   a fetch interrupted mid-write would leave a truncated entry that
    ///   looks like a hit forever.
    ///
    /// The temporary carries the process id so two stele processes fetching
    /// the same URL at the same moment cannot tread on each other; the loser
    /// of the rename simply overwrites with identical bytes.
    pub fn store(
        &self,
        url: &str,
        bytes: &[u8],
        limits: gfx::Limits,
    ) -> Result<PathBuf, FetchError> {
        fs::create_dir_all(&self.dir).map_err(FetchError::CacheWrite)?;
        let key = key(url);
        let temp = self.dir.join(format!("{key}.{}.part", std::process::id()));
        fs::write(&temp, bytes).map_err(FetchError::CacheWrite)?;
        if gfx::decode::probe_dimensions(&temp, limits).is_err() {
            // The bytes are junk, an unsupported format, or a bomb header.
            // Nothing about that is worth keeping, and leaving the `.part`
            // behind would grow the directory without ever being a hit.
            fs::remove_file(&temp).ok();
            return Err(FetchError::NotAnImage);
        }
        let path = self.dir.join(&key);
        if let Err(err) = fs::rename(&temp, &path) {
            fs::remove_file(&temp).ok();
            return Err(FetchError::CacheWrite(err));
        }
        self.evict();
        Ok(path)
    }

    /// Deletes least-recently-used entries until the directory is within
    /// [`Cache::max_bytes`].
    ///
    /// **Only ever deletes files this cache made**: a name of exactly 32 hex
    /// digits, or one of our own `.part` temporaries. The directory comes from
    /// `$XDG_CACHE_HOME`, which a reader may well have pointed somewhere
    /// shared or surprising, and a cache that enforces a size cap by deleting
    /// whatever it finds is a data-loss bug waiting for one misconfigured
    /// environment variable.
    ///
    /// Best-effort throughout: a directory it cannot read, or a file it cannot
    /// delete, leaves the cache over its cap rather than failing a document
    /// that has already loaded.
    fn evict(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        let mut ours: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
        let mut total: u64 = 0;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !is_cache_name(name) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            total = total.saturating_add(meta.len());
            ours.push((modified, meta.len(), entry.path()));
        }
        if total <= self.max_bytes {
            return;
        }
        // Oldest touch first: `lookup` has already bumped everything read this
        // session, so the front of this list is what nothing has asked for.
        ours.sort_by_key(|entry| entry.0);
        for (_, len, path) in ours {
            if total <= self.max_bytes {
                break;
            }
            if fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(len);
            }
        }
    }
}

/// Whether `name` is a file this cache created: a bare 32-hex-digit key, or
/// one of its `.part` temporaries.
fn is_cache_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    let is_key = stem.len() == 32
        && stem
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
    is_key && (stem == name || name.ends_with(".part"))
}

/// `url`'s cache key: FNV-1a/128, lower-case hex, always 32 characters.
///
/// The fixed width is load-bearing, not cosmetic — see the module doc. It is
/// also why this takes the URL's *bytes*: a URL is not required to be valid
/// UTF-8 once percent escapes are involved, and hashing bytes means there is
/// no decoding step that could fail and tempt a fallback that uses the URL
/// text itself.
pub fn key(url: &str) -> String {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in url.as_bytes() {
        hash ^= u128::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:032x}")
}

/// Best-effort "this entry was used just now".
///
/// `File::set_times` rather than reading and rewriting the file: the point is
/// to move a timestamp, and rewriting 200 KiB to do it would make every cache
/// hit cost more than it saves. A failure is ignored on purpose — a read-only
/// cache still serves hits, it just evicts in insertion order.
fn touch(path: &Path) {
    let Ok(file) = fs::File::options().write(true).open(path) else {
        return;
    };
    let now = SystemTime::now();
    let times = fs::FileTimes::new().set_accessed(now).set_modified(now);
    file.set_times(times).ok();
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;
    use crate::media::fetch::fake::png;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "stele-cache-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The traversal question, asked of the shapes an attacker would actually
    /// write. The oracle is not "does the path look safe" — it is the two
    /// properties that make it safe by construction: the file's parent is the
    /// cache directory itself, and its name is 32 hex digits.
    ///
    /// A test that merely asserted `!path.contains("..")` would pass against a
    /// naive implementation that URL-decoded `%2e%2e` afterwards.
    #[test]
    fn test_a_hostile_url_cannot_name_a_file_outside_the_cache_directory() {
        let dir = scratch("traversal");
        let cache = Cache::at(&dir, DEFAULT_CACHE_BYTES);
        let hostile = [
            "https://h/../../../../etc/passwd",
            "https://h/%2e%2e%2f%2e%2e%2fetc%2fpasswd",
            "https://h/..\\..\\windows\\system32",
            "https://h/a\0b",
            "https://h/a\nb",
            "https://h/////////////",
            "https://h/~/.ssh/id_rsa",
            "https://h/$HOME/.bashrc",
            "https://h/a?x=/../../y",
            // A URL that *is* a path traversal with nothing else in it.
            "../../../../../../etc/passwd",
        ];
        for url in hostile {
            let path = cache.path_for(url);
            assert_eq!(
                path.parent(),
                Some(dir.as_path()),
                "{url} escaped the cache directory: {path:?}"
            );
            let name = path.file_name().unwrap().to_str().unwrap();
            assert_eq!(name.len(), 32, "{url} produced {name}");
            assert!(
                name.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "{url} produced a name outside the hex alphabet: {name}"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Distinct URLs must reach distinct entries, or the cache would serve one
    /// document's diagram for another's. Checked over URLs that differ in one
    /// byte, in the query, in the scheme and in trailing punctuation — the
    /// places a weak key folds things together.
    #[test]
    fn test_urls_that_differ_at_all_get_different_keys() {
        let urls = [
            "https://example.com/a.png",
            "https://example.com/b.png",
            "http://example.com/a.png",
            "https://example.com/a.png?v=1",
            "https://example.com/a.png?v=2",
            "https://example.com/a.png/",
            "https://example.com/A.png",
            "",
        ];
        let mut seen: Vec<String> = Vec::new();
        for url in urls {
            let k = key(url);
            assert_eq!(k.len(), 32, "{url}");
            assert!(!seen.contains(&k), "{url} collided with an earlier URL");
            seen.push(k);
        }
        // And the key is a pure function of the URL — the same input twice is
        // the same name, which is the whole basis of the second-open-is-free
        // promise.
        assert_eq!(
            key("https://example.com/a.png"),
            key("https://example.com/a.png")
        );
    }

    /// Storing validates through `gfx::decode`, so bytes that are not an image
    /// never become a cache entry — and leave nothing behind either. The
    /// oracle is the directory listing: the whole point is that a rejected
    /// download costs no disk.
    #[test]
    fn test_bytes_that_are_not_an_image_are_refused_and_leave_no_file() {
        let dir = scratch("not-an-image");
        let cache = Cache::at(&dir, DEFAULT_CACHE_BYTES);
        let url = "https://example.com/junk.png";
        let error = cache
            .store(url, b"<html>404 not found</html>", gfx::Limits::default())
            .unwrap_err();
        assert!(matches!(error, FetchError::NotAnImage), "{error:?}");
        assert!(cache.lookup(url).is_none());
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert!(
            left.is_empty(),
            "a rejected download left {} file(s) behind",
            left.len()
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A real image round-trips, and the oracle is independent of this file:
    /// the stored entry's dimensions, read back through `gfx::decode`, are the
    /// ones the PNG was built with.
    #[test]
    fn test_a_stored_image_comes_back_byte_for_byte_and_still_decodes() {
        let dir = scratch("round-trip");
        let cache = Cache::at(&dir, DEFAULT_CACHE_BYTES);
        let url = "https://example.com/pic.png";
        let bytes = png(48, 24);
        let path = cache.store(url, &bytes, gfx::Limits::default()).unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(
            gfx::decode::probe_dimensions(&path, gfx::Limits::default()).unwrap(),
            (48, 24)
        );
        assert_eq!(cache.lookup(url).as_deref(), Some(path.as_path()));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A zero-byte or directory entry at a key's path is not a hit. Without
    /// this the first interrupted write would poison that URL permanently:
    /// every later open would "hit", fail to decode, and never refetch.
    #[test]
    fn test_an_empty_or_non_file_entry_is_a_miss_rather_than_a_poisoned_hit() {
        let dir = scratch("poisoned");
        let cache = Cache::at(&dir, DEFAULT_CACHE_BYTES);
        let empty_url = "https://example.com/empty.png";
        std::fs::File::create(cache.path_for(empty_url)).unwrap();
        assert!(cache.lookup(empty_url).is_none());

        let dir_url = "https://example.com/dir.png";
        std::fs::create_dir_all(cache.path_for(dir_url)).unwrap();
        assert!(cache.lookup(dir_url).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The cap is enforced, and it is enforced by deleting the entry nothing
    /// has asked for.
    ///
    /// Three images, a cap that fits two, and a `lookup` on the first between
    /// the second and third store — so insertion order and use order disagree.
    /// A first-in-first-out eviction passes an "is the cap enforced" test and
    /// fails this one, which is the difference the touch in `lookup` exists to
    /// make.
    #[test]
    fn test_the_cap_evicts_the_least_recently_used_entry_not_the_oldest() {
        let dir = scratch("evict-lru");
        let first = png(64, 64);
        // Two entries fit, three do not.
        let cache = Cache::at(&dir, (first.len() as u64) * 2 + 1);

        let a = "https://example.com/a.png";
        let b = "https://example.com/b.png";
        let c = "https://example.com/c.png";
        cache.store(a, &first, gfx::Limits::default()).unwrap();
        // mtime resolution on some filesystems is coarse; space the writes so
        // the ordering under test is a real ordering and not a tie.
        std::thread::sleep(std::time::Duration::from_millis(20));
        cache
            .store(b, &png(64, 64), gfx::Limits::default())
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        // `a` is read here, which must make `b` the least recently used.
        assert!(cache.lookup(a).is_some());
        std::thread::sleep(std::time::Duration::from_millis(20));
        cache
            .store(c, &png(64, 64), gfx::Limits::default())
            .unwrap();

        assert!(
            cache.lookup(c).is_some(),
            "the entry just stored must survive"
        );
        assert!(
            cache.lookup(a).is_some(),
            "the entry read most recently must survive"
        );
        assert!(
            cache.lookup(b).is_none(),
            "the least recently used entry must have been evicted"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Eviction touches only what this cache made. `$XDG_CACHE_HOME` is a
    /// reader-supplied path and pointing it at a shared directory must cost
    /// them nothing — a size cap enforced by deleting strangers' files is a
    /// data-loss bug one environment variable away.
    #[test]
    fn test_eviction_never_deletes_a_file_this_cache_did_not_create() {
        let dir = scratch("evict-strangers");
        let cache = Cache::at(&dir, 1);
        let bystander = dir.join("important-notes.md");
        std::fs::File::create(&bystander)
            .unwrap()
            .write_all(b"do not delete me")
            .unwrap();
        // A name that is hex but the wrong length, and one that is the right
        // length but not hex: both near-misses of the key alphabet.
        let near_a = dir.join("deadbeef");
        let near_b = dir.join("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz");
        std::fs::write(&near_a, b"x").unwrap();
        std::fs::write(&near_b, b"x").unwrap();

        cache
            .store(
                "https://example.com/a.png",
                &png(64, 64),
                gfx::Limits::default(),
            )
            .unwrap();

        assert!(bystander.exists(), "eviction deleted a bystander file");
        assert!(near_a.exists(), "eviction deleted a short hex-looking file");
        assert!(near_b.exists(), "eviction deleted a non-hex file");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The name test that keeps the one above honest: what `is_cache_name`
    /// says yes to has to be exactly the set this file writes.
    #[test]
    fn test_only_a_32_hex_key_or_its_part_file_is_recognised_as_ours() {
        assert!(is_cache_name(&key("https://example.com/a.png")));
        assert!(is_cache_name(&format!("{}.1234.part", key("https://x/"))));
        assert!(!is_cache_name("deadbeef"));
        assert!(!is_cache_name("important-notes.md"));
        assert!(!is_cache_name(&"A".repeat(32)));
        assert!(!is_cache_name(&format!("{}.png", key("https://x/"))));
        assert!(!is_cache_name(""));
    }

    /// `$XDG_CACHE_HOME` wins, `$HOME/.cache` is the fallback, and a relative
    /// `$XDG_CACHE_HOME` is ignored rather than obeyed. Asserted on the
    /// resolution *rule* rather than by setting the variables, because
    /// `std::env::set_var` is `unsafe` under edition 2024 and this crate is
    /// `deny(unsafe_code)` — which is also precisely why `Cache::at` exists.
    #[test]
    fn test_the_user_cache_lands_under_a_stele_directory_of_an_absolute_root() {
        let cache = Cache::user();
        assert!(
            cache.dir.is_absolute(),
            "the user cache must be absolute, got {:?}",
            cache.dir
        );
        assert_eq!(
            cache.dir.file_name().and_then(|n| n.to_str()),
            Some("stele"),
            "the user cache must live in its own directory: {:?}",
            cache.dir
        );
        assert_eq!(cache.max_bytes, DEFAULT_CACHE_BYTES);
        // The two documented roots, and nothing else, are what it can be under.
        let parent = cache.dir.parent().unwrap();
        let expected = [
            std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")),
            Some(std::env::temp_dir()),
        ];
        assert!(
            expected
                .iter()
                .flatten()
                .any(|candidate| candidate == parent),
            "{parent:?} is none of the documented cache roots"
        );
    }
}
