# Review: Phase 2 — document sourcing (attempt 2)

Worktree: `/Users/r/repos/stele/.code-foundations/wave-worktrees/phase-2`, at `119e0f0`
(single wip commit; prior revision of the same commit is `c61bc5d`, pre-amend).

## Executed Results (Step 0)

| Command | Result |
|---|---|
| `script -q /dev/null cargo test --workspace` | exit 0 — **439 passed, 0 failed** |
| `cargo test -p stele` (in a controlling terminal) | 155 lib + all integration targets pass |
| `cargo clippy --workspace --all-targets` | exit 0, zero warnings |
| `cargo fmt --all -- --check` | exit 0 |

The full-workspace run was executed from inside a controlling terminal (`script -q /dev/null`),
terminated on its own, and left the terminal usable — every subsequent command in the same
session ran normally.

## Requirement Fulfillment

### DW-2.1 — `stele -` renders markdown piped on stdin and still responds to keys
PREMISE:  "`stele -` renders markdown piped on stdin and still responds to keys."
EVIDENCE: `crates/stele/src/cli.rs:96-104` (`-` → `DocumentSource::Stdin`);
          `crates/stele/src/loader.rs:146` (`read_bounded(std::io::stdin().lock())`);
          `crates/stele/Cargo.toml:35` (`crossterm` `use-dev-tty`, so keys resolve
          against `/dev/tty` when stdin is a pipe);
          `crates/stele/tests/document_source.rs:78-125`.
TRACE:    `argv = ["stele","-"]` + document on the pipe → `Cli::source()` → `Stdin` →
          `read_bounded(stdin)` → parse → `TerminalGuard::enter` → the pty shows
          `piped-heading`, `body from the pipe` and `(stdin)` in the status row; `ICANON|ECHO`
          are cleared; `q` typed **at the tty** exits 0 emitting exactly `RESTORE`.
VERDICT:  **PASS** — `test_dw_2_1_piped_stdin_renders_and_still_answers_keys_from_the_tty`
          (pty, ran and passed), plus `test_dw_2_1_a_bare_dash_reads_the_document_from_stdin_not_from_a_file`,
          `test_dw_2_1_a_bare_dash_names_the_stdin_source`, `test_dw_2_1_stdin_source_names_itself_and_reports_no_change`.

### DW-2.2 — `--watch` re-renders within one poll interval of an external write, preserving the anchored block
PREMISE:  "`--watch` re-renders within one poll interval of an external write, preserving the
          anchored block."
EVIDENCE: `crates/stele/src/main.rs:328-340` (the poll/reload branch);
          `crates/stele/src/app.rs:557-609` (`line_of_reloaded` / `place`).
TRACE 1 (idle):   external write → `event::poll(250 ms)` times out → `changed_since` true →
          `load_with` → `reload_document` → repaint. Observed on the pty in
          `test_dw_2_2_an_external_write_repaints_within_a_poll_interval_and_keeps_the_top_block`.
TRACE 2 (input pending, **fails**): `session.watch && !event::poll(250 ms)` — `poll_reload`
          runs **only when `poll` times out**. With a key pending on every iteration the
          reload branch is never entered. Measured on a pty: keys fed at ~14/s (macOS
          autorepeat), file rewritten with a marker → **the marker never appeared in 8.03 s**.
TRACE 3 (duplicated content, **fails**): reader on the 21st of 30 identical blocks;
          one paragraph prepended. `line_of_reloaded` step 1 misses (the shifted `NodeId`
          names a different-fingerprint block), step 2 picks
          `min_by_key(|ordinal| ordinal.abs_diff(anchor.ordinal))` — but every candidate's
          ordinal has shifted by the insertion, and the comparison is against the *unshifted*
          old ordinal. The nearest match is the copy **before** the reader's. Painted window
          became `["duplicated block text","","unique-020",…]` where it was
          `[…,"unique-021",…]`.
VERDICT:  **FAIL** — see Issues 1 and 2.

