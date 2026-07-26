# Discovery + Design: Phase 3 — Heading navigation, TOC overlay, image residency

## Files Found

| File | Relevance |
|---|---|
| `crates/layout/src/lib.rs` | `LayoutTree { lines, line_blocks, width }`, `Semantic::Heading(u8)`, `first_line_of`, `block_at`. Derives `PartialEq/Eq/Hash`. |
| `crates/layout/src/block.rs` | `Ctx` (owns `lines`/`line_blocks`/`current_block`/`depth`), `walk_block`'s `BlockKind::Heading` arm at `:256`. |
| `crates/stele/src/app.rs` | `AppState`, `handle_key_event` → `handle_control_chord` / `handle_key`, `relayout_preserving_anchor` (Phase 1), `set_status`, `status`. No `Mode` yet. |
| `crates/stele/src/media/sink.rs` | `GfxMediaSink`. `placements: HashMap<NodeId, Placement>` carries residency *and* visibility; `CAP = 32` is applied to `placements.len()`. `evict_stale` at `:307`; the documented off-by-one at `:1219-1225`. |
| `crates/gfx/src/decode.rs` | `letterbox` at `:278` — the single Triangle resize at `:287`. |
| `crates/stele/src/painter.rs` | `frame_with_status` → `frame_body` (calls `media.begin_frame`) + `paint_status_row`. No overlay path. |
| `crates/stele/src/main.rs` | Event loop; `handle_chrome_key` for keys needing `ctx`/`painter`. |
| `crates/stele/tests/stale_placement.rs` | Holds the only integration-level `TerminalGfx` wire model (not yet in `tests/common/`). |

## Current State

Headings exist only as `Semantic::Heading(level)` runs inside laid-out lines — nothing collects them.
`AppState` has no mode; every key goes straight to one of two flat match tables.

The media sink already separates *visibility* (`placed`, ended by `a=d,d=i` at the frame boundary) from
*residency* (the raster, ended by `a=d,d=I`). That split is sound and stays. What is broken is the
arithmetic governing residency and the map it lives in:

- `evict_stale` computes `cutoff = frame_counter - (DATA_GRACE_FRAMES + 1)` and evicts
  `last_seen_frame <= cutoff`. At the top of frame *F* that frees the raster of anything not painted in
  frame *F-1* — **before** frame *F*'s paints can claim it. The resident window is therefore always
  empty in the one case it exists for: scroll off, scroll back. `sink.rs:1219-1225` says so and
  deliberately does not pin it.
- `CAP = 32` is checked against `placements.len()`, and `placements` *is* the residency map. So raster
  retention is silently capped at 32 records — exactly what DW-3.6 forbids.

## Gaps vs the plan

