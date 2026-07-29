# Discovery + Design: Phase 1 - Make every surviving dependency claim true

## Files Found
- `crates/gfx/src/svg.rs` (829 lines) — the only file in scope.
- `crates/gfx/src/decode.rs:641` — `test_dw_6_3_no_input_the_dimension_cap_admits_can_exceed_the_allocation_cap`, the raster counterpart the rewritten test must keep cross-referencing. Confirmed present at that exact line.
- `docs/code-standards.md` — present, read.

## Current State
All six defect sites are present at the line numbers the plan gives. Nothing has drifted.

## Citation Verification (done against the vendored crates, not the prompt)

`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`

| Claim in `svg.rs` / plan | Verdict | Evidence |
|---|---|---|
| `roxmltree-0.21.1/src/parse.rs:531` "Allow infinite amount of references at zero depth" | Correct | `fn inc_references` at 531; comment at 533; `if self.depth == 0 { Ok(()) }` at 532-534 |
| `LoopDetector` caps depth at 10, references at 255 per root | Correct | `if self.depth < 10` at 510; `if self.references == u8::MAX` at 536; `references: u8` at 504, doc "Number of references resolved by the root reference" at 503 |
| `roxmltree` `nodes_limit` cannot see wide entity expansion | Correct | check is `self.doc.nodes.len() >= self.opt.nodes_limit` at 562, per *node*; `append_text` accumulates into `after_text: Vec<Cow<str>>` (553, 613) and joins once at 627 |
| `parse.rs:371` — `ParsingOptions::default()` leaves `entity_resolver: None` | Correct | `impl Default for ParsingOptions` at 371, `entity_resolver: None` at 376 |
| `usvg-0.47.0/src/parser/mod.rs:36` for the 1,000,000 cap | **DEAD, as the plan says** | 36 is the doc on `ElementsLimitReached`; the variant is declared at 37, `Display`ed at 65, and those are its only two occurrences in the whole crate |
| Live check at `svgtree/parse.rs:392` | Correct | `if doc.nodes.len() > 1_000_000 {` at 392, `return Err(Error::NodesLimitReached);` at 393 |
| Error is named `NodesLimitReached` | Correct **but the plan mis-frames whose it is** | `svgtree/parse.rs:6` is `use roxmltree::Error;`. `NodesLimitReached` is `roxmltree::Error::NodesLimitReached` (`roxmltree-0.21.1/src/parse.rs:100`), *not* a usvg variant — usvg's own `Error` (`parser/mod.rs:29`) has no such variant. Writing "usvg's `Error::NodesLimitReached`" would itself be a false dependency claim. |
| `<use>` depth 1024 at `svgtree/parse.rs:182` | Live, but the characterization is loose | `if depth > 1024` at 182 is a **general XML nesting-depth** cap: `depth + 1` on ordinary children (213) *and* on `<use>` (205 → `parse_svg_use_element` → 618, so two levels per hop). Not a `<use>`-specific cap. |
| `mod.rs:147` for `from_data`'s fixed `nodes_limit` | Live but points into the wrong function | `from_data` is at 98 and calls `Self::from_str` (102, 105); `from_str` is declared at 146 and its `ParsingOptions` literal is 147-150; the `u32::MAX` itself is `roxmltree-0.21.1/src/parse.rs:375` |
| `svgtree/parse.rs:139` for namespace-based resolution (`svg.rs:219`) | Correct | `if !matches!(node.tag_name().namespace(), None \| Some(SVG_NS))` at 139 |
| `decode.rs:641` cross-reference | Correct | test declared at exactly 641 |

## Gaps
Three findings beyond what the plan enumerates. Two are out of this phase's scope and are reported, not absorbed.

1. **Plan inaccuracy (affects wording I must write).** `NodesLimitReached` belongs to `roxmltree`, not `usvg`. Handled by naming it `roxmltree::Error::NodesLimitReached` with the `svgtree/parse.rs:6` import cited.
2. **Out of scope — imprecise header claim.** `svg.rs:25` calls the 1024 cap "`<use>` depth 1024". It is a general nesting cap. The header narrative is explicitly OUT except the two named citations, and the citation itself is live, so it stays. At the `MAX_XML_NODES` site (IN scope) I state it precisely in a way that still agrees with the header.
3. **Out of scope — orphaned doc comment.** `svg.rs:319-325` (`/// Parses `source` into a `usvg` tree...`) is attached to `enum Fonts` at 327-328, not to `fn parse` at 342, and `Fonts`'s own one-line doc is stranded as its last line (326). `fn parse` has no doc at all. A real defect, not a dependency claim, and not in the plan's IN list.

4. **Plan file-scope gap — acted on, flagged.** `crates/gfx/src/decode.rs:51` names `svg::test_the_input_caps_cannot_outgrow_the_output_cap` in the `Limits` doc, and the surrounding sentence claims that test "keeps the two sides in a stated relationship" between svg's input caps and this pixel budget. DW-1.6's rename breaks the pointer, *and* deleting the 2 GiB assertion makes the claim substantively false — nothing relates svg's input caps to `Limits` any more. The plan's File scope is `svg.rs` only, so this is an out-of-scope edit made deliberately rather than leaving the commit containing a stale pointer of exactly the kind it exists to remove. Two lines, revertable with `git checkout crates/gfx/src/decode.rs`.

