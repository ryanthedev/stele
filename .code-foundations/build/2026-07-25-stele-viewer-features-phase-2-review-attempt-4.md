# Review: Phase 2 — document sourcing (`stele -`, `--watch`, parse-once, shared AST)

Reviewed at `45d9f4f` in `.code-foundations/wave-worktrees/phase-2`, against Phase 1 baseline `7187b5f`.

## Executed Results (Step 0)

| Command | Result |
|---|---|
| `script -q /dev/null cargo test --workspace` | exit 0 — **442 passed, 0 failed, 5 ignored** (the 5 are pre-existing `#[ignore]`s that need a live Ghostty GUI) |
| `cargo clippy --workspace --all-targets` | exit 0, no warnings |
| `cargo fmt --all -- --check` | exit 0 |

Run from inside a controlling terminal via `script -q /dev/null`, as instructed.

## Requirement Fulfillment

### DW-2.1
PREMISE:  "`stele -` renders markdown piped on stdin and still responds to keys."
EVIDENCE: `crates/stele/src/cli.rs:96-104` (`-` → `DocumentSource::Stdin`); `crates/stele/src/loader.rs:146` (`read_bounded(std::io::stdin().lock())`); `crates/stele/Cargo.toml:25-37` (`crossterm` `use-dev-tty`, which is what makes keys resolve when stdin is a pipe).
TRACE:    `stele -` with the document on a pipe and the pty as ctty → clap parses `file = "-"` → `Cli::source()` returns `Stdin` → `load_with` reads the pipe to EOF, UTF-8-checks, parses → viewer paints `piped-heading` / `body from the pipe` / `(stdin)` on the status row → `q` typed at the **tty** (not stdin) quits with exit 0 and the exact restore sequence.
VERDICT:  PASS — `test_dw_2_1_piped_stdin_renders_and_still_answers_keys_from_the_tty` (real pty, ran green), plus `test_dw_2_1_a_bare_dash_reads_the_document_from_stdin_not_from_a_file` proving `-` is not opened as a path, and `test_dw_2_1_a_bare_dash_names_the_stdin_source`.

### DW-2.2
PREMISE:  "`--watch` re-renders within one poll interval of an external write, preserving the anchored block."
EVIDENCE: `crates/stele/src/main.rs:65` (`WATCH_POLL_INTERVAL = 250 ms`), `:303-305` (`until_next_tick`), `:334-348` (`debounce_wait` / `watch_tick_due`), `:484-486` (tick runs on every outer iteration, independent of the event branch), `:360-389` (`poll_reload`); `crates/stele/src/app.rs:283-287, 419-460, 599-612` (content+occurrence re-anchor).
TRACE:    external write → `changed_since` sees `mtime > wall_clock_at(loaded_at)` → `load_with` → full parse+layout completed before `state` is touched → single assignment → repaint; the reader's top block is re-found by `(fingerprint, occurrence)`, not by `NodeId`.
VERDICT:  PASS — `test_dw_2_2_an_external_write_repaints_within_a_poll_interval_and_keeps_the_top_block` (row 1 compared verbatim before/after), plus the two starvation tests, all green. **Independently reproduced by me** (probes below): reload landed in 122 ms / 140 ms / 225 ms under sustained resize storms at 5 / 16 / 40 ms intervals, and 129 ms under simultaneous keys *and* resizes.

### DW-2.3
PREMISE:  "`--watch -` is rejected at CLI parse with a message naming the conflict."
EVIDENCE: `crates/stele/src/cli.rs:96-104`, `:64-81` (`CliError::WatchStdin` + manual `Display`); `crates/stele/src/main.rs:72-78` (rejected before the load and before any terminal call).
TRACE:    `stele - --watch` → `Cli::source()` → `Err(WatchStdin)` → `eprintln!("stele: --watch cannot be combined with `-`: stdin is a stream read once to end, not a file whose changes can be watched")` → `ExitCode::FAILURE`, stdout empty.
VERDICT:  PASS — `test_dw_2_3_watch_with_stdin_exits_nonzero_before_touching_the_terminal` (real binary; asserts nonzero exit, `--watch` and `stdin` both named, **stdout empty**) and two unit tests, all green.

