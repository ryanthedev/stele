//! Edge-case and behavior tests past the DW floor: every named edge case
//! from the phase spec, plus seams the implementation surfaced (sizer
//! integration, style emission, range clamping, list mechanics).

use ast::{Document, InlineKind, NodeId, NodeRef};
use layout::{
    CellSize, IntrinsicSizer, LayoutConfig, LayoutTree, Line, LineItem, NullSizer, Run, Semantic,
    StyleId, layout,
};
use width::{WidthConfig, WidthEngine};

fn engine() -> WidthEngine {
    WidthEngine::new(WidthConfig::default())
}

fn lay(doc: &Document, width: u16) -> LayoutTree {
    layout(doc, width, &LayoutConfig::default(), &engine(), &NullSizer)
}

fn all_runs(tree: &LayoutTree) -> Vec<&Run> {
    tree.lines(0..tree.line_count())
        .filter_map(|l| match l {
            Line::Items(items) => Some(items.iter().filter_map(|item| match item {
                LineItem::Run(run) => Some(run),
                LineItem::Box(_) => None,
            })),
            Line::Reserved(_) => None,
        })
        .flatten()
        .collect()
}

fn text_of(tree: &LayoutTree) -> Vec<String> {
    tree.lines(0..tree.line_count())
        .map(|l| match l {
            Line::Items(items) => items
                .iter()
                .map(|item| match item {
                    LineItem::Run(run) => run.text.clone(),
                    // Visible in goldens so an inline box can never slip into
                    // a line unnoticed the way a silent skip would allow.
                    LineItem::Box(b) => format!("<box {}x{}>", b.cols, b.rows),
                })
                .collect::<String>(),
            // The container prefix is shown too: a reserved line carries the
            // gutter bar / list marker for its row, and rendering the box
            // alone is exactly how a list item whose only child is an image
            // lost its marker unnoticed.
            Line::Reserved(r) => format!(
                "{}<reserved {}x{}>",
                r.prefix
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>(),
                r.boxed.cols,
                r.boxed.rows
            ),
        })
        .collect()
}

fn assert_bound(tree: &LayoutTree) {
    for (i, line) in tree.lines(0..tree.line_count()).enumerate() {
        assert!(
            line.width() <= tree.width(),
            "line {i} width {} exceeds bound {}",
            line.width(),
            tree.width()
        );
    }
}

/// Sizes every image/math node at one fixed natural size.
struct FixedSizer(CellSize);

impl IntrinsicSizer for FixedSizer {
    fn size(&self, node: NodeId, doc: &Document) -> Option<CellSize> {
        match doc.node(node) {
            Some(NodeRef::Inline(i))
                if matches!(i.kind, InlineKind::Image { .. } | InlineKind::Math { .. }) =>
            {
                Some(self.0)
            }
            _ => None,
        }
    }
}

// ---- named edge cases ------------------------------------------------

#[test]
fn test_empty_document_yields_empty_tree() {
    let doc = Document::parse("");
    let tree = lay(&doc, 80);
    assert_eq!(tree.line_count(), 0);
    assert!(tree.is_empty());
    assert_eq!(tree.lines(0..10).count(), 0, "range past end must clamp");
}

#[test]
fn test_unbreakable_word_break_anywhere_fallback() {
    let word = "a".repeat(60);
    let doc = Document::parse(&word);
    let tree = lay(&doc, 24);
    assert_bound(&tree);
    assert!(tree.line_count() > 1, "60-cell word must span lines at 24");
    let joined: String = text_of(&tree).join("");
    assert_eq!(joined, word, "break-anywhere must not lose characters");
}

#[test]
fn test_code_block_clips_never_wraps() {
    let long = "x".repeat(90);
    let src = format!("```\n{long}\nshort\n```\n");
    let doc = Document::parse(&src);
    let tree = lay(&doc, 24);
    assert_bound(&tree);
    assert_eq!(
        tree.line_count(),
        2,
        "two source lines must stay two layout lines (clip, no wrap)"
    );
    let lines = text_of(&tree);
    assert!(
        lines[0].ends_with('\u{2026}'),
        "clipped code line carries the indicator: {:?}",
        lines[0]
    );
    assert_eq!(lines[1], "short");
    let has_indicator_style = all_runs(&tree)
        .iter()
        .any(|r| r.style_id == StyleId::Semantic(Semantic::OverflowIndicator));
    assert!(has_indicator_style);
}

#[test]
fn test_nested_list_indent_clamped_leaves_content_room() {
    let mut src = String::new();
    for depth in 0..15 {
        src.push_str(&"  ".repeat(depth));
        src.push_str("- deepitem\n");
    }
    let doc = Document::parse(&src);
    let tree = lay(&doc, 24);
    assert_bound(&tree);
    // The deepest item's text must still be present (possibly broken),
    // not swallowed by indent.
    let joined: String = text_of(&tree).join("\n");
    assert!(
        joined.matches("deepitem").count() >= 10,
        "deep items lost to indent:\n{joined}"
    );
}

