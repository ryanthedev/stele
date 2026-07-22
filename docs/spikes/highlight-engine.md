# Spike B: Highlight Engine + Mermaid Crate

Empirical measurements for the highlight-engine and Mermaid-crate decisions gating P7.
All numbers below were produced by actually building and running the code shown; none
are estimated or taken from documentation prose without independent verification.

Environment: `cargo 1.95.0`, `rustc 1.95.0` (both Homebrew, on PATH), macOS/arm64
(Mach-O 64-bit executables). All scratch builds were done **outside** this repo, under
`/tmp/stele-spike-b/`, in three throwaway `cargo new --bin` projects (`lumis-spike`,
`syntect-spike`, `mermaid-spike`). Nothing under `/tmp/stele-spike-b/` is part of this
repo or this deliverable.

The 20 target languages (per plan Notes / DW-7.1): rust, python, javascript,
typescript, go, c, cpp, java, csharp, ruby, swift, kotlin, zig, bash, json, yaml,
toml, html, css, sql.

---

## Task 1: lumis vs syntect+two-face

### 1a. lumis — feature-gating mechanism

`cargo add lumis@0.12.0` inside a scratch binary crate, then read the registry copy of
its `Cargo.toml` at
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/lumis-0.12.0/Cargo.toml`.

Finding: lumis gates every language behind its own Cargo feature (`lang-rust`,
`lang-python`, `lang-go`, …), each pulling in one `tree-sitter-<lang>` optional
dependency plus a matching `lumis-core/lang-<lang>` feature. `default = ["all-languages"]`
enables all ~115 languages unless `default-features = false` is set. All 20 target
languages exist as individual features — none are missing or need substituting:

```
lang-rust, lang-python, lang-javascript, lang-typescript, lang-go, lang-c, lang-cpp,
lang-java, lang-csharp, lang-ruby, lang-swift, lang-kotlin, lang-zig, lang-bash,
lang-json, lang-yaml, lang-toml, lang-html, lang-css, lang-sql
```

(Note: the feature is `lang-sql`, but the corresponding `languages::Language` enum
variant is `Language::SQL`, not `Language::Sql` — a naming mismatch encountered while
writing `main.rs`, fixed by using the correct variant.)

Scratch `Cargo.toml` used for the size measurement (`/tmp/stele-spike-b/lumis-spike/Cargo.toml`):

```toml
[dependencies]
lumis = { version = "0.12.0", default-features = false, features = [
    "lang-rust", "lang-python", "lang-javascript", "lang-typescript", "lang-go",
    "lang-c", "lang-cpp", "lang-java", "lang-csharp", "lang-ruby", "lang-swift",
    "lang-kotlin", "lang-zig", "lang-bash", "lang-json", "lang-yaml", "lang-toml",
    "lang-html", "lang-css", "lang-sql",
] }

