# Math rendering: the one-row baseline boundary

This file exercises stele's RaTeX → `txm` → literal-TeX degradation ladder,
and specifically the rule in `crates/layout/src/inline.rs`'s `wrap()`: an
inline math box whose natural size is **exactly one row tall** (and narrower
than the content column) rides the text baseline like a word. Anything
**taller than one row, or wider than the content column,** falls through to
claim its own line(s) — the same path display math and block images use.

All sizes quoted below were measured directly against this checkout's
`math::intrinsic_em_size` / `math::render_text` (the same functions
`stele::media::sizer::ImageSizer` calls), not guessed from the TeX source.
`rows <= 1` is the baseline-riding condition.

**Rows are quoted; columns are not.** A formula's em baseline is the
terminal's own cell height (`sizer::math_baseline_px`), so one em is one row
and a formula's row count is the same on every terminal — that is what makes
`rows` a fact worth writing down here. Its *column* count is not: an em-square
covers twice as many columns in a 12x28 cell as in a 24x48 one, so any number
this file quoted would be true only for whoever measured it.

This file previously quoted both, from the 24x48 fallback geometry that every
test in the workspace uses and no reader's terminal reports. The numbers were
real and the conclusions drawn from them were wrong: `a+b=c` measured 1 row
there and 2 rows on an actual Ghostty, which is the bug this section exists to
catch and the bug the quoted numbers hid.

## Inline math mid-sentence

A paragraph with several formulas woven through it, so wrapping around each
one is visible: the energy $E=mc^2$ relates mass and energy, the quadratic
$x^2+y^2=r^2$ describes a circle, and Euler's identity $e^{i\pi}+1=0$ ties
five constants together in one line. Further along, the derivative
$f'(x)=2x$ of $f(x)=x^2$ mid-sentence, and the sum $x=1$ compared against
$n=2$ near the end of the paragraph, should all wrap as ordinary words do.

*expect: every formula above measures 1 row and rides the baseline — this
paragraph should wrap as plain flowing text with small math-shaped gaps, not
break into extra lines around each formula.*

## Inline math at the start, end, and alone

$x^2+y^2=r^2$ opens this line before any other text.

This line ends with inline math at the very last position: $a+b=c$

$a+b=c$

*expect: all three ride the baseline (each measures 1 row, narrow). The
start-of-line and end-of-line cases should sit flush against the paragraph
edges with no stray leading/trailing gap; the solo-paragraph case renders as
a one-line paragraph, not a reserved block region, since the box-vs-line
choice in `wrap()` never checks position — only size.*

## The boundary itself: near-1-row vs taller-than-1-row

Simple formulas that measure **exactly 1 row** (verified: `a+b=c` → 0.778 em
tall → 1 row; `x_1` → 0.581 em → 1 row; on every cell geometry):

Simple sum $a+b=c$ and subscript $x_1$ sitting in a sentence.

*expect: both ride the baseline — this is the core regression case for the
bug we just fixed. This sentence must stay ONE line (or wrap only at normal
word boundaries), never break into three lines around the formulas.*

Formulas that measure **taller than 1 row** (verified: `\frac{a}{b}` → 1.794
em → 2 rows; `\sum_{i=0}^{n}` → 3.089 em → 4 rows; `\int_0^\infty` → 2.326 em
→ 3 rows; on every cell geometry):

A fraction $\frac{a}{b}$, a limited sum $\sum_{i=0}^{n}$, and a limited
integral $\int_0^\infty$ all appear here mid-paragraph.

*expect: each of these three breaks OUT of the paragraph flow onto its own
reserved row(s) — they are taller than 1 row, so the `wrap()` boundary
routes them to the standalone-box path (`Piece::Box`) instead of riding the
baseline, even though they're written as ordinary inline `$…$` math.*

## Superscripts and subscripts, stacked

$e^{i\pi}$ and the doubly-stacked $x^{y^{z}}$ appear here.

*expect: both measured 1 row tall (cols 2 and cols 3 respectively) despite
the stacked exponents — RaTeX keeps the raised superscript within the same
cell row at this size, so both ride the baseline. This is a genuinely
counter-intuitive result worth checking with your own eyes: "stacked"
does not automatically mean "breaks out" — only actual row height does.*