#[test]
fn test_image_scaled_to_fit_preserving_aspect() {
    let doc = Document::parse("![diagram](d.png)\n");
    let sizer = FixedSizer(CellSize {
        cols: 200,
        rows: 50,
    });
    let tree = layout(&doc, 100, &LayoutConfig::default(), &engine(), &sizer);
    assert_bound(&tree);
    let reserved: Vec<_> = tree
        .lines(0..tree.line_count())
        .filter_map(|l| match l {
            Line::Reserved(r) => Some(r.boxed),
            _ => None,
        })
        .collect();
    assert!(!reserved.is_empty(), "sized image must reserve lines");
    let r = &reserved[0];
    assert_eq!(r.cols, 100, "wider-than-viewport image scales to width");
    assert_eq!(r.rows, 25, "aspect ratio preserved (200x50 -> 100x25)");
    assert_eq!(
        reserved.len(),
        usize::from(r.rows),
        "one Reserved line per row"
    );
    assert!(
        reserved
            .iter()
            .all(|x| (x.node_id, x.cols, x.rows) == (r.node_id, r.cols, r.rows)),
        "all rows carry the same box identity and extent"
    );
    // ...but each names its own position in the box. A consumer that sees
    // only a scrolled slice of these lines has no other way to tell "row 0
    // of the box" from "row 7, the top 7 scrolled off"; the media sink needs
    // exactly that to crop a top-truncated placement instead of painting the
    // whole image downward over the text below it.
    assert_eq!(
        reserved.iter().map(|x| x.row).collect::<Vec<_>>(),
        (0..r.rows).collect::<Vec<_>>(),
        "row index must ascend 0..rows across the box's lines"
    );
}

#[test]
fn test_table_overflow_ladder_rung1_wrap_in_cell() {
    // Natural widths overflow 40 but min widths fit: cells wrap, no
    // clipping, bound held.
    let src = "| One | Two |\n| --- | --- |\n| a long sentence that wraps | another long sentence here |\n";
    let doc = Document::parse(src);
    let tree = lay(&doc, 40);
    assert_bound(&tree);
    let body_rows = tree.line_count() - 2; // header line + rule
    assert!(body_rows > 1, "the row must wrap onto multiple lines");
    assert!(
        !all_runs(&tree)
            .iter()
            .any(|r| r.style_id == StyleId::Semantic(Semantic::OverflowIndicator)),
        "rung 1 needs no clipping"
    );
}

#[test]
fn test_table_overflow_ladder_rung2_per_column_floors() {
    // Min-content (unbreakable words) exceeds the width: columns squeeze
    // below min, words break anywhere, still no clipping.
    let src = "| A | B |\n| - | - |\n| aaaaaaaaaaaaaaaaaaaaaaaa | bbbbbbbbbbbbbbbbbbbbbbbb |\n";
    let doc = Document::parse(src);
    let tree = lay(&doc, 24);
    assert_bound(&tree);
    assert!(
        !all_runs(&tree)
            .iter()
            .any(|r| r.style_id == StyleId::Semantic(Semantic::OverflowIndicator)),
        "rung 2 fits without clipping"
    );
    // Both words survive intact across broken lines.
    let joined: String = text_of(&tree).join("");
    assert_eq!(joined.matches('a').count(), 24, "the 24 a's survive intact");
    assert_eq!(joined.matches('b').count(), 24);
}

#[test]
fn test_table_overflow_ladder_rung3_clip_with_indicator() {
    let cols = 15;
    let header: String = (0..cols).map(|i| format!("| h{i} ")).collect::<String>() + "|\n";
    let rule: String = "| - ".repeat(cols) + "|\n";
    let row: String = "| x ".repeat(cols) + "|\n";
    let doc = Document::parse(&(header + &rule + &row));
    let tree = lay(&doc, 24);
    assert_bound(&tree);
    assert!(
        all_runs(&tree)
            .iter()
            .any(|r| r.style_id == StyleId::Semantic(Semantic::OverflowIndicator)),
        "rung 3 must clip with an indicator"
    );
}

// ---- sizer seam ------------------------------------------------------

#[test]
fn test_null_sizer_lays_out_image_as_alt_text() {
    let doc = Document::parse("![friendly alt text](image.png)\n");
    let tree = lay(&doc, 80);
    let joined = text_of(&tree).join("");
    assert!(joined.contains("friendly alt text"), "{joined:?}");
    assert!(
        all_runs(&tree)
            .iter()
            .any(|r| r.style_id == StyleId::Semantic(Semantic::ImageAlt)),
        "alt text carries the ImageAlt role"
    );
    assert!(
        !tree
            .lines(0..tree.line_count())
            .any(|l| matches!(l, Line::Reserved(_))),
        "NullSizer must never reserve"
    );
}

#[test]
fn test_null_sizer_lays_out_math_as_tex_source() {
    let doc = Document::parse("Euler: $e^{i\\pi}+1=0$\n");
    let tree = lay(&doc, 80);
    let joined = text_of(&tree).join("");
    assert!(joined.contains("e^{i\\pi}+1=0"), "{joined:?}");
    assert!(
        all_runs(&tree)
            .iter()
            .any(|r| r.style_id == StyleId::Semantic(Semantic::MathTex))
    );
}

#[test]
fn test_sizer_receives_resolvable_node_id() {
    // The NodeId handed to the sizer must resolve, via Document::node, to
    // the very image node (the P6 contract).
    struct Probe;
    impl IntrinsicSizer for Probe {
        fn size(&self, node: NodeId, doc: &Document) -> Option<CellSize> {
            match doc.node(node) {
                Some(NodeRef::Inline(i)) => match &i.kind {
                    InlineKind::Image { dest, .. } => {
                        assert_eq!(dest, "p6/target.png");
                        Some(CellSize { cols: 10, rows: 2 })
                    }
                    _ => None,
                },
                _ => None,
            }
        }
    }
    let doc = Document::parse("![x](p6/target.png)\n");
    let tree = layout(&doc, 80, &LayoutConfig::default(), &engine(), &Probe);
    let reserved: Vec<_> = tree
        .lines(0..tree.line_count())
        .filter_map(|l| match l {
            Line::Reserved(r) => Some(r.boxed),
            _ => None,
        })
        .collect();
    assert_eq!(reserved.len(), 2);
    // And the emitted Reserved carries that same id.
    match doc.node(reserved[0].node_id) {
        Some(NodeRef::Inline(i)) => {
            assert!(matches!(&i.kind, InlineKind::Image { dest, .. } if dest == "p6/target.png"))
        }
        other => panic!("Reserved node_id resolved to {other:?}"),
    }
}

