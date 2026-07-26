# Discovery + Design: Phase 4 — Incremental search

## Files Found

| File | Lines | Relevance |
|---|---|---|
| `crates/stele/src/app.rs` | 1453 | `AppState`, `handle_key_event`/`handle_control_chord`, `StatusLine`/`set_status`, `relayout`/`relayout_preserving_anchor` (Phase 1 seams) |
| `crates/stele/src/painter.rs` | 951 | `frame` → `frame_with_status` → `frame_body` → `paint_items` → `paint_run`; `paint_run:418` is the per-frame `Decor::highlight` call |
| `crates/stele/src/decor/mod.rs` | 270 | `Decor` trait, `StructuralDecor`, `structural_style` (exhaustive over `Semantic`) |
| `crates/stele/src/decor/themed.rs` | 156 | `ThemedDecor` — the bridge to `crates/highlight` |
| `crates/highlight/src/highlighter.rs` | 228 | `highlight_line`, `highlight_with_timeout` (250 ms cap at :39, fallback at :64-67) |
| `crates/highlight/src/theme.rs` | 882 | `SEMANTIC_ROLES`, `semantic_attrs`, `semantic_role_index`, `build_palette` 256-distinctness invariant |
| `crates/layout/src/lib.rs` | 369 | `Semantic`, `StyleId`, `Run`, `Line`, `LayoutTree` (`block_at`, `first_line_of`, `lines`) |
| `crates/stele/src/main.rs` | 368 | `run_session`'s two `frame_with_status` call sites |
| `crates/width/src/engine.rs` | 326 | Phase 1's committed-benchmark pattern (`test_dw_1_7_..._at_least_2x_faster`) — the template DW-4.7 follows |

## Current State

Phase 1 landed everything Phase 4 was told to build on: a reserved status row (`Painter::frame_with_status`, content height `rows - 1`), `StatusLine { position_pct, name, message }` with a frame-count TTL, and `relayout_preserving_anchor`. `handle_key_event` dispatches Control chords first with fallthrough, then unmodified codes. `/`, `n`, `N` are all unbound today. No `Mode` enum exists yet — this phase introduces it.

`paint_run` calls `self.decor.highlight(&run.text, run.aux.as_deref())` for every `Semantic::CodeBlock` run on every frame; `ThemedDecor` forwards straight to `highlight_line`, which runs tree-sitter. Nothing is memoized anywhere in the paint path.

## Gaps

| # | Gap | Resolution |
|---|---|---|
| 1 | **File scope omits `crates/layout/src/lib.rs`**, but `Semantic` is defined there and the `Produces` contract requires `Semantic::SearchMatch`/`SearchCurrent`. | Add the two variants there. Not a scope change — it is the only place the contract can be honoured, and the project conventions already anticipate it ("adding a `Semantic` variant means every match arm in `theme.rs` and `decor/mod.rs` must handle it"). Recorded, not silently absorbed. |
| 2 | **File scope omits `crates/stele/src/main.rs`.** Key handling reaches the binary for free (`run_session` already calls `state.handle_key_event`), but match *highlighting* does not — the painter needs the overlay. | Two-line change: swap the two `frame_with_status` calls for `frame_with_search(..., state.search_overlay(), ...)`. Phase 1's file scope did include `main.rs`; Phase 4's omission reads as an oversight, since a keystroke feature that never reaches the event loop is inert. |
| 3 | `Decor::highlight` returns `Vec<Run>` with no way to say "this result came from the timeout fallback". The constraint forbids caching exactly that result. | Widen the seam: `Decor::highlight` returns `highlight::Highlighted { runs, cacheable }`. Two impls + test spies to update. |
| 4 | `Match` addressing must survive a wrap boundary, but `LayoutTree` exposes lines, not block text. | Match over a per-block join of the laid-out line texts (see Design 2). |
| 5 | No benchmark harness crate exists (no `criterion`, no `benches/`). | Follow Phase 1's precedent exactly: an in-suite A/B against a preserved baseline path, ratio-asserted. |
| 6 | `theme.rs`'s `all_semantics()` test helper is a hand-maintained list of every `Semantic`, which the exhaustive-match discipline cannot police — a new variant compiles fine and silently goes untested. | Added both new variants to it. Worth flagging for later phases: the codebase's "no wildcard arms" rule protects the style tables but not this list, and DW-7.2's palette test fails confusingly (a set inequality dump) rather than clearly when it drifts. |

