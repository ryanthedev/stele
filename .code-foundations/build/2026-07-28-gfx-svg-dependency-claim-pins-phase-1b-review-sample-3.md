# Review: Phase 1b — SVG internal-subset scan (sample 3)

## Executed Results (Step 0)

- `cargo test --workspace --all-features --no-fail-fast` → **818 passed, 1 failed**.
  The single failure is `stele::document_source::test_dw_2_2_an_external_write_repaints_within_a_poll_interval_and_keeps_the_top_block`:

  ```
  assertion `left != right` failed: the status ruler must reflect the longer document
    left:  "/private/tmp/claude-501/-Users-r-repos-stele/94dcf237-6f05-4342-8726-8e025e182ad"
    right: "/private/tmp/claude-501/-Users-r-repos-stele/94dcf237-6f05-4342-8726-8e025e182ad"
  ```

  Both sides are the *same truncated prefix* of the mandated `TMPDIR`. Re-run with a short `TMPDIR` (`/private/tmp/c3`): `10 passed; 0 failed`. **Environmental, caused by the mandated long temp path**; unrelated to `crates/gfx`. Not attributed to this phase.
- 818 includes 6 tests from three concurrent reviewers' untracked `crates/gfx/tests/zz_review_*.rs` probes. Net workspace baseline **812 ≥ 806**.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` → **silent** (`Finished dev profile`).
- `cargo fmt --all --check` → the only `Diff in` path is another reviewer's untracked `crates/gfx/tests/zz_review_sample2_probe.rs`. **No tracked file, and no file under review, is unformatted.**
- `cargo test -p gfx --lib svg::` → `23 passed; 0 failed`.

**Pre-fix revert experiment (Step 0, mandated).** I restored the pre-fix `refuse_internal_subset` body verbatim in `crates/gfx/src/svg.rs`, ran `cargo test -p gfx --lib svg::`, then restored the post-fix file from a byte copy. Result: `17 passed; 6 failed`.

| New test | Pre-fix | Pins its named defect? |
|---|---|---|
| `test_dw_1b_1_a_subset_behind_a_quoted_gt_is_refused_like_the_unquoted_twin` | FAIL — `an internal subset is refused: (10, 10)` | Yes |
| `test_dw_1b_2_a_single_quoted_literal_hides_no_subset_either` | FAIL — `(10, 10)` | Yes |
| `test_dw_1b_2_both_public_literals_are_walked_for_a_hidden_subset` | FAIL — `(10, 10)` | Yes |
| `test_dw_1b_3_a_bracket_inside_a_literal_is_not_an_internal_subset` | FAIL — `parses: Malformed("… declares its own XML entities …")` | Yes |
| `test_dw_1b_4_an_unterminated_literal_is_refused_rather_than_walked_past` | FAIL — `Malformed("svg: expected '[' or '>' not 'h' at 3:13")` | Yes |
| `test_dw_1b_6_the_reproduced_bypass_is_refused_through_probe_dimensions` | FAIL — `(10, 10)` | Yes |
| `test_dw_1b_4_no_prefix_of_a_hostile_doctype_walks_out_of_bounds` | **PASS** | **No** — see Notes |
| `test_a_plain_doctype_is_still_accepted` (unchanged) | PASS | n/a — collateral check |

## Requirement Fulfillment

### DW-1b.1
PREMISE:  "`<!DOCTYPE svg SYSTEM "x>y" [ <!ENTITY a "…"> ]>` is refused, with the same error as the unquoted form. Pinned by a test that fails against the pre-fix scan."
EVIDENCE: `crates/gfx/src/svg.rs:154-172` (walk), `:847-860` (test)
TRACE:    `SYSTEM "x>y" [ <!ENTITY …`ic → `"` opens a quoted run → `>` inside it is consumed by the `(Some(_), _)` arm → closing `"` clears the quote → `[` hits `(None, b'[')` → `Malformed("svg: declares its own XML entities…")`. The test additionally asserts `format!("{hidden:?}") == format!("{plain:?}")`, so the quoted and unquoted twins produce a byte-identical error. Fails pre-fix with `an internal subset is refused: (10, 10)`.
VERDICT:  **PASS**

