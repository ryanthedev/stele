# Review: Phase 1b — SVG internal-subset guard (security sample 2)

## Executed Results (Step 0)

| Command | Result |
|---|---|
| `cargo test --workspace --all-features --no-fail-fast` | **818 passed, 1 failed, 8 ignored** (exit 101) |
| `cargo test -p gfx --lib svg::` | 23 passed, 0 failed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | silent |
| `cargo fmt --all --check` | clean (exit 0) |

The single failure is `stele::document_source::test_dw_2_2_an_external_write_repaints_within_a_poll_interval_and_keeps_the_top_block`. It is an artifact of the mandated 90-character `TMPDIR`: the test asserts the 80-column status ruler *changes* after the document grows, and the long temp path clips the differing suffix off the ruler. Re-run under `TMPDIR=/tmp/rv2s` → `1 passed; 0 failed`. Not attributable to `crates/gfx/src/svg.rs`, and outside this phase's files.

---

## Requirement Fulfillment

### DW-1b.1
PREMISE:  `<!DOCTYPE svg SYSTEM "x>y" [ <!ENTITY a "…"> ]>` is refused, with the same error as the unquoted form. Pinned by a test that fails against the pre-fix scan.
EVIDENCE: `crates/gfx/src/svg.rs:848-860` (`test_dw_1b_1_a_subset_behind_a_quoted_gt_is_refused_like_the_unquoted_twin`); guard at `svg.rs:154-172`.
TRACE:    `SYSTEM "x>y" [ <!ENTITY …`→ walk enters quote at `"`, the `>` at `x>y` is consumed as quoted, quote closes, `[` seen outside a quote → `Malformed("svg: declares its own XML entities…")`. The unquoted twin `SYSTEM "xy"` reaches the same `[` and produces a byte-identical `Debug`; the test asserts equality of both.
PRE-FIX:  Replicated `HEAD:crates/gfx/src/svg.rs:113-130` verbatim and ran the same fixture: `SYSTEM "x>y"` → `Ok(())` (accepted), twin `SYSTEM "xy"` → `Err`. The test genuinely discriminates.
VERDICT:  **PASS**

### DW-1b.2
PREMISE:  The single-quoted form `SYSTEM 'x>y'` and the two-literal `PUBLIC "…>…" "…>…"` form are refused too, each with its own test.
EVIDENCE: `svg.rs:865-871` (`test_dw_1b_2_a_single_quoted_literal_hides_no_subset_either`), `svg.rs:879-885` (`test_dw_1b_2_both_public_literals_are_walked_for_a_hidden_subset`). Two separate tests, as required.
TRACE:    `(None, b'"' | b'\'') => quote = Some(byte)` (`svg.rs:159`) opens on either quote and `(Some(open), b) if b == open` (`svg.rs:157`) closes only on the matching one, so `'x>y'` hides its `>` and `PUBLIC "…>…" "…>…"` walks both literals in sequence; the `[` after them is seen outside a quote → entity refusal. Both tests passed in the run above.
PRE-FIX:  Both fixtures → `Ok(())` against the replicated pre-fix scan.
VERDICT:  **PASS**

### DW-1b.3
PREMISE:  `SYSTEM "a[b"` — a `[` inside a literal, no real subset — still parses; a bracket in a quoted literal is not an internal subset.
EVIDENCE: `svg.rs:890-897` (`test_dw_1b_3_a_bracket_inside_a_literal_is_not_an_internal_subset`).
TRACE:    `<!DOCTYPE svg SYSTEM "a[b">` → `"` opens quote, `[` matches `(Some(_), _) => {}` and is ignored, `"` closes, `>` outside a quote → `Ok(())`; roxmltree then parses and the test asserts `probe == (40, 20)`. Passed.
PRE-FIX:  The pre-fix scan returned `Err("declares its own XML entities")` on this fixture — a false positive the new walk removes. The test is a genuine new pin in both directions.
VERDICT:  **PASS**

