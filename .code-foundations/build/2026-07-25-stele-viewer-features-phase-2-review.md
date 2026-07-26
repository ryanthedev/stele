# Review: Phase 2 — document sourcing (`stele -`, `--watch`, parse-once, shared `Rc`)

## Executed Results (Step 0)

Run from `/Users/r/repos/stele/.code-foundations/wave-worktrees/phase-2`.

- Test suite: `cargo test --workspace` → **exit 0**, 0 failed across the workspace. Phase-relevant targets: `stele` lib unit 149 passed; `stele` bin unit 1 passed; `tests/document_source.rs` 6 passed; `tests/cli_errors.rs` 4 passed; `tests/hardening.rs` 4 passed; `tests/pty_pipeline.rs` 4 passed.
- Typecheck / lint: `cargo clippy --workspace --all-targets` → clean, no warnings.
- Format: `cargo fmt --all -- --check` → clean (exit 0).

**The green suite is environment-dependent.** See Issue 1: one of the tests above passes only because the harness this review ran in has no controlling terminal. Re-run under a tty and it hangs.

## Requirement Fulfillment

### DW-2.1
PREMISE:  "`stele -` renders markdown piped on stdin and still responds to keys."
EVIDENCE: `crates/stele/src/cli.rs:96-104` (`-` → `DocumentSource::Stdin`); `crates/stele/src/loader.rs:94-101` (`stdin().lock().read_to_end`); `crates/stele/Cargo.toml:38` (`use-dev-tty`, so keys come off `/dev/tty`); test `crates/stele/tests/document_source.rs:78`.
TRACE:    `stele -` with `# piped-heading\n\nbody from the pipe\n` on a pipe and a pty as ctty → document reaches the screen, status row reads `(stdin)`, `ICANON|ECHO` clear, `q` typed at the tty quits with exactly `RESTORE` and exit 0.
VERDICT:  **PASS** (backed by `test_dw_2_1_piped_stdin_renders_and_still_answers_keys_from_the_tty`, passed in Step 0)

### DW-2.2
PREMISE:  "`--watch` re-renders within one poll interval of an external write, preserving the anchored block."
EVIDENCE: `crates/stele/src/main.rs:39` (`WATCH_POLL_INTERVAL = 250 ms`), `:328-340` (poll-timeout tick), `:278-307` (`Session::poll_reload`); `crates/stele/src/app.rs:286-290` (`reload_document` sets `document_changed`), `:436-443` (anchor path forced). Tests: `tests/document_source.rs:135`, `src/app.rs:1548/1578/1597/1631`, `src/loader.rs:359`.
TRACE (re-render): external `fs::write` with nothing typed → `event::poll(250ms)` times out → `changed_since` true → `load_with` → `reload_media` → `reload_document` → frame on the wire; measured under the test's 2 s budget.
TRACE (anchor, **failing case**): 200-line fence, reader parked on the `AFTER-THE-FENCE` paragraph at line 203 → author prepends a 3-line code fence → reload → reader is at line **4**, top line `"code 0"`. The `NodeId` shift resolves the anchor to the 200-line fence and `line_of` returns its first line. 201 lines of content drift, into a different block.
VERDICT:  **FAIL** — the re-render half holds; "preserving the anchored block" does not for block insertion/removal above the reader. See Issue 2.

### DW-2.3
PREMISE:  "`--watch -` is rejected at CLI parse with a message naming the conflict."
EVIDENCE: `crates/stele/src/cli.rs:96-104` + `:64-81` (`CliError::WatchStdin`, manual `Display`); `crates/stele/src/main.rs:46-52` (checked immediately after `Cli::parse()`, before any read and before terminal entry). Tests: `src/cli.rs:151`, `tests/cli_errors.rs:49`.
TRACE:    `stele - --watch` → `Cli::source()` → `Err(WatchStdin)` → `eprintln!("stele: {err}")` → `ExitCode::FAILURE`. Black-box run asserts exit ≠ 0, stderr contains `--watch` and `stdin`, stdout empty (nothing painted).
VERDICT:  **PASS**

