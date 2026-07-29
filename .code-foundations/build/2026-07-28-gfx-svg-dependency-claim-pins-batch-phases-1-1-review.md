# Review: Phase 1 — Make every surviving dependency claim true

Commit under review: `e3d2a80 docs(gfx): six claims about other people's code, now true`

## Executed Results (Step 0)

| Command | Result |
|---|---|
| `cargo test -p gfx` | 56 passed, 0 failed (lib) + 1 passed / 2 ignored (`live_ghostty`) + 1 ignored (`svg_cost`) |
| `cargo test --workspace --all-features` | exit 0; every suite `ok`, 0 failed across 41 result lines (incl. 361-, 99-, 56-, 54-test binaries) |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | exit 0, silent |
| `cargo fmt --all --check` | exit 0, silent |

Named regression tests, all observed passing in the run above:
`test_a_wide_entity_bomb_is_refused`, `test_a_plain_doctype_is_still_accepted`,
`test_an_entity_bomb_is_refused_rather_than_expanded`, `test_the_node_cap_is_reachable_within_the_byte_cap`.

## Behaviour-unchanged claim

`git show HEAD -- crates/gfx/src/svg.rs crates/gfx/src/decode.rs` verified line by line. Every hunk is a doc/comment change except two:

1. `fn test_the_input_caps_cannot_outgrow_the_output_cap` → `fn test_the_node_cap_is_reachable_within_the_byte_cap` (rename), with the `ROXMLTREE_MAX_EXPANSION`/2 GiB assertion deleted and the node-reachability assertion kept verbatim.
2. `decode.rs:51` cross-reference retargeted to the new test name.

No `const` value, no guard body, no `ParsingOptions` field, no call in `parse`/`probe`/`rasterize`/`read_source` changed. **Claim holds.**

## Requirement Fulfillment

### DW-1.1
PREMISE: "No comment in `crates/gfx/src/svg.rs` attributes an entity-expansion bound to `roxmltree` that is **operative today**. Each such comment states `refuse_internal_subset` as the operative bound, cross-referencing the module doc for the measurement rather than restating it."
EVIDENCE: svg.rs:10 (`Not bounded by roxmltree`), :22, :76–83, :96–97, :365, :647–654, :711–714; historical sites preserved at :12–15 and :733–737.
TRACE: `grep -nE "roxmltree" crates/gfx/src/svg.rs` → 12 comment sites. Each entity-expansion site reads as a *denial* of a roxmltree bound (`is not one`, `it is not a bound`, `not anything roxmltree does`) or as explicit history (`the earlier defence did not stop`, `Measured before the fix`). No site states a live roxmltree entity bound. No site restates the 76 KiB/256 MiB/4.7 s measurement outside the module doc; :82 and :654 point back to it instead.
VERDICT: **PASS** — on its literal terms. See DW-1.2 and Finding 1: the `refuse_internal_subset` attribution these comments now carry is itself overstated (svg.rs:100 "refusing that construct refuses the entire class" is demonstrably false), but that is a defect in the *replacement* claim, scored under DW-1.2.

### DW-1.2
PREMISE: "`MAX_SVG_BYTES`'s doc states its replacement argument — no internal subset ⇒ no expansion ⇒ peak text ≈ source bytes — with no appeal to any roxmltree expansion behaviour."
EVIDENCE: crates/gfx/src/svg.rs:75–83.
TRACE: The doc states, verbatim: *"the only way one reaches a parse here is a DOCTYPE's internal subset — that is what is refused … So peak text stays at roughly the source's own size and this one number caps both."* Executed counter-example (test written and run, see Issue 1): a 12,651-byte file whose DOCTYPE is `<!DOCTYPE svg SYSTEM "x>y" [ <!ENTITY a "A"×500 > ]>` followed by 4,000 `&a;` references returns `Ok((10, 10))` from `gfx::decode::probe_dimensions` in 50 ms, having expanded to **2,000,000 bytes** of text — 158x the source, with the guard silent. The stated implication chain is false at its first link.
VERDICT: **FAIL**

