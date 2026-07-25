# Plan: stele viewer features — navigation, search, links, and the frame budget
**Created:** 2026-07-25
**Status:** ready
**Complexity:** complex
**Review cadence:** 2
---
## Context

stele renders markdown well but you can only scroll it: `j/k/d/u/f/b/g/G`, three CLI flags, no way to find anything, jump anywhere, follow a link, or reload a changed file. Twelve capabilities close that gap: incremental search (`/`, `n`/`N`), heading jump (`]]`/`[[`), TOC overlay (`t`), `--watch` auto-reload, link following (`Tab`/`Shift-Tab` to select, `Enter` to open, `Backspace` to go back), mouse wheel scroll and click-to-open, code-block copy via OSC 52 (`y`), runtime width and theme toggles (`+`/`-`, `T`), `stele -` from stdin, a status line with position percentage and `Ctrl-G` file info, and section folding under a heading.

Separately, a measure-first audit found frames 40× slower than necessary on code-heavy viewports (2,525 µs vs 64 µs, from re-running tree-sitter on every visible code line every frame) and ~100 ms stalls when a large image scrolls back into view (a one-frame raster grace whose arithmetic is documented as unusable at `crates/stele/src/media/sink.rs:1221`). Those costs stay invisible while scrolling is the only interaction and become obvious once search, folding, and a link stack raise repaint frequency. Per the user's call, each perf fix ships **inside the feature phase whose interaction exposes it**, rather than as a separate up-front phase.

## Constraints

- Ghostty is the only terminal target. No sixel, no iTerm2, no other-terminal backend. Graphics stay gated on `TERM_PROGRAM=ghostty` and disabled under `TMUX`.
- `#![forbid(unsafe_code)]` in every crate; `crates/stele` keeps `deny` with its single commented signal-handler opt-out.
- PDF-viewer model holds: full parse, retained layout tree, viewport. Nothing streams.
- No user theme files — the theme toggle switches between the two built-in variants only.
- Dependency direction stays one-way; nothing may depend on `stele`.
- Existing invariants hold: no rendered line exceeds the layout width; tests re-measure through the width engine rather than trusting `Run.width`; every colored theme role stays distinct after 256-downsampling; heading levels stay above WCAG AA.
- Conventions in `docs/code-standards.md` apply — exhaustive matches with no wildcard arms, hand-rolled error enums, sentence-style test names, DW-tagged tests for plan requirements, shared pty harness under `crates/stele/tests/common/`.

## Chosen Approach

**Mode enum inside `AppState`** — extend the existing `handle_key_event` dispatch with an explicit `Mode` (Normal, Search, Toc, LinkSelect, and a fold-aware Normal), each phase adding variants rather than plumbing. **Rationale:** it matches the codebase's exhaustive-match discipline (a new mode becomes a compile error everywhere it must be handled), keeps all interaction state in one inspectable struct the existing tests already drive, and adds no dynamic dispatch to the paint path, which is about to get faster rather than slower. **Fallback:** if the mode match in `handle_key_event` outgrows readability around Phase 5, extract per-mode key handlers as free functions taking `&mut AppState` — a mechanical split that changes no state ownership.

## Rejected Approaches

- **Overlay stack of `Box<dyn Overlay>`:** cleaner separation per feature, but introduces dynamic dispatch and a second paint path into a codebase whose painter is a single tight loop, and it hides interaction state from the existing state-machine tests.
- **Separate view layer with an event router:** the right shape for a much larger TUI, but it would require moving scroll/anchor state out of `AppState` — a refactor of working, well-tested code that none of these twelve features actually needs.

---
## Implementation Phases

### Phase 1: Status line, viewport chrome, and runtime toggles
**Model:** sonnet
**Skills:** code-foundations:cc-routine-and-class-design, code-foundations:code-clarity-and-docs
**Gate:** Full

**Goal:** Reserve a status row and give the viewport a message channel, then use it to ship the runtime width (`+`/`-`) and theme (`T`) toggles and `Ctrl-G` file info.

