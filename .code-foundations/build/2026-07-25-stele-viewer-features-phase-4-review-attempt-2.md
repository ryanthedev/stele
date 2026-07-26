# Review: Phase 4 — incremental search + highlight cache (attempt 2)

Reviewed at `3d1af10` in `.code-foundations/wave-worktrees/phase-4`. Working tree clean before and after review; all reviewer probe files removed.

## Executed Results (Step 0)

| Command | Result |
|---|---|
| `cargo test --workspace` | 54 test binaries, **all ok**, 0 failed. 5 ignored, all pre-existing (`dump_failures`, live-Ghostty/GUI probes) |
| `cargo test --workspace --release` | **all ok**, 0 failed |
| `cargo clippy --workspace --all-targets` | clean — zero warnings, zero errors |
| `cargo fmt --all -- --check` | clean (exit 0) |

## Requirement Fulfillment

### DW-4.1 — **FAIL**
PREMISE: "`/` opens a query prompt in the status row; typing updates it; `Esc` restores the pre-search scroll position."
EVIDENCE: `crates/stele/src/main.rs:222-228` (`run_session`), `crates/stele/src/main.rs:298-322` (`handle_chrome_key`); guard that exists only on the other path at `crates/stele/src/app.rs:437-439`.
TRACE: `/` → `handle_chrome_key` returns `false` → `handle_key_event` → `begin_search()`; status row paints `/` (observed on the wire). `Esc` → `cancel_search()` → `set_scroll(origin)` (test at `app.rs:1936`, passes). **But** `T`/`+`/`-` → `handle_chrome_key` matches them *before* `AppState` ever sees the key, acts on them, and returns `true`, which short-circuits `state.handle_key_event(key)` via `&&`. The character never reaches `handle_search_key`, so `self.search.query.push(c)` never runs. `handle_chrome_key` has no `Mode::Search` guard.

Demonstrated against the **real binary over a real pty** (reviewer probe, since removed). Typing `/The`:

```
status rows: ["/", "/", "/h  [1/3]", "/he  [1/2]"]
```

- after `/`  → `/`
- after `T`  → still `/`; the H1 foreground on the wire flips `38;2;241;197;197` → `38;2;94;18;18` (dark→light theme toggled)
- after `h`  → `/h  [1/3]`
- after `e`  → `/he  [1/2]`

Final query is `he`, not `The`, and the reader's theme has flipped. Same for `+`: typing `/plus+` ends at `/plus  [1/1]` — the `+` was consumed by `state.widen(ctx)`.

This also contradicts the invariant `app.rs:433-435` states in prose: *"while the query prompt is open every key is text (or prompt editing), so the chord and motion tables below must not see it at all — `q` types a `q` rather than quitting."* The guard was placed inside `AppState`; `main.rs`'s chrome table sits outside it.

VERDICT: **FAIL**

### DW-4.2 — PASS
PREMISE: "Matching is case-insensitive for an all-lowercase query and case-sensitive once the query contains an uppercase character."
EVIDENCE: `app.rs:156-158` (`case_sensitive`), `app.rs:904-919` (`match_len_at`/`chars_match`).
TRACE: `"alpha"` over `alpha/Alpha/ALPHA` → `query.chars().any(char::is_uppercase)` false → `chars_match` folds → 3 matches, each slicing back to the document's own casing. `"Alpha"` → flag true → 1 match. Independently reproduced in my own probe (`Needle/needle/NEEDLE` → 3/1/1; `Ärger/ärger` → 2/1). Backing tests `test_dw_4_2_*` (`app.rs:2039`, `:2071`) ran green in Step 0.
Note: the matching *logic* is correct, but an uppercase query beginning with `T` cannot be typed at all in the shipped binary — see DW-4.1. Filed once, there.
VERDICT: PASS

### DW-4.3 — PASS
PREMISE: "`n`/`N` cycle forward and backward through matches and wrap at both ends."
EVIDENCE: `app.rs:531-540` (`step_match`).
TRACE: 3 matches, `current=0`. `n` → `(0+1)%3=1`, `(1+1)%3=2`, `(2+1)%3=0` (wrap). `N` → `step = count-1 = 2`, so `(0+2)%3=2` (wrap, no underflow), `1`, `0`. Independently reproduced (`n`→1,2,0,1; `N`→0,2,1,0). `test_dw_4_3_*` (`app.rs:2087`) additionally asserts each landing match slices back to the query and lands inside the viewport. Ran green.
VERDICT: PASS

