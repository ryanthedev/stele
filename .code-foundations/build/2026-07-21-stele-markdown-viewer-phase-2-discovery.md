# Discovery + Design: Phase 2 - CommonMark+GFM parser

## Files Found
- `Cargo.toml` (workspace: resolver 3, edition 2024, rust-version 1.95.0, member `crates/probe`) — needs `crates/ast` added as member per dispatch.
- `.github/workflows/ci.yml` — fmt / clippy `--workspace --all-targets --all-features -D warnings` / test `--workspace --all-features` / linkage / spike-artifacts. No workflow change needed for this phase (see DW-2.5 design note).
- `crates/probe/**` — Phase 1 output; no coupling to this phase.
- `crates/ast/` and `fixtures/` — **did not exist**; created in this phase.
- Vendored this session (with provenance): `crates/ast/tests/spec/` — see Test Infrastructure.

## Current State
Greenfield for this phase. Phase 1 delivered workspace + CI + probe harness; nothing parser-related exists. Toolchain is Homebrew rust 1.95.0 (no rustup). For DW-2.3, rustup + nightly + cargo-fuzz 0.13.2 were installed PATH-non-invasively (`--no-modify-path`; Homebrew cargo remains the default `cargo`; nightly is invoked via explicit `~/.cargo/bin` path only for fuzzing).