**Scope:**
- IN: a reserved bottom row painted every frame (position percentage, document name, transient messages); `Ctrl-G` file info; `+`/`-` width adjustment within `LayoutConfig`'s clamp; `T` toggling the built-in dark/light variant; scroll-anchor preservation across every relayout these cause; `BufWriter` around stdout in `run_session`; ASCII fast path in `WidthEngine::display_width`.
- OUT: search prompt UI (Phase 4), overlay modes (Phase 3), mouse (Phase 6).

**Constraints:** The status row must reduce the content viewport height, not overpaint content. The ASCII fast path must be exercised by the pinned Ghostty width corpus, not only by tests that measure through the same fast path.
**Edge cases:** Terminal one or two rows tall (status row must degrade, not panic); width toggle at the clamp boundary; theme toggle while an image is placed (placements must survive or be re-placed); a panic between `BufWriter` fill and flush must not leave a half-frame after the terminal restore.
**Depends on:** none | **Unlocks:** Phase 2, Phase 3, Phase 4, Phase 5, Phase 6
**File scope:** `crates/stele/src/painter.rs`, `crates/stele/src/app.rs`, `crates/stele/src/main.rs`, `crates/stele/src/terminal.rs`, `crates/width/src/**`, `crates/stele/tests/**`
**Produces:** `AppState::set_status(StatusMessage)` + `AppState::status() -> StatusLine` (position %, name, transient message with a frame-count TTL); `Painter` reserves the last row; `AppState::relayout_preserving_anchor(&LayoutContext, LayoutConfig)` — the entry point every later phase calls after a width, theme, fold, or reload change.

**Approach notes:** User chose interleaved perf: the two paint/layout-cost items land here because the toggles are what force repeated relayout.
**File hints:** `crates/stele/src/app.rs` — `relayout`/`apply_resize_burst` already preserve the anchor; `crates/width/src/engine.rs:68` — `display_width`; `crates/width/corpus/ghostty-1.3.1-widths.json` — the independent width oracle.

**Done when:**
- [ ] DW-1.1: A rendered frame reserves exactly one status row; content height is `rows - 1` and no content line is overpainted.
- [ ] DW-1.2: The status row shows scroll position as a percentage that reads 0% at the top and 100% at `max_scroll`.
- [ ] DW-1.3: `Ctrl-G` shows file name, byte size, and line count; the message clears after a bounded number of frames.
- [ ] DW-1.4: `+`/`-` change content width within `LayoutConfig`'s clamp and the top visible block stays the top visible block.
- [ ] DW-1.5: `T` swaps theme variant; every heading level still clears WCAG AA in the new variant.
- [ ] DW-1.6: stdout is wrapped in a `BufWriter`; a forced panic mid-frame still restores the terminal with no buffered bytes emitted afterward.
- [ ] DW-1.7: `display_width` returns identical results to the pre-change path for every entry in the pinned Ghostty corpus, and a committed benchmark shows the ASCII path at least 2× faster than the pre-change baseline on ASCII-only input recorded in the same harness.

**Difficulty:** MEDIUM
**Uncertainty:** Whether reserving a row disturbs existing kitty placement rows; the media sink's placement math may need the reduced viewport height threaded through.

### Phase 2: Document sourcing — stdin and `--watch`
**Model:** sonnet
**Skills:** code-foundations:cc-defensive-programming, code-foundations:aposd-designing-deep-modules
**Gate:** Full

**Goal:** Turn "the document" into a source abstraction that covers a file path, stdin, and a reloadable file, then ship `stele -` and `--watch`.

**Scope:**
- IN: `stele -` reading markdown from stdin with key input taken from `/dev/tty`; `--watch` polling the file's mtime through the existing `event::poll` timeout and reloading on change; scroll-anchor preservation across reload; parse-once fix in `mermaid::preprocess`; `Rc`-shared `Document` between the app and `GfxMediaSink`.
- OUT: watching directories or included files; following links (Phase 6).

