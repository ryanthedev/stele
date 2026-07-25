# Discovery + Design: Phase 1 - Status line, viewport chrome, and runtime toggles

## Files Found
All exist; none need creating.
- `crates/stele/src/app.rs` — `AppState`, `LayoutContext`, `relayout`/`apply_resize_burst` (anchor preservation already correct and tested).
- `crates/stele/src/painter.rs` — `Painter::frame`, `sanitize`, `clip_to_width` (barricade primitives to reuse).
- `crates/stele/src/main.rs` — `run_session` event loop; unbuffered `stdout().lock()` write target today.
- `crates/stele/src/terminal.rs` — `TerminalGuard`, `install_panic_hook`, `RESTORE_SEQUENCE`.
- `crates/width/src/engine.rs` — `WidthEngine::display_width` (grapheme-segmentation loop, no fast path yet).
- `crates/stele/tests/painter_frame.rs` — `assert_only_known_escapes` whitelist scanner; `test_degenerate_viewports_and_scrolls_still_emit_one_anchored_sync_block` already tolerates `rows <= h+1` — one row of headroom that anticipates exactly this feature.
- `crates/highlight/src/theme.rs` (out of file scope, read-only) — already has `test_heading_tiers_clear_wcag_aa_against_the_reference_backgrounds` for both variants; DW-1.5 can lean on it rather than duplicate WCAG math into production code.

## Current State
`AppState` has no status/message concept, no independent content-width state (layout width == terminal width always), and `handle_key_event(key) -> bool` has no access to `LayoutContext`. `Painter::frame` paints exactly `size.height` rows, no reserved row. `run_session` writes straight to `stdout().lock()`, unbuffered. `Ctrl-g` (lowercase, CONTROL) currently falls through to the unmodified `'g'` binding (jump to top) — an accident of the "strictly additive" Ctrl fallthrough design, not a deliberate binding.

## Gaps
- No status/chrome concept anywhere.
- `Ctrl-g` is already bound (jump-to-top, via fallthrough) — direct conflict with DW-1.3, addressed below (Decision 1).
- `display_width` has no ASCII fast path.
- `stdout` unbuffered; no BufWriter; no defense against a BufWriter flushing a stale half-frame after a panic-driven restore.

## Code Standards
Applied: exhaustive matches (no wildcard arms) on `Semantic`/style tables; hand-rolled error enums (none needed this phase — no new fallible surface); sentence-style test names; DW-tagged test names; shared pty harness under `tests/common/`; three-group import ordering; never cast width `as u16` (all new width arithmetic here is either `u16`-native config clamps or already-saturated `usize`/`u32`, no new cast site).

## Test Infrastructure
`crates/stele/tests/common/{pty,render,fixtures}.rs` — real-pty harness (`Pty`, `read_until`, `contains`, `assert_restores_the_terminal`) and a cell-grid replay model (`render_row`) driven purely by painted bytes. `crates/width/tests/common/mod.rs` — `load_pinned_corpus()`, out of this phase's file scope (`crates/width/tests/**` not listed), so corpus-grounded width tests for this phase live inside `crates/width/src/engine.rs`'s own `#[cfg(test)] mod tests` instead, reading the same JSON directly.

## DW Verification

