# Review: Phase 3 — heading navigation and image residency

Worktree: `/Users/r/repos/stele/.code-foundations/wave-worktrees/phase-3` @ `b49dff6`

## Executed Results (Step 0)

- `cargo test --workspace` → **432 passed, 0 failed** (exit 0)
- `cargo clippy --workspace --all-targets` → clean, no warnings
- `cargo fmt --all -- --check` → clean (exit 0)
- `cargo run --release -p gfx --example downscale_probe` → reproduced (numbers under DW-3.5 / claim 5)
- Reviewer-authored timing probe (written, run, deleted; tree left clean) → numbers under DW-3.5

## Requirement Fulfillment

### DW-3.1
PREMISE:  `]]`/`[[` move to the next/previous heading and no-op with a status message in a document with none.
EVIDENCE: `crates/stele/src/app.rs:376-397` (`jump_heading`), `:474-499` (bracket pairing ahead of the chord table), `crates/layout/src/lib.rs:355-368` (`next_after` / `previous_before`, both strict)
TRACE:    `]` `]` at scroll 0 with headings at lines `[0, 6, 12, …]` → `pending=Some(']')` → second `]` matches → `next_after(0)` = index 1 → `line_of(1)` = 6 → `set_scroll(6)`, scroll changed so no message. Empty outline → `next_after` = `None` → `target=None` → `set_status("no headings in this document")`, scroll untouched. At the last heading → `next_after` = `None` → `"last heading"`.
VERDICT:  PASS — `test_dw_3_1_bracket_bracket_moves_to_the_next_heading_and_back`, `..._a_document_with_no_headings_reports_instead_of_moving`, `..._a_heading_as_the_first_and_last_block_clamps_at_both_ends`, `..._a_lone_bracket_is_not_a_motion_and_does_not_eat_the_next_key`, all green in Step 0.

Both listed edge cases for this item are covered: zero headings (message, no move, both directions) and heading-as-first/last block (clamp + message at each end, no wrap). The clamp path is correct in both sub-cases I traced — a last heading *below* `max_scroll` clamps, moves nothing on the second press, and then reports.

### DW-3.2
PREMISE:  `t` opens a scrollable TOC listing every heading with its level; `Enter` jumps to the selected one; `Esc` returns to the prior scroll position.
EVIDENCE: `crates/stele/src/app.rs:402-411` (`open_toc`, captures `toc_return_scroll`), `:419-449` (`toc_rows` windowing + level rendering), `:509-551` (`handle_toc_key`), `crates/stele/src/painter.rs:191-231` (`frame_overlay` / `overlay_body`), `crates/stele/src/main.rs:277-288` (mode-dispatched paint)
TRACE:    `t` at scroll 40 → outline non-empty → `index_at_or_before(40)` = 4 → `toc_return_scroll=40`, `Mode::Toc{selected:4}`. `toc_rows(5)` with 30 entries → `height=5`, `first = 4.saturating_sub(2).min(25) = 2` → rows 2..7, `selected` flag true on index 4 → text `"  ## Heading 4"` (indent `2*(level-1)`, then `level` `#`s). `Enter` → `line_of(4)` → `set_scroll`, `Mode::Normal`. `Esc` → `set_scroll(40)`, `Mode::Normal`.
VERDICT:  PASS — six green tests: `test_dw_3_2_t_opens_a_toc_listing_every_heading_with_its_level`, `..._enter_jumps_to_the_selected_heading_and_leaves_the_overlay`, `..._esc_returns_to_the_scroll_position_the_toc_was_opened_from`, `..._a_toc_longer_than_the_screen_scrolls_to_keep_the_selection_visible`, `..._t_reports_instead_of_opening_when_there_are_no_headings`, `..._a_viewport_too_short_for_the_overlay_paints_no_rows`.

Edge cases: TOC longer than screen — the 30-entry/5-row test walks the whole list and asserts the window is full and the selection is on screen at every step. Terminal too short — `toc_rows(0)` returns empty (guard at `app.rs:426`), and `overlay_body`'s `for row in 0..size.height` is a zero-iteration loop, so the overlay frame degenerates to the status row with no panic; the state test also confirms the overlay is still dismissable from that viewport. Zero headings — `open_toc` refuses and reports rather than showing a blank screen.

