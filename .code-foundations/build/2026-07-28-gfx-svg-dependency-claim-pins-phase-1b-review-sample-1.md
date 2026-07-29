# Review: Phase 1b — SVG internal-subset guard (sample 1)

## Executed Results (Step 0)

- `cargo test -p gfx` → 63 passed, 0 failed (lib) + 1 passed / 2 ignored (live_ghostty) + 0/1 ignored (svg_cost)
- `cargo test --workspace --all-features` → **826 passed, 0 failed, 8 ignored**, exit 0
  - Run under `TMPDIR=/private/tmp/c501rev1`. Under the *mandated* long TMPDIR the suite exits 101 with one failure in `stele::document_source` — an artifact of path length, not of this phase. See Notes.
  - 826 includes 13 tests from two other reviewers' scratch files present in the tree (`crates/gfx/tests/zz_review_sample2_probe.rs`, `zz_review_sample3_probe.rs`). Net of those: 813, still ≥806.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` → exit 0, no diagnostics
- `cargo fmt --all --check` → exit 0, clean

## Requirement Fulfillment

### DW-1b.1
PREMISE: `<!DOCTYPE svg SYSTEM "x>y" [ <!ENTITY a "…"> ]>` is refused, with the same error as the unquoted form. Pinned by a test that fails against the pre-fix scan.
EVIDENCE: `crates/gfx/src/svg.rs:154-172` (walk), test at `:848-860`
TRACE: `subset_behind(r#"SYSTEM "x>y""#)` → walk enters quote at `"`, `>` at index of `x>y` is consumed as quoted, quote closes, `[` seen with `quote == None` → `Malformed("…declares its own XML entities…")`; `assert_eq!` against the `SYSTEM "xy"` twin's `Debug` string holds.
Pre-fix check: I restored the old `find('>')` body verbatim into `refuse_internal_subset` and re-ran — this test failed with `an internal subset is refused: (10, 10)`. File restored byte-identical afterwards (`diff` clean).
VERDICT: PASS

### DW-1b.2
PREMISE: The single-quoted form `SYSTEM 'x>y'` and the two-literal `PUBLIC "…>…" "…>…"` form are refused too, each with its own test.
EVIDENCE: `crates/gfx/src/svg.rs:865-871` and `:879-885`; guard arm `(None, b'"' | b'\'')` at `:159`
TRACE: `SYSTEM 'x>y'` → `'` opens the quote, `>` opaque, `[` outside → entity error. `PUBLIC "-//a>b//EN" "http://c>d/x.dtd"` → both literals opened and closed in turn, `[` outside → entity error. Both ran green; both failed against the restored pre-fix scan.
VERDICT: PASS

### DW-1b.3
PREMISE: `SYSTEM "a[b"` — a `[` inside a literal, no real subset — still parses.
EVIDENCE: `crates/gfx/src/svg.rs:890-897`
TRACE: `<!DOCTYPE svg SYSTEM "a[b">` → `[` is seen with `quote == Some(b'"')` so it hits the `(Some(_), _) => {}` arm, then `>` outside the quote → `Ok(())`; `probe` returns `(40, 20)`. Against the pre-fix scan this test **failed** (old code did `rest[..end].contains('[')` and refused) — a genuine false-positive regression pin.
VERDICT: PASS

### DW-1b.4
PREMISE: An unterminated literal (`SYSTEM "x` with no closing quote) is refused rather than accepted, and does not panic or scan out of bounds.
EVIDENCE: `crates/gfx/src/svg.rs:905-916` (refusal) and `:922-931` (prefix sweep); fall-through `Err` at `:170-172`
TRACE: `<!DOCTYPE svg SYSTEM "x\n<svg …/>` → prologue truncated at the root `<svg`, walk enters the quote and never leaves, loop exhausts → `Malformed("svg: DOCTYPE does not close before the root element")`. The prefix sweep slices `hostile[..end]` for every `end` in `0..=len` and calls the guard; nothing panicked. The refusal test failed against the pre-fix scan (old code accepted); the prefix sweep passed pre-fix too, so the refusal test is the discriminating half.
VERDICT: PASS