**Constraints:** No new filesystem-watcher dependency — poll mtime on the existing event loop timeout. Reload must not tear a frame: parse and lay out fully, then swap.
**Edge cases:** stdin is not a terminal and `/dev/tty` is unavailable (fail with a clear message, not a hang); `--watch` combined with `-` (watching stdin is meaningless — reject at CLI parse); file deleted or replaced mid-session; file truncated to empty; reload while the document is scrolled past the new end.
**Depends on:** Phase 1 | **Unlocks:** Phase 5, Phase 6
**File scope:** `crates/stele/src/loader.rs`, `crates/stele/src/cli.rs`, `crates/stele/src/main.rs`, `crates/stele/src/media/**`, `crates/stele/src/decor/mermaid.rs`
**Produces:** `DocumentSource::{Path(PathBuf), Stdin}` with `load(&self) -> Result<LoadedDocument, LoadError>` and `changed_since(&self, Instant) -> bool`; `LoadedDocument` carrying `Rc<Document>` so the media sink shares rather than clones.

**Approach notes:** Perf interleave: parse-once and the `Document` share both live on the load path, so they ship with the phase that rebuilds it.
**File hints:** `crates/stele/src/loader.rs` — existing load + error type; `crates/stele/src/decor/mermaid.rs:25` — the redundant `Document::parse`; `crates/stele/src/main.rs:136` — the `doc.clone()` into the sink.

**Done when:**
- [ ] DW-2.1: `stele -` renders markdown piped on stdin and still responds to keys.
- [ ] DW-2.2: `--watch` re-renders within one poll interval of an external write, preserving the anchored block.
- [ ] DW-2.3: `--watch -` is rejected at CLI parse with a message naming the conflict.
- [ ] DW-2.4: A deleted or unreadable file under `--watch` shows a status-line error and keeps the last good render instead of exiting.
- [ ] DW-2.5: A document with no mermaid fences is parsed exactly once at startup (asserted by instrumenting the parse count, not by timing).
- [ ] DW-2.6: The media sink holds a shared `Rc<Document>`; no full AST clone occurs at startup.

**Difficulty:** MEDIUM
**Uncertainty:** Whether crossterm's event source can be pointed at `/dev/tty` cleanly when stdin is a pipe.

### Phase 3: Heading navigation — jump, TOC overlay, and image residency
**Model:** fable
**Skills:** code-foundations:aposd-designing-deep-modules, code-foundations:performance-optimization
**Gate:** Full

**Goal:** Build the heading outline once at layout time, expose `]]`/`[[` jumps and a full-screen TOC overlay on it, and make the image raster survive the fast viewport movement those jumps create.

**Scope:**
- IN: an `Outline` of headings (level, text, anchor block) derived from the layout tree; `]]`/`[[` jumping to the next/previous heading; `Mode::Toc` full-screen overlay listing headings with selection and `Enter` to jump, `Esc`/`t` to dismiss; fixing the raster grace arithmetic documented as unusable at `sink.rs:1221`; a byte-budgeted LRU for decoded rasters; two-stage image downscale.
- OUT: fuzzy filtering of the TOC (out of scope for this plan — the query-entry machinery lands in Phase 4, but filtering the TOC with it is not a deliverable here); folding (Phase 5).

**Constraints:** Residency and visibility stay separate concepts — the sink's module doc records the bug that came from conflating them. The 32-placement cap must not silently cap raster retention; if residency records share the placements map, split them. Re-measure the scroll-back cost after fixing the off-by-one before building the LRU — the audit's 99–102 ms may be largely that bug.
**Edge cases:** Document with zero headings (both jumps and the overlay must no-op with a status message); a heading as the very first or last block; TOC longer than the screen (must scroll); overlay entered while images are placed (every placement must be taken down and restored correctly); a terminal too short to render the overlay.
**Depends on:** Phase 1 | **Unlocks:** Phase 5
**File scope:** `crates/stele/src/media/**`, `crates/gfx/src/decode.rs`, `crates/layout/src/**`, `crates/stele/src/app.rs`
**Produces:** `Outline { entries: Vec<OutlineEntry { level: u8, text: String, block: NodeId }> }` with `AppState::outline() -> &Outline` and `AppState::jump_to_block(NodeId)`; `Mode::Toc { selected: usize }` handled in `handle_key_event`.