[profile.release]
strip = true
```

### 1b. lumis — proves it links and runs

`/tmp/stele-spike-b/lumis-spike/src/main.rs` calls `lumis::highlight()` with a
`TerminalBuilder` (ANSI output) for Rust, Python, Go, and SQL, and prints the raw ANSI
bytes. Command and result:

```
$ cargo build --release
$ ./target/release/lumis-spike
=== Rust (ANSI, 351 bytes out) ===
<colored ANSI-escaped `fn main() { println!("hi"); }`>
=== Python (ANSI, 135 bytes out) ===
<colored ANSI-escaped `def main(): print('hi')`>
=== Go (ANSI, 365 bytes out) ===
<colored ANSI-escaped Go source>
=== SQL (ANSI, 337 bytes out) ===
<colored ANSI-escaped SQL source>
```

All four produced non-empty, syntax-colored ANSI output — the crate genuinely links,
initializes tree-sitter grammars for the configured languages, and highlights.

### 1c. lumis — raw-span API verdict: YES, exposed

Evidence, in order of how it was found:

1. `src/lib.rs` (top-level docs) documents a **streaming callback API**,
   `lumis::highlight::highlight_iter`, whose callback signature is
   `FnMut(&str, Language, Range<usize>, &'static str, &Style)` — text, language, byte
   range, **scope/capture name**, and a theme-resolved style. Read directly from
   `~/.cargo/registry/.../lumis-0.12.0/src/highlight.rs` lines 333–401.
2. To confirm the scope name is independent of any theme (i.e. it really is a raw
   capture name the caller can map to its own theme, not just a pre-resolved color),
   `main.rs` calls `highlight_iter(code, Language::Rust, None, …)` with `theme = None`
   and prints `(byte_range, scope, text)` for every emitted span. Output (18 spans for
   `"fn main() { let x = 1; }"`), captured directly from the built binary:

   ```
   byte 0..2   scope="keyword.function"       text="fn"
   byte 3..7   scope="function"               text="main"
   byte 7..8   scope="punctuation.bracket"    text="("
   byte 12..15 scope="keyword"                text="let"
   byte 16..17 scope="variable"               text="x"
   byte 18..19 scope="operator"               text="="
   byte 20..21 scope="number"                 text="1"
   byte 21..22 scope="punctuation.delimiter"  text=";"
   byte 23..24 scope="punctuation.bracket"    text="}"
   ```

   This is exactly `(byte_range, capture_name)` — the type is
   `(Range<usize>, &'static str, &str)`.
3. Additionally, `lumis::events` and `lumis::highlights` are re-exported from
   `lumis-core` (`pub use lumis_core::events; pub use lumis_core::highlights;`), giving
   a `HighlightEvent` enum (`Start { scope_index, language }` / `Source { start, end }`
   / `End`) and a `HIGHLIGHT_NAMES: [&str; 293]` scope-name table. There is also a
   crate-internal `highlight_events()` / `highlight_events_with_options()` function
   returning `Vec<lumis_core::events::HighlightEvent>` — but these are marked
   `#[doc(hidden)]` with the comment "Exposed for conformance tooling; not part of the
   stable public API" (`src/highlight.rs` lines 403–421). **`highlight_iter` is the
   supported, documented raw-span API; the event-vector form exists but is explicitly
   not a stable contract.**

Verdict: lumis exposes raw spans via the public, documented `highlight_iter` function —
confirmed by reading the source and by running code that extracts and prints span
tuples with `theme: None`.

### 1d. lumis — stripped release binary size

```
$ cd /tmp/stele-spike-b/lumis-spike && cargo build --release
$ ls -la target/release/lumis-spike
-rwxr-xr-x  1 r  wheel  32626400 ... target/release/lumis-spike
$ file target/release/lumis-spike
target/release/lumis-spike: Mach-O 64-bit executable arm64
```

**32,626,400 bytes = 32.63 MB (decimal) / 31.11 MiB.** Stripped via
`[profile.release] strip = true` in the scratch `Cargo.toml` (cargo's built-in strip,
not a manual `strip` invocation — this compiles to the platform strip equivalent).
This is with exactly the 20 target-language features enabled, `default-features = false`
— i.e. this is not the ~115-language default build, which would be substantially larger.

### 1e. syntect + two-face — setup and language coverage

`cargo add syntect` resolved to `syntect = "5.3.0"` (latest stable); `cargo add two-face`
resolved to `two-face = "0.5.1"`.

two-face bundles its syntax definitions as prebuilt `.bin` blobs
(`generated/syntaxes-*.bin`), so coverage cannot be read off a file listing — it was
checked empirically by loading the syntax set at runtime and querying each of the 20
extensions via `find_syntax_by_extension`. `/tmp/stele-spike-b/syntect-spike/src/main.rs`:

```
$ ./target/release/syntect-spike
=== two-face coverage check for 20 target languages (by file extension) ===
rust         (.rs    ) -> FOUND: Rust
python       (.py    ) -> FOUND: Python
javascript   (.js    ) -> FOUND: JavaScript (Babel)
typescript   (.ts    ) -> FOUND: TypeScript
go           (.go    ) -> FOUND: Go
c            (.c     ) -> FOUND: C
cpp          (.cpp   ) -> FOUND: C++
java         (.java  ) -> FOUND: Java
csharp       (.cs    ) -> FOUND: C#
ruby         (.rb    ) -> FOUND: Ruby
swift        (.swift ) -> FOUND: Swift
kotlin       (.kt    ) -> FOUND: Kotlin
zig          (.zig   ) -> FOUND: Zig
bash         (.sh    ) -> FOUND: Bourne Again Shell (bash)
json         (.json  ) -> FOUND: JSON
yaml         (.yaml  ) -> FOUND: YAML
toml         (.toml  ) -> FOUND: TOML
html         (.html  ) -> FOUND: HTML
css          (.css   ) -> FOUND: CSS
sql          (.sql   ) -> FOUND: SQL
Missing count: 0 of 20
```

All 20 languages resolved via `two_face::syntax::extra_newlines()` — the syntax set is
loaded fully (not feature-gated per language the way lumis is), so there is no
equivalent "configure for exactly 20" step; the whole bundled set (hundreds of Sublime
syntaxes, curated by the `bat` project) ships regardless.

Live highlight proof (same 4 languages as lumis, `HighlightLines` + `as_24_bit_terminal_escaped`):

```
--- rust (394 bytes ANSI out) ---
--- python (312 bytes ANSI out) ---
--- go (441 bytes ANSI out) ---
--- sql (231 bytes ANSI out) ---
```

All four produced non-empty colored ANSI output.

Raw-span check for syntect (for comparison, not required by the threshold rule since
lumis already has spans): the high-level `HighlightLines` API returns
`Vec<(Style, &str)>` per line — a **resolved** style, not a scope name. To get a raw
capture-equivalent, you must drop to `syntect::parsing::ParseState` +
`syntect::parsing::ScopeStack`, which yields, per byte range, the **entire TextMate
scope stack** (e.g. `[<source.rust>, <meta.function.rust>, <storage.type.function.rust>]`)
rather than a single flat name:

```
byte 0..2  scope=[<source.rust>, <meta.function.rust>, <meta.function.rust>, <storage.type.function.rust>]  text="fn"
byte 3..7  scope=[<source.rust>, <meta.function.rust>, <entity.name.function.rust>]                          text="main"
```

This is a legitimate raw-span mechanism, but it hands the caller a whole scope stack to
resolve against a theme (TextMate-style prefix matching), which is materially more
integration work than lumis's single flat `&'static str` scope name per span.

### 1f. syntect + two-face — stripped release binary size

```
$ cd /tmp/stele-spike-b/syntect-spike && cargo build --release
$ ls -la target/release/syntect-spike
-rwxr-xr-x  1 r  wheel  2137232 ... target/release/syntect-spike
$ file target/release/syntect-spike
target/release/syntect-spike: Mach-O 64-bit executable arm64
```

**2,137,232 bytes = 2.14 MB (decimal) / 2.04 MiB.** Stripped via the same
`[profile.release] strip = true`. Note this pulls in `onig`/`onig_sys` (Oniguruma,
compiled from bundled C source via the `cc` crate) as the default regex engine for
syntect 5.3 — a native-code dependency, though a small one size-wise.

### 1g. Size comparison

| Engine | Stripped release binary | Languages covered | Raw spans |
|---|---|---|---|
| lumis 0.12.0 (20 features only) | 32,626,400 B = 32.63 MB / 31.11 MiB | 20/20 | Yes — public `highlight_iter`, flat scope name + byte range |
| syntect 5.3.0 + two-face 0.5.1 | 2,137,232 B = 2.14 MB / 2.04 MiB | 20/20 | Yes, but low-level — `ParseState`/`ScopeStack`, full TextMate scope stack |

lumis is ~15.3x larger than syntect+two-face for this measurement.

---

## Task 2: Mermaid crate evaluation

`cargo add mermaid-text@0.57.0` in `/tmp/stele-spike-b/mermaid-spike`. Its own
dependencies are `ascii-dag`, `chrono` (no default features), `unicode-width` —
lightweight and pure-Rust (`#![forbid(unsafe_code)]` at the crate root, confirmed in
`src/lib.rs`). `js-sys`/`wasm-bindgen` appear transitively in `cargo fetch` output but
are conditional-compilation-only (wasm target); the native `cargo build --release`
above did not link them.

