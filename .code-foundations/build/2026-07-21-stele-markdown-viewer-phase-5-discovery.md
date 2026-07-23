# Discovery + Design: Phase 5 - Viewport + paint

## Files Found

`crates/stele` does not exist yet — this phase creates it from scratch. Consumed (read, not modified):

- `crates/layout/src/lib.rs` — `layout()`, `LayoutTree`, `Line`, `Run`, `StyleId`, `Semantic`, `Reserved`, `CellSize`, `IntrinsicSizer`, `NullSizer`, `LayoutConfig` (exact signatures confirmed by reading the file, not guessed).
- `crates/layout/src/block.rs` / `inline.rs` — confirmed `CodeBlock` literal lines are emitted as `Semantic::CodeBlock`-tagged runs (`ctx.literal_block`), and that `IntrinsicSizer::size` is only consulted for `InlineKind::Image` (and, by the same seam, `Math`), producing `Line::Reserved` rows when it returns `Some`. This means a real `Reserved` line is constructible from `crates/stele`'s own tests by parsing an `![alt](path)` image and supplying a test `IntrinsicSizer` — no need to fabricate a `LayoutTree` by hand (its fields are private; only `layout()` constructs one).
- `crates/ast/src/ast.rs` — `NodeId(pub(crate) u32)` (opaque outside the crate, `Copy+Eq+Hash+Ord`), `Document::parse`, `InlineKind::Image { dest, title, children }`.
- `crates/width/src/engine.rs` — `WidthEngine::new(WidthConfig{ambiguous_wide})`, `cluster_width(&str)->u16`, `display_width(&str)->usize`, free fn `graphemes`.
- `crates/probe/src/launch.rs`, `lib.rs` — existing project error-handling convention: `thiserror`-derived enums with `#[error("...")]` messages, `#[source]` on wrapped I/O errors. Followed for `LoadError`.
- `docs/spikes/ghostty-caps.md` — mode 2026 verdict: **recognized, reset by default**, and **coexists with crossterm holding raw mode** (item 9, verified under concurrent `crossterm::event::poll`). No fallback to an owned differ is triggered.
- `Cargo.toml` (workspace root) — `resolver = "3"`, `edition = "2024"`, members list; `crates/probe`'s `Cargo.toml` shows `crossterm = "0.29"` already pinned workspace-wide as a precedent version.
- Verified directly against the vendored `crossterm-0.29.0` source (`~/.cargo/registry/.../crossterm-0.29.0/src/terminal.rs`): `BeginSynchronizedUpdate`/`EndSynchronizedUpdate` commands exist and emit exactly `CSI ?2026h` / `CSI ?2026l` — matching Spike A's measured verdict at the byte level.

## Current State

Workspace has four members (`ast`, `layout`, `probe`, `width`); no binary crate. CI runs fmt/clippy(-D warnings, --all-features)/test/linkage/spike-artifacts jobs against the workspace as a whole, so a fifth member is picked up automatically once added to `Cargo.toml`'s `members` list.

## Gaps

- No `crates/stele` directory, `Cargo.toml` entry, or `main.rs`/`lib.rs` — all created this phase.
- Plan's DW text (body) describes DW-5.2/5.3/5.5/5.6 as PTY-captured assertions; **the dispatch prompt explicitly relaxes this** ("you need not use real SIGWINCH", "test the error path, not a live process if simpler", "PTY capture" language dropped from the dispatch's own DW wording) — no PTY infrastructure is available or required in this build; all tests drive the painter/app-state seams directly with in-memory buffers, per the dispatch's own Constraints section ("Make the painter testable WITHOUT a real terminal ... Do not bury the paint logic behind unmockable global terminal state"). This is a resolved gap, not an open one — the dispatch prompt is the authoritative DW text for this build.
- `layout::Run`/`Line::Reserved`/`Semantic` etc. carry no per-line `NodeId` for `Line::Runs` (only `Line::Reserved` carries one). This bounds what "topmost visible block preserved across relayout" (DW-5.3) can mean exactly — see Design Decisions.

## Code Standards

No `docs/code-standards.md` found in the repo. Followed the project's own established conventions instead (read directly from `crates/probe`, `crates/layout`, `crates/width`): `thiserror` for typed errors with `#[error(...)]` display strings; `#![forbid(unsafe_code)]` crate-wide where achievable; doc comments on every public item explaining *why*, not just *what*; small leaf modules (`block.rs`/`inline.rs`/`table.rs` pattern in `layout`) rather than one large file.

## Test Infrastructure

`cargo test --workspace` (plain `#[test]`, no external test framework). `width` uses `proptest` as a dev-dependency for property tests and gates a live-Ghostty test behind a `corpus-tool` feature so the default build stays dependency-light — the same feature-gating pattern is available if a future phase needs it, not needed here. No existing integration-test (`tests/`) directory pattern to match beyond the general convention of one file per concern. `CARGO_TARGET_DIR=/tmp/stele-p5` used for every cargo invocation per the dispatch prompt.

## DW Verification