No prerequisites missing. No behaviour change required by any DW item.

## Code Standards
- Unit tests in `#[cfg(test)] mod tests` at the bottom of the file — already the shape here.
- Test names are sentences describing the behaviour; `test_dw_*` only for tests tracing a plan requirement. The renamed test is a behavioural pin, not a DW tracer, so `test_the_node_cap_is_reachable_within_the_byte_cap` is the right form.
- "A constant that encodes a measurement": the value carries the measurement that produced it. This is exactly what the `MAX_SVG_BYTES` and `MAX_XML_NODES` doc rewrites must preserve.
- "A test must be able to fail" — the reason the dirty-test demonstration is required.

## Test Infrastructure
`cargo test -p gfx`, built-in harness, no fixtures needed. The rewritten test is pure arithmetic over two `const`s, so its dirty run is a two-line mutation of `MAX_XML_NODES`.

## DW Verification

| DW-ID | Done-When Item | Status | Test Cases |
|-------|---------------|--------|------------|
| DW-1.1 | No comment attributes an operative entity-expansion bound to `roxmltree` (sites 1, 2, 5); each names `refuse_internal_subset` and cross-references the module doc; `svg.rs:10-17` and `706-716` unchanged | COVERED | Prose; verified by reading + `git diff` showing those two ranges untouched. Behaviour unchanged proven by `test_an_entity_bomb_is_refused_rather_than_expanded`, `test_a_wide_entity_bomb_is_refused`, `test_a_plain_doctype_is_still_accepted` |
| DW-1.2 | `MAX_SVG_BYTES` doc states no-internal-subset ⇒ no expansion ⇒ peak text ≈ source bytes, with no roxmltree expansion appeal | COVERED | Prose; `test_a_wide_entity_bomb_is_refused` + `test_a_plain_doctype_is_still_accepted` are the executable half of the argument (subset refused, plain DOCTYPE still parsed) |
| DW-1.3 | No comment claims `usvg` sets no limit; both sites agree with the header | COVERED | Prose at `MAX_XML_NODES` doc and the node-limit test doc; `test_a_document_past_the_node_limit_is_refused` still passes |
| DW-1.4 | usvg citation is live code at all three sites; `grep -c ElementsLimitReached` = 0 | COVERED | `grep -c ElementsLimitReached crates/gfx/src/svg.rs`; `grep -n 'mod.rs:36' crates/gfx/src/svg.rs` |
| DW-1.5 | `grep -c ROXMLTREE_MAX_EXPANSION` = 0; no other `const` restates a dependency value | COVERED | `grep -c ROXMLTREE_MAX_EXPANSION crates/gfx/src/svg.rs`; `grep -n 'const ' crates/gfx/src/svg.rs` audited (`MAX_SVG_BYTES`, `SNIFF_BYTES`, `RENDER_TIME_CAP`, `MAX_XML_NODES`, `CIRCLE` — all stele's own) |
| DW-1.6 | Test renamed to what it asserts; doc records the 5.24x headroom; `decode.rs:641` cross-reference survives | COVERED | `test_the_node_cap_is_reachable_within_the_byte_cap` passes; dirty run at `MAX_XML_NODES = 1_048_577` fails, at `1_048_576` and `400_000` passes |
| DW-1.7 | `cargo test -p gfx` passes, clippy silent | COVERED | `cargo test -p gfx`, `cargo test --workspace --all-features`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo fmt --all --check` |

**All items COVERED:** YES

## Design Decisions

No interface design here — this is prose plus one test deletion. The decisions that matter:

- **`MAX_SVG_BYTES`'s replacement argument is stated as a fact about stele's own code.** The bound is `refuse_internal_subset`: custom entities are the only construct that makes an XML document grow, they are declarable only in an internal subset, that is refused, and every remaining substitution (`&amp;`, `&#x2014;`) shrinks its source. So peak text is roughly source bytes and this constant caps both. Nothing about roxmltree's expansion behaviour is load-bearing; the doc points at the module header for what roxmltree actually does.
- **The 255 figure does not appear in any rewritten site.** It survives only where the plan protects it (`svg.rs:10-17`, `706-716`) as the history explaining why the old fixture passed.
- **The error is named `roxmltree::Error::NodesLimitReached`, once, in the header**, with `svgtree/parse.rs:6` cited so a reader can see why a usvg check raises a roxmltree error. The other two sites carry `svgtree/parse.rs:392` alone — naming it three times would be the restatement this phase exists to remove.
- **The nested entity-bomb test's doc says what its assertion actually proves.** Its assertion is `Malformed(_)`, which the subset refusal produces, and the plan puts every other test's assertions out of scope. So the doc stops claiming the bound is asserted and instead points at `test_a_wide_entity_bomb_is_refused` as the test that checks the guard by its message.
- **The cap-relationship test keeps only its genuine half** and gains the headroom number, so a future reader knows the pin is 5.24x loose rather than tight.

## Prerequisites
- [x] `crates/gfx/src/svg.rs` present at the stated line numbers
- [x] Vendored `roxmltree-0.21.1`, `usvg-0.47.0`, `fontdb-0.23.0` available for verification
- [x] `decode.rs:641` cross-reference target exists

## Recommendation
BUILD. All seven DW items are meetable with prose edits, one constant deletion, one test rename and one assertion deletion. No production code path changes.
