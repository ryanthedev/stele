# Plan: gfx SVG dependency-claim pins
**Created:** 2026-07-28
**Status:** in-progress
**Started:** 2026-07-28
**Current Phase:** 1
**Complexity:** simple
**Review cadence:** 1
---
## Context

**Problem:** `b1b2cee` fixed the entity-bomb guard in `crates/gfx/src/svg.rs` and rewrote the module header to state what was actually measured, but the correction never reached the rest of the file. Five artifacts of the disproven argument survive, and each reads as a live safety claim:

| # | Site | Surviving claim | Why it is wrong |
|---|---|---|---|
| 1 | `svg.rs:66-72` | `MAX_SVG_BYTES` is justified as the number that turns *"`roxmltree`'s 'expansion is linear in the source'"* into an absolute ceiling | Growth is **quadratic**; the header at `svg.rs:20-22` says so and contradicts this |
| 2 | `svg.rs:673-684` | source *"can expand about 255x through entity references before `roxmltree` refuses"* | The 255 cap is per **root** reference and disabled at depth zero (`roxmltree-0.21.1/src/parse.rs:531`) |
| 3 | `svg.rs:687` | `const ROXMLTREE_MAX_EXPANSION: u64 = 255` | A dependency constant **restated, never compared** to anything roxmltree owns — it cannot fail if roxmltree changes |
| 4 | `svg.rs:133-140`, `svg.rs:608-609` | *"`usvg` sets no limit of its own, so this is the whole of that bound"* | The header at `svg.rs:23-27` cites usvg's 1,000,000-element and `<use>`-depth-1024 caps and calls `MAX_XML_NODES` *"a tightening rather than the only bound"* |
| 5 | `svg.rs:623-625` | *"`roxmltree` is what bounds this, and the bound is asserted rather than trusted"* on the entity-bomb test | Wrong twice. `refuse_internal_subset` is what bounds it — the fixture declares an internal subset (`svg.rs:629-637`) and is refused at `svg.rs:343` before roxmltree resolves an entity. And the bound is *not* asserted: the test checks only `DecodeError::Malformed(_)` (`svg.rs:640`), which the subset refusal produces, so it would pass identically if roxmltree bounded nothing |
| 6 | `svg.rs:23-27` | The 1,000,000-element cap is attributed to `usvg-0.47.0/src/parser/mod.rs:36` | That is the doc on `Error::ElementsLimitReached`, which is **declared (`mod.rs:37`) and `Display`ed (`mod.rs:65`) but never constructed** — verified, those are its only two occurrences in the crate. The live check is `if doc.nodes.len() > 1_000_000 { return Err(Error::NodesLimitReached) }` at `svgtree/parse.rs:392`. The cap is real and does apply on this path; only the pointer is dead |

Separately, `test_probing_a_labelled_drawing_does_not_load_fonts` (`svg.rs:797-813`) pins "probe loads no fonts" only through `elapsed < RENDER_TIME_CAP`. That assertion is **order- and clock-dependent, not unconditionally vacuous**: with a cold `OnceLock` it would catch a regression (the ~480 ms load lands inside `within_time_cap` and `probe` returns `Malformed`), but `fonts()` is a process-wide `OnceLock` (`svg.rs:283-290`) that `test_text_in_a_drawing_is_actually_drawn` warms in the same binary, and on a container with few fonts installed the load can finish inside 250 ms. Under either condition the assertion goes quiet. The fact being pinned is static — `probe` passes a constant `Fonts::Never` (`svg.rs:402`) — so pinning it with a stopwatch is the wrong instrument regardless.