### DW-2.4
PREMISE:  "A deleted or unreadable file under `--watch` shows a status-line error and keeps the last good render instead of exiting."
EVIDENCE: `crates/stele/src/loader.rs:179-194` (unstattable → `changed_since == true`); `crates/stele/src/main.rs:379-387` (failure sets a status message, touches no tree).
TRACE:    file deleted → `changed_since` true → `load_with` → `Err(Io)` → `state.set_status("reload failed: could not read file: …")` → repaint; tree unchanged, process alive.
VERDICT:  PASS for the requirement as worded — `test_dw_2_4_a_deleted_file_reports_on_the_status_row_and_keeps_the_last_frame` asserts all three halves and ran green; I re-observed the status row directly (`"reload failed: could not read file: No such file or directory (os error 2)"` on row 24, `still-here` still on row 1).
**See Issue 1** — the *recovery* half of this same mechanism is broken; that is filed under the "file deleted or replaced mid-session" edge case, not against this DW's wording.

### DW-2.5
PREMISE:  "A document with no mermaid fences is parsed exactly once at startup (asserted by instrumenting the parse count, not by timing)."
EVIDENCE: `crates/stele/src/loader.rs:220-246` (`thread_local` `PARSE_COUNT`, `counted_parse`); `crates/stele/src/decor/mermaid.rs:31-37` (`parse` reuses the fence-finding parse when nothing splices).
TRACE:    `load_with` → `frontmatter::apply` (pure text, no parse — verified: `frontmatter.rs` contains no `Document::parse` call) → `mermaid::parse` → `counted_parse` once → `rendered()` returns `None` → the same `Document` is returned. Delta = 1.
VERDICT:  PASS — `test_dw_2_5_a_document_without_mermaid_is_parsed_exactly_once` (delta asserted `== 1`), its counterpart pinning the mermaid path at exactly 2, and `test_dw_2_5_the_load_path_never_calls_document_parse_directly` (the guard that keeps the counter honest). I independently confirmed every non-test `Document::parse` in `crates/stele/src` is the one inside `counted_parse`.

### DW-2.6
PREMISE:  "The media sink holds a shared `Rc<Document>`; no full AST clone occurs at startup."
EVIDENCE: `crates/stele/src/media/sink.rs:156-162` (`doc: Rc<Document>`), `:199-213` (`new(doc: impl Into<Rc<Document>>)`); `crates/stele/src/main.rs:95` (`Rc::clone`), `:191-193` (`GfxMediaSink::new(Rc::clone(&session.doc), …)`); `crates/stele/src/loader.rs:100-106`.
TRACE:    `load_with` allocates one `Rc<Document>` → `main` `Rc::clone`s the handle (refcount 2, one allocation) → the sink is constructed from a third clone; `strong_count` is the oracle a type signature cannot be.
VERDICT:  PASS — `test_dw_2_6_the_sink_holds_the_caller_s_rc_not_a_copy` and `test_dw_2_6_the_sink_shares_the_loaded_document_rather_than_cloning_it` (1 → 2 → 1 around construction/drop), both green.

**All requirements met:** YES (all six DW items PASS). The failing verdict below rests on a listed edge case, not on a DW item.

## Test-DW Coverage

- [x] Every DW item has at least one automated test that **ran in Step 0**, named with its DW id.
- [x] Coverage matches the stated 100% level: each DW has both a unit-level and a black-box/pty-level test except DW-2.5 and DW-2.6, which are memory/instrumentation properties with no wire signature and are covered by the instrumented counter and `Rc::strong_count` respectively — the only oracles that can fail for those claims.
- No gaps.

## Edge Cases