### DW-1b.5
PREMISE: `test_a_plain_doctype_is_still_accepted` still passes unchanged.
EVIDENCE: `crates/gfx/src/svg.rs:810-818`
TRACE: `git diff crates/gfx/src/svg.rs` contains no line touching that test (grep for the name over the diff: no match; all 9 removed lines are the old scan body). Ran by name: `test svg::tests::test_a_plain_doctype_is_still_accepted ... ok`. The Illustrator/Inkscape `PUBLIC "-//W3C//DTD SVG 1.1//EN" "…svg11.dtd"` DOCTYPE walks both literals and exits on the unquoted `>` → `Ok`, probe `(40, 20)`.
VERDICT: PASS

### DW-1b.6
PREMISE: The bypass is refused through the public path `gfx::decode::probe_dimensions`, not merely at the unit level.
EVIDENCE: `crates/gfx/src/svg.rs:942-965`
TRACE: fixture written to disk, `crate::decode::probe_dimensions(&path, Limits::default())` → `read_source` sniffs, byte cap passes, `svg::probe` → `parse` → guard → `Malformed("…own XML entities…")`. Green; failed pre-fix with `the hidden subset is refused on the public path: (10, 10)`.
Scope note: this DW item is satisfied *for the reproduced `SYSTEM "x>y"` fixture*. It is not evidence that the guard is sound — see Issues 1 and 2, both demonstrated through this same entry point.
VERDICT: PASS

### DW-1b.7
PREMISE: Full workspace suite green (≥806 passed, 0 failed), clippy silent, fmt clean.
EVIDENCE: command output above
TRACE: 826 passed / 0 failed / 8 ignored (813 net of foreign scratch files); clippy exit 0 with an empty log; fmt exit 0.
VERDICT: PASS (with the TMPDIR and foreign-scratch caveats in Notes)

**All requirements met:** YES — every Done-When item passes. The phase nonetheless FAILS on the dispatch's central task; see Issues.

## Test-DW Coverage

- [x] Every DW item has an automated test that ran in Step 0 (`test_dw_1b_1`…`test_dw_1b_6`, plus `test_a_plain_doctype_is_still_accepted` for 1b.5 and the three gate commands for 1b.7).
- [x] Test names carry DW ids; coverage matches "every behavioural done-when item verified by execution".
- [x] DW-1b.1's "fails against the pre-fix scan" clause independently verified by restoring the pre-fix body: 6 of the 7 `test_dw_1b_*` tests failed.

No gaps. The gap is not in DW coverage — it is that the DW list does not cover the guard's actual attack surface.

## The central task: defeating the guard

I found **two independent bypass families**, both reaching `roxmltree` with a live internal subset through `gfx::decode::probe_dimensions`. Both are well-formed XML: production **[22]** `prolog ::= XMLDecl? Misc* (doctypedecl Misc*)?` with **[27]** `Misc ::= Comment | PI | S` makes a comment or PI before the DOCTYPE fully legal, so these are not malformed-input edge cases — they are ordinary documents.

### Family A — a `<svg` substring in the prologue truncates the scan

`refuse_internal_subset` (`svg.rs:145`) anchors on `source.find("<svg")`. That is a raw substring search, not an element match. Any earlier occurrence — inside a prologue comment or PI — cuts `prologue` short of the real DOCTYPE, so `prologue.find("<!DOCTYPE")` returns `None` and the guard returns `Ok(())` at `:150` without examining anything.

Minimal fixture, 142 bytes:

```
<!--<svg--><!DOCTYPE svg [ <!ENTITY a "hello"> ]>
<svg xmlns="http://www.w3.org/2000/svg" width="7" height="9"><desc>&a;</desc></svg>
```

→ `probe_dimensions` returned `*** BYPASS *** Ok((7, 9))`. The identical document without the 11-byte comment was refused: `Malformed("svg: declares its own XML entities, which this refuses to expand")`.

Scaled, one 256 B entity referenced 32,768 times, all through `probe_dimensions`:

| Fixture | Prologue inserted before the DOCTYPE | Source | Entity text | Factor | Elapsed | Result |
|---|---|---|---|---|---|---|
| B0 control | *(none)* | 98,698 B | 8,388,608 B | 85x | 815 µs | refused — "declares its own XML entities" |
| B1 | `<!-- <svg -->` | 98,712 B | 8,388,608 B | 85x | 165 ms | **BYPASS `Ok((10, 10))`** |
| B2 | `<!--<svgx-->` | 98,711 B | 8,388,608 B | 85x | 168 ms | **BYPASS `Ok((10, 10))`** |
| B3 | `<?stele <svg ?>` | 98,714 B | 8,388,608 B | 85x | 171 ms | **BYPASS `Ok((10, 10))`** |
| B4 | `<!-- <svg/> --><?pi x?>` | 98,722 B | 8,388,608 B | 85x | 166 ms | **BYPASS `Ok((10, 10))`** |
| B5 | `<!-- <SVG -->` | 98,712 B | 8,388,608 B | 85x | 411 µs | refused (the `find("<svg")` for the real root wins) |

B2 is worth singling out: `<svgx` is not an element and does not even satisfy `contains_element`'s delimiter rule, yet `find("<svg")` matches it. The bypass needs no plausible SVG syntax at all.

A corollary I hit while building the fixtures: `<!-- <svg -->` also makes `looks_like_svg` succeed on the comment, so a document whose real root sits *past* the 1024-byte sniff window still gets routed to the SVG parser. Family A therefore also answers the brief's sniff-window question in the affirmative.

### Family B — the first `<!DOCTYPE` shadows the real one

`prologue.find("<!DOCTYPE")` (`svg.rs:149`) takes the **first** occurrence, and the walk returns `Ok(())` the moment it meets that declaration's unquoted `>` (`:166`). A decoy DOCTYPE inside a prologue comment or PI shadows the real, subset-bearing one — which is never examined. This family does not need the `<svg` trick.

Minimal fixture, 142 bytes:

```
<!-- <!DOCTYPE svg> --><!DOCTYPE svg [ <!ENTITY a "hi"> ]>
<svg xmlns="http://www.w3.org/2000/svg" width="7" height="9"><desc>&a;</desc></svg>
```

→ `*** BYPASS *** Ok((7, 9))`.

| Fixture | Prologue inserted | Source | Entity text | Factor | Elapsed | Result |
|---|---|---|---|---|---|---|
| D0 control | *(none)* | 98,698 B | 8,388,608 B | 85x | 802 µs | refused — "declares its own XML entities" |
| D1 | `<!-- <!DOCTYPE svg SYSTEM "decoy.dtd"> -->` | 98,741 B | 8,388,608 B | 85x | 169 ms | **BYPASS `Ok((10, 10))`** |
| D2 | `<!DOCTYPE svg SYSTEM "decoy.dtd">` (bare, no comment) | 98,732 B | — | — | 798 µs | refused by roxmltree — two DOCTYPEs is not well-formed |
| D3 | `<?pi <!DOCTYPE svg SYSTEM "d.dtd"> ?>` | 98,736 B | 8,388,608 B | 85x | 169 ms | **BYPASS `Ok((10, 10))`** |

Note the guard's own doc (`svg.rs:141-143`) anticipates a `<!DOCTYPE` in a prologue comment and calls it a **false positive**. D1 shows it is a false **negative** — the opposite direction, and the one the same sentence says "costs the reader their terminal".

### What held

| Shape tried | Outcome |
|---|---|
| `<!doctype` / `<!DocType` (mixed case) | Held. roxmltree rejects: `invalid name token at 2:2`. XML is case-sensitive and production [28] fixes the literal `'<!DOCTYPE'`, so a lowercase spelling is not legal XML — the exact-case `find` is correct. |
| `<svg` inside the DOCTYPE's own `SystemLiteral` | Held. Prologue truncates *inside* the quote, walk never closes it, fall-through `Err` at `:170` — fails closed. |
| `<svg` reachable only via an entity value | Held. The `[` always precedes the value, so the walk meets it first. |
| Namespace-prefixed root `<ns0:svg>` | Held. `find("<svg")` misses it, so `prologue` is the whole source and the DOCTYPE is fully scanned. |
| Adjacent quote runs, `]]>` inside the DOCTYPE (`PUBLIC "a""b" 'c]]>d' [ … ]`) | Held. Refused with the entity message. |
| Two bare DOCTYPEs, no comment (D2) | Held — by roxmltree, not by the guard. |
| External entity `<!ENTITY xxe SYSTEM "file:///etc/passwd">` behind a Family-A bypass | Not resolved: `unknown entity reference 'xxe'`. Confirms `entity_resolver: None` (`svg.rs:136-138`). No SSRF or file-read reachable. |
| Any other entry point into `parse` skipping the guard | None. `parse` is called only from `probe` (`:463`) and `rasterize_now` (`:520`), and `refuse_internal_subset` is its first line (`:404`). Verified by grep over `crates/gfx/src/`. |

