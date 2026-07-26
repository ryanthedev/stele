# Review: Phase 4 — incremental search + highlight cache (attempt 3)

Worktree `.code-foundations/wave-worktrees/phase-4`, `cc47dee` on base `c3b552f`.
Reviewed from the code and executed results only; the discovery file and the two
prior reviews were deliberately not read.

## Executed Results (Step 0)

| Command | Result |
|---|---|
| `script -q /dev/null cargo test --workspace` | **535 passed, 0 failed, 5 ignored** (exit 0) |
| `script -q /dev/null cargo test --workspace --release` | **535 passed, 0 failed, 5 ignored** (exit 0) |
| `script -q /dev/null cargo clippy --workspace --all-targets` | exit 0, **0 warnings / 0 errors** |
| `cargo fmt --all -- --check` | exit 0 |

The 5 ignored tests are all pre-existing live-Ghostty / conformance gates
(`crates/{width,gfx,probe}/tests/live_ghostty*.rs`, `crates/ast/tests/conformance.rs`);
none belongs to this phase.

Beyond the suite I ran my own pty probes against the real binary
(`target/debug/stele`, python `pty` harness, controlling terminal via
`setsid` + `TIOCSCTTY`), and two temporary in-crate probe tests that were
deleted afterwards (`git status` clean at the end of the review).

## Requirement Fulfillment

### DW-4.1
PREMISE:  `/` opens a query prompt in the status row; typing updates it; `Esc` restores the pre-search scroll position.
EVIDENCE: `crates/stele/src/app.rs:1050` (`'/'` → `begin_search`), `:905-920` (`begin_search`/`cancel_search`), `:514` (`Mode::Search` owns the status row), `:883-901` (`handle_search_key`), `:397-412` (`chrome_action`), `:249-262` (`Mode::captures_all_keys`), `crates/stele/src/main.rs:563-587` (`handle_chrome_key` is now decision-free).
TRACE:    scroll to 20 → `/` → `Mode::Search{origin:20}`, status row `"/"` → type `paragraph 70` → `refresh_incremental` → `find_matches` → `first_match_at_or_after(20)` → `reveal_current_match` scrolls away from 20 → `Esc` → `cancel_search` → `set_scroll(20)`, `Mode::Normal`, `SearchState::default()`.
EXECUTED: `app::tests::test_dw_4_1_slash_opens_a_prompt_typing_updates_it_and_esc_restores_the_scroll_position`, `..._backspace_shortens_the_query_and_esc_from_an_empty_query_still_restores`, `..._no_printable_key_is_claimed_as_chrome_while_a_query_is_open`, `..._the_chrome_keys_are_ordinary_characters_inside_a_query`; pty: `search_key_routing::test_dw_4_1_every_printable_key_reaches_the_query_including_the_chrome_keys`, `..._typing_t_into_a_query_does_not_swap_the_theme`, `test_t_still_swaps_the_theme_when_no_query_is_open`. Plus my own probe: all 94 printable ASCII characters (0x21–0x7E) typed one at a time into a live query, each verified on the status row of the resulting frame — none lost, none acted.
VERDICT:  **PASS**

### DW-4.2
PREMISE:  Matching is case-insensitive for an all-lowercase query and case-sensitive once the query contains an uppercase character.
EVIDENCE: `crates/stele/src/app.rs:171-173` (`SearchState::case_sensitive` = `query.chars().any(char::is_uppercase)`), `:1540-1555` (`match_len_at` / `chars_match`).
TRACE:    doc `alpha one / Alpha two / ALPHA three`; query `alpha` → `case_sensitive == false` → `chars_match` folds per char → 3 matches, each slicing back to the document's own casing. Query `Alpha` → `case_sensitive == true` → 1 match.
EXECUTED: `app::tests::test_dw_4_2_lowercase_query_is_case_insensitive_and_an_uppercase_one_is_not`, `..._smart_case_reads_the_uppercase_flag_from_non_ascii_letters_too` (`ärger`/`Ärger`).
VERDICT:  **PASS**