### DW-2.3 — `--watch -` is rejected at CLI parse with a message naming the conflict
PREMISE:  "`--watch -` is rejected at CLI parse with a message naming the conflict."
EVIDENCE: `crates/stele/src/cli.rs:96-104`, message at `cli.rs:74-79`;
          `crates/stele/src/main.rs:46-52` (before any read, before the terminal).
TRACE:    `argv = ["stele","-","--watch"]` → `Cli::source()` → `Err(CliError::WatchStdin)` →
          `eprintln!("stele: --watch cannot be combined with `-`: stdin is a stream read once
          to end, not a file whose changes can be watched")` → `ExitCode::FAILURE`, stdout empty.
VERDICT:  **PASS** — `test_dw_2_3_watch_with_stdin_exits_nonzero_before_touching_the_terminal`
          (black-box, asserts empty stdout so nothing was painted) and
          `test_dw_2_3_watch_with_stdin_is_rejected_naming_both_flags`.

### DW-2.4 — A deleted or unreadable file under `--watch` shows a status-line error and keeps the last good render instead of exiting
PREMISE:  verbatim above.
EVIDENCE: `crates/stele/src/loader.rs:185-187` (unstat-able ⇒ "changed", so the failure is
          surfaced rather than swallowed); `crates/stele/src/main.rs:296-305` (`Err` arm sets
          the status message and touches **no** tree).
TRACE:    `rm file.md` → `changed_since` true → `load_with` → `Err(Io)` →
          `state.set_status("reload failed: could not read file: …")` → repaint. Status row
          read off the pty contains `reload failed` and `could not read file`; row 1 still
          reads `still-here`; `child.try_wait()` is `None`; `q` afterwards still exits 0.
VERDICT:  **PASS** — `test_dw_2_4_a_deleted_file_reports_on_the_status_row_and_keeps_the_last_frame`
          (pty), `test_a_watched_file_that_reappears_is_reloaded`,
          `test_dw_2_4_a_missing_path_reports_changed_so_the_failure_is_seen`,
          `test_dw_2_4_a_file_deleted_after_a_good_load_reports_changed_then_fails`.

### DW-2.5 — A document with no mermaid fences is parsed exactly once at startup (instrumented, not timed)
PREMISE:  verbatim above.
EVIDENCE: `crates/stele/src/loader.rs:220-246` (thread-local `PARSE_COUNT`, `counted_parse`);
          `crates/stele/src/decor/mermaid.rs:32-38` (`parse` reuses the doc it already parsed
          to find fences); `crates/stele/tests/hardening.rs:139-200` (source-level guard that
          no load-path file calls `Document::parse` directly, so the counter is the whole truth).
TRACE:    `"# Title\n\nA paragraph.\n\n```rust…```"` → `counted_parse` (+1) → `rendered()`
          returns `None` → doc returned. `parse_count()` delta = **1**. A renderable
          `graph TD` fence gives delta **2** (the spliced text is different text).
VERDICT:  **PASS** — `test_dw_2_5_a_document_without_mermaid_is_parsed_exactly_once`,
          `test_dw_2_5_a_renderable_mermaid_fence_costs_exactly_one_extra_parse`,
          `test_dw_2_5_a_fence_free_document_is_parsed_once_and_a_rendered_one_twice`,
          `test_dw_2_5_the_load_path_never_calls_document_parse_directly`.
          I independently confirmed the guard's premise: every `Document::parse` in
          `crates/stele/src` outside `counted_parse` is inside a `#[cfg(test)]` module.

### DW-2.6 — The media sink holds a shared `Rc<Document>`; no full AST clone occurs at startup
PREMISE:  verbatim above.
EVIDENCE: `crates/stele/src/media/sink.rs:157` (`doc: Rc<Document>`), `sink.rs:203-212`
          (`impl Into<Rc<Document>>`, no `Rc::new` on a clone);
          `crates/stele/src/loader.rs:104` (`LoadedDocument.doc: Rc<Document>`);
          `crates/stele/src/main.rs:69` and `main.rs:165` (`Rc::clone`, both).