| DW-ID | Done-When Item | Status | Test Cases |
|-------|---------------|--------|------------|
| DW-5.1 | Open/scroll/quit works; terminal state restored on quit AND on induced panic (Drop guard + panic hook; unit test asserts the guard runs and emits the restore sequence) | COVERED | `app::tests::test_dw_5_1_scroll_navigation_and_quit`; `terminal::tests::test_dw_5_1_guard_drop_emits_restore_sequence`; `terminal::tests::test_dw_5_1_panic_hook_body_emits_restore_sequence` |
| DW-5.2 | Full scroll of a 10k-line document: every emitted frame wrapped in paired mode-2026 begin/end markers; no differ | COVERED | `tests/painter_frame.rs::test_dw_5_2_full_scroll_frames_are_2026_paired` |
| DW-5.3 | Resize storm (50 simulated events, driven directly): no crash, final layout correct for final width, topmost visible block preserved | COVERED | `app::tests::test_dw_5_3_resize_storm_no_crash_and_final_width_correct`; `app::tests::test_dw_5_3_topmost_visible_block_preserved_across_resize_storm` |
| DW-5.4 | Missing file and invalid-UTF-8 input each produce a clean error message and nonzero exit code | COVERED | `loader::tests::test_dw_5_4_missing_file_produces_clean_error`; `loader::tests::test_dw_5_4_invalid_utf8_produces_clean_error`; `tests/cli_errors.rs::test_dw_5_4_missing_file_exits_nonzero_with_clean_stderr`; `tests/cli_errors.rs::test_dw_5_4_invalid_utf8_exits_nonzero_with_clean_stderr` |
| DW-5.5 | Injection fixture (ESC/OSC/APC/DECSET bytes in document text) renders inert: captured output has no escape bytes other than the painter's own known sequences | COVERED | `tests/painter_frame.rs::test_dw_5_5_injection_fixture_renders_inert` (whitelist scanner: every `0x1B` byte in the captured frame must open one of the painter's own fixed CSI shapes — sync begin/end, cursor-position, clear-to-EOL, SGR — or the test fails) |
| DW-5.6 | `--max-width 60` clamps content width to 60 on a wider (100-col) terminal | COVERED | `tests/painter_frame.rs::test_dw_5_6_max_width_flag_clamps_layout_width`; `cli::tests::test_dw_5_6_cli_parses_max_width_flag` |

**All items COVERED:** YES

## Design Decisions

**1. `crates/stele` is both a lib and a bin.** P6/P7 implement `MediaSink`/`Decor` from separate crates (`crates/gfx`, `crates/highlight`) and must import the trait definitions; a bin-only crate can't be depended on. `src/lib.rs` exposes `painter`, `media`, `decor`, `app`, `cli`, `loader`, `terminal` as public modules and re-exports the paint-facing types (`Run`, `StyleId`, `Style`, `Reserved`, `CellSize`, `NodeId`) per the plan's `Produces`. `src/main.rs` is thin glue: parse CLI → load file → build `Document`/`WidthEngine`/`LayoutConfig` → enter terminal → run the event loop → drop the guard. All logic with a decision to test lives in the lib; `main.rs` itself is not unit-tested (it is glue over real crossterm I/O), consistent with "Make the painter testable WITHOUT a real terminal."

**2. Paint emission is hand-rolled raw ANSI bytes, not `crossterm::style`/`queue!`.** Terminal *plumbing* (raw mode, alternate screen, input events, resize events) uses crossterm per the plan's approach note. Paint *content* (SGR runs, cursor positioning, clear-to-EOL, the 2026 markers) is written as raw byte constants/`write!` calls directly to the `&mut dyn Write` the painter is handed. Rationale: DW-5.2 and DW-5.5 both require byte-exact assertions over the emitted stream ("every frame wrapped in paired markers", "no escape bytes other than the painter's own known sequences") — hand-rolled bytes make the barricade's output surface a small, closed, enumerable set the test can whitelist-scan, rather than trusting a library's internal formatting. Crossterm's own `BeginSynchronizedUpdate`/`EndSynchronizedUpdate` were confirmed (by reading the vendored source) to emit the identical bytes (`CSI ?2026h`/`l`), so this is a testability/control choice, not a capability gap.

**3. `Painter::frame`'s signature adds `&mut self` and `out: &mut dyn Write` to the plan's `Painter::frame(&LayoutTree, scroll: usize, size: Size)`.** The plan's own Constraints section requires this exact addition ("`frame(...)` should write to a `&mut dyn Write`... so tests can capture and assert on the exact byte stream") — the three named arguments are preserved verbatim and in order; `&mut self` is needed because `Painter` owns registered `media`/`decor` trait objects and a `WidthEngine`. This is implementing the constraint, not deviating from the contract.