### DW-4.3
PREMISE:  `n`/`N` cycle forward and backward through matches and wrap at both ends.
EVIDENCE: `crates/stele/src/app.rs:1051-1052` (bindings), `:961-970` (`step_match`, modular arithmetic with `+ len - 1` for the backward step).
TRACE:    3 matches, `current == 0`; `n` → 1 → 2 → `(2+1)%3 == 0`; `N` from 0 → `(0+2)%3 == 2` → 1 → 0. Each step calls `reveal_current_match`, and the test asserts the match's line is inside the viewport afterwards and still slices to the query.
EXECUTED: `app::tests::test_dw_4_3_n_and_shift_n_cycle_forward_and_backward_and_wrap_at_both_ends`; `..._n_before_any_search_reports_nothing_rather_than_no_matches` pins the empty-set path.
VERDICT:  **PASS**

### DW-4.4
PREMISE:  Every visible match is highlighted and the current match is visually distinct from the others.
EVIDENCE: `crates/stele/src/painter.rs:720-745` (`visible_spans`), `:753-798` (`project_match`), `:806-866` (`apply_overlay`/`split_run`/`push_piece`), `:590-599` (`paint_run` applies the overlay **after** `expand`, so search wins over syntax), `crates/layout/src/lib.rs:135-144` (the two roles).
TRACE:    4 matches on 4 visible lines, `current == 0` → `visible_spans` tags index 0 `SearchCurrent` and the other three `SearchMatch` → the wire carries `\x1b[…SearchCurrent…]needle` exactly once and the `SearchMatch` introducer exactly three times; `n` moves the single current styling without creating a second.
EXECUTED: `painter::tests::test_dw_4_4_every_visible_match_is_highlighted_and_the_current_one_is_distinct`, `..._a_match_spanning_a_wrap_boundary_is_highlighted_on_both_lines`, `..._a_match_spanning_a_blank_line_inside_a_block_is_highlighted_in_full`, `..._a_search_highlight_wins_over_syntax_highlighting_in_a_code_block`, `test_a_highlight_covers_the_match_and_nothing_around_it`. My pty probe additionally read the painted `SearchCurrent` SGR off the wire and confirmed the highlighted bytes are exactly `kestrel`, before and after two mid-query resizes.
VERDICT:  **PASS**

### DW-4.5
PREMISE:  A query with no matches leaves the viewport unmoved and reports so in the status row.
EVIDENCE: `crates/stele/src/app.rs:941-956` (`refresh_incremental` returns before touching the scroll when `matches.is_empty()`), `:178-191` (`SearchState::prompt` → `"/{query} — no matches"`), `:991-997` (`report_no_matches` for `Enter`/`n`).
TRACE:    scroll 25 → `/` → each of the 21 characters of `zzzznotinthisdocument` re-runs the search, finds nothing, and returns early; the test asserts `scroll == 25` after **every** keystroke. Prompt reads `… — no matches`; `Enter` and a following `n` each set the transient `no matches: …` message and still do not move.
EXECUTED: `app::tests::test_dw_4_5_a_query_with_no_matches_leaves_the_viewport_unmoved_and_says_so`.
VERDICT:  **PASS**

### DW-4.6
PREMISE:  Two adjacent frames over identical code-block content invoke the underlying syntax highlighter once, not twice.
EVIDENCE: `crates/stele/src/painter.rs:662-676` (`expand` routes code-block runs through `HighlightCache::get_or_compute`), `crates/highlight/src/cache.rs:119-136`, `crates/highlight/src/highlighter.rs` `classify`/`Highlighted::cacheable`, `crates/stele/src/decor/mod.rs:32-40` (trait contract), `crates/stele/src/decor/themed.rs:50-56`.
TRACE:    12-line rust fence, `CountingDecor{cacheable:true}` → frame 1 calls `highlight` 12 times (one per visible code line); frame 2 over the same tree/scroll/size adds **0** calls. With `cacheable:false` the same spy is re-invoked on all 6 frames and `cache.is_empty()` holds.
EXECUTED: `painter::tests::test_dw_4_6_two_frames_over_the_same_code_block_invoke_the_highlighter_once`, `..._a_non_cacheable_highlight_is_re_invoked_on_every_frame`, `..._a_document_with_no_code_block_never_invokes_the_highlighter`, `..._registering_a_decor_drops_the_cache_built_by_the_previous_one`, `cache::tests::test_dw_4_6_a_repeated_lookup_computes_once_and_returns_the_same_runs`, `..._a_non_cacheable_result_is_recomputed_on_every_lookup`.
VERDICT:  **PASS**