// ---- style emission --------------------------------------------------

#[test]
fn test_layout_emits_only_semantic_style_ids() {
    // The Capture range belongs to P7; layout must never mint one.
    let src = "# h *em* **st** `code` [l](u) ~~s~~\n\n> q\n\n- i\n\n| a |\n| - |\n| b |\n";
    let doc = Document::parse(src);
    for width in [24u16, 100] {
        let tree = lay(&doc, width);
        for run in all_runs(&tree) {
            assert!(
                matches!(run.style_id, StyleId::Semantic(_)),
                "layout emitted {:?}",
                run.style_id
            );
        }
    }
}

#[test]
fn test_semantic_roles_reach_runs() {
    let doc = Document::parse("## Head *emph* **strong** `code` [link](u)\n");
    let tree = lay(&doc, 100);
    let styles: Vec<StyleId> = all_runs(&tree).iter().map(|r| r.style_id).collect();
    for want in [
        Semantic::Heading(2),
        Semantic::Emph,
        Semantic::Strong,
        Semantic::CodeInline,
        Semantic::Link,
    ] {
        assert!(
            styles.contains(&StyleId::Semantic(want)),
            "missing role {want:?} in {styles:?}"
        );
    }
}

#[test]
fn test_run_widths_match_engine_measurement() {
    let doc = Document::parse("plain 中文 and 👍 emoji\n");
    let tree = lay(&doc, 100);
    let engine = engine();
    for run in all_runs(&tree) {
        assert_eq!(
            usize::from(run.width),
            engine.display_width(&run.text),
            "run width disagrees with engine for {:?}",
            run.text
        );
    }
}

// ---- structure -------------------------------------------------------

#[test]
fn test_width_clamps_to_config_range() {
    let doc = Document::parse("hello\n");
    assert_eq!(lay(&doc, 5).width(), 24, "below min clamps up");
    assert_eq!(lay(&doc, 500).width(), 100, "above max clamps down");
    assert_eq!(lay(&doc, 60).width(), 60, "in range passes through");
}

#[test]
fn test_lines_range_clamps_not_panics() {
    let doc = Document::parse("a\n\nb\n\nc\n");
    let tree = lay(&doc, 80);
    let n = tree.line_count();
    assert_eq!(tree.lines(0..usize::MAX).count(), n);
    assert_eq!(tree.lines(n + 5..n + 9).count(), 0);
    #[allow(clippy::reversed_empty_ranges)]
    let inverted = tree.lines(3..1).count();
    assert_eq!(inverted, 0);
}

#[test]
fn test_tight_vs_loose_list_spacing() {
    let tight = lay(&Document::parse("- a\n- b\n- c\n"), 80);
    assert_eq!(tight.line_count(), 3, "tight list: no blank rows");
    let loose = lay(&Document::parse("- a\n\n- b\n\n- c\n"), 80);
    assert_eq!(loose.line_count(), 5, "loose list: blank row between items");
}

#[test]
fn test_ordered_list_start_and_delimiter() {
    let doc = Document::parse("5) five\n6) six\n");
    let lines = text_of(&lay(&doc, 80));
    assert_eq!(lines, vec!["5) five", "6) six"]);
}

#[test]
fn test_task_list_markers() {
    let doc = Document::parse("- [x] done\n- [ ] todo\n");
    let tree = lay(&doc, 80);
    let lines = text_of(&tree);
    assert_eq!(lines, vec!["[x] done", "[ ] todo"]);
    assert!(
        all_runs(&tree)
            .iter()
            .any(|r| r.style_id == StyleId::Semantic(Semantic::TaskMarker))
    );
}

#[test]
fn test_hard_break_forces_new_line() {
    let doc = Document::parse("alpha  \nbeta\n");
    let lines = text_of(&lay(&doc, 80));
    assert_eq!(lines, vec!["alpha", "beta"]);
}

#[test]
fn test_nested_blockquote_prefix_stacks() {
    let doc = Document::parse("> > inner\n");
    let lines = text_of(&lay(&doc, 80));
    assert_eq!(lines, vec!["\u{2502} \u{2502} inner"]);
}

#[test]
fn test_table_right_alignment_pads_left() {
    let doc = Document::parse("| Num |\n| --: |\n| 7 |\n");
    let lines = text_of(&lay(&doc, 80));
    // Column width = max("Num", "7") = 3; the body cell is right-aligned.
    assert_eq!(lines[2], "  7");
}

#[test]
fn test_thematic_break_spans_width_exactly() {
    let doc = Document::parse("---\n");
    let tree = lay(&doc, 40);
    assert_eq!(tree.line_count(), 1);
    let line = tree.lines(0..1).next().unwrap();
    assert_eq!(line.width(), 40, "rule fills the content width");
}

