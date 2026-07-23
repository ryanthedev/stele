# Discovery + Design: Phase 4 - Layout engine

## Files Found
- `crates/ast/src/lib.rs`, `crates/ast/src/ast.rs` — public AST surface: `Document::parse(&str) -> Document` (infallible), `Document::blocks()`, `Document::node(NodeId) -> Option<NodeRef>`, `Document::nodes()`, `NodeId` (dense pre-order, `index()`), `Span`, typed `Block`/`BlockKind`, `Inline`/`InlineKind`, `ListKind`, `Alignment`, `AlertKind`, `NodeRef` with `child()`/`children()`. `MAX_TREE_DEPTH = 700` and the doc comment explicitly guarantees recursive consumers are safe.
- `crates/width/src/engine.rs` — `WidthEngine::new(WidthConfig { ambiguous_wide })`, `cluster_width(&str) -> u16`, `display_width(&str) -> usize`, free fn `graphemes(&str)`. Pure, total.
- `fixtures/*.md` — 18 fixture files (headings, lists, tables, code, math, alerts, footnotes, pathological-lite, …). Consumed by `crates/ast/tests/spans.rs` via a `*.md` glob (invariant checks only — new `.md` files are safe to add and get span-checked for free).
- `Cargo.toml` — workspace members `crates/{ast,probe,width}`; edition 2024, rust 1.95, release profile `strip+lto+codegen-units=1`.
- `crates/layout` — does NOT exist yet (expected; this phase creates it).

## Current State
Phases 1–3 complete. The AST and width crates expose exactly the seams the plan pins; `crates/ast` has zero runtime deps, `crates/width` depends on unicode-segmentation/width/properties. No layout code exists anywhere. No `fixtures/layout/` goldens exist.

## Gaps
- Plan says `Line = Vec<Run…> | Reserved{…}` but does not say how a multi-row reserved box maps onto the flat line sequence that `lines(Range<usize>)` (P5 scroll math) addresses. Resolved by design decision D4 below (one `Line` per terminal row; a `rows = R` box emits R consecutive `Line::Reserved` entries carrying the same struct) — this stays within the pinned shapes, no plan update needed.
- Plan doesn't state how nested inline styles (strong inside heading) collapse into a single `StyleId` per run. Resolved by D5 (innermost-role-wins over a style stack). Semantic carries enough (heading level, emph, strong, code, link, …) for P5's structural styling; theme composition is P7's problem.
- `ast::AlertKind` doesn't derive `Hash`, so it can't be embedded in a `Hash`-derived `Semantic`. Resolved: `Semantic::AlertTitle(AlertTone)` with a local 5-variant mirror enum.
- No `docs/code-standards.md` (see below).

## Code Standards
No `docs/code-standards.md` found. Followed the de-facto conventions read from `crates/ast` and `crates/width`: `#![forbid(unsafe_code)]`, module-level `//!` docs explaining the seam and what is hidden, plan-pinned signatures called out in doc comments, zero/minimal runtime deps, DW-tagged integration tests under `tests/`, edition/license/rust-version inherited from the workspace.

## Test Infrastructure
Plain `cargo test` integration tests in `crates/*/tests/*.rs`; DW-named tests (`test_dw_…`) plus behavior tests past the floor (pattern set by `crates/ast/tests/api.rs`, `spans.rs`). Fixture corpus loaded via `CARGO_MANIFEST_DIR/../../fixtures` glob with sorted paths. Golden regeneration will use an env var (`UPDATE_LAYOUT_GOLDENS=1`), goldens stored under `fixtures/layout/` as `.txt` (invisible to the ast `*.md` glob).

## DW Verification

| DW-ID | Done-When Item | Status | Test Cases |
|-------|---------------|--------|------------|
| DW-4.1 | Deterministic — identical AST + width + sizer → structurally identical tree (hash-compared, two independent layout calls) | COVERED | `test_dw_4_1_deterministic_hash_over_fixtures` — every fixture, two independent `layout()` calls, `DefaultHasher` hash equality + `PartialEq` equality |
| DW-4.2 | Golden snapshots pass at width 24 AND width 100 for the fixture corpus | COVERED | `test_dw_4_2_goldens_width_24`, `test_dw_4_2_goldens_width_100` — rendered-line snapshots per fixture under `fixtures/layout/` |
| DW-4.3 | No fixture table (including a pathological one) exceeds the width bound after the overflow ladder | COVERED | `test_dw_4_3_no_table_exceeds_width_bound` — all fixtures (incl. new `fixtures/pathological-table.md`) at widths 24, 40, 100: every line's measured width ≤ clamped width |
| DW-4.4 | `layout(ast, w2, …)` equals fresh `parse`+`layout` at w2 for all fixtures | COVERED | `test_dw_4_4_reflow_equals_fresh_parse_layout` — layout doc at 100 then at 24; compare to fresh parse + layout at 24 (equality + hash) |
| DW-4.5 | 1 MB document lays out in <100 ms in release; measured, real number reported | COVERED | `test_dw_4_5_one_megabyte_under_100ms` — synthetic ≥1 MB mixed document, layout-only wall clock; asserts <100 ms under `cfg(not(debug_assertions))`; run via `cargo test --release`, number reported in build output |

