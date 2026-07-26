# Discovery + Design: Phase 2 — Document sourcing (stdin and `--watch`)

## Files Found

| File | Current state |
|---|---|
| `crates/stele/src/loader.rs` | 68 lines: `LoadError::{Io,InvalidUtf8}` + `load_document(&Path) -> Result<String, LoadError>`. No source abstraction, no parse. |
| `crates/stele/src/cli.rs` | `Cli { file, max_width, no_images, frontmatter }`. No `--watch`, no `-`, no validation step. |
| `crates/stele/src/main.rs` | Load → frontmatter → mermaid preprocess → `Document::parse` → layout → `run_session` with a **blocking `event::read()`** loop. `doc.clone()` at line 149 into the sink. |
| `crates/stele/src/decor/mermaid.rs` | `preprocess(&str) -> Cow<str>` parses internally (line 25); `main` then parses the result again. Two parses even with zero mermaid fences. |
| `crates/stele/src/media/sink.rs` | `GfxMediaSink { doc: Document, .. }`, `new(doc: Document, base_dir)`. 39 call sites workspace-wide. |
| `crates/stele/src/app.rs` | `LayoutContext<'a> { doc: &'a Document, .. }`, `relayout_preserving_anchor` (Phase 1 seam), `no_reflow_occurred(previous_width)`. |
| `crates/stele/tests/common/{pty,render,fixtures}.rs` | Shared pty harness (`Pty`, `read_until`, `drain_quiet`, `assert_restores_the_terminal`), a CUP/EL-honouring terminal model (`render_row`), scratch dirs. |

## Assumption Verification

| Assumption | Verdict | Evidence |
|---|---|---|
| crossterm can read keys from `/dev/tty` while stdin is a pipe | **FALSE at crossterm's defaults on macOS — fixed by a feature flag** | Source reading said yes: `tty_fd()` (`file_descriptor.rs:143`) opens `/dev/tty` whenever `isatty(STDIN)` is false, and both `enable_raw_mode` (`terminal/sys/unix.rs:114,132`) and the event source (`event/source/unix/mio.rs:37`) go through it. **Running it said no.** The first pty run of DW-2.1 rendered the piped document correctly, then printed `stele: Failed to initialize input reader` (`crossterm/src/event/read.rs:57`) and exited before reading a key. Cause, measured with a standalone probe rather than inferred: XNU's controlling-terminal device has no kqueue filter — `kevent(EVFILT_READ)` on `/dev/tty` returns `EINVAL` while the same call on the underlying `/dev/ttysNNN` succeeds — so mio's registration in `UnixInternalEventSource::from_file_descriptor` fails. **Resolution:** crossterm's `use-dev-tty` feature swaps the mio source for a `poll(2)`-based one (`event/source/unix/tty.rs`), which has no such restriction. One line in `crates/stele/Cargo.toml`, adds the `filedescriptor` crate; DW-2.1 then passes and the whole suite stays green. See the deviation note at the end. |
| `/dev/tty` unavailable must fail, not hang | **Already handled** | `tty_fd()` returns `io::Error`; `TerminalGuard::enter` propagates it and `main` prints `stele: could not enter raw mode: …` and exits 1. `query_cell_px` short-circuits on `!stdin.is_terminal()` (`terminal.rs:186`), so no query round-trip can hang on a pipe. |
| mtime polling on the `event::poll` timeout is responsive enough | **CONFIRMED** | `event::poll(Duration)` already used for the resize debounce (`main.rs:239`). A 250 ms watch timeout gives a worst-case 250 ms reload latency, well inside "one poll interval". |

## Gaps