TRACE:    `load` → `Rc::new(doc)`, count 1 → `main` `Rc::clone` → 2 → `GfxMediaSink::new(Rc::clone(...))`
          → 3. `strong_count` is the oracle; `drop(sink)` returns it. Independently grepped:
          the only `doc.clone()` call sites in `crates/stele/src` are inside `#[cfg(test)]`
          modules, so no production path deep-copies the AST.
VERDICT:  **PASS** — `test_dw_2_6_the_sink_holds_the_caller_s_rc_not_a_copy`,
          `test_dw_2_6_the_sink_shares_the_loaded_document_rather_than_cloning_it`.

**All requirements met:** NO — DW-2.2.

## Edge cases

| Edge case | Handled | Evidence |
|---|---|---|
| stdin not a terminal **and** `/dev/tty` unavailable | YES | Observed: forked a child with `setsid()` (no controlling terminal), confirmed `open("/dev/tty")` fails with `ENXIO`, then `execv(stele, ["-"])` with the document on a pipe. Output: `stele: could not enter raw mode: Device not configured (os error 6)`, **exit 1 after 0.06 s**. Same for a file argument. Does not hang. |
| `--watch` combined with `-` | YES | DW-2.3 above; rejected at parse, stdout empty. |
| File deleted or replaced mid-session | YES | DW-2.4 above, plus `test_a_watched_file_that_reappears_is_reloaded`. |
| File truncated to empty | YES | `test_a_watched_file_truncated_to_empty_repaints_instead_of_crashing` (pty, row 1 blank, process alive) and `test_dw_2_4_a_reload_to_an_empty_document_survives_and_clamps_to_the_top`. |
| Reload while scrolled past the new end | YES | `test_dw_2_2_a_reload_past_the_new_end_clamps_instead_of_dangling`; `relayout` ends in `set_scroll`, which clamps to `max_scroll`. Independently reproduced: 200 paragraphs → `G` → reload to 3 paragraphs, `scroll() <= max_scroll()`. |

## Specific claims scrutinised

### 1. "The suite can no longer capture a real terminal" — SUBSTANTIATED for the current tests
Every site that spawns the binary and reaches terminal entry does `setsid()` +
`ioctl(TIOCSCTTY)` on a pty slave the test owns: `tests/common/pty.rs:196-202` and `:345-351`,
`tests/panic_mid_frame.rs:58-64`, `tests/tmux_graphics.rs:195-202`. `tests/cli_errors.rs` is
the only spawn site without a pty, and all four of its tests provably exit before
`crossterm::terminal::size()` (missing file, invalid UTF-8 file, `--watch -`, and `-` fed
invalid UTF-8) — verified by reading each and by the target finishing in 0.03 s.

**The deadline guard was verified by making a child overrun**, not by inspection: I copied
`run_bounded` verbatim into a scratch test, swapped the binary for `/bin/sh -c "sleep 600"`,
and ran it. It panicked at the deadline (`guard returned after 3.011825667s, err = true`) — it
fails, it does not silently pass.

Residual limitation (Note 3 below): `child.kill()` sends `SIGKILL`, which nothing can catch, so
a *future* cli_errors test that did reach terminal entry would still be killed with the
terminal in raw mode + alternate screen. The guard converts a hang into a failure; it does not
make the capture impossible. No current test triggers it.

### 2. Anchor preservation across insertion above the reader — PARTLY SUBSTANTIATED
I built my own fixtures (`AppState` public API, asserting on the **painted rows** of the whole
visible window, never on line numbers).

| My fixture | Result |
|---|---|
| Prepend a paragraph / a bullet list / a 300-line code fence / 25 paragraphs, reader on `para-030` of 60 | PASS — visible window byte-identical in all four |
| Delete a 200-line fence above the reader | PASS — window identical |
| Resize 40→70 with a *rewrapping* anchored block | PASS — reader stays on `MARK020` |
| Reload then resize | PASS — reader stays on `MARK020` |
| Edit to the anchored block itself (30 paragraphs above, 30 below) | PASS in effect — scroll 60 → 60, top row is the edited text |
| Anchored block deleted entirely | Survives — scroll 60 → 59, top row is the blank separator; `scroll <= max_scroll` |
| **Duplicated identical content** | **FAIL — 10 of 15 combinations put the reader on a different copy** (Issue 2) |