| Gap | Resolution |
|---|---|
| Plan's file scope omits `crates/stele/src/painter.rs` and `main.rs`, but DW-3.2 requires a full-screen overlay to be *painted* and DW-3.3 asserts on the placements the overlay frame emits. | Implemented anyway, minimally: `AppState` produces the overlay's rows (pure, testable), `Painter::frame_overlay` paints them through the same `media.begin_frame` boundary, `main.rs` picks the path by mode. Recorded here as a deviation rather than an UPDATE_PLAN — the omission is clerical, not architectural. |
| `Produces` fixes `OutlineEntry { level, text, block }`, but `block` cannot address a heading nested inside a blockquote (`line_blocks` only tags top-level blocks, so `first_line_of(heading)` is `None` and two headings in one blockquote share an anchor). | `Outline` keeps the entry shape verbatim **and** records each heading's tree line privately. Navigation uses the line (exact for every heading); `jump_to_block(NodeId)` uses `first_line_of` and falls back to the outline's line. |
| Plan lists a "two-stage image downscale" as IN scope. | **Dropped — measured, does not pay.** See Measurements. |
| `DATA_GRACE_FRAMES` frame-count grace and the byte budget would be two mechanisms evicting the same thing. | The byte budget becomes the sole governor of residency (DW-3.6's wording: "retention is governed by the byte budget"). The frame sweep is removed, not repaired — it was an admitted approximation of a viewport margin the trait cannot observe, and a byte budget is the honest form of that intent. Its exact pre-change behavior survives behind a `#[doc(hidden)]` test switch so DW-3.5's baseline is the real old code. |

## Code Standards

`#![forbid(unsafe_code)]` (`deny` in `stele`); exhaustive matches, no wildcard arms on `Semantic`;
hand-rolled error enums with manual `Display`; sentence-style test names; DW-tagged tests; shared
harness under `crates/stele/tests/common/` — so `TerminalGfx` moves out of `stale_placement.rs` into
`common/` rather than being copied into a new file. Never assert on `Run.width`. Import grouping:
`std`, blank, external+workspace alphabetical, blank, `crate::`.

## Test Infrastructure

`cargo test` built-in. Unit tests in `#[cfg(test)] mod tests` at the bottom of each file; cross-crate
and terminal-level behavior in `crates/stele/tests/` with `mod common;`. Phase 1's precedent for a
"committed benchmark" (DW-1.7, `crates/width/src/engine.rs:294`) is a wall-clock A/B of the new path
against the *retained* pre-change path inside one `#[test]`, with a generous ratio floor. DW-3.5
follows it exactly.

## Measurements

`cargo run --release -p gfx --example downscale_probe` (committed), 6000×6000 RGBA source, best of 3:

| Target | decode | decode + one-stage (today) | decode + two-stage |
|---|---|---|---|
| 2400×2400 (a full-width box at `FALLBACK_CELL_PX` 24×48) | 273 ms | 445 ms | 417 ms |
| 1000×800 | 275 ms | 352 ms | 331 ms |
| 500×400 | 277 ms | 363 ms | 325 ms |

**Assumption 2 (two-stage downscale still helps at the real ~2400 px target) — NOT CONFIRMED.**
At 2400×2400 the two-stage resize saves 28 ms of a 445 ms operation: **6% end-to-end**, and the decode
that dominates it is untouched either way. The original audit's 500×400 case is where the win lives
(45% of the resize stage, still only 10% end-to-end). 6% for a second resize path and a hand-rolled
halving loop is squarely the performance skill's "trading maintainability for <10% gain" red flag, so
the two-stage step **does not ship**. The probe stays committed as the evidence.

**Assumption 1 (most of the 99–102 ms scroll-back is the off-by-one) — CONFIRMED, and understated.**
The off-by-one does not shave the scroll-back frame, it *removes* it. Measured by the committed
benchmark (`sink.rs::test_dw_3_5_…`), same harness, same sink, 6000×6000 source into a 40×20 cell box
(960×960 raster target):

| Scroll-back return frame | Time |
|---|---|
| Pre-change residency window (`simulate_pre_change_residency_for_test(true)`) | **5.018 s** |
| Shipped | **9 µs** |

≈ 557,000× — because the two frames are not the same work at all. The baseline decodes 36 megapixels,
rescales, PNG-encodes and base64-transmits; the shipped frame emits one `a=p` escape. The DW's 10× floor
has five orders of magnitude of margin, which is why it is expressed as a ratio and not a latency budget.

Because the fix alone is this decisive, the byte-budgeted LRU was **not** built to make scroll-back
fast — it was built because DW-3.6 requires retention to stop being capped at 32, and because a cache
with no bound is not a cache. Both facts are stated rather than conflated.

(The timings above are in `cargo test`'s debug profile, where the `image` stack is now built at
`opt-level = 3` via a `[profile.dev.package.*]` block — see the note in the workspace `Cargo.toml`.
Without it the same fixture cost 11 s to encode and 11 s to decode, and the media suite paid for it on
every run.)

## DW Verification

| DW-ID | Done-When Item | Status | Test Cases |
|---|---|---|---|
| DW-3.1 | `]]`/`[[` move to next/previous heading; no-op with a status message when there are none | COVERED | `app.rs`: `test_dw_3_1_bracket_bracket_moves_to_the_next_heading_and_back`, `test_dw_3_1_a_document_with_no_headings_reports_instead_of_moving`, `test_dw_3_1_a_heading_as_the_first_and_last_block_clamps_at_both_ends`, `test_dw_3_1_a_lone_bracket_is_not_a_motion` |
| DW-3.2 | `t` opens a scrollable TOC with levels; `Enter` jumps; `Esc` restores prior scroll | COVERED | `app.rs`: `test_dw_3_2_t_opens_a_toc_listing_every_heading_with_its_level`, `test_dw_3_2_enter_jumps_to_the_selected_heading_and_leaves_the_overlay`, `test_dw_3_2_esc_returns_to_the_scroll_position_the_toc_was_opened_from`, `test_dw_3_2_a_toc_longer_than_the_screen_scrolls_to_keep_the_selection_visible`, `test_dw_3_2_t_reports_instead_of_opening_when_there_are_no_headings`, `test_dw_3_2_a_viewport_too_short_for_the_overlay_paints_no_rows` |
| DW-3.3 | Entering and leaving the TOC leaves exactly the placements the returning frame paints | COVERED | `tests/toc_overlay.rs`: `test_dw_3_3_the_toc_frame_takes_every_placement_down_and_the_returning_frame_puts_back_exactly_what_it_paints` |
| DW-3.4 | Image scrolled off and back within the residency budget is re-placed, not re-transmitted | COVERED | `sink.rs`: `test_dw_3_4_a_box_scrolled_off_and_back_is_re_placed_without_re_transmitting`, `test_dw_3_4_a_raster_the_budget_evicted_is_re_transmitted_when_it_returns`, `test_dw_3_4_100_scroll_cycles_transmit_each_image_exactly_once` |
| DW-3.5 | Committed benchmark: scroll-back return frame for a 6000×6000 image ≥10× faster than the pre-change baseline in the same harness | COVERED | `sink.rs`: `test_dw_3_5_scroll_back_return_frame_is_at_least_10x_faster_than_the_pre_change_baseline` |
| DW-3.6 | Retention governed by the byte budget, not the 32-placement cap; >32 images still retain rasters up to the budget | COVERED | `sink.rs`: `test_dw_3_6_forty_images_keep_their_rasters_although_only_32_may_be_placed`, `test_dw_3_6_the_byte_budget_evicts_least_recently_used_and_never_a_placed_box` |

**All items COVERED:** YES (6 of 6, matching the dispatch list).

## Design Decisions

### Design: `Outline`

**Approaches considered**

1. **Built in `layout` during the block walk**, stored on `LayoutTree`, exposed as `LayoutTree::outline()`.
2. **Derived in `stele`** by scanning the tree's lines for `Semantic::Heading` runs and grouping by `block_at`.
3. **Derived from the `Document` AST** in `stele`, independent of layout.

| Criterion | A (layout walk) | B (scan lines) | C (AST) |
|---|---|---|---|
| Interface simplicity | 1 accessor | 1 accessor + a scanner `stele` owns | 1 accessor |
| Information hiding | Heading level, text and line all known where they are produced | `stele` must learn that "a heading is a line whose runs are `Semantic::Heading`" — layout internals leak | Heading text is honest; *line* is not knowable |
| Anchoring exactness | Exact line per heading, free | Re-derives what `line_blocks` already knows | Cannot address a line at all — needs a second layout pass |
| Phase 5 (folding) | Folding is a layout-walk concern; the outline is already there | Would need re-deriving after every fold | Unusable — folded headings are a layout fact |
| Cost | O(headings), once per layout | O(lines) per relayout | O(blocks), but a second traversal |

**Choice: A.** Layout is the only place that already holds level, text, the emitted line index and the
anchoring block simultaneously; anywhere else re-derives at least one of them. Sacrificed: `LayoutTree`
carries a field a media-free consumer never reads (a few hundred bytes for a large document).

**Depth check** — interface methods: `LayoutTree::outline()`, plus `Outline`'s `entries` (public field,
per the plan), `len`/`is_empty`, `line_of`, `next_after`, `previous_before`, `line_for_block`. Hidden:
that the outline is a parallel `entries`/`lines` pair, that heading text is flattened from AST inlines
rather than from runs (so `## a *b* c` reads as `a b c`, not as three fragments), that nested headings
anchor by line rather than by block. Common case (`]]`): one call, no knowledge of any of it.

