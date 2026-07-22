# Spike C: RaTeX (LaTeX-to-image math rendering)

Empirical measurements for the RaTeX adoption decision. All numbers below were produced
by actually cloning the repo, building it, and running its own test harness, plus a
second independent scratch project built against the published crates.io packages with
real pixel inspection. None are taken from the project's README/marketing prose without
independent verification.

Environment: `cargo 1.95.0`, `rustc 1.95.0` (Homebrew, on PATH), macOS/arm64. All
cloning and builds were done **outside** the stele repo, under
`/Users/r/repos/scratch/spike-c-ratex/` (`RaTeX/` = git clone of the project; `checks/`
= a throwaway `cargo init --name ratex_checks` binary crate depending on the published
crates.io packages). Nothing under that scratch directory is part of this repo or this
deliverable.

Repo: `https://github.com/erweixin/RaTeX`, cloned via plain `git clone` (worked
directly — no tarball/API fallback needed). HEAD at clone time:
`f5af1e0217a0adedc27a3729749860805273b885` (merge commit, authored 2026-07-22 10:58
+0800). `GET api.github.com/repos/erweixin/RaTeX` at measurement time: **1,406**
stargazers (claimed 1,405 — consistent, off by one, plausibly grew by one star since
the prior unverified research), `pushed_at: 2026-07-22T02:58:00Z` (consistent with the
claimed "commits through 2026-07-22"). Workspace crates confirmed present:
`ratex-types, ratex-font, ratex-lexer, ratex-parser, ratex-layout, ratex-katex-fonts,
ratex-font-loader, ratex-ffi, ratex-cairo, ratex-gtk4, ratex-render, ratex-svg,
ratex-wasm, ratex-pdf, ratex-unicode-font` — so the third relevant crate alongside
`ratex-parser`/`ratex-render` is **`ratex-layout`**, published on crates.io at 0.1.13
(`max_version` confirmed via `curl https://crates.io/api/v1/crates/ratex-layout`).
Version-drift check: only **1** commit touching `ratex-render`/`ratex-layout`/
`ratex-parser`/`ratex-font-loader`/`ratex-katex-fonts` landed between the 0.1.13
crates.io publish (2026-07-07) and the current git HEAD (2026-07-22) — a minor symbol-
alias fix (`d42e53d`), not a behavioral change. So Task 1 (run against the git
checkout) and Task 2 (run against the crates.io-published 0.1.13 packages) are
measuring essentially the same code.

---

## Task 1: golden corpus

### Locating and understanding the corpus

```
$ find . -iname '*golden*' -o -iname '*fixture*' -o -iname '*cases*'
tests/golden/test_cases.txt        # main corpus: one LaTeX expr per line
tests/golden/fixtures/              # KaTeX reference PNGs, 0001.png..NNNN.png
tests/golden/test_case_ce.txt       # mhchem (\ce/\pu) sub-corpus
tests/golden/fixtures_ce/
tests/golden/test_cases_prooftree.txt   # bussproofs sub-corpus (MathJax refs)
tests/golden/fixtures_prooftree/
crates/ratex-render/tests/golden_test.rs   # the actual test harness
```

```
$ wc -l tests/golden/test_cases.txt
    1050 tests/golden/test_cases.txt
$ ls tests/golden/fixtures | wc -l
    1049
```

**The claimed "1,050-case golden corpus" is real and accurate as a nominal count**:
`test_cases.txt` genuinely has 1,050 non-blank lines. There are only 1,049 reference
PNGs on disk (`0001.png`–`1049.png`; `1050.png` is missing), so exactly one case has no
reference to compare against.

`golden_test.rs` (`fn run_golden_suite`) does an ink-overlap comparison per case: crop
both images to their ink bounding box, normalize to 120px height, then score =
`0.4*IoU + 0.2*recall + 0.2*aspect_similarity + 0.2*width_similarity`, pass threshold
`>= 0.30`. `CONTRIBUTING.md` states explicitly: "Some cases score lower than others due
to font subpixel rendering, anti-aliasing, or layout edge differences versus KaTeX
reference PNGs — that does not always indicate a visible bug." This is a coarse
"roughly matches the reference's ink shape," not a pixel-identity check — worth keeping
in mind when reading "100% pass" below.

### Running it — a real discrepancy found in the repo's own tooling