| DW-ID | Done-When Item | Status | Test Cases |
|-------|---------------|--------|------------|
| DW-1.1 | Exactly one status row reserved; content height `rows-1`; no overpaint | COVERED | `tests/status_row.rs::test_dw_1_1_*` (content height, no content on status row, degenerate 1/2-row terminal) |
| DW-1.2 | Position % reads 0 at top, 100 at max_scroll | COVERED | `app.rs::test_dw_1_2_*` (top, mid, max_scroll==0 case) |
| DW-1.3 | Ctrl-G shows name/byte size/line count; clears after bounded frames | COVERED | `app.rs::test_dw_1_3_*` (content, TTL decay, empty file, no-trailing-newline file) |
| DW-1.4 | `+`/`-` adjust width within clamp; top visible block preserved | COVERED | `app.rs::test_dw_1_4_*` (widen/narrow preserve anchor, clamp boundary no-op) |
| DW-1.5 | `T` swaps variant; every heading clears WCAG AA in new variant | COVERED | `tests/theme_toggle.rs::test_dw_1_5_*` (both variants' heading fg differ + both clear AA, reusing the public `highlight`/`stele::decor::themed` API — no edits to `crates/highlight`) |
| DW-1.6 | `BufWriter`; panic mid-frame leaves no buffered bytes after restore | COVERED | `tests/panic_mid_frame.rs::test_dw_1_6_*` (real pty, env-var fault injection) |
| DW-1.7 | ASCII fast path identical to old path on corpus; ≥2x faster, committed benchmark | COVERED | `width/src/engine.rs::test_dw_1_7_*` (corpus-derived mixed strings, pure-ASCII vs old-formula equivalence, control-byte/multibyte exclusion, timing benchmark) |

**All items COVERED:** YES

## Design Decisions

1. **Ctrl-g repurposed, not added alongside.** DW-1.3 pins the same chord (`ctrl('g')`, lowercase) the existing `handle_control_chord` fallthrough currently sends to "jump to top" — that behavior was explicitly the *pre-Ctrl-aware* binding preserved for backward compatibility ("strictly additive... when the chord means nothing to us"), not a deliberate feature. Ctrl-g now means something to us, so per the established pattern (Ctrl-c/d/u/f/b already stop falling through), it gets its own arm and no longer falls through. `test_control_falls_through_to_the_pre_existing_binding`'s `ctrl('g')` assertion is updated (not deleted outright — replaced with a comment explaining the reassignment and a pointer to the new DW-1.3 test). Ctrl-G (uppercase, end-of-doc) is untouched.

2. **`Painter::frame` stays; `Painter::frame_with_status` is new and does the real work.** ~25 existing call sites (several in `examples/*.rs`, outside file scope) call `Painter::frame` with the old 4-arg signature. Rather than break all of them, `frame` becomes a one-line wrapper calling `frame_with_status(..., &StatusLine::default(), out)` — so DW-1.1's "a rendered frame reserves exactly one status row" is universally true (both entry points reserve it), while zero existing call sites need touching. Verified against every existing `.frame(` call site: none assert exact/total row-count equality that an appended trailing row would break; `test_degenerate_viewports_and_scrolls_still_emit_one_anchored_sync_block`'s `rows <= h+1` tolerance already has exactly the headroom this adds.

3. **`AppState` owns `content_width: u16`, decoupled from `size.width`.** `relayout(ctx, width, new_size)` already supports layout-width ≠ viewport-width as a matter of its existing signature; `+`/`-` only had to start using that degree of freedom. `content_width` is resynced to `tree.width()` at the end of every `relayout` call (including plain terminal resizes), so a live resize always wins over a stale toggle override, and repeated `+`/`-` presses compose correctly at the clamp boundary (clamped eagerly, not deferred — avoids a "dead zone" where several `+` presses at the ceiling silently absorb the next `-` press).

4. **`relayout_preserving_anchor(&LayoutContext, LayoutConfig)` is the pinned entry point; `+`/`-`/`T` all route through it.** Per the plan's own approach note ("the toggles are what force repeated relayout"), `T` calls it too even though nothing in the tree changes for a theme swap — cheap (one `no_reflow_occurred`-detected no-op reflow) and keeps every chrome-mutating key going through one anchor-preserving path rather than a special case per key.

5. **Theme state lives in `main.rs`, not `AppState`.** `AppState` has no knowledge of `Decor`/`Theme` today and none is added — `Painter::register_decor` is the only mutator, owned by `main.rs`. `run_session` tracks its own `(Variant, ColorMode)` pair, flips it on `T`, and calls `state.relayout_preserving_anchor(...)` per Decision 4. This keeps `AppState` pure/unit-testable (no `highlight` dependency added to `app.rs`).

6. **`+`/`-`/`T` are intercepted in `run_session`, not inside `AppState::handle_key_event`.** They need resources `handle_key_event`'s existing `(key) -> bool` signature doesn't carry (`ctx` for relayout; `painter`+theme state for `T`). Widening that signature would touch ~30 existing call sites for no benefit this phase — Ctrl-G needed no such widening because `FileInfo` is static per-session data baked into `AppState` at construction, so it *can* stay fully inside `handle_control_chord`. A new `handle_chrome_key` free function in `main.rs` checks these three codes (guarded on no CONTROL modifier) before falling through to `state.handle_key_event`; it's covered by the DW-1.4/1.5 tests exercising the `AppState`-side methods it calls directly (`widen`/`narrow`/`relayout_preserving_anchor`), not by testing `run_session` itself (consistent with `main.rs`'s existing "not itself unit tested" stance).