### Design: mode dispatch

`Mode { Normal, Toc { selected: usize } }` on `AppState`, matched first in `handle_key_event`, per the
plan's "extend the existing dispatch". `]]`/`[[` are two-keystroke sequences, so `handle_key_event`
also carries `pending: Option<char>`, consumed by the next key in `Normal` mode only. `pending` is
*not* folded into `Mode`: `Mode` answers "what is on the screen", and a half-typed bracket is not on
the screen. `Esc`'s restore target is a private `toc_return_scroll`, so `Mode::Toc { selected }` keeps
the shape the plan fixed.

The overlay's content is produced by `AppState::toc_rows(height) -> Vec<TocRow>` — a pure function of
outline + selection + height, which is what makes DW-3.2's scrolling and short-terminal cases unit
testable without a terminal. The painter only prints what it is handed.

### Design: residency

**Approaches considered**

1. **Split `placements` into a `RasterCache` (residency) + a `placed` list (visibility)**; byte budget with
   LRU eviction inside the cache; `CAP` applied to `placed.len()` only.
2. **Keep one map, add a `bytes` field and a second budget check**, leaving `CAP` on `placements.len()`
   but exempting non-placed records.
3. **Keep one map, keep the frame grace, widen `DATA_GRACE_FRAMES`** and fix the cutoff.

