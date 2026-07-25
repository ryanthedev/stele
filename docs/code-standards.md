<!-- base-commit: 85d878e438fcde1993f724586dce5b4c43c03032 -->
<!-- generated: 2026-07-25 -->

# Code Standards

Conventions extracted from the stele workspace. Only what deviates from ordinary Rust practice.

---

## Forbidden Patterns

**Never write `unsafe`.** Every crate carries `#![forbid(unsafe_code)]`. The one exception is `crates/stele`, which uses `#![deny(unsafe_code)]` with a comment explaining the single scoped opt-out (a POSIX signal handler that restores the terminal); see `crates/stele/src/lib.rs:12-19`. Adding a second opt-out needs the same treatment: a scoped `#[allow]`, and a comment stating what breaks without it.

**Never cast a computed width with `as u16`** — it wraps, and a wrapped width slips an over-wide run past a fit check while `Run.width` lies about it:

```rust
// BAD — a token wider than 65,535 cells wraps to a small number and passes the fit check
let width = display_width(token) as u16;

// GOOD — from crates/layout/src/inline.rs:354; arithmetic in usize, saturated at the boundary
pub(crate) fn cells(engine: &WidthEngine, s: &str) -> u16 { /* saturating */ }
```

**Never assert on `Run.width` in a test.** A run that lies about its own width passes any assertion that trusts it. Re-measure the painted text through the width engine instead — that is the only oracle that can fail.

**Never add a wildcard arm to a `Semantic` or `Capture` match.** The style tables (`crates/highlight/src/theme.rs`, `crates/stele/src/decor/mod.rs`) are exhaustive on purpose so a new variant is a compile error rather than silently painting plain.

**Never let a terminal reply be trusted without parsing.** OSC/CSI replies are parsed defensively and return `Option`/`Result`; malformed input degrades to a documented fallback, never a panic. See `parse_channel` in `crates/highlight/src/theme.rs`.

---

## Code Examples

### A style table arm

```rust
// DO — from crates/highlight/src/theme.rs; exhaustive, no wildcard, each arm's
// reasoning lives in the doc comment above the function rather than inline
match semantic {
    Semantic::Heading(level) => heading_attrs(level, fg.is_none()),
    Semantic::Strong | Semantic::TableHeader | Semantic::FootnoteLabel => bold,
    // ...every remaining variant named explicitly
}

// DON'T — a wildcard silently absorbs the next variant someone adds
match semantic {
    Semantic::Heading(_) => bold,
    _ => Style::default(),
}
```

### A constant that encodes a measurement

```rust
// DO — the value carries the measurement that produced it, so the next
// person to "tidy" it knows what they are about to break
/// Three tiers, not six [...] a six-rung version of this table put H6 at
/// 3.13:1 and H5 at 4.20:1, both under WCAG AA's 4.5:1.
const HEADING_RAMP: [(f64, [f64; HEADING_TIERS]); 2] = [ /* ... */ ];

// DON'T — a bare tuned number invites silent regression
const HEADING_RAMP: [(f64, [f64; 3]); 2] = [(0.62, [0.86, 0.76, 0.68]), /* ... */];
```

---

## Error Handling

Hand-rolled error enums with a manual `Display` impl. `thiserror` is used **only** in `crates/probe` (a dev/spike crate) — do not add it to a shipping crate.

```rust
// From crates/gfx/src/decode.rs:78 — variants carry the data a caller needs to
// decide, not a pre-formatted string
pub enum DecodeError {
    Io(std::io::Error),
    Malformed(String),
    ExceedsLimits { width: u32, height: u32 },
}
impl std::fmt::Display for DecodeError { /* ... */ }
```

Fallible paths return `Result`/`Option` and degrade to a documented fallback. `expect` is acceptable only where the invariant is stated in the message and is guaranteed by construction:

```rust
// From crates/highlight/src/theme.rs — the message names the invariant that makes it safe
self.palette.get(role_index).expect(
    "role_index is always < TOTAL_ROLES by construction of the *_role_index functions",
)
```

---

## Imports & Dependency Direction

Three groups, blank-line separated, `std` first, then external + workspace crates alphabetically, then `crate::`:

```rust
// From crates/stele/src/app.rs
use ast::{Document, NodeId};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use layout::{IntrinsicSizer, LayoutConfig, LayoutTree, layout};
use width::WidthEngine;

use crate::painter::Size;
```

Dependency direction is strict and one-way:

```
width  ast  →  layout  →  highlight / gfx / math / mermaid  →  stele
```

