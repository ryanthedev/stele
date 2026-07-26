# Discovery + Design: Phase 5 - Section folding

## Files Found
- `crates/layout/src/block.rs` — the block walker (`Ctx`, `walk_blocks`); headings enter at the `BlockKind::Heading` arm of `walk_block`; `line_blocks` (per-line top-level block tag) declared ~line 40.
- `crates/layout/src/lib.rs` — `layout()` entry point, `Outline`/`OutlineEntry` (Phase 3), `LayoutTree`.
- `crates/stele/src/app.rs` — `AppState`, `Mode`, `ChromeAction`, `relayout`/`relayout_preserving_anchor` (already documented in Phase 1 as "the entry point every later phase calls after a width, theme, fold, or reload change"), the content-addressed reload anchor (`fingerprint`/`occurrence_of`/`line_of_reloaded`, Phase 2), `SearchState`/key routing (Phase 4).
- `crates/stele/src/main.rs` — `handle_chrome_key`, the two call sites (main loop, resize drain).
- `crates/stele/src/media/sink.rs` — module doc confirms "a box that is not painted in a frame is not on the screen after that frame": placement is recomputed fresh every frame from what the tree paints, so DW-5.3 falls out of folding removing `Line::Reserved` rows for free, with no sink-side change needed.
- `crates/stele/tests/toc_key_routing.rs`, `search_key_routing.rs` — the "drive the real binary" pty pattern the routing seam note asks for.

## Current State
Phase 3 built `Outline { entries: Vec<OutlineEntry{level, text, block}> }`, rebuilt every layout from the block walk. Phase 1 already anticipated fold as a `relayout_preserving_anchor` caller. No fold machinery exists yet; `Mode` has `Normal`/`Toc`/`Search`, no fold-specific variant.

## Gaps
- The plan's file scope lists only `block.rs`/`app.rs`/`painter.rs`, but `FoldState` and the walk-level fold consultation cannot exist without touching `layout/src/lib.rs` (parallel to `Outline` living there). Treated as necessary wiring, not scope creep — the file list undercounts by one file, same class of gap Phase 3 hit with `layout/src/**`.
- `painter.rs` needed no change at all: the fold marker reuses `Semantic::Heading(level)`, so no new exhaustive-match arm, no new decor mapping. Listed in scope but nothing to do there.

## Code Standards
Applied: exhaustive matches (no new `Semantic`/`Mode` variant added, so nothing to make exhaustive), hand-rolled errors (none introduced — folding has no fallible path), sentence-style DW-tagged test names, never assert on `Run.width` (fold width test re-measures via `WidthEngine::display_width` over concatenated run text), saturating width math (`inline::cells`, reused as-is).

## Test Infrastructure
`crates/layout/tests/*.rs` (pure layout, `engine()`/`lay()` fixtures), `crates/stele/src/app.rs`'s own `#[cfg(test)] mod tests` (AppState-level), `crates/stele/tests/common/pty.rs` (`spawn_viewer`/`type_bytes`/`read_one_frame`) for real-binary routing tests, `crates/stele/tests/reserved_column.rs`-style `Painter` + `GfxMediaSink` byte-level placement tests.

## DW Verification

