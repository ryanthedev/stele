# Plan: stele — terminal markdown viewer for Ghostty
**Created:** 2026-07-21
**Status:** complete — all 7 phases built and committed on `feature/stele-markdown-viewer`; only Phase 1 has an on-disk review artifact (see Execution Log)
**Started:** 2026-07-21 23:05
**Current Phase:** 7 (done; post-phase fix/coverage work and a 15-defect bug bash followed — see Execution Log)
**Completed:** 2026-07-23 23:22 (Phase 7 commit `602b2d1`); post-phase hardening ran through 2026-07-24 22:16 (`00d4e9d`)
**Complexity:** complex
---
## Context

Every terminal markdown renderer is a batch string transformer: width baked in at render time, images as alt text, Unicode width measured with tables that disagree with the displaying terminal. None is a document viewer — none retains a layout tree, reflows on resize, or renders math. stele is a terminal markdown viewer for Ghostty that behaves like a PDF viewer: open a complete document, lay it out, scroll it, resize it, and see images, tables, and formulas rendered correctly.

Grounding research (verified findings, falsification results, scope decisions S1–S5): `.code-foundations/research/2026-07-20-tui-markdown-renderer.md` and `2026-07-21-tui-markdown-renderer-part2.md`.

## Constraints

- Rust; single static binary; no runtime external dependencies.
- Raw Ghostty is the only must-work target (S3). Multiplexers degrade to alt text; Alacritty is not a target.
- Documents at rest — full parse, retained layout tree, viewport, reflow on resize; **no incremental parse** (S4).
- Content width clamps to a min/max range (S2): **defaults min 24 / max 100 cells**, max overridable via `--max-width`; effective width = clamp(terminal width, min, max). The mechanism is the contract; the defaults are tunable.
- Own: parser, layout tree, table auto-layout, kitty emission, Ghostty width-correction layer, theme/color mapping. Adopt: `unicode-width`/`unicode-segmentation`, `image`, RaTeX (+txm fallback), a Mermaid crate (chosen by Spike B), highlight engine per spike (S5, amended: no differ exists under approach C).
- **Document text is hostile on every path** — parse (P2), decode (P6), and emission (P5/P7): C0/C1 bytes in content must never reach the terminal raw, and untrusted URLs must never be interpolated into escape sequences unsanitized.
- Binary viewer first; modular internals; no public API design up front.
- S1 standing assumption: no existing tool does this (not re-litigated).

## Chosen Approach

**C — retained layout, immediate paint.** Layout tree retained; each frame repaints the visible viewport slice inside a mode-2026 synchronized-update block; no cell-grid differ exists. Deletes an entire owned subsystem and its known hard part (differ-survives-resize), and Unicode-placeholder images become ordinary cells repainted like text. **Fallback:** approach A (retained cell grid + owned damage-tracking differ) — contained to the paint layer; the layout tree is identical in both.

**Provenance honesty:** mode 2026 is a *plan-introduced* mechanism — it appears in neither research document, and part 2's build-readiness table recommended adopting ratatui's differ instead. Approach C supersedes that recommendation on the argument that a differ saves bytes nothing needs saved at document-viewer frame rates. The mechanism is unverified until DW-1.2; a negative verdict triggers the approach-A fallback.

## Rejected Approaches

- **A — full retained pipeline with owned differ:** the differ saves bytes nothing needs saved at document-viewer frame rates; largest owned-code chunk with the worst-known hard part. Retained as fallback.
- **B — ratatui-hosted widget:** fights every research finding — no document layout, resize resets the diff baseline to full repaint, image placement outside its model.

---
## Implementation Phases

### Phase 1: Bootstrap + spikes
**Model:** sonnet
**Skills:** code-foundations:cc-defensive-programming (resolved at build SETUP — the probe harness is an external-I/O boundary whose defining edge case is timeout handling against a non-answering terminal)
**Gate:** Standard

**Goal:** Stand up the workspace and CI, and resolve every unverified dependency the architecture rests on as written verdicts before anything is built on them.

**Scope:**
- IN: cargo workspace + CI (fmt, clippy -D warnings, test). PTY-driven Ghostty probe harness (`crates/probe`), reusable by P3/P6. **Spike A (Ghostty capabilities):** live verdicts for kitty `a=q` query; chunked direct transmission; virtual placement `U=1` + Unicode placeholders; deletion `a=d,d=i`; mode 2026; mode 2027 default state; cell-geometry sources (`ReportCellSize`, `CSI 14t`/`16t`, `TIOCGWINSZ` pixel fields — which answer, and agreement); OSC 10/11 background query; kitty emission while crossterm holds raw mode (coexistence). **Spike B (engines):** stripped-binary size for lumis (the 20 target languages, Notes) vs syntect+two-face; lumis raw-span API confirmed/refuted; decision thresholds: ≤30 MB with spans → lumis, ≥100 MB or no spans → syntect, between → recorded judgment. Mermaid crate evaluation (text-grid output preferred — see Decision Log), naming crate + output form. **Spike C (RaTeX):** 1,050-case corpus reproduction; <16 px, transparency, dark-background checks; adopt/hedge/fallback verdict.
- OUT: any production rendering code.

**Edge cases:** probe read timeouts (a non-answering terminal must not hang the harness); CI has no Ghostty (spike results are committed artifacts, not CI steps).

**Depends on:** none | **Unlocks:** Phase 2, 3, 5, 6, 7 (2–3 directly; 5–7 consume spike verdicts and the probe harness)
**File scope:** `Cargo.toml`, `rust-toolchain.toml`, `.github/**`, `docs/spikes/**`, `crates/probe/**`
**Produces:** `docs/spikes/ghostty-caps.md`, `highlight-engine.md`, `ratex.md` — each ends in a decision block (capability → verdict → consequence) that P5/P6/P7 consume verbatim. `crates/probe` as a cross-phase seam: `Probe::open(GhosttyPty) -> Probe`; `query(&mut self, seq: &[u8], timeout: Duration) -> Option<Vec<u8>>`; `cursor_pos(&mut self) -> (u16, u16)`; `measured_width(&mut self, s: &str) -> u16` (consumed by P3 corpus, P5 frame assertions, P6 placement assertions).

