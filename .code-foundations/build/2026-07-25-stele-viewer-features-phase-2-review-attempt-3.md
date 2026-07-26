# Review: Phase 2 — Document sourcing (`stele -`, `--watch`), attempt 3

Worktree: `/Users/r/repos/stele/.code-foundations/wave-worktrees/phase-2` @ `7e97026 wip(phase-2): document sourcing`

## Executed Results (Step 0)

| Command | Result |
|---|---|
| `script -q /dev/null cargo test --workspace` | exit 0 — **440 passed, 0 failed, 5 ignored** (the 5 are pre-existing `#[ignore]`s requiring a live Ghostty GUI, plus `ast::conformance::dump_failures`) |
| `cargo clippy --workspace --all-targets` | clean, no warnings |
| `cargo fmt --all -- --check` | clean |

The full pass was run **from inside a controlling terminal** (`script -q /dev/null`). It completed in ~90 s, never blocked, and left the invoking terminal's line discipline intact — the first review's "the suite can capture a real terminal" blocker is fixed and stays fixed.

## Requirement Fulfillment

### DW-2.1
PREMISE:  `stele -` renders markdown piped on stdin and still responds to keys.
EVIDENCE: `crates/stele/src/cli.rs:96-104` (`-` → `DocumentSource::Stdin`); `crates/stele/src/loader.rs:146` (`read_bounded(std::io::stdin().lock())`); `crates/stele/Cargo.toml:25-37` (`crossterm` `use-dev-tty`, which is what makes key reads fall to `/dev/tty` when stdin is a pipe).
TRACE:    `stele -` with `# piped-heading\n\nbody from the pipe\n` on a pipe → `Cli::source()` → `DocumentSource::Stdin` → bytes read from the pipe → parsed → painted; `q` typed at the pty master → `event::read()` from `/dev/tty` → `handle_key_event` → quit + exact restore sequence, exit 0.
VERDICT:  **PASS** — `document_source.rs::test_dw_2_1_piped_stdin_renders_and_still_answers_keys_from_the_tty` (ran, ok) asserts the heading, the body, the `(stdin)` status name, that raw mode is on, that `q` produces exactly `RESTORE`, and that the line discipline comes back. `cli_errors.rs::test_dw_2_1_a_bare_dash_reads_the_document_from_stdin_not_from_a_file` (ran, ok) pins the routing half.

### DW-2.2
PREMISE:  `--watch` re-renders within one poll interval of an external write, preserving the anchored block.
EVIDENCE: `crates/stele/src/main.rs:284-306` (`until_next_tick` / `watch_tick_due`), `:318-347` (`poll_reload`), `:432-434` (tick checked outside the event branch); `crates/stele/src/app.rs:599-612` (`line_of_reloaded`).
TRACE (idle / under keys): external write → mtime newer than `loaded_at` → `load_with` → new tree laid out fully → `state.reload_document` → single assignment → repaint. Measured independently: idle reload landed **0.29 s** after the write (my own pty probe); the repo's `test_dw_2_2_a_reload_lands_while_keys_are_arriving_continuously` (ran, ok) proves the same under continuous `j`/`k`.
TRACE (resize storm): external write → SIGWINCH stream at 100 Hz → `run_session` enters `while event::poll(RESIZE_DEBOUNCE).unwrap_or(false)` at `main.rs:395` → every 50 ms window is refilled by the next resize → the loop never exits → `watch_tick_due()` at `:432` is never reached → **no reload and zero bytes painted for 6+ s**.
VERDICT:  **FAIL** — the bound holds when idle and under key autorepeat, and fails under a sustained resize stream. See Issue 1.

### DW-2.3
PREMISE:  `--watch -` is rejected at CLI parse with a message naming the conflict.
EVIDENCE: `crates/stele/src/cli.rs:96-104`, `:64-81` (`CliError::WatchStdin` + manual `Display`); `crates/stele/src/main.rs:53-59` (rejected before any read and before the terminal is touched).
TRACE:    `stele - --watch` → `Cli::source()` → `file == "-"` and `watch` → `Err(WatchStdin)` → `eprintln!("stele: --watch cannot be combined with \`-\`: stdin is a stream read once to end…")` → `ExitCode::FAILURE`, stdout empty.
VERDICT:  **PASS** — `cli.rs::test_dw_2_3_watch_with_stdin_is_rejected_naming_both_flags` and `cli_errors.rs::test_dw_2_3_watch_with_stdin_exits_nonzero_before_touching_the_terminal` both ran, ok; the latter also asserts stdout is empty, i.e. nothing was painted.