| DW-ID | Done-When Item | Status | Test Cases |
|-------|---------------|--------|------------|
| DW-5.1 | Toggling a fold collapses to one marked line with hidden-line count, restores exactly on re-toggle | COVERED | `layout/tests/fold.rs::test_dw_5_1_toggling_a_fold_collapses_to_one_marked_line_and_restores_exactly`, `test_dw_5_1_the_reported_hidden_count_matches_the_lines_actually_removed`; `app.rs::test_dw_5_1_toggle_fold_collapses_and_restores_exactly` |
| DW-5.2 | Fold state survives a width change and a `--watch` reload, keyed by node not line | COVERED | `app.rs::test_dw_5_2_fold_survives_a_width_change`, `test_dw_5_2_fold_survives_a_watch_reload_by_content_identity` |
| DW-5.3 | Folding a range containing a placed image removes that placement; unfolding restores it | COVERED | `tests/fold_placement.rs::test_dw_5_3_folding_a_section_with_an_image_removes_its_placement_and_unfolding_restores_it` |
| DW-5.4 | Collapse-all leaves exactly one line per top-level heading; expand-all restores the full document | COVERED | `layout/tests/fold.rs::test_dw_5_4_...`; `app.rs::test_dw_5_4_collapse_all_and_expand_all` |
| DW-5.5 | Folding while scrolled inside the folded range leaves the viewport at the fold marker, never past the end | COVERED | `app.rs::test_dw_5_5_folding_while_scrolled_inside_the_range_snaps_to_the_marker` |
| DW-5.6 | No folded or unfolded line exceeds the layout width, re-measured through the width engine | COVERED | `layout/tests/fold.rs::test_dw_5_6_...`; `app.rs::test_dw_5_6_no_line_exceeds_width_after_a_fold_driven_relayout` |

**All items COVERED:** YES

## Design Decisions

**`FoldState` lives in `crates/layout`, not `crates/stele`.** It must be consulted *during* the walk (the plan's own wording), and the walk lives in `layout`. `AppState` owns an instance and is the only mutator; the crate boundary stays one-way (`layout` never depends on `stele`).

**Additive `layout_with_folds`, not a signature change to `layout`.** ~60 existing call sites across `layout`'s and `stele`'s test suites call the 5-arg `layout()`. Changing its signature would force touching every one of them, well past this phase's scope. `layout()` is now a thin wrapper calling `layout_with_folds(..., &FoldState::default())`; every existing caller is untouched. `AppState::relayout` is the one call site that switches to `layout_with_folds`.

**Section boundary: block-index scan on `blocks: &[Block]`, not the `Outline`.** The layout walk needs the boundary in *block* index terms to skip a slice during the walk itself; `AppState`'s viewport-snap decision (DW-5.5) needs it in *line* index terms against the already-built tree. Both implement "next heading of equal or shallower level," independently, because they run over different data at different times (mid-walk, block list vs. post-layout, `Outline`). Documented at both sites rather than sharing a helper across the crate boundary that doesn't otherwise exist.

**Folding is top-level-only, matching how `Outline`/`line_blocks` already work.** A heading nested inside a blockquote or list item is not itself addressable as a top-level fold range (its `OutlineEntry::block` already resolves to the *enclosing* top-level block for the same reason). The plan's own edge case ("nested headings — folding H2 inside a folded H1") refers to heading-*level* nesting between top-level siblings (H1 then H2 as consecutive top-level blocks), not AST containment, and that case is directly covered (`test_folding_an_h1_whose_section_contains_an_already_folded_h2_hides_both_under_one_marker`). A heading genuinely inside a blockquote is out of this phase's practical reach; noted, not silently special-cased.

**Hidden-line count via a recursive dry-run walk, not a separate calculator.** `Ctx::count_lines` re-walks the folded slice on a scratch `Ctx` sharing the real `FoldState` minus the one heading being measured (removing just that id, not clearing the whole set, is what makes a nested fold's own marker count as 1 line in the parent's hidden count instead of infinitely re-triggering itself — first attempt without the removal stack-overflowed immediately, caught by the test suite, not review). This reuses the exact same layout logic being measured, so the reported count can never drift from what folding actually produces.

**Fold marker reuses `Semantic::Heading(level)`; no new `Semantic` variant.** The marker's text (`▸ Title (N hidden lines)`) carries the information DW-5.1 asks for; the heading's own style already renders it distinctly from body text. Avoiding a new variant means zero changes to the exhaustive style tables in `highlight::theme` and `stele::decor` — those crates are out of this phase's file scope, and touching them for a purely cosmetic distinction the DW items do not require would be scope creep in the other direction.

**No new `Mode` variant.** Folding has no overlay — it is a document-affecting toggle, architecturally identical to `+`/`-`/`T` (Phase 1). Extended `ChromeAction` instead: `ToggleFold`, `ExpandAllFolds`, `CollapseAllFolds`, gated by the existing `Mode::captures_all_keys()` for free (Toc/Search already swallow every key before `chrome_action` sees it).

**Key bindings: `z` / `R` / `M`, not the plan text's `zR`/`zM` chord flavor.** The full plan file's Scope bullet says "`zR`/`zM`-style expand-all and collapse-all"; the dispatch prompt's own Scope text (the version actually gating this phase's DW items) says only "expand-all and collapse-all," naming no keys. A two-key `z`-prefix chord would need chrome_action (a pure `&self` decision) to read a `pending` flag armed by a *previous*, unrelated call to the mutable normal-key handler — and since a completed chrome action never reaches `handle_key_event` (main.rs short-circuits), nothing would reliably clear that flag, leaving a stale `z` able to reinterpret a later, unrelated `R` or `M` as a fold command. That is exactly the key-routing defect class the phase's own seam note warns about, self-inflicted. Single unmodified keys avoid it entirely; no DW item names a specific key, so this is documented latitude, not a silent contract change.