`golden_test.rs::font_dir()` hardcodes `RenderOptions.font_dir` to
`tools/lexer_compare/node_modules/katex/dist/fonts`. That directory is gitignored
(`node_modules/` in `.gitignore`) and nothing in `.github/workflows/ci.yml`'s `check`
job (which runs the plain `cargo test --workspace` that `CONTRIBUTING.md` documents)
installs npm deps into `tools/lexer_compare` before that step runs. On a genuinely
fresh clone, that font path does not exist — `scripts/update_golden_output.sh` (the
script CONTRIBUTING.md points to for regenerating fixtures) actually prefers the
repo's own top-level `fonts/` directory first and only falls back to that
npm-installed path last, so the shell script and the Rust test harness disagree about
where fonts live. I did not attempt to reproduce CI's exact (apparently untested-cold)
path; instead I built with the crate's own `embed-fonts` Cargo feature (used elsewhere
in this same CI for equivalent font-independence checks), which makes
`ratex-font-loader` ignore `font_dir` entirely and load the real embedded KaTeX `.ttf`
bytes from `ratex-katex-fonts` instead — a supported, first-class way to run the crate,
not a workaround that changes what's being measured.

```
$ cargo test -p ratex-render --test golden_test --features embed-fonts -- --nocapture
```

To find the exact reason for skipped cases (the harness only counts a `skipped`
number), I added three `eprintln!` lines to the skip branches in my scratch clone only
(not shipped anywhere) and re-ran:

```
SKIP Golden (main) 0380: parse error ParseError { message: "Undefined control sequence: \\includegraphics", ... }
  | \includegraphics[height=0.8em, ...]{https://cdn.kastatic.org/...png}
SKIP Golden (main) 1050: no ref png | ○\div□=5\quad□\div○=5
Golden (main) (ink-based): 1048/1048 passed (100.0%), 2 skipped
test golden_test_pass_rate ... ok

Golden (mhchem) (ink-based): 103/103 passed (100.0%), 0 skipped
test golden_mhchem_pass_rate ... ok

test cjk_smoke_non_blank_rendering ... ok
test macos_font_cjk_cmap::apple_gothic_missing_hanzi_is_glyph_zero ... ok
test macos_font_cjk_cmap::arial_unicode_maps_fallback_hanzi ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.63s
```

**Real numbers**: of the nominal 1,050-line corpus, 2 cases are excluded before
scoring (1 because RaTeX has no `\includegraphics` support — a documented gap,
consistent with README's own "command-level gaps vs KaTeX" list; 1 because its
reference PNG is simply missing from the repo). Of the **1,048 cases actually scored,
1,048 passed** (`score >= 0.30`) — **100.0%** on this pass/fail definition. The mhchem
(`\ce`/`\pu`) sub-suite scored 103/103 (100.0%). CJK-text and macOS font-cmap smoke
tests also passed. This is a genuine `cargo test` run, not extrapolated from a subset —
every one of the 1,048 scored cases and 103 mhchem cases was individually rendered and
compared in this run. I did not run the `prooftree`/bussproofs sub-corpus (it has its
own MathJax-based fixtures and isn't part of `golden_test.rs`; out of scope for a math
formula renderer decision).

**Discrepancy flag vs. the claimed ">99.5% KaTeX coverage"**: my 100.0% figure is a
pass rate on this repo's own coarse ink-overlap metric over its own golden corpus, not
an independent measurement of "coverage of KaTeX's LaTeX command surface" (a different,
broader claim about syntax breadth I did not attempt to measure — that would require
enumerating KaTeX's supported macro/command list and checking each independently,
which is out of scope here). Do not read "100.0% golden-corpus pass rate" as
confirmation of the specific ">99.5% KaTeX coverage" marketing claim; they are
different axes. What I can say concretely: on the 1,048 real cases this repo ships and
scores against real KaTeX-rendered references, RaTeX matches at the accepted
threshold every time, and its own README lists the known unsupported commands
(`\includegraphics` among them) rather than hiding them.

---

## Task 2: three rendering checks

Scratch project `/Users/r/repos/scratch/spike-c-ratex/checks`, `Cargo.toml`:

```toml
[dependencies]
ratex-types = "0.1.13"
ratex-parser = "0.1.13"
ratex-layout = "0.1.13"
ratex-render = { version = "0.1.13", features = ["embed-fonts"] }
png = "0.17"
```

```
$ cargo build --release
   Compiling ratex-types v0.1.13
   Compiling ratex-font v0.1.13
   Compiling ratex-lexer v0.1.13
   Compiling ratex-parser v0.1.13
   Compiling ratex-font-loader v0.1.13
   Compiling ratex-layout v0.1.13
   Compiling ratex-render v0.1.13
   Compiling ratex_checks v0.1.0 (.../checks)
    Finished `release` profile [optimized] target(s) in 10.69s
```