`crates/stele` is the only crate that may depend on the others. **Nothing may depend on `stele`** — when a type is needed on both sides of a seam it is duplicated field-for-field with a comment saying so (see `highlight::theme::Style` vs `stele::painter::Style`), never inverted.

---

## Testing Patterns

Framework: built-in `cargo test`. `proptest` is used in `crates/width` only.

**Test names are sentences describing the behavior**, not the function under test:

```rust
// DO — from the suite; the name states what would be false if it failed
fn test_a_box_that_leaves_the_screen_is_unplaced_but_keeps_its_raster()
fn test_a_zero_cell_axis_falls_back_instead_of_dividing_by_zero()
fn test_a_neighbouring_xtwinops_report_is_not_believed()

// DON'T
fn test_paint() / fn test_resolve_works()
```

**Tests tracing a plan requirement carry its DW id**, so a done-when item maps to an executable check: `test_dw_2_3_pathological_inputs_stay_fast`, `test_dw_7_2_downsample_256_keeps_every_role_distinct_dark_and_light`.

**Integration tests share one harness.** `crates/stele/tests/common/` holds the pty driver, render helpers and fixtures; each test file declares `mod common;` and takes what it needs (`crates/stele/tests/common/mod.rs:8`). Do not re-implement a pty or a frame parser in a new test file.

**A test must be able to fail.** Prefer an oracle independent of the code under test — the pinned Ghostty width corpus (`corpus/ghostty-1.3.1-widths.json`), a re-measurement through the width engine, or bytes captured off a real pty.

---

## Naming Conventions

Crates are single common nouns, no prefix: `ast`, `width`, `layout`, `gfx`, `highlight`, `math`, `mermaid`, `probe`, `stele`.

Domain terms, used consistently:

| Term | Means |
|---|---|
| **run** | A styled text span within a line (`Run`) |
| **placement** | An image drawn on screen now; distinct from **residency** (its pixel data living in the terminal) |
| **cell** | A terminal character cell — the unit of all layout arithmetic |
| **rung** / **tier** | A step in a per-level style ramp |
| **seam** | A trait boundary a later phase implements (`MediaSink`, `Decor`, `IntrinsicSizer`) |

---

## File Organization

```
crates/
├── ast/         # parse → Document, NodeId, spans
├── width/       # grapheme + east-asian width engine (pinned corpus oracle)
├── layout/      # Document → LayoutTree; blocks, inline wrap, tables
├── highlight/   # theme, palette, tree-sitter highlighting, OSC 8
├── gfx/         # kitty graphics protocol emission + image decode
├── math/        # TeX → PNG
├── mermaid/     # ```mermaid fences → box-drawing grids
├── probe/       # spike/dev crate: drives a real Ghostty pty
└── stele/       # binary + library: app state, painter, terminal, seam impls
    ├── src/decor/   # Decor seam: structural + themed
    ├── src/media/   # MediaSink seam: gfx-backed image/math sink
    └── tests/common/ # shared pty + render harness
```

Unit tests live in a `#[cfg(test)] mod tests` at the bottom of the file they test. Cross-crate and terminal-level behavior goes in `crates/stele/tests/`.

---

## Technology Decisions

- **Ghostty is the only supported terminal.** Graphics are gated on `TERM_PROGRAM=ghostty` and disabled under `TMUX` (`crates/stele/src/main.rs:68-69`). Do not add sixel or iTerm2 backends.
- **Kitty graphics protocol only** for images and math (`crates/gfx/src/protocol.rs`). Placements are moved by reusing the same `(image id, placement id)` pair across frames rather than re-transmitting.
- **PDF-viewer model, not streaming.** Parse the whole document, retain the layout tree, scroll a viewport. Never render incrementally as bytes arrive.
- **No user theme files.** Dark/light is chosen from the terminal's own OSC 11 background reply; the palette is generated, not configured.
- **Release profile is already maximal** (`lto = true`, `codegen-units = 1`, `strip = true`). Performance work must come from the code, not compiler flags.
- Edition 2024, `rust-version = 1.95.0`, resolver 3. `crates/ast/fuzz` is excluded from the workspace so stable fmt/clippy/test never touch it.

---

## Exemplar Files

**`crates/highlight/src/theme.rs`** — exhaustive style tables, generated palette with a distinctness invariant enforced at construction, constants that carry their measurements, tests that pin mapping rather than mere uniqueness.

**`crates/stele/src/media/sink.rs`** — the module doc states the invariants (visibility vs residency) *and* the bug that came from conflating them; a seam implementation with its failure modes written down.

**`crates/layout/src/inline.rs`** — saturating width arithmetic at the boundary, with the doc comment explaining the wrap-around it prevents.
