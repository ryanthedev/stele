# Review: Phase 1 - stele viewer features

## Executed Results (Step 0)
- Test suite: `cargo test --workspace` → all green. Every crate's suite passed, including the width/stele suites directly relevant to this phase: `status_row.rs` (3 passed), `theme_toggle.rs` (2 passed), `panic_mid_frame.rs` (1 passed, real pty subprocess), `scroll_boundaries.rs` (1 passed), `app.rs` unit tests (all passed, embedded in the `stele` lib test binary run), `width` crate's `engine::tests` (13 passed) and `corpus_agreement.rs` (2 passed). No failures anywhere in the workspace.
- Typecheck: implicit in `cargo test --workspace` (builds all targets); no errors.
- Clippy: `cargo clippy --workspace --all-targets` → `Finished` with zero warnings.
- Fmt: `cargo fmt --all -- --check` → no output, clean.

## Requirement Fulfillment

### DW-1.1
PREMISE:  A rendered frame reserves exactly one status row; content height is `rows - 1` and no content line is overpainted.
EVIDENCE: `crates/stele/src/main.rs:96-99` (`size.height = rows.saturating_sub(1)`); `crates/stele/src/painter.rs:158-206` (`frame_with_status`/`paint_status_row`, row `= size.height + 1`, 1-indexed — one past the content rows `0..size.height`); `crates/stele/tests/status_row.rs:48-114`.
TRACE:    30-line fenced doc, `rows=10` ⇒ `content_size.height=9`. `frame_body` paints tree lines 0..8 into terminal rows 1..9; `paint_status_row` writes at row `9+1=10`. Test asserts row 9 (1-indexed) still shows `"line 8"` and row 10 shows the ruler (`doc.md` + `%`) and does **not** contain `"line 9"` (the line that would have landed there had the reservation not happened). Ran and passed.
VERDICT:  PASS

### DW-1.2
PREMISE:  The status row shows scroll position as a percentage that reads 0% at the top and 100% at `max_scroll`.
EVIDENCE: `crates/stele/src/app.rs:166-177` (`position_pct`); tests at `app.rs:1151-1172`.
TRACE:    50-line doc, viewport 10 rows ⇒ `max_scroll > 0`. At `scroll=0`, `position_pct() = 0`. After `G` (jump to `max_scroll`), `position_pct() = 100`. Dirty case: a document that fits the viewport (`max_scroll==0`) reads `100`, not `0` — deliberately, since the reader has already seen the whole document. Both tests ran and passed.
VERDICT:  PASS

### DW-1.3
PREMISE:  `Ctrl-G` shows file name, byte size, and line count; the message clears after a bounded number of frames.
EVIDENCE: `crates/stele/src/app.rs:185-193` (`show_file_info`), `:73` (`STATUS_MESSAGE_TTL_FRAMES = 100`), `:204-219` (`status`); tests at `app.rs:1175-1229`.
TRACE:    `build("line one\nline two\nline three\n", ...)`, `ctrl('g')` ⇒ `show_file_info` sets a message containing `"test.md"`, `"30 bytes"` (source len), `"3 lines"`. Zero-byte and no-trailing-newline dirty cases both pass (`str::lines` semantics, not `\n`-count). TTL test calls `status()` 100 times, message present each time, then absent on the 101st. Traced arithmetic: `set_status` seeds `ttl=100`; each `status()` call decrements while `ttl>1` and returns `Some` regardless, so it is visible for exactly 100 calls and gone on the 101st — matches the test. All ran and passed.
VERDICT:  PASS

### DW-1.4
PREMISE:  `+`/`-` change content width within `LayoutConfig`'s clamp and the top visible block stays the top visible block.
EVIDENCE: `crates/stele/src/app.rs:224-248` (`widen`/`narrow`/`adjust_width`), `:260-264` (`relayout_preserving_anchor`); tests at `app.rs:1232-1352`.
TRACE:    60-paragraph doc scrolled 30 lines in; `widen`/`narrow` each preserve `block_at(scroll)` identity (anchor-based relayout, not proportional). Clamp-boundary dirty case: 50 `widen()` calls from width 40 land exactly at `config.max_width` (not beyond), and one subsequent `narrow()` moves immediately to `max_width - WIDTH_STEP` (not requiring several presses to "unstick"). Symmetric for `min_width`. All four tests ran and passed.
VERDICT:  PASS