**Approach notes:** Perf interleave: jumps are exactly what move large images on and off screen, so the residency work ships here. The user was told the off-by-one may be most of the win — measure before building the LRU.
**File hints:** `crates/stele/src/media/sink.rs:112` — `DATA_GRACE_FRAMES`; `sink.rs:1221` — the documented off-by-one; `sink.rs:403` — the 32-placement cap sharing the placements map; `crates/gfx/src/decode.rs:287` — the Triangle resize (entry point at `:168`).

**Done when:**
- [ ] DW-3.1: `]]`/`[[` move to the next/previous heading and no-op with a status message in a document with none.
- [ ] DW-3.2: `t` opens a scrollable TOC listing every heading with its level; `Enter` jumps to the selected one; `Esc` returns to the prior scroll position.
- [ ] DW-3.3: Entering and leaving the TOC leaves exactly the placements the returning frame paints — no stale image survives.
- [ ] DW-3.4: An image scrolled off-screen and back within the residency budget is re-placed without re-transmitting its pixel data.
- [ ] DW-3.5: A committed benchmark shows the scroll-back return frame for a 6000×6000 image at least 10× faster than the pre-change baseline recorded in the same harness.
- [ ] DW-3.6: Raster retention is governed by the byte budget, not by the 32-placement cap; a document with more than 32 images still retains rasters up to the budget.

**Difficulty:** HIGH
**Uncertainty:** How much of the 99–102 ms is the off-by-one versus genuine decode cost; the two-stage downscale's benefit at the real ~2400 px target is unmeasured.

### Phase 4: Incremental search
**Model:** fable
**Skills:** code-foundations:cc-control-flow-quality, code-foundations:performance-optimization
**Gate:** Full

**Goal:** Add `/` query entry with literal smart-case matching, `n`/`N` traversal, and match highlighting — and make the repaint it triggers per keystroke cheap by caching syntax highlighting across frames.

**Scope:**
- IN: `Mode::Search` reading a query into the status row; literal matching, case-insensitive unless the query contains an uppercase character; `Enter` to accept, `Esc` to cancel and restore position; `n`/`N` to cycle matches; highlighting every visible match plus a distinct style for the current one; memoized `Decor::highlight` results keyed by line text and language.
- OUT: regex, whole-word, or multi-line matching; search across folded-away content (Phase 5 defines fold semantics).

**Constraints:** Matching runs over the laid-out line text so a match is addressable by (line, column range) for highlighting; a match spanning a wrap boundary highlights on both lines. The highlight cache must be bounded and must not cache a result produced by the highlighter's timeout fallback.
**Edge cases:** Empty query; query with no matches (status message, position unchanged); wrapping past the last match back to the first; a match inside a code block that is also syntax-highlighted (search highlight must win); a query containing multi-byte graphemes.
**Depends on:** Phase 1 | **Unlocks:** Phase 5
**File scope:** `crates/stele/src/app.rs`, `crates/stele/src/painter.rs`, `crates/stele/src/decor/**`, `crates/highlight/src/**`
**Produces:** `SearchState { query: String, matches: Vec<Match { block: NodeId, line: usize, range: Range<usize> }>, current: usize }`; `Semantic::SearchMatch` and `Semantic::SearchCurrent` style roles resolved by both decor paths.

**Approach notes:** Perf interleave: search repaints on every keystroke, so the highlight cache ships here.
**File hints:** `crates/stele/src/painter.rs:361` — `paint_run` calling `Decor::highlight` per run per frame; `crates/highlight/src/highlighter.rs:39` — the 250 ms timeout whose fallback must not be cached.