**Constraints:**
- No change to guard *behaviour*. `refuse_internal_subset`, `MAX_SVG_BYTES`, `MAX_XML_NODES`, `RENDER_TIME_CAP` and the `OnceLock` font caching keep their current semantics and values. This is documentation and test-validity work.
- Every claim that survives must be either a fact about stele's own code or a behaviour a test would actually catch. No third-party constant restated as a local `const`.
- The perishable measurements (76 KiB → 256 MiB → 4.7 s; 808 bytes → 60 ms) already appear at `svg.rs:18-20` and `svg.rs:712-713`. Corrected prose cross-references the module doc rather than adding a third copy.
- Workspace rules hold: `#![forbid(unsafe_code)]`, `test_*` naming, clippy clean.
- `docs/code-standards.md` (generated 2026-07-25, base-commit `85d878e`) is a few commits stale but its conventions are unchanged by this work; regenerating it here would put an unrelated diff in these commits, so it is deliberately left alone.

**Success criteria:** No comment in `svg.rs` attributes a bound to `roxmltree` or `usvg` that those crates do not provide; no test asserts a number only a dependency owns; and "probe requests no font database" is pinned by an order-independent, clock-independent assertion that is *demonstrated* to fail when `probe` is switched to `Fonts::IfPresent`.

---
## Implementation Phases

### Phase 1: Make every surviving dependency claim true
**Skills:** code-foundations:code-clarity-and-docs
**Model:** opus
**Gate:** Standard
**Depends on:** none
**File scope:** crates/gfx/src/svg.rs

**Goal:** Bring all six surviving claims into agreement with what the dependencies actually do, and delete the restated roxmltree constant along with the argument it served.

**Scope:**
- IN: `svg.rs:66-72` (`MAX_SVG_BYTES` doc), `svg.rs:133-140` (`MAX_XML_NODES` doc), `svg.rs:608-610` and `svg.rs:623-625` (test doc comments), `svg.rs:673-684` (the 255x rationale), `svg.rs:685-704` (the test body), the test's name, and — **citation only** — the two dead pointers in header item 2: `svg.rs:23-27` (`mod.rs:36` → `svgtree/parse.rs:392`, error named `NodesLimitReached` not `ElementsLimitReached`) and `svg.rs:29-31` (`mod.rs:147` sits inside `from_str`, declared at `mod.rs:146`; `from_data` is `mod.rs:98` and reaches it indirectly).
- OUT: The module header's *narrative* at `svg.rs:1-50` — correct, and the source of truth the rest is reconciled *to* — except the two citations named above, which are corrected in place without touching the surrounding argument. Also out: `svg.rs:10-17` and `svg.rs:706-716`, the two entity-expansion comments that are already correct post-`b1b2cee` and keep the per-root/depth-zero facts as the history explaining why the old fixture passed. And `refuse_internal_subset` and every other function body; all cap values; every other test's assertions.

**Edge cases:**
- The 255 figure is not false — it is real but per root reference and disabled at depth zero. Prose that erases it loses the reason the old fixture passed; prose that keeps it as a bound repeats the defect. It survives only as history, explicitly marked non-load-bearing.
- Deleting the whole test loses its genuine half: the node cap must be reachable within the byte cap or one of the two is decoration. That assertion stays.
- After the 2 GiB assertion goes, `MAX_SVG_BYTES` must not be left with *no* argument at all — the replacement is that no internal subset means no expansion, so peak text ≈ source bytes ≤ `MAX_SVG_BYTES`.
- The cross-reference to `decode.rs`'s `test_dw_6_3_no_input_the_dimension_cap_admits_can_exceed_the_allocation_cap` (`decode.rs:641`) is the raster counterpart of this test and must survive the rewrite.

**Produces:** A `svg.rs` whose every dependency-attributed claim matches the module header, and a `tests` module containing no restated third-party constant. Phase 2 adds to that module and must not reintroduce one.