7. **`FileInfo` is computed once at load and passed into `AppState::new`.** Byte size and line count are properties of the file on disk, not the preprocessed (frontmatter/mermaid-stripped) source — captured immediately after `loader::load_document` succeeds, before those transforms shadow the variable. `line_count` uses `str::lines().count()`, which is trailing-newline-invariant (DW-1.3's dirty case) rather than `\n`-occurrence counting, which would differ by one depending on a trailing newline's presence.

8. **`BufWriter` safety: a poisoned, dependency-injected `Write` wrapper, not reliance on `BufWriter::drop`.** `BufWriter::drop` best-effort-flushes its buffer unconditionally, including during panic unwinding — which would let a half-painted frame's buffered bytes reach the terminal *after* `terminal::on_panic` has already written `RESTORE_SEQUENCE`. `terminal::PanicGuardedWriter<'a, W>` wraps the real stdout handle and silently discards every `write`/`flush` once a `&'a AtomicBool` flag it holds is poisoned; `install_panic_hook` poisons the *process-global* instance before writing the restore sequence. The type takes the flag by reference (not a hardcoded global) specifically so unit tests can poison a private, non-shared `AtomicBool` with zero cross-test race risk — the existing `terminal::signals` module's global statics already show the concurrency hazard a shared-mutable-static test would have.

9. **DW-1.6's fault injection is a `main.rs`-local, env-var-gated `Write` wrapper (`PanicAfterBytes`), not a `Painter`-level test hook.** The panic must occur while genuine frame bytes sit unflushed inside the real `BufWriter`/`PanicGuardedWriter` stack in the *spawned subprocess* the pty test drives — an env var is the only way to configure that across the process boundary `Command::spawn` puts up. Mirrors the existing `GfxMediaSink::force_*_failure_for_test` precedent (a production-shipped, narrowly-named, opt-in-only test hook), just parameterized by env var instead of a setter because the pty harness cannot call methods on the child process.

10. **ASCII fast path: printable range only (0x20..=0x7E), never bare `is_ascii()`.** `correction.rs::base_char_width` returns 0 for most control characters (`unicode-width` returns `None` for them), so a fast path treating *any* ASCII byte (including C0/DEL) as width 1 would diverge from the slow path exactly on the Test Plan's named dirty case. Implemented as a free function (`is_printable_ascii`), not a method — the decision needs no `WidthConfig` state, since ASCII's East Asian Width class is always Narrow/Neutral regardless of the `ambiguous_wide` policy bit.

11. **DW-1.7's "exercised by the corpus" requirement is satisfied two ways.** (a) A new test in `engine.rs` reads `corpus/ghostty-1.3.1-widths.json` directly (duplicating the tiny bit of loading logic `crates/width/tests/common/` has, since that module is out of this phase's file scope) and checks `display_width` on corpus clusters padded with ASCII on both sides against the corpus's measured widths. (b) The pre-existing `crates/width/tests/property_display_width.rs::test_display_width_still_agrees_with_a_literal_per_cluster_sum` — already committed, its own doc comment states it is inert until `display_width` "grows a faster path" — becomes a real, passing regression check the moment the fast path lands; no edit needed since it's out of file scope and already correct.

## Prerequisites
- [x] All files exist.
- [x] No missing dependencies — `highlight` and `layout` are already `stele` dependencies; `serde_json` is already a `width` dev-dependency, available to `#[cfg(test)]` code in `src/`.

## Recommendation
BUILD.
