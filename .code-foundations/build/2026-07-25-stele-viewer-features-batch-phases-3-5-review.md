# Batch Review: Phase 3 (rebase delta) + Phase 5 (section folding)

Reviewed at `1c4192f` (HEAD), clean tree. Skills loaded: `cc-control-flow-quality`,
`aposd-verifying-correctness`.

## Executed Results (Step 0) — shared by both phases

Run once from inside a controlling terminal (`script -q /dev/null`), cited by both blocks.

| Command | Result |
|---|---|
| `cargo test --workspace` | **555 passed, 0 failed, 5 ignored** (exit 0) |
| `cargo clippy --workspace --all-targets` | clean, no warnings |
| `cargo fmt --all -- --check` | clean |

The 5 ignored are all environment-gated (`requires a real Ghostty install and an
interactive GUI session`, `dump_failures`), not skipped assertions.

Every FAIL below rests on a reproduction I wrote and executed against this tree. All
probe files were deleted afterward; `git status` is clean and the 555/0 baseline was
re-confirmed after removal.

---

# Review: Phase 5 — Section folding (`1c4192f`)

## Requirement Fulfillment

### DW-5.1
PREMISE:  "Toggling a fold collapses its section to one marked line showing the hidden-line count, and restores it exactly on re-toggle."
EVIDENCE: `crates/layout/src/block.rs:186-215` (`emit_fold_marker`), `:526-535` (`fold_marker_text`)
TRACE:    `# Installing and configuring the development toolchain` + 3 body paragraphs, layout width 40 → `emit_fold_marker` builds `"▸ Installing and configuring the development toolchain (6 hidden lines)"` → `inline::clip_runs(runs, content_width=40, …)` truncates from the right → the painted line is `"▸ Installing and configuring the develo…"`. One marked line: yes. Showing the hidden-line count: **no** — the count is the first thing the clip discards.
VERDICT:  **FAIL** (collapse-to-one-line and exact restore both hold; the *count* half does not)

The count is lost at ordinary widths, not just pathological ones. `LayoutConfig::default()`
is `min_width: 24, max_width: 100` (`crates/layout/src/lib.rs:48-55`), and `-` steps
`content_width` down 4 cells at a time (`app.rs:368`, `:591-599`), so widths 40 and 60 are
squarely inside normal operation. Measured across six realistic heading/width pairs, the
count survived **2 of 6**:

| Width | Painted marker | Count shown |
|---|---|---|
| 20 | `▸ Installing and co…` | no |
| 40 | `▸ Installing and configuring the develo…` | no |
| 60 | `▸ Installing and configuring the development toolchain (6 h…` | no — truncated to `(6 h…` |
| 80 | `▸ Installing and configuring the development toolchain (6 hidden lines)` | yes |
| 40 | `▸ Frequently asked questions about depl…` | no |
| 40 | `▸ Getting started (6 hidden lines)` | yes |

Width 60 is the worst of the three outcomes: `(6 h…` is not an absent count, it is a
count that *looks* rendered while its unit has been eaten.

Root cause: `fold_marker_text` puts the variable-length title first and the fixed-length
count last, then the whole string is clipped right-to-left. The one datum the marker
exists to add is the one guaranteed to be sacrificed first.

The existing tests cannot see this. `layout/tests/fold.rs:57` uses the title `"One"` at
width 40; `fold.rs:228` (DW-5.6) does use a long title at width 20 but asserts only the
width bound, never that the count survives; `app.rs:4458` uses `"Heading 00"`. Every
fixture is short enough that the clip never reaches the suffix.

### DW-5.2
PREMISE:  "Fold state survives a width change and a `--watch` reload, keyed by node rather than line."
EVIDENCE: `crates/stele/src/app.rs:684-709` (`reseat_folds`), `:630-661` (`reload_document`)
TRACE:    See the two traces below.
VERDICT:  **FAIL** (width change: PASS. `--watch` reload: two demonstrated failures.)

The width-change half is correct and covered — `app.rs:4497` re-checks the fold after
`widen()` and I confirmed it passes.

The reload half fails because `reseat_folds` re-keys against **two outlines with different
entry sets**. `reload_document` captures `old_outline` from the *fold-abbreviated* tree
(headings inside a folded range emit no outline entry — `walk_blocks` skips the whole
range at `block.rs:369-373`), then deliberately clears folds so the new tree's outline is
*fully expanded* (`app.rs:645-649`). `reseat_folds` then computes an occurrence index in
the abbreviated outline and applies it to the expanded one. Those indexes are not
comparable.