### DW-2.4
PREMISE:  "A deleted or unreadable file under `--watch` shows a status-line error and keeps the last good render instead of exiting."
EVIDENCE: `crates/stele/src/loader.rs:134-149` (unstat-able → `changed_since` returns `true`, so the failure surfaces), `crates/stele/src/main.rs:296-305` (`Err` arm sets a status message and touches no tree). Tests: `tests/document_source.rs:215` and `:264`, `src/loader.rs:381/390`, `src/app.rs:1616`.
TRACE:    file deleted mid-session → next tick `changed_since` = true → `fs::read` errors → status row reads `reload failed: could not read file: …`, row 1 still `still-here`, `child.try_wait()` is `None` (still running), `q` still quits cleanly.
VERDICT:  **PASS**

### DW-2.5
PREMISE:  "A document with no mermaid fences is parsed exactly once at startup (asserted by instrumenting the parse count, not by timing)."
EVIDENCE: `crates/stele/src/loader.rs:175-201` (`PARSE_COUNT` thread-local, `counted_parse`), `crates/stele/src/decor/mermaid.rs:32-38` (`parse` — the load path's only text→`Document` step), `crates/stele/src/loader.rs:110-111` (production call chain), `crates/stele/tests/hardening.rs:139` (bypass guard). Tests: `src/loader.rs:282/302`, `src/decor/mermaid.rs:260`.
TRACE:    `main` → `source.load_with(options)` → `decor::mermaid::parse` → `counted_parse` (+1) → `rendered()` returns `None` → document returned. `parse_count()` delta over a real `DocumentSource::Path(...).load()` of a fence-free file = 1; a renderable `graph TD` fence = 2. The counter is on the production function `main` calls, not a test double.
VERDICT:  **PASS**

### DW-2.6
PREMISE:  "The media sink holds a shared `Rc<Document>`; no full AST clone occurs at startup."
EVIDENCE: `crates/stele/src/loader.rs:56` (`LoadedDocument.doc: Rc<Document>`), `crates/stele/src/media/sink.rs:162/208`, `crates/stele/src/main.rs:69` and `:165` (both `Rc::clone`, never `(*doc).clone()`). Tests: `src/loader.rs:325`, `src/media/sink.rs:2708`, `:2739`.
TRACE:    `Rc::strong_count` = 1 after `load()` → 2 after `GfxMediaSink::new(Rc::clone(&doc), …)` → 1 after `drop(sink)`. A deep copy would leave it at 1 throughout. `Document` does derive `Clone` (`crates/ast/src/lib.rs:42`), so this is a real, not vacuous, guarantee. Grep of `crates/stele/src/` finds no `Document` clone outside `#[cfg(test)]`.
VERDICT:  **PASS**

**All requirements met:** NO (DW-2.2)

## Test-DW Coverage

- [x] Every DW-2.x item has DW-tagged tests that executed in Step 0 (23 `test_dw_2_*` functions across `src/cli.rs`, `src/loader.rs`, `src/app.rs`, `src/decor/mermaid.rs`, `src/media/sink.rs`, `tests/cli_errors.rs`, `tests/document_source.rs`, `tests/hardening.rs`).
- [x] Coverage level (100%) — each DW item is covered by automated tests, not by desk-checking.
- [x] Conventions: sentence-style names, DW tags present, pty harness shared via `tests/common/pty.rs::spawn_viewer`, no assertion on `Run.width` in the new tests.

Gaps (do not by themselves fail the phase; recorded because they weaken the oracles):
- The DW-2.2 pty test asserts the repaint arrived within `ONE_POLL_BUDGET = 2 s` against a 250 ms `WATCH_POLL_INTERVAL` — 8× the requirement's bound, so a regression that made the tick 1 s would still pass.
- The DW-2.2 pty test's reload appends *below* the reader, a case the raw-scroll path would also satisfy; the discriminating assertion lives only in the unit test at `src/app.rs:1548`.
- No test exercises the anchor under block insertion/removal above the reader — the case that fails (Issue 2).

## Dead Code

No unreachable code after early returns; no unused imports; no debug statements; no commented-out blocks. `cargo clippy --all-targets` is clean.

Non-blocking (see Notes): `decor::mermaid::preprocess` (`src/decor/mermaid.rs:46`) now has no production caller — only its own tests — since `parse` took over the load path.

## Correctness Dimensions

| Dimension | Status | Evidence |
|-----------|--------|----------|
| Concurrency | PASS | Single-threaded by construction: `Rc` not `Arc`, `thread_local!` parse counter, `--watch` is a poll on the event loop's own timeout with no watcher thread (`main.rs:328`). No shared mutable state crosses a thread. |
| Error Handling | PASS | Barricade at `DocumentSource::load_with`: I/O and UTF-8 both become a typed `LoadError` with manual `Display` (`loader.rs:22-39`); `poll_reload`'s `Err` arm mutates nothing (`main.rs:296-305`); a failed reload was traced to "last good render survives, process alive". No empty catch, no swallowed reload error. |
| Resources | **See Notes** | No fd/lock leak found — `fs::read` and `stdin().lock()` are scoped; `Rc` graph is acyclic (`Document` holds no back-reference), and `test_a_reload_takes_every_image_off_the_terminal_and_swaps_the_document` proves the old `Rc` drops to count 1 on reload. Unbounded stream read is measured under Notes; not failed because no DW item or listed edge case bounds input size. |
| Boundaries | PASS | Empty document (0 blocks, 0 bytes, 0 lines) traced through load, reload and status; scroll past the new end clamps via `set_scroll`→`max_scroll` (`app.rs:298-300`); `block_span` floors at 1 so `span - 1` cannot underflow; `checked_sub` guards the pre-epoch clock case (`loader.rs:143-148`). |
| Security | PASS | The only new untrusted surface is stdin bytes; they are UTF-8-validated before parse, and fallback text still routes through `painter::sanitize`/`clip_to_width`. No new `unsafe` under `src/**` (asserted by `tests/hardening.rs`). |

## Loaded-Skill Criteria

| Skill | Criterion | Status | Evidence |
|-------|-----------|--------|----------|
| cc-defensive-programming | No executable code inside assertions | PASS | All `assert!`/`assert_eq!` are in `#[cfg(test)]`; no `debug_assert` with side effects in production code. |
| cc-defensive-programming | No empty catch / silently swallowed errors | PASS | Every new error path reports: `main.rs:48-51`, `:63-67`, `:296-305`. The pre-existing `let _ = write!` in `media/sink.rs::write_text_row` is untouched by this phase. |
| cc-defensive-programming | External input validated at the barricade | PASS (with a Note) | `load_with` validates encoding and I/O for both sources at one entry (`loader.rs:89-117`), and the module doc names itself the barricade. No size ceiling on either source — recorded under Notes, not failed, because no requirement or listed edge case names one. |
| cc-defensive-programming | Assertions for bugs; error handling for anticipated runtime conditions | PASS | Missing file / bad UTF-8 / deleted mid-session are all `Result`, never assertions. `changed_since`'s "cannot stat ⇒ report changed" (`loader.rs:134-149`) is the correct fail-loud choice for a viewer. |
| cc-defensive-programming | Correctness-vs-robustness stance suits the domain | PASS | A read-only viewer leans robustness; the reload keeps the last good render rather than exiting, and that trade is stated at `main.rs:270-277`. |
| aposd-designing-deep-modules | Deep interface / information hiding | PASS | `DocumentSource::load`/`load_with` is one method hiding read → UTF-8 → frontmatter → mermaid → parse, with the parse-once rule kept in one place; `MediaSink::reload_document` hides sink-side node invalidation from `main`. |
| aposd-designing-deep-modules | No information leakage across the boundary | PASS | The `-` convention lives only in `cli.rs:96-104` (`main` and `loader` never compare against `"-"`); `Painter::reload_media` keeps the sink un-handed-out. |
| aposd-designing-deep-modules | No pass-through / shallow layers | PASS (with a Note) | `Painter::reload_media` (`painter.rs:125-127`) is a one-line forward, i.e. a pass-through by the letter of the rule, but it buys real encapsulation (no sink accessor) and its doc says so. Not a defect. |
| aposd-designing-deep-modules | No single-use / false generality | PASS (with a Note) | `mermaid::preprocess` is now general-for-nobody: no production caller remains. Recorded under Notes. |
| aposd-designing-deep-modules | No silent failure (failures observable to callers) | PASS | `poll_reload` returns `bool` "repaint needed", surfaces the reason on the status row, and de-duplicates via `last_failure` rather than dropping it. |

## Notes (non-blocking)

| # | Finding | Confidence | Severity | Evidence |
|---|---|---|---|---|
| N1 | **Unbounded stream read.** `DocumentSource::Stdin` calls `read_to_end` with no ceiling. Measured: a 400 MB piped document produced **1.61 GB peak RSS** and 20 s of work before the process even discovered it could not enter raw mode. `fs::read` on the file path has the same shape. No DW item or listed edge case bounds input size, so this is a Note, not a FAIL. | High (measured) | Medium | `loader.rs:94-101`; `/usr/bin/time -l ./target/debug/stele -` on a 400 MB pipe |
| N2 | **DW-2.2 timing assertion is 8× loose.** `ONE_POLL_BUDGET = 2 s` against `WATCH_POLL_INTERVAL = 250 ms`. The test would not catch a poll interval regressed to 1 s. | High | Low | `tests/document_source.rs:37`, `main.rs:39` |
| N3 | **`mermaid::preprocess` has no production caller.** Kept alive only by its own unit tests plus a drift test against `parse`. Public API with zero real callers. | High | Low | `src/decor/mermaid.rs:46`; grep across `crates/` finds callers only inside that file's `#[cfg(test)]` |
| N4 | **Misattached doc comment.** The block at `app.rs:255-285` opens as documentation for `relayout_preserving_anchor` ("The entry point every later phase calls…") and then, without a separator, continues into the `reload_document` text and attaches to `reload_document`. `relayout_preserving_anchor` (`app.rs:292`) is left with no doc comment at all. | High | Low | `crates/stele/src/app.rs:255-296` |
| N5 | **Media placements are dropped outside the synchronized-update block on reload.** `poll_reload` calls `painter.reload_media(...)` (which emits `a=d,d=I` for every live image) *before* `state.reload_document` lays out and before the next frame is painted. For an image-bearing document a reload therefore blanks images, then re-transmits them on the next frame — a visible flicker. It is a deliberate, documented choice (`media/mod.rs:93-97`), and it cannot tear text, so it is not a reload-atomicity defect; it is a polish gap. | High | Low | `main.rs:289-295`, `media/mod.rs:86-101` |
| N6 | **Future-dated mtime causes a permanent reload loop.** `changed_since` compares `mtime > wall_clock_at_since`; a file whose mtime is ahead of the wall clock (clock skew, NFS, `touch -t`) satisfies this on every tick, so the viewer re-parses, re-lays-out and repaints four times a second indefinitely. Not demonstrated end-to-end; derived from the comparison at `loader.rs:143-147`. | Medium | Low | `crates/stele/src/loader.rs:134-149` |
| N7 | **Persistent reload failure stops being visible after 100 frames.** `last_failure` suppresses re-setting the status message, and `STATUS_MESSAGE_TTL_FRAMES` expires the existing one, so after ~100 repaints (keypresses) the error disappears while the file is still missing and never returns. | Medium | Low | `main.rs:298-304`, `app.rs:73`, `app.rs:209-224` |
| N8 | **`--watch` ticks are starved during sustained input.** The watch branch runs only when `event::poll` times out; a held key keeps `poll` returning `true`, so reloads are deferred until the user stops typing. Benign for a reader, but it means "within one poll interval" is conditional on an idle keyboard. | High | Low | `main.rs:328-340` |
| N9 | **`test_a_bare_document_is_moved_into_the_rc_not_copied_into_it` does not test its own claim.** It asserts only that `sink.doc.blocks()` is non-empty, which a deep copy would also satisfy. The real move-vs-copy oracle is the `strong_count` test next to it. | High | Low | `src/media/sink.rs:2723-2732` |
| N10 | **`use-dev-tty` also reaches `crates/probe`.** Cargo unifies features across the workspace, so `probe`'s plain `crossterm = "0.29"` now builds with `use-dev-tty` too. No failure observed (`probe` tests pass), but the backend swap is wider than the Cargo.toml comment implies. | High | Low | `crates/probe/Cargo.toml:14`, `crates/stele/Cargo.toml:38` |
| N11 | **The new transitive dependency pulls in `thiserror 1.0.69` and `winapi`.** `filedescriptor 0.8.3` arrives with `use-dev-tty`. `docs/code-standards.md:69` bans `thiserror` in shipping crates; that rule is about authored code, not transitive deps, so this is informational — but it is now in the shipping binary's tree. | High | Low | `Cargo.lock` diff vs `HEAD~1` |
| N12 | **The `/dev/tty`-unavailable message names raw mode, not the tty.** `stele: could not enter raw mode: Device not configured (os error 6)` is clean, prefixed and non-hanging (verified by observation), but it does not tell the user that the problem is a missing controlling terminal. Edge case is satisfied; wording could be better. | High | Low | Observed: `printf '…' \| ./target/debug/stele -` with no ctty → exit 1 in ~0 s |

### Verified claims (recorded so they are not re-litigated)

- **`use-dev-tty` is genuinely required, not a preference.** I removed the feature, rebuilt, and ran `cargo test -p stele --test document_source`: `test_dw_2_1_piped_stdin_renders_and_still_answers_keys_from_the_tty` **failed** (child dead, `write` to the pty master returned EIO), while the other five pty tests passed. The Cargo.toml comment's mechanism claim is consistent with the observed failure. Worktree restored to pristine afterwards (`git status` clean).
- **No regression on the ordinary file-argument path.** All non-stdin pty tests (`document_source.rs` ×5, `pty_pipeline.rs` ×4, `cell_geometry_query.rs` ×6) pass with the feature enabled.
- **The parse counter measures production, not a test path.** `main` → `load_with` → `mermaid::parse` → `counted_parse`; `tests/hardening.rs:139` walks the four load-path sources and fails on any direct `Document::parse` above their `#[cfg(test)]` marker.
- **No reference cycle and no stale-document read after reload.** `Rc::ptr_eq(&sink.doc, &replacement)` and `Rc::strong_count(&old) == 1` are both asserted at `src/media/sink.rs:2786-2790`.
- **A failed reload cannot leave a partially-swapped state.** `load_with` returns `Err` before `Session::doc`, `loaded_at`, the sink or `AppState` are touched (`main.rs:288-306`); the parse completes fully before any swap. Confirmed behaviourally by the deleted-file and reappearing-file pty tests.
- **Edge case: `--watch` + `-`** — rejected at parse (DW-2.3, PASS). **File deleted/replaced mid-session** — PASS. **Truncated to empty** — PASS (`test_a_watched_file_truncated_to_empty_repaints_instead_of_crashing`). **Scrolled past the new end** — PASS (`test_dw_2_2_a_reload_past_the_new_end_clamps_instead_of_dangling`). **stdin not a terminal and `/dev/tty` unavailable** — PASS by observation: clean `stele:`-prefixed message, exit 1, no hang.

## Issues (FAIL)

### 1. `tests/cli_errors.rs` hangs the suite — and hijacks the user's terminal — whenever the test process has a controlling terminal

- **File:** `crates/stele/tests/cli_errors.rs:74` (`test_dw_2_1_stdin_alone_is_accepted_and_reads_the_pipe`)
- **Demonstrated by:** ran the compiled `cli_errors` test binary under a real pty (`script -q /dev/null <bin> --exact test_dw_2_1_stdin_alone_is_accepted_and_reads_the_pipe`). The process was still alive after 25 s and had to be killed; a 60 s run produced only `running 4 tests` / the test's name and never completed. The same binary finishes instantly when the harness has no controlling terminal — which is why `cargo test --workspace` was green in Step 0.
- **Root cause traced:** the test comment assumes "no controlling terminal here, so the viewer fails at `enter raw mode` rather than rendering." That assumption holds only for the no-ctty case. With `use-dev-tty` enabled (this phase's change), `crossterm::terminal::size()` and raw-mode entry resolve against `/dev/tty` — the developer's own terminal — so the child succeeds, enters raw mode and the alternate screen, and blocks forever in `event::read()`. The parent's `child.wait_with_output()` never returns.
- **Confirmed to be caused by this phase's dependency change:** I rebuilt with `use-dev-tty` removed and re-ran the identical binary under `script`; it **finished** rather than hanging. With the feature, it hangs. This is precisely the "a test passes only because of a behavior difference between the two input backends" risk — the test passes on the no-ctty backend behaviour and hangs on the new one.
- **Blast radius beyond the hang:** the child leaves the developer's real terminal in raw mode and the alternate screen while it is wedged.
- **Fix:** do not let this test's child reach terminal entry. Either give it a pty of its own via the existing `common::pty::spawn_viewer(..., ChildStdin::Pipe(..))` harness (which already handles `setsid`/`TIOCSCTTY` and asserts the real DW-2.1 behaviour in `document_source.rs:78`), or force the no-ctty path deterministically (e.g. a `pre_exec` `setsid()` on the child so `/dev/tty` cannot resolve) and then assert the exact message and exit status rather than only two negatives. Add a wall-clock bound so a future regression fails instead of hanging.

### 2. DW-2.2's "preserving the anchored block" does not hold when a block is inserted or removed above the reader — measured drift up to 201 lines into a different block

- **File:** `crates/stele/src/app.rs:286-290` (`reload_document`) and `:482-491` (`line_of`), via the positional `NodeId` anchor at `:448-456`.
- **Demonstrated by** a probe test I wrote and ran against the real `AppState::reload_document`:

  ```
  assertion `left == right` failed: REPRO: reader was on AFTER-THE-FENCE (line 203);
  after the reload they are on line 4 showing "code 0"
    left: "code 0"
   right: "AFTER-THE-FENCE"
  ```

  Fixture: `intro` / a 200-line code fence / the paragraph `AFTER-THE-FENCE` / 60 tail paragraphs. Reader parked on `AFTER-THE-FENCE`. The author prepends a three-line code fence to the top of the file — an ordinary `--watch` edit — and the reload lands the reader on `code 0`, 201 lines of content backwards, inside a different block.
- **Mechanism:** a `NodeId` is a position in the re-parsed node stream, so prepending a block shifts every id. `Anchor.block` then names a *different* source block; because `first_line_of` still returns `Some` for it, the ratio fallback at `app.rs:441` never runs and the wrong answer is taken as authoritative.
- **Magnitude is shape-dependent**, measured across insertion shapes on the same fixtures (content-relative drift, negative = backwards):

  | Insertion above the reader | Drift |
  |---|---|
  | one paragraph (flat document) | −2 lines |
  | one paragraph / one heading / two paragraphs (fence fixture) | −1 line |
  | a 3-item bullet list (flat document) | −10 lines |
  | **a small code fence (fence fixture)** | **−201 lines** |
  | one paragraph *deleted* above (flat document) | +1 block |

- **Why this is a FAIL and not a Note.** The code documents the limitation (`app.rs:275-285`) and characterises it as "The reader lands **near** where they were rather than exactly on it." The measurement contradicts that characterisation: 201 lines and a different block is not "near", and the shape that triggers it (a code fence added above what you are reading, under `--watch`) is a first-order use of the feature, not an exotic edge. The requirement's words are "preserving the anchored block", and the anchored block is not preserved. This is a demonstrated wrong result against a DW item, not an unlisted edge case or a style opinion.
- **Fix (either is sufficient for the requirement's substance):**
  1. Sanity-check the resolved anchor before trusting it — e.g. reject `line_of`'s answer and fall back to the proportional ratio when the resolved block's span differs from `Anchor.span` by more than some factor, or when the resulting line is further from the ratio estimate than the old viewport height. That bounds the damage without a search.
  2. Make the anchor content-addressed rather than positional (hash the anchored block's source text, or carry its `span` byte offset from `Block::span`, which the mermaid preprocessor already relies on at `decor/mermaid.rs:76`) so a shifted ordinal cannot silently resolve to a different block.

  Whichever is chosen, add the reproduction above as a DW-2.2 test — the current DW-2.2 tests cover in-place growth and append-below only, neither of which shifts a `NodeId`.

**Verdict: FAIL** — blockers: (1) `tests/cli_errors.rs:74` hangs the suite and takes over the terminal when a controlling tty is present, a direct consequence of this phase's `use-dev-tty` switch; (2) DW-2.2's anchor preservation fails for block insertion/removal above the reader, demonstrated at 201 lines of drift into a different block.

Everything else in the phase verified clean: DW-2.1, DW-2.3, DW-2.4, DW-2.5 and DW-2.6 all pass with execution evidence, every listed edge case is handled, `use-dev-tty` is confirmed load-bearing rather than incidental, the parse counter measures the real production path, and the `Rc` share is genuine with no cycle and no stale read after reload.