### Public API (read from `~/.cargo/registry/.../mermaid-text-0.57.0/src/lib.rs`)

```rust
pub fn render(input: &str) -> Result<String, Error>;
pub fn render_with_width(input: &str, max_width: Option<usize>) -> Result<String, Error>;
pub fn render_ascii(input: &str) -> Result<String, Error>;
pub fn render_ascii_with_width(input: &str, max_width: Option<usize>) -> Result<String, Error>;
pub fn render_with_options(input: &str, opts: &RenderOptions) -> Result<String, Error>;
pub fn to_ascii(s: &str) -> String;
```

`Error` is an enum (`EmptyInput`, `UnsupportedDiagram(String)`, `ParseError(String)`,
`TooWide { requested, actual }`). Output type is always `String`.

### Real run, exact task diagram

`/tmp/stele-spike-b/mermaid-spike/src/main.rs` feeds this literal input to
`mermaid_text::render`:

```
graph TD
  A[Start] --> B{Decision}
  B -->|Yes| C[Do thing]
  B -->|No| D[Skip]
```

Actual captured output (`$ ./target/release/mermaid-spike`):

```
             ┌───────┐
             │ Start │
             └───────┘
                 └┐
                  │
                  │
                  │
                  │
                  │
                  │
            ╱─────▾────╲
            │ Decision │
            ╲──────────╱
                 │ └─────┐
                 │       │
                 │       │
             Yes │       │No
                 │       │
                 │       │
            ┌────┘       │
      ┌─────▾────┐   ┌───▾──┐
      │ Do thing │   │ Skip │
      └──────────┘   └──────┘
```