**Trace 5.2-a — a nested fold is silently dropped.**
Source `# A / x body / ## B / y body / # C / z body`; press `M` (collapse-all) →
`collapsed = {A, B, C}`. Baseline with no reload, unfold A:

```
A / x body / ▸ B (2 hidden lines) / ▸ C (2 hidden lines)
```

Now `--watch` reload of the **byte-identical source**, then unfold A:

```
A / x body / B / y body / ▸ C (2 hidden lines)
```

`collapsed` went 3 → 2. B's id is not in `old_outline` (it was inside A's folded range),
so `reseat_folds`' `position(|e| e.block == id)?` returns `None` and the `filter_map`
drops it. B's fold did not survive.

**Trace 5.2-b — the fold re-seats onto the *wrong* heading.** Worse than dropped.
Source: `# A / aaa / ## Notes / first notes / # B / bbb / ## Notes / second notes`
(two H2s both titled `Notes`). Fold `A`, then fold the one visible `Notes` — the
**second** one, under B. Before the reload:

```
▸ A (6 hidden lines) / B / bbb / ▸ Notes (2 hidden lines)
```

After a `--watch` reload of the byte-identical source:

```
▸ A (4 hidden lines) / B / bbb / Notes / second notes
```

The fold jumped from the second `Notes` to the **first** one (the one inside A — visible
in A's hidden count dropping 6 → 4), and the heading the reader actually folded is now
wide open. With A folded, the abbreviated `old_outline` is `["A", "B", "Notes"]`, so the
folded `Notes` computes `occurrence = 0`; applied to the fully-expanded new outline,
`.nth(0)` selects the first `Notes`.

### DW-5.3
PREMISE:  "Folding a range containing a placed image removes that placement; unfolding restores it."
EVIDENCE: `crates/stele/tests/fold_placement.rs:47`, `crates/stele/src/app.rs:4581`
TRACE:    `# One / ![p](p.png) / # Two` → fold `One` → `layout_with_folds` skips the range so no `Line::Reserved` is emitted → `Painter::frame` never calls `sink.paint` for that node → the frame's bytes contain no `\x1b_Ga=p`. Unfold → placement returns, byte-for-byte identical to never having folded.
VERDICT:  **PASS**

Verified beyond the committed tests. `fold_placement.rs` builds a **fresh** `Painter` and
`GfxMediaSink` per frame, so it never exercises a persistent sink across a fold. I drove
one sink through `open @0 → folded @0 → folded @30 → folded @0 → unfolded @0 → @30 → @0`
against the `TerminalGfx` wire model: `visible` was empty at every folded frame and held
exactly one image at every open frame, and the image kept its id (`4111466497`) across
the fold — a re-place, not a re-decode. Placement and residency both stay correct.

### DW-5.4
PREMISE:  "Collapse-all leaves exactly one line per top-level heading; expand-all restores the full document."
EVIDENCE: `crates/stele/src/app.rs:912-920` (`collapse_all`), `:900-902` (`expand_all`); tests `app.rs:4637`, `layout/tests/fold.rs:189`
TRACE:    `# A / x / ## B / y / # C / z` → `collapse_all` collects every outline entry's block → relayout → `walk_blocks` collapses A (swallowing B) and C → 2 marker lines, `x`/`y`/`z` all absent. `expand_all` clears the set → tree compares equal to the pre-collapse baseline.
VERDICT:  **PASS**

Also probed the cross-phase case (collapse-all when every heading is already inside one
folded H1) — markers correct, and `expand_all` restored both body paragraphs.

### DW-5.5
PREMISE:  "Folding while scrolled inside the folded range leaves the viewport at the fold marker, never past the end."
EVIDENCE: `crates/stele/src/app.rs:885-896` (`toggle_fold` arms `pending_fold_snap`), `:1299-1310` (`relayout` consumes it)
TRACE:    Reader at a line inside `Heading 01`'s body → `section_line_range(1).contains(scroll)` is true and `folding` is true → `pending_fold_snap = Some(target)` → relayout → `first_line_of(target)` is the marker line (`walk_blocks` sets `current_block` before `emit_fold_marker`, so the marker line is tagged with the heading's block) → `set_scroll` clamps to `max_scroll`.
VERDICT:  **PASS**