| # | Gap | Resolution |
|---|---|---|
| G1 | `changed_since(&self, Instant)` — `Instant` is monotonic and cannot be compared to a file's `SystemTime` mtime. | Implementable as pinned: `SystemTime::now() - since.elapsed()` reconstructs the wall clock at `since`. Documented with its limits (clock skew; a file restored with an *older* mtime is not detected). No plan change needed. |
| G2 | `load(&self) -> Result<LoadedDocument, LoadError>` takes no arguments, but the pipeline it must own needs `--frontmatter`. | The pinned no-arg `load()` is kept **verbatim** (default policy) and delegates to `load_with(LoadOptions)`. The seam is not redesigned; one overload is added. |
| G3 | `AppState::no_reflow_occurred` uses width alone as a proxy for "the tree is identical". A reload changes the document at the *same* width, so it would report "no reflow" and skip the anchor path entirely — defeating DW-2.2 whenever the edit is above the anchor. | New `AppState::reload_document(ctx, file_info)` sets a one-shot `document_changed` flag that `relayout` consumes, forcing the anchor path. The Phase-1 `relayout_preserving_anchor` signature is untouched. |
| G4 | `GfxMediaSink` resolves media by `NodeId` against its own `Document`. After a reload those ids name different nodes — stale/wrong images. | Defaulted `MediaSink::reload_document(&mut self, Rc<Document>, &mut dyn Write)`; `GfxMediaSink` deletes every placement (screen + raster) and swaps the doc. `Painter::reload_media` forwards. |
| G5 | `LayoutContext<'a>` borrows one `&Document` for the whole session; under `--watch` the document is replaced. | `Session` owns `Rc<Document>` and mints a fresh `LayoutContext` per call (`Session::ctx()`). `LayoutContext` itself is unchanged. |
| G6 | Changing `GfxMediaSink::new` to `Rc<Document>` would touch 39 call sites. | `new(doc: impl Into<Rc<Document>>, ..)`. `Rc<T>: From<T>` and the reflexive `From` make both old (`Document`) and new (`Rc<Document>`) call sites compile unchanged. |
| G7 | `mermaid::preprocess` is used by `tests/painter_frame.rs:284,314`. | Keep `preprocess` as the text transform; add `mermaid::parse(&str) -> Document` for the load path. Both share one private `rendered(source, &doc) -> Option<String>`. |

## Code Standards