#[test]
fn test_reflow_at_different_widths_from_same_doc() {
    // Beyond DW-4.4's fixture sweep: several widths from one retained doc,
    // each equal to a fresh parse+layout.
    let src = "# T\n\nSome paragraph that wraps at narrow widths only.\n\n- one\n- two\n";
    let doc = Document::parse(src);
    for w in [24u16, 37, 61, 100] {
        let reflow = lay(&doc, w);
        let fresh = lay(&Document::parse(src), w);
        assert_eq!(reflow, fresh, "width {w}");
    }
}

// --- Phase 4 gate-review regressions: u16 width-arithmetic overflow ---
// A token wider than u16::MAX cells must not wrap around a fit check. These
// assert against the engine-remeasured painted text, NOT Run.width, so a run
// that lies about its own width cannot make the assertion pass.

/// Real painted width of every line, measured independently of `Run.width`.
fn assert_bound_remeasured(tree: &LayoutTree) {
    let eng = engine();
    for (i, text) in text_of(tree).iter().enumerate() {
        let painted = eng.display_width(text);
        assert!(
            painted <= tree.width() as usize,
            "line {i}: painted {painted} cells exceeds bound {} — text len {}",
            tree.width(),
            text.len()
        );
    }
}

#[test]
fn test_unbreakable_word_past_u16_is_still_bounded() {
    // 65_600 cells: truncates to 64 under a bare `as u16`, slipping the
    // break-anywhere guard. Must break by cluster and stay within width.
    let doc = Document::parse(&"a".repeat(65_600));
    let tree = lay(&doc, 100);
    assert_bound_remeasured(&tree);
    assert_bound(&tree); // Run.width must also be honest, not just the text
}

#[test]
fn test_code_line_past_u16_is_clipped_not_emitted_whole() {
    let src = format!("```\n{}\n```\n", "x".repeat(65_600));
    let doc = Document::parse(&src);
    let tree = lay(&doc, 80);
    assert_bound_remeasured(&tree);
    // Code is clipped, never wrapped: a single content line out.
    let content: Vec<_> = text_of(&tree)
        .into_iter()
        .filter(|l| l.contains('x'))
        .collect();
    assert_eq!(content.len(), 1, "code must clip to one line, not wrap");
    assert!(content[0].contains('…'), "clip indicator expected");
}

#[test]
fn test_many_column_table_neither_panics_nor_overflows() {
    // >21_846 columns overflows u16 separator math (debug panic / release
    // wrap); the composed row also sums past u16::MAX. Must survive both.
    let n = 22_000;
    let header = format!("|{}\n", "a|".repeat(n));
    let delim = format!("|{}\n", "-|".repeat(n));
    let doc = Document::parse(&format!("{header}{delim}"));
    let tree = lay(&doc, 100);
    assert_bound_remeasured(&tree);
}

// ---- inline media boxes ----------------------------------------------

/// Every `LineItem::Box` on the line, with the line index it sits on.
fn inline_boxes(tree: &LayoutTree) -> Vec<(usize, layout::Reserved)> {
    tree.lines(0..tree.line_count())
        .enumerate()
        .flat_map(|(i, l)| match l {
            Line::Items(items) => items
                .iter()
                .filter_map(|item| match item {
                    LineItem::Box(b) => Some((i, *b)),
                    LineItem::Run(_) => None,
                })
                .collect::<Vec<_>>(),
            Line::Reserved(_) => Vec::new(),
        })
        .collect()
}

#[test]
fn test_one_row_inline_box_rides_the_baseline_inside_its_sentence() {
    // The reported bug: a one-row formula used to flush the line before and
    // after itself, shattering "before $x$ after" into three lines with the
    // spaces swallowed. It must stay one flowing line.
    let doc = Document::parse("Inline math like $e^{i\\pi}+1=0$ and more text.\n");
    let tree = layout(
        &doc,
        80,
        &LayoutConfig::default(),
        &engine(),
        &FixedSizer(CellSize { cols: 10, rows: 1 }),
    );

    let boxes = inline_boxes(&tree);
    assert_eq!(boxes.len(), 1, "the formula is one inline box: {boxes:?}");
    assert_eq!(boxes[0].1.rows, 1, "an inline box is always one row tall");

    assert!(
        !tree
            .lines(0..tree.line_count())
            .any(|l| matches!(l, Line::Reserved(_))),
        "a box that fits the baseline must not claim its own rows"
    );

    let text = text_of(&tree);
    let line = &text[boxes[0].0];
    assert!(
        line.contains("Inline math like <box 10x1> and more text."),
        "text, box, and BOTH surrounding spaces stay on one line: {line:?}"
    );
}

#[test]
fn test_inline_box_taller_than_one_row_still_claims_its_own_rows() {
    // Display math and block images want their own rows; only the baseline
    // case changed. A 2-row box must still reserve 2 whole lines.
    let doc = Document::parse("Before $$\\int_0^\\infty$$ after.\n");
    let tree = layout(
        &doc,
        80,
        &LayoutConfig::default(),
        &engine(),
        &FixedSizer(CellSize { cols: 10, rows: 2 }),
    );
    assert!(inline_boxes(&tree).is_empty(), "not an inline box");
    let reserved: Vec<_> = tree
        .lines(0..tree.line_count())
        .filter_map(|l| match l {
            Line::Reserved(r) => Some(r.boxed),
            _ => None,
        })
        .collect();
    assert_eq!(reserved.len(), 2, "one Reserved line per row");
}