Covered from both entry positions (`app.rs:4673` inside the body, `:4710` on the heading's
own line), and `set_scroll`'s clamp gives the "never past the end" half structurally.

### DW-5.6
PREMISE:  "No folded or unfolded line exceeds the layout width, re-measured through the width engine."
EVIDENCE: `crates/layout/tests/fold.rs:228`, `crates/stele/src/app.rs:4730`
TRACE:    Marker text → `inline::clip_runs(…, self.content_width(), true, self.engine)` → every emitted line's concatenated run text, re-measured with `engine.display_width()`, is `<= tree.width()`.
VERDICT:  **PASS**

Both tests re-measure through the width engine and never assert on `Run.width`, per the
project convention. I extended this myself across a fold-then-narrow sweep
(100/80/60 → 80, 60, 40, 24, 20, 10, 4, 1) and found no overflow at any width.

**All requirements met:** **NO** — DW-5.1 and DW-5.2 fail.

## Edge Cases (prompt-listed — unhandled is a FAIL)

| Edge case | Status | Evidence |
|---|---|---|
| Folding the last heading in a document | PASS | `fold.rs:116`; `fold_range`'s `map_or(blocks.len(), …)` handles the no-following-heading case |
| Nested headings (folding an H2 inside a folded H1) | PASS | `fold.rs:132`, `fold.rs:160`; `count_lines` recursion terminates (each level takes a strictly shorter slice, and nesting is bounded by H1..H6) |
| Folding while scrolled inside the range being folded | PASS | DW-5.5 above |
| Folding a range containing a placed image | PASS | DW-5.3 above, plus my persistent-sink probe |
| Search match inside a folded range — `n` expands or skips, **and the choice is stated in the status row** | **FAIL** | below |

The implementation chose **skip**, which is a legitimate choice — but the choice is stated
only in a Rust doc comment (`app.rs:1367-1373`) and a test message (`app.rs:4769`), never
in the status row. The only status strings `AppState` can produce are `NO_HEADINGS` and
`"no matches: {query}"` (`app.rs:283`, `:1168`); nothing mentions folding.

Executed: `/needle<Enter>` on `# One / needle here. / # Two / plain text.` → 1 match →
fold `One` → matches drops to 0 → status row is `None`. Press `n`:

```
status row right after folding over the match: None
status row after pressing n:                   Some("no matches: needle")
```

The reader is told the document does not contain `needle`. It does — the match is inside a
fold they can reopen with `z`. The requirement asked for the choice to be surfaced
precisely so this reading could not happen, and the current behavior is not merely silent
but affirmatively wrong.

## Test-DW Coverage (level: 100%)

| DW | Automated test that ran in Step 0 | Adequate |
|---|---|---|
| 5.1 | `layout/tests/fold.rs:57`, `:91`; `app.rs:4458` | **Gap** — every fixture title is short enough that the clip never reaches the count suffix |
| 5.2 | `app.rs:4497` (width), `app.rs:4528` (reload) | **Gap** — the reload test folds one top-level heading with a unique title, the only shape that survives `reseat_folds` |
| 5.3 | `tests/fold_placement.rs:47`, `app.rs:4581` | Yes (fresh sink per frame; I covered the persistent-sink case) |
| 5.4 | `app.rs:4637`, `fold.rs:189` | Yes |
| 5.5 | `app.rs:4673`, `:4710` | Yes |
| 5.6 | `fold.rs:228`, `app.rs:4730` | Yes |
| Search-in-fold edge case | `app.rs:4755` | **Gap** — asserts the skip, never asserts the status row the requirement demands |
| `z`/`R`/`M` routing | `tests/fold_key_routing.rs:73`, `:115` (real binary over a pty) | Yes |

Every DW item has execution evidence. Three coverage gaps let real defects through rather
than leaving an item unevidenced.

## Dead Code

None found. No unused imports, no unreachable code after early returns, no debug
statements, no commented-out blocks in the changed files. `emit_fold_marker`'s
`unreachable!` (`block.rs:194`) is a locally-provable invariant guarded one call away by
`fold_range`, not dead code.

## Correctness Dimensions