| Edge case | Status | Evidence |
|---|---|---|
| stdin not a terminal and `/dev/tty` unavailable | PASS | My probe: child `setsid()` with **no** `TIOCSCTTY`, document on a pipe → exits 1 in <100 ms with `stele: could not enter raw mode: Device not configured (os error 6)`, no alternate-screen sequence on stdout, no hang. |
| `--watch` with `-` | PASS | DW-2.3 above. |
| File deleted or replaced mid-session | **FAIL** | Deletion itself is handled (DW-2.4). The recovery is not — see Issue 1, reproduced. |
| File truncated to empty | PASS | `test_a_watched_file_truncated_to_empty_repaints_instead_of_crashing` (green): row 1 empty, process alive. `loader::test_an_empty_file_loads_as_an_empty_document`. |
| Reload while scrolled past the new end | PASS | `test_dw_2_2_a_reload_past_the_new_end_clamps_instead_of_dangling`; my own fixture (60 paragraphs scrolled to line 82, reloaded to 3 paragraphs) landed at `scroll == max_scroll` with no panic. `set_scroll` clamps (`app.rs:305-307`). |

## Specific Claims — independent verification

I wrote and ran my own pty probes (`Pty::open` + real `TIOCSWINSZ`, so genuine `SIGWINCH` → crossterm → `Event::Resize`), then deleted them. Results:

**1. The resize-storm fix.**

| Resize interval | `--watch` reload latency | Non-watch frames / resizes over 2 s |
|---|---|---|
| 5 ms | 122 ms | 9 / 172 |
| 16 ms (real 60 Hz drag) | 140 ms | 9 / 87 |
| 40 ms | 225 ms | 8 / 44 |

All three land far inside one poll interval, and the viewer keeps painting throughout with no `--watch` involved. No resize event is dropped: after a 30-resize storm I set a final, distinct geometry (40×45) and the settled frame's longest content run measured exactly 45 cells — the last size won. Double-counting is structurally impossible: `apply_resize_burst` (`app.rs:684-688`) uses `sizes.last()` only.

**Busy-spin: verified absent, by argument and by measurement.** `until_next_tick()` is zero exactly when `watch_tick_due()` is true (both test `last_tick.elapsed() >= WATCH_POLL_INTERVAL`), so a zero wait always reaches a tick that resets the clock; without `--watch` the outer `event::read()` blocks. Measured: 489 real resizes over 3 s cost the process under 10 ms of CPU (`ps utime/stime` unchanged at `0:00.00`); an idle `--watch` session likewise.

**2. Coalescing preserved.** The claimed 86→9 ratio reproduces: 9 frames for 172 resizes, 9 for 87, 8 for 44 — ~200 ms of coalescing per frame at every rate, i.e. the burst ceiling and not one relayout per event. The in-tree assertion `frames * 3 <= resizes` holds with wide margin at all three rates.

**3. Composition with the earlier fixes.** Reload landed in 129 ms with keys *and* resizes arriving together for the whole window (a key every 10 ms interleaved with a resize every 10 ms). Continuous keys alone and continuous resizes alone are covered by the two in-tree tests, both green.

**4. The anchor — my own fixtures, all passing.**

| Case | Result |
|---|---|
| 3 blocks inserted above the reader | reader stays on `delta` |
| 3 blocks deleted above the reader | reader stays on `delta` |
| 4 identical blocks, reader on the 3rd, 2 blocks inserted above | lands on the **3rd** copy (occurrence index, not ordinal) |
| 4 identical blocks, a copy above deleted | lands on the last remaining copy, still the same text |
| the anchored block rewritten | proportional fallback runs; position valid, within a viewport of where they were |
| the anchored block deleted outright | no panic, `scroll <= max_scroll`, lands on the following block (`echo`) |
| reader 40 lines into an 80-line fence, blocks inserted above | still on `code-040` — the offset survives, not just the block |