The rest of DW-1.2 is met: the doc makes no appeal to roxmltree expansion behaviour (its only roxmltree sentence, :82–83, explicitly disclaims one), and `MAX_SVG_BYTES` is not left argument-free.

### DW-1.3
PREMISE: "No comment claims `usvg` sets no limit. Every site agrees: usvg caps at 1,000,000 nodes and 1024 nesting depth, and `MAX_XML_NODES` is a tightening applied a stage earlier."
EVIDENCE: svg.rs:23–29, :147–152, :626–631; vendored `usvg-0.47.0/src/parser/svgtree/parse.rs:392` and `:182`.
TRACE: `grep -nEi "sets no|no limit|sets none|none of its own|entirely this module" crates/gfx/src/svg.rs` → **0 hits**; both former sites (`usvg` sets no limit of its own` at the old MAX_XML_NODES doc, `usvg` sets none` at the old test doc) are gone. Against the vendored source: `parse.rs:392` is exactly `if doc.nodes.len() > 1_000_000 {` → `Err(Error::NodesLimitReached)`; `parse.rs:182` is exactly `if depth > 1024 {` → same error. `1_000_000 / 200_000 = 5`, so "five times lower" is exact, and roxmltree's `nodes_limit` is consumed at `Document::parse_with_options` while usvg's fires in `svgtree::Document::parse_tree`, one stage later — "applied a stage earlier" is correct.
VERDICT: **PASS** (see Finding 5 for a wording imprecision at two untouched sites)

### DW-1.4
PREMISE: "The usvg citation points at live code at every site. `grep -c ElementsLimitReached crates/gfx/src/svg.rs` returns 0, and no site copies the dead pointer forward."
EVIDENCE: svg.rs:25, :148, :627; `usvg-0.47.0/src/parser/mod.rs:37`, `:65`.
TRACE: `grep -c ElementsLimitReached crates/gfx/src/svg.rs` → **0**. `grep -rn ElementsLimitReached usvg-0.47.0/src resvg-0.47.0/src` → exactly two hits, `mod.rs:37` (enum variant declaration) and `mod.rs:65` (`Display` arm). No construction site anywhere — the variant is genuinely dead, so the old `usvg-0.47.0/src/parser/mod.rs:36` pointer was a dead citation. The replacement `svgtree/parse.rs:392` is a live `return Err(...)` on the reachable element-append path, and `svgtree/parse.rs:6` is exactly `use roxmltree::Error;`, confirming the "imports there rather than using an error of its own" claim. `svgtree/parse.rs:182` is likewise live and reachable: `parse_svg_use_element(... depth + 1 ...)` at `:205` and children at `:213`/`:618` all feed it.
VERDICT: **PASS**

### DW-1.5
PREMISE: "`grep -c ROXMLTREE_MAX_EXPANSION crates/gfx/src/svg.rs` returns 0, and no other `const` in the module restates a dependency-owned value."
EVIDENCE: svg.rs:84, :92, :143, :158, :527.
TRACE: `grep -c ROXMLTREE_MAX_EXPANSION crates/gfx/src/svg.rs` → **0**. `grep -n "const " crates/gfx/src/svg.rs` → five declarations. `MAX_SVG_BYTES` (4 MiB), `SNIFF_BYTES` (1024), `RENDER_TIME_CAP` (250 ms), `MAX_XML_NODES` (200,000) and the test fixture `CIRCLE` are all this module's own policy numbers; none equals or mirrors a roxmltree/usvg/resvg/image constant. `SNIFF_BYTES = 1024` coincides numerically with usvg's depth cap but is unrelated and is documented as a read-size/search-window pairing.
VERDICT: **PASS**