**4. `Decor::highlight` is invoked only for `Semantic::CodeBlock` runs, with `lang: None`.** The trait's own documented purpose (P7's approach note: "Token splitting is paint-side... `Decor::highlight(line_text, lang) -> Vec<Run>` is a pure run transformation") ties it to code-block syntax highlighting specifically, not general text. `layout::Run` carries no fence-info-string/language association at the line level (that association lives on the `BlockKind::CodeBlock.info` field, several layers above the flattened `Line` the painter sees) — plumbing the real language through is P7's job (its file scope explicitly owns `crates/stele/src/decor/**`). P5 wires the seam correctly and completely (the trait is called, at the right place, with the right shape of input) but passes `lang: None` since no per-line language is available yet; this is recorded here rather than silently assumed, per the "never guess" discipline. The default `StructuralDecor::highlight` is the identity behavior the plan describes: it hands back the whole line as one unchanged `Semantic::CodeBlock` run (matching "no highlighting" — a plain code block) with `width: 0`, which the painter recomputes.

**5. "Topmost visible block preserved" (DW-5.3) is implemented as proportional scroll-position preservation, not block-identity anchoring.** `LayoutTree::Line::Runs` carries no `NodeId` (only `Line::Reserved` does) — there is no public seam in `crates/layout` to map "the block that was on line N" forward into a re-laid-out tree. `AppState::relayout` instead preserves `scroll / max_scroll` as a fraction and reapplies it to the new `max_scroll`, which: (a) exactly preserves position when a resize doesn't change wrapping for the visible region (the common case, and the case DW-5.3's test targets with non-reflowing content so the assertion is exact, not approximate); (b) keeps the document start visible when the document was fully visible before resize (`old_max == 0` ⇒ `ratio == 0`); (c) degrades gracefully (never panics, never leaves scroll out of bounds) when wrapping does change. Exact block-identity anchoring would require `crates/layout` to carry a `NodeId` per `Line::Runs`, which is out of this phase's file scope (`crates/layout/**` is not in Phase 5's file scope) — flagged here rather than silently reinterpreted.

**6. EINTR is handled by `std::fs::read` itself, not re-implemented.** `Read::read_to_end` (which `fs::read` uses) retries internally on `ErrorKind::Interrupted` — this is a documented guarantee of Rust's standard I/O, not project-specific code. No additional retry loop is written; this is recorded so the "unhandled edge case" checklist item isn't silently dropped.

**7. Unreadable-file (permission-denied) is not given an independent test.** It resolves through the exact same `LoadError::Io` branch as "missing file" (both are `fs::read` failures) and the missing-file test already exercises that branch and message shape end-to-end (including the real binary's exit code). A dedicated `chmod`-based test was considered and rejected: many CI containers run as root, where Unix permission bits don't block reads, making such a test either flaky or silently vacuous — a worse outcome than not writing it.

**8. `LayoutContext<'a>` parameter object.** `AppState::relayout`/`apply_resize_burst` need `&Document`, `&LayoutConfig`, `&WidthEngine`, `&dyn IntrinsicSizer` together; bundling them avoids a 6-parameter routine (cc-routine-and-class-design: parameter thresholds) and gives the four arguments a name as a group, since they always travel together for the lifetime of one document session.

**9. Panic-hook testing does not call `std::panic::set_hook` inside the test binary.** Rust test binaries are multi-threaded and share one process-global panic hook; installing a real hook from a test would leak into every other test's assertion-failure reporting and could corrupt unrelated test output under `cargo test`'s default parallel runner. Instead, the hook's entire body is factored into a private, directly callable function (`terminal::on_panic(&mut dyn Write)`) that `install_panic_hook`'s closure calls with real `io::stdout()`; the test calls `on_panic` directly with a capture buffer, which proves the exact bytes the hook would emit without ever mutating global process state. Combined with the separate `TerminalGuard::for_test`-based Drop test, this satisfies the dispatch's literal ask ("a unit test can assert the guard runs and emits the restore sequence") for both halves (guard, hook) without the cross-test-contamination risk of a real global hook swap.

**10. Sanitization strips C0 (0x00–0x1F), DEL (0x7F), and C1 (0x80–0x9F).** The plan's barricade text says "C0 ... and C1"; DEL (0x7F) is added defensively (cc-defensive-programming: barricade completeness) since it is a control character with no legitimate printable use in document text and costs nothing extra to strip. ESC (0x1B) is already inside the C0 range, so every named injection form (raw ESC, OSC `ESC ]`, APC `ESC _`, DECSET `ESC [?...h`) is neutralized by removing its leading ESC byte alone — nothing after a stripped ESC can reassemble into an escape sequence.

**11. Horizontal clipping and width recomputation follow the Phase 4 `u16` gotcha exactly.** `clip_to_width` accumulates in `usize` (via `WidthEngine::cluster_width(..) as usize`) and only casts/saturates to `u16` once, at the point a clipped run's final width is stored — never `display_width(..) as u16` bare. Clipping walks `width::graphemes`, never raw bytes or `char`s, so a multi-byte cluster is never split.

## Prerequisites

- [x] Phase 1 (`crates/probe`, spike verdicts) — present, read.
- [x] Phase 4 (`crates/layout`) — present, read; exact signatures used, not guessed.
- [x] `crossterm = "0.29"` already used elsewhere in the workspace (`crates/probe`) — version precedent confirmed; 2026 support confirmed against vendored source.
- [x] No missing prerequisites.

## Recommendation

BUILD.