### DW-4.4 — PASS
PREMISE: "Every visible match is highlighted and the current match is visually distinct from the others."
EVIDENCE: `painter.rs:653-678` (`visible_spans`), `:686-731` (`project_match`), `:739-799` (`apply_overlay`/`split_run`/`push_piece`), `:527-532` (overlay applied *after* `expand`).
TRACE: 4 matches on 4 rows, viewport 10 → 3 rows painted with `\x1b[0m\x1b[3;4m` (SearchMatch), exactly 1 with `\x1b[0m\x1b[1;3;4m` (SearchCurrent); `n` moves the distinct style without creating a second. Independently confirmed: with 20 matches and a 4-row viewport at scroll 6, exactly the on-screen ones are painted (6 bytes) and no more; a match spanning a blank line with only its tail on screen highlights exactly the 2 visible bytes.
VERDICT: PASS

### DW-4.5 — PASS
PREMISE: "A query with no matches leaves the viewport unmoved and reports so in the status row."
EVIDENCE: `app.rs:515-526` (`refresh_incremental` returns before `reveal_current_match`), `:163-176` (`prompt`), `:561-567` (`report_no_matches`).
TRACE: from scroll 25, each character of `zzzznotinthisdocument` → matches empty → early return, scroll stays 25. Status row observed on the wire in my probe: `"\x1b[2m/gamzzz — no matches\x1b[0m"`. `Enter` and a later `n` both re-report without moving. `test_dw_4_5_*` (`app.rs:2129`) ran green.
VERDICT: PASS

### DW-4.6 — PASS
PREMISE: "Two adjacent frames over identical code-block content invoke the underlying syntax highlighter once, not twice."
EVIDENCE: `painter.rs:595-609` (`expand`), `crates/highlight/src/cache.rs:119-136` (`get_or_compute`).
TRACE: 12-line rust fence, spy `Decor` counting `highlight` calls. Frame 1 → 12 calls. Frame 2 → still 12. Ran green in Step 0 (`test_dw_4_6_two_frames_over_the_same_code_block_invoke_the_highlighter_once`, `painter.rs:1682`), through the real `Painter::frame` path rather than a cache-only stub.
VERDICT: PASS

### DW-4.7 — PASS
PREMISE: "A committed benchmark shows code-heavy frame time at least 10× lower than the pre-change baseline recorded in the same harness."
EVIDENCE: `painter.rs:1851-1944`; baseline arm `Painter::with_cache_capacity(engine, 0)` (`painter.rs:163-170`) → `HighlightCache::new(0)` never retains (`cache.rs:153-156`), so its `expand` calls the decor on every run, which is the pre-change path.
TRACE: I ran the committed benchmark three times in **each** profile:

| Profile | uncached/frame | cached/frame | ratio | asserted floor |
|---|---|---|---|---|
| release | 1.173 / 1.238 / 1.244 ms | 93.9 / 100.7 / 102.0 µs | 12.5x / 12.3x / 12.2x | 10x — **passes** |
| debug | 6.449 / 6.520 / 6.471 ms | 1.498 / 1.485 / 1.513 ms | 4.3x / 4.4x / 4.3x | 3x — passes |

The numbers reproduce the table in the test's own doc comment exactly. The benchmark also asserts both arms emit byte-identical frames, so a "faster" arm cannot win by dropping work. The baseline is genuine and, if anything, *conservative*: it uses the phase's new allocation-free `write_sgr`, so the true pre-phase frame (parse + the old `format!`/`join` SGR path) was ~1.26 ms, giving ~13.4x. Requirement asks for a benchmark showing ≥10x; the release arm is in the prescribed suite and passes.
VERDICT: PASS (see Note 8 on the debug-arm floor)