### DW-1.6
PREMISE: "The cap-relationship test is renamed to what it now asserts, and its doc records the actual headroom (`MAX_SVG_BYTES` 4,194,304 vs `MAX_XML_NODES` × 4 = 800,000, a 5.24x margin). The cross-reference to `decode.rs`'s `test_dw_6_3_...` survives."
EVIDENCE: svg.rs:702–728; decode.rs:642.
TRACE: Test is `fn test_the_node_cap_is_reachable_within_the_byte_cap` (:721) and its sole assertion is `MAX_SVG_BYTES as u64 >= u64::from(MAX_XML_NODES) * 4` — the name now describes exactly that. Doc at :717–719 states 800,000 vs 4,194,304 and "5.24x of headroom"; `4_194_304 / 800_000 = 5.2429` ✓. Doc at :719 states "Raising the node cap past 1,048,576 is what trips this"; the assertion fails when `MAX_XML_NODES * 4 > 4_194_304`, i.e. `MAX_XML_NODES > 1_048_576` ✓ exact. Cross-reference at :707 resolves — `test_dw_6_3_no_input_the_dimension_cap_admits_can_exceed_the_allocation_cap` exists at decode.rs:642. Test observed passing in Step 0.
VERDICT: **PASS**

### DW-1.7
PREMISE: "`cargo test -p gfx` passes and `cargo clippy --workspace --all-targets --all-features -- -D warnings` is silent."
EVIDENCE: Executed Results table above.
TRACE: `cargo test -p gfx` → 56/0/0 plus the two integration binaries, exit 0. Clippy exit 0 with no diagnostic lines. `cargo fmt --all --check` and the full workspace suite also clean.
VERDICT: **PASS**

**All requirements met:** NO — DW-1.2 fails.

## Edge cases

| Edge case | Status | Evidence |
|---|---|---|
| 255-per-root survives only as history, explicitly non-load-bearing | PASS | svg.rs:12–15 sits under "Not bounded by `roxmltree`" and is immediately followed by "explicitly allows an unlimited number of root references"; svg.rs:733–737 is framed "the earlier defence did not stop … Measured before the fix". Neither presents it as a live bound; neither erases it. |
| Cap-relationship test keeps its genuine assertion | PASS | svg.rs:722–727 retains `MAX_SVG_BYTES >= MAX_XML_NODES * "<g/>".len()` unchanged from the pre-rename version. |
| `MAX_SVG_BYTES` not left with no argument | PASS (but see DW-1.2) | svg.rs:75–83 supplies a full replacement argument. It is present; it is also false. |
| `decode.rs` cross-reference survives | PASS | svg.rs:707 → decode.rs:642; and decode.rs:51 → svg.rs:721. Both directions resolve. |

## Dependency-claim audit (the central task)

Every comment in `svg.rs` attributing a bound, limit, default, guarantee or behaviour to a dependency, checked against the vendored sources under `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`.