### DW-1.5
PREMISE:  `T` swaps theme variant; every heading level still clears WCAG AA in the new variant.
EVIDENCE: `crates/stele/src/main.rs:293-300` (`T` handler), `:309-314` (`toggled_variant`, exhaustive match, no wildcard); `crates/stele/tests/theme_toggle.rs`.
TRACE:    `paint_heading(Dark)` then `paint_heading(Light)`; `fg_before` parses the real `38;2;r;g;b` SGR the painter emitted immediately before `"Heading One"` on the wire. Dark vs. Light give different colors (test 1). Both variants' extracted H1 foreground clears the WCAG AA 4.5:1 ratio against their reference background (test 2). Both ran and passed. See **Notes** — the second test's name claims "every heading level" but only exercises H1 (heading tier 0, per `crates/highlight/src/theme.rs:354-357` "levels 1–2 the loudest"); tiers 1 and 2 (levels 3–6) are not exercised through painted bytes in this phase's test, though `crates/highlight`'s own pre-existing, already-passing tests (`test_heading_tiers_clear_wcag_aa_against_the_reference_backgrounds`, `test_heading_ramp_is_monotone_in_contrast`) do cover all six levels at the `Theme::resolve` level, and the wire-level swap-actually-happens claim only needs one level to be proven. Not a demonstrated defect — treated as a Note, not a FAIL.
VERDICT:  PASS

### DW-1.6
PREMISE:  stdout is wrapped in a `BufWriter`; a forced panic mid-frame still restores the terminal with no buffered bytes emitted afterward.
EVIDENCE: `crates/stele/src/main.rs:172-182` (`BufWriter::new(PanicGuardedWriter::new(...))`); `crates/stele/src/terminal.rs:688-738` (`PanicGuardedWriter`, `frame_poison_flag`), `:745-752` (`install_panic_hook` poisons before restoring); `crates/stele/tests/panic_mid_frame.rs`.
TRACE:    Real subprocess, `STELE_TEST_PANIC_AFTER_BYTES=80` injects a panic inside `PanicAfterBytes::write`, which sits *outside* the `BufWriter` in the writer stack (`main.rs:176-182`) — so the panic fires before that write ever reaches the `BufWriter`'s internal buffer, while earlier frame bytes are already sitting there unflushed. Panic hook poisons `FRAME_POISONED` first, then writes `RESTORE_SEQUENCE` directly to real stdout (bypassing the `BufWriter`/`PanicGuardedWriter` chain entirely). Unwinding then drops `out`; `BufWriter::drop`'s best-effort flush calls `PanicGuardedWriter::write`, which is now poisoned and discards silently. Pty assertion: no `0x1b` byte appears in the wire after the restore sequence. Ran (real pty subprocess) and passed. Unit-level isolation test (`test_dw_1_6_panic_guarded_writer_passes_bytes_through_until_poisoned_then_discards`) also ran and passed.
VERDICT:  PASS