### DW-2.4
PREMISE:  A deleted or unreadable file under `--watch` shows a status-line error and keeps the last good render instead of exiting.
EVIDENCE: `crates/stele/src/loader.rs:185-187` (an unstat-able path reports *changed*, so the failure is surfaced rather than swallowed); `crates/stele/src/main.rs:337-345` (failure sets a status message and touches no tree; `loaded_at` deliberately not advanced).
TRACE:    file deleted → `changed_since` → `true` → `load_with` → `Err(Io)` → `set_status("reload failed: could not read file: …")` → repaint of the **same** tree with the new status row; process still alive; `q` still quits.
VERDICT:  **PASS** — `document_source.rs::test_dw_2_4_a_deleted_file_reports_on_the_status_row_and_keeps_the_last_frame` (ran, ok) asserts all three: the message, that row 1 still reads `still-here`, and `try_wait().is_none()`. `test_a_watched_file_that_reappears_is_reloaded` (ran, ok) covers the file coming back.

### DW-2.5
PREMISE:  A document with no mermaid fences is parsed exactly once at startup (asserted by instrumenting the parse count, not by timing).
EVIDENCE: `crates/stele/src/loader.rs:220-246` (thread-local `PARSE_COUNT`, `counted_parse`); `crates/stele/src/decor/mermaid.rs` `parse()` reuses the fence-scanning parse when `rendered()` returns `None`.
TRACE:    fence-free source → `counted_parse` (count +1) → `rendered()` finds no replacements → `None` → the same `Document` is returned. Delta = 1. A renderable fence splices and re-parses: delta = 2.
VERDICT:  **PASS** — `loader::tests::test_dw_2_5_a_document_without_mermaid_is_parsed_exactly_once` (delta == 1) and `…_a_renderable_mermaid_fence_costs_exactly_one_extra_parse` (delta == 2) both ran, ok. `hardening.rs::test_dw_2_5_the_load_path_never_calls_document_parse_directly` (ran, ok) is what keeps the counter an honest oracle — without it the count could be bypassed. Good work: the negative half (a fence *does* cost two) is what stops "parsed once" degenerating into "never parses twice".

### DW-2.6
PREMISE:  The media sink holds a shared `Rc<Document>`; no full AST clone occurs at startup.
EVIDENCE: `crates/stele/src/loader.rs:100-106` (`LoadedDocument.doc: Rc<Document>`); `crates/stele/src/main.rs:76` and `:172-174` (`Rc::clone` into the sink); `crates/stele/src/media/sink.rs:162` (`doc: Rc<Document>`).
TRACE:    startup → one `Rc::new(doc)` → `Rc::clone` to `Session` → `Rc::clone` to `GfxMediaSink` → three handles, one allocation. Verified independently: every `doc.clone()` in `crates/stele/src` is below a `#[cfg(test)]` marker (`sink.rs:976`, `sizer.rs:218`), so no production path deep-copies the AST.
VERDICT:  **PASS** — `loader::tests::test_dw_2_6_the_sink_shares_the_loaded_document_rather_than_cloning_it` (ran, ok) asserts `Rc::strong_count` 1 → 2 → 1 across the sink's lifetime, which a deep copy would leave at 1.

**All requirements met:** NO — DW-2.2 fails under a resize storm.

## Test-DW Coverage

- [x] Every DW item has at least one automated test that ran in Step 0, and DW-2.1/2.2/2.4 are additionally proved black-box through the real binary on a real pty.
- [x] Tests are DW-tagged and sentence-named per `docs/code-standards.md`.
- [x] Coverage matches the 100% level for the *stated* conditions.
- [ ] **Gap:** no automated test exercises the resize-storm condition for DW-2.2, which is why the defect below survived two fix rounds. The starvation test that exists (`test_dw_2_2_a_reload_lands_while_keys_are_arriving_continuously`) covers keys only.
- [ ] **Gap (minor):** the "`/dev/tty` unavailable" edge case has no automated test. I verified it by observed behavior instead (below).

## Dead Code