### DW-4.7
PREMISE:  A committed benchmark shows code-heavy frame time at least 10× lower than the pre-change baseline recorded in the same harness.
EVIDENCE: `crates/stele/src/painter.rs:1918-2011` (the benchmark), `:163-170` (`with_cache_capacity(0)` — the baseline arm is a real constructor that reproduces the pre-change paint path, not a test hook).
TRACE:    40 real-Rust code lines at 80 cols, 20 frames per arm, both warmed; baseline `capacity == 0` never retains, so every line is re-parsed exactly as before the phase. Ratio asserted against a 10× floor in release and a 3× floor in debug, plus a byte-equality assertion that the two arms paint **identical** frames (so a "faster" frame cannot win by dropping highlighting).
EXECUTED: I ran the benchmark 4× in release and 3× in debug:
- release: `uncached 1.199 ms/frame, cached 100.5 µs/frame, 11.9×` · `11.9×` · `12.3×` · `12.1×` (floor 10×)
- debug:   `uncached 6.48 ms/frame, cached 1.58 ms/frame, 4.1×` · `4.2×` · `4.3×` (floor 3×)
VERDICT:  **PASS** (margin note below)

### DW-4.8
PREMISE:  Both new style roles stay distinct from every existing role after 256-color downsampling.
EVIDENCE: `crates/highlight/src/theme.rs:282-292` (`SearchMatch`/`SearchCurrent` at `TRAILING_ROLE_BASE`/`+1`, appended past the capture block rather than inserted), `:34-52` (`SEMANTIC_ROLES` / `TRAILING_ROLE_BASE` / `CAPTURE_ROLES`), `:171-187` (attributes), `crates/stele/src/decor/mod.rs:104-126` (themeless attribute table).
TRACE:    `Theme::new(variant, Downsample256).resolve(...)` for both variants: the two search colors differ from each other and from every one of the 33 `Semantic` variants **and** every `Capture` role. The exhaustiveness guard (`_exhaustiveness_guard`, `theme.rs:589-616` and `decor/mod.rs:304-331`) makes a future `Semantic` variant a compile error in the role lists, so the "every role" claim cannot silently go stale.
EXECUTED: `theme::tests::test_dw_4_8_search_roles_stay_distinct_from_every_other_role_after_256_downsample`, `..._search_roles_differ_by_attributes_even_with_color_stripped`, `decor::tests::test_dw_4_8_search_roles_are_distinguishable_on_the_themeless_path_too`, plus the palette-stability regression gates `test_capture_colors_are_pinned_so_a_new_role_cannot_silently_restyle_code_blocks` and `test_the_capture_block_still_begins_immediately_after_the_semantic_roles`.
VERDICT:  **PASS**

**All requirements met:** YES

## Focus-of-this-pass verification (my own probes)

### 1. Key routing

| Probe | Method | Result |
|---|---|---|
| Every printable char reaches an open query | real binary over a pty; typed all 94 chars 0x21–0x7E one at a time, read the status row of each resulting frame | PASS — final row `/!"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\]^_\`abcdefghijklmno` (clipped at 80 cols, nothing lost) |
| `T` still swaps the theme with no query open | pty; compared the heading's truecolor fg before/after | PASS |
| `+` / `-` still relay out with no query open | pty; compared rendered rows before/after, and that `+` exactly undoes `-` | PASS |
| TOC does not lose chrome keys | pty; `t`, three `-` and a `+` under the overlay, then `Esc` and a byte-identical document comparison | PASS (`toc_key_routing::test_dw_3_2_a_chrome_key_pressed_under_the_toc_does_not_reach_the_document`) |
| Same routing in the resize drain as when idle | pty; `SIGWINCH` immediately followed by `T+-x` **with a query open** | PASS — status row read `/T+-x` and the heading's fg was unchanged |
| Same routing in the resize drain, TOC | `toc_key_routing::test_dw_3_2_a_chrome_key_read_during_a_resize_drain_does_not_reach_the_document` | PASS |
| Inverse: no key that previously acted is diverted or double-handled | pty; `G`, `g`, `j`, `k`, `Ctrl-D`, `Ctrl-U`, `t`, `Esc`, `Ctrl-G`, `q`, `Ctrl-C` all still act | PASS |