**Done when:**
- [ ] DW-4.1: `/` opens a query prompt in the status row; typing updates it; `Esc` restores the pre-search scroll position.
- [ ] DW-4.2: Matching is case-insensitive for an all-lowercase query and case-sensitive once the query contains an uppercase character.
- [ ] DW-4.3: `n`/`N` cycle forward and backward through matches and wrap at both ends.
- [ ] DW-4.4: Every visible match is highlighted and the current match is visually distinct from the others.
- [ ] DW-4.5: A query with no matches leaves the viewport unmoved and reports so in the status row.
- [ ] DW-4.6: Two adjacent frames over identical code-block content invoke the underlying syntax highlighter once, not twice.
- [ ] DW-4.7: A committed benchmark shows code-heavy frame time at least 10× lower than the pre-change baseline recorded in the same harness.
- [ ] DW-4.8: Both new style roles stay distinct from every existing role after 256-color downsampling.

**Difficulty:** HIGH
**Uncertainty:** Whether match addressing survives a relayout at a new width without recomputation.

### Phase 5: Section folding
**Model:** sonnet
**Skills:** code-foundations:cc-control-flow-quality, code-foundations:aposd-verifying-correctness
**Gate:** Standard

**Goal:** Let a heading's section collapse to a single marked line and expand again, with scroll position, images, and search results all staying correct across the change.

**Scope:**
- IN: a fold toggle on the heading at or above the cursor position; a folded heading rendering with a marker and a hidden-line count; `zR`/`zM`-style expand-all and collapse-all; fold state surviving relayout; placements inside a folded range being evicted and restored on unfold.
- OUT: persisting fold state across restarts; folding non-heading blocks.

**Constraints:** Fold state is keyed by `NodeId`, not by line index, so it survives a width change or reload. A folded range must contribute exactly one line to the layout, and no rendered line may exceed the layout width.
**Edge cases:** Folding the last heading in a document; nested headings (folding H2 inside a folded H1); folding while scrolled inside the range being folded (viewport must move to the fold marker); folding a range containing a placed image; a search match inside a folded range (`n` must expand it or skip it — pick one and state it in the status row).
**Depends on:** Phase 1, Phase 2, Phase 3, Phase 4 | **Unlocks:** none
**File scope:** `crates/stele/src/app.rs`, `crates/layout/src/block.rs`, `crates/stele/src/painter.rs`
**Produces:** `FoldState { collapsed: HashSet<NodeId> }` consulted during layout walk; `AppState::toggle_fold()`, `expand_all()`, `collapse_all()`.

**Approach notes:** Section extent comes from the `Outline` built in Phase 3 — a section runs to the next heading of equal or shallower level.
**File hints:** `crates/layout/src/block.rs:256` — where headings enter the layout walk; `crates/layout/src/block.rs:40` — the per-line block tag folding uses to find a range.

**Done when:**
- [ ] DW-5.1: Toggling a fold collapses its section to one marked line showing the hidden-line count, and restores it exactly on re-toggle.
- [ ] DW-5.2: Fold state survives a width change and a `--watch` reload, keyed by node rather than line.
- [ ] DW-5.3: Folding a range containing a placed image removes that placement; unfolding restores it.
- [ ] DW-5.4: Collapse-all leaves exactly one line per top-level heading; expand-all restores the full document.
- [ ] DW-5.5: Folding while scrolled inside the folded range leaves the viewport at the fold marker, never past the end.
- [ ] DW-5.6: No folded or unfolded line exceeds the layout width, re-measured through the width engine.

**Difficulty:** MEDIUM
**Uncertainty:** Whether fold-aware layout is cheaper as a layout-walk filter or a post-layout line filter.

### Phase 6: Mouse, link following, and clipboard
**Model:** fable
**Skills:** code-foundations:cc-defensive-programming, code-foundations:ca-architecture-boundaries
**Gate:** Full
**Security-sensitive:** yes

**Goal:** Make the document actionable — wheel scrolling, `Tab`-cycled link selection with `Enter` to open, a document stack with `Backspace`, click-to-open, and `y` to copy a code block via OSC 52.

**Scope:**
- IN: crossterm mouse capture with wheel scroll and click; `Mode::LinkSelect` cycling visible links with `Tab`/`Shift-Tab`; `Enter` opening the selected link; relative markdown opening in-place via a document stack with `Backspace` to return; any other resolvable local file opened the same way; `http`/`https` URLs handed to the OS opener; `y` copying the code block under the selection to the clipboard via OSC 52.
- OUT: editing; opening non-`http(s)` URL schemes; following links inside folded ranges without expanding them first.