The 201-line regression the commit message describes is genuinely fixed; the ordinal tiebreak
is not.

### 3. "Fingerprint consulted only on the document-changed path" — SUBSTANTIATED
`relayout` (`app.rs:441-453`) branches on the one-shot `document_changed` flag taken at
`app.rs:430`; `line_of` (`app.rs:640-644`) never touches the fingerprint. The two cannot be
confused at different widths, and I checked *why*: `relayout` ends with
`self.content_width = self.tree.width()` (`app.rs:439`), and `reload_document` goes through
`relayout_preserving_anchor`, which lays out at exactly that already-clamped width — so a
reload's old and new trees are always at the same width and their fingerprints are comparable.
Confirmed behaviourally by my two resize probes above, including one where the anchored block
rewraps (a fingerprint comparison would have missed and dropped to the ratio fallback; the
reader stayed put).

### 4. The stdin size cap — SUBSTANTIATED, both sides, and bounded
Verified against the real binary, not just the unit tests:

| Input | Result |
|---|---|
| stdin exactly 67 108 864 B | accepted (proceeds to terminal entry: `could not enter raw mode`) |
| stdin 67 108 865 B | `stele: document is larger than the 64 MiB stele will read` |
| file 67 108 865 B | same message |
| `yes \| stele -` (unbounded stream) | refused in **0.03 s real, 77.7 MB peak RSS** (`/usr/bin/time -l`) |

The unbounded-stream case is the real proof that `read_bounded`'s `take(limit + 1)`
(`loader.rs:78`) bounds the *read*, not just the check: an infinite pipe costs ~64 MiB, not the
machine. Unit tests cover both sides via sparse files
(`test_a_document_past_the_size_ceiling_is_refused_by_the_barricade`,
`test_a_document_exactly_at_the_size_ceiling_still_loads`).

### 5. Test integrity vs. the prior revision (`c61bc5d`) — SUBSTANTIATED, no coverage lost
Diffed every test name in every changed file. **Nothing was deleted**; six tests were added and
two renamed, both strictly strengthened:

- `test_a_bare_document_is_moved_into_the_rc_not_copied_into_it` →
  `test_a_bare_document_argument_produces_the_same_sink_as_a_shared_one`. The old body's only
  assertion was `!format!("{:?}", sink.doc.blocks()).is_empty()`, which is **vacuously true**
  (`format!("{:?}", [])` is `"[]"`). The new one compares the actual protocol commands emitted
  by both constructor forms. Pure gain.
- `test_dw_2_1_stdin_alone_is_accepted_and_reads_the_pipe` →
  `test_dw_2_1_a_bare_dash_reads_the_document_from_stdin_not_from_a_file`. Both original
  negative assertions (`!--watch`, `!could not read file`) are retained; the new test adds
  nonzero exit, the `stele:` prefix, and a positive `not valid UTF-8` proving the pipe was
  read. The "a valid piped document really renders" claim it used to make (by hanging) moved to
  the pty test `test_dw_2_1_piped_stdin_renders_and_still_answers_keys_from_the_tty`, which is
  a stronger oracle. Pure gain.

## Test-DW Coverage
- [x] Every DW item has an automated test that ran in Step 0 (names listed per item above).
- [x] Every prompt-listed edge case has an automated test, except "`/dev/tty` unavailable",
      which is covered by recorded observed behaviour (see the table above) — no automated test
      exists for it. Non-blocking against the verdict rules (the case *is* handled), but it is a
      gap against the stated 100 % coverage level; see Note 5.
- [ ] **DW-2.2's coverage does not reach the two failing cases**: no test exercises a reload
      while input is arriving, and the existing duplicated-content test
      (`test_dw_2_2_a_reload_prefers_the_nearest_copy_of_duplicated_content`) uses a fixture
      with 21-block spacing between the duplicates, which is wide enough to hide the tiebreak
      bug. See Issues 1 and 2.