The inverse also holds by construction. Base `c3b552f`'s guard was
`if key.modifiers.contains(CONTROL) || state.mode() != Mode::Normal { return false }`;
the replacement is `captures_all_keys()` (false only for `Normal`) followed by the
same CONTROL check — the same predicate, reordered. Double-handling is
impossible at both call sites: `main.rs:420` and `:466` short-circuit
(`!handle_chrome_key(..) && state.handle_key_event(key)`), so a key the chrome
table claims never reaches `handle_key_event`. `/`, `n` and `N` were unbound at
the base commit, so nothing pre-existing was displaced.

### 2. Search meets reload

Driven through the real binary under `--watch`:

- 120-paragraph doc, `G` to the tail, `/kestrel` → `[1/3]`, current match painted in `SearchCurrent`. The file was then replaced with a 5-line document mid-query: the status row became `[1/1]` on its own, the single remaining match was the only thing highlighted, and `Esc` landed on real content (`tiny doc top` and `only one kestrel now` both on screen) with the ruler back on the status row. The child stayed alive and exited 0.
- The growing direction too: a 3-line doc searched for `kestrel`, replaced mid-query by a 200-paragraph doc → the match set was re-found (`/4]`) and `Esc` landed on painted content.
- Mid-query resizes (80 → 44 → 31 columns): the painted `SearchCurrent` bytes still spelled exactly `kestrel` at every width, and `Esc` cleared every search highlight.

Structurally this is `relayout` → `reseat_search(reflowed)` (`app.rs:1146-1156`,
re-seats `origin` on the anchored scroll only when something actually reflowed)
→ `recompute_matches` (`:1176-1185`, full re-scan of the installed tree, `current`
clamped). `set_scroll` clamps to `max_scroll` on every path, so `Esc` cannot land
past the end. Covered in-tree by `test_a_reload_while_a_search_is_open_reseats_the_escape_origin`,
`..._leaves_every_match_addressing_the_new_document`,
`test_a_relayout_that_reflows_nothing_leaves_the_escape_origin_alone`,
`test_matches_are_recomputed_after_a_relayout_that_rewraps_them`,
`test_narrowing_while_a_search_is_active_keeps_every_match_addressing_its_text`.

### 3. Cross-phase regressions

- **TOC overlay (Phase 3):** both `toc_key_routing` pty tests and all 9 `toc_overlay` tests pass; my own pty round trip under the overlay reproduced them.
- **Image residency:** I wrote a temporary probe that paints the same image-bearing document at the same scroll twice — once via `frame`, once via `frame_with_search` with a live two-match overlay straddling the image — and compared the emitted kitty graphics command stream. Identical (2 ops each). This is also true by construction: `paint_line` routes `Line::Reserved` to `paint_reserved` without ever seeing the span list (`painter.rs:460-466`), and `paint_items` advances its byte cursor over runs only, matching `append_line_text`/`line_text_len`, which both skip boxes. `stale_placement`, `scroll_placement`, `media_fallback_position` and `tmux_graphics` all pass.

### 4. Test integrity