### DW-1.7
PREMISE:  `display_width` returns identical results to the pre-change path for every entry in the pinned Ghostty corpus, and a committed benchmark shows the ASCII path at least 2× faster than the pre-change baseline on ASCII-only input recorded in the same harness.
EVIDENCE: `crates/width/src/engine.rs:84-98` (`display_width`/`display_width_via_graphemes`, the latter named as "the exact pre-change computation"), `:111-113` (`is_printable_ascii`); tests at `engine.rs:160-324`.
TRACE:    Pure-ASCII fixtures (including a 5,000-byte string) agree with `s.len()` and with `display_width_via_graphemes` exactly (equivalence, not "looks right"). Dirty fixtures (tab, DEL, combining mark, CJK, flag pair) are proven to decline the fast path (`is_printable_ascii` asserted `false` first) and still agree with the slow path; the tab case's literal number (`"a\tb" ⇒ 2`, not 3) is spelled out to catch a fast path that wrongly counted control bytes as width 1. The pinned 200+-entry Ghostty corpus is exercised padded with ASCII (`"ok {cluster} yo"`), so the *whole string* is guaranteed non-ASCII and correctly declines the fast path (`declined_fast_path > 100` asserted) while the *dispatch* through `display_width`'s public entry point is what is checked, not a hand-picked fixture. The speed test measures `display_width` vs. `display_width_via_graphemes` wall-clock over 2,000 iterations of a 9,200-byte ASCII string and asserts `fast * 2 <= slow`; this is the "committed benchmark…recorded in the same harness" the requirement names (a `#[test]`, not a `criterion` bench — matching the requirement's own wording, and there is no `benches/` directory or `[[bench]]` target in `crates/width/Cargo.toml`, so this is the only artifact answering the requirement). All four tests ran and passed.
VERDICT:  PASS

**All requirements met:** YES

## Test-DW Coverage
- [x] DW-1.1 through DW-1.7 each have at least one test whose name is tagged `dw_1_N`, all ran in Step 0, all passed. Full list of DW-tagged test names captured by `grep -rn "dw_1_[1-7]"` across `src/` and `tests/`: 21 tests, one or more per DW item.
- [x] Test coverage matches the stated 100% level for these seven items — every item has automated-test evidence, no item relies on "recorded observed behavior" as a substitute.
- Gap noted (not a coverage FAIL): DW-1.5's second test name overstates what it checks (see Notes); the underlying claim is still covered by evidence, just split across two test suites (`stele`'s wire-level test for one heading tier, `highlight`'s pre-existing test for all six).

## Dead Code
None found in the reviewed files. No unreachable code after early returns, no debug prints, no commented-out blocks, no unused imports (clippy confirms).

## Correctness Dimensions
| Dimension | Status | Evidence |
|-----------|--------|----------|
| Concurrency | N/A | The viewer is single-threaded; the only concurrency-adjacent state touched by this phase is `FRAME_POISONED` (an `AtomicBool`), written once by the panic hook and read by `PanicGuardedWriter`, both on the same thread in the single-process, no-spawned-threads architecture. The one genuinely concurrent surface (the `SIGTERM`/`SIGHUP`/`SIGINT` handler in `terminal::signals`) is pre-existing, unchanged by this phase's diff, and its own tests still pass. |
| Error Handling | PASS | `paint_status_row`/`frame_with_status` propagate every `io::Result` via `?`, and `frame_with_status` still runs `SYNC_END` + flush on the error path (`painter.rs:170-172`, `and_then`/`.and(closed)` pattern predates this phase but the new `paint_status_row` call is folded into the same chain — traced: a write failure inside `paint_status_row` short-circuits `painted`, but `SYNC_END` is still attempted and its own result is combined via `.and(closed)`, so the sync block is never left open). |
| Resources | PASS | No new file handles, sockets, or locks introduced by this phase. `PanicGuardedWriter` borrows a `&'static AtomicBool` (no allocation); `BufWriter` wraps stdout exactly once in `main.rs`. |
| Boundaries | PASS | Traced `paint_status_row`'s `u32::from(size.height) + 1` explicitly to rule out a `u16` wraparound at `size.height == u16::MAX` (the doc comment states the reason; the arithmetic is genuinely widened to `u32` before the add, so no wrap occurs). Traced `position_pct`'s `scroll as u64 * 100` for a document with billions of lines: theoretically overflows `u64` only past ~1.8×10^17 lines, unreachable for a markdown viewer — not a realistic defect. Traced the 1-row/2-row terminal edge cases in `status_row.rs` against `main.rs`'s `rows.saturating_sub(1)` derivation — both degrade to painting only the status row, no panic. |
| Security | N/A | No new untrusted-input surface in this phase; `sanitize`/`clip_to_width` are pre-existing and unchanged, and still exercised by their own passing tests. |

## Edge Cases

| Edge case | Status | Evidence |
|---|---|---|
| Terminal one or two rows tall | PASS | `status_row.rs`'s two dedicated tests (`content_size.height=0` and `=1`) directly model `main.rs`'s `rows=1`/`rows=2` derivation; both assert the status row still paints (`"0%"`, or content row 1 + status row 2) with no panic. Ran and passed. |
| Width toggle at the clamp boundary | PASS | `app.rs`'s two clamp-boundary tests (`widen` saturating at `max_width` then `narrow` moving immediately off it, and the symmetric `min_width` case). Ran and passed. |
| Theme toggle while an image is placed (placements must survive or be re-placed) | PASS (indirect) | `app.rs:1362-1411`'s `test_theme_toggle_style_relayout_preserves_reserved_box_node_identity` proves the exact mechanism the media sink (`crates/stele/src/media/sink.rs`, out of this phase's file scope) depends on: a same-width relayout — precisely what `T`'s handler triggers via `relayout_preserving_anchor` — produces an identical `NodeId` for the reserved image box. Since `GfxMediaSink::placements` is keyed by `NodeId` and `replace_if_cached` reuses a resident raster whenever the target render size is unchanged (`sink.rs:583-601`, already covered by that file's own passing tests), stable `NodeId` + unchanged target size ⇒ a theme toggle re-places rather than re-transmits. No test in this phase's own suite exercises `GfxMediaSink` end-to-end across a real `T` press (that would need a file outside this phase's scope), so the coverage is architectural + a load-bearing unit test of the one new invariant, not a full integration proof. Judged sufficient given `sink.rs` was not part of this phase's write scope and its own placement-survival tests already pass. |
| A panic between `BufWriter` fill and flush must not leave a half-frame after the terminal restore | PASS | `panic_mid_frame.rs`, traced above under DW-1.6. Real pty subprocess, asserts zero `0x1b` bytes after the restore sequence. Ran and passed. |