**5. Test integrity across four revisions.** Diffed `c61bc5d → 119e0f0 → 7e97026 → 45d9f4f` (all four amend generations are still in the reflog) plus the `7187b5f` baseline. Three test **names** disappeared; every one was replaced by a strictly stronger test, and no assertion was lost:

| Removed | Replaced by | Verdict |
|---|---|---|
| `test_dw_2_1_stdin_alone_is_accepted_and_reads_the_pipe` | `test_dw_2_1_a_bare_dash_reads_the_document_from_stdin_not_from_a_file` | keeps both original negative assertions, **adds** `contains("not valid UTF-8")`. Stronger. |
| `test_dw_2_2_a_reload_prefers_the_nearest_copy_of_duplicated_content` | `test_dw_2_2_duplicated_content_re_anchors_to_the_copy_the_reader_was_on` | old test used the now-deleted `ordinal_of`; new one sweeps 9 gap×insert combinations and uses a unique marker above the reader as the oracle. Stronger. |
| `test_a_bare_document_is_moved_into_the_rc_not_copied_into_it` | `test_a_bare_document_argument_produces_the_same_sink_as_a_shared_one` + `test_dw_2_6_the_sink_holds_the_caller_s_rc_not_a_copy` | old body asserted only `!blocks().is_empty()`; new pair asserts painted protocol bytes and `strong_count`. Stronger. |

Every other assertion-line deletion across the four revisions was the `top_line_text` → `topmost_line_text` rename or the `Command::output()` → `run_bounded` refactor. Nothing was weakened.

## Dead Code

None blocking. No unreachable code after early returns, no `dbg!`/`todo!`/`#[allow(dead_code)]`, no commented-out blocks introduced. Two minor observations in Notes.

## Correctness Dimensions

| Dimension | Status | Evidence |
|-----------|--------|----------|
| Concurrency | PASS | Single-threaded event loop; `PARSE_COUNT` is `thread_local` precisely so the DW-2.5 delta is exact under `cargo test`'s multi-threaded runner. No shared mutable state added. Adversarial case traced: two loader unit tests parsing concurrently cannot perturb each other's delta. |
| Error Handling | **FAIL** | Issue 1: a reload failure is latched into the status row and never reconciled when the reload later succeeds. Reproduced end-to-end. |
| Resources | PASS | `read_bounded` (`loader.rs:75-87`) uses `take(MAX+1)`, so an oversized source costs one extra byte, not all of it — traced against the adversarial case (a 400 MB pipe) and pinned at both sides of the boundary by `…past_the_size_ceiling…` and `…exactly_at_the_size_ceiling…`. `GfxMediaSink::reload_document` deletes every placement through `delete_placement`, which removes the map entry *and* the LRU entry, so a `--watch` reload cannot leak terminal-side rasters (`test_a_reload_takes_every_image_off_the_terminal_and_swaps_the_document`). |
| Boundaries | PASS | Adversarial cases traced: `place()` (`app.rs:617-624`) does `span - 1`, safe because `block_span` returns `.max(1)`; `anchor.span` is likewise ≥ 1 so the rescale cannot divide by zero; `f64 as usize` saturates rather than wrapping; `block_at` returns `None` past the end so `block_span`'s loop terminates. Empty document → `anchor()` returns `None` → proportional fallback with `max_scroll == 0` → scroll 0. Verified live by the truncate-to-empty test. |
| Security | PASS | The barricade is real and single-entry (`DocumentSource::load_with`): size ceiling first, UTF-8 validation second, then preprocessing. `-` is interpreted in exactly one place (`Cli::source`), so a file literally named `-` cannot be confused with the stream, and `--` disambiguates a leading-dash filename (`test_any_other_path_names_a_file_source_dash_or_not`). No new untrusted-input path bypasses the loader. |

## Loaded-Skill Criteria