Confirmed by direct inspection of the program's own output: this is a genuine Unicode
box-drawing text-grid render — real `┌┐└┘│─╱╲▾` glyphs forming actual box shapes and a
decision-diamond, with all four node labels (`Start`, `Decision`, `Do thing`, `Skip`)
and both edge labels (`Yes`, `No`) present and correctly routed. It is not an error
string, not a stub, and not a plain unrendered fallback — `render()` returned `Ok(...)`
with 811 bytes of output.

### `render_ascii()` — real behavior vs. documented contract (finding)

`render_ascii()`'s own doc comment and doctest assert `out.is_ascii()` unconditionally
("Every character in the returned string is guaranteed to be `< 0x80`"). Running it on
the *same* diagram above:

```
$ ./target/release/mermaid-spike | tail -15 | grep -P '[^\x00-\x7F]'
            ╱-----v----╲
            ╲----------╱
```

`render_ascii()` on this diagram returns a string containing `╱` and `╲` — non-ASCII
diagonal box-drawing characters used to render the diamond/decision-node shape — so
`out.is_ascii()` is actually `false` for any diagram containing a decision node. Root
cause, confirmed by reading `to_ascii()`'s substitution match arms in `src/lib.rs`
(lines ~949–989): the function's `match` has no arm for `'╱'` or `'╲'`, only for the
box corners, T-junctions, and arrow tips — the diagonal diamond-corner glyphs added for
decision-node rendering were not added to the ASCII substitution table. This is a real,
reproducible gap between the crate's documented guarantee and its actual behavior for
this specific shape; noted here as an honest finding, not papered over. It does not
block adoption (Unicode is stele's primary target per Ghostty; ASCII mode is a
documented fallback path for legacy terminals, not the primary render path), but a
future stele integration should not rely on `render_ascii()`'s ASCII-purity guarantee
for diagrams containing decision nodes without its own post-filter.

---

## Decision

capability: 20-language syntax highlighting with a raw-span (byte-range + capture-name)
API for stele's own theme layer → verdict: **judgment call — adopt `lumis` 0.12.0**,
configured with `default-features = false` and only the 20 target-language `lang-*`
features (not the `all-languages` default) → consequence: stele's `crates/highlight`
crate integrates against `lumis::highlight::highlight_iter` (public, documented;
callback gives `(text, Language, Range<usize>, &'static str scope, &Style)`) and maps
lumis's flat scope names directly to stele's own `Style`/theme table; accept a
~31.11 MiB / 32.63 MB stripped binary contribution — this falls in the plan's
30–100 MB "between" band (not ≤30 MB, so not a threshold-triggered pick), so this is a
reasoned judgment, not a rule match: lumis's overage past the 30 MB auto-yes bar is
marginal (about 4–9% depending on decimal-vs-binary unit) and far from the 100 MB
reject bar (it uses <1/3 of that budget); tree-sitter-based highlighting is AST-precise
rather than regex-heuristic; and lumis's raw-span API (single flat scope name per span)
is materially cheaper to integrate against a caller-owned theme than syntect+two-face's
alternative (`ParseState`/`ScopeStack`, which hands back a full TextMate scope stack
requiring the caller to reimplement prefix-based scope resolution). The empirically
measured alternative — syntect 5.3.0 + two-face 0.5.1, 2,137,232 B = 2.14 MB / 2.04 MiB,
~15.3x smaller, also covering all 20 languages — is recorded above in full for P7 to
revisit if binary-size distribution pressure turns out to dominate this tradeoff.

capability: rendering `mermaid` code fences as an in-terminal text diagram without an
image protocol → verdict: **adopt `mermaid-text` 0.57.0** (`https://github.com/leboiko/markdown-reader`,
`crates/mermaid-text`), output form = genuine Unicode box-drawing text grid via
`mermaid_text::render(input: &str) -> Result<String, mermaid_text::Error>` → consequence:
stele's `crates/mermaid` crate calls `render()` (or `render_with_options` for
width-constrained / ANSI-colored variants) on mermaid fence contents and emits the
returned `String` directly into the document flow like any other block; on
`Err(Error::UnsupportedDiagram(_) | Error::ParseError(_) | Error::EmptyInput)`, fall
back to the plan's documented Decision Log path — render the fence as a plain code
block — since real diagram-type/parse failures are expected for the long tail of
Mermaid syntax; do not rely on `render_ascii()`'s documented ASCII-purity guarantee
for diagrams containing decision-node (`{...}`) shapes without stele's own post-filter,
since it is empirically false in that case (verified above) despite the crate's own
doctest asserting it.
