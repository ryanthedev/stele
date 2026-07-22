//! DW-2.3 fuzz target: `Document::parse` must never panic, OOM, or take
//! more than 1s on any input (enforced by `-timeout=1 -rss_limit_mb=2048`).
//! The target also re-checks the span invariants on every parse, in both
//! extension profiles, and serializes through the HTML shim.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);
    for opts in [ast::ParseOptions::default(), ast::ParseOptions::commonmark()] {
        let doc = ast::Document::parse_with(&s, &opts);
        let _ = ast::html::to_html(&doc);
        let src = doc.source();
        for node in doc.nodes() {
            let sp = node.span();
            assert!(sp.start <= sp.end, "span inverted: {sp:?}");
            assert!(sp.end <= src.len(), "span out of bounds: {sp:?}");
            assert!(
                src.is_char_boundary(sp.start) && src.is_char_boundary(sp.end),
                "span not on char boundary: {sp:?}"
            );
        }
    }
});