- `tests/toc_overlay.rs` is the only pre-existing test file touched (9 lines). The change adds the `Mode::Search { .. }` arm and swaps `frame_with_status` for `frame_with_search(.., state.search_overlay(), ..)`. The claim that this still mirrors `main.rs::paint` **holds**: `main.rs:516-532` routes `Mode::Normal | Mode::Search{..}` to `frame_with_search` and `Mode::Toc` to `frame_overlay`, exactly as the harness now does; and `frame_with_status` is *defined* as `frame_with_search(.., SearchOverlay::default(), ..)` (`painter.rs:239`), which with no search active is byte-identical to `state.search_overlay()` (`matches: &[]`, `current: None`). The edit was forced by `Mode` gaining a variant into a wildcard-free match, not by an assertion being relaxed — no assertion in that file changed.
- Nothing else was weakened. `git diff c3b552f..HEAD | grep '^-.*fn test_'` returns nothing; exactly one assertion line was removed (`assert!(runs.len() > 1)` in `decor/themed.rs`), replaced in place by `assert!(highlighted.runs.len() > 1)` **plus** a new `cacheable` assertion. Comparing the full set of `fn test_*` names between `c3b552f` and `HEAD` across `crates/`: 0 deleted, 50 added.

## Test-DW Coverage

- [x] Every DW item has DW-tagged automated tests that ran in Step 0 (mapping in the Requirement Fulfillment section above).
- [x] Coverage matches the stated 100% level — every DW has at least one test, and the two that a unit test structurally cannot reach (DW-4.1's routing seam, DW-4.4's wire-level styling) additionally have pty / byte-level gates.
- No DW rests on "observed behavior" alone.

## Dead Code

None found. `cargo clippy --workspace --all-targets` is clean (0 warnings), so no unused imports, unreachable code, or unused bindings. No `#[allow(...)]`, `dbg!`, `todo!`, or stray `eprintln!` in the phase's files; the single `println!` (`painter.rs:1993`) is the DW-4.7 benchmark's measurement output inside `#[cfg(test)]`.

## Correctness Dimensions

| Dimension | Status | Evidence |
|-----------|--------|----------|
| Concurrency | N/A | Single-threaded. The only thread in reach is `highlighter::highlight_with_timeout`'s pre-existing guard, and it is joined-or-abandoned inside one synchronous `miss` call; `HighlightCache` is `Rc`-based and never leaves the painter. |
| Error Handling | PASS | The overlay adds no I/O. `frame_with_search` still writes `SYNC_END` on **every** exit path including the error one (`painter.rs:262-267`) — traced and re-confirmed against `test_frame_closes_the_sync_update_block_after_a_recoverable_write_error`. `split_run` refuses to slice on a non-char-boundary rather than panicking mid-frame (`:833-835`). |
| Resources | PASS | Cache is bounded by construction: `insert` evicts FIFO **before** inserting (`cache.rs:153-168`), capacity 4096, and a non-`cacheable` result is never retained. `test_the_cache_never_grows_past_its_capacity_and_evicts_the_oldest_first` and `test_a_zero_capacity_cache_recomputes_every_time_and_never_retains` executed. `register_decor` clears it, so a `T` cannot serve one decor's answers as another's. |
| Boundaries | PASS | Adversarially probed. `count - 1` in `step_match` is guarded by the `count == 0` early return; `first_match_at_or_after` falls back to 0; `matches.get(current)` is fallible; `line_containing` uses `partition_point(..).saturating_sub(1)` and returns `Option`; `reveal_current_match`'s `line + 1 - height` cannot underflow because the branch implies `line >= height`; `search_overlay()` reports `current: None` exactly when it cannot back the index. I also probed grapheme splitting — a query matching one regional indicator of `🇺🇸`, the base of `e◌́`, or the first emoji of a ZWJ family — at exact-fit viewport widths: no panic, no width drift, no trailing text dropped. |
| Security | PASS | `sanitize` still runs on every piece **after** the overlay splits a run (`painter.rs:607`), so a match cannot be used to smuggle an escape byte past the barricade. A matched piece loses its `Link` style and therefore emits no OSC 8 opener, so a highlight cannot re-target a hyperlink. Search operates on already-laid-out text; no new untrusted-input surface. |

## Loaded-Skill Criteria