| Criterion | A (split) | B (one map, two rules) | C (widen grace) |
|---|---|---|---|
| Satisfies DW-3.6 | Yes, structurally | Only by convention — the next reader can re-couple them | No |
| Bug class removed | Frame arithmetic gone entirely | Frame arithmetic stays | Arithmetic stays, tuned |
| Byte accounting | One place owns it | Spread across the sink | Absent |
| Interface | `get/insert/remove/bytes_resident` | Sink keeps every rule inline | — |
| Constraint compliance | "if residency records share the placements map, split them" — done | Violates it | Violates it |

**Choice: A.** The plan's constraint names it, and it is the only option where "residency is capped at
32" cannot come back by accident: after the split there is no expression anywhere that compares `CAP`
to a count of rasters. Sacrificed: one more file, and the cache must be told which nodes are pinned by
a live placement (passed as `&self.placed` at insert time) so an eviction cannot blank a box already on
screen this frame. Over-budget is *allowed* when every resident is pinned — a visible image beats a
respected budget, and it is bounded by `CAP` placements.

`RASTER_BUDGET_BYTES = 64 MiB`, carrying its derivation in a comment: a full-viewport raster at the
2400×2400 target is ~1–3 MB of PNG, so the budget holds tens of full-screen images and hundreds of
inline formulas — an order of magnitude past the 32-placement cap, which is the coupling DW-3.6 breaks.

**Consequences for existing tests (deliberate, not weakening).** Three DW-6.1 tests pin behavior this
phase is chartered to change:

- `test_dw_6_1_grace_period_evicts_a_node_absent_for_a_full_frame` pins the grace sweep DW-3.4 exists
  to remove. Replaced by the DW-3.4 pair (survives absence; budget-evicted raster is re-transmitted).
- `test_dw_6_1_a_slot_from_a_previous_frame_is_still_evicted_for_a_new_box` asserts a `d=I` fires when
  a 33rd box appears — literally the coupling DW-3.6 forbids. Retargeted: the 33rd still gets a real
  graphics placement, *and* the first 32 rasters are still resident.