## Code Standards

Applied from `docs/code-standards.md`: no `unsafe`; no wildcard arms on `Semantic` (three tables to extend — `layout` has none, `theme.rs::semantic_attrs`, `theme.rs::semantic_role_index`, `decor/mod.rs::structural_style`); no `as u16` on a computed width; never assert on `Run.width`; hand-rolled error enums (none needed here — every new path returns `Option`/degrades); sentence-style test names, DW-tagged where they trace a plan item; unit tests in `#[cfg(test)] mod tests` at the bottom of the file they test; imports in three groups.

## Test Infrastructure

Built-in `cargo test`. Unit tests live at the bottom of each source file — that is where every Phase 4 test goes, since each DW item is testable at exactly one crate boundary (`app.rs` for state, `painter.rs` for the wire, `theme.rs`/`decor/mod.rs` for roles, `highlighter.rs` for the cache). No pty test is needed: nothing here is terminal-reply-driven. `crates/stele/tests/common/` (pty + render harness) stays untouched.

## Assumption Verification (required before design)

**"Match positions survive relayout without full recomputation" — FALSE. Matches are recomputed on every relayout.**

`Match.line` is a `LayoutTree` line index and `Match.range` is a byte offset into that line's laid-out text. `relayout` replaces the tree wholesale: a width change rewraps paragraphs, so both the line index and the byte split-points move, and a `+`/`-` press or a terminal resize would leave every match pointing at text that is no longer there. Only `Match.block` (a `NodeId`) survives, and a block id alone cannot address a column range.

Per the dispatch instruction — correctness over the optimization — `AppState::relayout` re-runs the search against the new tree from the retained query. This is cheap relative to `layout()` itself (one literal scan of already-materialized text, no parse, no width measurement) and it is on the resize path, not the per-keystroke path. Pinned by `test_matches_are_recomputed_after_a_relayout_that_rewraps_them`, which asserts the recomputed range actually addresses the query text in the *new* tree.

## DW Verification

| DW-ID | Done-When Item | Status | Test Cases |
|-------|---------------|--------|------------|
| DW-4.1 | `/` opens a query prompt in the status row; typing updates it; `Esc` restores the pre-search scroll position | COVERED | `app.rs::test_dw_4_1_slash_opens_a_prompt_typing_updates_it_and_esc_restores_the_scroll_position`, `app.rs::test_dw_4_1_backspace_shortens_the_query_and_esc_from_an_empty_query_still_restores` |
| DW-4.2 | Case-insensitive for an all-lowercase query, case-sensitive once the query contains an uppercase character | COVERED | `app.rs::test_dw_4_2_lowercase_query_is_case_insensitive_and_an_uppercase_one_is_not`, `app.rs::test_dw_4_2_smart_case_reads_the_uppercase_flag_from_non_ascii_letters_too` |
| DW-4.3 | `n`/`N` cycle forward and backward through matches and wrap at both ends | COVERED | `app.rs::test_dw_4_3_n_and_shift_n_cycle_forward_and_backward_and_wrap_at_both_ends` |
| DW-4.4 | Every visible match is highlighted and the current match is visually distinct from the others | COVERED | `painter.rs::test_dw_4_4_every_visible_match_is_highlighted_and_the_current_one_is_distinct`, `painter.rs::test_dw_4_4_a_match_spanning_a_wrap_boundary_is_highlighted_on_both_lines`, `painter.rs::test_dw_4_4_a_search_highlight_wins_over_syntax_highlighting_in_a_code_block` |
| DW-4.5 | A query with no matches leaves the viewport unmoved and reports so in the status row | COVERED | `app.rs::test_dw_4_5_a_query_with_no_matches_leaves_the_viewport_unmoved_and_says_so` |
| DW-4.6 | Two adjacent frames over identical code-block content invoke the underlying syntax highlighter once, not twice | COVERED | `painter.rs::test_dw_4_6_two_frames_over_the_same_code_block_invoke_the_highlighter_once`, `highlighter.rs::test_dw_4_6_a_non_cacheable_result_is_recomputed_on_every_lookup` |
| DW-4.7 | A committed benchmark shows code-heavy frame time at least 10× lower than the pre-change baseline recorded in the same harness | COVERED (10× floor asserted in an optimized build, 3× unoptimized — see design decision 9) | `painter.rs::test_dw_4_7_cached_code_heavy_frames_are_at_least_10x_faster_than_the_uncached_baseline` |
| DW-4.8 | Both new style roles stay distinct from every existing role after 256-color downsampling | COVERED | `theme.rs::test_dw_4_8_search_roles_stay_distinct_from_every_other_role_after_256_downsample`, `decor/mod.rs::test_dw_4_8_search_roles_are_distinguishable_on_the_themeless_path_too` |