| Dimension | Status | Evidence |
|---|---|---|
| Concurrency | N/A | Single-threaded event loop; `FoldState`/`pending_fold_snap` are owned by `AppState` and touched only on the key path. No threads, async, or shared mutable state introduced. |
| Error Handling | N/A | The fold path has no fallible operation — `layout_with_folds` is pure and total, and every lookup uses `Option` combinators with a defined fallback. Reload I/O errors are Phase 2's and unchanged. |
| Resources | PASS | Verified against the raster cache: byte-budgeted LRU with a pinning rule, `insert` refunds replaced bytes, `remove` refunds and returns the id to free. Fold/unfold round trips leak nothing (probe: id stable across fold, `stored` never grows). |
| Boundaries | **FAIL** | The narrow-width / long-title boundary drops the marker's hidden count (DW-5.1). Empty outline, last heading, and width-1 all handled correctly. |
| Security | N/A | No new untrusted input. Marker text is built from heading text that already flows through the same clip-and-paint path as any heading run. |

## Loaded-Skill Criteria

| Skill | Criterion | Status | Evidence |
|---|---|---|---|
| cc-control-flow-quality | Max nesting depth ≤ 3 | PASS | Deepest is `walk_blocks` at 3 (`while` → `if depth == 1` → `if let Some(end)`), `block.rs:359-379` |
| cc-control-flow-quality | McCabe ≤ 10 per routine | PASS | `relayout` is the highest at ~7; `walk_blocks` ~5, `fold_range` ~4, `reseat_folds` ~4 (flat iterator chains, no nesting) |
| cc-control-flow-quality | Guard clauses for error/absent cases | PASS | `fold_range` (`block.rs:118-123`), `toggle_fold` (`app.rs:887-890`), `place_box` (`sink.rs:470`) all exit early at the top |
| cc-control-flow-quality | Loop selection / exit design | PASS | `walk_blocks`' index-based `while` + `continue` is the correct choice — it is what lets a folded range be skipped in one step; documented at `block.rs:353-358` |
| cc-control-flow-quality | Descriptive loop indexes | PASS | `index`, `node_id`, `entry` throughout; no bare `i`/`j`/`k` in the changed code |
| cc-control-flow-quality | Boolean expression clarity | PASS | `toggle_fold`'s compound condition is factored through the named `folding` (`app.rs:892-894`) |
| aposd-verifying-correctness | Requirements coverage | **FAIL** | DW-5.1 and DW-5.2 have code that does not implement what the requirement states — see traces above |
| aposd-verifying-correctness | Concurrency safety | N/A | No shared mutable state, threads, or async introduced |
| aposd-verifying-correctness | Error handling — no silent-failure path | **FAIL** | `recompute_matches` (`app.rs:1374`) silently drops matches inside a folded range, and `report_no_matches` then asserts the query has no matches at all. A wrong answer delivered confidently is the silent-failure archetype this criterion names. |
| aposd-verifying-correctness | Resource management | PASS | See Resources above |
| aposd-verifying-correctness | Boundary conditions | **FAIL** | Narrow-width marker truncation, above |
| aposd-verifying-correctness | Security | N/A | No untrusted input newly handled |

## Notes (non-blocking) — Phase 5

| # | Finding | Confidence | Severity |
|---|---|---|---|
| 1 | `collapse_all` **replaces** rather than unions, and reads the fold-abbreviated outline. Pressing `M` twice drops nested folds: executed `M`, `M`, unfold A → `y body` visible where the first `M` alone leaves `▸ B (2 hidden lines)`. This directly contradicts `collapse_all`'s own doc comment (`app.rs:904-911`), which promises the inner heading stays folded when the outer is opened. Same root cause as DW-5.2. No DW item requires it, so it is not a blocker. | High (executed) | 🟡 Med |
| 2 | `z` pressed above the first heading of a document that *has* headings reports `"no headings in this document"`. `index_at_or_before` returns `None` for a preamble position (`layout/src/lib.rs:378-380`) and `toggle_fold` maps that to `NO_HEADINGS`. Executed and confirmed. The message is false; "no section to fold here" would be honest. | High (executed) | 🟡 Med |
| 3 | `M` and `R` on a headingless document are completely silent, while `z` reports `NO_HEADINGS`. Executed: both status rows are `None`. Inconsistent feedback for three keys in the same family. | High (executed) | ⚪ Low |
| 4 | Re-toggling a fold restores the *tree* exactly but not the reader. Executed: scroll 14 → fold → 12 → unfold → 12, not 14. DW-5.1's "restores it exactly" reads as being about the section, and the tree equality tests back that, so this is not a blocker — but a reader who folds to glance and unfolds to resume has lost their line. | High (executed) | ⚪ Low |
| 5 | `count_lines` clones the entire `FoldState` `HashSet` per fold marker per relayout (`block.rs:196`). Bounded and off the per-keystroke path, but it is a heap allocation in a layout walk. | High (read) | ⚪ Low |
| 6 | `reload_document` splits one condition across two `if had_folds` blocks straddling a relayout (`app.rs:645-659`). Correct and documented, but the reader has to hold the flag across the intervening call. | High (read) | ⚪ Low |
| 7 | Folding removes nested headings from the outline entirely, so the TOC and `]]`/`[[` shrink while a fold is up. Defensible (you cannot jump to a hidden heading) and not a listed requirement — but it is the shared root cause of DW-5.2 and note 1, and worth deciding deliberately rather than inheriting. | High (executed) | 🟡 Med |

