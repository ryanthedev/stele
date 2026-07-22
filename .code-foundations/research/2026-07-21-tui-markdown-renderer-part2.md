# TUI Markdown Renderer — Build Requirements, Part 2 (D4, D5, D6, D7, D2b)

**Summary.** No shipped Rust markdown parser exposes a resumable parse API, and ratatui gives you neither a document layout tree nor content-based table sizing — but comrak is internally line-incremental and forkable, tree-sitter-markdown is genuinely incremental today, terminal table auto-layout is already solved by comfy-table, and pure-Rust Mermaid rendering exists — while **D7 (the crate-by-crate stack verification) produced zero verified findings and remains entirely unanswered**. D2b also produced nothing in the run, but was closed afterwards against Ghostty's own documentation: kitty graphics protocol support is confirmed.

- **Date:** 2026-07-21
- **Status:** draft
- **Scope:** part 2 of two. Part 1 (`2026-07-20-tui-markdown-renderer.md`) covers D1 prior art, D2 terminal graphics protocols, D3 text measurement; its findings are locked and are not restated or revised here. This document is to be merged manually.

## What remains open

| # | Open item | Why it matters | Source |
|---|---|---|---|
| 1 | ~~**D2b is unanswered.**~~ **RESOLVED 2026-07-21** outside the run: Ghostty's own docs confirm kitty graphics protocol support. Residual gap is *which subset* of the protocol is implemented. | No longer a single point of failure for the image feature; downgraded to a scoping question about protocol coverage. | [ghostty.org/docs/features](https://ghostty.org/docs/features) |
| 2 | **D7 is unanswered.** No verified claim names a crate, version, or maintenance status for kitty-protocol emission, image decode/pre-scale, unicode width + grapheme segmentation, syntax highlighting, or the cell-grid differ. | The brief's build-readiness verdict for these five subsystems cannot be produced from this pass without guessing. | run `openQuestions[1]`; finding 19 |
| 3 | **No named stable-prefix technique was found.** The pass surfaced the *mechanisms* (pulldown-cmark `into_offset_iter` byte ranges, tree-sitter edit-then-reparse) but no named approach from editors or LSP servers, and no evidence on how coarse a re-parse boundary must be to stay correct. | This is the hard core of the project (D4) and it is the part with the least external precedent. | run `openQuestions[2]`; finding 1 |
| 4 | **Neither incremental path was benchmarked.** Fork comrak to add `feed`/`finish`, or adopt tree-sitter-markdown's split block/inline grammar — no measurement exists of either, nor of whether incremental reparse even beats a naive full re-parse at realistic document sizes on a per-chunk cadence. | Decides the core architecture. | run `openQuestions[3]` |
| 5 | **Falsification claim 1 (nobody does streaming markdown → terminal) is unresolved**, and claim 5 (wcwidth correction tables) is unresolvable from this evidence set. | See the falsification table. | finding 18 |

### Method caveat that qualifies every finding below

Every verifier reported the session's WebSearch budget exhausted (200/200) **before the adversarial counter-evidence step could run**. All 39 surviving claims were verified by direct primary-source inspection — raw crate source, docs.rs, W3C spec text, the crates.io API, GitHub code and issue APIs — with no open-web sweep for contradicting commentary. For API-surface and source-code claims that is the stronger evidence class; for "does anyone already do X" questions (falsification claim 1 especially) it is a real blind spot, because absence of search is not absence of evidence.

Run stats: 5 angles, 29 sources fetched, 145 claims extracted, **40 verified** (3 verifiers each), 39 confirmed, 1 failed verification, 20 findings after synthesis. The 105 unverified claims were never selected for the verify phase; none of the 40 that were selected came from the D7 or D2b angles.

The brief's two tooling traps recurred and were caught: two docs.rs page summaries fabricated content about markdown-rs (one asserted "all fields default to true" when the `Default` impl sets 12 false; another returned a nonexistent `to_mdast` signature and a wrong release date), and a WebFetch summarizer misreported the CSS 2.2 and css-tables-3 spec text. Any claim below not backed by raw source or spec text should be treated as suspect; nearly all are so backed.

---

## D4 — Incremental parse and streaming render

**Confidence on all findings in this section: high.**

### Parser API surface

No shipped Rust markdown parser exposes a resumable, feed-more-input, or restartable parse API. pulldown-cmark, comrak, and markdown-rs each take the complete document as a single borrowed `&str` and return a fresh owned result, so adding a chunk requires re-parsing the whole grown prefix.

- pulldown-cmark: every constructor (`new`, `new_ext`, `new_with_broken_link_callback`, plus a fourth `new_with_callbacks` on unreleased master) is `(text: &'input str, …) -> Self`. The only non-constructor methods are `reference_definitions` and `into_offset_iter`. No method accepts input or returns saved state.
- comrak: `pub fn parse_document<'a>(arena, md: &str, options) -> Node<'a>` is the sole public parse entry — `grep '^\s*pub fn'` over all 2951 lines of `src/parser/mod.rs` returns only it plus a `pub(crate) fn consume`; `grep 'feed'` returns zero.
- markdown-rs 1.0.0: `grep '^pub '` over `src/lib.rs` yields exactly three `pub fn` (`to_html`, `to_html_with_options`, `to_mdast`), all `&str`-in / owned-out; greps for stream/increment/resum/partial/feed return zero. Only `to_mdast` is usable by a TUI, and `mdast::Node` has no lifetime parameter — the whole tree is reallocated per parse with no patch path.

Sources: <https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.Parser.html>, <https://github.com/kivikakk/comrak/blob/main/src/parser/mod.rs>, <https://docs.rs/markdown/latest/markdown/>, <https://docs.rs/markdown/latest/src/markdown/lib.rs.html>

### The prefix primitive that does exist