#[test]
fn test_inline_box_wider_than_the_content_column_claims_its_own_rows() {
    // It cannot share a line with anything, so the standalone path (which
    // also scales it to fit) is the only correct answer.
    let doc = Document::parse("Text $wide$ text.\n");
    let tree = layout(
        &doc,
        24,
        &LayoutConfig::default(),
        &engine(),
        &FixedSizer(CellSize { cols: 90, rows: 1 }),
    );
    assert!(inline_boxes(&tree).is_empty(), "too wide to ride a line");
    assert!(
        tree.lines(0..tree.line_count())
            .any(|l| matches!(l, Line::Reserved(_))),
        "falls back to a standalone reserved box"
    );
}

#[test]
fn test_inline_box_wraps_to_the_next_line_when_it_does_not_fit() {
    // It wraps like a word rather than overflowing the content column.
    let doc = Document::parse("aaaaaaaaaa bbbbbbbbbb $x$ cccc\n");
    let tree = layout(
        &doc,
        24,
        &LayoutConfig::default(),
        &engine(),
        &FixedSizer(CellSize { cols: 8, rows: 1 }),
    );
    let boxes = inline_boxes(&tree);
    assert_eq!(boxes.len(), 1, "still exactly one box: {boxes:?}");
    assert!(
        boxes[0].0 > 0,
        "the box wrapped onto a later line instead of overflowing"
    );
    assert_bound(&tree);
}

#[test]
fn test_inline_box_does_not_merge_the_runs_on_either_side_of_it() {
    // Text after a box must start a fresh run, never fold into the run that
    // preceded the box (which would silently reorder the line's content).
    let doc = Document::parse("aa $x$ bb\n");
    let tree = layout(
        &doc,
        80,
        &LayoutConfig::default(),
        &engine(),
        &FixedSizer(CellSize { cols: 4, rows: 1 }),
    );
    let items: Vec<_> = tree
        .lines(0..tree.line_count())
        .filter_map(|l| match l {
            Line::Items(items) if !items.is_empty() => Some(items.clone()),
            _ => None,
        })
        .next()
        .expect("one content line");
    let box_at = items
        .iter()
        .position(|i| matches!(i, LineItem::Box(_)))
        .expect("a box on the line");
    assert!(box_at > 0, "text precedes the box");
    assert!(box_at + 1 < items.len(), "text follows the box");
    assert!(matches!(items[box_at - 1], LineItem::Run(_)));
    assert!(matches!(items[box_at + 1], LineItem::Run(_)));
}

#[test]
fn test_display_math_claims_its_own_rows_even_when_it_measures_one_row() {
    // Regression: flow was decided purely by measured size, so a SHORT display
    // formula like `$$E=mc^2$$` (one row) rode the text baseline as if the
    // author had written `$…$`. `$$` is an explicit statement that the formula
    // stands apart from the prose; size must not override it.
    let doc = Document::parse("Einstein wrote $$E=mc^2$$ on the board.\n");
    let tree = layout(
        &doc,
        80,
        &LayoutConfig::default(),
        &engine(),
        &FixedSizer(CellSize { cols: 8, rows: 1 }),
    );
    assert!(
        inline_boxes(&tree).is_empty(),
        "display math must never ride the baseline: {:?}",
        text_of(&tree)
    );
    assert!(
        tree.lines(0..tree.line_count())
            .any(|l| matches!(l, Line::Reserved(_))),
        "display math claims its own row: {:?}",
        text_of(&tree)
    );
}

#[test]
fn test_inline_math_of_the_same_size_still_rides_the_baseline() {
    // The other half of the pair: identical measured size, `$…$` instead of
    // `$$…$$`, must still flow inside the sentence. Together these two pin the
    // rule as "author intent decides", not "size decides".
    let doc = Document::parse("Einstein wrote $E=mc^2$ on the board.\n");
    let tree = layout(
        &doc,
        80,
        &LayoutConfig::default(),
        &engine(),
        &FixedSizer(CellSize { cols: 8, rows: 1 }),
    );
    let boxes = inline_boxes(&tree);
    assert_eq!(boxes.len(), 1, "inline math rides the line: {boxes:?}");
    let line = &text_of(&tree)[boxes[0].0];
    assert!(
        line.contains("Einstein wrote <box 8x1> on the board."),
        "one flowing sentence: {line:?}"
    );
}

// ---------------------------------------------------------------------------
// Container prefixes on reserved media rows, and containers with no content.
//
// All four of these are *position* assertions: which column the box starts at,
// which marker text precedes it, which row an item occupies. The suite that
// missed them asserted `line.width() <= tree.width()` — true of a box drawn
// from column 0 across its own blockquote gutter — and "one Reserved line per
// row", true of a box whose list marker was never emitted at all.