## Dead Code
None blocking. No unreachable code after an early return, no debug statements, no commented-out
blocks; `cargo clippy --all-targets` is silent (which would have caught unused imports).
`MediaSink::evict` remains production-dead (tests only) — pre-existing, and the trait doc says
so explicitly at `media/mod.rs:73-86`. Note only.

## Correctness Dimensions

| Dimension | Status | Evidence |
|---|---|---|
| Concurrency | **FAIL** | Not threads — event-loop starvation. `main.rs:328` gates `poll_reload` on `event::poll` *timing out*; sustained input never lets it run. Demonstrated on a pty: no reload for 8.03 s at ~14 keys/s. Issue 1. |
| Error Handling | PASS | Every failure on the load path is a `LoadError` variant with a prose `Display` and no raw `Debug` dump; `poll_reload`'s `Err` arm changes no tree and reports on the status row; the only `.ok()`/`unwrap_or` additions in the diff are test-file cleanup. Probed the adversarial inputs: missing path, invalid UTF-8, oversize (both sources), empty file, unstat-able path — all produce clear messages, none panic. |
| Resources | PASS | The adversarial case is an unbounded pipe: `read_bounded`'s `take(limit + 1)` holds `yes \| stele -` to 77.7 MB peak RSS and 0.03 s. `File` handles are dropped by `read_bounded`'s move; `GfxMediaSink::reload_document` frees every placement (screen and raster) before swapping the doc, so a `--watch` session cannot accumulate them. |
| Boundaries | PASS | Probed each: `block_span` returns `.max(1)`, so `place`'s `span - 1` cannot underflow; `anchor.span >= 1`, so the rescale cannot divide by zero; `f64 as usize` saturates in Rust; `set_scroll` clamps to `max_scroll`; `max_scroll` is `saturating_sub`. Empty document → `block_at` returns `None` → `anchor()` returns `None` → ratio fallback, scroll 0. Reload past the new end clamps. All exercised. |
| Security | PASS | The only untrusted inputs are the document bytes and argv. Bytes are size-bounded and UTF-8-validated at the single barricade before anything downstream sees them; argv's one cross-flag rule is decided before a byte is read. No shell-out, no path interpolation into a command. |

## Loaded-Skill Criteria

| Skill | Criterion | Status | Evidence |
|---|---|---|---|
| cc-defensive-programming | External input validated at entry (barricade) | PASS | `loader.rs:1-9` names itself the barricade and is the only entrance; both `read_bounded` (size) and `String::from_utf8` (encoding) run before any consumer. CLI validated at `cli.rs:96`, before any read. |
| cc-defensive-programming | No empty catch / swallowed errors | PASS | Grepped every `+` line of the production diff for `.ok()` / `unwrap_or` / `let _ =` / `Err(_)`: all hits are in `#[cfg(test)]` cleanup. `changed_since`'s two "unable to prove ⇒ report changed" paths (`loader.rs:186`, `:192`) are the *loud* direction, and are documented as such. |
| cc-defensive-programming | No executable code in assertions; assertions for bugs only | PASS | No `debug_assert!` and no production `assert!` added by this phase; every anticipated failure is a `Result`. |
| cc-defensive-programming | Correctness-vs-robustness strategy chosen and consistent | PASS | A viewer, so robustness: DW-2.4's "keep the last good render, report on the status row" is the *return previous answer + display error message* pair, applied consistently across delete/truncate/restore. |
| cc-defensive-programming | Barricade does not replace defence in depth | PASS | Downstream of the barricade the code still tolerates its inputs rather than trusting them: an empty document, a zero-block tree and a scroll past the end all resolve without panicking. |
| aposd-designing-deep-modules | Deep interface / information hiding | PASS | `DocumentSource::load_with` is one method hiding read + bound + UTF-8 + frontmatter + mermaid + parse, and `loader.rs:5-9` states the reason (startup and reload must not drift). `Anchor`, `fingerprint`, `block_runs`, `ordinal_of`, `place` are all private; callers see only `reload_document`. |
| aposd-designing-deep-modules | Granularity mismatch — caller doing the module's work | PASS | `main.rs` never compares a path to `"-"`, never re-implements the reload pipeline, and never touches the anchor; `Painter::reload_media` forwards rather than exposing the sink (`painter.rs:124-129`). |
| aposd-designing-deep-modules | Silent Failure — failures observable to callers | PASS (with Note 4) | A reload failure produces a visible status-row change. It does *decay*: the message TTL is 100 frames and `last_failure` suppresses re-showing it, so a still-missing file eventually goes quiet. Not a violation (the failure is surfaced), recorded as Note 4. |
| aposd-designing-deep-modules | False abstraction — interface hides what the caller needs | **FAIL** | `line_of_reloaded` presents "the reader's block, resolved by content" but silently returns a *different, equally-hashing* block when identical content exists, with no signal to `relayout` that the resolution was ambiguous. Demonstrated in Issue 2. |