### DW-1b.2
PREMISE:  "The single-quoted form `SYSTEM 'x>y'` and the two-literal `PUBLIC "…>…" "…>…"` form are refused too, each with its own test."
EVIDENCE: `crates/gfx/src/svg.rs:159` (`(None, b'"' | b'\'')`), `:864-871` and `:878-885` (two separate tests)
TRACE:    `'` and `"` both enter `quote = Some(byte)`, and the close arm compares `b == open`, so each delimiter only closes on its own kind. `PUBLIC "-//a>b//EN" "http://c>d/x.dtd" [` walks two independent quoted runs and reaches `[` outside both → entity refusal. Both fail pre-fix with `(10, 10)`.
VERDICT:  **PASS**

### DW-1b.3
PREMISE:  "`SYSTEM "a[b"` — a `[` inside a literal, no real subset — still parses; a bracket in a quoted literal is not an internal subset."
EVIDENCE: `crates/gfx/src/svg.rs:158` (`(Some(_), _) => {}`), `:889-897` (test)
TRACE:    `"a[b"` → `[` consumed inside the quoted run → closing `"` → `>` at `(None, b'>')` → `Ok(())` → parse proceeds → `probe` returns `(40, 20)`. Executed: test green. Fails pre-fix (`rest[..end].contains('[')` saw the bracket and refused).
VERDICT:  **PASS**

### DW-1b.4
PREMISE:  "An unterminated literal (`SYSTEM "x` with no closing quote) is refused rather than accepted, and does not panic or scan out of bounds."
EVIDENCE: `crates/gfx/src/svg.rs:170-172` (fall-through `Err`), `:904-916` (refusal test), `:921-931` (prefix/panic test)
TRACE:    `SYSTEM "x\n<svg …` — `<svg` truncates the prologue, so the slice ends mid-literal; `quote` stays `Some(b'"')` to the end of the iterator; the loop falls through to `Malformed("svg: DOCTYPE does not close before the root element")`. Out-of-bounds is impossible by construction: the loop is `for &byte in &prologue.as_bytes()[doctype..]`, an iterator with no indexing. I additionally ran all 84 prefixes of the hostile fixture through the walk — no panic. Refusal test fails pre-fix.
VERDICT:  **PASS** (see Notes for the prefix test's pinning value)

### DW-1b.5
PREMISE:  "`test_a_plain_doctype_is_still_accepted` still passes unchanged — the legitimate Illustrator/Inkscape DOCTYPE is not collateral."
EVIDENCE: `crates/gfx/src/svg.rs:809-818`; `git diff crates/gfx/src/svg.rs` shows the test body appears only as unchanged context.
TRACE:    `PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://…/svg11.dtd">` → two quoted runs consumed opaquely → `>` outside a quote → `Ok(())` → `probe` → `(40, 20)`. Executed green both post-fix and against the restored pre-fix body.
VERDICT:  **PASS**

### DW-1b.6
PREMISE:  "The bypass is refused through the public path `gfx::decode::probe_dimensions`, not merely at the unit level."
EVIDENCE: `crates/gfx/src/svg.rs:941-965` (writes the fixture to disk, calls `crate::decode::probe_dimensions`); reachability at `crates/gfx/src/decode.rs:250-251` and `:287-288`, guard at `crates/gfx/src/svg.rs:404`.
TRACE:    file on disk → `decode::probe_dimensions` → `svg::read_source` (sniff + byte cap) → `svg::probe` → `within_time_cap` → `parse` → `refuse_internal_subset` (first statement) → `Malformed("… own XML entities …")`. Fails pre-fix with `the hidden subset is refused on the public path: (10, 10)`.
VERDICT:  **PASS**

### DW-1b.7
PREMISE:  "Full workspace suite green (≥806 passed, 0 failed), `cargo clippy --workspace --all-targets --all-features -- -D warnings` silent, `cargo fmt --all --check` clean."
EVIDENCE: run outputs recorded above.
TRACE:    818 passed ≥ 806 (812 excluding concurrent reviewers' probes). Clippy silent. fmt clean on every tracked file. The one failure is a `stele` watch-loop test that compares two terminal-truncated path strings and fails only because the mandated `TMPDIR` is 78+ chars; it passes 10/10 under a short `TMPDIR`, and touches no file in this phase.
VERDICT:  **PASS** (with the environmental-failure caveat recorded)

**All requirements met:** YES

## Test-DW Coverage

- [x] Every DW item has an automated test that ran in Step 0 (DW-1b.1 → `test_dw_1b_1_*`; DW-1b.2 → two tests; DW-1b.3 → `test_dw_1b_3_*`; DW-1b.4 → two tests; DW-1b.5 → `test_a_plain_doctype_is_still_accepted`; DW-1b.6 → `test_dw_1b_6_*`; DW-1b.7 → the gate commands themselves).
- [x] Coverage matches the stated level (every behavioural item verified by execution).
- [x] Test names carry DW ids.
- One gap in *pinning* rather than coverage: `test_dw_1b_4_no_prefix_of_a_hostile_doctype_walks_out_of_bounds` passes against the pre-fix body — see Notes.

## Dead Code

None found. No unused imports, no unreachable code after early returns, no debug statements, no commented-out blocks in `crates/gfx/src/svg.rs`. Clippy `-D warnings` is silent across all targets.

## Correctness Dimensions

| Dimension | Status | Evidence |
|-----------|--------|----------|
| Concurrency | PASS | `within_time_cap` abandons the worker rather than joining it (`svg.rs:437-453`) — documented, and the abandoned thread's result is dropped. See Notes for the amplification Issue 1 gives it. |
| Error Handling | PASS | No `unwrap`/`expect`/panic on any production path. `sizer.rs:193` uses `.ok()?` → alt text; `sink.rs:703-711` matches `Err(_)` → `delete_placement` + `degrade_to_text` (sanitized alt text). A refused document degrades cleanly and never propagates as a panic. |
| Resources | PASS | `read_source` bounds the read at `MAX_SVG_BYTES` from the file's own length before reading; `Pixmap` allocation is the fitted target box. |
| Boundaries | PASS | Byte-slice iteration with no indexing; every one of the 84 truncations of the hostile fixture returns a `Result` without panic (executed). ASCII-only delimiters, so no UTF-8 boundary hazard. |
| Security | **FAIL** | Issue 1 — the entity barricade is defeated by three spec-legal prologue constructs, demonstrated end-to-end through `probe_dimensions`. |

## Loaded-Skill Criteria

| Skill | Criterion | Status | Evidence |
|-------|-----------|--------|----------|
| cc-defensive-programming | External input validated at entry (barricade placed at the external boundary) | **FAIL** | Issue 1. The barricade is placed correctly (`parse`'s first statement, reached by both entry points) but its predicate is evadable: a hostile file passes validation and reaches the parser with a live internal subset. Demonstrated by execution. |
| cc-defensive-programming | A barricade does not replace defense-in-depth on security-critical paths | **FAIL** | Issue 1. Once the entity guard no-ops, the only remaining bound is `RENDER_TIME_CAP`, a *liveness* cap that does not bound memory: the abandoned worker keeps expanding after `recv_timeout` returns. `MAX_SVG_BYTES` is explicitly documented (`svg.rs:76-83`) as bounding peak text *because* entities cannot be declared — that premise is false in the bypass. |
| cc-defensive-programming | Assertions used for bugs only; no executable code inside assertions | N/A | The module carries no `assert!`/`debug_assert!` on any production path; error signalling is `Result<_, DecodeError>` throughout. |
| cc-defensive-programming | No empty catch blocks / no silently swallowed errors | PASS | Every `Err` is either returned or mapped to a `DecodeError` with a message. The `(Some(_), _) => {}` and `(None, _) => {}` match arms are scanner state transitions, not swallowed errors. |
| cc-defensive-programming | Correctness-vs-robustness stance is coherent and consistent | PASS | Leans correctness at the parse boundary (refuse rather than render an unread declaration — `svg.rs:170-172`) and robustness at the UI boundary (alt text, never a panic). Both halves executed and observed. |
| cc-defensive-programming | Error-handling strategy applied consistently | PASS | One strategy — `Result<_, DecodeError::Malformed>` with a human-readable message — used for every refusal in the module; matches `crate::decode`'s existing pattern. |

## Notes (non-blocking)

| # | Finding | Confidence | Severity |
|---|---|---|---|
| N1 | `test_dw_1b_4_no_prefix_of_a_hostile_doctype_walks_out_of_bounds` **passes against the pre-fix body** — it does not pin a defect the fix introduced or removed. The pre-fix scan could not panic either (`rest.find('>')` always yields a char boundary; `rest[..end]` is in-bounds). My judgement: **keep it**. It costs one green test and it is a genuine forward guard on the *new* byte-indexed walk, which is the first version of this code where an out-of-bounds slice was ever plausible. But it should be described as a robustness guard rather than a regression pin, and it should not be counted toward "pinned by a test that fails against the pre-fix scan." | High (executed both ways) | Low |
| N2 | The doc comment on `refuse_internal_subset` (`svg.rs:117`) says production 28 "admits only `Name` and `ExternalID` (75)" between `<!DOCTYPE` and the subset's `[`. Verbatim production 28 also admits `S` (whitespace) in two places. Harmless to the argument — whitespace carries neither `>` nor a quote — but the enumeration is incomplete as written. | High (spec-verified) | Very low |
| N3 | A failed probe is never memoized. `ImageSizer` has no cache (`crates/stele/src/media/sizer.rs:193`, `.ok()?`) and `RasterCache` is populated only on decode success (`crates/stele/src/media/sink.rs:664`, reached only from the `Ok` arm at `:704`). A refused SVG is therefore re-probed on every relayout — cheap at 407 µs when the guard fires, catastrophic when it does not (Issue 1). Pre-existing, outside this phase's files. | High (traced + measured) | Medium |
| N4 | `refuse_internal_subset` walks bytes; measured cost on a 4 MiB unterminated-literal prologue (the worst case that reaches the end of the loop) is **2.9 ms**, versus 1.4 ms for the pre-fix `find`. It also runs inside `within_time_cap` on the worker thread, not on the layout thread. The scan's own cost is not a DoS vector. The prompt asked me to try to make it slow enough to matter; I could not. | High (measured) | None |
| N5 | `contains_element` (used by the sniff) is case-insensitive and namespace-prefix aware, while `refuse_internal_subset`'s prologue cut is a literal `find("<svg").or(find("<SVG"))`. A file opening `<sVg` or `<ns0:svg` passes the sniff but leaves `prologue` set to the whole source. That direction is *safe* (more is scanned, and the walk still stops at the DOCTYPE's own `>`), but the two spellings of "where does the root start" have drifted apart and the safe direction is incidental rather than argued. | High (read + replicated) | Low |

## Issues

### 1. The entity barricade is defeated by three spec-legal prologue constructs — a hostile SVG reaches the parser with a live internal subset

- **File:** `crates/gfx/src/svg.rs:145-151` (prologue cut + `find("<!DOCTYPE")`)
- **Severity:** High · **Confidence:** Confirmed by execution through the public path
- **Demonstrated by:** a probe test I wrote and ran (`crates/gfx/tests/zz_review_sample3_probe.rs`, since removed), driving real files off disk through `gfx::decode::probe_dimensions`. Each fixture declares a 232-byte entity referenced 200,000 times (~46 MiB if expanded):

  ```
  [control.svg]         Err(Malformed("svg: declares its own XML entities, which this
                            refuses to expand"))                      in    406.958µs
  [comment-decoy.svg]   Err(Malformed("svg: gave up after 250 ms — too complex to draw"))
                                                                      in 253.491417ms
  [pi-decoy.svg]        Err(Malformed("svg: gave up after 250 ms — too complex to draw"))
                                                                      in 254.784584ms
  [early-svg-text.svg]  Err(Malformed("svg: gave up after 250 ms — too complex to draw"))
                                                                      in 251.581542ms
  [svg-in-literal.svg]  Err(Malformed("svg: DOCTYPE does not close before the root element"))
                                                                      in    636.416µs
  ```

  The control is the shape `test_dw_1b_6` pins, and the guard refuses it in 407 µs. The three middle fixtures are refused by a **different guard** — `RENDER_TIME_CAP` — after burning the full 250 ms budget with the entity expansion actually running. `refuse_internal_subset` returned `Ok(())` for all three.

- **The three bypasses, and why each is legal XML** (productions verified against W3C XML 1.0 Fifth Edition, not from memory — prolog `[22] prolog ::= XMLDecl? Misc* (doctypedecl Misc*)?`, `[27] Misc ::= Comment | PI | S`, `[15] Comment ::= '<!--' ((Char - '-') | ('-' (Char - '-')))* '-->'`, whose only content restriction is no `--`):

  | | Fixture | TRACE |
  |---|---|---|
  | A | `<!-- <!DOCTYPE svg SYSTEM "decoy"> -->` then the real `<!DOCTYPE svg [ <!ENTITY …> ]>` | `prologue.find("<!DOCTYPE")` (`svg.rs:149`) matches the **decoy inside the comment**. The walk consumes `"decoy"` as a quoted run, hits the decoy's `>` at `(None, b'>')` (`svg.rs:166`) → `Ok(())`. The real DOCTYPE, 30 bytes later, is never examined. |
  | B | `<?d <!DOCTYPE svg SYSTEM "z"> ?>` then the real DOCTYPE | Identical, via a PI. `Misc` admits `PI` before `doctypedecl`. |
  | C | `<!-- <svg -->` then the real DOCTYPE | `source.find("<svg")` (`svg.rs:145`) matches the text **inside the comment**, so `prologue` is cut *before* the real DOCTYPE. `prologue.find("<!DOCTYPE")` then returns `None` → `svg.rs:150` → `Ok(())`. The guard never runs a single byte of its walk. `looks_like_svg` still passes on the same text, so the file reaches the SVG path. |

  Fixture D (`SYSTEM "<svg "`) is refused, but by accident: the prologue cut lands mid-declaration, so the walk runs off the end and hits the unterminated-literal arm. It is not evidence the guard handled the construct.

- **These claims in the reviewed file are falsified by the above:**
  - `svg.rs:22` — "[`refuse_internal_subset`] is what actually stops it." It does not stop A, B or C; `RENDER_TIME_CAP` does.
  - `svg.rs:76-83` (`MAX_SVG_BYTES`) — "Custom entities are the only construct that makes an XML document grow, and the only way one reaches a parse here is a DOCTYPE's internal subset — that is what is refused … So peak text stays at roughly the source's own size and this one number caps both." In A/B/C an internal subset *does* reach the parse, so `MAX_SVG_BYTES` bounds the source only. A 78 KiB fixture with a 64 KiB entity referenced 4096 times (256 MiB of text) reached the parser and was again stopped only by the clock: `Err(Malformed("svg: gave up after 250 ms")) in 255.673708ms`.
  - `RENDER_TIME_CAP` is a liveness bound, not a memory bound: `within_time_cap` (`svg.rs:437-453`) returns on `recv_timeout` and **abandons** the worker, which keeps expanding after the caller has moved on.

- **Blast radius, measured.** `ImageSizer` does not cache a failed probe (N3), so this is re-paid per relayout — every fold, resize and theme swap, which is exactly the cost the module doc calls out at `svg.rs:48-50`:

  ```
  8 relayouts of ONE bypassed image:                   2.056122583s  (250–255 ms each,
                                                        layout thread blocked)
  8 relayouts of the SAME bomb when the guard fires:      1.466916ms
  ```

  A ~1400x difference on the layout thread, from one document, with each timed-out probe leaving another abandoned expanding thread behind it.

- **Not introduced by Phase 1b.** I replicated both scanner bodies standalone: the pre-fix scan also returned `Ok` for A, B and C. The phase strictly improved the walk. It is in scope because the phase's subject *is* the completeness of this guard, because the dispatch required verifying the refusal is "total", and because it is a demonstrated violation of a loaded skill's barricade criterion (verdict rule (e)).

- **Fix:** locate the `doctypedecl` by walking the prologue rather than by substring search — skip `<?…?>` and `<!--…-->` runs as opaque, and take the first `<!DOCTYPE` that survives; cut the prologue at the first `<` that opens a real element rather than at the literal text `<svg` (reuse `contains_element`'s delimiter logic so the sniff and the cut agree — N5). Then repair the two claims above.

### 2. A measurement claim in a new doc comment is wrong by 4x and contradicts the sibling comment about the same fixture

- **File:** `crates/gfx/src/svg.rs:844-846`
- **Severity:** Medium · **Confidence:** Confirmed by measurement against the restored pre-fix body
- **Demonstrated by:** the test doc says "Measured through `probe_dimensions` before the fix: `Ok((10, 10))` in **23 ms**, **a megabyte** of entity text expanded. The unquoted twin … was refused in 127 µs." The fixture it refers to (`test_dw_1b_6`, `svg.rs:948`) is 8,192 references to a 32-character entity = **262,144 bytes = 256 KiB**, not a megabyte. I re-ran that exact fixture through `probe_dimensions` with the pre-fix body in place:

  ```
  [hidden] Ok((10, 10)) in 12.1645ms;  entity text would be 262144 bytes
  [twin]   Err(Malformed("svg: declares its own XML entities…")) in 107.375µs
  ```

  The function doc at `svg.rs:106-111` states **256 KiB and 12 ms** for the same measurement, which matches. So the two comments contradict each other and the test-side one is the wrong one, on both magnitude (4x) and timing (23 ms vs 12 ms).

- **Standing:** this alone would not trip the verdict rules — it is not a DW item, a failing test, or a skill criterion. I am listing it under Issues rather than Notes because the phase already fails on Issue 1 and this belongs in the same fix, and because the dispatch states a false claim in these comments is a finding at code-defect severity.
- **Fix:** change "a megabyte of entity text" to "256 KiB" and "23 ms" to "12 ms" at `svg.rs:845`, or delete the duplicated measurement and point at the function doc.

## Verified as correct

The XML spec claims in the guard's doc comment were checked against the W3C XML 1.0 Fifth Edition (`https://www.w3.org/TR/xml/`, cross-checked against `REC-xml-20081126`), not from memory. **All four are accurate:**

| Production | Doc's claim | Spec |
|---|---|---|
| [11] `SystemLiteral` | `('"' [^"]* '"') \| ("'" [^']* "'")` | Exact match — a `>` inside a system literal is legal. |
| [13] `PubidChar` | "lists neither `>` nor `[`" | `#x20 \| #xD \| #xA \| [a-zA-Z0-9] \| [-'()+,./:=?;!*#@$_%]` — correct, neither appears. |
| [28] `doctypedecl` | `'<!DOCTYPE' S Name (S ExternalID)? S? ('[' intSubset ']' S?)? '>'` | Exact match. |
| [75] `ExternalID` | `'PUBLIC' S PubidLiteral S SystemLiteral` | Exact match (the PUBLIC alternative). |

The derived argument is also sound as far as it goes: `Name` (productions 4/4a/5) cannot contain `>`, `"`, `'` or `[`, and production 28 admits nothing between `<!DOCTYPE` and the subset's `[` except `S`, `Name` and `ExternalID` — so *given a correctly located `doctypedecl`*, the one-pass walk is exact. Issue 1 is a defect in locating it, not in the walk.

## Reachability

Every path that hands SVG source to the parser goes through the guard. `parse` (`svg.rs:403-404`) calls `refuse_internal_subset` as its first statement; `probe` (`:463`) and `rasterize_now` (`:520`) are its only callers; `decode::probe_dimensions` (`decode.rs:250-251`) and `decode::decode_and_scale` (`decode.rs:287-288`) are the only SVG entry points outside the module. A workspace-wide grep for `roxmltree`, `usvg::Tree`, `svg::parse|probe|rasterize|read_source` and `refuse_internal_subset` outside `svg.rs` returns only those four `decode.rs` lines. **The barricade is correctly placed; Issue 1 is that its predicate is evadable, not that it can be routed around.**

## Degradation after refusal

Clean, verified by trace and by the executed probes. `sizer.rs:193` maps the error to `None` via `.ok()?`, and `layout/inline.rs:234` renders the image's alt text for a `None` size. `sink.rs:703-711` matches `Err(_)`, calls `delete_placement`, then `degrade_to_text(sanitize(&alt), …)`. No `unwrap`, `expect` or `panic!` on either production path, and the error never escapes to the terminal. The only cost is that nothing is logged — `DecodeError` is never named anywhere in `crates/stele/src`, so a refusal leaves no diagnostic trace — and that failures are not memoized (N3).

**Verdict: FAIL** — blocker: Issue 1 (entity barricade defeated by a decoy `<!DOCTYPE` in a prologue comment or PI, or by the text `<svg` appearing before the DOCTYPE; demonstrated end-to-end through `probe_dimensions`, and a demonstrated violation of the loaded `cc-defensive-programming` barricade criterion). All seven Done-When items are individually satisfied, and six of the seven new tests genuinely fail against the pre-fix scan.
