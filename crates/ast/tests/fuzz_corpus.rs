//! Local replay of the cargo-fuzz corpus.
//!
//! `crates/ast/fuzz/corpus/` is gitignored — it is a machine-local
//! artifact of the DW-2.3 fuzz runs, not a checked-in suite — so this test
//! is `#[ignore]`d and skips itself when the corpus is absent. It exists
//! so a parser change can be replayed against ten thousand accumulated
//! inputs without waiting on a fresh fuzz run:
//!
//! ```text
//! cargo test -p ast --test fuzz_corpus -- --ignored --nocapture
//! ```
//!
//! It asserts the same invariants `fuzz_targets/parse.rs` does, under both
//! parse profiles: every span is ordered, in bounds, and on `char`
//! boundaries, so `&source[span]` never panics — and HTML rendering runs
//! to completion on every input.

use ast::{Document, ParseOptions};
use std::path::PathBuf;
use std::time::Instant;

#[test]
#[ignore = "replays a gitignored local corpus; run explicitly"]
fn test_every_fuzz_corpus_input_parses_with_sound_spans_under_both_profiles() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fuzz/corpus/parse");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("no corpus at {dir:?} — nothing to replay");
        return;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    paths.sort();

    let profiles = [
        ("default", ParseOptions::default()),
        ("commonmark", ParseOptions::commonmark()),
    ];
    let start = Instant::now();
    let mut files = 0usize;
    let mut nodes = 0usize;
    for path in &paths {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        // The fuzz target feeds arbitrary bytes through the same lossy
        // conversion `Document::parse` documents for invalid UTF-8.
        let src = String::from_utf8_lossy(&bytes);
        files += 1;
        for (name, opts) in &profiles {
            let doc = Document::parse_with(&src, opts);
            let text = doc.source();
            for node in doc.nodes() {
                let sp = node.span();
                assert!(
                    sp.start <= sp.end,
                    "{path:?} [{name}]: inverted span {sp:?}"
                );
                assert!(
                    sp.end <= text.len(),
                    "{path:?} [{name}]: span {sp:?} past {} bytes",
                    text.len()
                );
                assert!(
                    text.is_char_boundary(sp.start) && text.is_char_boundary(sp.end),
                    "{path:?} [{name}]: span {sp:?} off a char boundary"
                );
                // The invariant the three above exist to protect.
                let _ = &text[sp.start..sp.end];
                nodes += 1;
            }
            let _ = ast::html::to_html(&doc);
        }
    }
    let dt = start.elapsed();
    println!("replayed {files} corpus files × 2 profiles, {nodes} nodes, in {dt:?}");
    assert!(files > 0, "corpus directory was present but empty");
}