| # | svg.rs | Claim | Cited | Vendored source says | Verdict |
|---|---|---|---|---|---|
| 1 | :10 | Entity expansion not bounded by roxmltree | — | Confirmed via 2–4 below | TRUE |
| 2 | :12–13 | `LoopDetector` caps depth at 10, references at 255 per root reference | — | `roxmltree/parse.rs:510` `if self.depth < 10`; `:536` `if self.references == u8::MAX`; `:525–527` resets `references` only at depth 0 | TRUE |
| 3 | :13–15 | Unlimited root references, "Allow infinite amount of references at zero depth" | `roxmltree-0.21.1/src/parse.rs:531` | `:531` is `fn inc_references`; the quoted comment is at `:533`, its guard `if self.depth == 0 { Ok(()) }` at `:532–534` | TRUE, citation off by 2 (Finding 2) |
| 4 | :15–18 | Consecutive refs → one text node + unbounded `after_text` `join`ed to one `String` | `parse.rs:599` | `:599` `fn append_text`: `if self.after_text.is_empty() { append_node(Text) }` then unconditional `after_text.push`. `nodes_limit` is checked in `append_node` (`:562`), so it never fires. The `join` is at `:627` in `merge_text` | TRUE, `join` cited to the wrong function (Finding 4) |
| 5 | :18–20 | 76 KiB → 256 MiB in 4.7 s, parses successfully | — | Mechanism verified from source (4 above); the numbers are a prior-phase measurement, not re-run here | UNVERIFIED (Finding 7) |
| 6 | :23–25 | usvg caps at 1,000,000 nodes | `usvg/src/parser/svgtree/parse.rs:392` | `:392` `if doc.nodes.len() > 1_000_000 { return Err(Error::NodesLimitReached) }` | TRUE, exact |
| 7 | :25–26 | `<use>` depth 1024 | `svgtree/parse.rs:182` | `:182` `if depth > 1024 { return Err(Error::NodesLimitReached) }` — a general element-nesting check that `<use>` also spends (`:205`, `:213`, `:618`) | TRUE, slightly narrow (Finding 8) |
| 8 | :26–28 | Both raise `roxmltree::Error::NodesLimitReached`, imported by usvg | `svgtree/parse.rs:6` | `:6` `use roxmltree::Error;` | TRUE, exact |
| 9 | :31–33 | `from_data` hands off to `from_str` | `usvg/src/parser/mod.rs:98`, `:146` | `:98` `pub fn from_data`, calling `Self::from_str` at `:102`/`:105`; `:146` `pub fn from_str` | TRUE, exact |
| 10 | :33–35 | `from_str`'s `ParsingOptions` leaves `nodes_limit` at `u32::MAX` default, unreachable | `mod.rs:147`, `roxmltree/parse.rs:375` | `mod.rs:147–150` sets only `allow_dtd: true, ..Default::default()`; `roxmltree/parse.rs:375` is `nodes_limit: u32::MAX,` | TRUE, exact |
| 11 | :41–44 | `<use>` expansion stops only at usvg's 1,000,000 elements | — | The cap counts `doc.nodes.len()`, and `svgtree::NodeKind` (`svgtree/mod.rs:184–191`) has `Root`, `Element`, `Text` — "nodes", not "elements" | IMPRECISE (Finding 5) |
| 12 | :52–54 | `.svgz` decompression is `from_data`'s job; that entry point parses with no node limit | — | `mod.rs:99–101` `decompress_svgz` then `from_str`; `from_str` per 10 above | TRUE |
| 13 | :60–63 | usvg/tiny_skia reached through resvg, not depended on directly | — | `crates/gfx/Cargo.toml` lists only `base64`, `image`, `resvg` | TRUE |
| 14 | :76–83 | No internal subset reaches a parse ⇒ peak text ≈ source bytes | — | **Falsified by execution** — see Issue 1 | **FALSE** |
| 15 | :105–107 | External DTD never fetched; `ParsingOptions::default()` leaves `entity_resolver` at `None` | `roxmltree/parse.rs:371` | `:371` `impl Default for ParsingOptions`, `entity_resolver: None` at `:376`; `resolve_entity` (`:757–762`) returns `Ok(None)` when the resolver is `None` | TRUE, citation off by 5 (Finding 3) |
| 16 | :96–100 | `refuse_internal_subset` is "the whole defence"; refusing the internal-subset construct "refuses the entire class" | — | **Falsified by execution** — see Issue 1 | **FALSE** |
| 17 | :235–237 | usvg resolves elements by namespace + local name, never literal spelling | `svgtree/parse.rs:139` | `:139` `if !matches!(node.tag_name().namespace(), None \| Some(SVG_NS))`, then `:143` `EId::from_str(node.tag_name().name())` | TRUE, exact |
| 18 | :283–285 | Text is converted to paths during parsing, so an empty fontdb drops labels | — | `converter.rs:631/634` and `:704/707` call `super::text::convert(...)` inside `convert_doc`, which `from_xmltree` invokes; `text::convert` is at `parser/text.rs:100` | TRUE |
| 19 | :292–294 | 516 ms first text SVG vs 8 ms after | — | Machine measurement, not re-run | UNVERIFIED (Finding 7) |
| 20 | :341–343 | `from_str` builds its own `ParsingOptions` with `nodes_limit` unset, unreachable | — | Same as 10 | TRUE (but misplaced — Finding 1) |
| 21 | :352–356 | `usvg::Tree::size()` from root `width`/`height`/`viewBox`, never text extents | — | `converter.rs:348–351` computes `size` from `resolve_svg_size(&svg, opt)` before any child conversion; `resolve_svg_size` (`:489–504`) reads only `AId::Width`, `AId::Height`, `parse_viewbox()` on the root; `size` is moved into `Tree` at `:368` and never revised | TRUE |
| 22 | :389 | usvg and resvg offer no cancellation hook | — | `grep -rni "cancel\|abort"` over `usvg-0.47.0/src` and `resvg-0.47.0/src` → 0 hits | TRUE |
| 23 | :626–628 | usvg caps the same path at 1,000,000 nodes | `svgtree/parse.rs:392` | Same as 6 | TRUE, exact |
| 24 | :733–737 | The classic nested bomb *is* caught by roxmltree's 255-per-root cap | — | The fixture's `&g;` subtree spends 10+100+1000+… references against one root, and `references` is reset only at depth 0 (`parse.rs:525–527`), so it passes 255 | TRUE |
| 25 | :781–783 | Nothing stops `<use>` before usvg's 1,000,000 elements | — | Same imprecision as 11 | IMPRECISE (Finding 5) |