None found. No unused imports (clippy `--all-targets` is clean), no unreachable code after early returns, no `dbg!`/`todo!`/commented-out call sites in the diff. The `eprintln!`s in `main.rs` are the intended user-facing error path.

## Correctness Dimensions

| Dimension | Status | Evidence |
|-----------|--------|----------|
| Concurrency | **FAIL** | Not thread concurrency — event-loop scheduling. The inner resize-debounce loop starves the watch tick indefinitely under a ≥20 Hz resize stream. Demonstrated below. |
| Error Handling | PASS | `LoadError` (`loader.rs:25-50`) is a hand-rolled enum with manual `Display` and no raw `Debug` leakage (`test_dw_5_4_*` assert `!contains("Os {")`). Reload failures degrade to "keep the last good render + say why", never an exit. `changed_since` returning `true` for an unstat-able path (`loader.rs:185-187`) is the correct direction — it converts a silent failure into a reported one. |
| Resources | PASS | `read_bounded` (`loader.rs:75-87`) uses `take(limit+1)` so an oversized source costs one extra byte, not all of it; the `File` is consumed by the read and dropped. `Rc` (not `Arc`) is correct for a single-threaded painter. Idle CPU under `--watch` measured at **0.0%** over 10 s — the 250 ms poll is a real sleep, not a spin. Memory under a 6 s / 600-event resize storm: RSS flat at ~10 MB, so the unbounded `sizes` accumulation is not a leak in practice. |
| Boundaries | PASS | `block_span` (`app.rs:634-640`) returns `.max(1)`, which is what makes `span - 1` in `place` (`:623`) and the `/ anchor.span` division (`:621`) safe. Empty document → `anchor()` returns `None` → proportional fallback, no panic. Size ceiling is exact on both sides (`test_a_document_exactly_at_the_size_ceiling_still_loads` / `…past_the_size_ceiling_is_refused`). |
| Security | PASS | Both untrusted sources go through one barricade: bounded read (64 MiB), UTF-8 validation, then parse. `-` is only the stdin convention as a whole argument (`cli.rs:97`), so `-notes.md` is a file. Alt text and TeX are `sanitize`d before reaching the terminal. |

### The resize-storm defect, demonstrated

`crates/stele/src/main.rs:395`:

```rust
while event::poll(RESIZE_DEBOUNCE).unwrap_or(false) {
    match event::read() {
        Ok(Event::Resize(w, h)) => sizes.push(Size { width: w, height: h.saturating_sub(1) }),
        …
    }
}
```

`RESIZE_DEBOUNCE` is a **fixed** 50 ms and is re-armed by every arriving resize. `Session::until_next_tick()` shrinks correctly, but it governs only the *outer* poll; this inner loop is outside its reach, so a steady resize stream holds the loop open for as long as it lasts and `session.watch_tick_due()` at `:432` is never reached.

My own pty probe (child spawned on a private pty with `setsid` + `TIOCSCTTY`, file written externally, wire read continuously):

| Resize interval | Reload seen during the storm? | Time to marker | Bytes painted during 6 s |
|---|---|---|---|
| none (idle) | yes | **0.29 s** | — |
| 10 ms (~100 Hz) | **no** | >6 s (gave up) | **0** |
| 40 ms (~25 Hz) | **no** | >6 s (gave up) | **0** |
| 70 ms (~14 Hz) | yes | **0.06 s** | 1631 |

The threshold is exactly `RESIZE_DEBOUNCE`. Below it the viewer emits *nothing at all* — it is not merely the reload that is starved, it is every frame. It recovers as soon as the storm stops. A sustained ≥20 Hz resize stream is what a GUI terminal produces for the whole duration of a window drag.

## Loaded-Skill Criteria