**`toggle_fold()`/`expand_all()`/`collapse_all()` are zero-arg, matching the plan's literal `Produces` signatures** (contrast Phase 1's `relayout_preserving_anchor(&LayoutContext, LayoutConfig)`, spelled out with types when the plan means to pin them — Phase 5's three are written bare). They only mutate `AppState::folds` (plus, for `toggle_fold`, a private `pending_fold_snap: Option<NodeId>` recording whether the current viewport is about to be folded away); the caller (`main.rs::handle_chrome_key`, mirroring `Widen`/`Narrow`) performs the actual `relayout_preserving_anchor` afterward, consistent with Phase 1's own doc comment naming that method the entry point for a fold change.

**DW-5.5's exact snap is inside `AppState::relayout`, not a special path.** `relayout` already computes a scroll target via the anchor mechanism; when `pending_fold_snap` is set (only by `toggle_fold`, and only when the *current* scroll line falls inside the section about to collapse — computed against the pre-fold `Outline`'s line range, before `self.tree` is replaced), the computed target is overridden with the marker's post-fold line via `first_line_of`, then clamped by the existing `set_scroll`. Every other caller of `relayout` (`widen`, `narrow`, theme toggle, reload, resize) leaves the field `None` and is unaffected.

**DW-5.2's reload survival is content-addressed, mirroring Phase 2's scroll anchor — but keyed on the `Outline`'s flattened heading text, not on painted-line hashing.** Reusing the scroll anchor's line-fingerprint approach directly would hash *whatever currently renders* at the heading's line — which is the marker text (title + a hidden count) when folded, and the full heading+body when not — so the same heading fingerprints differently depending on its own fold state and could never be matched consistently across a reload. `OutlineEntry::text` (`block::heading_text`) is fold-invariant by construction: `emit_fold_marker` and `Ctx::heading` both compute it from the same AST children, regardless of which one runs. Folds are re-keyed by `(level, text, occurrence-among-same-level-and-text)` against the *old* outline (captured before reload) and re-resolved against the *new* one after — the same "content plus occurrence, never position" principle Phase 2 established, applied one level up (outline entries instead of painted block spans). Implemented as two relayout passes on `reload_document` only when folds are non-empty: pass 1 lays out the new document fold-free (so stale old-document `NodeId`s in `self.folds` cannot spuriously collide with an unrelated new node), `reseat_folds` re-keys against the resulting `Outline`, pass 2 relays out with the corrected fold set. A heading whose content was deleted or edited beyond recognition is silently dropped from `folds` — the same graceful-degradation the scroll anchor already accepts for the same reason.

## Prerequisites
- [x] Required files exist
- [x] Dependencies available (Phase 1-4 all landed: `Outline`, `relayout_preserving_anchor`, `Mode`/`chrome_action`, media residency-vs-placement split)
- [x] No missing prerequisites

## Recommendation
BUILD.