- `test_dw_6_1_100_scroll_cycles_stay_balanced_and_never_exceed_cap` asserts `deletes > 0`. With the
  budget governing, five small images are never evicted; the assertion becomes the stronger "each image
  is transmitted exactly once across 100 cycles".

### Found while implementing (not in the plan)

Splitting residency from the placement cap opened a hole the old coupling had hidden: `replace_if_cached`
placed a resident raster **unconditionally**, because under the shared map "resident" implied "within the
32 cap". Once 40 rasters can be resident, a frame showing all 40 boxes is 40 cache *hits* and would have
emitted 40 placements. The quota is now checked on the cache-hit path too, and pinned by
`test_the_placement_cap_holds_even_when_every_raster_is_already_resident`.

## Rebase onto Phase 2 (`a71c649`)

Phase 2 landed first and restructured the key-dispatch and session code this phase extends. Three
textual conflicts (two import lines, one paint site); two real behavioural interactions underneath them.

| Interaction | Resolution | Why not the other way |
|---|---|---|
| **Chrome keys vs. `Mode::Toc`.** `main.rs` offers every key to `handle_chrome_key` *before* `AppState`, in the main loop and in the resize drain. Ungated, `T`/`+`/`-` fired while the overlay was up — re-theming and relaying out a document the reader could not see, then handing it back changed on `Esc`. | `handle_chrome_key` returns early unless `state.mode() == Mode::Normal`. Chrome keys are document-reading keys; a mode that owns the keyboard owns all of it. | The alternative — teaching the TOC to forward `T`/`+`/`-` — leaves the key-stealing *shape* intact, and the shape is the defect: any binding added to the chrome table silently outranks every mode `AppState` will ever have, invisibly to the mode. Phase 4's `Mode::Search` reads text, and `+`, `-`, `T` are characters someone types into a query. |
| **Reload vs. residency.** Phase 2's `MediaSink::reload_document` swept `self.placements`; after the Phase 3 split that map is two — `cache` (residency) and `placed` (visibility). The auto-merge kept the old expression, which no longer compiles, and porting it to the *visible* set would have been the quiet wrong answer. | Sweep `cache.resident_nodes()`. A raster outlives its placement by design, so the resident set is strictly larger and is where a node from a dead document survives unseen. `placed` is cleared too, so the postcondition holds by construction rather than by an invariant declared elsewhere in the file. | Sweeping `placed` passes Phase 2's own reload test — it places an image and reloads immediately, so the node is both placed and resident. `test_a_reload_frees_a_raster_that_outlived_its_placement` scrolls the image off first, which is the only state that distinguishes the two, and it fails under that mutation. |

An open TOC is also re-seated on reload (`AppState::reseat_toc`): `selected` indexed the old outline, so
it is clamped into the new one, or the overlay is dismissed with the `no headings` message when the
reloaded document has none. Without it the overlay highlights a row the reader cannot see and `Enter`
resolves to no line at all.

**Every new test here was mutation-checked** — the guard removed, the test observed to fail with the
right message, the guard restored. That mattered: the first version of the routing test asserted "the
overlay is still on screen after `T`", which passes with or without the gate, because an ungated chrome
key changes the document *behind* the overlay and the next frame is still the overlay. The assertion had
to move to the far side of `Esc` (identical rows after a `+`/`-` round trip; identical SGR palette after
a `T` read inside the resize drain) before it could fail at all.

## Prerequisites

- [x] Phase 1 landed (`relayout_preserving_anchor`, reserved status row, content height `rows - 1`)
- [x] `layout` exposes `first_line_of`/`block_at` for jumping
- [x] Sink already separates `a=d,d=i` from `a=d,d=I`
- [x] `image` crate available for the DW-3.5 fixture

## Recommendation

**BUILD.** Ship: the `Outline` + `]]`/`[[` + `Mode::Toc` overlay; the residency split with a
byte-budgeted LRU; the removal of the frame-grace arithmetic. Do **not** ship the two-stage downscale —
measured at 6% end-to-end at the real target, and said so rather than shipping a no-op.