| Skill | Criterion | Status | Evidence |
|-------|-----------|--------|----------|
| cc-defensive-programming | External input validated at entry | PASS | `DocumentSource::load_with` (`loader.rs:137-162`) is the single barricade for both untrusted sources: size ceiling → UTF-8 → preprocess → parse. Nothing downstream re-reads bytes. |
| cc-defensive-programming | Barricade design (validate outside, assume inside) | PASS | The module doc (`loader.rs:1-9`) states the barricade explicitly and the code honours it — `app.rs` and `painter.rs` never touch raw bytes. CLI-level contradictions are rejected before the barricade is even reached. |
| cc-defensive-programming | No empty catch blocks / silent swallowing | PASS (1 note) | `event::poll(RESIZE_DEBOUNCE).unwrap_or(false)` is a documented degradation, not a swallow. `let _ = write!` in `sink.rs::write_text_row` does discard write errors, but it is pre-existing and outside this phase's diff — Note only. |
| cc-defensive-programming | Assertions for bugs only; no executable code in assertions | PASS | No `assert!`/`debug_assert!` in the phase's production code; all runtime conditions are `Result`-handled. |
| cc-defensive-programming | Correctness-vs-robustness strategy fits the domain | PASS | A viewer should lean robust: a failed reload keeps the last good render and reports on the status row rather than exiting (`main.rs:337-345`) — the "return previous answer + display error message" strategy, correctly chosen and documented. |
| aposd-designing-deep-modules | Deep interface / information hiding | PASS | One method (`load_with`) hides read + ceiling + UTF-8 + frontmatter + mermaid + parse, and its doc comment states *why* the pipeline may not be reassembled by callers. `LoadError` carries the limit rather than a pre-formatted string, keeping the message decision with the caller. |
| aposd-designing-deep-modules | Silent Failure red flag | PASS (1 note) | Reload failure is observable on the status row. One narrow gap: a *persistent* failure whose message has aged out of the 100-frame TTL is never re-shown (see Notes). |
| aposd-designing-deep-modules | Information leakage | PASS | The `-` convention lives only in `cli.rs:97` (asserted by `test_any_other_path_names_a_file_source_dash_or_not`); neither `main` nor the loader compares a path to `"-"`. |
| aposd-designing-deep-modules | Shallow module / pass-through layer | PASS (note) | `Painter::reload_media` is a one-line forward to the sink, but the justification given (the painter owns the sink; exposing it would let a caller paint outside a frame) is sound. |
| aposd-designing-deep-modules | Temporal decomposition | N/A | `Session` groups by what outlives a document, not by execution order. |

## Notes (non-blocking)