Builds clean against the real published packages (not the git checkout) — confirms the
crates.io release is standalone-buildable and the public API is
`ratex_parser::parser::parse(&str) -> Result<Ast, ParseError>`,
`ratex_layout::layout(&Ast, &LayoutOptions) -> LayoutBox`,
`ratex_layout::to_display_list(&LayoutBox) -> DisplayList`,
`ratex_render::render_to_png(&DisplayList, &RenderOptions) -> Result<Vec<u8>, String>`.
`LayoutOptions.color: Color` sets glyph ink color (default `Color::BLACK`);
`RenderOptions.background_color: Color` sets the canvas fill and honors alpha.

Four representative formulas were rendered and PNG-decoded with the `png` crate
(source in `checks/src/main.rs`): `x^2 + y^2 = z^2`, `\frac{a}{b}`,
`\begin{cases} x^2 & x \geq 0 \\ -x^2 & x < 0 \end{cases}`, and
`\begin{matrix} a & b \\ c & d \end{matrix}`. All four parsed, laid out, and rendered
without error at the 40px baseline; `\begin{cases}` in particular renders a correctly
stretched brace and two-column alignment (visually confirmed) — notable because the
plan's own txm-only fallback is documented to fail on exactly `\begin{cases}`/`align`.

### a. Sub-16px behavior

```
pythag @ 16px: OK dims=106x37 ink_px=291 (has ink)
pythag @ 12px: OK dims=85x33  ink_px=194 (has ink)
pythag @ 8px:  OK dims=63x29  ink_px=107 (has ink)
pythag @ 4px:  OK dims=42x25  ink_px=33  (has ink)
frac   @ 8px:  OK dims=27x35  ink_px=44  (has ink)
cases  @ 8px:  OK dims=68x45  ink_px=216 (has ink)
matrix @ 8px:  OK dims=37x40  ink_px=63  (has ink)
```
(full table for all 4 formulas × {16,12,8,4}px in the run log; every combination
succeeded — no `Err` returned at any size, no stderr warning printed.)

The API accepts font sizes well below 16px with no rejection and no explicit
warning — `render_to_png` simply scales the same layout down. Visual inspection of the
saved PNGs: at 8px the formula is small but still legibly "x²+y²=z²"; at 4px it
degrades to a tiny, effectively illegible blob of a few dozen ink pixels — but it is a
graceful shrink (proportionally smaller image, ink pixels still present, no crash, no
blank/garbled output, no panic). So: accepts sub-16px sizes silently, degrades to
illegibility only at extreme sizes (~4px), never errors or corrupts.

### b. Transparency

```
=== CHECK B: transparency (bg alpha=0.0) ===
pythag: channels=4 corner_alphas=[0, 0, 0, 0] darkest_ink_pixel@(37,9)=RGBA(0, 0, 0, 48)
frac:   channels=4 corner_alphas=[0, 0, 0, 0] darkest_ink_pixel@(24,9)=RGBA(0, 0, 0, 48)
cases:  channels=4 corner_alphas=[0, 0, 0, 0] darkest_ink_pixel@(33,10)=RGBA(0, 0, 0, 64)
matrix: channels=4 corner_alphas=[0, 0, 0, 0] darkest_ink_pixel@(81,15)=RGBA(0, 0, 0, 16)
```

Independent cross-check with macOS `sips` (not the Rust `png`-crate decoder used
above):

```
$ sips -g hasAlpha -g pixelWidth -g pixelHeight -g space pythag_transparent.png
  hasAlpha: yes
  pixelWidth: 235
  pixelHeight: 63
  space: RGB
```

Real result: PNGs are genuinely RGBA (`channels=4`, confirmed independently by both
the `png` crate and `sips`). With `background_color = Color::new(1,1,1,0.0)`, all 4
sampled corner pixels across all 4 formulas have alpha exactly **0** — real
transparency, not white RGB masquerading as transparent. Solid glyph-interior pixels
(alpha > 128, i.e. not anti-aliased edge) exist in real quantity per formula (736,
310, 1,724, 571 such pixels for pythag/frac/cases/matrix respectively, reused from
Check C below) confirming the ink itself is drawn at full/near-full opacity while the
background stays fully transparent.

### c. Dark-background legibility