### What the bypass costs, once the time cap is the only thing left

At scale the wall-clock cap does fire and the caller gets an error — but `within_time_cap` (`:437-453`) detaches the worker. Measured: 3,900,408 B of source (inside `MAX_SVG_BYTES`) declaring one 256 B entity referenced 1,300,000 times = **332,800,000 B of entity text**.

```
probe returned after 255.456458ms: Some(Malformed("svg: gave up after 250 ms — too complex to draw"))
RSS at return:               18.6 MiB
RSS +1000 ms after return:   20.8 MiB
RSS +3000 ms after return:   29.7 MiB
RSS +5000 ms after return:   38.6 MiB      (still climbing, ~4.4 MiB/s)
```

The caller has its error; the process is still allocating toward 332 MB on a detached thread. `within_time_cap`'s doc claims the abandoned thread "finishes and drops its result harmlessly" (`:427-428`). On this path it does not.

## Verify the comment against the spec

Checked against W3C REC-xml-20081126 (fetched, not recalled). **All four production claims are correct, verbatim.**

| Claim in the comment | Spec | Verdict |
|---|---|---|
| `SystemLiteral` is `('"' [^"]* '"') \| ("'" [^']* "'")` (production 11) — `svg.rs:103-104` | [11] `SystemLiteral ::= ('"' [^"]* '"') \| ("'" [^']* "'")` | Correct, character-for-character |
| production 28 is `'<!DOCTYPE' S Name (S ExternalID)? S? ('[' intSubset ']' S?)? '>'` — `svg.rs:114-115` | [28] identical, internal-subset group optional | Correct, including the optional group |
| `ExternalID` is production 75, only free-form regions are its literals — `svg.rs:116-118` | [75] `ExternalID ::= 'SYSTEM' S SystemLiteral \| 'PUBLIC' S PubidLiteral S SystemLiteral` | Correct |
| `PubidChar` (13) lists neither `>` nor `[` — `svg.rs:122-123` | [13] `PubidChar ::= #x20 \| #xD \| #xA \| [a-zA-Z0-9] \| [-'()+,./:=?;!*#@$_%]` | Correct — neither character appears |
| test doc quoting [75] and [13] — `svg.rs:873-877` | as above | Correct |

The *reasoning built on* those productions is also locally sound: within a single DOCTYPE declaration, no unquoted `>` can precede the `[`, so the one-pass walk is exact. The defect is not in the walk. It is that the walk is handed the wrong substring — see Issues 1 and 2.

## Dead Code

None blocking. One near-dead branch under Notes.

## Correctness Dimensions

| Dimension | Status | Evidence |
|-----------|--------|----------|
| Concurrency | PASS | `within_time_cap` uses an `mpsc` channel with a moved owned `String`; no shared mutable state. `fonts()`/`no_fonts()` are `OnceLock`. The detached thread touches nothing the caller holds. (Its *memory* behaviour is Issue 3, a resource finding, not a data race.) |
| Error Handling | PASS | Every fallible path returns a typed `DecodeError`; no `unwrap`/`expect` outside tests; `read_source` retries `EINTR` and loops on short reads (`:234-241`). |
| Resources | **FAIL** | Detached worker keeps allocating after the caller is served — measured RSS 18.6 → 38.6 MiB over 5 s and still rising, against 332 MB of pending entity text, while `probe_dimensions` had already returned an error. `svg.rs:437-453`. |
| Boundaries | PASS | `prologue.as_bytes()[doctype..]` is safe: `doctype` comes from `find` on the same `&str`. Byte walk on ASCII delimiters cannot land mid-codepoint (continuation bytes are ≥ 0x80). Prefix sweep over `0..=len` panicked on nothing. `fit`/`dimensions` clamp with `.max(1)` and saturating `as u32`. |
| Security | **FAIL** | `refuse_internal_subset` is the module's stated sole defence against entity expansion (`svg.rs:22`, `:96`) and is bypassed by two families of legal XML — demonstrated end-to-end through `gfx::decode::probe_dimensions` with `Ok((7, 9))` on a 142-byte fixture and 85x expansion at scale. |

