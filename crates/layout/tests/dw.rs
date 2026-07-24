//! The phase's Done-When floor: DW-4.1 … DW-4.4 (DW-4.5 lives in
//! `tests/perf.rs` so it can be run `--release` on its own).
//!
//! Golden regeneration: `UPDATE_LAYOUT_GOLDENS=1 cargo test -p layout`.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;

use ast::Document;
use layout::{LayoutConfig, LayoutTree, Line, LineItem, NullSizer, layout};
use width::{WidthConfig, WidthEngine};

fn fixture_paths() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("fixtures/ exists")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    paths.sort();
    assert!(paths.len() >= 15, "fixture corpus unexpectedly small");
    paths
}

fn engine() -> WidthEngine {
    WidthEngine::new(WidthConfig::default())
}

fn lay(doc: &Document, width: u16) -> LayoutTree {
    layout(doc, width, &LayoutConfig::default(), &engine(), &NullSizer)
}

fn tree_hash(tree: &LayoutTree) -> u64 {
    let mut h = DefaultHasher::new();
    tree.hash(&mut h);
    h.finish()
}

/// Human-reviewable golden rendering: text content per line.
fn render(tree: &LayoutTree) -> String {
    let mut out = String::new();
    for line in tree.lines(0..tree.line_count()) {
        match line {
            Line::Items(items) => {
                for item in items {
                    match item {
                        LineItem::Run(run) => out.push_str(&run.text),
                        LineItem::Box(b) => out.push_str(&format!(
                            "[inline-box node={} {}x{}]",
                            b.node_id.index(),
                            b.cols,
                            b.rows
                        )),
                    }
                }
            }
            Line::Reserved(r) => out.push_str(&format!(
                "[reserved node={} {}x{}]",
                r.node_id.index(),
                r.cols,
                r.rows
            )),
        }
        out.push('\n');
    }
    out
}

#[test]
fn test_dw_4_1_deterministic_hash_over_fixtures() {
    for path in fixture_paths() {
        let src = std::fs::read_to_string(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        for width in [24u16, 100] {
            // Two fully independent layout calls: fresh parse, fresh
            // engine, fresh sizer.
            let doc_a = Document::parse(&src);
            let tree_a = layout(
                &doc_a,
                width,
                &LayoutConfig::default(),
                &engine(),
                &NullSizer,
            );
            let doc_b = Document::parse(&src);
            let tree_b = layout(
                &doc_b,
                width,
                &LayoutConfig::default(),
                &engine(),
                &NullSizer,
            );
            assert_eq!(
                tree_hash(&tree_a),
                tree_hash(&tree_b),
                "{name}@{width}: hashes differ"
            );
            assert_eq!(tree_a, tree_b, "{name}@{width}: trees differ structurally");
        }
    }
}

fn check_goldens(width: u16) {
    let golden_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/layout");
    let update = std::env::var_os("UPDATE_LAYOUT_GOLDENS").is_some();
    if update {
        std::fs::create_dir_all(&golden_dir).unwrap();
    }
    for path in fixture_paths() {
        let src = std::fs::read_to_string(&path).unwrap();
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let doc = Document::parse(&src);
        let rendered = render(&lay(&doc, width));
        let golden_path = golden_dir.join(format!("{stem}.w{width}.txt"));
        if update {
            std::fs::write(&golden_path, &rendered).unwrap();
        } else {
            let golden = std::fs::read_to_string(&golden_path).unwrap_or_else(|_| {
                panic!(
                    "missing golden {golden_path:?} — run UPDATE_LAYOUT_GOLDENS=1 cargo test -p layout"
                )
            });
            assert_eq!(
                rendered, golden,
                "{stem}@{width}: layout differs from golden snapshot"
            );
        }
    }
}

#[test]
fn test_dw_4_2_goldens_width_24() {
    check_goldens(24);
}

#[test]
fn test_dw_4_2_goldens_width_100() {
    check_goldens(100);
}

#[test]
fn test_dw_4_3_no_table_exceeds_width_bound() {
    // Stronger than the letter of the DW: *every* line of *every* fixture
    // (tables included — pathological-table.md is in the corpus) must fit
    // the clamped width, at the DW widths plus one mid width.
    for path in fixture_paths() {
        let src = std::fs::read_to_string(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let doc = Document::parse(&src);
        for width in [24u16, 40, 100] {
            let tree = lay(&doc, width);
            for (i, line) in tree.lines(0..tree.line_count()).enumerate() {
                assert!(
                    line.width() <= tree.width(),
                    "{name}@{width}: line {i} is {} cells wide (bound {})",
                    line.width(),
                    tree.width()
                );
            }
        }
    }
}

#[test]
fn test_dw_4_4_reflow_equals_fresh_parse_layout() {
    for path in fixture_paths() {
        let src = std::fs::read_to_string(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        // Retained document, laid out at w1 first (any hidden width/source
        // state from that call would poison the second).
        let doc = Document::parse(&src);
        let _at_w1 = lay(&doc, 100);
        let reflowed = lay(&doc, 24);
        // Fresh parse + layout straight at w2.
        let fresh_doc = Document::parse(&src);
        let fresh = lay(&fresh_doc, 24);
        assert_eq!(
            tree_hash(&reflowed),
            tree_hash(&fresh),
            "{name}: reflow hash differs from fresh parse+layout"
        );
        assert_eq!(reflowed, fresh, "{name}: reflow ≠ fresh parse+layout");
    }
}