**Done when:**
- [ ] DW-1.1: Workspace builds; CI green on fmt, clippy `-D warnings`, test; release binary passes a linkage assertion (no dynamic dependencies beyond the platform's system libs — `otool -L`/`ldd` check in CI).
- [ ] DW-1.2: `ghostty-caps.md` records a measured live-Ghostty verdict for all nine capability items above.
- [ ] DW-1.3: `highlight-engine.md` names the highlight engine (thresholds applied, sizes recorded, raw-span verdict) and the Mermaid crate with its output form.
- [ ] DW-1.4: `ratex.md` records reproduced corpus pass rate and the three rendering checks.
- [ ] DW-1.5: Probe harness drives a real Ghostty session via PTY with per-probe timeout.

**Difficulty:** MEDIUM
**Uncertainty:** Ghostty verdicts can invalidate approach C (mode 2026) or the image model (placeholders) — that is why this phase exists first.

### Phase 2: CommonMark+GFM parser
**Model:** fable
**Skills:** code-foundations:aposd-designing-deep-modules, code-foundations:cc-pseudocode-programming, code-foundations:cc-defensive-programming
**Gate:** Full

**Goal:** An owned parser from untrusted markdown text to a typed, span-carrying AST, measured against the official CommonMark conformance suite.

**Scope:**
- IN: CommonMark 0.31.2 block + inline parsing; GFM tables, strikethrough, task lists, autolinks; extension *syntax* behind AST nodes: `$`/`$$` math, footnotes, GitHub alerts, YAML frontmatter, fence info strings. Byte-range source spans on every node. A minimal AST→HTML serializer used only by the conformance harness. The shared `fixtures/` corpus (markdown documents spanning the feature surface) that P4/P7 goldens extend.
- OUT: rendering of any kind; raw HTML interpretation (preserved as literal text nodes); smart punctuation.

**Constraints:** `#![forbid(unsafe_code)]`. Input is untrusted: recursion depth cap, allocation caps, linear-time guarantees on pathological delimiter nesting.
**Edge cases:** emphasis delimiter-run algorithm; link reference definitions (defined after use); tab expansion; entity references; list/blockquote laziness; CRLF; NUL; deeply nested constructs; quadratic-backtracking inputs.

**Depends on:** Phase 1 | **Unlocks:** Phase 4, 6, 7
**File scope:** `crates/ast/**`, `fixtures/**`
**Produces:** `crates/ast`: `Document::parse(&str) -> Document` (infallible — worst case is text), typed `Block`/`Inline` enums, `Span { start: usize, end: usize }` on every node.
**Security-sensitive:** yes

**Approach notes:** Conformance is measured through the AST→HTML shim against the vendored spec tests — that shim exists for testability only and is not a product surface.

**Done when:**
- [ ] DW-2.1: ≥649/652 CommonMark spec tests pass; every deviation documented with rationale.
- [ ] DW-2.2: GFM extension test suites (tables, strikethrough, task lists, autolinks) pass.
- [ ] DW-2.3: 1 hour of cargo-fuzz: no panic, no OOM, no >1s single-input parse.
- [ ] DW-2.4: Spans reconstruct the exact source slice for every node in the fixture corpus.
- [ ] DW-2.5: `forbid(unsafe_code)` holds crate-wide (CI-checked).

**Difficulty:** HIGH
**Uncertainty:** The last ~10 conformance cases are where solo parsers stall; cap accepted deviations at 3.

### Phase 3: Width engine
**Model:** sonnet
**Skills:** code-foundations:aposd-designing-deep-modules
**Gate:** Standard

**Goal:** A deep, tiny width module — grapheme clusters in, Ghostty-correct cell counts out — hiding UCD tables, the correction layer, and every Unicode trap behind three functions.

**Scope:**
- IN: grapheme iteration via `unicode-segmentation`; base width via `unicode-width`; owned Ghostty correction layer (VS15/VS16, ZWJ sequences, regional-indicator pairs, Fitzpatrick modifiers); configurable ambiguous-width (1 or 2); verification corpus measured against live Ghostty via the P1 probe harness.
- OUT: mode 2027 negotiation (the viewer renders against Ghostty's measured default behavior); any terminal other than Ghostty.

**Edge cases:** flag pairs (2 cells, GB12/GB13); VS16 promoting narrow→wide and VS15 width-neutral; family ZWJ sequences; combining marks; Hangul jamo; zero-width characters; tabs (policy owned by layout, not width).

**Depends on:** Phase 1 | **Unlocks:** Phase 4
**File scope:** `crates/width/**`
**Produces:** `crates/width`: `WidthEngine::new(WidthConfig { ambiguous_wide: bool }) -> WidthEngine`; `WidthEngine::cluster_width(&self, &str) -> u16`; `WidthEngine::display_width(&self, &str) -> usize`; free fn `graphemes(&str) -> impl Iterator<Item = &str>` (segmentation is config-independent).

**Approach notes:** Gate deviation, recorded: P3 creates a cross-phase seam, but in a greenfield workspace every phase does — Full is reserved for seams carrying untrusted data or architectural risk. `crates/width` is a pure function library; Standard.

**Done when:**
- [ ] DW-3.1: 100% agreement with live-Ghostty measured widths over a ≥200-case corpus (CJK, ZWJ emoji, flags, VS15/16, combining, Hangul), committed as an artifact pinned to the Ghostty version.
- [ ] DW-3.2: Property test: `display_width` equals the sum of `cluster_width` over `graphemes` for arbitrary strings.
- [ ] DW-3.3: No cluster reports width >2; zero-width classes report 0.

**Difficulty:** MEDIUM
**Uncertainty:** Ghostty updates can shift measured widths — corpus results pin the Ghostty version recorded in `ghostty-caps.md`.

### Phase 4: Layout engine
**Model:** fable
**Skills:** code-foundations:aposd-designing-deep-modules
**Gate:** Full

**Goal:** A pure, deterministic layout function from AST + width to a retained tree of line boxes — the central seam of the system and the thing no existing tool has.

**Scope:**
- IN: block layout (headings, paragraphs, lists, blockquotes, code blocks, rules); inline wrapping over grapheme clusters using `crates/width`; table auto-layout (min/max content widths, css-tables-3 §3.9.3 distribution; comfy-table's algorithm as reference); width clamped per the Constraints range; reserved boxes sized through the `IntrinsicSizer` seam (below) — the default `NullSizer` yields alt-text boxes, so P4 is complete without P6; styled runs carry semantic style IDs (theme resolution happens at paint).
- OUT: painting, ANSI emission, scrolling, graphics.

**Edge cases:** table exceeding max width after distribution (overflow ladder: wrap-in-cell → per-column floors → clip with indicator); unbreakable words longer than the line (break-anywhere fallback); code blocks wider than viewport (clip + indicator, no wrap); nested-list indent consuming the width budget; images wider than viewport (scale-to-fit against sizer dimensions); empty document.

**Depends on:** Phase 2, Phase 3 | **Unlocks:** Phase 5, 6
**File scope:** `crates/layout/**`, `fixtures/**` (extending the P2 corpus with layout goldens)
**Produces:** `crates/layout`: `layout(&Document, width: u16, &LayoutConfig, &WidthEngine, &dyn IntrinsicSizer) -> LayoutTree`; `trait IntrinsicSizer { fn size(&self, node: NodeId, doc: &Document) -> Option<CellSize> }` with provided `NullSizer` (`NodeId` is the single node-identity type across P4/P5/P6 — P6's cache is keyed by it at sizing time); `LayoutTree::lines(Range<usize>) -> impl Iterator<Item = &Line>`; `Line = Vec<Run { text, style_id: StyleId, width }> | Reserved { node_id: NodeId, cols, rows }`; `enum StyleId { Semantic(Semantic), Capture(u16) }` — layout emits only `Semantic`; the `Capture` range is allocated by `crates/highlight` (P7); `LayoutConfig { min_width: u16 /*24*/, max_width: u16 /*100*/ }` (ambiguous-width policy lives in `WidthEngine`, P3).

**Approach notes:** Reflow is re-layout from the retained AST, never from source — enforced by the signature taking `&Document`. The `IntrinsicSizer` seam is how P6 feeds image/math dimensions in without touching this crate. Security-sensitive deliberately **not** set (recorded deviation, mirroring P3's): P4 consumes the typed AST from barricaded P2 and emits nothing — injection is impossible here; the DoS surface (pathological tables) is covered by DW-4.5's time budget.

**Done when:**
- [ ] DW-4.1: Deterministic: identical AST + width + sizer → structurally identical tree (hash-compared).
- [ ] DW-4.2: Golden snapshots pass at width 24 and width 100 for the fixture corpus.
- [ ] DW-4.3: No fixture table (including pathological) exceeds the width bound.
- [ ] DW-4.4: `layout(ast, w2, …)` equals a fresh parse+layout at w2 for all fixtures.
- [ ] DW-4.5: 1 MB document lays out in <100 ms (release).

**Difficulty:** HIGH
**Uncertainty:** The overflow ladder's middle rungs may need tuning against real documents; the ladder order is the contract, the thresholds are not.

### Phase 5: Viewport + paint
**Model:** fable
**Skills:** code-foundations:cc-routine-and-class-design, code-foundations:cc-defensive-programming
**Gate:** Full

**Goal:** The binary: open a file, scroll it, resize it — whole-viewport immediate paint inside mode-2026 synchronized frames, no differ — with the paint boundary as the terminal-injection barricade.

**Scope:**
- IN: CLI: `stele <file.md>`, `--max-width <n>` (clamps per Constraints), plus flag plumbing for later phases (`--no-images`, `--frontmatter` parsed here, consumed by P6/P7); alt screen, raw mode, terminal restore on exit *and* panic (hook + Drop); keys: arrows, PgUp/PgDn, Home/End, g/G, q; SIGWINCH → debounce → re-layout at new width → scroll clamp preserving topmost visible block; paint = mode 2026 begin → cursor home → viewport lines as SGR runs + clear-to-EOL → end; **sanitization barricade: run text is stripped of C0/C1 bytes at the paint boundary — the only escapes on the wire are the painter's own**; structural styles only (theme engine arrives in P7); crossterm plumbing; hook seams for P6/P7: `trait MediaSink` and `trait Decor` defined here with no-op defaults in `src/media/mod.rs` and `src/decor/mod.rs` stubs.
- OUT: images, math (P6); highlighting, theme, extensions (P7); search; TOC.

**Edge cases:** resize burst during paint; document shorter than viewport; width below min (clamp + horizontal clip); unreadable/missing file (clean error, nonzero exit); invalid UTF-8 (refuse with message); EINTR; document text containing raw ESC/OSC/APC bytes (must render inert).

**Depends on:** Phase 1, Phase 4 | **Unlocks:** Phase 6, 7
**File scope:** `crates/stele/**` (including the `src/media/`, `src/decor/` stubs; P6/P7 later own those subtrees)
**Produces:** Working binary; paint seam `Painter::frame(&LayoutTree, scroll: usize, size: Size)`; hook trait contracts (P6/P7 build against these exactly):
```rust
trait MediaSink {
    fn paint(&mut self, reserved: &Reserved, rect: CellRect, out: &mut dyn Write);
    fn evict(&mut self, node_id: NodeId, out: &mut dyn Write);
}
trait Decor {
    fn highlight(&self, line_text: &str, lang: Option<&str>) -> Vec<Run>;
    fn resolve(&self, style_id: StyleId) -> Style;
}
```
Registration: `Painter::register_media(Box<dyn MediaSink>)`, `Painter::register_decor(Box<dyn Decor>)`. Defaults in the stubs: MediaSink's is a true no-op; Decor's is a **structural resolver** (identity `highlight`, structural-style `resolve` table — this is what P5 paints with). P5 re-exports the paint-facing types (`Run`, `StyleId`, `Style`); the painter recomputes `Run.width` after `highlight`, so `Decor` impls leave it unset.
**Security-sensitive:** yes

**Approach notes:** crossterm adopted for undifferentiated plumbing (user-approved deviation-in-spirit from S5); coexistence with raw kitty emission is verified in Spike A, and the fallback is rustix + owned input parsing.

**Done when:**
- [ ] DW-5.1: Open/scroll/quit works; terminal state restored on quit and on induced panic (PTY test).
- [ ] DW-5.2: Full scroll of a 10k-line document: every frame wrapped in paired 2026 markers (PTY capture asserts); manual visual pass confirms no tearing.
- [ ] DW-5.3: Resize storm (50 SIGWINCH/s for 5 s): no crash, final layout correct, topmost visible block preserved.
- [ ] DW-5.4: Missing file and invalid UTF-8 produce clean errors and nonzero exit.
- [ ] DW-5.5: Injection fixture (raw ESC/OSC/APC/DECSET bytes in document text) renders inert — PTY capture contains no non-painter escapes.
- [ ] DW-5.6: `--max-width 60` clamps content width to 60 on a wider terminal (PTY assertion).

**Difficulty:** MEDIUM
**Uncertainty:** Debounce interval and scroll-anchor policy may need feel-tuning; the invariants (no tear, anchor preserved, inert content) are the contract.

### Phase 6: Images + math
**Model:** fable
**Skills:** code-foundations:cc-defensive-programming
**Gate:** Full

**Goal:** Inline graphics on Ghostty — owned kitty emission, adopted decode, RaTeX math — with a verified degradation ladder everywhere graphics can't go.

**Scope:**
- IN: owned kitty emission (chunked ≤4096 base64, `f=100` PNG, image IDs, placement mode per `ghostty-caps.md` verdict — virtual `U=1` + Unicode placeholders if verified, else direct placement + repaint-behind); placement lifecycle: delete (`a=d,d=i`) on scroll-out beyond a one-viewport margin, cap 32 live placements; `image` crate decode with `Limits`; **`ImageSizer` implementing P4's `IntrinsicSizer`** via header-only dimension probe (no full decode at layout time), then re-layout; pre-scale to cell geometry from the Spike A-verified source (`ReportCellSize` or fallback per verdict); math: RaTeX → PNG cached by (content-hash, px-size), txm Unicode-grid fallback (user-forceable), literal-source last resort; alt-text degradation on `$TMUX`, probe failure, or `--no-images`; local file paths only.
- OUT: animation; remote (http) images; SVG.

**Constraints:** Every decode input is hostile until proven otherwise. Failure of any rung lands on the next rung, never on a crash or a blank region.
**Edge cases:** malformed/hostile image files; decompression-bomb dimensions; missing image path; RaTeX parse failure; cell-geometry change mid-session (re-rasterize, reuse em-unit layout); scroll across partially visible images; transparency on dark background (per `ratex.md`).

**Depends on:** Phase 1, Phase 2, Phase 4, Phase 5 | **Unlocks:** —
**File scope:** `crates/gfx/**`, `crates/math/**`, `crates/stele/src/media/**`, `crates/stele/src/main.rs` (wiring)
**Produces:** `crates/gfx`: `Emitter::{transmit(&mut self, id: ImageId, png: &[u8], out: &mut dyn Write), place(&mut self, id: ImageId, rect: CellRect, out: &mut dyn Write), delete(&mut self, id: ImageId, out: &mut dyn Write)}` (per-call writer, mirroring `MediaSink::paint`); `crates/math`: `render(tex: &str, px_height: u32) -> Result<Png, MathError>`, `render_text(tex: &str) -> Option<TextGrid>`; `ImageSizer: IntrinsicSizer`; `MediaSink` impl registered in the P5 stub.
**Security-sensitive:** yes

**Done when:**
- [ ] DW-6.1: Image renders in live Ghostty and survives 100 scroll cycles; emission log asserts create/delete balance under the eviction policy.
- [ ] DW-6.2: Math fixture set renders; each rung of RaTeX→txm→literal verified by injected failure.
- [ ] DW-6.3: Hostile-image fixtures (malformed PNG/JPEG, bomb dimensions): no crash, alt text shown, memory stays under cap.
- [ ] DW-6.4: With `$TMUX` set: alt-text path, zero graphics escapes emitted.
- [ ] DW-6.5: Cell-size change re-rasterizes without re-layout (em-cache hit asserted).

**Difficulty:** HIGH
**Uncertainty:** Placement mode depends wholly on the Spike A verdict; the repaint-behind fallback weakens scroll behavior — if artifacts prove unacceptable, that is the same approach-A escalation as a negative mode-2026 verdict (Assumptions rows 1–2).

### Phase 7: Highlighting + extensions
**Model:** fable
**Skills:** code-foundations:cc-routine-and-class-design, code-foundations:cc-defensive-programming
**Gate:** Full

**Goal:** The headline feature surface: spike-chosen highlight engine under an owned theme layer, Mermaid, and the GFM-adjacent extensions — with untrusted URLs and fence bodies treated as hostile.

**Scope:**
- IN: engine per `highlight-engine.md`; owned theme layer mapping captures/scopes → `Style` with truecolor and 256-color downsample; built-in dark + light themes via OSC 10/11 (Spike A-verified; fallback dark); the 20-language fixture set (Notes); Mermaid via the Spike B crate rendered as a text block (fallback: plain code fence); OSC 8 hyperlinks with **URL sanitization: scheme allowlist (http/https/file/mailto), C0/C1 and `;`-framing bytes stripped before interpolation**; task-list glyphs; footnotes (end section, OSC 8 fragment anchors); GitHub alerts (styled gutter); frontmatter hidden by default (`--frontmatter` shows).
- OUT: user theme files; language auto-detection without an info string.

**Edge cases:** unknown language tag → plain block; pathological code block (highlight time cap → plain); Mermaid parse failure → code fence; downsample collisions between theme roles; nested emphasis + strikethrough merge; NO_COLOR; hostile URLs (javascript:, embedded ESC/BEL/ST).

**Depends on:** Phase 1, Phase 2, Phase 5 | **Unlocks:** —
**File scope:** `crates/highlight/**`, `crates/mermaid/**`, `crates/stele/src/decor/**`, `crates/stele/src/main.rs` (wiring), `fixtures/**` (extending the P2 corpus with highlight/extension goldens)
**Produces:** Full feature surface; `crates/highlight`: `Theme::resolve(&self, id: StyleId) -> Style` (covers both `Semantic` and `Capture` variants; the crate allocates the `Capture` range); `Decor` impl registered in the P5 stub.
**Security-sensitive:** yes

**Approach notes:** Token splitting is paint-side: code blocks never wrap (P4 clip policy), so `Decor::highlight(line_text, lang) -> Vec<Run>` is a pure run transformation that cannot change layout — no `crates/layout` hook needed.

**Done when:**
- [ ] DW-7.1: 20-language golden SGR snapshots pass.
- [ ] DW-7.2: 256-color mode: theme roles downsample to distinct colors; NO_COLOR yields structural styles only.
- [ ] DW-7.3: Mermaid fixture renders; induced parse failure falls back to code fence.
- [ ] DW-7.4: Footnote refs emit OSC 8 fragment anchors whose targets match back-reference anchors 1:1 (emission assertion; terminal-side navigation is out of scope).
- [ ] DW-7.5: Alerts, task lists, frontmatter render per fixtures.
- [ ] DW-7.6: Hostile-URL fixtures: disallowed schemes and embedded control bytes never reach an OSC 8 sequence (PTY capture assertion).

**Difficulty:** MEDIUM
**Uncertainty:** Engine choice is Spike B's verdict; theme-layer API is stable either way (capture names vs scope names differ only inside `crates/highlight`).

---
## Test Coverage
**Level:** 100% — defined as: every DW item across all phases maps to at least one test-plan item below.

**Ghostty-dependent tests** (DW-1.2, DW-3.1, DW-6.1, and the manual passes) run locally via `just verify-ghostty` against a live Ghostty and produce **committed artifacts** (verdict docs, corpus results, emission logs); CI asserts artifact presence and freshness pins, plus all terminal-independent layers.

## Test Plan
- [ ] T-1: CI pipeline green on clean checkout incl. linkage assertion (DW-1.1); the three spike docs exist with parseable decision blocks (DW-1.2/1.3/1.4); probe harness times out cleanly against a non-answering PTY (dirty, DW-1.5).
- [ ] T-2: Vendored CommonMark suite ≥649/652 (DW-2.1); GFM suites (DW-2.2); 1h fuzz — panics, OOM, >1s inputs all zero (dirty, DW-2.3); span-reconstruction property test (DW-2.4); CI check that `forbid(unsafe_code)` is present (DW-2.5); pathological nesting + backtracking fixtures parse in linear time (boundary).
- [ ] T-3: 200-case live-Ghostty width corpus 100%, artifact committed (DW-3.1); concat/sum property test (DW-3.2); zero-width and >2 bounds (boundary, DW-3.3); corpus run under both ambiguous=1 and ambiguous=2 configs; degenerate-cluster fixtures — lone combining marks, unpaired regional indicators, interrupted ZWJ sequences, isolated VS15/16 (dirty).
- [ ] T-4: Determinism hash test (DW-4.1); golden snapshots at widths 24 and 100 (boundary, DW-4.2); pathological table fixtures never exceed width (dirty, DW-4.3); reflow-equals-fresh-layout property (DW-4.4); 1 MB perf budget (DW-4.5); unbreakable-word and empty-document fixtures (dirty); NullSizer yields alt-text boxes (seam).
- [ ] T-5: PTY open/scroll/quit + panic-restore (DW-5.1); 2026 frame-pairing assertion over full scroll (DW-5.2 automated half); resize-storm soak (dirty, DW-5.3); missing-file/invalid-UTF-8 error paths (dirty, DW-5.4); ESC/OSC/APC injection fixture renders inert (dirty, DW-5.5); `--max-width` clamp on a wide PTY (DW-5.6); width-below-min clamp (boundary).
- [ ] T-6: Scroll-cycle placement create/delete balance (DW-6.1); math failure-ladder injection (dirty, DW-6.2); hostile-image fixture set under memory cap (dirty, DW-6.3); `$TMUX` zero-escape assertion (DW-6.4); cell-size-change em-cache hit (DW-6.5); image at exactly viewport width (boundary).
- [ ] T-7: 20-language SGR goldens (DW-7.1); 256/NO_COLOR downsample (DW-7.2); Mermaid fallback injection (dirty, DW-7.3); OSC 8 anchor-matching assertion (DW-7.4); extension fixtures (DW-7.5); hostile-URL and control-byte fixtures (dirty, DW-7.6); unknown-language and highlight-time-cap paths (dirty).
- [ ] Manual: side-by-side visual pass in live Ghostty per phase 5–7 (tearing check for DW-5.2's manual half, image scroll, theme legibility dark + light).

---
## Assumptions

*Documentation audit (2026-07-24): every row's "Verify Before Phase" has now passed (P1–P7 are all shipped). Added column records what the shipped code/commit messages actually show, mined from the P1 spike verdicts and the P6/P7 commit messages and discovery docs — original columns left unedited below.*

| Assumption | Confidence | Verify Before Phase | Fallback If Wrong | Actual Outcome (post-hoc) |
|---|---|---|---|---|
| Ghostty supports virtual placements + Unicode placeholders + deletion | MED | P1 (blocks P6) | Direct placements + repaint-behind; if scroll artifacts unacceptable → approach-A escalation (row 2's fallback) | Partially falsified. P1's `ghostty-caps.md` verdict: `U=1` is sequence-accepted but **visually unverified** (no live pass run). P6 took the row's own stated first fallback — direct placement + repaint-behind — not virtual placement, and not the deeper approach-A escalation. No manual visual pass had run as of the P6 commit; `080e410` (post-phase) made the DW-6.1 pass "actually observable," but whether it was ever executed against a live GUI is not established from the repo (see "could not establish" below). |
| Ghostty supports mode 2026 | MED-HIGH | P1 (blocks P5) | Approach A: owned differ at the paint layer | Confirmed true (P1 verdict); approach C shipped as designed, no differ, no fallback triggered. |
| Ghostty answers a usable cell-geometry query (ReportCellSize, `CSI 14t/16t`, or TIOCGWINSZ pixels) | MED | P1 (blocks P6) | Assume default cell aspect from font metrics flag `--cell-size`; degrade image scaling quality | Partially falsified, not fallen back. OSC 1337 ReportCellSize was **unanswered**; `CSI 16t` answered instead and P6 used it — satisfies the assumption's own "or" clause, so no degraded-quality fallback was needed. |
| RaTeX corpus claim reproduces | MED | P1 (blocks P6 math) | ReX (git dep) or txm-only | Confirmed: 1048/1048 corpus reproduced. Not adopted as-is, though — `LayoutOptions.color` defaults to black at 1.24:1 contrast on dark backgrounds and had to be overridden explicitly (P1 verdict, carried into P6's hardcoded ink color). |
| lumis exposes raw spans at acceptable binary size | MED | P1 (blocks P7) | syntect + two-face; accept ~2-year grammar staleness | Threshold technically missed, judgment call made anyway. lumis measured ~31 MiB — over the plan's own ≤30 MB clean-adopt line — recorded in the P1 summary as "a judgment call inside the 30-100 MB band," and adopted rather than falling back to syntect+two-face (measured 2.14 MB, recorded only as "the revisit option"). |
| A Spike B Mermaid crate emits usable text-grid output | MED | P1 (blocks P7 mermaid) | Render mermaid fences as plain code blocks in v1 | Partially falsified. mermaid-text 0.57.0 was adopted, but the P7 discovery doc records the spike's claim of ASCII-pure output as a "documented false-purity gap... empirically false for decision-node diamonds" — P7 had to route around it rather than use the crate's advertised path as-is. Did not fall back to plain code blocks. |
| Solo parser reaches ≥649/652 conformance | HIGH | P2, continuously | Document deviations; hard cap 3 | Exceeded: 652/652, zero documented deviations (P2 commit message). |
| css-tables-3 §3.9.3 distribution ports to cell units | HIGH | P4 | comfy-table's iterative algorithm as reference implementation | Confirmed — P4 shipped the distribution with DW-4.2 goldens passing; no evidence the comfy-table fallback was needed. |
| crossterm coexists with raw kitty emission on the same tty | MED | P1 (Spike A) | rustix + owned input parsing (Ghostty-only keeps it tractable) | Confirmed — P5 shipped on crossterm and P6 emits raw kitty sequences through the same session; no fallback triggered. |
| Em-unit caching keeps re-rasterize cheap on geometry change | MED | P6 (DW-6.5) | Longer debounce + async re-rasterize | Confirmed — DW-6.5 shipped with an asserted cache-hit test (P6 commit message); no fallback triggered. |

## Decision Log

| Decision | Alternatives Considered | Rationale | Phase |
|---|---|---|---|
| Approach C: retained layout, immediate paint, no differ — plan-introduced, supersedes research's adopt-ratatui-differ recommendation | A (owned differ), B (ratatui) | Differ saves bytes nothing needs at document rates; 2026 + placeholder cells make repaint atomic and image-safe; verdict-gated by DW-1.2 | P5 |
| Width range defaults min 24 / max 100, `--max-width` override | Fixed width; unbounded | Readability ceiling + graceful floor; mechanism is the contract, defaults tunable | P4 |
| Own the parser | comrak, pulldown-cmark | Conformance suite bounds the work; span-carrying AST owned by layout | P2 |
| Raw HTML preserved as literal text | HTML subset rendering | No HTML engine in scope; honest rendering beats partial | P2 |
| Viewer does not enable mode 2027 | DECSET 2027 | Measured default Ghostty behavior is the contract; child processes reset the mode silently | P3 |
| P3 gate Standard despite new seam (deviation) | Full per seam rule | Greenfield: every phase creates seams; Full reserved for untrusted-data or architectural-risk seams | P3 |
| `IntrinsicSizer` trait seam for image/math dimensions | P6 patches layout crate; dimensions in AST | Keeps layout pure and P4 complete without P6; header-only probe avoids decode at layout time | P4/P6 |
| Sanitization barricade at paint boundary; URL allowlist at OSC 8 emission | Strip at parse; trust content | Painter is the single choke point where all text meets the wire (cc-defensive barricade) | P5/P7 |
| Overflow ladder for tables (wrap → floors → clip) | Always-wrap; always-clip | Matches CSS practice; degrades progressively | P4 |
| crossterm for terminal plumbing | Owned rustix I/O layer | Undifferentiated, bounded; Spike A verifies coexistence | P5 |
| P5 paints structural styles only; theme engine lands in P7 | Theme in P5 | Keeps P5 shippable and P7's engine choice isolated | P5/P7 |
| Mermaid: text-grid-output crate preferred | Image-output mermaid via gfx | Avoids P7→gfx coupling; image mermaid deferred | P7 |
| Images: local paths only in v1 | http fetch | Network + TLS + cache is scope creep on a security-sensitive phase | P6 |
| Built-in dark/light themes only, OSC 10/11 selected | User theme files | YAGNI for v1; theme layer API doesn't preclude it | P7 |

---
## Notes

- **The 20 target languages** (Spike B measurement set and DW-7.1 goldens): rust, python, javascript, typescript, go, c, cpp, java, csharp, ruby, swift, kotlin, zig, bash, json, yaml, toml, html, css, sql.
- **Replanning after P5 is expected, not failure** — the user chose everything-in-one-plan knowing later phases are specced against an unvalidated core; P6/P7 bodies should be re-read against the as-built seams before dispatch. *Documentation audit (2026-07-24): no distinct replanning event or document exists on disk — there is no post-P5 revision to this plan file's P6/P7 bodies, and no separate replan artifact in `.code-foundations/`. What did happen, per the P6 and P7 discovery docs, is that each build agent's own Discovery step read the as-built P4/P5 seams directly (e.g. P6's discovery cites `crates/stele/src/media/mod.rs`'s pinned `MediaSink` signature as "not renegotiable"; P7's documents which seams it could not change) and adjusted scope within-phase rather than through a separate planning pass — arguably satisfying the spirit of this note but not literally "replanning" as a tracked activity. P7 additionally needed an out-of-scope orchestrator patch (see Phase 7 Execution Log entry) to actually wire fence-language and link-URL data through the frozen P4/P5 `Run`/`Style` schema — a seam gap this note anticipated in kind (P6/P7 specced against an unvalidated core) but which surfaced as a wiring fix after P7's build rather than as a plan revision before it.*
- The emphasis delimiter-run algorithm and link reference definitions are the parser's hard 20% — budget P2 accordingly.
- The PTY probe harness (P1) is a standing verification asset: P3's width corpus, P5's injection/frame assertions, and P6's placement assertions all run through it.
- Research doc originals remain in `~/repos/scratch/.code-foundations/research/`; the copies here are canonical from now on.
- Build-wave note: P2∥P3 and P6∥P7 are DAG-parallel, but P2/P6/P7 carry Full gates, which build does not co-schedule — expect effectively serial execution. The shared `main.rs` wiring lines in P6/P7 scopes are conflict-free under that serialization.

---
## Execution Log

### Phase 1: Bootstrap + spikes (Gate: Standard)
- [x] BUILD: Discovery + design + implementation (stub → implement → validate) complete
- [x] REVIEW: Verification passed (attempt 2 — attempt 1 FAILed on a demonstrated silent-failure defect in `Launcher::run_probe`, fixed and regression-tested)
- [x] Committed
Commit: efd2633
Summary: Cargo workspace, CI (fmt/clippy -D warnings/test/linkage), and `crates/probe` — a PTY harness that drives a real Ghostty window via `open -na Ghostty.app --args -e` (note: `ghostty -e` hangs) with per-probe timeouts. All three spikes ran live and produced measured verdict docs in `docs/spikes/`. **Verdicts that change downstream phases: mode 2026 is supported (approach C confirmed, no differ needed); mode 2027 is ON by default, contradicting the research's source-reading (P3 must build its corpus against 2027-ON and pin the Ghostty version); OSC 1337 ReportCellSize is unanswered — use `CSI 16t` for cell geometry (P6); kitty deletion is silent, so P6's create/delete balance must assert from the client send-log; virtual placement `U=1` is sequence-accepted but visually unverified, needing one manual pass in P6; highlight engine is lumis 0.12.0 (~31 MiB, judgment call inside the 30-100 MB band — syntect+two-face measured 2.14 MB and is recorded as the revisit option); Mermaid is mermaid-text 0.57.0 emitting a real box-drawing grid; RaTeX adopted on 1048/1048 corpus, but `LayoutOptions.color` MUST be set explicitly — the library default is black at 1.24:1 contrast on dark backgrounds.**

*Note (documentation audit, 2026-07-24): the branch's current `git log` shows this phase's commit as `df5a423`, not `efd2633`. `efd2633` still exists as a commit object in this repo but is not on `feature/stele-markdown-viewer`'s current history — likely superseded by a rebase/amend after this line was written. Left as originally recorded rather than silently rewritten; flagging the discrepancy rather than guessing which sha is authoritative.*

### Phase 2: CommonMark+GFM parser (Gate: Full)
- [x] BUILD: Discovery + design + implementation (stub → implement → validate) complete
- [ ] REVIEW: no review artifact found on disk (`.code-foundations/build/*-phase-2-review*.md` does not exist) — unrecorded, not asserted as passed
- [x] Committed
Commit: 7e8b417
Summary: An owned two-phase parser (block structure via an open-block stack, then inline parsing with a delimiter stack) from untrusted markdown to a typed, span-carrying AST. Conformance: 652/652 CommonMark examples (exceeds the DW-2.1 ≥649/652 target, zero documented deviations) against the vendored 0.31.2 suite, measured by exact-string-equality HTML comparison (stricter than the official normalizing runner); GFM tables 8/8, autolinks 11/11, task lists 2/2, strikethrough 2/2. `#![forbid(unsafe_code)]` crate-wide; `Document::parse` infallible; container/bracket depth capped at 200, tree depth at 700, iterative (non-recursive) flatten/conversion/traversal. Commit message records gate-review fixes attributed to "a single what-breaks judge, GATE PASS" (no separate review doc on disk): hard-break detection was miscounting entity-decoded trailing spaces against spec 2.5; the 999-char link-label limit was counting bytes instead of characters, wrongly rejecting non-ASCII labels; a doc comment claiming `None` for foreign node ids was corrected; the HTML shim was marked `#[doc(hidden)]`. One open, low-severity item: whether a list item should interrupt an open table is untested by the vendored suite.

### Phase 3: Width engine (Gate: Standard)
- [x] BUILD: Discovery + design + implementation (stub → implement → validate) complete
- [ ] REVIEW: no review artifact found on disk (`.code-foundations/build/*-phase-3-review*.md` does not exist) — unrecorded, not asserted as passed
- [x] Committed
Commit: 996d324
Summary: A pure three-function module (`cluster_width`/`display_width`/`graphemes`) hiding UCD tables and the Ghostty correction layer. DW-3.1 backed by a real live measurement: 206 cases driven through a real Ghostty 1.3.1 GUI window via the P1 probe harness, cursor-delta measured, committed as `corpus/ghostty-1.3.1-widths.json` pinned to the Ghostty version; the measurement binary never links the engine, so the agreement test compares independent sources. The live run corrected two first-draft assumptions, both recorded in `correction.rs`: U+2661+VS16 measures 1 not 2 (VS16 does not promote a base character with Emoji=NO — gated on the UCD Emoji property via `unicode-properties` rather than applying unconditionally), and a standalone U+200D with nothing to join measures 0 not 2 (the ZWJ/Fitzpatrick override now applies only to multi-codepoint clusters). DW-3.2/3.3 covered by proptest properties over arbitrary strings.

### Phase 4: Layout engine (Gate: Full)
- [x] BUILD: Discovery + design + implementation (stub → implement → validate) complete
- [ ] REVIEW: no review artifact found on disk (`.code-foundations/build/*-phase-4-review*.md` does not exist) — unrecorded, not asserted as passed
- [x] Committed
Commit: 6e281cf
Summary: The central seam — a pure, deterministic layout function from `Document` + `WidthEngine` to a retained line-box tree. Block layout, inline wrapping over grapheme clusters, table auto-layout with the css-tables-3 §3.9.3 distribution, and the overflow ladder (wrap-in-cell → per-column floors → clip-with-indicator). Images/math enter only through the `IntrinsicSizer` seam; `NullSizer` yields alt-text boxes so P4 stands alone. DW-4.1 determinism, DW-4.2 goldens at width 24 and 100 (38 human-reviewed snapshots), DW-4.3 width-bound, DW-4.4 reflow-equals-fresh-layout, DW-4.5 1 MB in ~50 ms release (budget 100 ms). `#![forbid(unsafe_code)]`. Commit message records a gate-review verdict of "initial GATE FAIL → PASS" (again, no separate review doc on disk): the crate's core invariant ("no line exceeds the layout width") was breakable by constructible input — a token wider than 65,535 cells wrapped around under `display_width(..) as u16` to a small number, slipping the break-anywhere/clip guards and, separately, panicking debug builds via overflow in table-width arithmetic. Fixed by doing width decisions in `usize` and saturating at the `u16` seam. Non-blocking notes recorded as still open: tab expansion is fixed 4-space not tab-stop; `Reserved` rows carry no prefix gutter (deferred to P5/P6 — this became one of the bug-bash's fixes, see closing section below); ambiguous-width header-rule alignment is cosmetic.

### Phase 5: Viewport + paint (Gate: Full)
- [x] BUILD: Discovery + design + implementation (stub → implement → validate) complete
- [ ] REVIEW: no review artifact found on disk (`.code-foundations/build/*-phase-5-review*.md` does not exist) — unrecorded, not asserted as passed
- [x] Committed
Commit: 32f9ce2
Summary: The binary — open, scroll, resize, repaint the whole viewport inside mode-2026 synchronized frames (no differ). crossterm plumbing, alt screen + raw mode with restore on exit and panic, key handling, debounced SIGWINCH → re-layout → scroll clamp. The paint boundary is the terminal-injection barricade: every run's text is stripped of control code points before reaching the wire. Hook seams (`MediaSink`, `Decor`) defined here with no-op/structural defaults. DW-5.1 through DW-5.6 all covered. `#![forbid(unsafe_code)]`, zero unsafe. Commit message records gate-review fixes (again attributed to a single what-breaks judge, GATE PASS, no separate doc on disk) including a raw-mode leak on a failed alt-screen write, an in-session error message getting wiped by the terminal restore, a Drop-vs-panic-hook double-restore race, and — significant for defensive posture — that the DW-5.5 injection barricade test as originally written was materially looser than advertised (accepted any CSI `...h/l/H/K/m`, so an injected `?1049h` passed; only scanned ESC-opened sequences, missing raw C1/BEL). Tightened to a strict whitelist. Recorded as **not fixed, needing an orchestrator/user call**: DW-5.3 preserved scroll proportionally rather than by source block, because true block-anchoring needed a per-line `NodeId` the layout tree didn't expose — this was later closed by the post-phase commit `f0904de` (see closing section).

### Phase 6: Images + math (Gate: Full)
- [x] BUILD: Discovery + design + implementation (stub → implement → validate) complete
- [ ] REVIEW: no review artifact found on disk (`.code-foundations/build/*-phase-6-review*.md` does not exist) — unrecorded, not asserted as passed
- [x] Committed
Commit: 5054afe
Summary: Owned kitty graphics emission (chunked base64, `f=100` PNG, direct placement + repaint-behind — **not** virtual `U=1`, because the P1 spike left `U=1` visually unverified and no live-Ghostty visual pass was available in the build environment), bounded image decode, and RaTeX math with a txm-then-literal-source fallback ladder. Images/math enter layout through `ImageSizer` (header-only probe, so a decompression bomb is never decoded at layout time) and paint through the P5 `MediaSink` seam. DW-6.1 through DW-6.5 covered; DW-6.1's live-Ghostty visual pass specifically was **not executed** (no controlling terminal in this environment) — later addressed by post-phase commit `080e410`. `#![forbid(unsafe_code)]`. Commit message records "initial GATE FAIL → PASS" with three blocking findings, all confirmed against the code first: frame boundaries were inferred from `rect.y <= previous y`, so scrolling *up* (every box's y moves down) made consecutive frames look identical and an image was never re-placed, drifting permanently out of alignment with its text; eviction only ran from inside `paint()`, which `Painter` never calls for `evict`, so an image scrolled fully off-screen was never deleted for the rest of the session; deletion emitted `a=d,d=i` (drops placements, not pixel data), so every eviction leaked one orphaned copy. Also fixed: fallback text clipped by chars instead of display cells; math bounded only font size, not the projected pixmap (a 4 KiB TeX budget could still demand hundreds of MB); TeX group nesting capped at 64 to avoid stack exhaustion. Recorded as still open: a multi-row box whose top rows are scrolled off still misplaces (needs a P4/P5 seam change) — closed by post-phase commit `ce626d2`/`f0904de`-adjacent work; replies suppressed (`q=2`) so a terminal-side transmit rejection is invisible.

### Phase 7: Highlighting + extensions (Gate: Full)
- [x] BUILD: Discovery + design + implementation (stub → implement → validate) complete
- [ ] REVIEW: no review artifact found on disk (`.code-foundations/build/*-phase-7-review*.md` does not exist) — unrecorded, not asserted as passed
- [x] Committed
Commit: 602b2d1
Summary: The final phase — lumis-backed per-line syntax highlighting under an owned theme layer (truecolor + 256-color downsample, dark/light, NO_COLOR), OSC 8 hyperlinks and footnote anchors with URL sanitization, Mermaid diagrams as box-drawing grids, GitHub alerts, task lists, frontmatter hide. DW-7.1 through DW-7.6 covered. `#![forbid(unsafe_code)]`. **Notable: the P7 build agent's own discovery doc records that it could not, within its file scope, wire code-fence language or link URLs through to `Decor` — `crates/layout` discarded the fence `info` string and the painter hardcoded `highlight(text, None)`, and `Run`/`Style` had no URL channel — so as built by the phase alone, no code block would show per-language color and no link would emit OSC 8.** The commit message confirms this and records that **the orchestrator, not the phase's build agent, closed the gap outside the phase's nominal file scope**: added a single `aux: Option<Box<str>>` field to `Run` (fence language for a CodeBlock run, URL for a Link run), threaded the fence info token through `literal_block`, and wrote `hyperlink_open`'s sanitizing OSC 8 builder into the painter. Mermaid and frontmatter run as source preprocessors before parse rather than through `Decor` (`Decor::highlight` is constrained to be a pure run transformation that cannot change line/column count). Added end-to-end wiring tests (`tests/p7_wiring.rs`) the phase itself could not reach. Commit message states: "Plan complete: all 7 phases built, reviewed, and committed" — the word "reviewed" there refers to the what-breaks gate-review process described in each phase's own commit message, not to a review document on disk; no `*-phase-2-review.md` through `*-phase-7-review.md` exists in `.code-foundations/build/`, only Phase 1 has one.

### Post-phase: hardening, coverage, and the bug bash
Not phase entries — no plan phase corresponds to these; recorded here so the log accounts for every commit after Phase 7 rather than going silent.

Eleven commits (`080e410` through `25f46d5`) followed Phase 7 with targeted fixes and coverage: making the DW-6.1 live visual pass actually observable (`080e410`), block-anchored scroll on resize closing the DW-5.3 gap recorded as open in the Phase 5 summary above (`f0904de`), a stable kitty placement id fix (`579152d`), inline media riding the text baseline and formulas rendering at a fixed em letterboxed to their cell box (`ce626d2`, `8143b59`), a headless pipeline dump tool, a build-commit stamp, a display-math layout fix, an injection-barricade test against real painted bytes, and Mermaid/highlighting coverage sweeps (`9c203a8`, `5036b3e`, `3db4ff5`, `1c5a4eb`, `7cc7aba`, `25f46d5`).

The branch then closes with `00d4e9d`, "the bug bash — 15 defects, one archetype," four parallel review agents across four waves against the shipped P1–P7 tree. **Archetype named in the commit message: "a count cannot see a position"** — an invariant was asserted as a count, a balance, or a bound-that-holds-by-construction (e.g. `line.width() <= tree.width()`, true of a `Line::Reserved` by construction and therefore incapable of failing; `creates == 40, deletes == 8`, both numbers right while 8 images were simply absent) standing in for "is the output correct," so the test suite was green while the defect was visible on screen. Four representative defects, one per wave: a multi-row image whose top scrolled above the viewport painted downward over document text (P6 territory, DW-6.1's balance assertion couldn't see it); a height-only resize inside a 400-line code fence teleported the reader from line 200 to line 0 (P5 territory, DW-5.3's block-granularity assertion couldn't see it); an image inside a blockquote or list painted at column 0 with the gutter/marker missing (P4 territory, no test had ever nested a reserved box); and an LRU eviction that deleted images it had already painted within the same frame. All 15 defects and their fixes are itemized in the commit message; gate reported as fmt clean, clippy `--all-targets -D warnings` clean, 328 passed / 0 failed / 6 ignored (needing a live Ghostty GUI). The 15 findings documents themselves (~5k lines) are recorded as session scratch, not committed to the repo.

| Item | Value |
|---|---|
| Post-phase commits | 11 (`080e410`..`25f46d5`) + 1 bug-bash commit (`00d4e9d`) = 12 |
| Bug-bash defect count | 15 |
| Archetype | invariant-as-count/balance/construction-bound standing in for output correctness |
| Phases whose territory was touched by the bug bash | P4 (reserved-box column), P5 (resize anchoring), P6 (placement rows, LRU eviction) — all fixed *after* being marked done above |
| Test status at close | 328 passed / 0 failed / 6 ignored (live-Ghostty-only) |