| Skill | Criterion | Status | Evidence |
|-------|-----------|--------|----------|
| cc-defensive-programming | No executable code inside assertions | PASS | No `debug_assert!` added; all `assert!` are in `#[cfg(test)]`. |
| cc-defensive-programming | No empty catch / silently swallowed errors | PASS (with note) | The three swallow sites are all in the resize drain (`main.rs:445-466`): `event::poll(wait).unwrap_or(false)`, `Ok(_) => break`, `Err(_) => break`. Traced: each merely *ends the burst*; the outer loop's `event::poll(…)?` / `event::read()?` re-raise a persistent I/O error on the next iteration, so no error is lost, only deferred by one iteration. Not a violation; see Notes for the readability cost. |
| cc-defensive-programming | External input validated at the barricade entry | PASS | `loader.rs:75-148` — bounded read, then `String::from_utf8`, before any consumer sees the bytes. Both source kinds go through the same function by construction. |
| cc-defensive-programming | Assertions for bugs only; anticipated failures get error handling | PASS | Every anticipated failure (missing file, bad UTF-8, oversize, bad flag combination) is a hand-rolled enum variant with a manual `Display`, never an assertion. |
| cc-defensive-programming | Error-handling strategy chosen and applied consistently | **FAIL** | The chosen strategy for a reload failure is *display an error message + return previous answer*. It is applied on the failure edge and never unwound on the success edge — the message outlives the condition it describes. Issue 1. |
| cc-defensive-programming | Correctness-vs-robustness posture appropriate for the domain | PASS | A viewer correctly leans robust: a vanished file keeps the reader's render and scroll rather than shutting down. Explicitly reasoned in `poll_reload`'s doc comment and matched by the code. |
| aposd-designing-deep-modules | Deep interface / information hiding | PASS | `DocumentSource::load_with` is one method hiding read → bound → UTF-8 → frontmatter → mermaid → parse, and its doc comment states *why* it must stay one method (two callers reassembling it would have to keep DW-2.5 true twice). `MediaSink::reload_document` hides node-renumbering and terminal-side placement state behind one call, defaulted to a no-op. |
| aposd-designing-deep-modules | No information leakage across module boundaries | PASS | The `-` convention lives only in `cli.rs`; `main` and `loader` never compare a path to `"-"`. The `Rc` sharing decision is stated once, in `LoadedDocument`. |
| aposd-designing-deep-modules | No pass-through / shallow layer | PASS | `Painter::reload_media` is a one-line forward but earns its place: it keeps the sink un-exposed so a caller cannot paint through it outside a frame, which is stated in the doc comment. |
| aposd-designing-deep-modules | **Silent Failure** — failures observable to callers, not only errors hidden | **FAIL** | `Session::poll_reload` returns `bool` meaning "repaint needed", which collapses two distinct outcomes — *nothing changed* and *still failing, already reported* — into the same `false` (`main.rs:381-383`). Because the caller cannot tell them apart, nothing in the loop can reconcile the latched failure state with a later success. Issue 1 is that collapse made visible. |
| aposd-designing-deep-modules | Generality sweet spot | PASS | `GfxMediaSink::new(impl Into<Rc<Document>>)` serves both the sharing caller and a test that owns its document, without a second constructor. |

## Notes (non-blocking)