Every `file:line` cited in the file exists in the vendored sources and points at real, reachable code. No dead citation remains.

## Test-DW Coverage

- [x] DW-1.6, DW-1.7 — automated tests run in Step 0 (`test_the_node_cap_is_reachable_within_the_byte_cap` plus the full gfx suite).
- [x] DW-1.1, DW-1.3, DW-1.4, DW-1.5 — prose items with no automated test possible; covered by recorded observed behaviour (the greps and vendored-source reads transcribed above), which is the coverage level the dispatch specifies for prose items.
- [x] DW-1.2 — covered by observed behaviour: an executed counter-example test (Issue 1), which is what produced the FAIL.
- [x] Behaviour-unchanged claim — all four named regression tests observed passing.

No gaps.

## Dead Code

None found. No unused imports (clippy `-D warnings` is silent), no unreachable code, no debug statements, no commented-out code blocks. The `ROXMLTREE_MAX_EXPANSION` const and its assertion were deleted outright rather than commented out.

## Correctness Dimensions

| Dimension | Status | Evidence |
|-----------|--------|----------|
| Concurrency | N/A | Change is comment-only; `within_time_cap`'s thread/channel structure is untouched by this diff. |
| Error Handling | N/A | No error path added, removed or altered. |
| Resources | N/A | No handle, allocation or lifetime changed. |
| Boundaries | PASS | Probed the one arithmetic site touched: `u64::from(MAX_XML_NODES) * 4` = 800,000 and `MAX_SVG_BYTES as u64` = 4,194,304 — both far inside `u64`, no overflow at any legal cap value; the documented trip point (1,048,576) is exact. |
| Security | **FAIL** | `refuse_internal_subset` is bypassable by a `>` inside a legal `SystemLiteral`, admitting the entity-expansion class the module exists to refuse. Demonstrated by executed test — see Issue 1. |

## Loaded-Skill Criteria