pulldown-cmark's laziness is output-side only — `Parser` implements `Iterator<Item = Event>` and `FusedIterator`, so events pull one at a time, but over an already-complete input buffer, not an input stream. `into_offset_iter(self) -> OffsetIter<'input, F>` produces `(Event, Range<usize>)` pairs mapping each event to its byte range in the markdown source. That is the exact primitive a stable-prefix / re-parse-from-offset strategy needs.

**Explicit caveat carried from the verifier:** *using* those offsets as re-parse boundaries is engineering inference, not documented practice, and the precision and stability of container-block Start/End ranges as re-parse boundaries was not verified.

Sources: <https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.Parser.html>, <https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.OffsetIter.html>

### comrak is forkable into a streaming parser

comrak's parser is internally line-incremental and holds all cross-line state in struct fields, so a `feed`/`finish` API is reachable via a fork or an upstream PR rather than a rewrite.

`fn parse(mut self, mut s: &str)` is a `while ix < end` loop using `jetscii::bytes!(b'\r', b'\n')` to find EOLs, calling `self.process_line(&s[ix..eol])`, then `finalize_document()` / `postprocess_text_nodes()`. All resumable state lives on `pub struct Parser<'a,'o,'c>` (refmap, root, current, line_number, offset, column, first_nonspace, indent, blank, curline_len, last_line_length, total_size); `process_line` resets only per-line scratch. `s`'s lifetime is unrelated to the arena's `'a` — content is copied into owned `String`s, so chunk-at-a-time feeding is viable. Independent corroboration: comrak's design mirrors cmark-gfm, and upstream cmark ships exactly `cmark_parser_feed(parser, buffer, len)` / `cmark_parser_finish(parser)` on the same block-parser architecture.

Real work remains, and it is bounded rather than a rewrite: `parse` is private and takes `self` by value, and would need `&mut self` plus a partial-line buffer (cmark keeps a `linebuf`) and an accumulated `total_size`. Separately, the `Parser` struct is unreachable today regardless of its `pub` marker — `lib.rs` declares `mod parser;` (private) and re-exports only `options`, `Options`, `ResolvedReference`, `parse_document`.

Sources: <https://github.com/kivikakk/comrak/blob/main/src/parser/mod.rs>, <https://raw.githubusercontent.com/commonmark/cmark/master/src/cmark.h>

### tree-sitter-markdown is genuinely incremental

It is the one truly incremental option in the candidate set. Its external scanner implements symmetric serialize/deserialize covering the full block state including the open-blocks stack — precisely the precondition tree-sitter's runtime gates subtree reuse on.

`scanner.c` on the `split_parser` branch (the repo's **default** branch, last commit 2026-07-19; `main` is a 2022 stub) serializes `state`, `matched`, `indentation`, `column`, `fenced_code_block_delimiter_length` and `memcpy`s the entire open_blocks stack, with `deserialize` restoring all of it, both wired to the tree-sitter ABI entry points. The runtime enforces the precondition at `lib/src/parser.c:783` — subtree reuse is gated on `ts_subtree_external_scanner_state_eq`. The reparse protocol is strict and two-step: `ts_tree_edit()` with a `TSInputEdit` on the old tree first, then `ts_parser_parse()` passing that edited tree, yielding a new tree that internally shares structure with the old one.

Two caveats from verification: (a) the failure mode without correct serialization is a silently **wrong** parse, not a safe fallback to full reparse — all external states would compare equal and stale subtrees would be reused; (b) `scanner.c` never bounds-checks against `TREE_SITTER_SERIALIZATION_BUFFER_SIZE` (zero references in the file), so pathological nesting could overflow or truncate state.

Sources: <https://github.com/tree-sitter-grammars/tree-sitter-markdown/blob/split_parser/tree-sitter-markdown/src/scanner.c>, <https://tree-sitter.github.io/tree-sitter/creating-parsers/4-external-scanners.html>, <https://github.com/tree-sitter/tree-sitter/blob/master/lib/src/parser.c>, <https://github.com/tree-sitter/tree-sitter/blob/master/docs/src/using-parsers/3-advanced-parsing.md>

### ratatui: differential repaint and resize

ratatui retains no layout tree and no document structure across frames or resizes, and a resize additionally defeats its differential repaint entirely, forcing a full repaint rather than a minimal delta.

`Terminal<B>`'s complete field list is backend, `buffers: [Buffer; 2]`, current, hidden_cursor, viewport, viewport_area, last_known_area, last_known_cursor_pos — no widget tree, no node graph. `Buffer { area: Rect, content: Vec<Cell> }` is flat, and `Buffer::resize` truncates / `Vec::resize`s with no content-preserving remap. `resize()` calls `set_viewport_area(next_area)` then `clear_viewport()`, whose body ends `self.buffers[1 - self.current].reset();` with the comment "Reset the back buffer to make sure the next update will redraw everything" — and `flush()` diffs exactly `buffers[1 - self.current]` against the current buffer. Upstream test `resize_fullscreen_triggers_clear_and_resets_back_buffer` asserts the back buffer equals `Buffer::empty(new_area)`. The only cross-frame layout persistence anywhere is a thread-local `LruCache<(Rect, Layout), (Segments, Spacers)>` (feature-gated on `layout-cache` + `std`, default size 500) — a memo of solved rects keyed on the area, so a resize changes the key, misses, and re-solves.

Precision note: `reset()` yields default cells, so the diff emits every non-default cell; combined with the physical `ClearType::All` on fullscreen, the observable result is a full repaint. Autoresize fires for Fullscreen and Inline viewports; `Viewport::Fixed` only on an explicit `resize()`.

**notcurses was not examined in this pass.** The brief asked about both ratatui and notcurses; no verified claim addresses notcurses' differential repaint or layout retention.

Sources: <https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/terminal/resize.rs>, <https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/terminal.rs>, <https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/buffer/buffer.rs>, <https://docs.rs/ratatui/latest/ratatui/layout/struct.Layout.html>, <https://docs.rs/ratatui-core/latest/src/ratatui_core/layout/layout.rs.html>

---

## D5 — Layout and tables

**Confidence on all findings in this section: high.**

### ratatui's solver is pane splitting, not document flow

ratatui's layout engine cannot size anything by measuring what a widget would render, and its solver is **kasuari**, the maintained Cassowary-family fork, not the `cassowary` crate. Layout's own docs: the algorithm "is based on the kasuari solver, a linear constraint solver that computes positions and sizes to satisfy as many constraints as possible in order of their priorities," and `split` is "a wrapper function around the kasuari solver to be able to split a given area into smaller ones." kasuari self-describes as "a fork of the unmaintained cassowary-rs crate with improvments and bug fixes," v0.4.12 released 2026-06-29; ratatui-core depends on `kasuari ^0.4` with no `cassowary` dependency.

Every `Constraint` variant is numeric — Min / Max / Length / Percentage / Ratio / Fill(u16) — with no Auto, Content, or FitContent. Layout's full method surface (new, vertical, horizontal, init_cache, direction, constraints, margin, flex, spacing, areas, try_areas, spacers, split, split_with_spacers) contains nothing that reflows or iterates content. `Layout::flex` distributes slack among already-numerically-constrained segments but never measures content. Wrapping lives in widgets (Paragraph + Wrap) and never feeds back into Layout sizing.

Sources: <https://docs.rs/ratatui/latest/ratatui/layout/struct.Layout.html>, <https://docs.rs/ratatui/latest/ratatui/layout/enum.Constraint.html>, <https://docs.rs/kasuari/latest/kasuari/>, <https://lib.rs/crates/ratatui-core>

### ratatui's Table has no content-based sizing

`Table::get_column_widths` (`table.rs:1041`) is the entire width logic: `let widths = if self.widths.is_empty() { vec![Constraint::Length(max_width / col_count.max(1) as u16); col_count] } else { self.widths.clone() };` then `Layout::horizontal(widths).flex(self.flex).spacing(self.column_spacing).split(columns_area)`. Nothing on that path inspects cell text; the only content-derived quantity is `column_count()` = `.map(|r| r.cells.len()).max()`, a count and not a measured width. So a renderer must compute min/max content widths itself before handing constraints to the widget.

Two precision notes. The docs warn "make sure to call the `Table::widths` method, otherwise the columns will all have a width of 0," but the **source shows equal division** on the empty-widths path — the docs' width-0 warning is misleading and should not be relied on. (This discrepancy is what killed the stronger candidate claim; see "Failed verification" below.) Second, the equal-width fallback divides before subtracting `column_spacing`, so columns do not exactly fill the area.

Sources: <https://docs.rs/ratatui/latest/ratatui/widgets/struct.Table.html>, <https://raw.githubusercontent.com/ratatui/ratatui/main/ratatui-widgets/src/table.rs>

### comfy-table already solves terminal table auto-layout

`dynamic.rs` documents and implements a seven-step arrangement: subtract borders/padding/fixed columns via `available_content_width()`; apply LowerBoundary minimums; find columns needing less than the average remaining space (including the MaxWidth constraint); fix their size and return the surplus to the pool; repeat until none are left; then divide the remaining space in relatively equal chunks. This is implemented by `find_columns_that_fit_into_average()` looping while `found_smaller`.

The wrap is **simulated during layout**: `optimize_space_after_split` / `longest_line_after_split` build a temporary `ColumnDisplayInfo` at `average_space` and call the real `split_line`; a column that wraps cleanly is frozen at its post-split longest-line width and its slack raised into `average_space` for the survivors. The freeze threshold is a bare literal, `if remaining_space >= 3 {`.

Overflow strategy is intra-cell wrapping at delimiters, never truncation or column hiding: `split_line()` splits by delimiter with a grapheme-aware mid-word fallback that carries the remainder to the next line. Truncation exists only via opt-in `row.max_height` (vertical); column hiding only via user-specified `Constraint::Hidden`; with `ContentArrangement::Disabled` the table simply overflows.

Sources: <https://raw.githubusercontent.com/Nukesor/comfy-table/main/src/utils/arrangement/dynamic.rs>, <https://raw.githubusercontent.com/Nukesor/comfy-table/main/src/utils/formatting/content_split/mod.rs>

### CSS auto table layout is formally incompatible with streaming

Column widths are per-column maxima over the min-content and max-content widths of every cell, so no width can be finalized until the last row arrives. CSS 2.1 §17.5.2.2, verbatim: "This algorithm may be inefficient since it requires the user agent to have access to all the content in the table before determining the final layout and may demand more than one pass." The contrast case is explicit in §17.5.2.1 (fixed layout): "the user agent can begin to lay out the table once the entire first row has been received. Cells in subsequent rows do not affect column widths."

The two per-cell measures: the minimum content width (MCW), where "the formatted content may span any number of lines but may not overflow the cell box," and the maximum cell width, "formatting the content without breaking lines other than where explicit line breaks occur"; per column, "determine a maximum and minimum column width from the cells that span only that column." css-tables-3 confirms the maximum-over-all-cells structure and restricts to the first row track only in fixed mode. Two refinements: css-tables-3 defines three per-column measures (the third, intrinsic percentage width, is irrelevant to a terminal), and the colSpan>1 update pass makes streaming strictly worse, not better.

**The mitigation lever:** the maxima are monotonically non-decreasing, so a prefix yields a valid lower bound that can only widen — widths can be rendered provisionally and only ever grow. Flagged explicitly: this observation came from a verifier, not from the spec.

Sources: <https://www.w3.org/TR/CSS21/tables.html>, <https://drafts.csswg.org/css-tables-3/>

### Slack distribution: use css-tables-3 §3.9.3, not CSS 2.1

CSS 2.1 supplies no usable slack-distribution rule. It states "UAs are not required to implement this algorithm… they can use any other algorithm even if it results in different behavior… The remainder of this section is non-normative," and on slack only "If the used width is greater than MIN, the extra width should be distributed over the columns," with no rule following; §17.1 concedes the auto algorithm "is not fully defined by this specification." (One sentence *is* normative — that inputs "must only include the width of the containing block and the content of… the table and any of its descendants" — an input-scoping MUST, not observable behavior.)

css-tables-3 §3.9.3 fills the gap with a directly portable algorithm: four sizing-guesses (min-content, min-content-percentage, min-content-specified, max-content) "in nondecreasing order," and "the used widths of the columns must be the linear combination (with weights adding to 1) of the two consecutive sizing-guesses whose width sums bound the available width," plus a second branch (§3.9.3.2, six ordered rules) for distributing excess beyond max-content.

Three adoption caveats: for Markdown/GFM tables there are no percentage or fixed widths, so all columns are auto-columns and the four guesses collapse to a two-point min↔max interpolation; the spec assumes continuous widths, so integer-cell rounding and remainder distribution are unspecified work; and css-tables-3 carries "Not Ready For Implementation" boilerplate (latest WD 2026-05-02), which is why the CSS 2.1 REC corroboration matters — the algorithm is descriptive of browsers, not a Recommendation.

Sources: <https://www.w3.org/TR/CSS21/tables.html>, <https://drafts.csswg.org/css-tables-3/>, <https://www.w3.org/TR/2026/WD-css-tables-3-20260502/>

### Overflow beyond max width is an invention

CSS permits a table to overflow its container rather than compress below the sum of column min-content widths, and defines no shrink-below-min-content, column-drop, or ellipsis policy. css-tables-3 §3.9.1: GRIDMIN is "the sum of the min-content width of all the columns plus cell spacing or borders"; "the used min-width of a table is the greater of the resolved min-width, CAPMIN, and GRIDMIN"; "if the table-root has 'width: auto', the used width is the greater of min(GRIDMAX, the table's containing block width), the used min-width of the table." So used width ≥ GRIDMIN unconditionally, and when GRIDMIN exceeds the container the table overflows — arithmetic, not inference. §3.9.3.2 is titled "Distributing *excess* width" and all six rules only increase widths; full-text greps for shrink/narrower/below-the-min return nothing on the width axis. CSS 2.2 §17.5.2.2 agrees. §3.6.1 adds that non-visible `overflow` on the table-root and wrapper "is ignored and treated as if its value was visible."

Note for a terminal: GRIDMIN includes cell spacing and borders — the separators. Any max-width overflow strategy under S2 is therefore work beyond the CSS reference.

Sources: <https://drafts.csswg.org/css-tables-3/>, <https://www.w3.org/TR/CSS22/tables.html>

### taffy is not a table engine, but has the measure hook

taffy v0.12.2 (released 2026-07-15) "currently implements the Flexbox, Grid and Block layout algorithms from the CSS specification." `taffy::compute` exposes only `compute_flexbox_layout` / `compute_grid_layout` / `compute_block_layout` (plus leaf/root/hidden/cached/round/print); `style::Display` has exactly four variants — Block, Flex, Grid, None — so a table mode is not even expressible. The roadmap issue #345 (open since 2023-01-30, last updated 2026-06-25) never mentions tables. Its coordinate space is f32, not integer terminal cells.

What it does give you: `compute_layout_with_measure` takes `MeasureFunction: FnMut(Size<Option<f32>>, Size<AvailableSpace>, NodeId, Option<&mut NodeContext>, &Style) -> Size<f32>`, documented for integrating "other layout modalities such as text or image layout," and `FnMut` so it can borrow a font or measurement registry.

Three qualifications: taffy's Grid *does* do min-content/max-content track sizing, so a grid could approximate columns without the CSS table rules (no colSpan distribution, no table-specific capping); the closure returns only a `Size`, not break positions, so wrap offsets must be recomputed or cached at paint time; and taffy's `round_layout` already rounds on cumulative viewport-relative coordinates and derives width/height from rounded edges, so nested-node drift is handled **if** you map 1 cell = 1.0 unit and use it rather than rounding per node yourself.

Sources: <https://docs.rs/taffy/latest/taffy/>, <https://github.com/DioxusLabs/taffy/issues/345>

---

## D6 — Feature surface and distribution

**Confidence on all findings in this section: high.**

### Parser feature coverage

| Feature | comrak 0.54.0 | pulldown-cmark 0.13.4 | markdown-rs 1.0.0 |
|---|---|---|---|
| Tables | `table` (opt-in bool) | `ENABLE_TABLES` | `gfm_table` (via `Constructs::gfm()`) |
| Task lists | `tasklist` | `ENABLE_TASKLISTS` | `gfm_task_list_item` |
| Footnotes | `footnotes`, `inline_footnotes` | `ENABLE_FOOTNOTES`, `ENABLE_OLD_FOOTNOTES` | `gfm_footnote_definition`, `gfm_label_start_footnote` |
| Admonitions / GitHub alerts | `alerts` → `Alert` node | `ENABLE_GFM` → `Tag::BlockQuote(Option<BlockQuoteKind>)` | **no field** |
| Definition lists | `description_lists` → `DescriptionList` node | `ENABLE_DEFINITION_LIST` | **no field** (`definition` is CommonMark link reference definitions — an easy misread) |
| Frontmatter | `front_matter_delimiter` → `FrontMatter(String)` | `ENABLE_YAML_STYLE_METADATA_BLOCKS`, `ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS` | `frontmatter` (off by default) |
| Math | `math_dollars`, `math_latex`, `math_code` → `Math` node | `ENABLE_MATH` → `Event::InlineMath` / `Event::DisplayMath` | `math_flow`, `math_text` → `Node::Math` / `Node::InlineMath` (off by default; **not** enabled by `gfm()`) |
| Mermaid | none (grep returns zero) | none — arrives as a fenced-code info string | **no field** |
| OSC 8 hyperlinks | none — renderer-side concern | renderer-side concern | renderer-side concern |
| Other | strikethrough, autolink, superscript, subscript, wikilinks (both title orders), header_id_prefix | strikethrough, smart punctuation, heading attributes, superscript, subscript, wikilinks, GFM | six-flag `gfm()` preset; ~20 CommonMark constructs true by default |

comrak's fields are plain bool/Option on a `#[derive(Default)]` struct in a 1489-line `options.rs`, and they are genuine parser extensions producing walkable AST — `nodes.rs` declares `FrontMatter(String)`, `DescriptionList`, `FootnoteDefinition`, `Table`, `TableRow`, `TableCell`, `TaskItem`, `Strikethrough`, `Superscript`, `FootnoteReference`, `Math`, `WikiLink`, `Subscript`, `Alert`. **Naming defect to carry into the plan: the struct is `Extension`, accessed as `options.extension.<field>`. `ExtensionOptions` does not compile against 0.54.0.**

pulldown-cmark 0.13.4 has exactly 15 flags. `ENABLE_GFM` delivers alerts as typed data — `Tag::BlockQuote(Option<BlockQuoteKind>)`, "only parsed & populated with `Options::ENABLE_GFM`", with `BlockQuoteKind` having exactly the five variants NOTE/TIP/IMPORTANT/WARNING/CAUTION. So GFM alerts need no custom rule; other admonition dialects (MkDocs `!!! note`, `:::note`) still would. **Time-sensitive:** upstream master has already added `ENABLE_CONTAINER_EXTENSIONS` and `ENABLE_HIGHLIGHT`, so the count of 15 is correct for 0.13.4 and goes stale on the next minor release.

markdown-rs's `impl Default for Constructs` is doc-commented "`CommonMark`" and sets frontmatter, all six `gfm_*`, `math_flow`, `math_text` and all five `mdx_*` to `false`; the other ~20 fields true. `Constructs::gfm()` is exactly six flags and notably does **not** enable math. The 34-field struct has no definition-list, alert, or mermaid field.

Sources: <https://raw.githubusercontent.com/kivikakk/comrak/main/src/parser/options.rs>, <https://raw.githubusercontent.com/kivikakk/comrak/main/src/nodes.rs>, <https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.Options.html>, <https://docs.rs/pulldown-cmark/latest/pulldown_cmark/enum.Tag.html>, <https://docs.rs/pulldown-cmark/latest/pulldown_cmark/enum.BlockQuoteKind.html>, <https://raw.githubusercontent.com/pulldown-cmark/pulldown-cmark/master/pulldown-cmark/src/lib.rs>, <https://docs.rs/markdown/latest/markdown/struct.Constructs.html>, <https://docs.rs/markdown/latest/src/markdown/configuration.rs.html>, <https://raw.githubusercontent.com/wooorm/markdown-rs/main/src/configuration.rs>

### Math is detection-only in all three parsers

Every candidate hands back the raw TeX string inside a marker node and none typeset anything, so the entire LaTeX-to-glyphs/image/cells path is unowned work regardless of parser choice. comrak's executable doc-tests show pure passthrough: `markdown_to_html("$1 + 2$ and $$x = y$$")` → `<p><span data-math-style="inline">1 + 2</span> and <span data-math-style="display">x = y</span></p>`; its AST is `NodeMath { dollar_math: bool, display_math: bool, literal: String }`, documented "contains raw text which is not parsed as Markdown." pulldown-cmark's `ENABLE_MATH` "emits two events `Event::InlineMath` and `Event::DisplayMath` that conventionally contain TeX formulas," both `CowStr<'a>` payloads; its own `html.rs` (lines 112-121) merely escapes the TeX into `<span class="math math-inline">`. markdown-rs's Math/InlineMath nodes likewise carry only `value: String`.

Parser selection therefore determines whether you get delimiter detection and a typed node for free; it has zero bearing on the rendering path.

Sources: <https://raw.githubusercontent.com/kivikakk/comrak/main/src/parser/options.rs>, <https://raw.githubusercontent.com/kivikakk/comrak/main/src/nodes.rs>, <https://docs.rs/pulldown-cmark/latest/pulldown_cmark/enum.Event.html>, <https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.Options.html>

### Mermaid: pure-Rust rendering exists, single-binary distribution survives

Mermaid rendering in a Rust binary does not require a JS runtime, a browser, or shelling to `mmdc`. Live crates.io API re-verification on 2026-07-21 found, none yanked: **merman** 0.8.0-alpha.3 (updated 2026-07-09, 13,130 downloads), **mermaid-rs-renderer** 0.3.1 (2026-07-06, 30,260 downloads), **mermaid-svg** 0.7.0 (2026-07-09), **mermaid-text** 0.57.0 (2026-07-21, 9,655 downloads, 81 published versions), plus **mmdflux** 2.6.0.

Dependency graphs pulled from `/api/v1/crates/{name}/{ver}/dependencies` contain **zero `-sys` crates**: mermaid-svg has one normal dep (thiserror); mermaid-rs-renderer has anyhow/fontdb/json5/once_cell/regex/serde/serde_json/thiserror/ttf-parser plus optional resvg/usvg/clap; merman has merman-core/chrono plus optional render/ascii/resvg/usvg/tiny-skia/image/krilla. merman's README: "does not launch a browser or JavaScript runtime to render diagrams."

Most directly relevant to a TUI: **mermaid-text** describes itself as "render Mermaid diagrams as Unicode box-drawing text — no browser, no image protocols, pure Rust" and ships as `crates/mermaid-text` inside `github.com/leboiko/markdown-reader`, whose README opens "the terminal markdown reader with hybrid live-preview editing, inline Mermaid diagrams, and LaTeX math" and calls mermaid-text "the text-mode fallback… covers 18 diagram types."

**Fidelity is the open risk, not dependencies.** merman's `max_stable_version` is 0.7.0 (0.8.0 is prerelease) and mermaid-rs-renderer's README says visual output "may not yet match mermaid-cli in all cases." All capability descriptions are first-party README text; nothing was compiled or run. Download counts of 9–30k on crates this young look like CI traffic rather than external adoption.

Sources: <https://crates.io/api/v1/crates?q=mermaid&per_page=20>, <https://crates.io/api/v1/crates/mermaid-text>, <https://raw.githubusercontent.com/Latias94/merman/main/README.md>, <https://github.com/leboiko/markdown-reader>

### comrak's mermaid hook

comrak has no native mermaid support and no diagram concept — `grep -ni mermaid src/parser/options.rs` on main exits 1. The designated hook is `RenderPlugins<'p>`'s `codefence_renderers: HashMap<String, &'p dyn CodefenceRendererAdapter>`, documented "provide language-specific renderers for codefence blocks. `math` codefence blocks are handled separately by Comrak's built-in math renderer, so entries keyed by `math` in this map are not used." GitHub code search across the whole repo returns `total_count=1` for mermaid: a test-local `struct MermaidAdapter` registered via `plugins.render.codefence_renderers.insert("mermaid".to_string(), &adapter)`. Test `language_specific_codefence_renderer_precedes_highlighter` shows the renderer wins over `codefence_syntax_highlighter`.

Correction to the original claim wording: `codefence_syntax_highlighter` could also intercept fences, and a caller can always walk the AST post-parse, so `codefence_renderers` is the *designated* hook, not the only route.

Sources: <https://raw.githubusercontent.com/kivikakk/comrak/main/src/parser/options.rs>, <https://github.com/kivikakk/comrak/blob/main/src/tests/plugins.rs>

### OSC 8 hyperlinks

No verified claim covers OSC 8. The run fetched the canonical OSC 8 reference (<https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda>, logged under the D6 angle) but no claim from it survived to verification. What *is* established is only the negative: none of the three parsers touch OSC 8 — it is a renderer-side concern in all of them.

---

## D7 — Rust stack verification

**This dimension produced zero verified findings. It is unanswered.**

Mapping all 39 confirmed claims to dimensions: D4 gets 13, D5 gets 16, D6 gets 10. **None** address kitty graphics protocol emission, image decoding or pre-scaling, unicode-width versus unicode-segmentation, the per-terminal width correction tables, syntect versus tree-sitter for highlighting, or the cell-grid differ as a named crate with a version and maintenance status. Of the 40 claims that reached the verify phase, none came from the D7 angle. Any build-readiness verdict on these five subsystems would be a guess.

**One adjacent fact does bear on the cell-grid differ**, established under D4 rather than D7: ratatui's differ is a double-buffer cell diff (`flush()` diffs `buffers[1 - self.current]` against the current buffer over a flat `Buffer { area: Rect, content: Vec<Cell> }`), and a resize resets the back buffer, forcing a full repaint. Confidence high. Source: <https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/terminal/resize.rs>. This is not a substitute for the D7 verification the brief asked for — no version, maintenance status, or "what it does not give you" assessment was produced.

### Leads only — fetched, but no claim survived to verification

These URLs are recorded in the run's source list under the D7 angle. **They are not evidence.** No claim extracted from them was verified, and nothing here should be treated as established. A planner should re-run D7 against them.

- <https://github.com/unicode-rs/unicode-width/issues/71>
- <https://pypi.org/pypi/wcwidth/json>
- <https://docs.rs/runefix-core/latest/runefix_core/>
- <https://docs.rs/kitty-graphics-protocol/latest/kitty_graphics_protocol/>
- <https://docs.rs/ratatui/latest/ratatui/buffer/struct.Buffer.html>
- <https://crates.io/api/v1/crates/syntect>

---

## D2b — Ghostty kitty graphics protocol

**RESOLVED 2026-07-21, outside the workflow run.** The research pass itself produced zero verified findings here; the gap was closed afterwards by a direct fetch of Ghostty's own documentation.

| Finding | Evidence | Confidence |
|---|---|---|
| Ghostty implements the kitty graphics protocol. | [ghostty.org/docs/features](https://ghostty.org/docs/features), verbatim: *"**Kitty graphics protocol**: Ghostty supports the Kitty graphics protocol, which allows terminal applications to render images directly in the terminal."* This is Ghostty's own documentation — the primary source the brief asked for, not a third-party list. | high |
| No minimum version is stated, and no caveats or partial-support notes accompany the claim. | Same page — version numbers and caveats are absent, not negative. | high |

**Residual gap.** The docs assert support but do not enumerate *which* parts of the protocol are implemented — the kitty spec is large (transmission mediums, placement, z-index, deletion, Unicode placeholders, animation). Support for the specific subset a markdown renderer needs — chunked base64 direct transmission, cell-anchored placement, and deletion on repaint — is **not** established by this source. Verify against Ghostty's source or a capability probe before the image subsystem is designed in detail.

Under S3 this is no longer a single point of failure for the image feature as a whole; it is now a scoping question about protocol coverage.

### Leads only — fetched, but no claim survived to verification

Recorded in the run's source list under the D2b angle. **Not evidence.** Re-run D2b against them.

- <https://github.com/ghostty-org/ghostty/blob/main/src/terminal/kitty/graphics_command.zig>
- <https://github.com/ghostty-org/ghostty/blob/main/src/terminal/kitty/graphics_exec.zig>
- <https://github.com/ghostty-org/ghostty/blob/main/src/terminal/kitty/graphics_unicode.zig>
- <https://github.com/ghostty-org/ghostty/issues/5255>
- <https://github.com/ghostty-org/ghostty/issues/8272>
- <https://github.com/ghostty-org/ghostty/blob/main/src/config/Config.zig>

---

## Hard edges

| Edge | Why it is hard | Unsolvable or merely unsolved | Mitigation | Confidence / source |
|---|---|---|---|---|
| No shipped Rust parser has a resumable parse API | pulldown-cmark, comrak, markdown-rs all take a complete `&str` and return a fresh owned result; markdown-rs's `mdast::Node` has no lifetime and no patch path | Unsolved (an API gap, not a barrier) | Three paths: fork comrak for `feed`/`finish`; adopt tree-sitter-markdown; or re-parse the grown prefix each chunk using `into_offset_iter` byte ranges as boundaries | high — [pulldown-cmark Parser](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.Parser.html), [comrak parser/mod.rs](https://github.com/kivikakk/comrak/blob/main/src/parser/mod.rs), [markdown-rs lib.rs](https://docs.rs/markdown/latest/src/markdown/lib.rs.html) |
| Committing a stable prefix that later input can invalidate | An unclosed fence retroactively reinterprets everything after it; no named editor/LSP technique was found, and the *stability* of pulldown-cmark's container-block Start/End ranges as re-parse boundaries was never verified | Unsolved; and the boundary-coarseness question is open | tree-sitter's edit-then-reparse protocol is the one documented mechanism; the offset-based approach is engineering inference | high on the primitive, **inference** on its use — [OffsetIter](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/struct.OffsetIter.html), [tree-sitter advanced parsing](https://github.com/tree-sitter/tree-sitter/blob/master/docs/src/using-parsers/3-advanced-parsing.md) |
| tree-sitter's silent-corruption failure mode | Without correct external-scanner serialization, stale subtrees are reused and the parse is silently *wrong* — not a safe fallback to full reparse. `scanner.c` never bounds-checks `TREE_SITTER_SERIALIZATION_BUFFER_SIZE` | Unsolved latent defect (upstream, in the grammar) | Bound the nesting depth, or patch the scanner; subtree reuse is gated at `parser.c:783` on `ts_subtree_external_scanner_state_eq` | high — [scanner.c](https://github.com/tree-sitter-grammars/tree-sitter-markdown/blob/split_parser/tree-sitter-markdown/src/scanner.c), [tree-sitter parser.c](https://github.com/tree-sitter/tree-sitter/blob/master/lib/src/parser.c) |
| Table widths cannot be finalized before the last row | CSS 2.1 §17.5.2.2 states outright that auto layout "requires the user agent to have access to all the content in the table before determining the final layout" | **Unsolvable in principle** for a true stream; only mitigable | Column maxima are monotonically non-decreasing — render provisional widths that only ever widen. (Monotonicity is a verifier's observation, not spec text.) | high on the spec, **inference** on the mitigation — [CSS 2.1 §17.5](https://www.w3.org/TR/CSS21/tables.html), [css-tables-3](https://drafts.csswg.org/css-tables-3/) |
| CSS defines no overflow policy below min-content | Used width ≥ GRIDMIN unconditionally; §3.9.3.2 rules only *increase* widths; non-visible `overflow` on a table root is ignored | Unsolved — genuinely absent from the reference | Any max-width strategy under S2 (truncate, hide columns, horizontal scroll, ellipsis) is an invention. comfy-table's precedent is intra-cell wrapping only, with `Constraint::Hidden` user-driven | high — [css-tables-3 §3.9.1](https://drafts.csswg.org/css-tables-3/), [CSS 2.2 §17.5.2.2](https://www.w3.org/TR/CSS22/tables.html) |
| Integer cells vs. continuous spec widths | css-tables-3's linear interpolation assumes continuous widths; rounding and remainder distribution across cells is unspecified | Unsolved, small | Own it explicitly. taffy's `round_layout` handles cumulative rounding *if* 1 cell = 1.0 unit | high — [css-tables-3](https://drafts.csswg.org/css-tables-3/), [taffy](https://docs.rs/taffy/latest/taffy/) |
| ratatui retains no layout tree; resize forces full re-solve and full repaint | `Terminal` holds only two flat buffers and rects; `Buffer::resize` has no content-preserving remap; `clear_viewport()` resets the diff baseline; the only cross-frame cache is an LRU keyed on `(Rect, Layout)`, which a resize misses by construction | Unsolved — architectural to ratatui | Own the document layout tree above ratatui and treat its buffer purely as a paint target; a resize is a re-derive either way | high — [resize.rs](https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/terminal/resize.rs), [terminal.rs](https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/terminal.rs), [buffer.rs](https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/buffer/buffer.rs) |
| No content-driven constraint anywhere in ratatui | Every `Constraint` variant is numeric; no Auto/Content/FitContent; `Table::get_column_widths` never inspects cell text | Unsolved — architectural | Compute min/max content widths yourself and hand numeric constraints down. comfy-table's `dynamic.rs` is a working, portable algorithm | high — [Constraint](https://docs.rs/ratatui/latest/ratatui/layout/enum.Constraint.html), [table.rs](https://raw.githubusercontent.com/ratatui/ratatui/main/ratatui-widgets/src/table.rs), [comfy-table dynamic.rs](https://raw.githubusercontent.com/Nukesor/comfy-table/main/src/utils/arrangement/dynamic.rs) |
| Math typesetting is owned by nobody | All three parsers hand back the raw TeX string in a marker node; comrak's own renderer just wraps it in a `<span>` | Unsolved — no candidate found in this pass | None identified. Parser choice is irrelevant to it | high — [comrak nodes.rs](https://raw.githubusercontent.com/kivikakk/comrak/main/src/nodes.rs), [pulldown-cmark Event](https://docs.rs/pulldown-cmark/latest/pulldown_cmark/enum.Event.html) |
| Mermaid crate fidelity is unproven | Pure-Rust dependency graphs are verified; *rendering capability* rests on first-party README text. Nothing was compiled or run. merman's stable is 0.7.0 (0.8.0 alpha); mermaid-rs-renderer admits output "may not yet match mermaid-cli" | Unsolved, and it is an evaluation task, not a build task | Compile mermaid-text against real diagrams before committing; the crates are young and fast-moving (mermaid-text shipped its 81st version on the search date) | high on deps, **first-party only** on capability — [crates.io mermaid query](https://crates.io/api/v1/crates?q=mermaid&per_page=20), [merman README](https://raw.githubusercontent.com/Latias94/merman/main/README.md) |
| D2b unconfirmed: Ghostty kitty graphics | No primary-source verification was obtained | Unknown — not yet investigated to conclusion | Re-run D2b against the Ghostty source URLs listed above before any image work is planned | n/a — zero findings |
| D7 unconfirmed: five subsystems | Zero verified claims; none of the 40 verified claims came from this angle | Unknown — not yet investigated to conclusion | Re-run D7 | n/a — zero findings |
| No adversarial counter-evidence sweep ran | WebSearch budget hit 200/200 before the counter-evidence step in every verifier | Method gap for this pass | Fine for API/source facts; a real blind spot for "does anyone already do X" (falsification claim 1) | run caveats |

---

## Falsification results

Verdicts use the brief's vocabulary: **confirmed** = the claim survives the evidence; **refuted** = the evidence shows the claim is false; **unresolved** = no bearing evidence, per the brief's rule that a claim is not confirmed merely because nothing contradicted it.

| # | Claim | Verdict | Evidence | Confidence |
|---|---|---|---|---|
| 1 | No open-source library does incremental/streaming markdown rendering to a terminal | **unresolved** | Nothing in this pass tested it. The nearest bearing signal is `leboiko/markdown-reader`, a Rust terminal markdown reader advertising "hybrid live-preview editing" — suggestive, but it was never examined for incremental parsing. The missing counter-evidence sweep makes this the weakest spot in the pass. Re-test before relying on it. | n/a — <https://github.com/leboiko/markdown-reader> |
| 2 | Terminal table layout is unsolved in practice — everyone overflows rather than implementing min/max content-width distribution | **refuted** | comfy-table's `dynamic.rs` implements real iterative content-width distribution with delimiter-aware wrap simulation and slack redistribution. One existence proof defeats a universal. | high — <https://raw.githubusercontent.com/Nukesor/comfy-table/main/src/utils/arrangement/dynamic.rs> |
| 3 | Mermaid rendering requires a JS runtime or shelling to `mmdc`, killing single-binary distribution | **refuted** | Four pure-Rust Mermaid crates with dependency graphs verified free of `-sys` crates; mermaid-text renders straight to Unicode box drawing. Caveat: rendering *fidelity* is unproven — the refutation is of the dependency claim, not a guarantee of output quality. | high on deps — <https://crates.io/api/v1/crates?q=mermaid&per_page=20>, <https://crates.io/api/v1/crates/mermaid-text> |
| 4 | No Rust markdown parser exposes resumable or incremental parsing; all require re-parsing from the start on each chunk | **split: confirmed as an API fact, refuted architecturally** | Confirmed for the shipped public APIs of pulldown-cmark, comrak and markdown-rs. Refuted as an architectural claim: comrak's private driver is already a per-line loop over struct-held state (and upstream cmark ships `feed`/`finish` on the same design), and tree-sitter-markdown is fully incremental today. | high — <https://github.com/kivikakk/comrak/blob/main/src/parser/mod.rs>, <https://github.com/tree-sitter-grammars/tree-sitter-markdown/blob/split_parser/tree-sitter-markdown/src/scanner.c> |
| 5 | No Rust crate ports Python wcwidth's per-terminal width correction tables, so that work must be written from scratch | **unresolved** | Zero claims in this pass bear on it — D7 was not researched. The `runefix-core` and `unicode-width#71` URLs were fetched but nothing from them was verified. Do not assume either direction. | n/a — no verified evidence |
| 6 | ratatui retains no document-level layout tree; a resize means re-deriving layout from source | **confirmed**, and more strongly than stated | No layout tree, no content-preserving buffer remap, and a resize also resets the diff baseline forcing a full repaint. The one cross-frame cache is an LRU keyed on `(Rect, Layout)` — a resize changes the key and misses by construction. | high — <https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/terminal/resize.rs>, <https://docs.rs/ratatui-core/latest/src/ratatui_core/layout/layout.rs.html> |

### Failed verification (distinct from the table above)

One candidate claim failed the run's own verification, 1-2. **This means the evidence offered did not establish it — it does NOT establish the negation.** A planner should re-test it rather than assume either way.

| Candidate claim | Vote | Why it failed | Source |
|---|---|---|---|
| ratatui's `Table` performs no content-based auto sizing **and** a Table with no widths set renders every column at width 0 | 1-2 | The first half survived and is reported under D5. The second half was contradicted by source: `get_column_widths` divides the available width equally when `widths` is empty. ratatui's own docs still carry the misleading "width of 0" warning, so the docs and the source disagree — treat the source as authoritative and re-test against the version you pin. | <https://docs.rs/ratatui/latest/ratatui/widgets/struct.Table.html> |

---

## Build readiness

> **SUPERSEDED IN PART, 2026-07-21.** Three of the five subsystems below were resolved after this pass by targeted follow-up work outside the workflow — see *Post-pass resolutions* immediately below. The original table is kept because its "cannot rate" entries record what the workflow itself did and did not establish.

### Post-pass resolutions (2026-07-21)

Scope decision **S5** was taken after this pass: build in-house rather than adopt, treating open-source implementations as reference material. That dissolves most of D7 as a blocker — "which crate" now matters only for the short adopt list. The own/adopt split:

| Layer | Decision | Basis |
|---|---|---|
| Markdown parser, layout tree, cell-grid differ, kitty emission, table auto-layout | **Own** | Nothing existing fits, and this is the product. Parser ownership is defensible rather than NIH because CommonMark ships an official ~650-case conformance suite, making the work bounded and verifiable; and because pulldown-cmark is an event stream rather than an AST while comrak's AST is arena-bound with lifetimes that fight a retained layout tree. |
| Ghostty width correction layer | **Own — no choice** | Per-terminal correction tables ship in Python's `wcwidth` ≥ 0.8.0 and exist nowhere in Rust. |
| Unicode base tables (`unicode-width`, `unicode-segmentation`) | **Adopt** | UCD-derived data, not logic. Owning them means owning a regeneration pipeline against an annually-revised corpus, plus hand-implementing UAX #29 GB1–GB13 including the ZWJ and regional-indicator traps flagged in pass 1. Zero differentiation. |
| Image decoding | **Adopt** (`image`) | Parsing hostile input is a security surface; months of work for nothing a user sees. |
| Mermaid | **Adopt or vendor** one of the four pure-Rust crates | Mermaid rendering means graph layout algorithms — a project, not a feature. |

**Math — RESOLVED, and it is the strongest differentiator found in either pass.** Shelling out is *not* required. Two pure-Rust paths were verified by compiling and running them, not by reading about them:

| Crate | Version / released | Health | Output | Fonts |
|---|---|---|---|---|
| **RaTeX** (`ratex-parser` / `-layout` / `-render`) | 0.1.13, 2026-07-07 | 1,405 stars; commits to 2026-07-22 | PNG bytes via tiny-skia; also SVG/PDF | 20 KaTeX TTFs **embedded** (`embed-fonts`) |
| **txm** | 0.1.5, 2026-07-17 | 308 stars, 5 releases in 8 days | 2D Unicode character grid | none |
| ReX (`KenyC/ReX`) | 0.1.2 — **git-only; the crates.io `rex` name is squatted by an unrelated FP language** | Last commit 2026-02-24 | tiny-skia `Pixmap` | OpenType MATH via `ttf-parser` |
| katex-rs · math-core / pulldown-latex · mathtex · tui-math | — | katex-rs is a real 45k-LOC port but 3 commits in 6 months; math-core emits MathML (needs a browser for layout); mathtex is a 4-control-sequence skeleton; tui-math's repo 404s and it depends on latex2mathml, dead since 2020 | — | — |

**Recommended path: RaTeX → PNG → kitty graphics, with txm as the no-graphics fallback.** Measured, not estimated: 6/6 formulas at ~200 µs each, **4.6 MB stripped with fonts embedded**, dependency tree 100% pure Rust — no `cc`, `bindgen`, or `pkg-config`. Kitty accepts PNG directly via `f=100`. 595+ control sequences; all matrix/align/gather/cases environments. Fidelity ceiling is KaTeX's, i.e. effectively none for document math.

*Strongest counterargument:* RaTeX is at 0.1.13, four months old, and the repository carries `.claude/`, `.cursor/` and `.agents/` directories — heavily AI-assisted. Its 1,050-case golden corpus is real, but the ">99.5% KaTeX coverage" claim is unreproduced. The hedge is ReX (more mature design, OpenType MATH) at the cost of a git dependency.

**The Unicode fallback is better than assumed.** The hard cap belongs to *character substitution* (unicodeit, pandoc — no subscript uppercase Latin exists in Unicode at all, and 9 lowercase letters are missing), not to *2D grid layout*. txm draws with box characters and escapes that cap: verified rendering of stretched matrix delimiters, nested fractions, `\sqrt[3]` with index, and integrals and sums with limits above and below. It fails on `\begin{cases}` and `align`.

**Nobody else in this space does math.** glow, mdcat, frogmouth, bat, slides and md-tui have none — open issues, zero code hits. presenterm shells out to `pandoc --to typst` → `typst compile --ppi 300` → PNG. euporie falls back across flatlatex / sympy / dvipng / utftex. **No pure-Rust in-process math renderer exists in a terminal markdown tool.** This partially restores the novelty argument that S4 weakened.

### Two hard edges the math work introduces

| Edge | Detail | Mitigation |
|---|---|---|
| **Kitty's `c`/`r` scaling does not solve resize** | It rescales without re-transmitting, but kitty implements it as `GL_LINEAR` (`graphics.c:710` → `shaders.c:276`) — bilinear, so upscaled math is blurry. | A font-size change requires re-rasterizing. Budget for it. |
| **Rasterization is size-dependent; layout is not** | RaTeX's layout is in **em units** — verified identical (`dl w=7.482`) at 16 px and 32 px. | Cache parse+layout across sizes; repeat only rasterization. Scrolling is free: transmit once (`a=T,i=<id>`), virtual placement (`U=1`) with Unicode placeholders, `a=d,d=i` to drop placements while keeping pixels. `ratatui-image` is the caching blueprint — content-hash plus target-size key, transmit-once behind an `AtomicBool`. |

### Syntax highlighting — decided by spike, not by argument

Stated as a headline feature, which changes the calculus. Findings:

- tree-sitter's incremental reparse — its headline advantage — is **void under S4**, since the document is parsed once into a retained tree.
- syntect's worst flaw is routed around: its markdown grammar hardcodes 42 languages and cannot dynamically embed (issue #650 hangs), but because the markdown parser is owned in-house, the highlighter receives `(fence body, lang tag)` directly and never enters that path.
- "Regex can't count" is **false** — Sublime's context stack handles nested comments, and backreferences handle `r#"…"#`.
- **Against syntect:** pinned to roughly two-year-old Sublime syntaxes. `async fn` in Rust goes unhighlighted; no PEP 701 f-strings. README: *"mostly complete… not under heavy development."* For a headline feature this is the dominant consideration.
- Third option: **`lumis` 0.12.0** — 110+ prewired tree-sitter grammars with per-language optional Cargo features, which directly attacks the size objection (the alarming figures — zola's "easily add 100 MB", an 89 MB SQL grammar — assume compiling everything). Risk: 8.6k downloads, 0.7→0.12 in three months, and whether it exposes raw spans is **unverified** — decisive, since the theme layer is owned in-house.

**Decisive unmeasured number:** no real measured static-binary size exists for multi-grammar tree-sitter. Resolve by spike, not research — build a toy binary with the 20 target languages via lumis and measure, and confirm raw spans are reachable. Under ~30 MB with spans exposed → take current grammars. At 100 MB, or spans locked behind a formatter → syntect + two-face (~2.9 MB, +0.6 MB for 100+ extra grammars) and accept the staleness.

*Caveat on the syntect path:* the April 2026 fancy-regex/Oniguruma parity fixes are **unreleased** — no tag after v5.3.0 — so pure-Rust parity currently requires a git dependency.

### Original pass table (retained)

The brief asks for an adopt / adapt / write-from-scratch rating on each of D7's five subsystems. **Four of the five cannot be rated from this pass**, because D7 produced zero verified findings — no crate, version, or maintenance status was established for any of them. Rating them anyway would be a guess, which is worth less than an honest blank.

| D7 subsystem | Rating | Basis | What would settle it |
|---|---|---|---|
| Kitty graphics protocol emission | **cannot rate — no evidence** | Zero verified claims. `kitty-graphics-protocol` was fetched as a lead only. | Verify the crate's version, maintenance status and what it omits — and D2b first, since Ghostty support gates the whole feature |
| Image decoding and pre-scaling | **cannot rate — no evidence** | Zero verified claims; no crate was even fetched as a lead. | Name a crate and verify decode formats, scaling quality, and cost per frame |
| Unicode width and grapheme segmentation | **cannot rate — no evidence** | Zero verified claims. `unicode-width#71`, `runefix-core` and the wcwidth PyPI metadata were fetched as leads and never verified. Falsification claim 5 is unresolved as a direct consequence. | Determine whether **any** Rust crate ports the per-terminal correction tables that Python's wcwidth ≥ 0.8.0 ships. If none does, this becomes a write-and-maintain-forever subsystem, which is the single most build-decision-relevant unknown in the whole pass |
| Syntax highlighting (syntect vs tree-sitter) | **cannot rate — no evidence** | Zero verified claims; the syntect crates.io endpoint was fetched as a lead only. Note the adjacency: if tree-sitter-markdown is adopted for D4 incrementality, a tree-sitter highlighter shares the runtime — but no verified claim supports that trade-off. | Compare on incremental re-highlight cost, grammar availability, and binary size, against the D4 parser decision |
| Cell-grid differ | **adopt ratatui's, with a known caveat** — the only one of the five with bearing evidence, and that evidence came from D4, not from a D7 verification pass | ratatui's `flush()` diffs a flat double buffer cell-by-cell; a resize resets the back buffer and forces a full repaint, and `Buffer::resize` does no content-preserving remap. Confidence high. <https://github.com/ratatui/ratatui/blob/main/ratatui-core/src/terminal/resize.rs> | Whether the full-repaint-on-resize behaviour is acceptable at the target document sizes, and whether ratatui's crate version and maintenance status hold up under the D7 check that was never run |

### The single largest unknown remaining

> **BOTH ITEMS BELOW ARE NOW RESOLVED.** Retained for the record; current status follows each.

~~**Whether Ghostty implements the kitty graphics protocol.**~~ **Resolved** against Ghostty's own documentation — see the D2b section. Residual question is protocol *coverage*, not existence: whether the specific subset a viewer needs (chunked base64 direct transmission, cell-anchored and virtual placement, deletion on repaint) is implemented. The math work makes virtual placements and Unicode placeholders load-bearing, so that subset check is now higher-stakes than it was.

~~**Whether any Rust crate ports the per-terminal width correction tables.**~~ **Resolved by decision rather than by discovery.** Under S5 this is owned in-house regardless of what exists, since the correction layer is Ghostty-specific and the base UCD tables are adopted from `unicode-width` / `unicode-segmentation`. Targeting one terminal (S3) bounds the work to a single correction table rather than the ~35-terminal corpus Python's `wcwidth` maintains.

### What is genuinely open, as of 2026-07-21

| # | Open item | Resolve by | Blocking? |
|---|---|---|---|
| 1 | Syntax highlighting: tree-sitter-via-lumis vs syntect + two-face | **Spike** — measure static binary size for 20 languages; confirm lumis exposes raw spans | No — either path ships; affects quality ceiling and binary size |
| 2 | Ghostty's kitty-protocol *coverage* for virtual placements, Unicode placeholders and deletion | **Spike** — drive a real Ghostty session; no agent in this project has done so | Yes for images and math rendering |
| 3 | RaTeX's real coverage against its unreproduced ">99.5% KaTeX" claim | **Spike** — run its 1,050-case corpus, plus behaviour below 16 px, transparent backgrounds, dark-background antialiasing | No — txm fallback exists |
| 4 | CommonMark parser effort, concentrated in the emphasis delimiter-run algorithm and link reference definitions | Estimate during phase decomposition | No |

**Method caveat carried from both workflow passes:** verifier agents exhausted their 200/200 WebSearch budget before running adversarial counter-evidence sweeps and fell back to primary-source reading. Claims that are readings of a specified document are sound. Claims of the form *"nobody has done X"* are weak — including the "no pure-Rust in-process math renderer exists" differentiator above, which came from a targeted follow-up agent rather than a workflow pass, and which rests on checking six named tools rather than an exhaustive survey.