## Notes (non-blocking)

1. **The DW-2.2 timing assertion is 8× looser than the stated interval.**
   `document_source.rs:37` sets `ONE_POLL_BUDGET = 2 s` against a `WATCH_POLL_INTERVAL` of
   250 ms. The test would still pass if the interval were raised to 1 s, so it does not really
   pin "within one poll interval". *Confidence: high. Severity: low.*

2. **`line_of_reloaded`'s fallback is O(document) hashing.** On a step-1 miss, `block_runs()`
   fingerprints every block in the document. Per reload only, not per poll, so it is not a
   throughput problem — but it is the path a large watched document takes on every save.
   *Confidence: high. Severity: low.*

3. **The deadline guard does not make terminal capture impossible.** `child.kill()` is
   `SIGKILL`, which neither `TerminalGuard`'s `Drop` nor `terminal::signals` can intercept, so a
   future cli_errors test that reached terminal entry would be killed with the developer's
   terminal in raw mode + alternate screen — a failure, but a messy one. A `SIGTERM`-then-
   `SIGKILL` escalation in `run_bounded` would let the existing restore path run.
   *Confidence: high (mechanism is certain). Severity: low (no current test triggers it).*

4. **A persistent reload failure eventually goes silent.** `STATUS_MESSAGE_TTL_FRAMES` is 100
   (`app.rs:87`) and `Session::last_failure` (`main.rs:299-301`) suppresses re-showing an
   identical message. After 100 painted frames the ruler returns while the file is still
   missing, and it is never re-reported. *Confidence: high. Severity: low.*

5. **No automated test for the "`/dev/tty` unavailable" edge case**, which is listed in the
   phase's edge cases and the coverage level is 100 %. I verified it by observed behaviour
   (clean message, exit 1, 0.06 s). A `pre_exec` doing `setsid()` and *not* claiming a ctty
   would make it a test. *Confidence: high. Severity: low.*

6. **`cli.rs:31-32`'s doc comment points the reader at the wrong place.** "With `-`, keys are
   read from `/dev/tty` instead (see [`Cli::source`])" — `Cli::source` says nothing about
   `/dev/tty`; that reasoning lives in `Cargo.toml:26-38`. *Confidence: high. Severity: trivial.*

7. **`anchor()` computes the fingerprint on every relayout**, including resizes where
   `line_of` will not consult it (`app.rs:468`). Wasted hashing of the anchored block's span
   once per resize burst. *Confidence: high. Severity: trivial.*

8. **`MediaSink::evict` is production-dead** (called only from tests). Pre-existing and
   documented at `media/mod.rs:73-86`. *Confidence: high. Severity: trivial.*

9. **`changed_since`'s wall-clock reconstruction has a microsecond-wide blind spot.**
   `SystemTime::now()` and `since.elapsed()` are sampled at slightly different instants; a write
   landing inside that window would not be seen until the *next* write. The doc comment covers
   the coarser wall-clock-jump case but not this one. Not reproducible on demand.
   *Confidence: medium. Severity: trivial.*

## Issues (FAIL)

