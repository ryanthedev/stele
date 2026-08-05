//! Untrusted-input hardening tests.
//!
//! DW-2.5: `#![forbid(unsafe_code)]` holds crate-wide — asserted here so the
//! CI `test` job fails if the attribute is ever removed (the attribute
//! itself makes any `unsafe` a compile error).
//!
//! DW-2.3 (CI proxy): the deterministic half of the fuzz gate — cmark's
//! pathological inputs must parse fast (linear time) and without panics.
//! The one-hour cargo-fuzz run is the other half (see crates/ast/fuzz/).

use ast::Document;
use std::time::{Duration, Instant};

#[test]
fn test_dw_2_5_forbid_unsafe_present() {
    let lib = std::fs::read_to_string(format!("{}/src/lib.rs", env!("CARGO_MANIFEST_DIR")))
        .expect("lib.rs readable");
    assert!(
        lib.contains("#![forbid(unsafe_code)]"),
        "crate root must forbid unsafe_code"
    );
}

/// Generous absolute budget per pathological input. Sized so linear parses
/// pass easily in debug CI while quadratic blowup (hundreds of millions of
/// operations at these input sizes) fails by an order of magnitude.
const BUDGET: Duration = Duration::from_secs(5);

fn assert_fast(name: &str, input: &str) {
    let t = Instant::now();
    let doc = Document::parse(input);
    let _ = ast::html::to_html(&doc);
    let dt = t.elapsed();
    assert!(
        dt < BUDGET,
        "pathological input {name} ({} bytes) took {dt:?}",
        input.len()
    );
}

#[test]
fn test_dw_2_3_pathological_inputs_stay_fast() {
    let n = 20_000;
    // Ported from cmark's pathological_tests.py.
    assert_fast(
        "nested strong emph",
        &format!("{}b{}", "*a **a ".repeat(n), " a** a*".repeat(n)),
    );
    assert_fast("emph closers no openers", &"a_ ".repeat(n));
    assert_fast("openers closers mod3", &format!("a**b{}", "c* ".repeat(n)));
    assert_fast("link closers no openers", &"]".repeat(n));
    assert_fast("link openers no closers", &"[".repeat(n));
    assert_fast("mixed openers closers", &"[]".repeat(n));
    assert_fast(
        "nested brackets",
        &format!("{}a{}", "[".repeat(n), "]".repeat(n)),
    );
    assert_fast("nested block quotes", &format!("{}a", "> ".repeat(n)));
    assert_fast("deeply nested lists", &{
        let mut s = String::new();
        for i in 0..1000 {
            s.push_str(&" ".repeat(i));
            s.push_str("- a\n");
        }
        s
    });
    assert_fast("backtick openers", &"`a".repeat(n));
    assert_fast("unclosed inline links", &"[a](b".repeat(n));
    assert_fast("unclosed pointy links", &"[a](<b".repeat(n));
    assert_fast(
        "many refdef misses",
        &format!("{}\n\n{}", "[a]: /u\n\n".repeat(500), "[b] ".repeat(n)),
    );
    assert_fast("tilde runs", &"~~a".repeat(n));
    assert_fast("dollar runs", &"$a".repeat(n));
    assert_fast(
        "many autolink candidates",
        &"www. w@b. http:/ ".repeat(n / 4),
    );
    assert_fast("whitespace lines", &" \n".repeat(n));
    assert_fast("setext candidates", &"a\n- \n".repeat(n / 2));

    // Grid tables recognize themselves by scanning forward over the whole
    // candidate table before opening anything, so every rule line that
    // fails to become a table is a scan that has to be paid for. The
    // argument that those scans partition the document rather than
    // stacking up is written out in `parser/grid.rs`; these are the
    // shapes it turns on, measured rather than reasoned about.
    //
    // "rules with no rows" is the one the argument is subtlest for: the
    // 20,000 rule lines are a single paragraph, and the grid arm refuses
    // to interrupt a paragraph, so only the first line is ever a
    // candidate.
    assert_fast("grid rules with no rows", &"+-+\n".repeat(n));
    assert_fast("grid rows with no rules", &"| |\n".repeat(n));
    assert_fast(
        "grid rules one char short",
        &"+---+\n| a |\n+--+\n".repeat(n / 3),
    );
    assert_fast(
        "grid ragged column counts",
        &"+-+-+\n| | |\n+-+\n".repeat(n / 3),
    );
    assert_fast(
        "giant grid table",
        &format!(
            "+-----+-----+\n{}",
            "| aaa | bbb |\n+-----+-----+\n".repeat(n / 2)
        ),
    );
    assert_fast(
        "grid table of one tall cell",
        &format!("+-----+\n{}+-----+\n", "| aaa |\n".repeat(n)),
    );
    assert_fast(
        "grid cells holding only bars",
        &"+-+-+\n| | |\n+-+-+\n".repeat(n / 3),
    );
    assert_fast(
        "grid table unclosed at eof",
        &format!("+-----+\n{}", "| aaa |\n".repeat(n)),
    );
}