## Loaded-Skill Criteria

| Skill | Criterion | Status | Evidence |
|-------|-----------|--------|----------|
| cc-routine-and-class-design | Parameter count ≤7 (graduated) across every new/changed routine | PASS | Highest count found: `Painter::frame_with_status(&mut self, tree, scroll, size, status, out)` — 5 explicit + `self` = 6, within the 6–7 "minor concern" band, no redesign needed. `paint_status_row`, `handle_chrome_key`, `adjust_width` all ≤4. |
| cc-routine-and-class-design | LSP / inheritance-vs-containment check | N/A | No new inheritance or trait-object substitution introduced by this phase; `LayoutContext` is a plain data-bundle (containment), not an inheritance hierarchy. |
| cc-routine-and-class-design | Cohesion classification of new/changed routines | PASS | `widen`/`narrow`/`adjust_width`, `paint_status_row`, `handle_chrome_key`, `show_file_info` are each one operation at their declared abstraction level (functional cohesion). `handle_control_chord`/`handle_key` are pre-existing dispatch tables, unchanged in shape by this phase beyond adding the `'g'` chord — still a defensible dispatch-table cohesion, as the existing doc comments already argue. |
| code-clarity-and-docs | Interface comments pass the "different words" test for every new public entity | PASS | Spot-checked `StatusLine`, `FileInfo`, `PanicGuardedWriter`, `frame_with_status`, `widen`/`narrow` — every one explains rationale/units/invariants in words distinct from the signature, not a restatement. |
| code-clarity-and-docs | Test names are sentence-style and accurately describe what the test checks | PLAUSIBLE (1 finding) | `theme_toggle.rs`'s `test_dw_1_5_every_heading_level_clears_wcag_aa_in_the_painted_bytes_of_both_variants` only exercises heading level 1 (tier 0) in both variants — the name's "every heading level" overstates its own body. Not a FAIL: the broader claim is true and covered elsewhere (pre-existing `highlight` crate tests, already passing), so this is a naming-accuracy Note rather than a demonstrated coverage gap for DW-1.5 as a whole. |
| code-clarity-and-docs | No stale/misleading comments, no dead TODOs, no commented-out code | PASS | None found in the reviewed files. |

## Notes (non-blocking)

- **Misleading test name (low severity, high confidence):** `crates/stele/tests/theme_toggle.rs`'s `test_dw_1_5_every_heading_level_clears_wcag_aa_in_the_painted_bytes_of_both_variants` tests only "Heading One" (H1) in both variants, not H2 through H6. Because `heading_tier` (`crates/highlight/src/theme.rs:356`) groups levels 1–2 into the same tier/color, H1 is representative of H2's color specifically, but tiers 1 and 2 (levels 3–6) are never exercised through painted bytes in this phase's suite — only through `crates/highlight`'s own pre-existing tests, which check `Theme::resolve` directly rather than the wire. Suggest either renaming the test to something like `test_dw_1_5_heading_one_clears_wcag_aa_in_the_painted_bytes_of_both_variants`, or looping over one representative level per tier (1, 3, 5) the way `crates/highlight`'s own tests already do, so the wire-level test is self-contained proof of the DW-1.5 claim rather than relying on a second crate's test to complete it.
- **Indirect edge-case coverage (low severity, medium confidence):** the "theme toggle while an image is placed" edge case is proven at the `NodeId`-stability layer (`app.rs`) rather than end-to-end through `GfxMediaSink`. This is a reasonable scope call — `crates/stele/src/media/sink.rs` was not part of this phase's file list — but a future regression in `replace_if_cached`'s target-size comparison, interacting specifically with a theme-only relayout, would not be caught by either this phase's tests or `sink.rs`'s own (which test scroll/eviction scenarios, not theme toggles). Worth a follow-up integration test in a later phase that owns `media/sink.rs`.
- **DW-1.7 benchmark form (informational, not a defect):** the "committed benchmark" is a `#[test]` under `cfg(test)`, not a `criterion`/`[[bench]]` artifact. This matches the requirement's own phrasing ("recorded in the same harness") and the crate has no `benches/` directory, so nothing is missing — flagged only so the reviewer's reasoning is visible, not as a gap.
- Every workspace convention checked (per-crate `#![forbid(unsafe_code)]` except `stele`'s single documented `#[deny(unsafe_code)]` + one commented `#[allow]` in `terminal::signals`; no `thiserror`; sentence-style test names throughout; DW-tagged tests; no `Run.width` assertions in any reviewed test) held with no exceptions found.

## Issues (if FAIL)
None — no blockers found.

**Verdict: PASS**