**Constraints:** The user explicitly chose the permissive link policy (relative markdown **and** arbitrary local files **and** external URLs), so the hardening carries the safety rather than the scope: invoke the OS opener with argv, never a shell; allowlist `http`/`https` schemes only; detect binary content before rendering an opened file and refuse with a status message; cap the size of an opened file; resolve and canonicalize paths before use. Mouse capture must be toggleable, since it takes over terminal text selection.
**Edge cases:** A link target that does not exist, is a directory, is a device file or FIFO, or is unreadable — the file-type check must happen *before* any read, since binary-detection-by-read can block forever on a character device or FIFO. A URL containing shell metacharacters or a newline; an OSC 52 payload larger than the terminal accepts; a click on a cell with no link; a document stack many levels deep, and `Backspace` at the root; a link that points at the document already open.

**Traversal and symlinks are ALLOWED, not refused.** The user chose arbitrary local files, so a `../` path or a symlink pointing outside the document's directory canonicalizes and opens like any other target. There is no directory jail. DW-6.5 guarantees only that no such path escapes into a shell or an unintended process invocation — it is not a containment claim, and a builder must not turn it into one by refusing these targets.
**Depends on:** Phase 1, Phase 2 | **Unlocks:** none
**File scope:** `crates/stele/src/app.rs`, `crates/stele/src/terminal.rs`, `crates/stele/src/loader.rs`, `crates/highlight/src/hyperlink.rs`
**Produces:** `Mode::LinkSelect { index: usize }`; `LinkTarget::{LocalDoc(PathBuf), LocalFile(PathBuf), Url(String)}` with `resolve(&self, base: &Path) -> Result<LinkTarget, LinkError>`; `DocumentStack` with `push(DocumentSource)` / `pop()`.

**Approach notes:** Link following is keyboard-first (`Tab` cycle + `Enter`) because stele has no cursor concept; mouse click is the second path to the same action, not the only one. The permissive policy is a stated user decision — do not narrow it, harden it.
**File hints:** `crates/stele/src/painter.rs:396` — `Run.aux` already carries link targets for OSC 8; `crates/highlight/src/hyperlink.rs` — existing `sanitize_url`; `crates/stele/tests/dw_7_6_hostile_links.rs` — the existing hostile-link corpus to extend.

**Done when:**
- [ ] DW-6.1: `Tab`/`Shift-Tab` cycle the links visible in the viewport with a visible selection indicator; `Enter` activates the selected one.
- [ ] DW-6.2: A relative markdown link opens in place and `Backspace` returns to the previous document at its previous scroll position.
- [ ] DW-6.3: An `http`/`https` link is handed to the OS opener via argv with no shell involved; a non-`http(s)` scheme is refused with a status message.
- [ ] DW-6.4: A link target that is missing, unreadable, a directory, a device file or FIFO, oversized, or binary is refused with a status message and leaves the current document rendered — with the file-type check performed before any read.
- [ ] DW-6.5: A crafted link containing shell metacharacters, newlines, or `../` traversal is handled without executing anything or escaping into an unintended process invocation. This is a no-process-escape guarantee only; traversal targets that resolve to readable files still open, per the chosen policy.
- [ ] DW-6.8: A link to a resolvable local file that is **not** markdown (e.g. a `.txt` or `.rs` file) opens in place and joins the document stack, exactly as a markdown target does — the permissive policy's third leg, which must not be dropped.
- [ ] DW-6.6: Mouse wheel scrolls the viewport; a click on a link activates it; a click on a non-link cell does nothing; mouse capture can be toggled off.
- [ ] DW-6.7: `y` emits a well-formed OSC 52 sequence carrying exactly the selected code block's text.

**Difficulty:** HIGH
**Uncertainty:** Which OS-opener invocation is correct across platforms without adding a dependency; whether Ghostty's OSC 52 acceptance has a payload ceiling worth chunking around.