| Skill | Criterion | Status | Evidence |
|-------|-----------|--------|----------|
| cc-control-flow-quality | Max nesting depth ≤ 3 | PASS | Deepest new code is 3 (`frame_body` `painter.rs:382-402`, `paint_run` `:601-646`); the search core is 2 (`find_matches` `app.rs:1442-1467`, `project_match` `painter.rs:764-797`). |
| cc-control-flow-quality | McCabe ≤ 10, or the flat-dispatch exception | PASS | `handle_key` (11 arms) and `handle_toc_key` (12 arms) are flat `match`es over a `KeyCode` with ≤3-line arms and no nesting inside them — the stated exception. `handle_search_key` is 5 arms; `chrome_action`, `step_match`, `refresh_incremental` are all ≤5. |
| cc-control-flow-quality | Guard clauses exit early; nominal path unnested | PASS | `chrome_action:398-405`, `reseat_search:1147-1152`, `recompute_matches:1177-1179`, `visible_spans:727-729`, `find_matches:1434-1436`, `split_run:826-835`, `push_piece:851-853`. |
| cc-control-flow-quality | Loop selection (`for` when the count is known, `while`/loop-with-exit otherwise) | PASS | `while line < tree.line_count()` where the block-run boundary is discovered, not known; `while let Some(..) = find_from(..)` for the unbounded scan; `for` over `overlay.matches`, `runs`, `spans`. |
| cc-control-flow-quality | Descriptive loop indexes, no `i`/`j`/`k` | PASS | `line`, `past`, `row`, `byte`, `col`, `offset`, `cursor`, `from`, `remaining`. |
| cc-control-flow-quality | Booleans as `true`/`false`; complex booleans named or parenthesized | PASS | `chars_match` (`app.rs:1553-1555`) parenthesizes its `||`/`&&`; `reflowed`, `line_full`, `painted_any`, `case_sensitive` are named intermediates rather than inline expressions. |
| cc-control-flow-quality | Single-use test extracted to a named boolean function | PASS | `Mode::captures_all_keys`, `SearchState::case_sensitive`, `SearchOverlay::is_empty`, `no_reflow_occurred` — each is one test with one caller, named. |
| cc-control-flow-quality | Table-driven where a 4th branch on the same classification appears | PASS | `semantic_attrs`, `structural_style`, `semantic_role_index` and `capture_role_index` are exhaustive tables, not if-else chains; the new roles are one row each. |
| cc-control-flow-quality | Boolean flag parameters (control coupling) | WARNING → Note | `case_sensitive` threaded through 4 helpers, plus `forward`/`reflowed`/`for_reload`. Consistent with the pre-existing codebase style, no demonstrable defect — Note 10. |
| performance-optimization | Correctness before speed | PASS | The cache is a pure memo; `test_dw_4_7…` asserts the two arms emit **identical bytes**, so speed was not bought with output. |
| performance-optimization | Measured before tuning; specific hot spot identified | PASS | The audit figure (2,525 µs/frame against a 64 µs budget, ~40×, attributed to the per-frame tree-sitter parse) is cited at `cache.rs:3-8` and `painter.rs:654-658`, and the benchmark measures the same hot spot live in both arms. |
| performance-optimization | Fundamental fix (add a cache) preferred over code tuning | PASS | Step 4 of the decision tree: the repeated expensive computation was eliminated rather than micro-tuned. The one micro-optimization present (`write_sgr` allocating nothing, `painter.rs:936-941`) carries its own measurement (~90 µs/frame) and its reason for being second. |
| performance-optimization | Change validated / regression-proofed with the same workload | PASS | Two independent gates: a call-count gate (DW-4.6) that fails at 1× on a lost cache, and a wall-clock ratio gate (DW-4.7) with a profile-aware floor. Reproduced by me 7×. |
| performance-optimization | Bounded memo, no unbounded growth | PASS | `DEFAULT_CACHE_CAPACITY = 4096`, FIFO eviction, executed capacity test. |
| performance-optimization | Kept only for a significant, measured speedup | PASS | 11.9–12.3× release. |
| performance-optimization | New per-frame cost introduced without measurement | WARNING → Note | `visible_spans` scans the whole match list from index 0 every frame. I measured it (Note 2): +115 µs/frame on a pathological document. Not a measure-first violation — nothing was optimized here — but it is an unmeasured cost the phase added. |