**Done when:**
- [ ] DW-1.1: No comment in `svg.rs` attributes an entity-expansion bound to `roxmltree` that is **operative today** (sites 1, 2 and 5). Each states `refuse_internal_subset` as the operative bound, cross-referencing the module doc for the measurement rather than restating it. The per-root and depth-zero facts survive as history where they explain why the old fixture passed: `svg.rs:10-17` and `svg.rs:706-716` are explicitly not changes, so this item is not a bare grep gate.
- [ ] DW-1.2: `MAX_SVG_BYTES`'s doc states its replacement argument — no internal subset ⇒ no expansion ⇒ peak text ≈ source bytes — with no appeal to any roxmltree expansion behaviour.
- [ ] DW-1.3: No comment claims `usvg` sets no limit (site 4). Both sites agree with the corrected header: usvg caps at 1,000,000 nodes and `<use>` depth 1024, and `MAX_XML_NODES` is a tightening applied a stage earlier.
- [ ] DW-1.4: The usvg citation is live code at all three sites (site 6). `svgtree/parse.rs:392` and `Error::NodesLimitReached` replace `mod.rs:36`/`ElementsLimitReached` everywhere the cap is cited, and no site rewritten under DW-1.3 copies the dead pointer forward. `grep -c ElementsLimitReached crates/gfx/src/svg.rs` returns 0.
- [ ] DW-1.5: `grep -c ROXMLTREE_MAX_EXPANSION crates/gfx/src/svg.rs` returns 0, and no other `const` in the module restates a dependency-owned value.
- [ ] DW-1.6: The surviving test is renamed to what it now asserts (e.g. `test_the_node_cap_is_reachable_within_the_byte_cap`), and its doc records the actual headroom (`MAX_SVG_BYTES` 4,194,304 vs `MAX_XML_NODES` × 4 = 800,000, a 5.24× margin) so a future reader knows how loose the pin is. The `decode.rs:641` cross-reference survives.
- [ ] DW-1.7: `cargo test -p gfx` passes and `cargo clippy --all-targets` is silent.

### Phase 2: Pin "probe requests no fonts" without a stopwatch
**Skills:** code-foundations:cc-quality-practices
**Model:** opus
**Gate:** Standard
**Depends on:** Phase 1
**File scope:** crates/gfx/src/svg.rs

**Goal:** Replace the order- and clock-dependent timing assertion with a pure, order-independent one, and prove it fails under the regression it exists to catch.

**Scope:**
- IN: Extracting the font-database selection at `svg.rs:355-357` into a pure function (`fn fontdb_for(fonts_needed: Fonts, source: &str) -> Arc<usvg::fontdb::Database>`); naming the mode `probe` passes as a `const` used at `svg.rs:402`; the body and doc comment of `test_probing_a_labelled_drawing_does_not_load_fonts`.
- OUT: `probe`'s behaviour (it keeps passing `Fonts::Never`), the `needs_fonts` predicate and its test, `test_text_in_a_drawing_is_actually_drawn`, the `OnceLock` caching, and `within_time_cap`.

**Approach (decided, not left open):** a runtime call counter on `fonts()` was considered and **rejected** — `within_time_cap` runs `parse` on a spawned thread and `test_text_in_a_drawing_is_actually_drawn` calls `fonts()` concurrently, so a global counter would need a serializing test mutex to avoid spurious failures, which is more machinery than a static fact deserves. The fact is pure: `parse`'s database selection is a function of `(Fonts, source)`. Assert it as one.

**Edge cases:**
- `fontdb::Database::is_empty()` is public and inherent at `fontdb-0.23.0/src/lib.rs:607`, reached through usvg's wholesale `pub use fontdb;` (`usvg-0.47.0/src/lib.rs:68`), and `Cargo.lock` resolves exactly one fontdb — so `fontdb_for(Fonts::Never, labelled).is_empty()` compiles with no new dependency or import (`svg.rs:283` already names `usvg::fontdb::Database`). The `Arc::ptr_eq(&fontdb_for(..), &no_fonts())` alternative pins the same fact if needed.
- `Options.fontdb` is `#[cfg(feature = "text")]` (`usvg-0.47.0/src/parser/options.rs:96`), so `fontdb_for`'s signature inherits that gate. `crates/gfx/Cargo.toml` enables `text` explicitly — this is the same gate `svg.rs:357` already sits behind, not new exposure.
- The `const PROBE_FONTS: Fonts = Fonts::Never;` assertion is shallow on its own — someone could inline `Fonts::IfPresent` at the call site and evade it. It is load-bearing only in conjunction with the `fontdb_for` assertion, and the DW below requires the conjunction to fail under the mutation, not either half.
- No `cfg(test)` conditional may decide which database `parse` receives; test and release builds must take the same path.
- The timing assertion may stay as a secondary signal, but must no longer be the thing that catches the regression.