## Loaded-Skill Criteria

| Skill | Criterion | Status | Evidence |
|-------|-----------|--------|----------|
| cc-defensive-programming | External input validated at entry | **FAIL** | `parse` (`svg.rs:404`) is the barricade for untrusted document text. It admits two whole classes of internal-subset-bearing document. Demonstrated: `<!--<svg--><!DOCTYPE svg [ <!ENTITY a "hello"> ]>…` → `Ok((7, 9))` through `probe_dimensions`. |
| cc-defensive-programming | Barricade design — validation at the boundary, sound inside | **FAIL** | Everything downstream (`MAX_SVG_BYTES`'s "peak text stays at roughly the source's own size", `test_the_node_cap_is_reachable_within_the_byte_cap`'s "bytes of source are bytes of text") reasons *from* the barricade holding. With it bypassed, 3.9 MB of source yields 332 MB of text — 85x — so both derived arguments are void. |
| cc-defensive-programming | Defense in depth on a security-critical path | **FAIL** | The module doc names the guard the sole defence (`:22`). The only backstop, `within_time_cap`, bounds latency but not memory (Issue 3). A security-critical path is never exempt from a second check. |
| cc-defensive-programming | No empty catch blocks / swallowed errors | PASS | Every `map_err` produces a typed, message-bearing `DecodeError`; nothing is discarded. |
| cc-defensive-programming | Assertions for bugs only; no executable code in assertions | PASS | No assertions on production paths; all anticipated bad input is `Result`-typed. Assertions appear only in `#[cfg(test)]`. |
| cc-defensive-programming | Correctness over robustness on a hostile-input path | PASS | Where the walk does run, it fails closed — an unterminated literal is refused rather than accepted (`:170-172`, DW-1b.4). The direction of the trade is right. |

## Notes (non-blocking)

| # | Finding | Confidence | Severity |
|---|---|---|---|
| N1 | `find("<SVG")` fallback (`:145`) guards a root spelling that cannot render anyway: an uppercase `<SVG>` root is rejected downstream by usvg (`SVG data parsing failed cause the document does not have a root node`). The branch is close to dead, and it also creates the B5 asymmetry where an uppercase decoy is caught but a lowercase one is not. | High — executed | Low |
| N2 | With a namespace-prefixed root (`<ns0:svg>`), `find("<svg")` misses it and `prologue` becomes the whole source, so a `<!DOCTYPE` appearing in body text or a `<desc>` could trigger a spurious refusal. Inverse of the bypass; I did not construct a fixture. | Medium — reasoned, not executed | Low |
| N3 | The guard scans a *substring* of the source while the parser sees the whole document. Any fix that keeps a substring-based scan will keep some version of this class. A prologue tokenizer (skip `<?…?>` and `<!--…-->` properly, then require the next construct to be the DOCTYPE or the root) is the shape that closes both families at once. | High | — (design) |
| N4 | Suite artifact: `stele::document_source::test_dw_2_2_an_external_write_repaints_within_a_poll_interval_and_keeps_the_top_block` fails under the mandated long `TMPDIR` — the status ruler truncates the path to 78 chars, so two different documents' rulers compare equal (`assertion left != right failed`). Passes with a short TMPDIR. Pre-existing and unrelated to this phase; reported, not chased. | High — executed both ways | Low (test-env) |
| N5 | The tree contains two other reviewers' scratch integration tests (`crates/gfx/tests/zz_review_sample2_probe.rs`, `zz_review_sample3_probe.rs`) contributing 13 tests to the 826 count. They must not be committed. My own harnesses were deleted and `crates/gfx/src/svg.rs` was restored byte-identical (`diff` clean) after the pre-fix experiment. | High | Low (hygiene) |

## Issues (FAIL)

1. **`refuse_internal_subset` is bypassed by a `<svg` substring anywhere earlier in the prologue.**
   - File: `crates/gfx/src/svg.rs:145`
   - Demonstrated by: `probe_dimensions` on `<!--<svg--><!DOCTYPE svg [ <!ENTITY a "hello"> ]>\n<svg … width="7" height="9"><desc>&a;</desc></svg>` (142 B) → `Ok((7, 9))`. At scale: 98,712 B source → 8,388,608 B of entity text (85x), `Ok((10, 10))` in 165 ms. Control without the comment → refused. Variants `<!--<svgx-->`, `<?stele <svg ?>`, `<!-- <svg/> -->` all bypass.
   - Fix: do not anchor on a raw `find("<svg")`. Walk the prologue as XML — consume `<?…?>` and `<!--…-->` to their real terminators — and only then decide where the root begins. Failing that, scan the whole source for `<!DOCTYPE` rather than a truncated prefix.

2. **The first `<!DOCTYPE` shadows the real one; a decoy in a prologue comment or PI hides the subset.**
   - File: `crates/gfx/src/svg.rs:149`, with the `Ok` return at `:166`
   - Demonstrated by: `probe_dimensions` on `<!-- <!DOCTYPE svg> --><!DOCTYPE svg [ <!ENTITY a "hi"> ]>\n<svg … width="7" height="9"><desc>&a;</desc></svg>` (142 B) → `Ok((7, 9))`. At scale: 98,741 B → 8,388,608 B (85x), `Ok((10, 10))` in 169 ms. PI variant likewise. Both are well-formed XML per [22]/[27].
   - Fix: the guard must not stop at the first `<!DOCTYPE` match, and must not treat comment/PI interiors as scannable text. Same prologue tokenizer as Issue 1 resolves both.

3. **`within_time_cap` bounds latency but not memory, and its doc says otherwise.**
   - File: `crates/gfx/src/svg.rs:437-453`, doc claim at `:427-428`
   - Demonstrated by: 3,900,408 B source (inside `MAX_SVG_BYTES`) → 332,800,000 B of entity text. `probe_dimensions` returned `Malformed("svg: gave up after 250 ms — too complex to draw")` at 255 ms with RSS 18.6 MiB; RSS then climbed monotonically to 38.6 MiB over 5 s at ~4.4 MiB/s, still rising. The comment "The abandoned thread finishes and drops its result harmlessly" is false on this path.
   - Fix: primarily, close Issues 1 and 2 so entity expansion never reaches the parser. Separately, correct the comment — the cap is a latency bound, not a memory bound, and it is not a backstop for the entity class.

4. **Six doc-comment safety claims are falsified by the executed bypasses.** (The dispatch treats a false claim in the safety argument as a defect at the same severity as a code defect.)
   - `crates/gfx/src/svg.rs:22` — "[`refuse_internal_subset`] is what actually stops it." It does not stop Families A or B.
   - `crates/gfx/src/svg.rs:96-100` — "This is the whole defence against entity expansion … refusing that construct refuses the entire class." The construct is not reliably detected.
   - `crates/gfx/src/svg.rs:140-143` — "A `<!DOCTYPE` inside a comment in the prologue would be a false positive; that costs one unusual file a render, where a false negative costs the reader their terminal." Exactly backwards: fixture D1 shows a `<!DOCTYPE` in a prologue comment produces a false **negative**.
   - `crates/gfx/src/svg.rs:75-83` (`MAX_SVG_BYTES`) — "peak text stays at roughly the source's own size and this one number caps both." Measured 85x (3.9 MB source → 332 MB text).
   - `crates/gfx/src/svg.rs:754-757` (test doc) — "No expansion factor enters the arithmetic … bytes of source are bytes of text." Same falsification; the node/byte cap conjunction test rests on it.
   - `crates/gfx/src/svg.rs:427-428` — "The abandoned thread finishes and drops its result harmlessly." See Issue 3.
   - Fix: land the guard fix first, then re-derive each claim against the fixed guard rather than editing the prose to match.

**Verdict: FAIL** — blockers: (1) prologue-truncation bypass via a `<svg` substring; (2) first-`<!DOCTYPE` shadowing via a decoy in a comment or PI; (3) the only remaining backstop bounds time, not memory, and its doc claims otherwise; (4) six falsified safety claims in the doc comments.

All seven Done-When items pass, and the XML spec citations are correct. The phase fixed the quoted-`>` bypass it set out to fix and did not fix the class. The dispatch's central task — "construct inputs that get an internal subset past this guard and into the parser, verified through `gfx::decode::probe_dimensions`; a bypass is a FAIL" — is met twice over, on 142-byte fixtures.