### DW-3.3
PREMISE:  Entering and leaving the TOC leaves exactly the placements the returning frame paints — no stale image survives.
EVIDENCE: `crates/stele/src/painter.rs:210` (`self.media.begin_frame(out)` is the first thing `overlay_body` does), `crates/stele/src/media/sink.rs:395-406` (`unplace_all`, `a=d,d=i`, cache untouched), `crates/stele/tests/toc_overlay.rs:144-192`
TRACE:    Document frame at scroll 4 places ids {A,B} → `t` → overlay frame calls `begin_frame` → `unplace_all` emits `d=i` for A and B, `placed.clear()`; `overlay_body` paints no `Line::Reserved`, so nothing re-places → terminal model's `visible` is empty, `stored` still holds {A,B} → `Esc` → document frame `begin_frame` (nothing live to unplace) then each painted box re-places → `visible == placed` and `|visible| == visible_boxes()`.
VERDICT:  PASS — `test_dw_3_3_the_toc_frame_takes_every_placement_down_and_the_returning_frame_puts_back_exactly_what_it_paints`, plus `test_dismissing_the_toc_re_places_the_same_rasters_instead_of_re_transmitting` and `test_a_toc_jump_repaints_at_the_new_position_with_only_that_screens_media`, all green.

This covers the listed edge case "overlay entered while images are placed": the harness scrolls to a position with real media before opening the TOC and asserts `expected > 0` so the test cannot pass vacuously. The assertion is against `common::termgfx::TerminalGfx`, a replay of the emitted wire (which itself asserts a put never names an unstored id), not an escape count.

### DW-3.4
PREMISE:  An image scrolled off-screen and back within the residency budget is re-placed without re-transmitting its pixel data.
EVIDENCE: `crates/stele/src/media/sink.rs:584-597` (`replace_if_cached`), `:338-345` (`begin_frame_inner` — no raster sweep), `crates/stele/src/media/residency.rs:81-87` (`get`, target-keyed hit + LRU touch)
TRACE:    Frame 1 paints A → `transmit_and_place` → `a=t` + `a=p`, `cache[A] = {id₁, target, bytes}`. Frame 2 paints only B → `unplace_all` emits `d=i` for id₁, cache untouched. Frame 3 paints A → `has_placement_slot(A)` true, `cache.get(A, target)` hits → `place_box` emits `a=p` only. Zero `a=t` on the return frame.
VERDICT:  PASS — `test_dw_3_4_a_box_scrolled_off_and_back_is_re_placed_without_re_transmitting` (asserts 0 transmits *and* re-place under the original id, with a baseline arm proving the fixture can tell the two apart), `test_dw_3_4_a_raster_the_budget_evicted_is_re_transmitted_when_it_returns` (the converse — so "never re-transmits" cannot be satisfied by an unbounded cache), `test_dw_3_4_100_scroll_cycles_transmit_each_image_exactly_once`.

### DW-3.5
PREMISE:  A committed benchmark shows the scroll-back return frame for a 6000×6000 image at least 10× faster than the pre-change baseline recorded in the same harness.
EVIDENCE: `crates/stele/src/media/sink.rs:1928-1998` (the benchmark), `:284-286` + `:352-362` (the pre-change simulation it measures against), `:983-1003` (fixture)
TRACE:    One sink, five frames. Frames 1-2 seed and scroll off; frame 3 is timed with the shipped residency (cache hit → one `a=p`); the pre-change switch goes on; frame 4 scrolls off; frame 5 is timed with the old sweep, which frees the raster at the top of the very frame that wants it, so the paint re-decodes / rescales / re-encodes / re-transmits. The test asserts `creates == 0` on the fixed frame and `creates == 1` on the baseline frame before comparing, so neither arm can be a no-op.
VERDICT:  PASS — `test_dw_3_5_scroll_back_return_frame_is_at_least_10x_faster_than_the_pre_change_baseline` green. I re-measured the same two frames independently (probe written, run, deleted): **fixed 8.125 µs, 58 bytes emitted, 0 transmits; baseline 5.091 s, 26 901 bytes emitted, 1 transmit → 626 614×**, against a 137 MB source PNG. The margin over the 10× floor is six orders of magnitude, so the assertion is not scheduling-noise sensitive.