---
## Test Coverage
**Level:** 100%

## Test Plan

- [ ] Unit: a rendered frame reserves exactly one status row and content occupies `rows - 1` with nothing overpainted (DW-1.1); dirty — a terminal one and two rows tall.
- [ ] Unit: status-line percentage at scroll 0, mid-document, and `max_scroll` (DW-1.2); dirty — a document shorter than the viewport where `max_scroll` is 0.
- [ ] Unit: `Ctrl-G` reports file name, byte size, and line count, and the message clears after its frame TTL (DW-1.3); dirty — a file with no trailing newline and a zero-byte file.
- [ ] Unit: width and theme toggles preserve the anchored block across relayout (DW-1.4, DW-1.5); dirty — toggle at both clamp boundaries.
- [ ] Unit: ASCII fast path equals the slow path across the pinned Ghostty corpus (DW-1.7); dirty — control bytes and multi-byte graphemes must not take the fast path.
- [ ] Integration: forced panic mid-frame restores the terminal with no buffered bytes after (DW-1.6), through the existing pty harness.
- [ ] Integration: `stele -` renders piped markdown and responds to keys (DW-2.1); dirty — stdin piped with no `/dev/tty` available.
- [ ] Integration: `--watch` reload preserves the anchored block (DW-2.2); dirty — file deleted, truncated to empty, and replaced mid-session (DW-2.4).
- [ ] Unit: `--watch -` rejected at CLI parse (DW-2.3).
- [ ] Unit: parse count is exactly 1 for a fence-free document (DW-2.5); sink holds a shared `Rc` (DW-2.6).
- [ ] Unit: outline built from a document with headings at every level (DW-3.1); dirty — zero headings, and a heading as the first and last block.
- [ ] Integration: TOC open/jump/dismiss leaves only the placements the returning frame painted (DW-3.2, DW-3.3); dirty — terminal too short for the overlay.
- [ ] Integration (extending the existing residency test at `crates/stele/src/media/sink.rs:1227`): an image scrolled off-screen and back within the residency budget is re-placed with no re-transmission of pixel data (DW-3.4); dirty — an image evicted past the budget re-transmits rather than placing a raster the terminal no longer holds.
- [ ] Benchmark (committed): image scroll-back return frame ≥10× faster than baseline (DW-3.5); raster retention respects the byte budget past 32 images (DW-3.6).
- [ ] Unit: `/` opens the query prompt, typing updates it, `Enter` accepts, and `Esc` restores the pre-search scroll position (DW-4.1); dirty — `Esc` on an empty query, and `Enter` on an empty query.
- [ ] Unit: smart-case behavior at the case boundary (DW-4.2); dirty — empty query, a no-match query that leaves the viewport unmoved and reports so (DW-4.5), and a query of multi-byte graphemes.
- [ ] Unit: `n`/`N` wrap at both ends (DW-4.3); match highlighting wins over syntax highlighting inside a code block (DW-4.4).
- [ ] Unit: highlighter invoked once across two identical frames (DW-4.6); dirty — a timed-out highlight result is not cached.
- [ ] Benchmark (committed): code-heavy frame ≥10× faster than baseline (DW-4.7). Unit: new style roles stay distinct after downsampling (DW-4.8).
- [ ] Unit: fold round-trip restores the document exactly (DW-5.1); fold state keyed by node survives width change and reload (DW-5.2).
- [ ] Unit: collapse-all then expand-all is identity (DW-5.4); dirty — folding while scrolled inside the range (DW-5.5), and nested headings.
- [ ] Integration: folding a range containing a placed image evicts and restores the placement (DW-5.3).
- [ ] Unit: no folded line exceeds the layout width, re-measured through the width engine (DW-5.6).
- [ ] Unit: link cycling and activation (DW-6.1); document stack push/pop restores scroll position (DW-6.2).
- [ ] Integration: a non-markdown local file target opens in place and joins the document stack (DW-6.8); a `../` or symlinked target that resolves to a readable file also opens rather than being refused.
- [ ] Unit: scheme allowlist accepts `http`/`https` and refuses everything else (DW-6.3).
- [ ] Unit (dirty, extending `dw_7_6_hostile_links.rs`) — targets that must be REFUSED with the current document left rendered: missing, unreadable, a directory, a device file or FIFO (type-checked before any read, so the test must prove no hang), oversized, binary content (DW-6.4). Targets that must be ACCEPTED: `../` traversal and symlinks resolving to readable files (DW-6.8). Targets that must neither execute nor spawn anything: shell metacharacters, embedded newlines, non-`http(s)` schemes, self-referential links (DW-6.5).
- [ ] Integration: wheel scroll, click-to-activate, click on a non-link cell, capture toggle (DW-6.6).
- [ ] Unit: OSC 52 payload carries exactly the code block's bytes (DW-6.7); dirty — a block larger than any plausible terminal ceiling.
- [ ] Manual (Ghostty, once per phase): the feature behaves in a real session — the pty harness proves emission, not perception.