## Inline math adjacent to punctuation

The result, $x=1$, follows directly after a comma and before one too.
Is it true that $a+b=c$? Yes — $a+b=c$! And in parentheses: ($n=2$).

*expect: rides the baseline in every position; punctuation immediately
touching the `$` on either side must not be swallowed into the math span or
pushed onto its own line.*

## Inline math inside emphasis, strong, and a link

Plain **bold text with $x^2$ inside it** continues past the formula.

A [link containing $a+b=c$ inline](https://example.com/math) here.

*expect: the formula still rides the baseline in both cases. Note (from
reading `crates/layout/src/inline.rs`'s `push_box`/`push_text`): a
successfully-sized math box carries no link `aux` at all, so if this
formula resolves to a PNG/txm box, the surrounding link's OSC 8 hyperlink
target is NOT carried onto it — only literal-TeX-fallback math inherits the
enclosing link, because that path goes through `push_text` instead. Bold
styling is dropped either way; math is never rendered as styled text.*

## Inline math inside a list item

- First item with $x_1$ and $x_2$ both riding inline.
- Second item where a fraction breaks out: $\frac{a}{b}$ here.
- Third item, plain.

*expect: list items flatten through the same `wrap()` path as paragraphs
(`allow_box: true`), so the first item's two subscripts ride the baseline
and the second item's fraction breaks out onto its own row(s) inside the
list — list markers should not disturb this rule.*

## Inline math inside a table cell (disabled by design)

| Formula | Description |
| --- | --- |
| $a+b=c$ | Simple sum |
| $\frac{a}{b}$ | A fraction |

*expect: math inside table cells is DISABLED by design — `crates/layout/src/table.rs`
flattens cell content with `allow_box: false`, so `try_size` always returns
`None` for both rows above regardless of formula height. Every cell must
show the literal `$…$` TeX source as plain text, NOT a rendered image or
txm grid, and NOT the bare `a+b=c` with the dollar signs stripped.*

## Inline math inside a blockquote

> A quoted claim: $e^{i\pi}+1=0$ is one of the most quoted identities in
> mathematics, and a quoted fraction $\frac{a}{b}$ appears right after it.

*expect: same rule inside a blockquote — the 1-row `e^{i\pi}+1=0` (if it
still measures 1 row with the extra terms) rides the baseline, while the
fraction breaks out onto its own row(s), still indented under the quote
marker.*

## Display math: simple, tall, and wide

Simple display math:

$$E = mc^2$$

*expect: this measured cols 7, rows 1 — SAME as an inline formula of the
same shape. The `display: true` flag only affects markdown semantics (it
came from `$$…$$` instead of `$…$`); the `wrap()` boundary never inspects
that flag, only the measured `CellSize`. So this one-row display formula
rides the baseline as the sole content of its paragraph line, rather than
opening a taller reserved block. This is worth double-checking visually —
it is easy to assume all `$$…$$` blocks reserve their own rows, and that
assumption is false at this size.*

Tall display math (nested continued fraction):

$$\cfrac{1}{1+\cfrac{1}{1+\cfrac{1}{1+\cfrac{1}{1+x}}}}$$

*expect: measured cols 14, rows 6 — well past the 1-row boundary, so this
claims its own reserved block of rows, growing downward from the paragraph.*

Very wide display math (must scale to fit the content column):

$$a_{1} x^{1} + a_{2} x^{2} + a_{3} x^{3} + a_{4} x^{4} + a_{5} x^{5} + a_{6} x^{6} + a_{7} x^{7} + a_{8} x^{8} + a_{9} x^{9} + a_{10} x^{10} + a_{11} x^{11} + a_{12} x^{12} + a_{13} x^{13} + a_{14} x^{14} + a_{15} x^{15} + a_{16} x^{16} + a_{17} x^{17} + a_{18} x^{18} + a_{19} x^{19} + a_{20} x^{20} = 0$$

*expect: measured cols 118, rows 1. At any normal terminal width (roughly
80-120 columns) this is wider than the content column even though it is
only 1 row tall, so the OTHER half of the `wrap()` boundary condition
(`cols <= content_width`) fails and it still breaks out to its own reserved
row, scaled down to fit. (If your terminal is unusually wide — comfortably
over ~120 columns — it could instead fit and ride the baseline; narrow the
window to see the intended breakout behavior.)*

## Matrices, aligned environments, and cases

$$\begin{pmatrix} a & b \\ c & d \end{pmatrix}$$

$$f(x) = \begin{cases} 1 & x > 0 \\ 0 & x = 0 \\ -1 & x < 0 \end{cases}$$

$$\begin{aligned} x &= 1 \\ y &= 2 \end{aligned}$$

*expect: all three measured multiple rows (pmatrix: rows 2, cases: rows 4,
aligned: rows 3), so all three break out onto their own reserved block —
none of these should ever ride a text baseline.*

## Greek letters, operators, and big operators

Inline: $\alpha + \beta = \gamma$, $\forall x \in \mathbb{R}$,
$\nabla \cdot \vec{F} = 0$, and $a \leq b \neq c \approx d$.

*expect: all four measured 1 row (narrow: cols 8, 6, 7, and 10
respectively) — all ride the baseline in this sentence.*

Big operators with limits, as display math:

$$\prod_{i=1}^n a_i$$

$$\bigcup_{i=1}^{n} A_i$$

*expect: both measured rows 3 (the stacked sub/superscript limits push the
height past 1 row) — both break out onto their own reserved rows, unlike
the plain Greek letters and relation operators above.*

## Ladder rung 2: RaTeX rejects, `txm` accepts

$\dv{y}{x}$ uses physics-package derivative notation that RaTeX's KaTeX-derived
parser does not implement, but `txm`'s smaller renderer does.

*expect: RaTeX parsing fails (verified: `intrinsic_em_size` returns `None`
for `\dv{y}{x}`), so this falls to the SECOND rung — a plain Unicode
text-grid rendered by `txm` (verified: `render_text` succeeds, cols 4, rows
1) — NOT a PNG, and NOT the raw literal TeX source.*

## Ladder rung 3: both reject — literal TeX source shows

Deliberately malformed: $\notarealcommand{$ here.

*expect: the tex content between the dollars is the malformed
`\notarealcommand{` (an unknown command with an unclosed brace — written as
`$\notarealcommand{$` so the `$…$` span still closes and the malformed
content survives verbatim into the math node). Verified directly: BOTH
`math::intrinsic_em_size` and `math::render_text` return `None` for this
exact string, so this must fall all the way to the THIRD rung — the raw TeX
source `\notarealcommand{` shown as literal text, dollar signs and all,
with no image and no txm grid.*

## An absurdly long formula near the pixmap bound

The formula below is 4095 characters — one under `math::MAX_TEX_LEN`'s 4096
cap — so it is still attempted rather than rejected outright for length,
but its raw em-width is so large that the sizer's `MAX_RESERVED_COLS` clamp
(200 cols) kicks in:

$$x + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + = y$$

*expect: RaTeX still parses this (verified: `intrinsic_em_size` succeeds),
producing an enormous raw width that the sizer clamps to `MAX_RESERVED_COLS`
= 200 columns (verified: measured cols 200, rows 1) — this is the pixmap
bound in action. It must break out onto its own reserved row (200 columns
is wider than any real terminal's content column) and must not hang, panic,
or silently vanish while RaTeX/`txm`/the rasterizer chew on nearly 4 KiB of
TeX.*

## Prose dollar signs that must NOT be parsed as math

Verified against this checkout's actual inline parser
(`crates/ast/src/parser/inline.rs`'s `handle_dollar`): a `$` only opens math
when the very next character is non-whitespace, and only closes when a
matching `$`/`$$` run is found whose preceding character is also
non-whitespace. Both sentences below fail that closing condition (a space
sits right before the second `$`), so no math node is produced at all —
confirmed by parsing them and finding zero `Math` nodes in the resulting
tree.

Prices in prose: I paid $5 and $10 for these two items, not fifty dollars.

Escaped and unclosed: I have \$5 saved, and here's a stray $5 with no
matching close anywhere in this sentence.

*expect: every `$` above renders as a literal dollar sign in plain text.
None of this should be treated as math, sent to RaTeX, or shown as a
missing-formula placeholder — it's just prose with dollar signs in it.*
