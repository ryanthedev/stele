# testdocs — manual visual test corpus

Documents for looking at stele in a real Ghostty window. Every gate in this
repo asserts on escape-sequence bytes rather than pixels, which is how two real
bugs shipped green: the ghost-image trail (a stacked placement emits a
perfectly balanced byte log) and inline math breaking its own sentence. These
exist to close that gap with human eyes.

**Not test fixtures.** `fixtures/` is glob-loaded by the layout golden tests —
`fixture_paths()` reads every `.md` in it — so anything added there changes
goldens. Nothing here is referenced by any test.

## Running

```
cargo build --release
./target/release/stele testdocs/01-commonmark-gfm.md
```

Check the build first. The commit sha is painted dim in the bottom-right corner
and printed by `--version`; a `-dirty` suffix means uncommitted edits. If a
screenshot and the code disagree, confirm the sha before believing either.

```
./target/release/stele --version
```

## The documents

| File | What it is for |
|---|---|
| `01-commonmark-gfm.md` | CommonMark 0.31.2 + GFM surface: headings, emphasis, lists, task lists, tables, blockquotes, alerts, links, footnotes, breaks, literal HTML, entities |
| `02-math.md` | The inline-vs-display boundary, all three degradation rungs, and prose `$5`/`$10` false positives |
| `03-media.md` | One-row-rides-baseline vs multi-row-claims-own-lines, containers, and every rejected path (remote URL, `.svg`, missing, traversal, corrupt) |
| `04-unicode-width.md` | CJK, ZWJ emoji, flags, VS15/16, combining marks, bidi, zero-width — with ASCII rulers so table misalignment is measurable |
| `05-code-highlighting.md` | All 20 enabled languages, unsupported-language fallback, clipping, inline-code edges |
| `06-hostile.md` | 27 attack classes with genuine control bytes: CSI, DECSET, OSC 52, forged kitty APCs, bidi trojan-source, hostile URLs, structural bombs |
| `07-mermaid.md` | All 18 diagram types `mermaid-text` detects, plus every fallback case |
| `08-scroll-10k.md` | 10,009 lines, 270 ordered checkpoints — scroll tearing (DW-5.2) and resize storms (DW-5.3) |

`img/` holds the media fixtures, including a deliberately corrupt `.png`.

## What is already checked automatically

`emit_dump` drives the real layout and paint pipeline headlessly and reports
both a width-bound check and an injection-barricade scan:

```
cargo run -p stele --example emit_dump -- testdocs/06-hostile.md 80
```

All eight documents currently pass both at widths 24, 40, 80, and 100. So a
human pass is confirming a result, not discovering one.

**What it cannot tell you** — and therefore what you are actually looking for:

- whether anything *looks* right: glyph size, alignment, spacing, color
- tearing or flicker during scroll and resize
- whether an image renders at all, in the right place, without a trail
- whether the terminal changes state it should not (title, colors, cursor,
  alt screen, clipboard, vanished images) while viewing `06-hostile.md`

One known blind spot in the automated scan: it consumes an OSC to its
terminator, so hostile bytes smuggled *inside* a legitimate OSC 8 URL are
swallowed rather than flagged. `hyperlink_open`'s scheme allowlist is what
defends that, and it still wants a live pass — see the hostile-URL sections.

## Known issues these documents record

Recorded rather than fixed, so a reader does not mistake them for new bugs.

| Issue | Where |
|---|---|
| ` ```rust,ignore ` falls back to plain, not Rust — the info string splits on whitespace only (CommonMark §4.5), but rustdoc/mdBook/GitHub all read the part before the comma as the language | `block.rs:299-306` |
| Aliases `shell` and `tsx` do not resolve, though `sh`, `ts`, `js`, `py`, `rb`, `rs`, `cs`, `c++`, `cxx`, `c#`, `yml` all do | `crates/highlight` |
| Mermaid box borders drift on wide graphemes — measured 45-column border against 48-column CJK content; `mermaid-text` sizes in display columns but paints into a per-character canvas and does not use stele's `width` crate | `mermaid-text` 0.57.0, pinned |
| `mermaid::render` takes no width budget, so nothing scales to the viewport; a wide diagram is clipped with `…` by the code-block path | `crates/mermaid/src/lib.rs:31` |
| `classDef`/`style`/`click`/`linkStyle` are silently ignored, so terminal diagrams are monochrome by design | `mermaid-text` |
| `accTitle`/`accDescr` render as spurious boxes in flowcharts and are hard parse errors in `erDiagram`/`sequenceDiagram` | `mermaid-text` |
| `architecture-beta` port-form edges (`a:R --> L:b`) silently draw nothing | `mermaid-text` |
| Inline media boxes are capped at one row — `LayoutTree` is flat, one `Line` per terminal row, and scroll addresses it by index | `layout/src/lib.rs` |