### DW-3.6
PREMISE:  Raster retention is governed by the byte budget, not by the 32-placement cap; a document with more than 32 images still retains rasters up to the budget.
EVIDENCE: `crates/stele/src/media/sink.rs:118` (`CAP`, placements only), `:131` (`RASTER_BUDGET_BYTES`), `:420-422` (`has_placement_slot` — the *only* use of `CAP`, checked against `placed.len()`), `crates/stele/src/media/residency.rs:122-154` (`insert` — eviction driven by `bytes > budget`)
TRACE:    40 images painted one per frame → each `transmit_and_place` inserts into `RasterCache`; `self.bytes` never exceeds 64 MiB for 40 4×4 PNGs, so the eviction loop never runs → `resident_count() == 40 > CAP`. Re-walking all 40 costs 40 cache hits and 0 transmits. Grepped the production code: no expression anywhere compares `CAP` to a count of rasters.
VERDICT:  PASS — `test_dw_3_6_forty_images_keep_their_rasters_although_only_32_may_be_placed`, `test_dw_3_6_a_thirty_third_box_is_placed_without_costing_any_raster`, plus six `media::residency::tests` unit tests (budget-fits-40, LRU victim selection, pinning, byte replacement on re-insert, target-keyed hit, remove refunds bytes).

**All requirements met:** YES

## Specific Claims Scrutinized

### 1. The benchmark's baseline is the real pre-change behaviour — verified

I read the pre-change code at `HEAD~1` rather than trusting the new comment. The old `evict_stale` (`git show HEAD~1:crates/stele/src/media/sink.rs`) ran after `frame_counter += 1` and freed every node with `last_seen_frame <= frame_counter - (DATA_GRACE_FRAMES + 1)`, i.e. `<= N - 2` at the top of frame *N*. A node last painted in frame *N-1* survives; a node absent from *N-1* is freed. The new `evict_everything_absent_last_frame` (`sink.rs:352-362`) runs before `unplace_all`, so `self.placed` still holds frame *N-1*'s boxes, and frees every resident not in that set. **Same predicate, same position in the frame.** Not a strawman.

Not a no-op or a cold-path comparison either: the baseline frame really emits a transmit (asserted in-test, and I measured 26 901 bytes of `a=t` payload against 58 bytes on the fixed frame), and it really pays the decode — my probe's 5.09 s is consistent with the 270 ms decode + resize the `downscale_probe` independently measures for the same 6000×6000 source. If anything the comparison is conservative: the baseline arm runs *after* the fixed arm, so its file read is served from a warm page cache.

### 2. Retargeted pre-existing tests — verified, all three are strengthenings

| Old test (HEAD~1) | Disposition | Assessment |
|---|---|---|
| `test_dw_6_1_a_slot_from_a_previous_frame_is_still_evicted_for_a_new_box` | Rewritten as `test_dw_3_6_a_thirty_third_box_is_placed_without_costing_any_raster` | Every assertion is retained verbatim (unplace count = CAP, exactly one put of id 33, `creates == 1`, no alt text) *except* the one that required a `d=I` — which is inverted to "no `d=I` at all" and joined by two new ones (`resident_count == CAP+1`, and a third frame proving box 0 comes back as a re-place). Genuinely stronger; the behaviour change is what DW-3.6 mandates. |
| `test_dw_6_1_grace_period_evicts_a_node_absent_for_a_full_frame` | Deleted | It pinned the frame-count grace, which no longer exists. What it was really protecting — that retention is *bounded* — is now pinned harder by `test_dw_3_4_a_raster_the_budget_evicted_is_re_transmitted_when_it_returns` (which measures the real cost of one raster and sets the budget one raster wide rather than guessing) and by `residency::tests::test_the_least_recently_used_raster_is_the_one_the_budget_frees`. Not a coverage loss. |
| `test_dw_6_1_100_scroll_cycles_stay_balanced_and_never_exceed_cap` | Rewritten as `test_dw_3_4_100_scroll_cycles_transmit_each_image_exactly_once` | The old assertions were a create/delete *balance* computed from the same log it was checking (self-referential) plus `deletes > 0`. The new ones are absolute: `creates == 5`, `deletes == 0`, `resident_count == 5`, and the replayed terminal draws exactly one image. The dropped `max_live <= CAP` clause counted transmits-minus-deletes, which was never a placement count anyway; the real cap invariant is now covered by `test_dw_6_1_cap_32_refuses_the_33rd_box_instead_of_evicting_a_live_one` (untouched) and by the new `test_the_placement_cap_holds_even_when_every_raster_is_already_resident`. |