**All items COVERED:** YES

Edge-case tests (each named edge case from the phase spec):
- table overflow ladder: `test_ladder_rung1_wrap_in_cell`, `test_ladder_rung2_per_column_floors`, `test_ladder_rung3_clip_with_indicator`
- unbreakable long word: `test_unbreakable_word_break_anywhere_fallback`
- code block wider than viewport: `test_code_block_clips_never_wraps`
- nested-list indent consuming budget: `test_nested_list_indent_clamped_leaves_content_room`
- image wider than viewport: `test_image_scaled_to_fit_preserving_aspect` (stub sizer)
- empty document: `test_empty_document_yields_empty_tree`
Plus beyond-DW behavior tests: NullSizer alt-text fallback, math text fallback, multi-row Reserved emission, `lines()` range clamping, width clamp to `LayoutConfig`, tight vs loose lists, ordered-list start numbering, table alignment, hard/soft breaks, blockquote nesting.

## Design Decisions

### Design: crates/layout module architecture (aposd-designing-deep-modules)

#### Approaches Considered
1. **A — Single-pass tree walker.** One recursive walk over `Document::blocks()` with a layout context (indent prefix stack, content width, style stack) appending `Line`s directly to the tree. Tables handled by a local measure-then-distribute step inside the walker.
2. **B — Two-stage box tree.** First pass builds an intermediate box tree (every block annotated with intrinsic min/max widths), second pass linearizes boxes into lines. Mirrors browser layout engines.
3. **C — Per-kind layout objects.** A `BlockLayout` trait with one impl per `BlockKind`, dispatched dynamically; shared context object threaded through.

#### Comparison
| Criterion | A | B | C |
|-----------|---|---|---|
| Interface simplicity | one public fn, zero internal types leak | same public fn, but a whole intermediate IR to maintain | trait + N impls + context type |
| Information hiding | high — wrapping/tables/prefixes all private | medium — box IR is knowledge duplicated from the AST | low — context struct becomes a shared-knowledge bus |
| Caller ease of use | identical (pinned signature) for all three | identical | identical |
| Fit to problem | good: only *tables* need measure-before-layout, and that is local to one subtree | pays the two-pass cost globally for a need that is table-local | classitis; execution-order structure (temporal decomposition) |
| DW-4.5 perf | single pass, O(n) | two full passes + IR allocation | dyn dispatch per block, extra indirection |

#### Choice: A
Rationale: the plan's pinned surface is already maximally deep — one function hides everything. B generalizes a table-only need (intrinsic pre-measurement) into a global IR nobody else consumes; APOSD says push that specialization *down* into the table module instead. C is the shallow-modules red flag. Sacrifice: if P6+ ever needs partial relayout, A has no incremental structure — acceptable, the plan explicitly specs full re-layout on resize.

#### Depth Check
- Interface methods: 1 free fn (`layout`), 1 trait method (`IntrinsicSizer::size`), 3 small accessors on `LayoutTree` (`lines`, `line_count`, `is_empty`). Everything else is plain data.
- Hidden details: greedy wrap algorithm, atom flattening, style stack, prefix composition, table min/max measurement, §3.9.3 distribution, overflow-ladder thresholds, break-anywhere fallback, image scaling math.
- Common case complexity: simple — `layout(&doc, w, &LayoutConfig::default(), &engine, &NullSizer)`.