**Produces:** n/a (terminal phase)

**Done when:**
- [ ] DW-2.1: The font-database selection is a pure function of `(Fonts, source)`, callable from a test with no reference to elapsed time, `OnceLock` initialization state, or test execution order.
- [ ] DW-2.2: `test_probing_a_labelled_drawing_does_not_load_fonts` asserts that the database `probe` selects for a text-bearing drawing is empty, and that the mode `probe` passes is `Fonts::Never`. It passes.
- [ ] DW-2.3: Demonstrated failure: with `probe` temporarily switched to `Fonts::IfPresent`, the new assertion fails and the failure output is recorded in the execution log. The old timing-only assertion's outcome under the same mutation is also recorded, together with the conditions (`OnceLock` warmth, host font-load speed) that determine it. The mutation is reverted.
- [ ] DW-2.4: No `cfg(test)` conditional determines which `fontdb` `parse` receives — verified by inspecting the diff for `cfg(test)` inside the parse path.
- [ ] DW-2.5: Full workspace suite green — the `b1b2cee` baseline is `806 passed, 6 ignored`, and the count may only rise — and `cargo clippy --all-targets` silent. Suite re-run at `--test-threads=16` to confirm order-independence.

---
## Test Coverage
**Level:** Every behavioural done-when item verified by execution. The prose items (DW-1.1, DW-1.2, DW-1.3, DW-2.4) are verified by reading plus the review pass; DW-1.4 and DW-1.5 by exact grep counts.

## Test Plan
- [ ] The renamed cap-relationship test passes after the constant is removed — DW-1.6.
- [ ] Dirty test, Phase 1: raising `MAX_XML_NODES` past **1,048,576** (the point where `MAX_XML_NODES × 4` exceeds `MAX_SVG_BYTES`) fails that test; a smaller bump does not, which is why the threshold is stated. Reverted after demonstrating — DW-1.6.
- [ ] `test_a_wide_entity_bomb_is_refused`, `test_a_plain_doctype_is_still_accepted` and `test_an_entity_bomb_is_refused_rather_than_expanded` still pass, proving the prose edits did not touch guard behaviour — Phase 1 constraint.
- [ ] `test_probing_a_labelled_drawing_does_not_load_fonts` passes with the pure assertions — DW-2.2.
- [ ] Dirty test, Phase 2: `probe` switched to `Fonts::IfPresent` fails the new assertion; the old timing assertion's behaviour under the same mutation is recorded — DW-2.3.
- [ ] `test_text_in_a_drawing_is_actually_drawn` still passes, proving the font path is intact — DW-2.5.
- [ ] `cargo test --workspace`, then again at `--test-threads=16`; `cargo clippy --all-targets` — DW-1.7, DW-2.5.

---
## Notes
- Practical exposure from all five prose defects is low: `refuse_internal_subset` means custom entities can no longer be declared at all, so the 255x arithmetic is unreachable in the current code. The reason to fix them is that stale prose reads as a live safety argument — the precise failure mode `b1b2cee` was written to correct, left half-corrected in the same commit.
- Neither phase is marked `**Security-sensitive:**`. Both touch a module that handles untrusted input; Phase 1 changes no production code at all, and Phase 2 only extracts an existing expression into a named function without changing what it returns. DW-2.4 converts the one real risk in that extraction (a `cfg(test)` divergence between tested and shipped parse paths) into a checked item, which is the proportionate answer.
- Engram carries the generalized rule from this line of work: *"Prose safety arguments citing third-party dependency code structures must be validated with tests to prevent silent decay during dependency updates."* This plan is that rule applied to its own origin site.
- Review cadence is 1 — the two phases touch one file and Phase 2 carries the only hard deliverable (a demonstrated failure), so it gets a clean reviewed baseline under it.

---
## Execution Log
_To be filled during /code-foundations:build_