`crates/stele/tests/stale_placement.rs` was modified only to import `TerminalGfx` from the new shared `tests/common/termgfx.rs` — the moved code is byte-identical and no assertion changed.

### 3. The removed frame-count grace — verified, no path evicts a displayed raster and no path places a freed one

Enumerated every eviction site and traced each:

- `RasterCache::insert` (`residency.rs:136-152`) is the only budget-driven eviction. Its victim search excludes `node` itself and every id in `pinned`, and `pinned` is built at `sink.rs:636` from `self.placed` — which, because `begin_frame` clears it, is exactly the set of boxes drawn so far in the frame the reader is looking at. When *every* resident is pinned the loop breaks and the cache goes over budget deliberately; the overshoot is bounded because only a placed box can pin and at most `CAP` are placed per frame.
- A resident that will be painted *later* in the same frame is not yet pinned and can be evicted mid-frame — but that is safe, because the later `replace_if_cached` then misses and re-transmits. No stale place.
- `delete_placement` is reachable from `MediaSink::evict` (no production callers), a failed image decode (`sink.rs:682`), and the RaTeX→txm fall-through (`sink.rs:769`). In every one of those, `replace_if_cached` has already returned `false` for the node, so the node is not in `placed` and cannot be on screen.
- The converse direction: `place_box` (`sink.rs:458`) early-returns unless `cache.peek(node_id)` is `Some`, and the cache is a faithful model of the terminal's stored set — every insert is preceded by an `a=t` on the same id (`sink.rs:627-637` reuses the existing id when a record exists, so a same-id retransmit replaces rather than orphans), and every removal returns an id the caller immediately deletes on the wire. `TerminalGfx::apply` asserts the invariant directly on the emitted bytes in both the unit and integration harnesses.

### 4. The placement cap cannot be exceeded by resident rasters — verified

`place_box` is the single emitter of `a=p` (`sink.rs:476`) and the single writer of `placed` (`:462-464`). Both of its callers gate on `has_placement_slot` — `replace_if_cached` at `:592` and `transmit_and_place` at `:624`. `has_placement_slot` is `placed.contains(&incoming) || placed.len() < CAP`, so a new node is admitted only while `placed.len() <= 31` and `placed` tops out at exactly 32. The warm-cache hole (40 residents ⇒ 40 hits ⇒ 40 unconditional places, if the quota were only checked on the transmit path) is closed by the check inside `replace_if_cached` and pinned by `test_the_placement_cap_holds_even_when_every_raster_is_already_resident`, which asserts exactly `CAP` puts, zero transmits, and alt text at the correct row for all eight over-quota boxes.

### 5. The dropped two-stage downscale — verified recorded, reproducible, and fully absent

`crates/gfx/examples/downscale_probe.rs` is committed and I ran it:

```
source: 6000x6000 RGBA, 63752850 PNG bytes
target 2400x2400: decode 270.0ms | decode+one-stage 451.2ms | decode+two-stage 415.0ms
target 1000x800:  decode 273.4ms | decode+one-stage 383.2ms | decode+two-stage 338.2ms
target  500x400:  decode 273.4ms | decode+one-stage 361.9ms | decode+two-stage 326.7ms
```