## Assumptions

| Assumption | Confidence | Verify Before Phase | Fallback If Wrong |
|---|---|---|---|
| crossterm can read key events from `/dev/tty` while stdin is a pipe | Medium | Phase 2 | Reject `stele -` with an explanatory error and ship stdin as read-only-until-EOF |
| mtime polling on the existing `event::poll` timeout is responsive enough for `--watch` | High | Phase 2 | Add a filesystem-watcher dependency as a last resort |
| Most of the 99–102 ms scroll-back stall is the documented grace off-by-one | Medium | Phase 3 | Build the byte-budgeted LRU as originally specced; re-measure before and after |
| Match positions survive relayout without full recomputation | Low | Phase 4 | Recompute matches on relayout — correctness over the optimization |
| Ghostty accepts OSC 52 clipboard writes without user configuration | Medium | Phase 6 | Report failure in the status line; document the Ghostty setting |
| The two-stage downscale still helps at the real ~2400 px target | Low | Phase 3 | Drop the two-stage step; keep the residency fix, which is measured |

## Decision Log

| Decision | Alternatives Considered | Rationale | Phase |
|---|---|---|---|
| Mode enum in `AppState` | Overlay stack of trait objects; separate view layer + event router | Matches exhaustive-match discipline; no dynamic dispatch in the paint path; existing state tests keep working | All |
| Perf fixes interleaved per feature | Perf-first phase; perf-last cleanup | User's explicit call; the four items with no natural feature owner (BufWriter, ASCII fast path, parse-once, `Rc` share) were assigned to the phase whose interaction exposes them | 1, 2, 3, 4 |
| Literal smart-case search | Case-sensitive literal; regex | User choice; matches less/vim expectations with no new dependency | 4 |
| Permissive link policy, hardened rather than narrowed | Relative markdown only; markdown + URLs | User's explicit choice with consequences stated; safety comes from argv invocation, scheme allowlist, binary detection, and size caps | 6 |
| Full-screen TOC overlay | Side panel; fuzzy jump prompt | No live layout-width change, so no re-placement of every reserved image box | 3 |
| `Tab`-cycled link selection | Cursor/caret model; vimium-style hint labels | stele has no cursor concept; a cycle is far less machinery and works identically for mouse and keyboard | 6 |

---
## Notes

- The status row in Phase 1 reduces the content viewport height, which the media sink's placement math consumes — verify reserved image boxes still land correctly before building on it.
- Phase 3 must re-measure after fixing the grace off-by-one at `sink.rs:1221` before investing in the LRU; the audit's number may be mostly that bug.
- The audit's numbers were taken with a scratch harness against a dirty tree. Phases 1, 3, and 4 each commit a benchmark, so later regressions are detectable — this is the first committed perf harness in the workspace.
- The light-variant palette has a pre-existing near-collision (two greens 6.0 rgb units apart) unrelated to this work. Out of scope; noted so it is not mistaken for a regression introduced here.
- Phases share `crates/stele/src/app.rs`, so file scopes overlap and the build runs effectively serially. That is expected, not a defect in the decomposition.

---
## Execution Log
_To be filled during /code-foundations:build_