### 1. `--watch` never reloads while keyboard input is arriving (DW-2.2)
- **File:** `crates/stele/src/main.rs:328-340`
- **Demonstrated by:** a pty test I wrote and ran. `--watch` on a 200-paragraph file; keys fed
  at ~14/s (macOS default autorepeat, and again at ~50/s); the file rewritten with a marker.
  Output: `under ~14 keys/s: reload marker seen = false, after 8.034573s`. The same fixture with
  no keys reloads in well under a second.
- **Why:** `if session.watch && !event::poll(WATCH_POLL_INTERVAL)?` runs `poll_reload` **only on
  the timeout branch**. Any pending event — an autorepeating `j`/`k`, or a resize storm — makes
  `poll` return `true` on every iteration, and the reload check is skipped indefinitely. DW-2.2
  says "within one poll interval of an external write"; here it is unbounded.
- **Fix:** decouple the reload check from the poll outcome. Keep a `last_checked: Instant` on
  `Session`, and after handling whatever `event::read()` returned (and on the timeout branch),
  call `poll_reload` whenever `last_checked.elapsed() >= WATCH_POLL_INTERVAL`. Add the
  starvation pty test alongside `test_dw_2_2_an_external_write_repaints_within_a_poll_interval…`.

### 2. Duplicated identical content re-anchors to the wrong copy (DW-2.2)
- **File:** `crates/stele/src/app.rs:590-596` (`line_of_reloaded`, the ordinal tiebreak)
- **Demonstrated by:** a test I wrote and ran, over documents alternating `gap` unique
  paragraphs with one identical block, reader parked on the 13th copy, then `n` paragraphs
  prepended. **10 of 15 combinations moved the reader to a different copy**, asserted on the
  painted window:

  | gap | inserted blocks | drift | window row 3 was → became |
  |---|---|---|---|
  | 1 | 1 | −4 lines | `unique-013` → `unique-012` |
  | 1 | 3 | −8 | `unique-013` → `unique-011` |
  | 1 | 10 | −20 | `unique-013` → `unique-008` |
  | 3 | 10 | −24 | `unique-039` → `unique-030` |
  | 10 | 10 | −22 | `unique-130` → `unique-120` |

  A second fixture (30 identical blocks interleaved with `unique-NNN` markers, one paragraph
  prepended) fails the same way: `[…,"unique-021",…]` → `[…,"unique-020",…]`.
- **Why:** step 2 selects `min_by_key(|(ordinal, _, _)| ordinal.abs_diff(anchor.ordinal))`.
  `anchor.ordinal` is the block's index in the **old** document; every candidate's ordinal in
  the new document has shifted by however many blocks were inserted above. Minimising distance
  to the *unshifted* value systematically drags the reader backwards once the insertion exceeds
  half the spacing between identical blocks — and at equal distance `min_by_key` keeps the first
  (lowest-ordinal) candidate, so an exact tie always resolves to the earlier copy.
- **Why the existing test misses it:** `test_dw_2_2_a_reload_prefers_the_nearest_copy_of_duplicated_content`
  (`app.rs:1923`) uses copies 21 blocks apart and prepends one block, so the correct candidate
  wins by 20. The bug needs `inserted > spacing / 2`.
- **Fix:** tiebreak on the anchor's **occurrence index among blocks with this fingerprint**
  rather than on its absolute ordinal. Carry `nth_match` on `Anchor` (how many earlier blocks
  hashed the same), and in step 2 pick the candidate with the same occurrence index, falling
  back to nearest-ordinal only when the count changed. That is invariant to insertion or
  deletion of *other* content, which is precisely the case that fails now. Note also that step 1
  (`app.rs:583-588`) short-circuits on the `NodeId` + fingerprint alone with no ordinal check,
  so it can accept a coincidentally-matching neighbour; the occurrence index should gate it too.

**Verdict: FAIL — blockers: DW-2.2 (Issue 1, watch reload starved by pending input;
Issue 2, duplicated content re-anchors to the wrong copy). DW-2.1, DW-2.3, DW-2.4, DW-2.5 and
DW-2.6 are clean, as are all five listed edge cases and claims 1, 3, 4 and 5.**