The numbers recorded in `crates/gfx/src/decode.rs:278-289` (445 / 417 / 273 ms) reproduce within noise. Nothing half-implemented survives: `decode.rs` contains exactly one `imageops::resize` with `FilterType::Triangle` at `:299`, no halving loop, no second resize path, and no dead helper — grepped for `halve` / `two_stage` / `two-stage`, which appear only in the doc comment and in the probe. One framing caveat under Notes.

## Test-DW Coverage

- [x] All six DW items have automated tests that ran and passed in Step 0 — no item rests on observed behaviour alone.
- [x] Every listed edge case has a named test: zero headings (`test_dw_3_1_a_document_with_no_headings_reports_instead_of_moving`, `test_dw_3_2_t_reports_instead_of_opening_when_there_are_no_headings`), heading first/last (`test_dw_3_1_a_heading_as_the_first_and_last_block_clamps_at_both_ends`), TOC taller than screen (`test_dw_3_2_a_toc_longer_than_the_screen_scrolls_to_keep_the_selection_visible`), overlay over live placements (`test_dw_3_3_...`), viewport too short (`test_dw_3_2_a_viewport_too_short_for_the_overlay_paints_no_rows`).
- [x] Test coverage matches the stated 100% level. The supporting layer is covered too: eight new `crates/layout/tests/behavior.rs` outline tests (levels/order, empty outline, text through emphasis/code/link, wrapped heading anchored at its first line, blockquote-nested heading addressability, strict-vs-inclusive motion semantics, tree determinism, setext headings) and six `media::residency::tests` unit tests.
- [x] Conventions honoured: DW-tagged sentence-style test names, shared harness under `crates/stele/tests/common/`, no assertion on `Run.width` (the outline tests use `line_text_at`, which reads painted text), no `thiserror`, no new `unsafe`, no wildcard arm on a `Semantic`/`Capture` match (`heading_text` matches `InlineKind` exhaustively).

## Dead Code

None blocking. No unreachable code after early returns, no debug statements, no commented-out blocks, no unused imports (clippy `--all-targets` is clean). Three test-support surfaces reach production code rather than `#[cfg(test)]` — see Notes 2 and 3.

## Correctness Dimensions

| Dimension | Status | Evidence |
|-----------|--------|----------|
| Concurrency | N/A | Nothing concurrent is introduced or touched: `AppState` and `GfxMediaSink` are `&mut self` state machines driven from one event loop; no threads, async, or shared state. |
| Error Handling | PASS | Traced the adversarial path: `decode_and_scale` `Err` → `delete_placement` (drops any stale placement) → `degrade_to_text` with sanitized alt text (`sink.rs:676-688`). `frame_overlay` mirrors `frame_with_status`'s discipline — every `?` lives in `overlay_body`, so `SYNC_END` + `flush` run on the error path and the terminal is never left inside a synchronized-update block. |
| Resources | PASS | Raster residency is bounded by an explicit byte budget with LRU eviction; the only unbounded case is "every resident pinned", which is bounded by `CAP` and deliberate. `alloc_id`'s skip loop terminates because the resident set is budget-bounded. Test-fixture temp files are the one blemish — Note 1, non-blocking. |
| Boundaries | PASS | Traced each: `toc_rows` — `height = min(height, entries.len())` then early return on `0`, so `entries.len() - height` cannot underflow and `first + height <= entries.len()`; a `selected` beyond the list yields no highlighted row rather than a panic. `overlay_body` — `for row in 0..size.height` caps `row` at `u16::MAX - 1`, so `row + 1` cannot overflow even at a degenerate viewport. `RasterCache` byte arithmetic — every `-=` is matched by the `+=` that put the same record in, so the `u64` cannot underflow; `order` and `resident` cannot desync (`touch` is only reached after residency is established, `remove` retains `order`), so the eviction `while` loop cannot spin. `Outline::push` records the line captured *before* the flow, so a wrapped heading is anchored at its first line and `lines` stays strictly increasing. |
| Security | PASS | Heading text is attacker-influenced input in a viewer that opens arbitrary markdown. `heading_text` (`block.rs:456-479`) is explicitly iterative over an explicit work stack, so `***…***` nested arbitrarily deep costs no stack depth — traced. On the way out, `overlay_body` calls `sanitize` **then** `clip_to_width` before writing, so an escape sequence embedded in a heading cannot reach the terminal and a CJK title cannot overflow its row and wrap-scroll the alternate screen. |