| # | Observation | Confidence | Severity |
|---|---|---|---|
| N1 | **Misplaced doc comment.** `main.rs:306-333` is one doc block attached to `debounce_wait`, but its first ten lines describe `watch_tick_due` ("Whether a watch tick is owed now, resetting the clock when it is…"). `watch_tick_due` (`:342`) is left with no doc at all, and `debounce_wait`'s rustdoc opens with a paragraph about a different function. A merge artefact from this revision, not a behaviour bug. | High (read directly) | Low |
| N2 | **Chrome keys are inert during a resize burst.** The drain loop calls `state.handle_key_event` directly (`main.rs:457-462`) and never `handle_chrome_key`, so `+`, `-` and `T` pressed while a window is being dragged are consumed and do nothing. Scroll/quit keys are unaffected. No DW covers this. | High (traced) | Low |
| N3 | **Failure message ages out while the file is still gone.** Demonstrated: with the watched file still deleted, 110 key presses aged `STATUS_MESSAGE_TTL_FRAMES` out and the status row returned to the ordinary ruler (`…/probe2-stillgone-64704.md — 100%`) over a document whose file no longer exists. Defensible as "transient message", and DW-2.4's wording ("shows a status-line error") is satisfied at the moment of failure — so this is a note, not a blocker. It shares a root cause with Issue 1. | High (reproduced) | Medium |
| N4 | **Error-swallowing readability.** `event::poll(wait).unwrap_or(false)` and `Err(_) => break` in the drain (`main.rs:445, 465`) are correct (see the skill table) but carry no comment saying *why* discarding the error is safe here. One line would stop the next reader from re-deriving it. | High | Low |
| N5 | **`DocumentSource::load()` has no production caller** — `main` uses `load_with`. It is a public convenience wrapper exercised only by unit tests. Fine as API surface; flagged only so it is a choice rather than an oversight. | High | Low |
| N6 | `crossterm::poll` returning true does not guarantee `read()` will not block on a partial escape sequence. Pre-existing, not introduced here, and I could not construct a case that reaches it. | Low | Low |

## Issues (FAIL)

### 1. A `--watch` reload failure is latched and never cleared, so the status row lies about a document that reloaded successfully

- **File:** `crates/stele/src/main.rs:371-378` (success arm of `Session::poll_reload`) with `crates/stele/src/app.rs:202-204, 225-240, 283-287` (`status_message`, its 100-frame TTL, and `reload_document`, which never touches it).
- **Demonstrated by:** a pty test I wrote and ran against the real binary. Sequence: start `stele --watch file.md` → delete the file → **status row:** `"reload failed: could not read file: No such file or directory (os error 2)"` → recreate the file with new content → the new content (`second-body`) renders, and the **status row is byte-identical to the failure message**. It stayed that way for all 12 subsequent frames I drove with key presses, and by construction stays for ~100.
- **Trace:** `set_status` stores `(text, 100)`. The failure frame spends 1, leaving 99. `poll_reload` then returns `false` for every repeat of the same failure, so no frames are spent while the file is gone. When the file returns, the success arm clears `self.last_failure` but nothing clears `AppState::status_message`; `reload_document` → `relayout_preserving_anchor` never touches it either. The next `status()` therefore returns the *stale* message, and does so for the remaining 99 frames.
- **Why this is a blocker:** the prompt lists "File deleted or replaced mid-session" as an edge case, and this is the second half of that sequence. The viewer displays a document it has just successfully reloaded while simultaneously asserting the reload failed — a false statement about current state, from the one row whose whole job is reporting current state. It is also a demonstrated violation of two loaded-skill criteria (consistent error-handling strategy; APOSD's Silent Failure red flag).
- **Why the suite misses it:** `document_source.rs::test_a_watched_file_that_reappears_is_reloaded` covers exactly this sequence but asserts only that `second-body` appears on the wire — it never inspects row 24. Two lines there would have caught it.
- **Fix:** clear the transient message on a successful reload. Concretely, give `AppState` a `clear_status` (or have `reload_document` take `self.status_message = None`) and call it from `poll_reload`'s `Ok` arm alongside `self.last_failure = None`. Then extend `test_a_watched_file_that_reappears_is_reloaded` to assert the status row no longer contains `reload failed` after the restore, so the pairing is pinned.

**Verdict: FAIL — one blocker: Issue 1 (latched `--watch` reload failure reported over a successfully reloaded document; listed edge case "file deleted or replaced mid-session", reproduced end-to-end).**

All six Done-When items PASS with execution evidence. The resize-storm fix, its interaction with the earlier tick and debounce fixes, coalescing, the anchor across all five named mutations, and test integrity across the four amend generations all verified independently and all hold.