### DW-1b.4
PREMISE:  An unterminated literal (`SYSTEM "x` with no closing quote) is refused rather than accepted, and does not panic or scan out of bounds.
EVIDENCE: `svg.rs:905-916` (`test_dw_1b_4_an_unterminated_literal_is_refused_rather_than_walked_past`), `svg.rs:922-931` (`test_dw_1b_4_no_prefix_of_a_hostile_doctype_walks_out_of_bounds`), fallthrough at `svg.rs:170-172`.
TRACE:    `SYSTEM "x\n` → quote opens at `"` and never closes; the `for` exhausts `prologue.as_bytes()[doctype..]` and falls through to `Malformed("svg: DOCTYPE does not close before the root element")`. Out-of-bounds is structurally impossible: the loop iterates a slice by value rather than indexing, and `prologue.as_bytes()[doctype..]` is a valid range because `doctype` came from `prologue.find`. The prefix test walks all 130 truncations of a hostile ASCII DOCTYPE; no panic. Both passed.
PRE-FIX:  `Ok(())` — the pre-fix scan accepted it.
VERDICT:  **PASS**

### DW-1b.5
PREMISE:  `test_a_plain_doctype_is_still_accepted` still passes unchanged — the legitimate Illustrator/Inkscape DOCTYPE is not collateral.
EVIDENCE: `svg.rs:810-818`. Diffed against `git show HEAD:crates/gfx/src/svg.rs` lines 767-775 — **byte-identical**, body and fixture.
TRACE:    `<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://…/svg11.dtd">` → both literals walked as opaque, `>` outside a quote → `Ok(())` → `probe == (40, 20)`. Passed in the run above.
VERDICT:  **PASS**

### DW-1b.6
PREMISE:  The bypass is refused through the public path `gfx::decode::probe_dimensions`, not merely at the unit level.
EVIDENCE: `svg.rs:942-965` (`test_dw_1b_6_the_reproduced_bypass_is_refused_through_probe_dimensions`); `crates/gfx/src/decode.rs:249-252`.
TRACE:    Fixture written to disk → `decode::probe_dimensions` → `svg::read_source` (sniff + byte cap) → `svg::probe` → `within_time_cap` → `parse` → `refuse_internal_subset` → `Malformed("…own XML entities…")`. I independently confirmed `parse` (`svg.rs:403-423`) is the only call site of `roxmltree::Document::parse_with_options` / `usvg::Tree::from_*` in the workspace, and that `refuse_internal_subset` is its first statement, so `probe` and `rasterize_now` are both covered and no caller reaches the parser around the guard.
VERDICT:  **PASS** — for *this* fixture shape. See the FAIL below: the same public path accepts four other shapes that do declare entities.

### DW-1b.7
PREMISE:  Full workspace suite green (≥806 passed, 0 failed), clippy silent, fmt clean.
EVIDENCE: Executed Results table above.
TRACE:    818 passed exceeds 806; clippy and fmt are clean. The literal "0 failed" is not met under the mandated `TMPDIR` — one `stele` PTY test fails on ruler clipping caused by the 90-character temp path, and passes under a short `TMPDIR`.
VERDICT:  **PARTIAL** — count and lint gates met; the zero-failure gate is met only outside the mandated environment. Not a blocker (demonstrated environmental, file outside this phase), but reported rather than rounded up.

**All requirements met:** NO — see Issue 1.

---

## The central task: attacking the guard from the parser's side

I read `roxmltree-0.21.1/src/tokenizer.rs` (`parse` at :242, `parse_misc` at :277, `parse_doctype` at :393, `parse_doctype_start` at :445, `parse_external_id` at :467) and walked its acceptance conditions against `svg.rs:144-173` line by line. **They disagree, and the disagreement is exploitable.**

### Root cause: the guard has no model of XML production 22

roxmltree's `parse` (tokenizer.rs:242-257) runs, in order: `parse_declaration` → **`parse_misc`** → `skip_spaces` → `if s.starts_with(b"<!DOCTYPE")`. `parse_misc` (tokenizer.rs:277-291) consumes `Comment | PI | S` in a loop. That is XML production 22 verbatim — `prolog ::= XMLDecl? Misc* (doctypedecl Misc*)?` — confirmed against the W3C REC.