## Loaded-Skill Criteria

| Skill | Criterion | Status | Evidence |
|-------|-----------|--------|----------|
| aposd-designing-deep-modules | Information hiding — data structures and algorithms stay internal | PASS | `RasterCache` owns LRU order, byte accounting and the pinning rule; `insert` returns evicted `ImageId`s rather than emitting escapes, keeping the wire entirely in the sink. Neither module can see the other's bound — grepped: no production expression compares `CAP` to a raster count. |
| aposd-designing-deep-modules | No information leakage — the same knowledge in two modules | PASS (one minor case, Note 6) | The residency/visibility split removes the leak the phase targeted (`placed` vs `RasterCache`, no shared map). `Outline` publishes `entries` while keeping `lines` private, which leaves callers holding the index-alignment rule — the one remaining spot, and it is documented. |
| aposd-designing-deep-modules | Single-use method / interface not widened past need | FAIL-adjacent, reported as Note 3 | `RasterCache::resident_nodes()` has exactly one caller, the test-only pre-change simulation; `bytes_resident`/`len` reach production only through `#[doc(hidden)]` accessors. 3 of 8 methods are test-driven. Not demonstrable as a defect — it is a matter of degree, so it lands in Notes per the demonstration bar. |
| aposd-designing-deep-modules | Silent failure — failures must be observable | PASS | `jump_heading` and `open_toc` both surface a status message rather than swallowing the key; `place_box`'s silent early return is protected by the invariant that residency always precedes placement, which the wire replay asserts. |
| aposd-designing-deep-modules | Temporal decomposition / shallow module | PASS | `Outline` and `RasterCache` are organised by knowledge (headings; who holds pixels), not by execution stage. `Outline` hides the parallel `lines` vector behind four intent-named queries whose distinction (`previous_before` is a motion, `index_at_or_before` is a containment query) is exactly the kind of thing a shallow interface would have leaked to callers. |
| performance-optimization | Measure before optimizing | PASS | The residency change is backed by an in-suite benchmark I independently reproduced (8.125 µs vs 5.091 s). The `Cargo.toml` dev-profile change is justified with recorded timings (Note 7 on their reproducibility). |
| performance-optimization | Prefer the fundamental fix (Stage 2: add a cache) over code tuning | PASS | The fix is architectural — a byte-budgeted raster cache plus unplace/re-place — not a micro-tune of the old sweep. |
| performance-optimization | Measure the change; revert what does not pay | PASS | The chartered two-stage downscale was measured at the real target and declined; I reproduced the measurement and confirmed no partial implementation was left in `decode.rs`. This is the "trading maintainability for <10% gain" red flag correctly refused. |
| performance-optimization | Validate no regression | PASS | Full suite green (432/432); the cap, residency, ladder and crop invariants are all still pinned by tests, several strengthened. |
| performance-optimization | Optimization did not degrade structure | PASS | The change *improved* structure — it split one overloaded `HashMap<NodeId, Placement>` into two modules with disjoint responsibilities. |

## Notes (non-blocking)

1. **The DW-3.5 fixture leaks ~137 MB of temp per test-binary run, and its cache comment is false.** Confidence: high (observed). Severity: low-medium. `write_huge_png` (`sink.rs:975-1003`) documents itself as "reusing one already on disk from an earlier run", but `scratch_dir` embeds `std::process::id()`, so the `metadata(...).is_ok_and(...)` early return can never fire across runs and the 6000×6000 PNG is re-encoded every time. `du -sh $TMPDIR/stele-sink-test-scroll-back-bench-*` shows ten distinct pid-tagged directories at 137 MB each (≈1.4 GB) from the handful of runs done for this review, none cleaned up. Pid-tagged scratch dirs are the repo's existing convention and every other fixture is a few hundred bytes, so this is new only in scale. Cheapest fix: drop the pid from *this* fixture's directory so the reuse branch actually works, or delete the file at the end of the test. This is also why the `stele` lib test binary now takes ~17 s (of which ~10 s is fixture encoding and ~5 s is the baseline arm).