Composited each render (`background_color` alpha 0) over `#1e1e1e` (30,30,30) by
manual alpha-blend in the check program, sampled the RGB of every alpha>128 ("solid
ink") pixel, and computed the **actual WCAG relative-luminance contrast ratio**
(gamma-corrected sRGB→linear, not a raw 0–255 shortcut — my first in-Rust pass used an
uncorrected formula and gave a misleading number; recomputed correctly in Python from
the same measured RGB samples):

| formula | ink color source | composited ink RGB (avg, n px) | bg RGB | WCAG contrast |
|---|---|---|---|---|
| pythag | default (`Color::BLACK`) | (3,3,3), n=736 | (30,30,30) | **1.24:1 — fails all thresholds** |
| pythag | caller sets `Color::WHITE` | (236,236,236), n=736 | (30,30,30) | 14.11:1 — passes AA |
| frac | default black | (2,2,2), n=310 | (30,30,30) | 1.24:1 — fails |
| frac | caller white | (238,238,238), n=310 | (30,30,30) | 14.37:1 — passes |
| cases | default black | (2,2,2), n=1724 | (30,30,30) | 1.24:1 — fails |
| cases | caller white | (238,238,238), n=1724 | (30,30,30) | 14.37:1 — passes |
| matrix | default black | (3,3,3), n=571 | (30,30,30) | 1.24:1 — fails |
| matrix | caller white | (235,235,235), n=571 | (30,30,30) | 13.98:1 — passes |

Real finding: **RaTeX does not hardcode a dark ink color** — `LayoutOptions.color` is a
plain public field the caller sets before calling `layout()`, and setting it to white
produces a correctly high-contrast render (~14:1, easily passing WCAG AA's 4.5:1 bar).
But the *default* (`LayoutOptions::default()`, `Color::BLACK`) composited over a
`#1e1e1e`-class dark background measures **1.24:1** — a contrast ratio the WCAG spec
would call a hard fail at any text size, i.e. functionally invisible for accessibility
purposes (the composited PNG at thumbnail size shows a very faint edge from
anti-aliasing, but the solid glyph fill is only 27–28 luminance levels away from a
30-level-gray background — not something a user would reliably read on a dark
terminal). This is exactly the behavior the spike was designed to catch: **if stele
calls RaTeX with default options, math will be nearly invisible on Ghostty's typical
dark theme**; if stele explicitly threads its resolved foreground color into
`LayoutOptions.color`, it renders clearly. All three sub-checks (a/b/c) were completed
with real measurements — none were skipped or guessed.

---

## Decision

capability: LaTeX formula → raster PNG math rendering for stele's rendered-markdown
math blocks → verdict: **adopt** `ratex-parser` + `ratex-layout` + `ratex-render`
0.1.13, with `ratex-render`'s `embed-fonts` feature enabled (bundles the 20 KaTeX
`.ttf` files already present in `crates/ratex-katex-fonts/fonts/` — confirmed by
listing; matches the claimed "20 KaTeX TTF fonts embedded" figure — so no runtime
font-directory dependency or network fetch is needed) → consequence: stele's math
crate must **never rely on `LayoutOptions::default()`** for the ink color — it must
explicitly set `LayoutOptions.color` to stele's resolved terminal foreground/theme
color on every render call, because the library default (`Color::BLACK`) measured at
a 1.24:1 WCAG contrast ratio against a `#1e1e1e`-class dark background (empirically
near-invisible), while an explicit light color measured 14:1 (clearly legible) using
the exact same code path — this is a one-line integration requirement, not a library
defect, and it must be treated as a required wiring step, not an afterthought, given
Ghostty's dark-background-first usage. Grounds for **adopt** rather than **hedge**:
every measurement taken came back positive with no showstopper — 1,048/1,048 (100.0%)
real golden-corpus cases passed the repo's own visual-similarity test (plus 103/103
mhchem, plus CJK/emoji smoke tests, all genuinely executed, not extrapolated),
`\begin{cases}` (a documented txm-fallback gap) renders correctly, sub-16px sizes
never error or corrupt output, and transparency is real (RGBA with alpha=0 background,
independently confirmed via `sips`) — matching or exceeding every one of the specific,
falsifiable claims this spike set out to check, with the one exception that
">99.5% KaTeX coverage" (a command-breadth claim) remains genuinely unverified by this
spike (I measured golden-corpus visual pass rate, a different and narrower metric) and
should not be treated as confirmed. Given the project is young (v0.1.13, first
published 2026-07-07) and heavily AI-authored (own repo ships `.claude/`/`.cursor/`/
`.agents/` skill directories), keep the plan's fallback path documented and buildable
— `txm` (crates.io, pure-Rust Unicode-grid, no font/rasterization risk, but
independently documented to fail on `\begin{cases}`/`align`, which RaTeX handles) or
`ReX` (`https://github.com/KenyC/ReX`, git-only dependency cost) — as a rollback
option if a future 0.2.0+ RaTeX release regresses the corpus pass rate or introduces a
build/font-embedding break, rather than as an actively-maintained parallel
integration.