The guard models production 28 (the DOCTYPE's *interior*) correctly, and models production 22 (how you *find* the DOCTYPE) not at all. It locates the declaration with two textual heuristics that a comment or PI defeats:

1. `source.find("<svg")` (`svg.rs:145`) — truncates the prologue at the first literal `<svg` **anywhere**, including inside a comment or PI, where roxmltree sees only comment text.
2. `prologue.find("<!DOCTYPE")` (`svg.rs:149`) — takes the **first** occurrence, including a decoy inside a comment or PI, where roxmltree ignores it entirely.

Either one alone is a complete bypass.

### Executed results — `gfx::decode::probe_dimensions`, real files on disk

Every row is the *same* bomb (one 800-byte entity, 2,000 references, ~6.9 KB source) with only the prologue decoy varying. Reproduced across three separate runs.

| Decoy inserted before the DOCTYPE | `probe_dimensions` result | Elapsed | Entity text expanded |
|---|---|---|---|
| *(none — control)* | `Err(Malformed("svg: declares its own XML entities…"))` | **122 µs** | 0 |
| `<!-- <svg -->` | **`Ok((10, 10))`** | 30.5 ms | **1.5 MiB** |
| `<!-- <!DOCTYPE x> -->` | **`Ok((10, 10))`** | 29.2 ms | **1.5 MiB** |
| `<?z <svg ?>` | **`Ok((10, 10))`** | 28.9 ms | **1.5 MiB** |
| `<?z <!DOCTYPE x> ?>` | **`Ok((10, 10))`** | 29.1 ms | **1.5 MiB** |

Thirteen bytes of comment turn a 122 µs refusal into a successful parse that expands 230x.

### Expansion factor bought

| Source | Refs | Expansion | Factor | Result |
|---|---|---|---|---|
| 6.9 KB | 2,000 | 1.5 MiB | 230x | `Ok((10,10))` in 32 ms |
| 9.4 KB | 8,000 | 6.1 MiB | 680x | `Ok((10,10))` in 120 ms |
| 13.0 KB | 16,000 | 12.2 MiB | **940x** | `Ok((10,10))` in 239 ms |
| 601 KB | 200,000 | 152.6 MiB | 266x | time cap at 254 ms |
| 2.1 MB | 700,000 | 534 MiB | 267x | time cap at 253 ms |

12.2 MiB of expansion returns **successfully** inside the 250 ms cap. Extrapolating the 267x ratio to `MAX_SVG_BYTES` (4 MiB) gives ~1.1 GiB of entity text on a single parse — the exact class the module doc says `refuse_internal_subset` "is what actually stops it."

`RENDER_TIME_CAP` is the only thing standing behind the breach, and it bounds *wall time on the calling thread*, not work. Measured: ten `probe_dimensions` calls on a 229 MiB bypassed bomb — one per relayout, and `svg.rs:48-50` notes `ImageSizer` never caches the probe — cost **2.54 s** of blocked layout thread and left RSS at 92 MiB with abandoned workers still running.

### Inputs where the two parsers agreed (also executed)

| Probe | Result | Notes |
|---|---|---|
| `<svg` inside the DOCTYPE's own `SystemLiteral`, before the `[` | `Err("DOCTYPE does not close before the root element")` | Truncation lands mid-quote; the walk runs off the end. Safe. |
| `<svg` inside the internal subset | refused | The `[` is always passed first. Safe. |
| Uppercase `<SVG` root | covered by `.or_else(find("<SVG"))` | Mixed-case `<sVg` finds neither → `prologue = source`, i.e. a *wider* scan. Safe direction. |
| DOCTYPE beyond the 1024-byte sniff window | `Err(Malformed("The file extension `.\"svg\"` was not recognized…"))` | `read_source` returns `Ok(None)`; falls to the raster path and is refused. No bypass — but not a mitigation either, since attack A puts `<svg` inside the window via a comment. |
| Innocent file whose comment mentions `<!DOCTYPE svg>` | `Ok((40, 20))` | Renders. Any fix must keep this. |
| Any path into `roxmltree`/`usvg` around the guard | none exists | `parse` (`svg.rs:403`) is the sole parser call site workspace-wide; `refuse_internal_subset` is its first line. |

One caution, reported for completeness: in a single early run the `<svg`-inside-`SystemLiteral` fixture printed `Ok((10, 10))` in 31 ms. It did **not** reproduce across three later runs on byte-identical input, and a verbatim replica of the guard returns `Err("does not close")` on those exact bytes. I could not reproduce it and do not claim it as a finding; noting it because concurrent processes share `std::env::temp_dir()` and a stale fixture is the likeliest explanation.

---

## Verify the comment against the spec

Checked verbatim against the W3C XML 1.0 REC (`https://www.w3.org/TR/xml/`), not from memory.

