# Review: Phase 4 — Incremental search + highlight cache

## Executed Results (Step 0)

- `cargo test --workspace` → **all pass**. 149 stele lib + 1 main + 50 highlight lib + 42 layout behavior + 5 layout dw + 1 layout perf + 15 ast api + 29 gfx + 20 math + 8 mermaid + 6 probe + integration suites. 0 failed; 5 ignored (pre-existing live-Ghostty / conformance gates).
- `cargo clippy --workspace --all-targets` → **clean**, 0 warnings.
- `cargo fmt --all -- --check` → **clean**, exit 0.
- `cargo test --release -p stele --lib test_dw_4_7` ×3 → pass at 12.4x / 12.4x / 12.6x.
- `cargo test -p stele --lib test_dw_4_7` ×3 (debug) → pass at 4.3x / 4.4x / 4.4x.
- Working tree left clean; every probe below was reverted (`git status --porcelain` empty).

## Requirement Fulfillment

### DW-4.1
PREMISE:  "`/` opens a query prompt in the status row; typing updates it; `Esc` restores the pre-search scroll position."
EVIDENCE: `app.rs:632` (`Char('/') => begin_search`), `app.rs:466` (`begin_search` stores `origin: self.scroll`), `app.rs:449-464` (`handle_search_key`), `app.rs:330-337` (`status()` returns `search.prompt()` in `Mode::Search`), `app.rs:476-483` (`cancel_search` → `set_scroll(origin)`).
TRACE:    80-paragraph doc, scroll 0 → `/` → `mode = Search{origin:0}`, `status().message == Some("/")` → 12 chars of `paragraph 60` → `search.query == "paragraph 60"`, prompt row starts `/paragraph 60`, `scroll() > 0` (incremental reveal) → `Esc` → `mode == Normal`, `scroll() == 0`, query and matches cleared.
VERDICT:  **PASS** — `test_dw_4_1_slash_opens_a_prompt_typing_updates_it_and_esc_restores_the_scroll_position`, plus `test_dw_4_1_backspace_shortens_the_query_and_esc_from_an_empty_query_still_restores` (restores from a non-zero origin, and survives backspace-past-empty). Both ran green in Step 0.