## Notes (non-blocking)

| # | Finding | Confidence | Severity |
|---|---|---|---|
| 1 | The query prompt is clipped by the status row's width. On an 80-column terminal a query longer than ~78 characters truncates and the `[n/m]` counter falls off the end entirely — the reader loses both the tail of what they typed and the match count. Observed directly (probe 1's final row). No DW asks for scrolling or eliding the prompt. | High (observed) | ⚪ Low |
| 2 | `visible_spans` (`painter.rs:731-743`) iterates `overlay.matches` from index 0 on every frame and `break`s only at the first match past the viewport's bottom, so a reader parked below their matches pays O(total matches) per frame. Measured on a 1 MB document with 25,644 matches, reader at the tail: **148.7 µs/frame with the overlay vs 33.9 µs without** (release, same tree/scroll/size). Matches are sorted by line, so a `partition_point` for the first match with `line >= scroll` would make this O(visible). | High (measured) | 🟡 Med |
| 3 | Incremental search re-scans the entire document on every keystroke (`refresh_incremental` → `find_matches`). Measured worst case 4.35 ms/keystroke on the same 1 MB document (release), and 4.25 ms for a pathological all-`e` query. Comfortable for interactive typing; recorded so the number exists. | High (measured) | ⚪ Low |
| 4 | DW-4.7's release floor has ~19% headroom (11.9–12.3× against a 10× floor across 4 runs on this host). The debug floor has ~37% (4.1–4.3× against 3×). A loaded CI host could dip under the release floor; the test's own doc comment acknowledges the profile sensitivity but not the margin. | High (measured 7×) | ⚪ Low |
| 5 | A match inside a link splits the OSC 8 hyperlink. `push_piece` re-tags the matched piece as `SearchMatch`, and `paint_run` only opens OSC 8 for pieces whose `style_id` is `Semantic::Link`, so the label becomes *two* hyperlinks with an unlinked gap where the match is. The behaviour is deliberate and documented (`painter.rs:860-864`); the terminal-side consequence (a split link target region) is not. | High (traced) | ⚪ Low |
| 6 | `Line::Reserved`'s `prefix` runs (a blockquote gutter or list marker beside a media box) paint real text but contribute nothing to `append_line_text`/`line_text_len`, so that text is unsearchable. Search and paint agree on the coordinate system, so there is no misalignment — only a small coverage gap. | High (traced) | ⚪ Low |
| 7 | A query that matches *part* of a grapheme cluster (one regional indicator of `🇺🇸`, the `e` of `e◌́`, the first emoji of a ZWJ family) emits an SGR reset in the middle of the cluster, so the terminal renders it broken. Probed at exact-fit viewport widths: no panic, no text dropped, no width drift. Not one of the prompt's listed edge cases. | High (probed) | ⚪ Low |
| 8 | `tests/search_key_routing.rs` re-implements `common::pty::spawn_viewer`'s spawn + `pre_exec` block (including its `unsafe`) rather than reusing it, because it needs `TERM_PROGRAM=ghostty` and the shared helper removes that variable. That env setting also turns on a cell-geometry query round trip the text-only assertions do not need. The file's own doc explains the `render_row` avoidance but not this. | High (read) | ⚪ Low |
| 9 | `apply_overlay` assumes `Decor::highlight` returns runs whose text concatenates back to the input line; a decor that violated it would silently shift every span offset on that line. Both in-tree implementations honour it and there is a test for the themed one, but the trait doc (`decor/mod.rs:26-40`) states the `cacheable` contract and not this one. | Med (traced, not demonstrated) | ⚪ Low |
| 10 | Boolean flag parameters (`case_sensitive` threaded through `find_matches` → `collect_block_matches` → `find_from` → `match_len_at` → `chars_match`; also `forward`, `reflowed`, `for_reload`) are control coupling. Consistent with the existing codebase, no defect. | High (read) | ⚪ Low |

## Issues (if FAIL)

None. No test failed, no TRACE produced a wrong result, no probe reproduced a defect, no listed edge case is unhandled, and no loaded-skill criterion is demonstrably violated.

**Verdict: PASS**