| Claim in `svg.rs` | Spec | Verdict |
|---|---|---|
| :104 — production **11** `SystemLiteral ::= ('"' [^"]* '"') \| ("'" [^']* "'")` | identical | **Correct** |
| :116-117 — production **28** `'<!DOCTYPE' S Name (S ExternalID)? S? ('[' intSubset ']' S?)? '>'` | identical | **Correct** |
| :117-118 — "between `<!DOCTYPE` and the subset's `[` it admits only `Name` and `ExternalID` (**75**)"; production 75 quoted at :874 as `'PUBLIC' S PubidLiteral S SystemLiteral` | `ExternalID ::= 'SYSTEM' S SystemLiteral \| 'PUBLIC' S PubidLiteral S SystemLiteral` | **Correct** |
| :122-123 — "`PubidChar` (**13**) lists neither `>` nor `[`" | `#x20 \| #xD \| #xA \| [a-zA-Z0-9] \| [-'()+,./:=?;!*#@$_%]` | **Correct** |
| :118-119 — "No unquoted `>` can precede the `[`, so whichever of the two turns up first outside a quote settles it." | true of a real `doctypedecl` | **Correct as stated, but the premise is unestablished** — the walk begins at a byte match for `<!DOCTYPE`, which need not be a `doctypedecl` at all (production 22 admits `Misc*` first). The soundness argument is about a production the guard never verifies it is looking at. |
| :141-143 — "A `<!DOCTYPE` inside a comment in the prologue would be a **false positive**; that costs one unusual file a render, where a false negative costs the reader their terminal." | — | **FALSE.** Executed: `<!-- <!DOCTYPE x> -->` before a real subset yields `Ok((10, 10))` with 1.5 MiB expanded — a **false negative**, in the exact construct the comment names. The comment asserts the error can only go the safe way; the demonstrated error goes the unsafe way. |

Four of the six spec claims are verbatim right; the fifth is right-but-load-bearing-on-an-unchecked-premise; the sixth is wrong in the direction that matters, and it is the sentence that licensed the design.

---

## Test-DW Coverage

- [x] DW-1b.1 → `test_dw_1b_1_a_subset_behind_a_quoted_gt_is_refused_like_the_unquoted_twin` (ran, passed)
- [x] DW-1b.2 → `test_dw_1b_2_a_single_quoted_literal_hides_no_subset_either` + `test_dw_1b_2_both_public_literals_are_walked_for_a_hidden_subset` (two tests, as required; both ran, passed)
- [x] DW-1b.3 → `test_dw_1b_3_a_bracket_inside_a_literal_is_not_an_internal_subset` (ran, passed)
- [x] DW-1b.4 → `test_dw_1b_4_an_unterminated_literal_is_refused_rather_than_walked_past` + `test_dw_1b_4_no_prefix_of_a_hostile_doctype_walks_out_of_bounds` (ran, passed)
- [x] DW-1b.5 → `test_a_plain_doctype_is_still_accepted` (ran, passed; byte-identical to HEAD)
- [x] DW-1b.6 → `test_dw_1b_6_the_reproduced_bypass_is_refused_through_probe_dimensions` (ran, passed)
- [x] DW-1b.7 → observed behaviour, commands above
- [x] Coverage level "every behavioural item verified by execution" — met.

**Gap:** no test covers a DOCTYPE reached past a prologue comment or PI, which is where the guard fails. Coverage of the *stated* items is complete; the item set does not span the guard's contract.

---

## Dead Code

None blocking. No unreachable code, no debug statements, no commented-out blocks, no unused imports (clippy `-D warnings` silent across `--all-targets`). One misplaced doc comment — see Notes.

---

## Correctness Dimensions

| Dimension | Status | Evidence |
|-----------|--------|----------|
| Concurrency | PASS | `within_time_cap` (`svg.rs:437-453`) moves an owned `String` into the worker; no shared mutable state. `fonts()`/`no_fonts()` use `OnceLock` and hand out `Arc` clones. `let _ = tx.send(...)` correctly ignores the disconnected-receiver case. |
| Error Handling | PASS | Every I/O and parse result is mapped to `DecodeError`; `read_source` retries `EINTR` (`svg.rs:238`) and reads in a loop rather than trusting one short `read`. No swallowed errors. |
| Resources | PASS (with note) | The `File` drops at scope end; the pixmap is bounded by the fitted target. Abandoned worker threads are documented design (`svg.rs:427-432`) — quantified under Notes, not a defect against any stated requirement. |
| Boundaries | PASS | The walk iterates a slice by value, never indexes; `doctype` is a `find` result so `[doctype..]` is in range; the byte walk is UTF-8-safe because every delimiter is ASCII and no continuation byte is (`svg.rs:152-153`). `test_dw_1b_4_no_prefix_of_a_hostile_doctype_walks_out_of_bounds` exercises all 130 truncations. |
| **Security** | **FAIL** | `refuse_internal_subset` accepts four documents that declare custom entities. `<!-- <!DOCTYPE x> -->` before a real internal subset → `probe_dimensions` returns `Ok((10, 10))` having expanded 1.5 MiB; 12.2 MiB from 13 KB of source (940x) still returns `Ok`. Full trace and table above. |