| Skill | Criterion | Status | Evidence |
|-------|-----------|--------|----------|
| code-clarity-and-docs | Stale comment: comment says X, code does Y (CRITICAL) | **FAIL** | svg.rs:76–83 and :96–100 assert a guarantee the code does not provide; demonstrated in Issue 1. |
| code-clarity-and-docs | Interface comment attached to the entity it describes | FAIL (pre-existing) | svg.rs:337–343 documents `fn parse` but sits immediately above `enum Fonts`, so it *is* `Fonts`'s doc comment. See Finding 1. |
| code-clarity-and-docs | "Different words" test — comments must not restate code | PASS | Every comment in the diff supplies rationale (why the cap, why the seam, why the citation moved), none restates a signature. |
| code-clarity-and-docs | Reference external sources precisely | PASS with notes | 12 of 12 `file:line` citations resolve to real code; three are anchored to the enclosing item rather than the exact line (Findings 2–4). |
| code-clarity-and-docs | Variable comment completeness (units, bounds, invariants) | PASS | `MAX_SVG_BYTES` (bytes), `SNIFF_BYTES` (bytes), `RENDER_TIME_CAP` (`Duration`), `MAX_XML_NODES` (count) each state unit and rationale; `MAX_XML_NODES` now also states its relationship to the downstream cap. |
| code-clarity-and-docs | Naming precision | PASS | `test_the_node_cap_is_reachable_within_the_byte_cap` names exactly the surviving assertion; the old name (`..._input_caps_cannot_outgrow_the_output_cap`) described a deleted one. |
| code-clarity-and-docs | Commented-out code / TODO-forever | PASS | None present. |
| code-clarity-and-docs | AI config docs reflect current architecture | N/A | No `CLAUDE.md`/`AGENTS.md` content touches this module's caps or citations. |

## Issues (FAIL)

### Issue 1 — `refuse_internal_subset` is bypassable, so `MAX_SVG_BYTES`'s new argument and the guard's own doc are both false

- **File:** `crates/gfx/src/svg.rs:75–83` (the DW-1.2 claim), `crates/gfx/src/svg.rs:96–100` and `:113–130` (the guard and its doc)
- **Confidence:** High (executed). **Severity:** High — it is both a false dependency-class claim and a live input-hardening hole.

**Mechanism.** The scan takes the DOCTYPE declaration to end at the first `>`:

```rust
let rest = &prologue[doctype..];
let end = rest.find('>').unwrap_or(rest.len());
if rest[..end].contains('[') { /* refuse */ }
```

