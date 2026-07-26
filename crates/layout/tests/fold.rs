//! Phase 5's Done-When floor, at the layout level: folding a heading's
//! section collapses it to one marked line (DW-5.1), collapsing every
//! heading leaves exactly one line per top-level heading (DW-5.4), and no
//! folded or unfolded line exceeds the layout width (DW-5.6). DW-5.2, DW-5.3
//! and DW-5.5 need `AppState` (reload identity, media placement, scroll) and
//! live in `crates/stele`'s own suite instead.

use ast::{Document, NodeId};
use layout::{FoldState, Line, LineItem, NullSizer, layout, layout_with_folds};
use width::{WidthConfig, WidthEngine};

fn engine() -> WidthEngine {
    WidthEngine::new(WidthConfig::default())
}

fn folded(doc: &Document, width: u16, folds: &FoldState) -> layout::LayoutTree {
    layout_with_folds(
        doc,
        width,
        &layout::LayoutConfig::default(),
        &engine(),
        &NullSizer,
        folds,
    )
}

/// A line's full painted text — every run's text concatenated, prefix
/// included — the same shape a reader sees on screen. Never trusts a
/// `Run.width`: callers re-measure this through the width engine themselves.
fn line_text(line: &Line) -> String {
    match line {
        Line::Items(items) => items
            .iter()
            .map(|item| match item {
                LineItem::Run(run) => run.text.as_str(),
                LineItem::Box(_) => "",
            })
            .collect(),
        Line::Reserved(r) => r.prefix.iter().map(|run| run.text.as_str()).collect(),
    }
}