## Loaded-Skill Criteria

| Skill | Criterion | Status | Evidence |
|-------|-----------|--------|----------|
| cc-defensive-programming | External input validated at entry (barricade) | **FAIL** | `refuse_internal_subset` **is** the barricade for the SVG trust boundary and its stated invariant ("no document reaching the parser can declare custom entities", `svg.rs:96-100`) is violated by four demonstrated inputs. Everything downstream — `MAX_SVG_BYTES`'s "bytes of source are bytes of text" argument (`svg.rs:75-83`), `test_the_node_cap_is_reachable_within_the_byte_cap`'s "no expansion factor enters the arithmetic" (`svg.rs:754-757`) — assumes inside the barricade what the barricade does not deliver. |
| cc-defensive-programming | Correctness over robustness on a data-corrupting path | **FAIL** | The guard chooses the safe direction correctly at every point it *reaches* (unterminated literal → refuse, `svg.rs:170`), but its two locators fail open: an unmatched heuristic yields `Ok(())` (`svg.rs:150`) rather than a refusal. On a path whose documented failure mode is unbounded memory on the layout thread, "I could not find a DOCTYPE" is being treated as "there is no DOCTYPE." |
| cc-defensive-programming | No empty catch / swallowed errors | PASS | Every `Result` is propagated or explicitly discarded with a stated reason. |
| cc-defensive-programming | Assertions for bugs only, not for anticipated input | PASS | No assertions on the production path; all hostile-input handling is `Result`-based. |
| cc-defensive-programming | Defense in depth on a security-critical path | PARTIAL | `RENDER_TIME_CAP` and `MAX_XML_NODES` did contain the larger bypassed bombs, which is why this is a 12 MiB incident rather than a 1.1 GiB one. But the node cap cannot see this class (consecutive references collapse to one text node, `svg.rs:16-18`) and the time cap bounds only the caller's wait — a real second layer for entity expansion does not exist. |

---

## Notes (non-blocking)

| # | Finding | Confidence | Severity |
|---|---|---|---|
| N1 | **Misplaced doc comment.** `svg.rs:380-386` documents `parse` ("Parses `source` into a `usvg` tree under this module's limits… `from_str` builds its own `ParsingOptions`…") but is attached to `enum Fonts` at :388 — its text runs straight into `Fonts`' own `///` line at :387 with no blank line. `fn parse` at :403 is left with no doc comment, and rustdoc renders the `parse` prose on `Fonts`. | Certain | Low |
| N2 | **`test_dw_1b_4_no_prefix_of_a_hostile_doctype_walks_out_of_bounds` asserts nothing** (`svg.rs:928-930`: `let _ = refuse_internal_subset(...)`). It is a panic/OOB probe, which is what its name and doc claim, so this is not a defect — but it will pass unchanged if the function is later rewritten to return a wrong-but-non-panicking answer. | Certain | Low |
| N3 | **Abandoned workers under a relayout storm.** `within_time_cap` detaches the worker (documented, `svg.rs:427-432`). Measured on a 229 MiB bypassed bomb: ten `probe_dimensions` calls cost 2.54 s of blocked layout thread and left RSS at 92 MiB with workers still running. Pre-existing design, and only reachable at this scale *because* of Issue 1 — folded into it rather than raised separately. | Certain | Med (contingent on Issue 1) |
| N4 | **The sniff window is not the mitigation it might look like.** A DOCTYPE large enough to push `<svg` past `SNIFF_BYTES` makes the file unrecognized (verified: `read_source` → `Ok(None)` → raster path → refused). But a comment carrying `<svg` inside the window defeats that incidentally, which is exactly attack A. Worth stating so nobody later treats the 1024-byte window as a second layer. | High | Low |
| N5 | **Workspace suite fails one `stele` PTY test under the mandated long `TMPDIR`** (ruler clipping at 80 columns). Passes under a short `TMPDIR`. Environmental, outside this phase's files; reported rather than dropped because DW-1b.7 says "0 failed". | Certain | Low |