**All items COVERED:** YES (8 of 8, matching the 8 DW-IDs in the dispatch)

## Design Decisions

**1. `Mode` is a two-variant enum inside `AppState`; `handle_key_event` guard-clauses to the search table.**
Per the plan's chosen approach. `Mode::Search { origin: usize }` carries the pre-search scroll on the variant itself, so the "restore on `Esc`" datum cannot outlive the mode that gives it meaning. `handle_key_event` opens with `if let Mode::Search { .. } = self.mode { return self.handle_search_key(key) }` — a guard clause, keeping the existing chord/plain dispatch at nesting depth 1 and untouched. `Ctrl-C` still quits from inside search (raw mode clears `ISIG`; nothing else can); every other Control chord is ignored while typing.

**2. `Match.range` is a byte range measured from the start of `Match.line`, running forward through the block's laid-out text.**
The constraint ("addressable by (line, column range)", "a match spanning a wrap boundary highlights on both lines") and the `Produces` struct (one `line`, one `range` per `Match`) only reconcile one way: `range.start` is a byte offset inside `line`'s own text, and `range.end` may run past that line's length, meaning the match continues onto `line + 1`, `line + 2`, … The painter projects the range back onto lines by subtracting each line's text length. A match inside one line — the overwhelmingly common case — has `range` exactly equal to its line-local byte range.

*Byte offsets, not cell columns, are the storage.* Columns are what the reader sees, but bytes are what safely slice a `&str`, and the painter must split runs to restyle them. The cell column is derived at paint time by the width engine, which is already the only oracle for it. Storing columns would mean converting twice and re-deriving the byte offset anyway.

*Matching runs over the per-block join of laid-out line texts.* One reusable `String` buffer, cleared per block, plus a `(line_index, offset_in_join)` map. Known and accepted limit: greedy wrapping consumes the space it breaks at, so `"hello world"` wrapped across two lines joins as `"helloworld"` — a query for the two-word phrase does not match across that break, while a query for `"lowo"` does. That is what "matching runs over the laid-out line text" means, taken literally; anything else would be matching over source text, which is not addressable by (line, column).

**3. Smart case is per-`char`, not via `to_lowercase()` on the haystack.**
Case-insensitivity is decided once per query (`query.chars().any(char::is_uppercase)`) and applied by comparing haystack and needle one `char` at a time, folding each through `char::to_lowercase()`. Lowercasing the haystack first would be simpler and wrong: `to_lowercase` is not length-preserving (U+0130 lowercases to two chars), so every byte offset after such a character would be off, and the highlight would land on the wrong text. Per-char comparison keeps haystack offsets exact by construction. The cost is no full Unicode case folding — `İ` will not match `i̇` — which is the documented, literal-matching scope.