## Issues (Phase 5)

1. **The fold marker does not show the hidden-line count at ordinary widths (DW-5.1)**
   - File: `crates/layout/src/block.rs:526-535` (`fold_marker_text`), applied at `:200-204`
   - Demonstrated by: measured markers at widths 20/40/60 — count absent in 4 of 6 realistic cases; at width 60 it truncates to `(6 h…`
   - Fix: make the count survive the clip. Clip the *title* to `content_width - width_of(" (N hidden lines)")` and append the count after clipping, rather than clipping the assembled string; fall back to the count alone when even that will not fit.

2. **Fold state does not survive a `--watch` reload (DW-5.2)**
   - File: `crates/stele/src/app.rs:684-709` (`reseat_folds`), driven from `:630-661`
   - Demonstrated by: trace 5.2-a (nested fold dropped, `collapsed` 3 → 2) and trace 5.2-b (fold re-seats onto the wrong duplicate-titled heading, hidden count 6 → 4)
   - Fix: capture the re-key basis from a **fully expanded** outline on both sides. Either lay the old document out once with an empty `FoldState` to obtain a complete `old_outline` before capturing occurrences, or key on a fold-independent path (level + text + ancestor chain) that is computable whether or not an ancestor is collapsed. The two outlines being compared must have the same entry set, or the occurrence index means different things on each side.

3. **The search-in-fold choice is never stated in the status row, and `n` reports a falsehood (listed edge case)**
   - File: `crates/stele/src/app.rs:1374-1383` (`recompute_matches`), `:1163-1170` (`report_no_matches`)
   - Demonstrated by: `/needle` → fold the containing section → status `None`; press `n` → `"no matches: needle"` on a document that does contain `needle`
   - Fix: track how many matches the fold suppressed and say so — e.g. `"no matches: needle (3 hidden by folds — R to expand)"` when the visible set is empty but the query matched before folding, and a shorter `"n hidden by folds"` note when stepping past a suppressed match.

**Phase 5 verdict: FAIL** — blockers: DW-5.1 (hidden count clipped away), DW-5.2 (fold state
lost or mis-seated across reload), search-in-fold edge case (choice not stated in the
status row; `n` reports a falsehood).

---

# Review: Phase 3 — rebase delta of heading navigation / image residency (`f1529e4`)

Scope as dispatched: only the three items introduced during the rebase. The rest of Phase 3
is out of scope and was not re-verified.

## Item 1 — the chrome-key gate, including keys read during a resize drain

PREMISE:  `T`/`+`/`-` must not reach the document while an overlay mode is active, including keys read during a resize drain. Verify the **current** behavior (`Mode::captures_all_keys()`), not the historical `mode() != Mode::Normal` gate.
EVIDENCE: `crates/stele/src/app.rs:427-445` (`chrome_action`), `:252-265` (`captures_all_keys`); both call sites `crates/stele/src/main.rs:420` (main loop) and `:466` (resize drain), funnelling through `handle_chrome_key` at `:563-604`
TRACE:    Key arrives (either loop) → `handle_chrome_key` → `state.chrome_action(key)` → `self.mode.captures_all_keys()` is the **first** guard, before the modifier check and before the key table → `Mode::Toc{..} => true`, `Mode::Search{..} => true` → returns `None` → `handle_chrome_key` returns `false` → the key falls through to `AppState::handle_key_event`, which the overlay's own table owns.
VERDICT:  **PASS**

