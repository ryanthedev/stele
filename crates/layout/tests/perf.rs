//! DW-4.5: a 1 MB document lays out in <100 ms in release.
//!
//! The assertion is enforced only in release builds (the DW's own terms);
//! a debug run still executes the full path and prints the measurement.
//! Run: `CARGO_TARGET_DIR=/tmp/stele-p4 cargo test --release -p layout --test perf -- --nocapture`

use std::time::Instant;

use ast::Document;
use layout::{LayoutConfig, NullSizer, layout};
use width::{WidthConfig, WidthEngine};

/// A realistic 1 MB+ document: headings, wrapping prose (with CJK and
/// emoji so the width engine does real work), lists, tables, code.
fn one_megabyte_markdown() -> String {
    let section = "\
## Section heading with some length to it

A paragraph of prose that wraps at any sane width, mixing *emphasis*, \
**strong text**, `inline code`, a [link](https://example.com/path), \
some 中文字符 for double-width measurement, and an emoji 👍 for cluster \
correction. The sentence keeps going long enough to guarantee several \
wrapped lines per paragraph at width one hundred.

- first item with a bit of text
- second item that is somewhat longer and will wrap on narrow widths
  - nested child item
- third item

| Name | Value | Description |
| ---- | ----: | ----------- |
| alpha | 1 | first row of the recurring table |
| beta | 22 | second row with more words in the cell |

```rust
fn compute(input: &str) -> usize {
    input.len() * 2 // representative code line
}
```

> A quoted remark closing out the section, long enough to wrap once.

";
    let mut doc = String::with_capacity(1_100_000);
    while doc.len() < 1_048_576 {
        doc.push_str(section);
    }
    doc
}

#[test]
fn test_dw_4_5_one_megabyte_under_100ms() {
    let src = one_megabyte_markdown();
    assert!(src.len() >= 1_048_576, "document must be at least 1 MB");
    let doc = Document::parse(&src); // parse cost is P2's budget, not ours
    let engine = WidthEngine::new(WidthConfig::default());
    let config = LayoutConfig::default();

    // Warm-up (page in the tree), then the measured run.
    let warm = layout(&doc, 100, &config, &engine, &NullSizer);
    assert!(!warm.is_empty());
    let start = Instant::now();
    let tree = layout(&doc, 100, &config, &engine, &NullSizer);
    let elapsed = start.elapsed();
    assert!(!tree.is_empty());

    println!(
        "DW-4.5: {} bytes -> {} lines in {:?} ({})",
        src.len(),
        tree.line_count(),
        elapsed,
        if cfg!(debug_assertions) {
            "debug build — 100 ms budget enforced in release"
        } else {
            "release build"
        }
    );
    if !cfg!(debug_assertions) {
        assert!(
            elapsed.as_millis() < 100,
            "DW-4.5 budget exceeded: {elapsed:?} >= 100 ms"
        );
    }
}