XML's `SystemLiteral` production admits any character except the quote delimiter, so `>` is legal inside a `SYSTEM "..."` identifier. `rest.find('>')` lands on that inner `>`, `end` cuts the slice before the internal subset opens, `contains('[')` is false, and the document is handed to `roxmltree` with `allow_dtd: true` and its entity declarations intact.

**Demonstrated by** (test written and run against `gfx::decode::probe_dimensions`, the real public path; file since removed):

```
source:   <?xml version="1.0"?>
          <!DOCTYPE svg SYSTEM "x>y" [ <!ENTITY a "AAAA…500×A"> ]>
          <svg … width="10" height="10"><desc>&a; ×4000</desc></svg>
result:   Ok((10, 10))          <-- parsed successfully, guard silent
source:   12,651 bytes
expanded: 2,000,000 bytes of text  (158x)
elapsed:  50 ms
control:  the identical document without the `>` in the SYSTEM literal
          -> Err(Malformed("svg: declares its own XML entities, …"))
```

Tiered entities amplify further while staying inside roxmltree's 255-per-root cap (`&c;` = 3 source bytes → 1,000 expanded bytes at 111 references):

```
TIERED: prologue=169B source=600,251B expanded=200,000,000B (333x)
        result=Err(Malformed("svg: gave up after 250 ms — too complex to draw"))
        elapsed=255ms
```

At 333x the time cap is what stops it, not any size bound — and `within_time_cap` abandons the worker thread, which keeps allocating. Extrapolated to `MAX_SVG_BYTES`, peak text is on the order of a gigabyte, not "roughly the source's own size".

**Why this fails DW-1.2 rather than landing in Notes.** DW-1.2 does not merely ask that `MAX_SVG_BYTES` carry *some* argument; it specifies the argument — "no internal subset ⇒ no expansion ⇒ peak text ≈ source bytes". That implication is false at its first link, and the sentence asserting it is new in this commit. The dispatch is explicit that a claim which is wrong is the defect class this work exists to eliminate.

**Note on attribution.** The guard itself predates this commit (`b1b2cee`) and this change set did not alter it — the behaviour-unchanged claim holds. What this commit added is a doc that *depends* on the guard being complete. Two fixes are available and they are not equivalent:

- **Fix the guard** (preferred): scan the DOCTYPE declaration quote-aware — skip `'…'` and `"…"` spans when hunting for the terminating `>` — and add a regression test with a `SYSTEM "x>y"` fixture. This makes the DW-1.2 sentence true as written and closes the hole. The prologue must stay under `SNIFF_BYTES` for a file to reach the guard at all, which bounds but does not remove the exposure.
- **Weaken the doc**: state only what the guard actually delivers. This satisfies nothing DW-1.2 asked for and leaves the bypass live.

## Notes (non-blocking)

| # | Finding | File | Sev | Conf |
|---|---|---|---|---|
| 1 | The doc comment for `fn parse` (`/// Parses source into a usvg tree under this module's limits…`, 7 lines) sits directly above `enum Fonts` with no blank separation, so it *is* `Fonts`'s doc comment — `enum Fonts` is documented as "Parses `source` into a `usvg` tree", and `fn parse` at :360 has no doc at all. The `Fonts` one-liner at :344 is swallowed as that block's last line. Pre-existing (identical in `HEAD~1`); clippy cannot see it. The claim itself is true (verified, row 20). | svg.rs:337–344 | Med | High |
| 2 | `roxmltree-0.21.1/src/parse.rs:531` is quoted for "Allow infinite amount of references at zero depth"; that string is at `:533`. `:531` is the enclosing `fn inc_references`. | svg.rs:14 | Low | High |
| 3 | `roxmltree-0.21.1/src/parse.rs:371` is cited for `entity_resolver` being `None`; `:371` is `impl Default for ParsingOptions`, the field is at `:376`. | svg.rs:107 | Low | High |
| 4 | `parse.rs:599` is cited for the whole sentence including "finally `join`ed into a single `String`". `:599` is `fn append_text`, which does the one-node/unbounded-push half; the `join` is at `:627` in `merge_text`. A second pointer would make the sentence checkable end to end. | svg.rs:18 | Low | High |
| 5 | Two untouched sites say "usvg's 1,000,000 **elements**" where the four updated sites say "nodes". The check is `doc.nodes.len() > 1_000_000` and `svgtree::NodeKind` (`svgtree/mod.rs:184–191`) includes `Root` and `Text(String)`, so "nodes" is the accurate word. Numerically all sites agree, which is why DW-1.3 passes. | svg.rs:42, :781 | Low | High |
| 6 | "An SVG has **three** separate ways to be hostile:" introduces a four-item list (entity expansion, node count, canvas size, wall time). Item 4 was appended without updating the count. Pre-existing. | svg.rs:8 | Low | High |
| 7 | The module's measured figures — 76 KiB → 256 MiB in 4.7 s (:18–20), 808 bytes → 60 ms (:43), 516 ms vs 8 ms font load (:292–294), ~480 ms (:355, :819) — are not derivable from the vendored sources and were not re-run here. The 4.7 s/256 MiB *mechanism* is confirmed from source (audit row 4); the magnitudes are trusted. `tests/svg_cost.rs` exists as the re-measurement path and the fonts doc points at it; the entity figure has no such harness. | svg.rs (several) | Low | Med |
| 8 | "`<use>` depth 1024" reads as a `<use>`-specific cap. `svgtree/parse.rs:182` bounds general element nesting, which `<use>` resolution happens to spend. The `MAX_XML_NODES` doc at :148–149 phrases this correctly ("1024 levels of nesting, which `<use>` expansion also spends"); the module header is the looser of the two. | svg.rs:25 | Low | High |

**Verdict: FAIL** — one blocker: DW-1.2's mandated argument for `MAX_SVG_BYTES` is falsified by an executed counter-example (`refuse_internal_subset` bypass via a `>` inside a `SYSTEM` literal), which also fails the Security dimension and the loaded skill's stale-comment criterion. DW-1.1, 1.3, 1.4, 1.5, 1.6, 1.7 all pass; all four edge cases are handled; the behaviour-unchanged claim holds; every remaining dependency citation resolves to live, correct code.