| # | Finding | Confidence | Severity |
|---|---|---|---|
| 1 | **Edge case verified by observed behavior, not by a test.** `stele -` with stdin a pipe and no controlling terminal (`os.setsid()`, stdout a pipe) exits in 0.01 s, rc=1, stderr `stele: could not enter raw mode: Device not configured (os error 6)`. Clear message, no hang — the case is **handled**, but nothing pins it. | High | Low |
| 2 | **One assertion dropped without counterpart** in the loader test rewrite: `HEAD~1`'s `test_valid_utf8_file_loads_successfully` asserted `content == "# Hello\n\nWorld.\n"`. Its successor asserts `byte_size`, `line_count` and `blocks().len()` but never that the loaded text equals the file's bytes. The API change (`String` → `Rc<Document>`) makes the old form impossible, and the pty tests assert real file text reaching the screen, so the guarantee is not lost — only moved. | High | Low |
| 3 | **The duplicated-content replacement did drop one assertion**, contrary to "no assertion silently dropped": the old `assert_eq!(ordinal_after, ordinal_before + 1, …)`. It could not survive — `ordinal_of` was removed in favour of `occurrence_of(first, fingerprint)`. The new `marker_above` oracle identifies the copy by content rather than by index arithmetic and sweeps 9 configurations against the old 1, so the replacement is genuinely stronger; the claim's wording is what is imprecise, not the test. | High | Low |
| 4 | **Two arithmetic claims in the discovery doc do not match the code.** The new test's doc comment and `discovery.md:122` say "10 of these **15** combinations", but the loop enumerates 3 × 3 = **9**. `discovery.md:124` says "**16** call sites"; the true count is **11** repointed (`grep -c` returns 16 because it counts the definition line plus 15 calls). | High | Low |
| 5 | **Helper consolidation is semantically clean.** The deleted `top_line_text` and the surviving `topmost_line_text` → `line_text` pair are observationally identical on every input: same line selected, same range, same `Run::text`-only extraction, same `""` for `Line::Reserved` and out-of-range, neither reads `Run.width`. No timeout, env var, argument or assertion changed. | High | — |
| 6 | **Residual duplication the consolidation did not reach.** `FRAME_END` is now defined twice with the same value (`tests/common/pty.rs:219` and `tests/tmux_graphics.rs:156`, the latter still using its local copy). Three inline `Command::new(env!("CARGO_BIN_EXE_stele"))` spawn sites remain alongside `spawn_viewer` (`common/pty.rs:360`, `panic_mid_frame.rs:45`, `tmux_graphics.rs:180`), with divergent `TERM_PROGRAM` handling. None were touched by this commit. | High | Low |
| 7 | **A persistent reload failure goes quiet after 100 repaints.** `last_failure` (`main.rs:339-341`) suppresses a repeat of an identical message, but `STATUS_MESSAGE_TTL_FRAMES` ages the message out after 100 `status()` calls. A reader who scrolls 100 times with the file still missing sees the ruler come back and is never told again. | Medium | Low |
| 8 | **`drain_for` polls at 10 ms** where the two pre-existing loops (`read_until`, `drain_quiet`) use 50 ms. Deliberate for a tight drain-while-typing loop, but it is the one numeric divergence introduced into the shared harness. | High | Low |
| 9 | **Removing the `NodeId` fast path costs nothing measurable.** On `testdocs/08-scroll-10k.md` (10008 tree lines), with the reader parked mid-document: whole-document anchor scan ≈ **18 µs release** / 4.5 ms debug, against `layout()` itself at 11.7 ms release / 69 ms debug and `Document::parse` at 5.3 ms / 36.9 ms. The scan is ~0.15% of a reload. The design decision to keep one always-correct path is well supported by the numbers. | High | — |
| 10 | **Anchor correctness independently confirmed** on my own fixtures (written from scratch, asserted on painted row text, since deleted): insertion above the reader at 1/5/50/**500** blocks; deletion of 1/5/30 blocks above; duplicated blocks at gap ∈ {1,2,25,60} × insertion ∈ {1,7,40} with the copy identified by the unique marker above it (12 combinations, all landed on the *same* copy); the anchored block edited; the anchored block deleted; a document where **every** block is identical (append below did not move the reader; prepend stayed on a valid line, no panic); and reload while scrolled past the new end (clamped to 0). All passed. Both first-review blockers are fixed. | High | — |

## Issues (FAIL)

1. **`--watch` reload latency is unbounded under a sustained resize stream — DW-2.2's own bound.**
   - File: `crates/stele/src/main.rs:395` (`while event::poll(RESIZE_DEBOUNCE).unwrap_or(false)`), starving the tick check at `crates/stele/src/main.rs:432`.
   - Demonstrated by: a pty probe I wrote (child on a private pty via `setsid` + `TIOCSCTTY`; `TIOCSWINSZ` toggled 80↔79 cols on a timer; file rewritten externally with a marker; master read continuously). At 10 ms and 40 ms resize intervals the marker never appeared within 6 s and **zero bytes** reached the wire; at 70 ms it appeared in 0.06 s; idle, 0.29 s. Threshold is exactly `RESIZE_DEBOUNCE` = 50 ms.
   - Why it matters beyond the letter of DW-2.2: scrutiny item 1 asks specifically whether "the loop's shrinking poll timeout cannot be reset indefinitely by a steady event stream." For the outer loop the answer is yes, and that fix is real and well tested. For this inner loop the answer is **no**. The fix addressed keys and non-key events and left the resize path untouched.
   - Provenance, stated plainly: the drain loop itself predates this phase (it is present at `HEAD~1`). What is new is the DW-2.2 guarantee layered on top of it, and the claim that the tick is now independent of input arrival. It is independent of *key* arrival, not of *resize* arrival.
   - Fix: bound the drain by a deadline rather than by inter-event silence — e.g. compute `let drain_until = Instant::now() + RESIZE_DEBOUNCE;` before the loop and poll on `drain_until.saturating_duration_since(Instant::now())`, additionally capped by `session.until_next_tick()` when `session.watch` is set, so the loop cannot outlive either budget. Add a resize-storm test alongside `test_dw_2_2_a_reload_lands_while_keys_are_arriving_continuously`; the pty harness in `tests/common/pty.rs` already has everything needed except a `TIOCSWINSZ` helper on `Pty`.

**Verdict: FAIL — blocker: DW-2.2's one-poll-interval bound does not hold under a sustained resize stream faster than the 50 ms debounce (`main.rs:395`); measured at 0 repaints and 0 bytes over 6 s, versus 0.29 s idle. DW-2.1, 2.3, 2.4, 2.5, 2.6 all PASS with execution evidence, and both first-review blockers are confirmed fixed.**