## Gaps
1. **Plan vs env:** cargo-fuzz/nightly absent → installed (above). No plan change needed.
2. **spec.json is not committed in the commonmark-spec repo** — it is generated from `spec.txt` by `test/spec_tests.py --dump-tests`. Vendored `spec.txt` @ tag 0.31.2 (sha256 `257c41ad…`) and generated `commonmark-0.31.2.json` locally (652 examples — matches DW-2.1's denominator). `spec_tests.py` imports a `cmark` module at top level, so the extraction logic is replicated in a vendored generator; both originals are kept for provenance.
3. **Scope tension resolved — raw HTML:** "raw HTML interpretation (preserved as literal text nodes)" is OUT, but ~75 spec tests (HTML blocks, Raw HTML) require *recognizing* HTML per spec rules. Resolution: the parser recognizes HTML blocks/inline HTML (recognition affects surrounding parse structure and is required for the 649 bar) and stores them as **uninterpreted literal nodes** (`HtmlBlock`/`HtmlInline` carrying raw text — no DOM, no attribute model). The HTML shim emits them verbatim (what the spec expects); terminal rendering phases will show them as literal text. Interpretation stays out of scope.
4. **GFM extension suite:** cmark-gfm's spec.txt (tag 0.29.0.gfm.13, sha256 `7d8e5814…`) vendored; the four in-scope extension sections extracted to `gfm-extensions.json`: Tables 8, Autolinks 11, Task list items 2, Strikethrough 2 = 23 cases. Disallowed-raw-HTML extension excluded (out of scope with raw-HTML interpretation).
5. **Unicode data with zero runtime deps:** the 0.31.2 flanking rules need P*/S* general categories, Zs, and link-label matching needs *full* case folding (spec example: `[ẞ]` matches `[SS]`; `str::to_lowercase` is insufficient). Generated static tables (`tables.rs`: 349 P∪S ranges, 7 Zs ranges, 1557 fold entries; `entities.rs`: 2125 semicolon-form HTML5 entities) via vendored `gen_tables.py` from python3 unicodedata (UCD 16.0.0, version recorded in file headers) + `entities.json` (html.spec.whatwg.org, sha256 `d741d877…`).

## Code Standards
No `docs/code-standards.md` found. Following workspace conventions observed in `crates/probe`: edition/license/rust-version inherited via `.workspace = true`, `thiserror`-style module docs, clippy `-D warnings` clean, rustfmt clean.

## Test Infrastructure
- Workspace tests run via `cargo test --workspace` (CI `test` job); integration tests live in `crates/<name>/tests/`.
- Vendored under `crates/ast/tests/spec/`: `commonmark-spec.txt` (0.31.2), `commonmark-0.31.2.json` (652 cases), `gfm-spec.txt` (0.29.0.gfm.13), `gfm-extensions.json` (23 cases), `entities.json`, generators (`gen_tables.py`), originals (`spec_tests.py`, `normalize.py`), `README.md` with URLs/tags/sha256s.
- Conformance harness: `tests/conformance.rs` parses the JSON with `serde_json` (dev-dependency only — runtime deps remain zero).
- Fuzzing: `crates/ast/fuzz/` (cargo-fuzz layout, excluded from the workspace) — target feeds arbitrary bytes to `Document::parse` via lossy UTF-8. libFuzzer flags carry the DW-2.3 gates: `-timeout=1` (>1s input), `-rss_limit_mb=2048` (OOM), crash = panic.

## DW Verification

| DW-ID | Done-When Item | Status | Test Cases |
|-------|---------------|--------|------------|
| DW-2.1 | ≥649/652 CommonMark spec tests pass; every deviation documented with rationale | COVERED | `test_dw_2_1_commonmark_conformance` — runs all 652, asserts pass ≥ 649 AND failing set ⊆ documented-deviation list in `crates/ast/CONFORMANCE.md` (cap 3) |
| DW-2.2 | GFM extension suites (tables, strikethrough, task lists, autolinks) pass | COVERED | `test_dw_2_2_gfm_tables` / `_strikethrough` / `_task_lists` / `_autolinks` — 23 vendored cases, all must pass |
| DW-2.3 | 1h cargo-fuzz: no panic, no OOM, no >1s parse | COVERED (evidence = real run + CI proxy) | Real 1h background fuzz run with `-timeout=1 -rss_limit_mb=2048`, actual wall time + stats reported honestly; deterministic CI proxy `test_dw_2_3_pathological_inputs_stay_fast` (ported cmark pathological cases under a time budget) |
| DW-2.4 | Spans reconstruct the exact source slice for every node in the fixture corpus | COVERED | `test_dw_2_4_spans_reconstruct_source` — walks every node of every `fixtures/*.md`: span in-bounds, char-boundary-valid, child ⊆ parent, ordered non-overlapping siblings, literal-leaf slice equality |
| DW-2.5 | `forbid(unsafe_code)` holds crate-wide (CI-checked) | COVERED | `#![forbid(unsafe_code)]` in `lib.rs` (compile-enforced crate-wide) + `test_dw_2_5_forbid_unsafe_present` asserts the attribute exists in `lib.rs`; both run in the CI `test`/`clippy` jobs — no workflow edit needed (workflow is outside this phase's file scope) |

**All items COVERED:** YES

### Assumption verification (from dispatch)
"Solo parser reaches ≥649/652 conformance (HIGH)" — no counter-evidence found. The spec's appendix specifies the exact two-phase algorithm the reference implementation uses; the plan's continuous-verification instruction applies: the conformance number is measured every run, and if the bar can't be cleared the phase returns UPDATE_PLAN with the real number and named failing cases. Proceeding.

## Design Decisions

### Design: `crates/ast` public surface (aposd-designing-deep-modules)

**Approaches Considered**
1. **A — Event/pull parser** (pulldown-cmark style): lazy event stream, tree assembled on demand. Memory-lean, but spans/NodeId/tree-retention (the *product* need — P4 walks a retained tree) must be bolted on top, and late-bound link reference definitions force awkward lookahead.
2. **B — Spec-strategy two-phase tree builder** (cmark reference style): phase 1 line-by-line block structure via an open-block stack; phase 2 inline parsing per leaf with a delimiter stack. Produces the retained tree directly; every conformance corner case maps 1:1 to a spec rule; link refs collected in phase 1, consumed in phase 2.
3. **C — Arena AST** (comrak style): all nodes in one `Vec`, `NodeId` = index, children as id lists. O(1) NodeId resolve, but every downstream consumer (P4's layout walk is the main mode of use) pays arena-lookup noise on every pattern match.

**Comparison**

| Criterion | A | B | C |
|-----------|---|---|---|
| Interface simplicity | poor (events + tree adapter) | good (one entry fn, owned tree) | good |
| Information hiding | leaks streaming model | full (algorithm invisible) | leaks arena to all consumers |
| Caller ease of use (P4 tree walk) | poor | best (nested enums, direct match) | mediocre |
| Conformance risk (649 bar) | high | lowest (spec's own algorithm) | low |
| NodeId resolve cost | n/a | O(depth) via path index | O(1) |

**Choice: B**, with a NodeId **path index** side-table: after parse, a numbering pass assigns pre-order `NodeId`s and records each node's child-index path; `Document::node(NodeId) -> Option<NodeRef>` resolves by path walk. Rationale: P4/P6 resolve NodeIds for the *few* Reserved nodes (images/math) — resolve is cold; the tree walk is hot and gets the best shape. Sacrificed: O(1) resolve (irrelevant at this call frequency).

**Node shape:** `struct Block { id: NodeId, span: Span, kind: BlockKind }` (same for `Inline`), with `pub enum BlockKind` / `InlineKind` holding the typed variants. Recorded nuance vs the plan's "typed `Block`/`Inline` enums": span+id live once on a wrapper struct instead of being repeated in ~30 enum variants — the pinned contract items (`Document::parse(&str) -> Document` infallible, `Span { start: usize, end: usize }` on every node, stable `NodeId` + resolution) are all held verbatim; the enum-vs-wrapper shape is implementation latitude and the *better* information-hiding shape (span/id access needs no 30-arm match). Not a seam redesign.

**Depth check:** public surface = `Document::parse`, `Document::blocks`, `Document::source`, `Document::node(NodeId)`, node types, `html::to_html` (shim, documented as conformance-harness-only). Hidden: the entire two-phase algorithm, delimiter stack, tab logic, entity/Unicode tables, depth caps. Common case (`Document::parse(src)` then walk) needs zero knowledge of internals.

### Parser internals (cc-pseudocode-programming — phase-level pseudocode; per-routine pseudocode written as header comments during implementation)

```
parse(src):
  block phase (iterative, one pass over lines):
    for each line (tracking byte offsets incl. CRLF, tab-stop column state):
      match open container blocks (blockquote '>', list-item indent)
      check lazy-continuation for paragraphs
      try to open new blocks (ATX, setext, thematic break, fenced/indented code,
        html block start conditions 1-7, list item, blockquote, GFM table,
        footnote definition, alert re-tag of blockquote, frontmatter at offset 0)
      add remaining text to the open leaf (paragraph/code/html), recording spans
    on close of a paragraph: extract link reference definitions from its head
  inline phase (per leaf paragraph/heading/table-cell text):
    single left-to-right scan producing flat inline items + delimiter stack
      (backticks first-match code spans, autolinks/html, entities, backslash,
       math $/$$ with code-span-like matching, GFM autolink literals in text)
    process brackets: links/images vs collected refdefs (full-casefold labels,
      openers deactivated after use -> linear), footnote refs
    process_emphasis with openers_bottom optimization (linear on pathological
      delimiter runs) covering * _ and GFM ~~
  numbering pass (iterative, explicit stack): assign pre-order NodeIds,
    build path index
```

### Hardening (cc-defensive-programming)

- **Barricade:** `Document::parse` is the trust boundary — total function, no panics on any input (fuzz-verified). Inside the crate, invariants use debug assertions; no `unwrap` on input-derived values in release paths.
- **NUL:** U+0000 → U+FFFD per spec §2.3 (content only; spans still address original source bytes).
- **Depth cap:** container nesting capped at 200 (marker beyond cap degrades to literal text — graceful, still-correct output; spec tests nest < 20). Internal tree walks (numbering, HTML shim, span checks) are iterative with explicit stacks, so no stack overflow regardless of inline-emphasis nesting depth.
- **Linear-time guarantees:** each source line consumed once in the block phase; `openers_bottom` in emphasis processing; bracket openers deactivated after first match attempt; code-span backtick openers matched by first-fit per run length. Ported cmark pathological inputs enforce this in `test_dw_2_3_pathological_inputs_stay_fast`.
- **Allocation bound:** all structures O(len(input)); refdef map and delimiter stack bounded by input size; fuzz `-rss_limit_mb=2048` enforces.
- **CRLF:** line splitting handles `\r\n`/`\r`/`\n`; spans keep original byte offsets.

### Other decisions
- **Extension AST nodes:** `Math { display: bool }` inline (`$…$`/`$$…$$`, code-span-like no-blank-line matching), `FootnoteDefinition`/`FootnoteReference`, `Alert { kind }` (blockquote whose first line is `[!NOTE|TIP|IMPORTANT|WARNING|CAUTION]`), `FrontMatter` (YAML `---` fence at byte 0 only, content kept raw), `CodeBlock { info }` for fence info strings, GFM `Table`/`Strikethrough`/`TaskItem { checked }`/autolink.
- **HTML shim** (`html.rs`): matches spec expected output exactly (entity escaping, cmark URL percent-encoding for href/src, tight/loose list `<p>` rules, GFM table HTML). `#[doc]`-noted as conformance-harness-only, not a product surface.
- **Deviation ledger:** `crates/ast/CONFORMANCE.md` lists any failing example numbers with rationale; the DW-2.1 test enforces ledger ⊆ reality and cap ≤ 3.
- **Fixtures:** `fixtures/*.md` — one file per feature cluster (headings/paragraphs, emphasis, links+refdefs, lists, blockquotes+laziness, code, html-literal, tables, task lists, strikethrough+autolinks, math, footnotes, alerts, frontmatter, unicode+entities, crlf, pathological-lite) so P4/P7 goldens can extend per-file.

## Prerequisites
- [x] Phase 1 complete (workspace, CI) — commit efd2633
- [x] Spec assets vendored with provenance (this session)
- [x] nightly + cargo-fuzz available for DW-2.3
- [x] No missing prerequisites

## Recommendation
BUILD — implement `crates/ast` per the chosen design: stub public surface → block phase → inline phase → HTML shim → conformance loop to ≥649 → GFM + extensions → fixtures + span test → 1h fuzz (background) → deviation ledger.

---

## Implementation Addendum (post-discovery decisions)

### `ParseOptions` — recorded interface addendum, not a seam change
Implementation proved that **always-on extensions cannot clear DW-2.1's bar**: 5 CommonMark examples (96, 98 frontmatter; 608, 611, 612 GFM autolink literals) contradict extension behavior *by design* — the extensions change the language. The deviation cap is 3, so "document them" was not available. Resolution, mirroring every reference implementation (cmark-gfm, comrak): runtime extension toggles.
- `Document::parse(&str) -> Document` — **unchanged, pinned contract**; enables everything (product config).
- `Document::parse_with(&str, &ParseOptions)` — additive; `ParseOptions::commonmark()` turns all extensions off.
- The conformance harness measures the CommonMark suite in the commonmark profile and the GFM suites in the default profile; both run the same parser code. Result: **652/652 CommonMark (zero deviations), 23/23 GFM.**

### Node-shape note
As designed: `Block`/`Inline` are span+id-carrying node structs wrapping typed `BlockKind`/`InlineKind` enums. All pinned items hold verbatim (`Document::parse`, `Span { start, end }` on every node, stable dense pre-order `NodeId` with `Document::node()` resolution + `Document::nodes()` iterative walk).

### Defects found by validation (fixed + regression-tested)
1. **Fuzz crash (83s in):** `www.` prefix check byte-sliced mid-char after NUL→U+FFFD replacement → panic. Fixed with byte-level compare; regression test `test_fuzz_regression_www_prefix_char_boundary`. Audit of the sibling paths hardened scheme/email autolink lookback against entity-decoded pending-buffer misalignment.
2. **Span/source mismatch:** `Document.source` stored the raw input while spans indexed the NUL-cleaned text; `source()` is now the cleaned text (documented), keeping every span valid.
3. **Hard-break span overlap:** trailing-space trimming left the Text span covering the break's spaces; fixed with an explicit flush end (caught by the DW-2.4 invariant test).

### Depth-cap design (constraint: recursion depth cap)
Block containers capped at 200 at open time; bracket stack capped at 200; total tree depth capped at `MAX_TREE_DEPTH = 700` by an iterative flattening pass during id-numbering. All internal walks (numbering, HTML shim, `nodes()`) are explicit-stack iterative. Public guarantee documented: recursive consumers of the tree are safe.
