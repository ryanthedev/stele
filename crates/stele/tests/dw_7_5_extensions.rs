//! DW-7.5: alerts, task lists, and frontmatter render per fixtures.
//!
//! Each fixture is parsed through the real `ast` -> `layout` pipeline
//! (P2-P4, already committed) and checked against `ThemedDecor`
//! (`crates/stele/src/decor/themed.rs`) — the actual P7 `Decor` a real
//! paint would use — rather than re-deriving expectations from the theme
//! module directly, so these tests exercise the full seam this phase
//! produces, not just `crates/highlight` in isolation.

use ast::{Document, NodeId};
use layout::{AlertTone, CellSize, IntrinsicSizer, Line, LineItem, Run, Semantic, StyleId};
use stele::decor::Decor;
use stele::decor::frontmatter;
use stele::decor::themed::ThemedDecor;
use stele::painter::Style;
use width::{WidthConfig, WidthEngine};

const ALERTS: &str = include_str!("../../../fixtures/alerts.md");
const TASKLISTS: &str = include_str!("../../../fixtures/tasklists.md");
const FRONTMATTER: &str = include_str!("../../../fixtures/frontmatter.md");

struct NullSizer;
impl IntrinsicSizer for NullSizer {
    fn size(&self, _node: NodeId, _doc: &Document) -> Option<CellSize> {
        None
    }
}

fn all_runs(source: &str) -> Vec<Run> {
    let doc = Document::parse(source);
    let engine = WidthEngine::new(WidthConfig::default());
    let config = layout::LayoutConfig::default();
    let tree = layout::layout(&doc, 80, &config, &engine, &NullSizer);
    tree.lines(0..tree.line_count())
        .flat_map(|line| match line {
            Line::Items(items) => items
                .iter()
                .filter_map(|item| match item {
                    LineItem::Run(run) => Some(run.clone()),
                    LineItem::Box(_) => None,
                })
                .collect(),
            Line::Reserved(_) => Vec::new(),
        })
        .collect()
}

fn decor() -> ThemedDecor {
    ThemedDecor::detect(None)
}

#[test]
fn test_dw_7_5_alert_titles_render_with_distinct_tone_colors() {
    let runs = all_runs(ALERTS);
    let decor = decor();
    let tones = [
        AlertTone::Note,
        AlertTone::Tip,
        AlertTone::Important,
        AlertTone::Warning,
        AlertTone::Caution,
    ];
    let mut colors = Vec::new();
    for tone in tones {
        let present = runs
            .iter()
            .any(|r| r.style_id == StyleId::Semantic(Semantic::AlertTitle(tone)));
        assert!(
            present,
            "fixture must contain an AlertTitle run for {tone:?}"
        );
        let style = decor.resolve(StyleId::Semantic(Semantic::AlertTitle(tone)));
        assert!(style.bold, "{tone:?} alert title must be bold");
        let fg = style.fg.expect("alert title must carry a themed color");
        colors.push(fg);
    }
    for i in 0..colors.len() {
        for j in (i + 1)..colors.len() {
            assert_ne!(
                colors[i], colors[j],
                "alert tones {:?} and {:?} collided on the same color: {colors:?}",
                tones[i], tones[j]
            );
        }
    }
}

#[test]
fn test_dw_7_5_bogus_alert_kind_stays_a_plain_blockquote_not_an_alert_title() {
    // Fixture comment: "Not an alert, just a quote." — `[!BOGUS]` is not a
    // recognized GitHub alert kind, so `ast` must not have parsed it as
    // one; no `AlertTitle` run should mention it.
    let runs = all_runs(ALERTS);
    let has_bogus_text = runs.iter().any(|r| r.text.contains("BOGUS"));
    assert!(
        has_bogus_text,
        "the literal '[!BOGUS]' text must still appear"
    );
    let alert_title_run_texts: Vec<&str> = runs
        .iter()
        .filter(|r| matches!(r.style_id, StyleId::Semantic(Semantic::AlertTitle(_))))
        .map(|r| r.text.as_str())
        .collect();
    assert!(
        !alert_title_run_texts.iter().any(|t| t.contains("BOGUS")),
        "BOGUS must not be styled as an alert title: {alert_title_run_texts:?}"
    );
}

#[test]
fn test_dw_7_5_task_markers_render_checked_and_unchecked_with_a_themed_style() {
    let runs = all_runs(TASKLISTS);
    let markers: Vec<&Run> = runs
        .iter()
        .filter(|r| r.style_id == StyleId::Semantic(Semantic::TaskMarker))
        .collect();
    // unchecked, checked, capital-checked (normalized), nested unchecked.
    assert_eq!(
        markers.len(),
        4,
        "expected 4 task markers in the fixture, got {markers:?}"
    );
    let texts: Vec<&str> = markers.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(
        texts.iter().filter(|t| **t == "[x] ").count(),
        2,
        "checked + capital-checked normalize to '[x] '"
    );
    assert_eq!(
        texts.iter().filter(|t| **t == "[ ] ").count(),
        2,
        "unchecked + nested unchecked"
    );

    let decor = decor();
    let style = decor.resolve(StyleId::Semantic(Semantic::TaskMarker));
    assert_ne!(
        style,
        Style::default(),
        "task markers must carry some themed styling, not fall through as plain text"
    );

    // The plain-item bullet (no checkbox) must NOT be styled as a task
    // marker — it's a `ListMarker`.
    let bullets: Vec<&Run> = runs
        .iter()
        .filter(|r| r.style_id == StyleId::Semantic(Semantic::ListMarker))
        .collect();
    assert!(
        !bullets.is_empty(),
        "the plain '- plain item' bullet must be a ListMarker"
    );
}

#[test]
fn test_dw_7_5_frontmatter_hidden_by_default_never_lays_out_at_all() {
    let hidden_source = frontmatter::apply(FRONTMATTER, false);
    let runs = all_runs(&hidden_source);
    assert!(
        !runs
            .iter()
            .any(|r| r.style_id == StyleId::Semantic(Semantic::FrontMatter)),
        "no FrontMatter run should exist once the block is stripped before parsing"
    );
    assert!(!runs.iter().any(|r| r.text.contains("stele fixture")));
    assert!(
        runs.iter().any(|r| r.text.contains("After frontmatter")),
        "body content must survive"
    );
}

#[test]
fn test_dw_7_5_frontmatter_flag_shows_it_styled_per_theme() {
    let shown_source = frontmatter::apply(FRONTMATTER, true);
    let runs = all_runs(&shown_source);
    let frontmatter_runs: Vec<&Run> = runs
        .iter()
        .filter(|r| r.style_id == StyleId::Semantic(Semantic::FrontMatter))
        .collect();
    assert!(
        !frontmatter_runs.is_empty(),
        "shown frontmatter must lay out as FrontMatter runs"
    );
    assert!(
        frontmatter_runs
            .iter()
            .any(|r| r.text.contains("stele fixture"))
    );

    let decor = decor();
    let style = decor.resolve(StyleId::Semantic(Semantic::FrontMatter));
    assert!(
        style.dim,
        "shown frontmatter should still read as de-emphasized, per the structural convention"
    );
}