The gate is a single function reached identically from both call sites, so it cannot hold in
one and not the other — I read both. The match on `Mode` is exhaustive with no wildcard arm,
so a future mode is a compile error at `captures_all_keys` rather than a silent omission,
which is the property the replacement was made for.

Covered by `tests/toc_key_routing.rs:127` (main loop) and `:204` (resize drain), both driving
the **real binary over a pty** and asserting on the SGR palette of the document the reader
returns to after `Esc` — the correct oracle, since an ungated chrome key does not close the
overlay and a screenshot of the overlay is identical either way. `tests/fold_key_routing.rs:115`
extends the same check to Phase 5's `z`/`R`/`M`, which inherit the gate for free.

## Item 2 — the reload path's raster sweep (placed vs. resident)

PREMISE:  A `--watch` reload must not leave a decoded raster addressed to a node of the replaced document, and must not place an image whose pixel data belongs to the previous document. The reported hazard: sweeping the **placed** set rather than the **resident** set passes the existing test while leaving a scrolled-off raster stale.
EVIDENCE: `crates/stele/src/media/sink.rs:832-856` (`reload_document`), `:441-446` (`delete_placement`), `crates/stele/src/media/residency.rs:111-113` (`resident_nodes`), `:158-163` (`remove`)
TRACE:    `reload_document` collects `self.cache.resident_nodes()` — **the resident set, not `placed`** — and calls `delete_placement` on each, which `cache.remove`s the node (refunding its bytes) and emits `a=d,d=I` (raster delete, not the visibility-only `d=i`). `placed` is then cleared, `doc` swapped. Post-state: `resident_count() == 0`, `resident_bytes() == 0`, terminal holds no old ids. A new node therefore cannot inherit old pixels, and `alloc_id`'s `holds_id` skip (`sink.rs:325-332`) has an empty set to skip against.
VERDICT:  **PASS**

Verified with a reproduction *and* a negative control, because the whole point of the
reported hazard is that the wrong version looks right.

Reproduction: two images far apart in one document; frame at scroll 0 (image A transmitted
and placed), then frame at scroll 40 (A **unplaced but still resident** — the exact state
the hazard lives in), then `reload_media` onto a new document. Replaying the wire through
`TerminalGfx`:

```
after frame @0:      stored={4111466497}  visible={4111466497: (3,1)}
after frame @40:     stored={4111466497}  visible={}          <- resident, not placed
after reload_media:  stored={}            visible={}
```

Negative control: I patched `sink.rs:842` to `let known: Vec<NodeId> = self.placed.clone();`
— the naive port the commit message describes — and re-ran. My probe **failed**
(`stored={16777217}` survived the reload), confirming it has power. I then ran the whole
committed `stele` suite against that same patch: **229 passed, 1 failed**, and the one
failure was `media::sink::tests::test_a_reload_frees_a_raster_that_outlived_its_placement`
(`sink.rs:3326`). The sink was restored and the 555/0 baseline re-confirmed.

So the hazard is not only fixed but genuinely regression-guarded by a committed test — and
that test is a real oracle, not a count: it asserts `screen.stored.is_empty()` after
replaying the emitted bytes, and asserts up front that the raster *was* resident
("...or this test is asserting nothing"). Its sibling `test_after_a_reload_a_node_never_inherits_the_previous_documents_pixels`
(`sink.rs:3350`) covers the second half, forcing the `NodeId` collision rather than hoping
for it.

## Item 3 — TOC re-seating on reload

PREMISE:  An open TOC whose `selected` index addressed the old outline must be re-seated.
EVIDENCE: `crates/stele/src/app.rs:713-731` (`reseat_toc`), called last from `reload_document` at `:660`
TRACE:    Reload → `Mode::Toc { selected }` → `count = self.tree.outline().len()`. `count == 0` → dismiss to `Mode::Normal` + `NO_HEADINGS` status. Otherwise → `selected.min(count - 1)`, clamped into the new outline. `toc_return_scroll` is re-seated on the anchored scroll in both branches, including the `Mode::Normal` early-return — so the next `Esc` is honest even when the overlay was not up.
VERDICT:  **PASS**