Applied: `#![deny(unsafe_code)]` (nothing new added — the pty harness's existing `allow` stays test-side, which `tests/hardening.rs` asserts); hand-rolled error enums with manual `Display` (`LoadError` extended, new `CliError`); no wildcard match arms; sentence-style test names with `test_dw_2_N_` prefixes; import grouping std / external+workspace / `crate::`; integration tests extend `tests/common/pty.rs` rather than growing a second pty.

## Test Infrastructure

`cargo test`, unit tests in `#[cfg(test)] mod tests` at file bottom, integration tests in `crates/stele/tests/` sharing `mod common;`. The pty harness spawns `env!("CARGO_BIN_EXE_stele")` with `setsid` + `TIOCSCTTY` in `pre_exec`; frames are delimited on the wire by `\x1b[?2026h … \x1b[?2026l`, and `common::render::render_row` replays a frame into a cell grid. Gap: no helper spawns the binary with stdin on a **pipe** (the `stele -` shape) — added as `pty::spawn_viewer(pty, args, ChildStdin)`, which makes the pty slave the controlling terminal via `ioctl(1, TIOCSCTTY)` when stdin is not the tty.

## DW Verification

| DW-ID | Done-When Item | Status | Test Cases |
|-------|---------------|--------|------------|
| DW-2.1 | `stele -` renders markdown piped on stdin and still responds to keys. | COVERED | `tests/document_source.rs::test_dw_2_1_piped_stdin_renders_and_still_answers_keys_from_the_tty` (pty + stdin pipe: heading on the wire, then `q` produces the exact restore sequence and exit 0); `loader.rs::test_dw_2_1_stdin_source_names_itself_and_reports_no_change` |
| DW-2.2 | `--watch` re-renders within one poll interval of an external write, preserving the anchored block. | COVERED | `tests/document_source.rs::test_dw_2_2_an_external_write_repaints_within_a_poll_interval_and_keeps_the_top_block` (scroll, capture frame, append externally, assert next frame arrives unprompted < 2 s, row 1 identical, status ruler changed); `app.rs::test_dw_2_2_a_reload_at_the_same_width_still_re_anchors` |
| DW-2.3 | `--watch -` is rejected at CLI parse with a message naming the conflict. | COVERED | `cli.rs::test_dw_2_3_watch_with_stdin_is_rejected_naming_both_flags`; `tests/cli_errors.rs::test_dw_2_3_watch_with_stdin_exits_nonzero_before_touching_the_terminal` |
| DW-2.4 | A deleted or unreadable file under `--watch` shows a status-line error and keeps the last good render instead of exiting. | COVERED | `tests/document_source.rs::test_dw_2_4_a_deleted_file_reports_on_the_status_row_and_keeps_the_last_frame` (delete mid-session: status row names the failure, row 1 still shows the heading, child still alive); `loader.rs::test_dw_2_4_a_missing_path_reports_changed_so_the_failure_is_seen` |
| DW-2.5 | A document with no mermaid fences is parsed exactly once at startup. | COVERED | `loader.rs::test_dw_2_5_a_document_without_mermaid_is_parsed_exactly_once`; `loader.rs::test_dw_2_5_a_renderable_mermaid_fence_costs_exactly_one_extra_parse`; `tests/hardening.rs::test_dw_2_5_the_load_path_never_calls_document_parse_directly` (source guard so the counter cannot be bypassed) |
| DW-2.6 | The media sink holds a shared `Rc<Document>`; no full AST clone occurs at startup. | COVERED | `loader.rs::test_dw_2_6_the_sink_shares_the_loaded_document_rather_than_cloning_it` (startup shape: `strong_count` 1 → 2 → 1); `media/sink.rs::test_dw_2_6_the_sink_holds_the_caller_s_rc_not_a_copy` |

**All items COVERED:** YES (6 DW-IDs in the dispatch prompt, 6 rows here)

## Design: `DocumentSource`

### Approaches Considered

1. **Trait object** — `trait Source { fn load(&self) -> …; fn changed_since(&self, Instant) -> bool; }` with `FileSource` / `StdinSource` impls.
2. **Enum with a load pipeline inside** (the plan's shape) — `enum DocumentSource { Path(PathBuf), Stdin }`; `load` owns read → UTF-8 → frontmatter → mermaid → parse and returns `LoadedDocument { doc: Rc<Document>, info: FileInfo }`.
3. **Thin enum + caller-side pipeline** — `load` returns `String`; `main` keeps doing the four preprocessing steps and the parse itself, as today.

### Comparison

| Criterion | 1 trait | 2 enum + pipeline | 3 thin enum |
|---|---|---|---|
| Interface simplicity | 2 methods + 2 types + a `Box<dyn>` | 2 methods, 1 type | 1 method, but 5 caller-side steps |
| Information hiding | Hides the source; leaks the pipeline | Hides source *and* pipeline; `-`/`/dev/tty`/mtime/parse-count all internal | Leaks the whole pipeline and the parse-once rule to every caller |
| Caller ease of use | `Box<dyn Source>` in `Session`, dynamic dispatch for two variants | `source.load()?` → everything needed | `main` and the reload path must both re-derive the same 5 steps |
| Reload correctness | fine | fine — one call site, so reload cannot drift from startup | **two** copies of the pipeline (startup + reload) that must stay in sync |
| Exhaustiveness (house rule: no wildcard arms) | traits give none | `match` on 2 variants, compile error on a third | same |

### Choice: 2 (the plan's shape)

Two source kinds, both known at compile time, is exactly where an enum beats a trait — and the parse-once rule (DW-2.5) only stays true if there is **one** text→`Document` path. Approach 3 is the granularity mismatch APOSD names: the caller does work belonging to the module, and duplicating it across startup and reload is how the two drift. Sacrificed: a third source kind (a URL, a pipe fd) becomes an enum edit rather than a new impl — acceptable, and the exhaustive match makes it a compile error rather than a silent gap.

### Depth Check
- Interface methods: 4 (`load`, `load_with`, `changed_since`, `display_name`) + `base_dir`.
- Hidden details: the `-` convention; `io::stdin().read_to_end` vs `fs::read`; UTF-8 validation; frontmatter stripping; mermaid fence rendering; the parse counter; the `Instant`→wall-clock mtime conversion; the "cannot stat ⟹ report changed" defensive rule.
- Common case complexity: **simple** — `source.load_with(opts)?` yields `Rc<Document>` + `FileInfo`.

## Defensive Programming (barricade)

`DocumentSource::load*` **is** the barricade's outer edge — the file/stdin bytes are the first untrusted input. Validation lives there (`InvalidUtf8`, io errors → `LoadError`), and everything inside (`layout`, `AppState`, `Painter`) may assume a valid `Document`. Error strategy for the watch loop is **"log warning and continue" + "return previous answer"**: a failed reload keeps the last good tree on screen and reports on the status row (DW-2.4) — the robustness lean, correct for a read-only viewer where a crash costs the reader their scroll position. `changed_since` returns `true` when it cannot stat the file: unable to prove *unchanged*, it reports changed so the failure surfaces through `load`'s real error rather than being silently swallowed (the Silent Failure red flag). No assertions added — every condition here is anticipated runtime input, not a programmer bug.

## Prerequisites
- [x] Phase 1 landed (`relayout_preserving_anchor`, status row, `PanicGuardedWriter`)
- [x] crossterm 0.29 `/dev/tty` fallback (verified above)
- [x] No new dependency needed (`std::fs::metadata`/`SystemTime` only)

## Recommendation

**BUILD.** All six DW items are reachable. Beyond the plan's `File scope` list, three files need edits the DW items force: `app.rs` (G3 — the anchor path is skipped on a same-width reload), `painter.rs` + `media/mod.rs` (G4 — the sink's `NodeId` map is stale after a reload), and `tests/common/pty.rs` + `tests/hardening.rs` (harness and the DW-2.5 source guard).

## Deviations from this design, and what execution changed

| # | Where | What happened |
|---|---|---|
| D1 | `crates/stele/Cargo.toml` | The verified-by-reading assumption above was wrong when run. `crossterm = { version = "0.29", features = ["use-dev-tty"] }` is now load-bearing for DW-2.1 on macOS. It is an implementation detail (a feature flag, no seam or scope change) rather than a plan-level fallback, so the phase continued rather than returning UPDATE_PLAN — but it changes the input path for *every* mode, and the full workspace suite was re-run as the evidence that nothing regressed. It also adds one transitive crate, `filedescriptor`. |
| D2 | `app.rs` reload anchoring | The first DW-2.2 unit test asserted an anchor that survived inserting whole blocks *above* the reader. It failed, and correctly: a `NodeId` is positional, so a re-parse renumbers every block below an insertion. The test was retargeted to the case the fix actually governs (a block above the reader changing *height*, which is the ordinary in-place edit under `--watch`), and the limit is written down on `AppState::reload_document` rather than papered over. |
| D3 | `tests/common/pty.rs` | The pty teardown needed `quit_and_reap`: a pty master's buffer is small, so a child whose final frames nobody reads blocks in `write` and never exits — a bare `wait()` after `q` hung the run instead of failing it. Found by hitting it. |

## Post-review corrections (round 2)

The independent review returned FAIL on two demonstrated blockers. Both are fixed here.

| Blocker | What was wrong | Fix | Evidence it is fixed |
|---|---|---|---|
| **1 — the suite wedged a real terminal** | `cli_errors.rs::test_dw_2_1_stdin_alone_is_accepted_and_reads_the_pipe` piped a *valid* document to `stele -` and assumed the child would fail at raw-mode entry "because there is no controlling terminal". D1's `use-dev-tty` switch invalidated that: run from a terminal, the child resolved `/dev/tty` to the **developer's own terminal**, entered raw mode and the alternate screen, and blocked in `event::read()` forever. It passed in CI only because CI had no ctty. | Two changes, because the class of defect matters more than the instance. (a) The test now pipes **invalid UTF-8**, so it fails at the loader barricade — strictly before `main` touches the terminal — and asserts `not valid UTF-8` (proving the pipe was read *as the document*) while ruling out `could not read file` (proving `-` was not opened as a path). (b) Every child spawned in `cli_errors.rs` now goes through `run_bounded`, which kills and **fails** on a 10 s deadline. The file's module doc states the invariant: anything that needs to reach the terminal belongs in `document_source.rs` with a pty. | `script -q /dev/null <cli_errors bin>` → 4 passed in **0.06 s** (was: alive at 25 s, never completed at 60 s). Guard verified by re-introducing the old shape: it now fails at 10.01 s with a message naming the fix, instead of hanging. |
| **2 — DW-2.2's anchor was not preserved across insertion above the reader** | `Anchor` carried a positional `NodeId`. Prepending a block shifts every id, and because `first_line_of` still returns `Some` for the shifted id, the wrong block was taken as authoritative and the ratio fallback never ran. Measured drift: −201 lines into a different block. | The anchor is now **content-addressed**: it carries a `fingerprint` (a hash of what the block paints) plus its `ordinal`. `line_of_reloaded` verifies rather than trusts — fast path when the id still hashes the same, otherwise a search for the block that *is* the reader's block by content (nearest ordinal wins, so duplicated text resolves to the near copy), otherwise `None` so the proportional fallback runs *when it should*. The fingerprint is consulted only on the document-changed path: on a resize the same block deliberately re-wraps, so a content check would be wrong there, not merely redundant. The doc comment that claimed "the reader lands near where they were" is gone — the claim is now true, not softened. | `test_dw_2_2_a_block_inserted_above_the_reader_still_leaves_them_on_their_own_block` covers paragraph, heading, bullet list, small fence and a 200-line fence, asserting the reader's **painted row text**; plus deletion-above and duplicated-content tests. Mutation-checked: with `line_of_reloaded` reverted to the positional lookup, all three new tests fail (`"code 0"`/`""` vs `"AFTER-THE-FENCE"`); with the fix they pass. |

Also fixed, from the review's non-blocking notes: **N1** (unbounded read — `MAX_DOCUMENT_BYTES` = 64 MiB now bounds *both* sources via `read_bounded`, using `take(limit + 1)` so an oversized source costs one extra byte rather than all of it; boundary tested on both sides), **N4** (the doc comment that had detached from `relayout_preserving_anchor` and absorbed into `reload_document`), and **N9** (the sink test whose only assertion a deep copy would also have satisfied).

## Post-review corrections (round 3)

The second review confirmed both round-2 blockers fixed, and demonstrated two new ones. Both are fixed here.

| Blocker | What was wrong | Fix | Evidence it is fixed |
|---|---|---|---|
| **1 — `--watch` never reloaded while keys were arriving** | The reload check hung off `event::poll`'s *timeout branch*. Any pending event skipped it, and an autorepeating key produces a pending event every iteration, so a held `j` starved the reload indefinitely — measured at ~14 keys/s with the marker unseen after 8 s. DW-2.2's latency bound simply did not exist under input, which every existing watch test missed because they all poll an **idle** viewer. | The tick is now a period on the monotonic clock, not a consequence of idleness. `Session::last_tick` + `watch_tick_due()` run the reload on schedule **after** whatever the event branch did, and `until_next_tick()` shrinks the loop's `poll` wait as the interval is used up — without the shrink, a steady event stream would reset the clock forever. The non-key event arm became `_ => {}` rather than `continue` for the same reason: it must fall through to the tick. | `test_dw_2_2_a_reload_lands_while_keys_are_arriving_continuously` feeds `j`/`k` continuously (~50/s) across the write and asserts the marker appears inside the budget. Mutation-checked against a *faithful* reproduction of the old shape (constant timeout **and** timeout-branch gating): the test fails with `the reload never landed … 5.000676375s elapsed with continuous input`, and passes with the fix. Note both halves are load-bearing — my first mutation attempt kept the shrinking timeout and the bug did not reproduce, which is itself evidence the shrink matters. |
| **2 — duplicated content re-anchored to the wrong copy** | `min_by_key(ordinal.abs_diff(anchor.ordinal))` compared candidate ordinals in the **new** document against the anchor's ordinal in the **old** one. Once an insertion above the reader exceeded half the spacing between identical blocks, nearest-ordinal dragged the reader backwards; exact ties always resolved to the earlier copy. The review measured this on its own fixtures: 10 of the 15 combinations it tried moved the reader, drift up to −24 lines. (That 15 is the reviewer's fixture count, not this repo's — the sweep added below runs **9** combinations, 3 gaps × 3 insertion sizes.) | The anchor now carries `(fingerprint, occurrence)` — "the *n*th block that paints this" — and `line_of_reloaded` picks the candidate at the same occurrence index. That is invariant to unrelated content appearing or disappearing above, which is the failing case. The `NodeId` fast path is **removed**, not merely gated: the reviewer is right that an id which resolves *and* hashes equal can still be the wrong copy, and the occurrence index needs the whole-document scan anyway, so one always-correct path beats two where the first is a trap. Missing occurrence clamps to the last match rather than refusing to anchor. | `test_dw_2_2_duplicated_content_re_anchors_to_the_copy_the_reader_was_on` sweeps `gap ∈ {1,3,10} × inserted ∈ {1,3,10}` — squarely inside the broken region — and asserts on the unique marker above the reader, which names *which* copy. Mutation-checked (`matches.first()` instead of the occurrence index): fails `unique-000` vs `unique-012`. |

The old duplicated-content test was **replaced, not renamed**: its duplicates sat 21 blocks apart while prepending one block, so the broken rule won by a margin of 20 and it passed while the rule was wrong. Separately, consolidating that test's helpers surfaced that round 2's `top_line_text`/`line_text` duplicated a pre-existing `topmost_line_text`/`line_text` pair in the same test module; the duplicates are gone and the **11** call sites that used them were repointed at the originals. (An earlier draft said 16; that was `grep -c` on the *result*, which also counted the surviving definition and the 4 pre-existing uses.) No test lost coverage.

## Post-review corrections (round 4)

The third review confirmed the round-3 pair fixed and independently reproduced the anchor fixtures at much larger scale, then demonstrated one remaining blocker.

**DW-2.2's latency bound failed under a sustained resize stream.** Round 3 bounded the *outer* loop's wait, but the resize debounce is an inner loop that re-armed a fixed 50 ms quiet period on every arriving event, entirely outside `until_next_tick()`'s reach. A resize stream faster than the debounce held it open indefinitely, so `watch_tick_due()` was never reached. Measured by the review: no reload at a 10 ms or 40 ms resize interval, working normally at 70 ms — the threshold was exactly `RESIZE_DEBOUNCE`.

The fix gives the burst a wall-clock ceiling as well as a quiet period. `Session::debounce_wait` caps each iteration's wait by the soonest of three things — the debounce quantum, a new `RESIZE_BURST_MAX` (200 ms), and, under `--watch`, the moment the next tick is owed — and a zero wait means "a deadline arrived, stop collecting" rather than "poll forever". The next loop iteration picks the stream back up, so nothing is dropped.

**Coalescing is preserved, and now asserted rather than assumed.** The debounce still folds a burst into one relayout; the burst is simply time-bounded instead of unbounded. Measured under a 2-second storm: **86 resize events → 9 frames**, i.e. ~200 ms of coalescing per repaint exactly as the ceiling intends. `test_a_sustained_resize_storm_keeps_repainting_rather_than_freezing` now asserts both directions — at least 2 frames (not frozen) and `frames * 3 <= resizes` (still coalescing), so removing the ceiling and removing the debounce would each fail it.

**On the freeze, asked about explicitly: yes, it was real, and it was not about `--watch`.** With the unbounded debounce, a viewer whose window is being dragged at any rate faster than 50 ms painted *nothing at all* until the drag stopped — reproduced here at zero bytes across 2.02 s of continuous resizing, with no `--watch` involved. A live window drag at 60 Hz emits a resize every ~16 ms, so this was reachable by ordinary use, not a synthetic rate. It is fixed by the same ceiling and covered by its own non-`--watch` test.

Both new tests are mutation-checked against the original unbounded debounce: they fail with `the viewer painted nothing at all across 2.019146375s of continuous resizing` and `the reload never landed during a sustained resize storm — 5.011354541s elapsed`, reproducing the review's measurements.

The pty harness gained `Pty::resize`, which drives `TIOCSWINSZ` on the master so the kernel delivers a real `SIGWINCH` — the tests exercise signal → crossterm → `Event::Resize`, not a fabricated event.

Two arithmetic claims in the round-3 notes above were wrong and are corrected in place: the sweep runs 9 combinations (not 15 — that was the reviewer's fixture count), and 11 call sites were repointed (not 16 — that grep counted the definition and the pre-existing uses too).

## Post-review corrections (round 5)

**A reload failure was never cleared when the reload later succeeded.** The success arm cleared `Session::last_failure`, but nothing cleared `AppState::status_message`, so `reload failed: … No such file or directory` sat under a correctly re-rendered document for ~100 frames — the reader was told the reload had failed while looking at its result.

Fixed one level deeper than suggested. Rather than clearing in `poll_reload`'s `Ok` arm, `AppState::reload_document` clears it — that is the single place a document is replaced, so the rule holds for every caller rather than for one call site.

**The audit the fix was asked to prompt found a second instance of the same shape.** There are exactly two producers of a transient status message: the reload failure, and `Ctrl-G`'s file info (`show_file_info`). The second has the identical defect — its byte and line counts describe the file that was open when the key was pressed, so a `--watch` reload landing inside the 100-frame TTL leaves numbers on screen measuring a document that no longer exists. Clearing at `reload_document` covers both, and any third producer added later. `clear_status`'s doc states the general rule: the TTL is a budget for a message the reader may not have finished reading, not a claim that the message is still true. No other part of the status path carries state that can outlive its condition — the ruler is recomputed from live state on every frame.

Test coverage: `test_a_watched_file_that_reappears_is_reloaded` now reads **whole frames** and asserts on row 24 (it previously stopped at `second-body` on the wire, which is before row 24 is even painted — that is why this survived four gates); plus two unit tests covering both producers. Mutation-checked: without `clear_status()` the pty test fails with the reviewer's exact string, and the unit test with `Some("reload failed: could not read file")` vs `None`.

**`+`/`-`/`T` during a resize burst — judged, and fixed rather than excused.** The two halves differ:

- `T` was a genuine input loss. The debounce drain read the key off the queue and offered it only to `handle_key_event`, which does not know `T`, so a theme toggle pressed while dragging a window edge was consumed and dropped. The drain now routes keys through `handle_chrome_key` first, in the same order the main loop uses. Covered by `test_a_theme_toggle_pressed_during_a_resize_burst_is_not_swallowed`, which compares the truecolor palette on the wire before and after; mutation-checked 3/3 red against the old drain.
- `+`/`-` are overridden either way, and that is correct. `apply_resize_burst` resyncs `content_width` to the terminal's actual width at the end of the burst — the pre-existing, documented "a real terminal resize always wins over a stale toggle" rule. They now *act* before being overridden rather than vanishing, which is consistent, but the visible outcome is unchanged and deliberately so.

Also fixed: the `watch_tick_due` doc comment had detached and merged into `debounce_wait` — the same misattachment shape as round 2's N4, introduced by my own round-3 edit inserting a function between a doc comment and its target. Both now carry their own.

Knowingly **not** addressed from round 2, all rated Low: N2 (the DW-2.2 pty timing budget is 2 s against a 250 ms interval), N3 (`mermaid::preprocess` has no production caller), N5 (media placements drop outside the sync block on reload — a flicker, not a tear), N6 (a future-dated mtime re-reloads every tick), N7 (a persistent reload failure stops being visible after the status TTL expires), N8 (watch ticks are starved while a key is held), N10–N12 (`use-dev-tty` reaches `crates/probe` through feature unification; `filedescriptor` pulls `thiserror` transitively; the no-ctty message names raw mode rather than the missing terminal).