### Concrete decisions (D1–D12)
- **D1 — Public surface** (pinned by plan, matched exactly): `layout(&Document, width: u16, &LayoutConfig, &WidthEngine, &dyn IntrinsicSizer) -> LayoutTree`; `trait IntrinsicSizer { fn size(&self, node: NodeId, doc: &Document) -> Option<CellSize> }` + `NullSizer`; `LayoutTree::lines(Range<usize>) -> impl Iterator<Item = &Line>`; `Run { text, style_id: StyleId, width }`; `Reserved { node_id: NodeId, cols, rows }`; `enum StyleId { Semantic(Semantic), Capture(u16) }`; `CellSize { cols: u16, rows: u16 }`; `LayoutConfig { min_width: 24, max_width: 100 }` via `Default`.
- **D2 — `Line` shape**: `enum Line { Runs(Vec<Run>), Reserved(Reserved) }` — exactly "runs or a Reserved" as pinned.
- **D3 — Internal modules**: `lib.rs` (public types + `layout`), `inline.rs` (inline flattening + greedy wrap), `block.rs` (block walker, prefixes, lists, quotes, code, headings, rules), `table.rs` (measure, distribute, ladder). All private except the pinned surface.
- **D4 — Multi-row reserved boxes**: one `Line` = one terminal row (scroll addressing invariant for P5). A reserved box with `rows = R` emits R consecutive `Line::Reserved` lines carrying the identical `Reserved` value; consumers find the box top by scanning back over equal `node_id`. Documented on the type.
- **D5 — Style resolution**: a style stack during inline flattening; a run's `Semantic` is the innermost applicable role (Code/Link/Math beat Emph/Strong beat Heading base). Layout emits ONLY `StyleId::Semantic`; `Capture(u16)` is declared but never constructed here (P7's range).
- **D6 — `Semantic` roles**: `Text`, `Heading(u8)`, `Emph`, `Strong`, `Strikethrough`, `CodeInline`, `CodeBlock`, `Link`, `ImageAlt`, `MathTex`, `ListMarker`, `TaskMarker`, `BlockquoteMarker`, `AlertTitle(AlertTone)` (local mirror of `ast::AlertKind`, which lacks `Hash`), `Rule`, `TableBorder`, `TableHeader`, `FootnoteRef`, `FootnoteLabel`, `Html`, `FrontMatter`, `OverflowIndicator`.
- **D7 — Wrapping**: inline tree flattens to styled segments split at whitespace; adjacent non-space segments glue into one unbreakable word group (style changes are not break points). Greedy fill; SoftBreak ≡ space, HardBreak forces a new line. A word group wider than the content width takes the break-anywhere fallback at grapheme-cluster boundaries. Every measurement goes through `WidthEngine::cluster_width`/`display_width` — never `.len()`/`.chars()`.
- **D8 — Decorations** (all measured through the engine; several are EAW-Ambiguous, which is exactly why): blockquote/alert prefix `"│ "`; bullets `"• "` (all levels), ordered `"N. "`/`"N) "` honoring `ListKind::Ordered { start, delim }`; task items `"[x] "`/`"[ ] "`; thematic break = `"─"` repeated to content width; clip indicator `"…"`. Continuation lines indent by the marker's measured width. Blocks are separated by one blank line (`Line::Runs(vec![])`); tight-list items are not.
- **D9 — Nested-indent clamp**: effective indent is capped so at least `min(8, width)` content columns always remain; total line width never exceeds the clamped width. (Threshold is tunable per plan; the invariant — never exceed width, never reach zero content columns — is the contract.)
- **D10 — Tables**: per-column min-content (widest unbreakable word group) and max-content (widest hard-line) widths measured through the engine; separator `" │ "` between cells, no outer border, header rule `"─…─┼─…"`. Distribution per css-tables-3 §3.9.3 (no external dependency — comfy-table consulted as reference only): fits-at-max → max widths; else min + remaining space distributed proportionally to (max−min), deterministic left-to-right remainder. Overflow ladder in the contract order: (1) **wrap-in-cell** — cells wrap at assigned widths ≥ min-content; (2) **per-column floors** — columns squeezed below min-content down to a floor (3 cols), cell text break-anywhere; (3) **clip with indicator** — if floors + separators still exceed the width, each composed table line is clipped to width with a trailing `"…"` `OverflowIndicator` run. Cell alignment honors `Alignment`.
- **D11 — Sizer integration**: `Image` and `Math` nodes call `sizer.size(node.id, doc)` with the AST's own `NodeId` (P6 resolves path/tex through `Document::node` with the same id). `Some(CellSize)` → Reserved lines, scaled to fit: `cols > content_width` ⇒ `cols' = content_width`, `rows' = max(1, ceil(rows·cols'/cols))` (aspect preserved). `None` (NullSizer) → alt-text/TeX-source text runs (`ImageAlt`/`MathTex`) wrapped normally — P4 is complete and testable without P6 (assumption VERIFIED, see below).
- **D12 — Determinism & purity**: no I/O, no clock, no globals, no `HashMap`/`HashSet` anywhere in layout (Vec-only); `#![forbid(unsafe_code)]`; all public data types derive `Debug/Clone/PartialEq/Eq/Hash`. `lines(range)` clamps the range to bounds instead of panicking. `HtmlBlock`/`HtmlInline` render as literal text (`Html` role); `FrontMatter` lays out as a literal block (`FrontMatter` role) — P5+ decides visibility.

### Assumption Verification (from dispatch)
**IntrinsicSizer + NullSizer lets P4 complete without P6 — VERIFIED.** `NullSizer::size` returns `None` for every node; the only callers of the sizer (Image, Math) have a total fallback (alt-text children / raw TeX) that wraps as ordinary styled text. No code path requires a `Some` sizer; the `Some` path is exercised in tests via a local stub sizer. No plan update needed.

## Prerequisites
- [x] `crates/ast` public API present and matches the pinned P2 contract
- [x] `crates/width` public API present and matches the pinned P3 contract
- [x] Fixture corpus present (18 files); adding `.md` fixtures is safe (ast span test only checks invariants)
- [x] Workspace ready for a new member (`crates/layout` to be added to `members`)

## Recommendation
**BUILD.** Create `crates/layout` (new workspace member) with the pinned public surface; author `fixtures/pathological-table.md`; add layout goldens under `fixtures/layout/`; DW + edge-case + behavior tests as mapped above; release-mode perf measurement for DW-4.5.