2. **The pre-change simulation ships in the production binary.** Confidence: high. Severity: low. `pre_change_residency` (`sink.rs:192`), `simulate_pre_change_residency_for_test` (`:284`) and `evict_everything_absent_last_frame` (`:352`) are ordinary (non-`#[cfg(test)]`) code, and the setter is `#[doc(hidden)] pub` on a public type — so an external caller can reinstate the known-broken residency window in a shipped build. It follows the precedent set by `force_ratex_failure_for_test` / `force_txm_failure_for_test`, and it is what makes DW-3.5's baseline honest rather than a re-description, so the trade is defensible. Worth revisiting if a `cfg(feature = "test-hooks")` gate is ever added for the existing hooks.

3. **Three of `RasterCache`'s eight methods exist only for tests.** Confidence: high. Severity: low. `resident_nodes()` has one caller (Note 2's simulation); `bytes_resident()` and `len()` reach production only via the `#[doc(hidden)]` `resident_bytes`/`resident_count` accessors on the sink. The production path uses `new`/`get`/`peek`/`holds_id`/`insert`/`remove`. Not a defect, but the module is a little wider than its job.

4. **`+`, `-` and `T` are still processed while the TOC is open.** Confidence: high (read `main.rs:222-227` and `:297-321`). Severity: low. `handle_chrome_key` runs before `AppState::handle_key_event` and does not consult `state.mode()`, so `+`/`-` in TOC mode relayout the document at a new width and move `scroll`, while `toc_return_scroll` still holds the line index captured against the *old* tree. `Esc` then restores a line number measured in a tree that no longer exists. DW-3.2 is arguably still satisfied ("returns to the position it was opened from"), no test covers it, and I did not demonstrate a user-visible wrong result — so this is a Note, not a finding. Gating `handle_chrome_key` on `Mode::Normal`, or recapturing `toc_return_scroll` on relayout, would close it.

5. **`decode.rs`'s "6% end to end" understates the two-stage delta.** Confidence: high (measured). Severity: low. My run shows 451 → 415 ms at the 2400×2400 target, which is 8%, and with the 270 ms decode (identical either way) excluded, the resize stage alone goes 181 → 145 ms, a 20% improvement. The *decision* still stands — the resize is on the cache-miss path only, and a hand-rolled halving loop is not worth 36 ms of a frame that already costs 450 — but the recorded framing picks the number that most flatters the conclusion. Worth restating as "8% end to end, 20% of the resize stage" so a future reviver of the idea sees the real figure.

6. **`Outline` publishes `entries` but hides `lines`.** Confidence: high. Severity: low. Callers must know `entries[i]` pairs with `line_of(i)` — the doc says so, and every current caller obeys it, but an `iter()` yielding `(&OutlineEntry, usize)` or an `entry(i)` accessor would make the pairing unable to come apart rather than merely documented as unable to.

7. **The `Cargo.toml` dev-profile timings are not reproducible from a committed artifact.** Confidence: medium. Severity: low. The comment cites "11 s decoding and rescaling the 6000x6000 fixture" and "~16 s on `gfx`'s own decode tests" as the before figures, but unlike the two-stage decision there is no committed probe to re-run. The change itself is sound and low-risk (dependency-only, dev profile only, our crates stay unoptimized and debuggable).

8. **`with_raster_budget` discards a populated cache without emitting deletes.** Confidence: high. Severity: low (latent). `sink.rs:226-229` replaces `self.cache` wholesale. It is a `mut self` builder called before any paint in every current use, so it is unreachable today; if it ever moves to a `&mut self` setter (e.g. a runtime budget knob), it would orphan every resident raster in the terminal.

**Verdict: PASS**