### DW-4.8 — PASS
PREMISE: "Both new style roles stay distinct from every existing role after 256-color downsampling."
EVIDENCE: `theme.rs:292-293` (roles placed at `TRAILING_ROLE_BASE`), `:426-467` (`build_palette`'s greedy 256-cell filter), test `theme.rs:678`.
TRACE: `Theme::new(variant, ColorMode::Downsample256)` → `apply_mode` returns `downsample_256(c)` (`color.rs:81`), so the test compares actual 256-cube cells. It sweeps `all_semantics()` **and** every `role::ALL` capture in both variants; ran green. Themeless and `NO_COLOR` halves covered separately (see Claim 3 below).
VERDICT: PASS

**All requirements met:** NO — DW-4.1 fails.

## Edge cases

| Edge case | Status | Evidence |
|---|---|---|
| Empty query | HANDLED | `find_matches` returns `Vec::new()` (`app.rs:798`); `report_no_matches` silent on empty (`app.rs:562`); reproduced independently |
| No matches (message, position unchanged) | HANDLED | See DW-4.5 |
| Wrapping past the last match to the first | HANDLED | See DW-4.3 |
| Match inside a syntax-highlighted code block | HANDLED | `painter.rs:1543` runs the real `ThemedDecor`; overlay applied after `expand`, so the match paints `SearchCurrent` and the capture SGR for that token is absent from the wire while neighbouring tokens keep ≥3 distinct truecolor codes |
| Multi-byte graphemes in the query | HANDLED | `app.rs:2244` (`日本` ×2, byte lengths exact); my own probe adds `本語` spanning a blank line and `héllo→` — every byte highlighted; `split_run` also re-checks `is_char_boundary` (`painter.rs:766`) |
| Match spanning a wrap boundary highlights on both lines | HANDLED | `painter.rs:1429` reads the wrap point out of the tree, asserts row 2 *opens* with the highlighted continuation, and counts total highlighted bytes == query length |
| Cache bounded; never caches a timeout fallback | HANDLED | `cache.rs:153-168` (FIFO eviction), `:129` (`if computed.cacheable`); tests at `cache.rs:219`, `:239`, `:266`; end-to-end at `painter.rs:1722` (spy decor re-invoked 6×3 times) |

## Claims scrutinised

**1. Zero-length line handling in `project_match` — HOLDS.** I built my own fixtures rather than reusing the test file's. All 11 arrangements highlight every byte of the query: blank at block start, blank at block end, 3 and 6 consecutive blanks, a match ending exactly at the byte before a blank, a match starting on the first byte after a blank, two separate blanks in one block, a multi-byte match across a blank, and a loose list. I also confirmed a match can never *anchor* on a zero-length line: `line_containing` (`app.rs:866-871`) picks the **last** line whose join-offset ≤ `at`, and a blank line shares its successor's offset. The `start >= length` guard does still fire on a genuinely out-of-range anchor — a synthetic `Match { line: 0, range: 10..14 }` against a 5-byte line restyles **zero** bytes. The walk terminates on `0..usize::MAX/2`, `0..0`, and a line index past the tree (`line` strictly increases each iteration). See Note 4 for the one behaviour that changed.

**2. Capture colors restored — HOLDS, verified exhaustively.** I built the palette at the pre-phase commit `7187b5f` in a separate worktree and at `3d1af10`, dumping the resolved `fg` for **all 25 `Capture` roles and all 22 colored `Semantic` roles in both variants** (94 lines). `diff` is empty — byte-identical. Not just the 6 pinned samples.

**3. Structural-path style collisions — HOLDS in both files.** `decor/mod.rs:118-126` and `theme.rs:181-189` both give `SearchMatch = {italic, underline}` and `SearchCurrent = {bold, italic, underline}`, clearing `Link`/`FootnoteRef` (`{underline}`) and `Heading(1)` (`{bold, underline}`). Both files assert non-collision against **every** `Semantic` variant, not a sample — `decor/mod.rs:386` on the themeless path, `theme.rs:807` under `NO_COLOR`. Both ran green.

**4. Exhaustiveness guards — HOLD, verified by mutation.** I added a `Semantic::ZzReviewProbe` variant to `crates/layout/src/lib.rs` and compiled. After satisfying every production match, the two test-side guards were the last two errors:

```
error[E0004]: non-exhaustive patterns: `Semantic::ZzReviewProbe` not covered
   --> crates/highlight/src/theme.rs:605:19     (all_semantics::_exhaustiveness_guard)
error[E0004]: non-exhaustive patterns: `Semantic::ZzReviewProbe` not covered
   --> crates/stele/src/decor/mod.rs:309:19     (every_semantic::_exhaustiveness_guard)
```

Reverted; tree clean. See Note 7 for the guard's residual limitation.

**5. DW-4.7's benchmark — HOLDS.** Ratio verified in both profiles (table above), baseline confirmed as the genuine pre-change path. On the DW as written: it is met. The benchmark is committed, it shows ≥10x, and `cargo test --workspace --release` — one of the four prescribed suite commands — asserts that floor and passes. The test's own doc comment states plainly that the debug arm gates only 3x and that a partial regression (e.g. 6x) would slip past the default `cargo test`; I confirmed that limitation is real and I consider it honestly disclosed rather than concealed. Note 8.

## Test-DW Coverage

- [x] Every DW item has an automated test that ran in Step 0 (`test_dw_4_1` … `test_dw_4_8`), plus independent reproduction by me.
- [ ] **Gap:** no test anywhere covers `main.rs`'s key routing (`run_session` / `handle_chrome_key`) while `Mode::Search` is active. That is precisely the untested seam the DW-4.1 defect lives in — `app.rs`'s tests call `AppState::handle_key_event` directly and never see the chrome table that runs before it.
- [ ] **Gap (100% level):** `highlighter::classify`'s `None → cacheable: false` arm is never exercised. Its own doc comment says it was split out "so that rule is testable"; no test calls it. Both the cache test and the painter test substitute a spy that hard-codes `cacheable: false`, so the production wiring from a real fallback to a non-cached result is unproven.

## Dead Code

None found. `cargo clippy --workspace --all-targets` is clean (unused imports, unreachable code, and dead code are all denied-by-default lints there). `highlight_line` remains exported and used by `crates/highlight/tests/golden_sgr.rs` and an example, so it is not orphaned by `highlight_detailed`.

## Correctness Dimensions

| Dimension | Status | Evidence |
|-----------|--------|----------|
| Concurrency | PASS | The only thread is `highlight_with_timeout`'s >4096-byte guard, pre-existing and unchanged by this phase. `HighlightCache` is `!Send` (`Rc`) and lives behind `&mut Painter`; no shared state added. Note 3 records an interaction I could not demonstrate. |
| Error Handling | PASS | `frame_with_search` still closes `SYNC_END` on every exit path including error (`painter.rs:254-259`); `split_run` degrades a non-char-boundary offset to a skipped highlight rather than a panicking slice (`:766`); `line_containing`/`reveal_current_match` return rather than index. |
| Resources | PASS | Cache bounded at 4096 entries with FIFO eviction, verified by `cache.rs:239` and by construction (`insert` evicts before inserting; capacity 0 stores nothing). `register_decor` clears it. Note 9 on the memory ceiling's documented estimate. |
| Boundaries | PASS | Probed adversarially: `range` of `usize::MAX/2`, empty range, out-of-tree line index, zero-length lines at every position, multi-byte queries, viewport height 3 with a match straddling it, `decimal(0)`/`decimal(255)`. No panic, no wrong output. |
| Security | PASS | `sanitize` unchanged and still on the only text path — overlay-split pieces go back through `paint_run`'s `sanitize` + `clip_to_width` (`painter.rs:540-541`). `push_piece` retags the matched piece away from `Semantic::Link`, so a matched link fragment emits no OSC 8 (`painter.rs:559`, `:793-798`). |

## Loaded-Skill Criteria

| Skill | Criterion | Status | Evidence |
|-------|-----------|--------|----------|
| cc-control-flow-quality | Max nesting depth ≤ 3 | PASS | Deepest new routine is `paint_run` (`painter.rs:515`) at 3 (`for` → `if clipped_text.is_empty()` → `if !sanitized.is_empty()`). `project_match`, `split_run`, `find_matches`, `visible_spans` are all ≤ 2. |
| cc-control-flow-quality | McCabe ≤ 10, or a flat exhaustive dispatch | PASS | `project_match` ≈ 7, `paint_run` ≈ 9, `write_sgr` ≈ 8. `semantic_role_index` (24 arms) and `semantic_attrs`/`structural_style` qualify under the flat-dispatch exception: exhaustive, no nesting inside arms, ≤1 line each. |
| cc-control-flow-quality | Guard clauses for error/degenerate cases | PASS | `handle_key_event`'s search guard (`app.rs:437`), `visible_spans`' empty/zero-height guard, `insert`'s `capacity == 0` guard, `apply_overlay` skipped entirely when `spans.is_empty()`. |
| cc-control-flow-quality | Booleans as `true`/`false`, not as match subjects | PASS | The previous revision's `match overlay.current == Some(index) { true => …, false => … }` was replaced with a plain `if`/`else` (`painter.rs:670-674`). |
| cc-control-flow-quality | Descriptive loop indexes | PASS | `line`, `row`, `tier`, `attempt`, `cursor`, `byte`, `col` — no bare `i`/`j`/`k` in the new code. |
| cc-control-flow-quality | Named boolean functions over inline tests | PASS | `case_sensitive`, `chars_match`, `no_reflow_occurred`, `is_empty`, `line_containing`. |
| performance-optimization | Measure before tuning | PASS | Module docs cite the measured 2,525 µs/frame vs a 64 µs budget that motivated the phase; `write_sgr`'s rewrite cites a measured ~90 µs/frame; the committed benchmark measures both arms in one harness. I reproduced the benchmark's numbers. |
| performance-optimization | Fundamental fix (cache/algorithm) before micro-optimization | PASS | The primary change is a memo of a pure function — APOSD Stage 2 "add a cache" — not loop-level tuning. The one micro-optimization (`write_sgr`) came second and only after it became the largest remaining item. |
| performance-optimization | Re-measure; keep only measured wins | PASS | DW-4.7 asserts a ratio floor *and* byte-identical output between arms, so a change that trades correctness for speed fails. |
| performance-optimization | Red flag — pass-through methods / shallow layers | WARNING | `frame` → `frame_with_status` → `frame_with_search` are three entry points each defaulting one argument (`painter.rs:204`, `:223`, `:244`). Documented and not on a per-run path, so no measurable cost — Note 5. |
| performance-optimization | Red flag — avoidable allocation on the hot path | WARNING | `HighlightCache::lookup` builds an owned `(String, Option<String>)` on **every hit** (`cache.rs:146-149`), on the exact path the phase exists to make fast. Author-acknowledged in the comment. Note 1. |

## Notes (non-blocking)

1. **`lookup` allocates on every cache hit** (`cache.rs:146-149`, medium confidence / low-medium severity). A hit should be a hash of a borrowed key; it is currently `line_text.to_string()` + `lang.map(str::to_string)` per code run per frame. Order-of-magnitude estimate against my measurement: a cached release frame is 94 µs for 40 lines ≈ 2.3 µs/line, and an 80-byte `String` alloc + hash is ~100–200 ns, so roughly 5–8% of the cached frame. The 10x floor is met regardless. Fixable with a `Borrow`-friendly key type or `raw_entry`.

2. **`visible_spans` is O(total matches) per frame** (`painter.rs:664-676`, high confidence / low severity). It iterates from index 0 and only `break`s past the viewport bottom. `overlay.matches` is sorted by line, so a `partition_point` to the first match at/after `scroll` would make it O(visible + log M). Unmeasured, so this is an observation, not a demanded change.

3. **`classify`'s fallback path is untested, and a repeatedly-timing-out long line spawns a thread per frame** (`highlighter.rs:104-115` and `:143-154`, medium confidence / low severity). Because a fallback is deliberately never cached, a >4096-byte line that keeps exceeding the 250 ms cap is re-attempted — and re-threaded — on every repaint. I could not demonstrate it (forcing a real 250 ms timeout is not honestly testable on an arbitrary host), and the threading is pre-existing, but the "never cache a fallback" rule is what makes it per-frame rather than once.

4. **A stale match now bleeds across a zero-length line into a different block** (`painter.rs:709-720`, high confidence / low severity, **not reachable in-app**). Demonstrated with a synthetic `Match { line: 0, range: 3..43 }` against `"short\n\nother line here\n"`: 17 bytes highlighted — 2 on line 0 plus all 15 of a *different block's* line 2. Before the blank-line fix, `0 >= 0` on the gap line stopped the walk. The `start >= length` guard is now reachable only on the anchor line, exactly as its comment says. This is unreachable through the app: `AppState::relayout` is the only place `tree` is reassigned and it always calls `recompute_matches` (`app.rs:677`). Worth knowing if a future phase ever paints a tree the matches were not computed against.

5. **Three pass-through paint entry points** (`painter.rs:204`/`:223`/`:244`, high confidence / cosmetic). Each defaults one argument for call-site compatibility; the perf skill flags the shape, but nothing here is in a loop.

6. **`decor/mod.rs:197` asserts on `Run.width`** (high confidence / cosmetic). `docs/code-standards.md` says never to. Pre-existing (unchanged context in this phase's diff), reported not chased. Arguably in the spirit of the rule since it asserts the field is *unset*.

7. **The exhaustiveness guards are reminders, not proofs** (high confidence / low severity). They force a compile error on a new variant, which I verified. But a developer can satisfy the guard's `match` arm without adding the variant to the returned `vec!`, leaving the list stale and the guard green. The docs at `decor/mod.rs:298-306` are honest about what the guard is for; this is the residual.

8. **DW-4.7's 10x floor gates only `--release`** (high confidence / low severity). `cargo test` asserts 3x; a partial regression landing at, say, 6x optimized would pass the default suite. The test's doc comment (`painter.rs:1818-1846`) states this explicitly, including a self-correction of an earlier, wrong claim that the removed cost was profile-independent. Disclosed, not hidden.

9. **The cache's memory-ceiling estimate is optimistic** (`cache.rs:29-31`, medium confidence / low severity). "A few megabytes at worst even for pathologically long lines" — 4096 entries × a 4 KB line's key plus its `Rc<[Run]>` partition is closer to 100 MB in the worst case. Still bounded, which is what the constraint requires; only the doc's number is generous.

10. **No test was weakened or deleted** relative to the previous revision (`dcd999d`, recovered from the reflog). The diff adds three tests (`test_capture_colors_are_pinned…`, `test_dw_4_4_a_match_spanning_a_blank_line…`, `test_the_capture_block_still_begins…`), removes none, and leaves `OPTIMIZED_FLOOR = 10` / `UNOPTIMIZED_FLOOR = 3` unchanged. The only deletions are comments and a `match`-on-bool restructured into an `if`. The `#[derive(Default)]` on `HighlightCache` was replaced with a hand-written impl that defaults to the real capacity instead of 0 — a strengthening.

## Issues (FAIL)

1. **The query prompt does not receive `T`, `+`, or `-`; those keys act as chrome instead.**
   - File: `crates/stele/src/main.rs:222-228` and `crates/stele/src/main.rs:298-322`
   - Demonstrated by: reviewer pty probe driving the shipped binary. `/The` leaves the query as `he` and flips the theme (H1 fg `38;2;241;197;197` → `38;2;94;18;18` on the wire); `/plus+` leaves the query as `plus`. Status rows read back off row 24: `["/", "/", "/h  [1/3]", "/he  [1/2]"]`.
   - Impact: `T` opens a huge fraction of capitalized English words, so a great many ordinary queries are silently corrupted *and* re-theme the viewer mid-search. `+`/`-` additionally trigger a full relayout under a half-typed query.
   - Fix: give `handle_chrome_key` the same guard `AppState::handle_key_event` already has — return `false` immediately when `matches!(state.mode(), Mode::Search { .. })`, so the key falls through to the prompt. `Mode` and `AppState::mode()` are already public. Add a test that exercises the `main.rs` routing (not just `AppState::handle_key_event`) with the prompt open — this seam currently has no coverage at all, which is why the defect survived.

**Verdict: FAIL — DW-4.1: typing `T`, `+`, or `-` while the query prompt is open is swallowed by `main.rs::handle_chrome_key` and never reaches the query (demonstrated end-to-end against the real binary). Every other Done-When item, every listed edge case, and all five scrutinised claims verified and passing.**