fn tree_text(tree: &layout::LayoutTree) -> String {
    tree.lines(0..tree.line_count())
        .map(line_text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn one_folded(id: NodeId) -> FoldState {
    let mut folds = FoldState::default();
    folds.toggle(id);
    folds
}

#[test]
fn test_dw_5_1_toggling_a_fold_collapses_to_one_marked_line_and_restores_exactly() {
    let doc = Document::parse("# One\n\nAlpha body.\n\n# Two\n\nBeta body.\n");
    let width = 40;
    let baseline = folded(&doc, width, &FoldState::default());
    let one_id = baseline.outline().entries[0].block;

    let mut folds = FoldState::default();
    folds.toggle(one_id);
    let tree = folded(&doc, width, &folds);

    let marker = line_text(tree.lines(0..1).next().expect("at least one line"));
    assert!(
        marker.contains("One") && marker.contains("hidden"),
        "the marker must carry the heading text and a hidden-line count: {marker:?}"
    );
    let all = tree_text(&tree);
    assert!(
        !all.contains("Alpha body"),
        "the folded section's body must not reach the tree at all: {all:?}"
    );
    assert!(
        all.contains("Two") && all.contains("Beta body"),
        "the sibling section must render exactly as before: {all:?}"
    );

    folds.toggle(one_id);
    let restored = folded(&doc, width, &folds);
    assert_eq!(
        restored, baseline,
        "re-toggling the same heading must restore the tree exactly"
    );
}

#[test]
fn test_dw_5_1_the_reported_hidden_count_matches_the_lines_actually_removed() {
    let doc = Document::parse("# One\n\nAlpha body.\n\nSecond paragraph.\n\n# Two\n\nBeta.\n");
    let width = 40;
    let baseline = folded(&doc, width, &FoldState::default());
    let one_id = baseline.outline().entries[0].block;
    let tree = folded(&doc, width, &one_folded(one_id));

    // The marker itself is one of the tree's visible lines, not an extra —
    // folding replaces N lines with 1, so exactly `N - 1` are gone from view,
    // which is `baseline.line_count() - tree.line_count()`.
    let removed = baseline.line_count() - tree.line_count();
    let marker = line_text(tree.lines(0..1).next().unwrap());
    let hidden: usize = marker
        .rsplit('(')
        .next()
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("no parsable hidden count in {marker:?}"));
    assert_eq!(
        hidden, removed,
        "the marker's hidden count must equal exactly how many lines disappeared: {marker:?}"
    );
}

#[test]
fn test_folding_the_last_heading_in_the_document_collapses_to_its_end() {
    let doc = Document::parse("# Only\n\nBody one.\n\nBody two.\n");
    let width = 40;
    let baseline = folded(&doc, width, &FoldState::default());
    let only_id = baseline.outline().entries[0].block;
    let tree = folded(&doc, width, &one_folded(only_id));

    assert_eq!(
        tree.line_count(),
        1,
        "folding the only (and therefore last) heading must leave exactly one line: {:?}",
        tree_text(&tree)
    );
}

#[test]
fn test_folding_an_h2_inside_an_open_h1_hides_only_the_h2s_own_section() {
    let doc = Document::parse("# One\n\nAlpha.\n\n## Sub\n\nSubtext.\n\n# Two\n\nBeta.\n");
    let width = 40;
    let baseline = folded(&doc, width, &FoldState::default());
    let sub_id = baseline.outline().entries[1].block;
    assert_eq!(baseline.outline().entries[1].text, "Sub");

    let tree = folded(&doc, width, &one_folded(sub_id));
    let all = tree_text(&tree);
    assert!(
        all.contains("One") && all.contains("Alpha"),
        "the enclosing H1 must render normally: {all:?}"
    );
    assert!(
        all.contains("Sub") && all.contains("hidden"),
        "the H2 must show its own marker: {all:?}"
    );
    assert!(
        !all.contains("Subtext"),
        "the H2's own body must be hidden: {all:?}"
    );
    assert!(
        all.contains("Two") && all.contains("Beta"),
        "the following H1 sibling must be unaffected: {all:?}"
    );
}

#[test]
fn test_folding_an_h1_whose_section_contains_an_already_folded_h2_hides_both_under_one_marker() {
    let doc = Document::parse("# One\n\nAlpha.\n\n## Sub\n\nSubtext.\n\n# Two\n\nBeta.\n");
    let width = 40;
    let baseline = folded(&doc, width, &FoldState::default());
    let one_id = baseline.outline().entries[0].block;
    let sub_id = baseline.outline().entries[1].block;

    let mut folds = FoldState::default();
    folds.toggle(sub_id);
    folds.toggle(one_id);
    let tree = folded(&doc, width, &folds);
    let all = tree_text(&tree);

    assert!(
        all.contains("One") && all.contains("hidden"),
        "the outer heading must show its own marker: {all:?}"
    );
    assert!(
        !all.contains("Alpha") && !all.contains("Sub") && !all.contains("Subtext"),
        "nothing inside the outer fold may reach the tree, including the nested \
         heading's own title: {all:?}"
    );
    assert!(
        all.contains("Two") && all.contains("Beta"),
        "the sibling H1 outside the fold must be unaffected: {all:?}"
    );
}

#[test]
fn test_dw_5_4_collapsing_every_heading_leaves_one_line_per_top_level_heading_and_expand_all_restores_it()
 {
    let doc = Document::parse("# A\n\nx\n\n## B\n\ny\n\n# C\n\nz\n");
    let width = 40;
    let baseline = folded(&doc, width, &FoldState::default());

    let mut folds = FoldState::default();
    for entry in &baseline.outline().entries {
        folds.toggle(entry.block);
    }
    let collapsed = folded(&doc, width, &folds);
    let marker_lines = collapsed
        .lines(0..collapsed.line_count())
        .filter(|l| line_text(l).contains('\u{25b8}'))
        .count();
    assert_eq!(
        marker_lines,
        2,
        "A and C are top-level; B is inside A's folded range and must never surface its \
         own marker: {:?}",
        tree_text(&collapsed)
    );
    let all = tree_text(&collapsed);
    assert!(
        !all.contains('x') && !all.contains('y') && !all.contains('z'),
        "every body line must be hidden once every heading is collapsed: {all:?}"
    );

    let expanded = folded(&doc, width, &FoldState::default());
    assert_eq!(
        expanded, baseline,
        "expand-all is `layout` with an empty `FoldState` — it must restore the full tree"
    );
}

/// Every run's text, re-measured through the width engine — never trusting
/// `Run.width` — must not exceed the tree's own layout width, on both the
/// folded marker line and every ordinary line around it (DW-5.6).
#[test]
fn test_dw_5_6_no_folded_or_unfolded_line_exceeds_the_layout_width() {
    let long_title = "A very long heading title that is deliberately far too wide to fit";
    let doc = Document::parse(&format!(
        "# {long_title}\n\nBody text that also runs long enough to wrap at a narrow width.\n\n\
         # Two\n\nBeta.\n"
    ));
    let eng = engine();
    for width in [20u16, 24, 40, 80] {
        let baseline = layout(
            &doc,
            width,
            &layout::LayoutConfig::default(),
            &eng,
            &NullSizer,
        );
        let one_id = baseline.outline().entries[0].block;
        for folds in [FoldState::default(), one_folded(one_id)] {
            let tree = layout_with_folds(
                &doc,
                width,
                &layout::LayoutConfig::default(),
                &eng,
                &NullSizer,
                &folds,
            );
            for line in tree.lines(0..tree.line_count()) {
                let text = line_text(line);
                let measured = eng.display_width(&text);
                assert!(
                    measured <= tree.width() as usize,
                    "line {text:?} measures {measured} cells, wider than the {}-cell \
                     layout width",
                    tree.width()
                );
            }
        }
    }
}

/// DW-5.1 regression: the hidden-line count must survive being narrowed, not
/// be the first thing a right-to-left clip discards. Reproduces the review's
/// own trace — `"Installing and configuring the development toolchain"` at
/// widths 20/40/60/80 — where the count previously survived only 2 of 4, and
/// at width 60 truncated to the actively misleading `"(6 h…"`.
///
/// A custom, wide-open `LayoutConfig` (rather than `LayoutConfig::default`,
/// whose 24-cell floor would silently clamp the width-20 case up to 24) is
/// what lets each width below actually apply.
#[test]
fn test_dw_5_1_the_hidden_count_survives_widths_that_previously_clipped_it() {
    let title = "Installing and configuring the development toolchain";
    let doc = Document::parse(&format!(
        "# {title}\n\nOne paragraph.\n\nAnother paragraph.\n\nA third one.\n\n\
         Yet a fourth.\n\nAnd a fifth.\n\nFinally a sixth.\n"
    ));
    let config = layout::LayoutConfig {
        min_width: 1,
        max_width: 200,
    };
    let eng = engine();
    for width in [20u16, 24, 40, 60, 80] {
        let baseline = layout(&doc, width, &config, &eng, &NullSizer);
        let one_id = baseline.outline().entries[0].block;
        let tree = layout_with_folds(&doc, width, &config, &eng, &NullSizer, &one_folded(one_id));
        let marker = line_text(tree.lines(0..1).next().unwrap());

        assert!(
            marker.contains("hidden line)") || marker.contains("hidden lines)"),
            "at width {width}: the count must be shown whole, not truncated mid-unit \
             (the review found `\"(6 h…\"` at width 60): {marker:?}"
        );
        let measured = eng.display_width(&marker);
        assert!(
            measured <= tree.width() as usize,
            "at width {width}: marker {marker:?} measures {measured} cells, wider than \
             the {}-cell layout width",
            tree.width()
        );
    }
}

/// The same regression, but for a title short enough to fit *and* the count
/// — the "nothing to trade off" case, which must still show both in full.
#[test]
fn test_dw_5_1_a_short_title_shows_the_full_count_unclipped() {
    let doc = Document::parse("# Getting started\n\nBody one.\n\nBody two.\n");
    let width = 40;
    let baseline = layout(
        &doc,
        width,
        &layout::LayoutConfig::default(),
        &engine(),
        &NullSizer,
    );
    let one_id = baseline.outline().entries[0].block;
    let tree = folded(&doc, width, &one_folded(one_id));
    let marker = line_text(tree.lines(0..1).next().unwrap());
    assert!(
        marker.contains("Getting started") && marker.contains("hidden line"),
        "a title that already fits must not lose the count either: {marker:?}"
    );
}