---

## Issues (FAIL)

### 1. `refuse_internal_subset` is bypassed by any prologue comment or processing instruction

- **File:** `crates/gfx/src/svg.rs:144-151` (both locators), doc claim at `:141-143`
- **Demonstrated by:** four fixtures driven through `gfx::decode::probe_dimensions` from real files on disk, reproduced across three runs:

  | Prologue decoy | Result | Expanded |
  |---|---|---|
  | *(control, none)* | `Err(Malformed("svg: declares its own XML entities…"))` in 122 µs | 0 |
  | `<!-- <svg -->` | `Ok((10, 10))` in 30.5 ms | 1.5 MiB |
  | `<!-- <!DOCTYPE x> -->` | `Ok((10, 10))` in 29.2 ms | 1.5 MiB |
  | `<?z <svg ?>` | `Ok((10, 10))` in 28.9 ms | 1.5 MiB |
  | `<?z <!DOCTYPE x> ?>` | `Ok((10, 10))` in 29.1 ms | 1.5 MiB |

  Ceiling that still returns `Ok`: 13 KB of source → **12.2 MiB** expanded (940x) in 239 ms. Beyond that `RENDER_TIME_CAP` fires, but only after the work is under way — 2.1 MB of source drives 534 MiB of expansion on a worker the caller has stopped waiting for.

- **Traces:**
  - *Comment-`<svg`*: `source.find("<svg")` matches inside `<!-- <svg -->` → `prologue = "<?xml version=\"1.0\"?>\n<!-- "` → `prologue.find("<!DOCTYPE")` → `None` → `svg.rs:150` returns `Ok(())`. roxmltree's `parse_misc` (tokenizer.rs:277) consumes the comment, then `starts_with(b"<!DOCTYPE")` (tokenizer.rs:255) matches the real declaration and `parse_entity_decl` (tokenizer.rs:495) registers the entity.
  - *Comment-`<!DOCTYPE`*: `prologue.find("<!DOCTYPE")` matches the decoy inside the comment; the walk reaches the decoy's unquoted `>` and `svg.rs:166` returns `Ok(())`, never examining the real declaration that follows.
  - PI variants are the same two traces via `parse_pi` (tokenizer.rs:286).

- **Why the existing tests miss it:** every fixture in the file puts the DOCTYPE first in the prologue. The guard's correctness argument (`svg.rs:113-119`) reasons about production 28 — the declaration's interior — and never about production 22, which is how the declaration is *located*. Production 22 is `XMLDecl? Misc* (doctypedecl Misc*)?`, and `Misc ::= Comment | PI | S`.

- **Fix:** stop locating the DOCTYPE textually. Walk the prologue as roxmltree does — skip `<?xml…?>`, then loop consuming `<!--`…`-->` and `<?`…`?>` and whitespace — and treat the *first non-Misc token* as the DOCTYPE candidate: if it starts with `<!DOCTYPE`, run the existing quote-aware walk on it; if it starts with `<`, there is no DOCTYPE; if a comment or PI is unterminated, refuse. This also removes the need for the `find("<svg")` truncation entirely, since the scan now stops at the root element by construction. Keep `test_a_plain_doctype_is_still_accepted` and the innocent-comment case (`<!-- see <!DOCTYPE svg> in the spec -->` must still render — verified `Ok((40, 20))` today) green, and add a test per decoy shape above.

- **Also fix:** the false claim at `svg.rs:141-143`. It states a comment-borne `<!DOCTYPE` "would be a false positive"; executed, it is a false negative. That sentence is the stated justification for accepting the heuristic, so it must not survive the fix.

---

**Verdict: FAIL** — blockers:
1. `refuse_internal_subset` accepts documents declaring custom entities whenever a comment or PI precedes the DOCTYPE (four demonstrated shapes, `Ok((10,10))` with up to 12.2 MiB expanded on the public `probe_dimensions` path); the guard's stated invariant, the `MAX_SVG_BYTES` argument that rests on it, and the `cc-defensive-programming` barricade criterion all fail with it.
2. `svg.rs:141-143` makes a false claim about the direction of that very error — asserting a comment-borne `<!DOCTYPE` can only over-refuse, when it under-refuses.

DW-1b.1 through DW-1b.6 are each individually PASS on their own fixtures, and DW-1b.7 is met but for one environmental `stele` failure. The phase fails on the demonstrated security defect and the false spec-adjacent claim, not on its Done-When list.