#[test]
fn test_reserved_box_in_a_blockquote_starts_after_the_gutter_on_every_row() {
    let doc = Document::parse("> before\n>\n> ![tall](t.png)\n>\n> after\n");
    let tree = layout(
        &doc,
        40,
        &LayoutConfig::default(),
        &engine(),
        &FixedSizer(CellSize { cols: 6, rows: 4 }),
    );
    assert_bound(&tree);
    let rows: Vec<(String, u16, u16)> = tree
        .lines(0..tree.line_count())
        .filter_map(|l| match l {
            Line::Reserved(r) => Some((
                r.prefix
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>(),
                r.prefix
                    .iter()
                    .fold(0u16, |acc, run| acc.saturating_add(run.width)),
                r.boxed.row,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(rows.len(), 4, "a 4-row box: {:?}", text_of(&tree));
    for (i, (text, col, row)) in rows.iter().enumerate() {
        assert_eq!(text, "\u{2502} ", "row {i} keeps the gutter bar");
        assert_eq!(*col, 2, "row {i} starts the box at column 2, past the bar");
        assert_eq!(*row, i as u16, "row {i} names its own index in the box");
    }
    // And the gutter must carry the blockquote role, not fall out as plain
    // text — the painter dims it, which is how a quote reads as a quote.
    let styles: Vec<StyleId> = tree
        .lines(0..tree.line_count())
        .filter_map(|l| match l {
            Line::Reserved(r) => r.prefix.first().map(|run| run.style_id),
            _ => None,
        })
        .collect();
    assert!(
        styles
            .iter()
            .all(|s| *s == StyleId::Semantic(Semantic::BlockquoteMarker)),
        "{styles:?}"
    );
}

#[test]
fn test_list_item_whose_only_child_is_a_box_still_emits_its_marker() {
    // The reader-visible symptom: an ordered list that begins at "2.".
    let doc = Document::parse("1. ![tall](t.png)\n2. second\n");
    let tree = layout(
        &doc,
        40,
        &LayoutConfig::default(),
        &engine(),
        &FixedSizer(CellSize { cols: 6, rows: 3 }),
    );
    let text = text_of(&tree);
    assert_eq!(
        text,
        vec![
            "1. <reserved 6x3>".to_string(),
            "   <reserved 6x3>".to_string(),
            "   <reserved 6x3>".to_string(),
            "2. second".to_string(),
        ],
        "the marker rides the box's first row and the indent the rest"
    );
    // Spelled out, because "1." is the whole finding: the list must not
    // renumber itself on screen.
    assert!(
        text[0].starts_with("1. "),
        "item 1's marker vanished, so the list starts at 2: {text:?}"
    );
}

#[test]
fn test_nested_list_box_starts_past_both_markers() {
    let doc = Document::parse("- outer\n  - ![tall](t.png)\n");
    let tree = layout(
        &doc,
        40,
        &LayoutConfig::default(),
        &engine(),
        &FixedSizer(CellSize { cols: 6, rows: 2 }),
    );
    assert_bound(&tree);
    let cols: Vec<(String, u16)> = tree
        .lines(0..tree.line_count())
        .filter_map(|l| match l {
            Line::Reserved(r) => Some((
                r.prefix
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>(),
                r.prefix
                    .iter()
                    .fold(0u16, |acc, run| acc.saturating_add(run.width)),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        cols,
        vec![("  \u{2022} ".to_string(), 4), ("    ".to_string(), 4),],
        "outer indent + inner bullet, then pure indent: {:?}",
        text_of(&tree)
    );
}

#[test]
fn test_empty_list_item_still_occupies_a_row_with_its_marker() {
    let tree = lay(&Document::parse("1. one\n2.\n3. three\n"), 40);
    let text = text_of(&tree);
    assert_eq!(
        text,
        vec![
            "1. one".to_string(),
            "2. ".to_string(),
            "3. three".to_string()
        ],
        "an empty item must not vanish and renumber the list"
    );
}

#[test]
fn test_empty_bullet_and_task_items_still_occupy_their_rows() {
    let tree = lay(&Document::parse("- [ ] task\n-\n- [x] done\n"), 40);
    assert_eq!(
        text_of(&tree),
        vec![
            "[ ] task".to_string(),
            "\u{2022} ".to_string(),
            "[x] done".to_string()
        ],
        "three items in, three rows out"
    );
}

/// A bare `>` is an empty *container*, and an empty container occupies its
/// line — the same answer an empty list item now gives. Asserted on the
/// rendered row and on where the gutter glyph sits, not on a line count: a
/// count cannot tell "the quote row is present with its bar in columns 0..2"
/// from "some blank row appeared somewhere".
#[test]
fn test_empty_blockquote_still_occupies_a_row_with_its_gutter() {
    let tree = lay(&Document::parse("before\n\n>\n\nafter\n"), 40);
    assert_eq!(
        text_of(&tree),
        vec![
            "before".to_string(),
            String::new(),
            "\u{2502} ".to_string(),
            String::new(),
            "after".to_string(),
        ],
        "a bare `>` must render one gutter row, not vanish"
    );
    // The glyph is at column 0 and is 2 cells wide, and it is a blockquote
    // marker rather than stray text — the painter dims it on that role, which
    // is how the empty quote reads as a quote and not as an indented blank.
    let Line::Items(items) = tree.lines(2..3).next().expect("the quote row") else {
        panic!("the quote row is a text line, not a reserved one");
    };
    let runs: Vec<(&str, u16, StyleId)> = items
        .iter()
        .map(|item| match item {
            LineItem::Run(r) => (r.text.as_str(), r.width, r.style_id),
            LineItem::Box(_) => panic!("no media box on an empty quote row"),
        })
        .collect();
    assert_eq!(
        runs,
        vec![(
            "\u{2502} ",
            2u16,
            StyleId::Semantic(Semantic::BlockquoteMarker)
        )],
        "the row is exactly the gutter bar, starting at column 0"
    );
}

/// Same defect, same function, reachable from ordinary markdown: an empty
/// footnote definition dropped its `[^a] ` label entirely, so a document with
/// a `[^a]` reference showed no definition at all.
#[test]
fn test_empty_footnote_definition_still_occupies_a_row_with_its_label() {
    let tree = lay(&Document::parse("text[^a]\n\n[^a]:\n"), 40);
    assert_eq!(
        text_of(&tree),
        vec!["text[^a]".to_string(), String::new(), "[^a] ".to_string(),],
        "an empty footnote definition must still show its label"
    );
    let Line::Items(items) = tree.lines(2..3).next().expect("the definition row") else {
        panic!("the definition row is a text line, not a reserved one");
    };
    let runs: Vec<(&str, u16, StyleId)> = items
        .iter()
        .map(|item| match item {
            LineItem::Run(r) => (r.text.as_str(), r.width, r.style_id),
            LineItem::Box(_) => panic!("no media box on an empty definition row"),
        })
        .collect();
    assert_eq!(
        runs,
        vec![("[^a] ", 5u16, StyleId::Semantic(Semantic::FootnoteLabel))],
        "the row is exactly the label, starting at column 0"
    );
}

#[test]
fn test_thematic_break_spans_the_content_width_exactly_under_ambiguous_wide() {
    // `─` is East Asian *Ambiguous*: 2 cells under this policy, so an odd
    // content width used to come out one cell short. The default-policy test
    // (`test_thematic_break_spans_width_exactly`) cannot see it — there dw is
    // 1 and the division is exact for every width.
    let wide = WidthEngine::new(WidthConfig {
        ambiguous_wide: true,
    });
    for width in [24u16, 25, 40, 41, 99] {
        let doc = Document::parse("para\n\n---\n");
        let tree = layout(&doc, width, &LayoutConfig::default(), &wide, &NullSizer);
        let rule = tree
            .lines(0..tree.line_count())
            .find(|l| match l {
                Line::Items(items) => items.iter().any(|i| match i {
                    LineItem::Run(r) => r.style_id == StyleId::Semantic(Semantic::Rule),
                    LineItem::Box(_) => false,
                }),
                Line::Reserved(_) => false,
            })
            .unwrap_or_else(|| panic!("no rule line at width {width}"));
        assert_eq!(
            rule.width(),
            tree.width(),
            "width {width}: rule is {} cells against a {}-cell content column",
            rule.width(),
            tree.width()
        );
    }
}

// ---- The heading outline (Phase 3) --------------------------------------

/// The plain text of the tree line at `index` — the oracle every outline
/// anchoring assertion below is checked against, so a recorded line index is
/// verified by *what is painted there* rather than by another index.
fn line_text_at(tree: &LayoutTree, index: usize) -> String {
    match tree.lines(index..index + 1).next() {
        Some(Line::Items(items)) => items
            .iter()
            .map(|item| match item {
                LineItem::Run(run) => run.text.clone(),
                LineItem::Box(_) => String::new(),
            })
            .collect(),
        _ => String::new(),
    }
}

#[test]
fn test_the_outline_records_every_heading_with_its_level_in_document_order() {
    let doc =
        Document::parse("# One\n\nbody\n\n### Three\n\nbody\n\n## Two\n\nbody\n\n###### Six\n");
    let tree = lay(&doc, 60);
    let levels: Vec<u8> = tree.outline().entries.iter().map(|e| e.level).collect();
    let texts: Vec<&str> = tree
        .outline()
        .entries
        .iter()
        .map(|e| e.text.as_str())
        .collect();
    assert_eq!(levels, vec![1, 3, 2, 6]);
    assert_eq!(texts, vec!["One", "Three", "Two", "Six"]);
}

#[test]
fn test_a_document_with_no_headings_has_an_empty_outline() {
    let tree = lay(&Document::parse("just a paragraph\n\nand another\n"), 40);
    assert!(tree.outline().is_empty());
    assert_eq!(tree.outline().len(), 0);
    assert_eq!(tree.outline().next_after(0), None);
    assert_eq!(tree.outline().previous_before(10), None);
    assert_eq!(tree.outline().line_of(0), None);
}

/// Heading text comes from the AST, not from the emitted runs — so the words
/// inside emphasis, a code span or a link survive. Collecting only the runs
/// styled `Semantic::Heading` would drop every one of them, because `flow`
/// overrides the style inside those inlines.
#[test]
fn test_outline_text_keeps_words_inside_emphasis_code_and_links() {
    let doc = Document::parse("## The *fast* `path` in [layout](./x.md)\n");
    let tree = lay(&doc, 60);
    assert_eq!(
        tree.outline().entries[0].text,
        "The fast path in layout",
        "a run-derived outline would read \"The  in \""
    );
}

/// A heading long enough to wrap is one entry anchored at its *first* line,
/// with its full text — not one entry per wrapped line, and not a truncated
/// title.
#[test]
fn test_a_wrapped_heading_is_one_entry_anchored_at_its_first_line() {
    let doc = Document::parse(
        "# A heading long enough that it certainly wraps across several lines at \
         a narrow layout width\n\nbody\n",
    );
    let tree = lay(&doc, 24);
    assert_eq!(tree.outline().len(), 1);
    assert_eq!(tree.outline().line_of(0), Some(0));
    assert!(
        tree.outline().entries[0].text.ends_with("layout width"),
        "the whole title must be recorded: {:?}",
        tree.outline().entries[0].text
    );
    assert!(
        tree.line_count() > 3,
        "the fixture must actually wrap, or this proves nothing"
    );
}

/// The nested case `first_line_of` cannot answer: a heading inside a
/// blockquote tags no line with its own node, so the outline's recorded line
/// is the only exact anchor — and it must point at the heading's own row.
#[test]
fn test_a_heading_inside_a_blockquote_is_still_addressable() {
    let doc = Document::parse("intro\n\n> # Quoted title\n>\n> quoted body\n\nafter\n");
    let tree = lay(&doc, 60);
    assert_eq!(tree.outline().len(), 1);
    let entry = &tree.outline().entries[0];
    assert_eq!(entry.text, "Quoted title");
    assert_eq!(entry.level, 1);
    assert!(
        tree.first_line_of(entry.block).is_some(),
        "the anchor block must be the enclosing top-level block, which does tag lines"
    );
    let line = tree.outline().line_of(0).expect("a recorded line");
    assert!(
        line_text_at(&tree, line).contains("Quoted title"),
        "the recorded line must be the heading's own row, got {:?}",
        line_text_at(&tree, line)
    );
    assert_eq!(tree.outline().line_for_block(entry.block), Some(line));
}

#[test]
fn test_outline_motions_are_strict_so_a_repeated_jump_keeps_moving() {
    let doc = Document::parse("# A\n\nbody\n\n# B\n\nbody\n\n# C\n");
    let tree = lay(&doc, 40);
    let lines: Vec<usize> = (0..3).map(|i| tree.outline().line_of(i).unwrap()).collect();
    assert_eq!(lines[0], 0);

    // From a heading's own line, `next_after` must not answer with that
    // heading — otherwise `]]` sticks.
    assert_eq!(tree.outline().next_after(lines[0]), Some(1));
    assert_eq!(tree.outline().next_after(lines[1]), Some(2));
    assert_eq!(tree.outline().next_after(lines[2]), None);
    assert_eq!(tree.outline().previous_before(lines[2]), Some(1));
    assert_eq!(tree.outline().previous_before(lines[0]), None);

    // `index_at_or_before` is the other rule: it *does* answer with the
    // heading you are standing on, which is what "which section am I in"
    // means.
    assert_eq!(tree.outline().index_at_or_before(lines[0]), Some(0));
    assert_eq!(tree.outline().index_at_or_before(lines[1] - 1), Some(0));
    assert_eq!(tree.outline().index_at_or_before(lines[1]), Some(1));
}

/// Layout is pure in its inputs, and the outline is part of the tree — so two
/// layouts of the same document at the same width must produce equal trees,
/// outline included.
#[test]
fn test_the_outline_is_part_of_the_trees_deterministic_identity() {
    let doc = Document::parse("# A\n\nbody\n\n## B\n\nbody\n");
    assert_eq!(lay(&doc, 50), lay(&doc, 50));
}

/// A setext heading (`===` / `---` underline) is a heading too, and reaches
/// the outline by the same path — the walker matches `BlockKind::Heading`,
/// not a syntax.
#[test]
fn test_a_setext_heading_reaches_the_outline() {
    let tree = lay(&Document::parse("Underlined\n==========\n\nbody\n"), 40);
    assert_eq!(tree.outline().len(), 1);
    assert_eq!(tree.outline().entries[0].level, 1);
    assert_eq!(tree.outline().entries[0].text, "Underlined");
}

/// The wash is not a line property — it is `Heading(1)`'s *background*, which
/// `wash_heading_lines` spreads by padding the line with `Heading(level)`
/// runs. So every other emitter that reaches for rung 1's colour through
/// `Heading(1)` inherits the band too, and two of them do exactly that: the
/// depth markers and the ember rule both index the ramp by *position in the
/// run*, so both start at rung 1 regardless of the heading's own level.
///
/// Shipped, that put a one-cell chip of wash under the first marker of every
/// H2–H6, and a sixth-of-the-measure block under the left end of every H1
/// rule. Neither is visible in the golden fixtures, which record text and not
/// colour, so this asserts on the style ids instead: a decoration takes
/// `HeadingRung`, which resolves to the same palette slot with no background.
///
/// H1 is the deliberate exception. Its marker sits *inside* its own band, so
/// it keeps the full `Heading` style — `HeadingRung` there would open a hole
/// at the left edge of the wash.
#[test]
fn test_heading_decorations_never_borrow_the_h1_wash() {
    for level in 2..=6u8 {
        let src = format!("{} Title\n", "#".repeat(usize::from(level)));
        let tree = lay(&Document::parse(&src), 60);
        let marker_styles: Vec<StyleId> = all_runs(&tree)
            .iter()
            .filter(|run| run.text.contains('\u{258c}') || run.text.contains('\u{2500}'))
            .map(|run| run.style_id)
            .collect();
        assert!(
            !marker_styles.is_empty(),
            "H{level} emitted no marker or rule runs to check"
        );
        assert!(
            !marker_styles.contains(&StyleId::Semantic(Semantic::Heading(1))),
            "H{level}'s decorations reach rung 1 through Heading(1), which carries the \
             H1 wash background: {marker_styles:?}"
        );
    }

    // The H1 rule is a separate line *below* the band, so it must not be
    // washed either — even though its own heading is the washed level.
    let tree = lay(&Document::parse("# Title\n"), 60);
    let rule_styles: Vec<StyleId> = all_runs(&tree)
        .iter()
        .filter(|run| run.text.contains('\u{2500}'))
        .map(|run| run.style_id)
        .collect();
    assert!(!rule_styles.is_empty(), "H1 emitted no rule to check");
    assert!(
        !rule_styles.contains(&StyleId::Semantic(Semantic::Heading(1))),
        "the H1 rule's first band carries the wash, painting a block under the \
         rule instead of only under the title: {rule_styles:?}"
    );
}