### DW-4.2
PREMISE:  "Matching is case-insensitive for an all-lowercase query and case-sensitive once the query contains an uppercase character."
EVIDENCE: `app.rs:152-154` (`case_sensitive()` = `query.chars().any(char::is_uppercase)`), `app.rs:~915` (`chars_match`), `app.rs:~900` (`match_len_at`, per-char comparison rather than folding the haystack).
TRACE:    `alpha one / Alpha two / ALPHA three` → query `alpha` → `case_sensitive() == false` → 3 matches, sliced back out of the tree as `["alpha","Alpha","ALPHA"]` (the document's case, not the typed case). Query `Alpha` → `case_sensitive() == true` → 1 match, slices to `Alpha`.
VERDICT:  **PASS** — `test_dw_4_2_lowercase_query_is_case_insensitive_and_an_uppercase_one_is_not` and `test_dw_4_2_smart_case_reads_the_uppercase_flag_from_non_ascii_letters_too` (`ärger` → 2, `Ärger` → 1; confirms `is_uppercase` not `is_ascii_uppercase`). The design's refusal to `to_lowercase()` the haystack is the right call and is what keeps byte offsets exact — verified by reading `match_len_at`, which accumulates `found.len_utf8()` from the *haystack* char.

### DW-4.3
PREMISE:  "`n`/`N` cycle forward and backward through matches and wrap at both ends."
EVIDENCE: `app.rs:633-634` (bindings), `app.rs:511-521` (`step_match`: `step = if forward {1} else {count-1}`, `current = (current + step) % count`).
TRACE:    3 matches, `current == 0` → `n`,`n`,`n` → 1, 2, 0 (wrap past last) → `N`,`N`,`N` → 2 (wrap past first), 1, 0. No underflow at index 0 because the backward step is `+ (count-1)`.
VERDICT:  **PASS** — `test_dw_4_3_n_and_shift_n_cycle_forward_and_backward_and_wrap_at_both_ends`. The test also asserts each landing match still slices to `needle` and sits inside the viewport, so traversal is not bookkeeping-only.

### DW-4.4
PREMISE:  "Every visible match is highlighted and the current match is visually distinct from the others."
EVIDENCE: `painter.rs:~646` (`visible_spans`), `painter.rs:~683` (`project_match`), `painter.rs:~727` (`apply_overlay`/`split_run`), `painter.rs:~510` (`paint_run` applies the overlay *after* `expand`).
TRACE:    4 matches on 4 rows in a 10-row viewport → wire carries `{SearchMatch SGR}needle` ×3 and `{SearchCurrent SGR}needle` ×1; `n` moves the distinct style without creating a second one. Distinctness comes from the two roles resolving to different `Style`s (asserted through `Decor::resolve`, not against a hardcoded escape).
VERDICT:  **FAIL** — the main claim holds and is well tested, but the projection drops part of a match. See Issue 1: a match whose byte range crosses a **zero-length line inside the same block** highlights only its first fragment. Demonstrated below.

### DW-4.5
PREMISE:  "A query with no matches leaves the viewport unmoved and reports so in the status row."
EVIDENCE: `app.rs:494-509` (`refresh_incremental` returns before `reveal_current_match` when `matches.is_empty()`), `app.rs:161-171` (`prompt()` → `/{query} — no matches`), `app.rs:523-530` (`report_no_matches`).
TRACE:    scroll 25 → `/` → each of 21 chars of `zzzznotinthisdocument` asserted to leave `scroll() == 25`; prompt contains `no matches`; `Enter` still 25 and sets a transient `no matches: …`; a subsequent `n` also 25 and reports again.
VERDICT:  **PASS** — `test_dw_4_5_a_query_with_no_matches_leaves_the_viewport_unmoved_and_says_so`. `test_n_before_any_search_reports_nothing_rather_than_no_matches` correctly pins the empty-query silence so the message is never a lie about a query nobody made.

### DW-4.6
PREMISE:  "Two adjacent frames over identical code-block content invoke the underlying syntax highlighter once, not twice."
EVIDENCE: `painter.rs:~594` (`expand` → `highlight_cache.get_or_compute`), `cache.rs:107-124`.
TRACE:    12-line rust fence, `CountingDecor{cacheable:true}` → frame 1: `calls == 12`; frame 2 over the same tree: `calls == 12` (unchanged).
VERDICT:  **PASS** — `test_dw_4_6_two_frames_over_the_same_code_block_invoke_the_highlighter_once` (painter), `test_dw_4_6_a_repeated_lookup_computes_once_and_returns_the_same_runs` and `test_dw_4_6_a_non_cacheable_result_is_recomputed_on_every_lookup` (cache). Also verified: a prose-only document never reaches `Decor::highlight` at all, and `register_decor` clears the memo.

### DW-4.7
PREMISE:  "A committed benchmark shows code-heavy frame time at least 10× lower than the pre-change baseline recorded in the same harness."
EVIDENCE: `painter.rs:~1063` (`test_dw_4_7_cached_code_heavy_frames_are_at_least_10x_faster_than_the_uncached_baseline`), `painter.rs:158-171` (`with_cache_capacity`), `cache.rs:141-144` (capacity 0 → never retains).
TRACE:    40 lines of realistic Rust at 80 cols, 20 frames each arm, both warmed. Release (my runs): uncached 1.186 / 1.207 / 1.239 ms per frame vs cached 95.9 / 97.7 / 98.4 µs — **12.4x, 12.4x, 12.6x**, clearing the 10x floor. Debug: 6.475 / 6.555 / 6.515 ms vs 1.514 / 1.505 / 1.484 ms — 4.3x–4.4x against a 3x floor.
VERDICT:  **PASS**, with the qualification examined and judged legitimate.

I checked the three things that could have made this a strawman:
1. **Is the baseline the genuine pre-change path?** Yes, and if anything it is *conservative*. `with_cache_capacity(0)` runs `decor.highlight` on every run exactly as `paint_run` did before this phase. It differs in two ways, both of which push the ratio **down**, not up: it pays an extra `Rc::from(Vec<Run>)` per run that the old path did not, and it uses the *new* allocation-free `write_sgr`. A true pre-change measurement (slow `write_sgr` in the baseline arm only) would score higher than 12.4x, not lower.
2. **Do both arms paint the same thing?** The test asserts `fast_bytes == slow_bytes`. A "faster" frame that dropped highlighting would fail.
3. **Is the profile qualification a real property or a way out?** Real. The *effect* is measured and reproducible on this host: the uncached path is 5.4x slower in debug while the cached path is 15.6x slower, so the ratio genuinely compresses. But see Note 5 — the code comment's stated *reason* for that ("a tree-sitter parse, which lives in optimized C whatever profile the crate is built in") is contradicted by its own numbers; if the parse were profile-independent the uncached arm would not have slowed 5.4x.

Two things the reader should know rather than discover: the doc comment claims 14.5x optimized; I measure 12.4–12.6x on this host across three runs (still ≥ 10). And the default `cargo test --workspace` run gates only at 3x — see Note 4.

### DW-4.8
PREMISE:  "Both new style roles stay distinct from every existing role after 256-color downsampling."
EVIDENCE: `theme.rs:40` (`SEMANTIC_ROLES: 22 → 24`), `theme.rs:260-261` (slots 22/23), `theme.rs:389-430` (`build_palette`'s greedy `used_downsampled` filter).
TRACE:    Dark and Light, `ColorMode::Downsample256`: `SearchMatch` ≠ `SearchCurrent`, and neither collides with any of the 28 entries of `all_semantics()` nor with any `role::ALL` capture fg. Distinctness is structural — every palette slot is admitted only if its 256-downsampled cell is unused — so the invariant holds for **all** roles, not just the new two; the pre-existing `test_dw_7_2_downsample_256_keeps_every_role_distinct` still passes at 24 slots.
VERDICT:  **PASS** — `test_dw_4_8_search_roles_stay_distinct_from_every_other_role_after_256_downsample`, `test_dw_4_8_search_roles_differ_by_attributes_even_with_color_stripped` (NoColor: both underline, only current is bold), `test_dw_4_8_search_roles_are_distinguishable_on_the_themeless_path_too`. Heading WCAG AA is unaffected — heading colors come from `heading_ramp_color(tier)`, which `SEMANTIC_ROLES` does not touch — and `test_heading_tiers_clear_wcag_aa_against_the_reference_backgrounds` passes at both Truecolor and Downsample256, both variants, H1–H6. See Notes 2 and 3 for two adjacent findings this DW item does not itself cover.

**All requirements met:** NO — DW-4.4 fails on the projection defect.

## Edge Cases (same standing as DW items)

| Case | Verdict | Evidence |
|---|---|---|
| Empty query | PASS | `find_matches` returns early on `query.is_empty()` (`app.rs:~800`); `test_an_empty_query_produces_no_matches_at_all` asserts no matches and `overlay.current == None`. |
| No matches (message + position) | PASS | Covered under DW-4.5. |
| Wrapping past last match to first | PASS | Covered under DW-4.3. |
| Match in a code block that is also syntax-highlighted (search wins) | PASS | `test_dw_4_4_a_search_highlight_wins_over_syntax_highlighting_in_a_code_block`, run with the real `ThemedDecor`: the match paints in `SearchCurrent`, the `CodeBlock` SGR never appears on it, and ≥3 distinct truecolor sequences survive around it so the rest of the line keeps its capture colors. `paint_run` applies the overlay after `expand`, so precedence is structural. |
| Query with multi-byte graphemes | PASS | `test_a_multi_byte_query_addresses_whole_characters` — `日本` yields 2 matches, each `range.len() == 6` (bytes, not chars), each slicing back to `日本`. `split_run` additionally re-checks `is_char_boundary` before slicing. See Note 12 for ZWJ clusters, which are outside literal-match scope. |

## Stated Constraints

| Constraint | Verdict | Evidence |
|---|---|---|
| A match spanning a wrap boundary highlights on both lines | **FAIL** | Holds for a true wrap (`test_dw_4_4_a_match_spanning_a_wrap_boundary_is_highlighted_on_both_lines` checks the wire on row 1 *and* row 2). Breaks when the intervening line has zero length — Issue 1. |
| Highlight cache is bounded | PASS | `cache.rs:141-156` evicts FIFO before insert; `test_the_cache_never_grows_past_its_capacity_and_evicts_the_oldest_first` asserts `len() <= 3` on every one of 10 inserts and that the oldest was the victim. Bound is by entry count, not bytes — see Note 9. |
| Must NOT cache a timeout-fallback result | PASS | `highlighter.rs:~100` (`classify`: `None` ⇒ `cacheable: false`, and `highlight_with_timeout` returns `None` for both timeout and error), `cache.rs:117` (`if computed.cacheable`). Proven at two levels: `test_dw_4_6_a_non_cacheable_result_is_recomputed_on_every_lookup` (cache stays empty over 5 lookups) and `test_a_non_cacheable_highlight_is_re_invoked_on_every_frame` (3 frames × 6 lines = 18 invocations, end to end through the painter). |

## Dead Code

None blocking. No unreachable code after early returns, no `dbg!`/`todo!`/`unimplemented!`/`eprintln!` in any changed implementation file, no commented-out blocks, no unused imports (clippy clean with `--all-targets`). Minor items in Notes 6–8.

## Correctness Dimensions

| Dimension | Status | Evidence |
|---|---|---|
| Concurrency | PASS | The cache is `Rc`-based and single-threaded, owned by `Painter`; the only thread in play is `highlight_with_timeout`'s pre-existing guard for >4096-byte lines, unchanged by this phase. No shared mutable state introduced. |
| Error Handling | PASS | `frame_with_search` preserves the existing discipline: `frame_body` + `paint_status_row` are `and_then`-chained, `SYNC_END` and `flush` run unconditionally, and `painted.and(closed)` reports the first error. `test_frame_closes_the_sync_update_block_after_a_recoverable_write_error` still passes. `write_sgr`'s new helpers propagate `io::Result` at every write. |
| Resources | PASS | Cache bounded (above). `Rc<[Run]>` means a hit is a refcount bump, no leak path — entries are dropped on eviction and on `clear()`. `register_decor` clears, so a decor swap cannot pin the old highlighter's output. |
| Boundaries | **FAIL** | Issue 1 — `project_match`'s `start >= length` guard misfires on a legitimate zero-length line and truncates the match. Demonstrated with two executed probes. |
| Security | PASS | Query text reaches the terminal only via `paint_status_row`, which `sanitize`s then `clip_to_width`s (`painter.rs:282-290`). Matched pieces are slices of run text that `paint_run` sanitizes per piece, so a split cannot smuggle a control sequence past the barricade. `split_run` re-checks `is_char_boundary` before every slice, so a hostile query cannot panic a painted frame. |

## Loaded-Skill Criteria

| Skill | Criterion | Status | Evidence |
|---|---|---|---|
| cc-control-flow-quality | Max nesting depth ≤ 3 | PASS | Deepest new routine is `find_matches` (while → while, plus a `let-else` guard) at 3; `project_match` 3; `split_run` 2; `handle_search_key` 2; `write_sgr` 2. |
| cc-control-flow-quality | McCabe ≤ 10 | PASS | `write_sgr` ≈ 6; `find_matches` ≈ 5; `project_match` ≈ 5; `collect_block_matches` ≈ 3; `visible_spans` ≈ 5. `semantic_attrs` / `structural_style` / `semantic_role_index` sit in the 10–20 band but qualify as flat exhaustive dispatch: no nesting inside arms, each arm one expression, cases exhaustive and compiler-enforced. |
| cc-control-flow-quality | Guard clauses at entry, nominal path unnested | PASS | `handle_key_event`'s `Mode::Search` guard (the right call — it is what stops `q` quitting mid-query, proven by `test_a_command_letter_is_just_text_while_a_query_is_being_typed`); `find_matches` empty-query; `visible_spans` empty-overlay/zero-height; `write_sgr`'s `*style == Style::default()`. |
| cc-control-flow-quality | Exhaustive matches, no wildcard arms over `Semantic` | PASS | Demonstrated, not assumed: I restored HEAD~1's `theme.rs` against the new `layout::Semantic` and got `E0004: non-exhaustive patterns: SearchMatch and SearchCurrent not covered` at both `semantic_attrs` and `semantic_role_index`. All three exhaustive sites (`theme.rs:145`, `theme.rs:231`, `decor/mod.rs:99`) handle the new variants; the only `Semantic` matches with a `_` arm are in test helpers. |
| cc-control-flow-quality | Table-driven over branch chains | PASS | `write_sgr`'s `[(enabled, code); 4]` array replaces four sequential `if`s; `decimal`'s fixed 3-byte buffer replaces `format!`. |
| cc-control-flow-quality | Loop selection / named indexes | PASS | `while` where the count is unknown (block scan, projection walk), `for` over known collections. Index names are `line`, `past`, `byte`, `col`, `cursor` — self-documenting. One nit in Note 13. |
| performance-optimization | Measure before tuning | PASS | A profile-first audit is cited with a number (2,525 µs/frame vs a 64 µs budget) and the fix targets exactly that hot spot. Not an unmeasured guess. |
| performance-optimization | Design/algorithm fix before micro-optimization | PASS | The primary fix is the Stage-2 "add a cache" — eliminating a repeated pure computation — not code tuning. The `write_sgr` micro-optimization is secondary and was reached only after the cache removed the dominant cost. |
| performance-optimization | Baseline is real, same harness/host/fixture | PASS | `with_cache_capacity(0)` is a real constructor exercising the real pre-change path, run in the same test, on the same host, over the same tree, with both arms warmed. Verified conservative (see DW-4.7). |
| performance-optimization | Each change validated against regression | PARTIAL → Note 4 | The cache has a committed gate. `write_sgr` has none: no committed benchmark, and the default suite's DW-4.7 floor is 3x. Byte-identity *is* protected by `golden_sgr.rs` and I verified it exhaustively (below), so the risk is a silent perf regression, not a correctness one. |
| performance-optimization | Not trading maintainability for <10% gain | PASS | I measured the `write_sgr` rewrite in isolation (release, 40 lines × 20 styled spans/frame): old 157.9 µs/frame, new 20.9 µs/frame. A ~137 µs/frame saving against a ~96 µs cached frame is far above the 10% bar; the added `DecimalU8` + two helpers are justified. |

### Claim 2 verified independently: `write_sgr` is byte-identical

I did not rely on the existing golden tests. I transcribed both implementations verbatim (old from `git show HEAD~1`, new from the worktree) into a standalone program and brute-forced **163,264 style combinations** — every one of the 16 bold/dim/italic/underline combinations × 4 bg presence/value cases × 769 fg cases (each of the three channels swept over all 256 values with the others fixed, plus a 12³ corner grid around every decimal-digit boundary: 0,1,9,10,11,99,100,101,199,200,254,255), plus a separate bg-only 12³ sweep.

**Result: zero mismatches.** `decimal()` is correct at 0, at 9/10, at 99/100, and at 255; the `*style == Style::default()` early return is exactly equivalent to the old empty-`codes` path; separator placement matches for every subset. `Style` has exactly the six fields the comparison covers, so the default-equality shortcut cannot mask a seventh. Claim verified.

## Notes (non-blocking)

| # | Finding | Confidence | Severity |
|---|---|---|---|
| 2 | **Every syntax capture color changed.** Bumping `SEMANTIC_ROLES` 22→24 shifts `capture_role_index = SEMANTIC_ROLES + pos` by two slots, so every capture draws a different palette entry. Demonstrated: with the palette geometry restored to 22 and the search roles routed to `None`, `Comment` was `rgb(232,205,186)` and is now `rgb(153,243,134)`; `Keyword` was `rgb(193,110,103)`, now `rgb(222,33,183)`; all 24 colored captures moved. Distinctness and WCAG invariants still hold, and no requirement forbids it — but this repaints every code block in the viewer, which is a wider blast radius than "add two roles" implies. Appending the search slots *after* the captures (index `SEMANTIC_ROLES + role::ALL.len()`) would have been inert. | High (measured) | Medium |
| 3 | **`StructuralDecor` maps `SearchMatch` to the same `Style` as `Link` and `FootnoteRef`** (all three land on the bare `underline` arm, `decor/mod.rs:106`). On that path a match inside a link is indistinguishable from an unmatched link. `test_dw_4_8_search_roles_are_distinguishable_on_the_themeless_path_too` checks Text/CodeBlock/CodeInline/Strong/Emph — precisely the roles that do not collide — and omits the two that do. Low real-world impact: `main.rs` registers `ThemedDecor` for the actual session, so `StructuralDecor` is only the pre-registration default. DW-4.8 is scoped to 256-downsampling, so this is not a failure of it. | High | Low |
| 4 | **The default `cargo test` run does not gate the 10× claim.** `test_dw_4_7`'s floor is 3x under `debug_assertions` and 10x otherwise, and nothing in the repo runs the release arm. A regression that recovered only, say, 6x optimized would pass `cargo test --workspace` silently. The reasoning for a lower debug floor is sound and the debug floor is not vacuous (a lost cache scores ~1x), but "the 10x number is asserted where it means something" is only true if someone runs `--release`. Consider a CI step, or a `#[ignore]`d release-only companion. | High | Medium |
| 5 | **The DW-4.7 doc comment's explanation is wrong even though its conclusion is right.** It says the removed cost "lives in optimized C whatever profile the crate is built in." My numbers contradict that: the uncached arm is 5.4x slower in debug (6.5 ms vs 1.2 ms), so the parse is *not* profile-independent. The real mechanism is that the parse slows 5.4x while the paint loop slows 15.6x, which compresses the ratio. Same conclusion, wrong reason — worth correcting so a future reader does not build on it. | High (measured) | Low |
| 6 | `HighlightCache` derives `Default`, which yields `capacity: 0` — a silently disabled cache. Nothing calls it today, but `HighlightCache::default()` is the obvious thing for a future caller to reach for and it would quietly restore the 2,525 µs frame. A hand-written `Default` delegating to `with_default_capacity()` would remove the trap. | High | Low |
| 7 | `HighlightCache::with_default_capacity`, `highlight`, `len`, and `is_empty` are public API exercised only by `cache.rs`'s own tests — production goes through `new(capacity)` + `get_or_compute`. Not dead by rustc's definition; unexercised surface nonetheless. | High | Low |
| 8 | `AppState::mode()` and `AppState::search()` are `pub` but read only from tests (the binary uses `search_overlay()` and `status()`). Same category as 7. | High | Low |
| 9 | The cache bound is by entry count (4096), not bytes. The doc's "a few megabytes at worst even for pathologically long lines" holds only because layout clips code-block runs to the viewport; it is not enforced. Fine today, worth knowing if clipping ever moves. | Medium | Low |
| 10 | `Mode::Search { origin }` holds a scroll index into the tree that existed when `/` was pressed. A resize mid-query relayouts and moves every line, but `origin` is not remapped — `Esc` then restores a position that no longer means what it did. No DW item or test covers it. | High (by inspection) | Low |
| 11 | `recompute_matches` clamps `current` rather than re-deriving it, justified by "the n-th match after it is the same text." That holds only while the count is unchanged. Because matching runs over *joined* laid-out text, a rewrap can create or destroy cross-boundary matches, and a match appearing before `current` shifts the reader onto a different one. I probed this (12 paragraphs, 90→30 cols) and the count held at 12 with no drift, so I could not demonstrate it — reporting it as a limit of the justification, not a proven bug. | Medium | Low |
| 12 | A match that covers only part of a grapheme cluster splits the cluster. Probed: searching `👨` inside `👨‍👩‍👧` produces the wire `…{SGR}👨{reset}\u{200d}👩\u{200d}👧…`, injecting SGR between the emoji and its ZWJ. No panic (the char-boundary check holds), but terminals will render the family emoji differently. Outside "literal matching" scope and not a listed edge case. | High (observed) | Low |
| 13 | `visible_spans` uses `match overlay.current == Some(index) { true => …, false => … }` where `if`/`else` is the idiomatic form for a boolean. Style only. | High | Low |
| 14 | Measured for context, no action: incremental search costs 3.5 ms per keystroke on a 972 KB / 17,999-line document in release (6,000 matches). Comfortably interactive. `find_from` is a naive O(n·m) scan with no committed benchmark, but at this scale it does not need one. | High (measured) | Low |
| 15 | A >4096-byte code line that persistently times out is correctly never cached, so it spawns a guard thread every frame. This is unchanged from the pre-phase behavior (which also re-ran every frame) and is the right trade against freezing a stall into the session — noting it only because the cache makes it a deliberate choice rather than an accident. | High | Low |

**Claim 5 — out-of-scope files.** Both edits are minimal and necessary, not creep. `crates/layout/src/lib.rs` adds exactly two enum variants plus a doc comment explaining why a role layout never emits belongs there (the exhaustive style tables are keyed on this enum, and putting them anywhere else would mean a parallel type). `crates/stele/src/main.rs` extracts the duplicated five-argument paint call into one `paint()` helper and routes it to `frame_with_search` — a real reason is given (`status()` spends a frame of TTL per call, so two drifted copies would age messages at different rates), and it is a net reduction in duplication. No unrelated changes in either.

## Issues (FAIL)

### 1. A match whose range crosses a zero-length line loses everything past that line

- **File:** `crates/stele/src/painter.rs`, `project_match`, the guard `if start >= length { return; }`
- **Cause:** a block's lines are joined for matching, and a blank line inside a block is a real line with `line_text_len == 0`. `project_match` walks forward subtracting each line's length; on reaching the empty line it has `start == 0` and `length == 0`, so `start >= length` fires. That guard exists to detect a *stale tree* ("the match claims bytes this line does not have"), but it cannot tell a stale offset from a legitimately empty line, so it aborts the walk and silently drops every remaining fragment.
- **Demonstrated by** two probes I wrote, ran, and reverted:

  **Probe A** — source `` ```rust\nabc\n\ndef\n``` ``, query `cdef`:
  ```
  matches: [Match { block: NodeId(0), line: 0, range: 2..6 }]
  spans:   {0: [Span { start: 2, end: 3, role: SearchCurrent }]}
  query bytes = 4, highlighted bytes = 1
  ```
  Layout confirms the shape: `line 0 len=3 "abc"`, `line 1 len=0 ""`, `line 2 len=3 "def"`, all `block=NodeId(0)`.

  **Probe B** — a plausible real query. Source `` ```rust\nfn foo() {\n\n}\n``` ``, query `{}`. The match is found (`line 0, range 9..11`) but the wire shows the `{` highlighted and the `}` on row 3 painted plain:
  ```
  …fn foo() \x1b[0m\x1b[1;4m{\x1b[0m\x1b[K\x1b[2;1H\x1b[K\x1b[3;1H\x1b[0m}\x1b[0m…
                        ^^^^^^^^ highlighted            ^^^^^^ plain
  ```
  The same shape occurs in loose lists (`- item one\n\n- item two` → `line 1 len=0`). Blockquotes are unaffected: their blank line still carries the `│ ` gutter (len 4).

- **Why it blocks:** the search reports a match to the reader and the painter shows only a fragment of it, so DW-4.4's "every visible match is highlighted" is only partly true, and the stated constraint "a match spanning a wrap boundary highlights on both lines" fails for the empty-line variant of the same walk. It cannot panic and cannot highlight the wrong text — severity is low and the affected matches are themselves join artifacts — but it is a demonstrated wrong result on the phase's own headline path.
- **Fix:** distinguish "empty line" from "stale offset". Skip a zero-length line and continue the walk rather than returning:
  ```rust
  let length = line_text_len(tree, line);
  if length == 0 {
      line += 1;
      continue;          // a blank line inside the block consumes no bytes
  }
  if start >= length { return; }   // now genuinely means a stale tree
  ```
  Add a regression test alongside `test_dw_4_4_a_match_spanning_a_wrap_boundary_is_highlighted_on_both_lines` using Probe B's fixture, asserting the highlighted byte count equals the query length.

---

**Verdict: FAIL** — one blocker: Issue 1, `project_match` truncates any match crossing a zero-length line inside a block (DW-4.4 and the wrap-boundary constraint). DW-4.1, 4.2, 4.3, 4.5, 4.6, 4.7, and 4.8 all pass with execution evidence; every listed edge case is handled; `write_sgr` is verified byte-identical across 163,264 combinations; the DW-4.7 baseline is genuine and conservative.