#[test]
fn test_parse_is_total_on_hostile_inputs() {
    // NUL, lone surrogate-free garbage, control bytes, huge runs — parse
    // must return a Document, never panic.
    let cases: Vec<String> = vec![
        "\0\0\0".into(),
        "a\0b".into(),
        "\u{FFFD}\u{FFFF}\u{10FFFF}".into(),
        "\r\r\n\r".into(),
        "\t\t\t- \t x".into(),
        "#".repeat(100_000),
        "\\".repeat(100_001),
        "&".repeat(50_000),
        "<".repeat(50_000),
        format!("[^{}]: x", "a".repeat(10_000)),
        "| --- |\n| --- |\n".repeat(1000),
    ];
    for (i, case) in cases.iter().enumerate() {
        let doc = Document::parse(case);
        let _ = ast::html::to_html(&doc);
        let _ = doc.node_count();
        let _ = i;
    }
}

/// Regression: fuzz crash 2026-07-22 (crash-b495df03…). `www.` prefix
/// check byte-sliced mid-char when a multi-byte char (the U+FFFD from NUL
/// replacement) followed `w`.
#[test]
fn test_fuzz_regression_www_prefix_char_boundary() {
    let bytes: &[u8] = &[
        49, 46, 32, 32, 103, 114, 97, 112, 104, 10, 32, 32, 32, 32, 81, 0, 0, 0, 32, 116, 119, 32,
        32, 255, 32, 32, 105, 110, 100, 101, 110, 116, 101, 100, 32, 99, 111, 100, 111, 32, 108,
        105, 110, 101, 115, 46, 10, 32, 32, 32, 10, 32, 32, 255, 32, 32, 105, 110, 100, 101, 110,
        116, 101, 100, 32, 99, 111, 100, 101, 10, 8, 0, 0, 0, 0, 0, 0, 0, 32, 98, 108, 111, 99,
        107, 32, 113, 0, 0, 0, 0, 0, 0, 0, 253, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 117, 111, 116, 101, 46, 10,
    ];
    let s = String::from_utf8_lossy(bytes);
    let doc = Document::parse(&s);
    let _ = ast::html::to_html(&doc);
    // Also the minimal form:
    let doc = Document::parse("w\0ww.x");
    let _ = ast::html::to_html(&doc);
}

#[test]
fn test_tree_depth_is_capped() {
    // 100k nested emphasis pairs would build a 100k-deep tree without the
    // flattening cap; recursive consumers would then overflow. Assert the
    // documented bound holds.
    let n = 100_000;
    let deep = format!("{}x{}", "*a ".repeat(n), " a*".repeat(n));
    let doc = Document::parse(&deep);
    let mut max_depth = 0usize;
    // Iterative depth walk.
    let mut stack: Vec<(ast::NodeRef<'_>, usize)> = doc
        .blocks()
        .iter()
        .map(|b| (ast::NodeRef::Block(b), 1))
        .collect();
    while let Some((node, d)) = stack.pop() {
        max_depth = max_depth.max(d);
        for c in node.children() {
            stack.push((c, d + 1));
        }
    }
    assert!(
        max_depth <= ast::MAX_TREE_DEPTH,
        "tree depth {max_depth} exceeds documented cap {}",
        ast::MAX_TREE_DEPTH
    );

    // Same for hostile blockquote/list nesting.
    let deep_blocks = "> ".repeat(100_000);
    let doc = Document::parse(&deep_blocks);
    let _ = doc.node_count();
}

/// Throughput sanity on a large real document (the 205 KB spec itself).
/// Loose bound: the DW-2.3 fuzz gate is <1s per input; a real document of
/// this size must parse far under that even in debug builds.
#[test]
fn test_large_real_document_parses_quickly() {
    let src = std::fs::read_to_string(format!(
        "{}/tests/spec/commonmark-spec.txt",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("vendored spec.txt readable");
    let t = Instant::now();
    let doc = Document::parse(&src);
    let dt = t.elapsed();
    assert!(
        doc.node_count() > 5_000,
        "spec.txt should parse to many nodes"
    );
    assert!(
        dt < Duration::from_secs(2),
        "205 KB real document took {dt:?}"
    );
}