Covered by `app.rs:3062` (re-seat onto the new document's headings) and `app.rs:3096`
(dismiss when the reload has no headings). Ordering verified: `reseat_toc` runs **after**
the fold-applying relayout, so it clamps against the outline of the tree actually
installed. I confirmed the Phase 5 interaction directly — fold a section, open the TOC on
its last entry, reload onto a one-heading document: `selected` clamped in range, scroll
`<= max_scroll`, no panic.

## Test-DW Coverage — Phase 3 delta

| Item | Test that ran in Step 0 | Adequate |
|---|---|---|
| Chrome gate, main loop | `toc_key_routing.rs:127` (pty, real binary) | Yes |
| Chrome gate, resize drain | `toc_key_routing.rs:204` (pty, real binary) | Yes, with a caveat (Note 3-a) |
| Chrome gate applied to `z`/`R`/`M` | `fold_key_routing.rs:115` | Yes |
| Reload raster sweep, resident-not-placed | `sink.rs:3326` | Yes — proven by negative control |
| Reload, no pixel inheritance | `sink.rs:3350` | Yes |
| TOC re-seat on reload | `app.rs:3062`, `app.rs:3096` | Yes |

## Dead Code

None found in the delta.

## Correctness Dimensions — Phase 3 delta

| Dimension | Status | Evidence |
|---|---|---|
| Concurrency | N/A | Single-threaded; the sink is owned by the `Painter` and touched only from the paint path |
| Error Handling | PASS | `place_box` returns silently when a node has no resident raster (`sink.rs:470-472`) rather than emitting a put for freed pixels — the correct choice, and observable because the frame then paints the text rung instead of nothing |
| Resources | PASS | Acquire/release paired: `insert` refunds replaced bytes, `remove` refunds and returns the id to free, `reload_document` drains to `bytes_resident() == 0`. Bounded growth via the byte budget; the pinning rule's deliberate overshoot is bounded by `CAP` placements per frame (`residency.rs:136-152`) |
| Boundaries | PASS | Empty resident set, every-resident-pinned (breaks rather than blanking), re-insert of the same node, `alloc_id` wrap-around all handled and unit-tested (`residency.rs:212-301`) |
| Security | N/A | No untrusted input in the delta |

## Loaded-Skill Criteria — Phase 3 delta

| Skill | Criterion | Status | Evidence |
|---|---|---|---|
| cc-control-flow-quality | Nesting ≤ 3 | PASS | `reload_document` and `unplace_all` are flat; `insert`'s `while` + `let else` is 2 |
| cc-control-flow-quality | McCabe ≤ 10 | PASS | `insert` ~4, `reload_document` ~2, `chrome_action` ~8 (flat key dispatch, one line per arm — the acceptable high-McCabe shape) |
| cc-control-flow-quality | Guard clauses | PASS | `chrome_action:428-435`, `place_box:470`, `reseat_toc:714` |
| cc-control-flow-quality | Loop-with-exit design | PASS | `insert`'s eviction `while` exits via `let else` when no unpinned victim exists, with the overshoot documented as deliberate |
| aposd-verifying-correctness | Requirements coverage | PASS | All three delta items traced to code and to an executed test |
| aposd-verifying-correctness | Concurrency / Security | N/A | As above |
| aposd-verifying-correctness | Error handling — no silent-failure path | PASS | The one early return (`place_box`) degrades to a visible text rung, not to nothing |
| aposd-verifying-correctness | Resource management | PASS | As above |
| aposd-verifying-correctness | Boundary conditions | PASS | As above |

## Notes (non-blocking) — Phase 3 delta

| # | Finding | Confidence | Severity |
|---|---|---|---|
| 3-a | `toc_key_routing.rs:204` issues `pty.resize(...)` then `pty.type_bytes(b"T")` and relies on timing to land the key **inside** the drain loop. If it lands in the main loop instead, the test still passes (the gate holds there too), so the drain path is not guaranteed to be exercised on any given run. The gate itself is one shared function reached identically from both sites, which I verified by reading `main.rs:420` and `:466`, so this is a test-determinism observation rather than a correctness doubt. | High (read) | ⚪ Low |
| 3-b | `crates/probe/src/lib.rs` carries no `unsafe_code` attribute at all while `crates/probe/src/io_raw.rs` and `pty.rs` contain `unsafe` blocks, against the project's stated per-crate rule. **Pre-existing** — `crates/probe` is untouched by both commits under review. Reported, not chased. | High (executed) | ⚪ Low |
| 3-c | The stated `deny(unsafe_code)` exception in `crates/stele` is still exactly one: `terminal.rs:478` (`#[allow(unsafe_code)]`, the signal handler). No second opt-out appeared in either commit. Verified as requested. | High (executed) | — |

**Phase 3 verdict: PASS** — all three rebase-delta items verified with execution evidence,
the raster sweep additionally confirmed by negative control.

---

# Cross-Phase Coherence

| # | Probe | Result |
|---|---|---|
| 1 | Fold a section, then `--watch` reload — do folds, scroll anchor, and any open overlay all survive coherently? | **FAIL** on folds (Phase 5, DW-5.2 — traces 5.2-a and 5.2-b). Scroll anchor and open TOC both survive correctly: executed fold → open TOC → reload onto a one-heading document; `selected` clamped in range, `scroll <= max_scroll`, no panic. The ordering in `reload_document` (clear folds → relayout → reseat folds → relayout → reseat TOC) is correct: `reseat_toc` clamps against the outline of the tree actually installed. |
| 2 | Open a search, then fold the section containing the current match | **FAIL** (Phase 5, listed edge case). The mechanism is coherent — `recompute_matches` runs on the shared relayout path so the match set follows the tree — but the reader is told `"no matches: needle"` about a query that matches. |
| 3 | Fold a section containing an image, then scroll away and back — placement and residency must both stay correct | **PASS**. Driven through one persistent `Painter`/`GfxMediaSink` (which no committed test does — `fold_placement.rs` rebuilds both per frame): `visible` empty at every folded frame, exactly one image at every open frame, id stable at `4111466497` across the fold, so unfolding costs a re-place and not a re-decode. Phase 5's claim that folding needs no sink change holds against Phase 3's actual residency split. |
| 4 | Collapse-all on a document whose headings are all inside one folded H1 | **PASS** with a caveat. Markers correct and `expand_all` restores `y body` and `z body`. The caveat is Phase 5 Note 1: `collapse_all` reads the fold-abbreviated outline and replaces the set, so a second `M` drops nested folds. Not a listed requirement. |
| 5 | Fold, then resize narrower — the marker line must still respect the layout width | **PASS** on the width bound. Swept start widths 100/80/60 → 80, 60, 40, 24, 20, 10, 4, 1; every line re-measured through the width engine stayed within `tree.width()`. The marker's *content* at those widths is the separate DW-5.1 failure. |

**Interfaces used as exposed, not as assumed:** verified. Phase 5 consumes Phase 3's
`Outline`/`OutlineEntry` (`level`, `text`, `block`), `Mode::captures_all_keys()`, and the
sink's recompute-placement-per-frame invariant, and each is the interface Phase 3 actually
exposes. The one seam defect — `reseat_folds` — is Phase 5's: it assumes `Outline` is a
complete list of the document's headings, but Phase 3's `Outline` is a list of the headings
*in the current tree*, and Phase 5 itself is what made those two things differ by teaching
`walk_blocks` to skip a folded range. Attributed to Phase 5, the later phase, per the
batch rule.

**Regressions introduced by the later phase into the earlier one:** none found. Phase 5's
`z`/`R`/`M` inherit `captures_all_keys()` rather than adding a fourth ungated table, and
`fold_key_routing.rs:115` proves it against the real binary. Folding shrinks the TOC while
a fold is up (Phase 5 Note 7), which is a deliberate consequence rather than a regression,
but it is the shared root cause of the DW-5.2 failure and deserves a deliberate decision.

---

## Verdict

**FAIL** — driven entirely by Phase 5.

- **Phase 3 (rebase delta): PASS** — clean. Nothing here needs a fix; the raster sweep is
  correct, regression-guarded, and confirmed by negative control.
- **Phase 5: FAIL** — three blockers:
  1. DW-5.1 — the marker's hidden-line count is clipped away at ordinary layout widths (absent in 4 of 6 realistic cases; truncated to `(6 h…` at width 60)
  2. DW-5.2 — fold state does not survive a `--watch` reload: nested folds are dropped, and a duplicate heading title re-seats the fold onto the wrong heading
  3. Listed edge case — the search-in-fold skip choice is never stated in the status row, and `n` reports `"no matches"` for a query that does match