**4. The highlight cache lives in `Painter`, keyed by `(line text, lang)`; cacheability flows out of `crates/highlight`.**
Measure-first placement: the profiled cost is `paint_run` re-invoking `Decor::highlight` per code line per frame (plan's own file hint, `painter.rs:361`), so the memo goes at that call site, where it covers every `Decor` impl rather than one. But only `crates/highlight` can tell a real highlight from the 250 ms timeout fallback, and the constraint forbids caching the latter — so `Decor::highlight` now returns `highlight::Highlighted { runs, cacheable }` and the painter retains only `cacheable` results. `HighlightCache` itself is a bounded FIFO map (`HashMap` + `VecDeque`, capacity 4096 lines — several screens of scrollback, and bounded as the constraint requires) holding `Rc<[Run]>`, so a hit is a refcount bump rather than a deep clone of the runs.

Conservatively, *every* path that reaches the plain-run fallback is marked non-cacheable, not just the timeout: an inline highlighter error is equally a "this attempt failed" answer rather than "this is how the line highlights". Superset of what the constraint demands, and simpler than distinguishing them.

`register_decor` clears the cache. Highlight *runs* are theme-independent (the theme only enters at `resolve`), so `T` does not strictly need it — but a decor swap is exactly the event that would invalidate a memo, and paying one `clear()` per keypress to keep that obvious is free.

**5. The two new roles get palette slots, so DW-4.8 holds by construction.**
`SEMANTIC_ROLES` 22 → 24, with `SearchMatch` at slot 22 and `SearchCurrent` at 23. `build_palette`'s greedy generator already refuses any candidate whose 256-downsampled value collides with an accepted one, so distinctness is enforced at construction rather than asserted after the fact — DW-4.8's test then verifies the property through the public `resolve` entry point. Attributes: `SearchMatch` → underline, `SearchCurrent` → bold + underline, so the two stay distinguishable on the themeless `StructuralDecor` path (which has no color at all) as well as through the palette.

**6. `frame_with_search` is added beside `frame_with_status`, mirroring Phase 1's `frame` → `frame_with_status` split.**
`frame_with_status` becomes the no-overlay wrapper. Zero existing call sites change, and the search overlay is an explicit argument rather than hidden painter state, so `frame`'s "deterministic given its arguments" promise survives.

**7. The DW-4.7 baseline is a cache capacity of 0.**
`Painter::with_cache_capacity(engine, 0)` never retains anything, so its paint path is byte-for-byte the pre-change path — the same trick Phase 1 used with `display_width_via_graphemes` ("a named oracle both this equivalence and the committed benchmark check against"). Both arms run in one test, on one host, over one fixture.

**8. `write_sgr` stopped allocating — a second, measured fix DW-4.7 turned out to require.** *(Decided during implementation, not at design time.)*
With the cache in, the first honest measurement came in at 4.5×, not 10×. Rather than tune the fixture until the number went up, the remaining cost was profiled per helper on representative run text: `sanitize` 30 ns/run, `clip_to_width` 78 ns/run, **`write_sgr` 116 ns/run** — ~93 µs of a frame, the largest single item left once the parse was gone. It was building a `Vec<String>` of `format!`ed SGR codes and `join(";")`ing them, three or four heap allocations per styled span, ~800 spans per code-heavy frame. Rewritten to assemble the parameters straight into the writer with a stack-buffered `u8`-to-decimal helper. Byte-identical output — the existing golden-SGR tests pin exact escape sequences and all 146 passed unchanged — and it raises the ratio for the arithmetic reason that matters: the ratio is `(parse + paint) / paint`, so cutting paint cost raises it while making every frame genuinely faster, search active or not.

**9. The 10× floor is asserted against an optimized build; an unoptimized one asserts 3×.** *(Decided during implementation; explanation corrected after review.)*
Both arms slow down without `-O`, but not at the same rate — the baseline by ~5.5×, the cached arm by ~15× — and dividing one by the other is what compresses 12× into 4×. The optimized number is the one that describes the binary readers run, and the 64 µs budget the original audit set is unreachable in a debug build for reasons unrelated to this cache. So the full floor is asserted where it means something and the default `cargo test` run still asserts a real 3× floor rather than skipping — a regression that loses the cache outright scores ~1× and fails in both profiles.

An earlier version of this note and of the test's doc comment explained the gap by claiming the removed cost was profile-*independent* ("optimized C in every profile"). Its own numbers refute that: a profile-independent parse could not have slowed 5.5×. Right conclusion, wrong mechanism; corrected in both places rather than left for a future reader to build on.

**Known limit of the gate:** nothing in the repo runs the release arm, so a *partial* regression — something recovering only 6× optimized — would pass `cargo test --workspace` silently. The debug floor catches a lost cache, not a degraded one. Gating the headline claim needs a release run in CI.

## Prerequisites

- [x] Phase 1 shipped (`relayout_preserving_anchor`, reserved status row, `StatusLine`) — verified in the worktree at `7187b5f`
- [x] `n`, `N`, `/` are unbound in the existing key tables — no binding is displaced
- [x] `Decor` has exactly two impls, both in file scope, so widening its return type is contained
- [x] `layout::Semantic`'s three consumer tables are all reachable and already exhaustive

## Recommendation

**BUILD.** Two file-scope additions are required and recorded above (`crates/layout/src/lib.rs` for the `Semantic` variants the `Produces` contract names; `crates/stele/src/main.rs` for two lines of paint wiring). Neither adds scope — both are the minimum needed to deliver exactly what the phase already specifies.

---

## Post-Review Corrections

An independent review returned FAIL on one blocker plus three findings. All four are fixed; each is pinned by a test that fails without the fix.

| # | Finding | Fix | Gate |
|---|---|---|---|
| **Blocker** | `project_match`'s `start >= length` staleness guard read `0 >= 0` on a zero-length line inside a block — a blank line in a code fence, the gap between loose-list items — and abandoned every fragment of the match past it. `fn foo() {` / blank / `}` searched for `{}` highlighted the brace and painted the closing one plain. | Take the empty-line case *before* the staleness guard: a zero-length line consumes no bytes, so the walk continues past it. The guard now only ever sees a genuinely out-of-range offset. | `test_dw_4_4_a_match_spanning_a_blank_line_inside_a_block_is_highlighted_in_full`, three fixtures (fence, fence with text either side, loose list), asserting **highlighted byte count equals query length** — a `contains` check passes on a partially highlighted match, which is exactly the defect. |
| 1 | Raising `SEMANTIC_ROLES` 22→24 shifted `capture_role_index = SEMANTIC_ROLES + pos`, silently repainting all 24 colored capture roles. Distinctness survives any permutation, so no existing test could see it. | Search roles moved past the capture block to a new `TRAILING_ROLE_BASE`; `SEMANTIC_ROLES` back to 22. Appending is inert — verified by dumping all 48 capture colors from the pre-phase commit and diffing against the current palette: **identical**. | `test_capture_colors_are_pinned_so_a_new_role_cannot_silently_restyle_code_blocks` (six pinned values, both variants, both ends of the block) and `test_the_capture_block_still_begins_immediately_after_the_semantic_roles`. |
| 2 | `StructuralDecor` gave `SearchMatch` bare `underline` — identical to `Link` and `FootnoteRef`. The themeless DW-4.8 test checked five roles and omitted exactly those. *Also found while fixing it:* `SearchCurrent` was `bold + underline`, identical to `Heading(1)`, and the same two collisions existed in `highlight::theme` under `NO_COLOR`. | Both search roles take attribute sets nothing else uses (`underline + italic`, `+ bold`), in **both** tables. | Both DW-4.8 attribute tests now iterate **every** `Semantic` variant. Each list carries a wildcard-free `_exhaustiveness_guard` match, so a new variant is a compile error until the list names it — verified by adding a variant and confirming both guards fail independently of the production tables. |
| 3 | The DW-4.7 comment's stated mechanism was contradicted by its own numbers. | Rewritten to report what is measured. See design decision 9. | — |

Two non-blocking traps from the review's notes were also closed: `HighlightCache`'s derived `Default` yielded `capacity: 0` (a silently disabled cache) and now delegates to the real capacity, and `visible_spans`'s `match` on a boolean is an `if`/`else`.

No existing test was weakened or deleted. Two were *strengthened*: the wrap-boundary test gained a byte-count assertion, and both DW-4.8 attribute tests went from a five-role sample to every role.


## Post-Review Corrections, Round 2

A second review returned FAIL on one blocker: `main.rs`'s chrome table (`+`, `-`, `T`) ran *before* `AppState::handle_key_event` with no mode guard, so those three keys were consumed as chrome while a query was open. Demonstrated against the shipped binary: `/The` searched for `he` and flipped the theme on the way through.

**The instance was one line. The cause was that the decision lived where nothing could test it.** `main.rs`'s own module doc says "all decision logic lives in the library … this file is thin glue over real crossterm I/O and is not itself unit-tested" — and `handle_chrome_key` held both the key table *and* its ordering, in violation of that. Two gates passed over the defect because every search test drove `AppState::handle_key_event`, the one entry point the bug was not on.

So the fix restores the stated architecture rather than patching the symptom:

| Layer | Change |
|---|---|
| `Mode::captures_all_keys()` | The question a mode answers about itself, in the file that owns `Mode`. Wildcard-free match, so Phase 3's `Mode::Toc` and Phase 6's `Mode::LinkSelect` each become a **compile error** here until their author states whether their mode owns the keyboard. |
| `AppState::chrome_action(key) -> Option<ChromeAction>` | The whole routing decision — mode guard first, then the chord guard, then the key table — moved into the library where a test can reach it. |
| `main.rs::handle_chrome_key` | Reduced to executing the returned `ChromeAction` against the `Painter` and `LayoutContext` that genuinely cannot move. |

This is why the fix is a seam change and not a guard: a guard in `main.rs` would work until the next mode forgets to add one, in a file that mode does not own. Asking the mode inverts the obligation, and the compiler enforces it.

**Coverage, at both levels the defect needed.**

| Test | Level | What it would have caught |
|---|---|---|
| `test_dw_4_1_no_printable_key_is_claimed_as_chrome_while_a_query_is_open` | unit | Every printable ASCII character, not the three that were wrong — the property is that a mode owning the keyboard owns all of it, so a fourth binding added later obeys it too. |
| `test_dw_4_1_the_chrome_keys_are_ordinary_characters_inside_a_query` | unit | Reproduces the event loop's real routing *order*, which is where the defect lived. |
| `test_the_chrome_keys_still_route_to_chrome_in_normal_mode` | unit | A "fix" that simply stopped routing `+`/`-`/`T` would pass everything else and silently delete DW-1.4 and DW-1.5. |
| `tests/search_key_routing.rs` (3 tests) | **real binary over a real pty** | The seam itself. Types `/T+-z` at the shipped binary and reads the status row off the wire; asserts the heading's painted foreground is unchanged (the theme did not flip); and asserts `T` *does* still swap the theme with no query open. |

Mutation-verified: removing the mode guard turns the pty status row into `"/z  [1/1]"` — the reviewer's exact symptom, all three keys stolen — while the normal-mode theme test still passes, confirming the guard is scoped rather than a blanket disable.

**Three harness hazards found while writing the pty test**, each documented at its call site because each cost real time and none is obvious:
- `common::pty::read_until` reads in 8 KiB blocks and routinely swallows the rest of the frame past its needle, so sequential waits must search the *accumulated* buffer from an explicit offset or they block on bytes already in hand.
- `Esc` and `q` written microseconds apart parse as a single `Alt+q` (a terminal encodes `Alt`-key as `ESC` key, and crossterm disambiguates by time), so the viewer never quit. The harness ends sessions with Ctrl-C, one unambiguous byte that quits from the prompt as well as from normal mode.
- A `try_wait` loop that stops draining the pty deadlocks: the child blocks writing into a full output buffer and so never reaches the read that would see the quit key. The harness drains while it waits.

The status row is read straight off the bytes rather than through `common::render::render_row`; that shared terminal model does not terminate on this session's startup wire, which is a pre-existing issue in a file outside this phase's scope and is left for its owner.


## Rebase onto Integration (Phase 1 + 2 + 3)

Rebased onto `c3b552f`. Three source files conflicted (`app.rs`, `main.rs`, `painter.rs`); the test module of `app.rs` was rebuilt from both parents rather than hunk-merged, because splicing interleaved test blocks by hand was producing silently malformed functions.

**The design collision, resolved rather than merged past.** Phase 3 hit the same key-stealing seam from the TOC side and fixed it with `state.mode() != Mode::Normal` in `handle_chrome_key`. That gate is correct and would have stayed correct until the next mode forgot to be added to it — in a file that mode does not own. Adopted this branch's `Mode::captures_all_keys()` and removed the ad-hoc gate, with `Mode::Toc { .. } => true` carrying Phase 3's rule unchanged: `+`/`-`/`T` stay inert under the overlay rather than relaying out a document the reader cannot see.

Verified by mutation, not by inspection: flipping `Mode::Toc` to `false` fails **Phase 3's own pty tests** — `test_dw_3_2_a_chrome_key_pressed_under_the_toc_does_not_reach_the_document` and `test_dw_3_2_a_chrome_key_read_during_a_resize_drain_does_not_reach_the_document` — as well as this branch's. Their guarantee is enforced by their assertions under the new design, which is the only way to claim the swap was safe.

**Routing parity across both call sites.** `main.rs` routes keys through `handle_chrome_key` in the main loop *and* inside Phase 2's resize drain; both now reach `AppState::chrome_action`. Phase 3's drain test is what proves it — it failed under the mutant above, so it genuinely exercises the new path rather than passing by coincidence.

**Search meets reload for the first time.** Two staleness bugs, one already covered and one not:

| Datum | Status | Resolution |
|---|---|---|
| `SearchState.matches` | Already correct | `reload_document` → `relayout_preserving_anchor` → `relayout` → `recompute_matches`. The mechanism built for resize covers reload for free. Now pinned by `test_a_reload_while_a_search_is_open_leaves_every_match_addressing_the_new_document`. |
| `Mode::Search { origin }` | **Was broken** | A raw line index into the replaced tree — `Esc` would drop the reader on a line that no longer exists (reload) or no longer means the same thing (reflow). This is exactly the datum Phase 3 re-seats for the TOC (`toc_return_scroll`), so `reseat_search` gives the same answer for the same reason. Skipped when nothing reflowed, so a theme swap does not throw away a good origin. |

The honest cost of that choice, stated in the code: resize or reload mid-query and `Esc` returns you to where the reflow left you rather than to the line you pressed `/` on. A defensible position beats a precisely wrong one, and it matches what the TOC already does.

This also closes review-attempt-1's Note 10, which flagged the resize half of the same problem and which I had acknowledged without fixing.

**Gates after rebase:** 535 passing in debug, 535 in release, 535 under `script -q /dev/null`, clippy clean, fmt clean. No test from any phase was weakened or deleted. One was *updated*: `tests/toc_overlay.rs`'s harness mirrors `main.rs::paint`, so it now calls `frame_with_search` for the document-showing modes exactly as `main.rs` does — keeping the mirror accurate, not relaxing its assertions (with no search active the overlay is empty and the bytes are identical).


## Benchmark Results (DW-4.7)

Recorded by `test_dw_4_7_cached_code_heavy_frames_are_at_least_10x_faster_than_the_uncached_baseline`, `crates/stele/src/painter.rs`. Fixture: 40 lines of real Rust in an 80-column viewport, `ThemedDecor` over the real lumis highlighter, 20 frames per arm after a warm-up frame.

| Build | Uncached baseline (`with_cache_capacity(0)` — the pre-change path) | Cached (shipped) | Ratio | Floor asserted |
|---|---|---|---|---|
| Optimized (`--release`) | 1 150–1 244 µs/frame | **94–97 µs/frame** | **12.2–12.9×** | 10× |
| Unoptimized (`cargo test`) | 6 407–6 811 µs/frame | 1 470–1 493 µs/frame | 4.3–4.6× | 3× |

Three runs per profile, after the review fixes; ranges rather than single figures because a single figure from a shared host is a number pretending to be a measurement.

The optimized cached frame lands at ~96 µs against the audit's 64 µs budget, from a ~1 200 µs baseline — the same order the audit measured (2 525 µs → 64 µs) on its own content. See design decisions 8 and 9 for why the floor is profile-dependent and for the second optimization the measurement forced.

**Fixture choice is load-bearing, and was itself measured.** The first fixture was synthetic filler (`let value_007 = 7 + 1;` × 40) and scored 3.6×. That is not a smaller cache benefit — it is a fixture that understates the cost being removed: tree-sitter's cost scales with how much syntax is on a line, while the painter's is capped by the viewport width, so one-expression-per-line filler is not "code-heavy" in the sense the audit meant. Measured side by side on the same host: 3.6× filler, 12.6× real code. The committed fixture is real Rust, and `realistic_code_fence`'s doc comment records why.

| Where the remaining cached frame went (per run, measured on representative token text) | Cost |
|---|---|
| `write_sgr` — `Vec<String>` + `format!` + `join` | 116 ns → **rewritten, zero-alloc** |
| `clip_to_width` — grapheme segmentation + `String` | 78 ns (left alone) |
| `sanitize` — `String` per run | 30 ns (left alone) |

Only the top item was changed. The other two are real costs and a plausible future target, but the measurement said they were not what stood between this phase and its done-when item, and optimizing unmeasured is how >50% of optimizations end up worthless.
