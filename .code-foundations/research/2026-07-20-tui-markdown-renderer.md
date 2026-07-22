# TUI Markdown Renderer — Build Requirements Research

**One sentence:** The two hypotheses that would most cheaply kill this project — that glamour already covers the ground, and that display width is a solved lookup — are both false, but the evidence gathered so far covers only D1 (glamour/glow), D2 (protocols, tmux, Alacritty) and D3 (text measurement); D4–D7 are **unresearched in this pass** and the go/no-go cannot be closed without them.

**Date:** 2026-07-20 · **Status:** draft · **Verification:** 27 claims survived 3-vote adversarial verification against primary sources (library source at pinned tags, protocol specs, Unicode annexes, GitHub API); 14 claims were refuted and are listed in *Refuted during verification*.

**What remains open:** D4 (incremental parsing / streaming render), D5 (layout and table engines), D6 (feature surface, math, mermaid), D7 (ecosystem comparison) have **zero verified findings**. Falsification claims 3, 5 and 6 are therefore unresolved.

Prior art beyond glamour/glow — mdcat, rich, textual/frogmouth, render-markdown.nvim, bat — was not opened, and per scope decision **S1** it will not be: the project now proceeds on the assumption that nothing existing does the job. This is an *assumption*, not a finding.

Scope decisions **S1–S3** (below) removed the multiplexer and cross-terminal hard edges by narrowing the target to raw Ghostty. They did not touch D4–D7, which is where the remaining engineering risk lives.

---

## Scope decisions (2026-07-21)

Taken by the user after reading the D1–D3 findings. These are *decisions*, not findings — they narrow what must be researched and built, and each carries a stated cost if later reversed.

| # | Decision | What it kills | Cost to reverse |
|---|---|---|---|
| S1 | **Assume no existing tool does what we want.** Stop treating the prior-art survey as a kill-check. | Open question 1 (mdcat, rich, textual/frogmouth, render-markdown.nvim, bat). D1 stays a one-library finding and that is now acceptable. | If one of them *does* retain a layout tree or stream, the novelty argument and possibly the ecosystem choice change. Unquantified — nobody opened them. **Re-examine in light of S4:** this waiver was taken while streaming was the differentiator. With streaming out of scope, the nearest existing tools are glow's TUI pager mode (scrolls, but glamour-backed — so no images and no reflow) and frogmouth (a Textual markdown browser, Python). Neither was ever opened. The gap they'd have to fail to fill is now narrower than it was when S1 was taken. |
| S2 | **Support a min/max width**, not arbitrary width. | Nothing in D3. This is a **layout** bound, not a measurement fix — see the note below. Narrows D5's table-overflow strategy space. | Low. |
| S3 | **Raw Ghostty is the must-work target.** Images need not work under tmux or herdr; degradation there is acceptable. | The two largest hard edges: *image lifecycle under a multiplexer* and *multiplexers defeat correction tables*. Also collapses *capability detection is fragmented* to a single known target. | **High, and this is the load-bearing one.** Every hard edge in D3 is a statement about disagreement *across* terminals. One target means one width table, one 2027 answer, one graphics protocol. Adding a second supported terminal reintroduces most of D3's cost. |
| S5 | **Build our own rather than adopt.** Open-source implementations are reference material, not dependencies. Hard parts are to be accounted for up front rather than delegated to a crate. | Most of D7 as a *blocker*. "Which crate" stops mattering for anything written in-house; D7 now matters only for the short adopt list below. | Low for the own list — that code is the product. **High for the adopt list**: Unicode table regeneration and image codec maintenance are unbounded treadmills with zero product differentiation, and owning them means owning them forever. |
| S4 | **Documents at rest. The PDF-viewer model.** Open a complete document, parse it fully, build a layout tree, render a viewport, repaint on scroll and resize. **No incremental parse of a growing input.** | Nearly all of D4 — the stable-prefix problem, resumable parsers, the comrak fork, tree-sitter adoption. Also deletes the hard edge *"CSS auto table layout is formally incompatible with streaming"*: with every row in hand, column widths compute exactly. | **High if reversed.** Retrofitting an incremental parser onto a batch layout tree is the expensive direction. Deciding this late costs far more than deciding it now. |

**S4 removes the incremental *parse* only — not the rendering architecture.** A viewport that scrolls and resizes still needs a retained layout tree and a differential cell-grid repaint. ratatui supplies neither (part 2, claim 6), so that work is unchanged by this decision. What S4 deletes is the requirement to render a document *prefix* whose meaning later input can invalidate.

**S4 also weakens, but does not overturn, the case against glamour.** Batch-only was one of four disqualifiers; under the PDF model it is no longer one. The remaining three still are: no image support at any level, width frozen at construction with no resize path, and no public re-layoutable IR. See the S1 caveat below — the prior-art waiver was taken while streaming was still the differentiator.

**S2 does not solve the width problem, and the doc should not be read as saying it does.** Min/max width bounds reflow; it does not tell you how many cells a ZWJ sequence occupies. S3 is what collapses D3 — from ~35 disagreeing implementations to one terminal that scores 100 on ZWJ in ucs-detect, ships in `wcwidth`'s `KNOWN_TERMINALS` correction table, and implements mode 2027 (opt-in — `modes.zig:297` carries no `.default = true`).

**What S1–S3 do *not* touch:** D4 (streaming and incremental parsing), D5 (table layout algorithm), D6 (math, mermaid, single-binary distribution), D7 (language). The hard core of the project is entirely in the unresearched half.

**Carried forward as an unverified dependency:** Ghostty's kitty-graphics support rests on secondary signal only (kitty's implementer list; terminfo.dev listing Ghostty 1.3.1). Neither was opened as a primary source. S3 makes this a single point of failure for the entire image feature — verify against Ghostty's source or docs before committing.

---

## D1 — Prior art, verified

Only **glamour** (the Charm markdown renderer behind glow, and the de-facto Go default) and **glow** were verified. Everything below is from source at pinned tags, not reputation.

### glamour's API is a width-baked, batch string transformer

| Fact | Evidence | Confidence |
|---|---|---|
| Top-level entry point is `Render(in string, stylePath string) (string, error)` — parameterized by **style**, not width. Width is a construction-time option `WithWordWrap(int)`. | [pkg.go.dev/github.com/charmbracelet/glamour](https://pkg.go.dev/github.com/charmbracelet/glamour); `glamour.go` L172-175 on main | high |
| Width is frozen at `NewTermRenderer` time: `WithWordWrap` mutates `tr.ansiOptions.WordWrap` (L173-175), and `ansi.NewRenderer(tr.ansiOptions)` (L88) binds it into goldmark at L89-95. There is **no exported width setter or resize method** — changing width requires constructing a new renderer. | `glamour.go` main + `ansi/renderer.go` L19, L33 | high |
| The top-level `Render`/`RenderBytes` do not even expose width — they call `NewTermRenderer(WithStylePath(stylePath))` only (L57-65) and silently take `defaultWidth = 80` (L25). Stronger than the hypothesis assumed. | `glamour.go` L25, L57-65 | high |
| No reflowable IR is public. `ansi.ElementRenderer` is `Render(w io.Writer, ctx RenderContext) error`; `RenderContext` has only unexported fields; `Options` holds config only (BaseURL, WordWrap, TableWrap, InlineTableLinks, PreserveNewLines, ColorProfile, Styles, ChromaFormatter). goldmark's AST is never surfaced; rendering is one-pass to an `io.Writer`. | pkg.go.dev `glamour/ansi` | high |
| **glamour v2 does not change this.** v2.0.0 (2026-03-09), v2.0.1 (2026-06-12). The upgrade guide's only rendering idiom is `NewTermRenderer(WithWordWrap(80))` → `Render(markdown)`, repeated at four places (L88-90, L101-102, L185-188, L212-215). `grep -niE "stream\|incremental\|resize"` over the whole guide: **zero hits**. | [UPGRADE_GUIDE_V2.md](https://github.com/charmbracelet/glamour/blob/main/UPGRADE_GUIDE_V2.md) | high |

### The `io.ReadWriter` surface is batch accumulation, not streaming

`TermRenderer` satisfies `io.Writer`/`io.Reader`/`io.Closer`, which reads as a streaming API and is not one. `Write` does nothing but `tr.buf.Write(b)` — zero parsing per write. The sole parse is one `tr.md.Convert(tr.buf.Bytes(), &tr.renderBuf)` inside `Close`. `renderBuf` is written only by `Close`, so a `Read` before `Close` drains an empty buffer and returns `io.EOF`. Identical on v1 (`master`) and v2 (`main`). Confidence: **high**. Source: `glamour.go` L246-264, main.

> pkg.go.dev's auto-generated summary describes these three methods as "an incremental API for streaming." That summary is wrong; the source contradicts it. Do not trust doc-summary tooling on this point.

### glamour emits no images, at the option level and the implementation level

The complete exported option surface at v1.0.0 is 16 constructors (`WithStandardStyle, WithAutoStyle, WithEnvironmentConfig, WithStylePath, WithStyles, WithStylesFromJSONBytes, WithStylesFromJSONFile, WithBaseURL, WithColorProfile, WithWordWrap, WithTableWrap, WithInlineTableLinks, WithPreservedNewLines, WithEmoji, WithChromaFormatter, WithOptions`) — verified set-identical against the tagged source, and the root package has no second Go file that could add more. The strings `image`, `kitty`, `sixel`, `iterm`, `graphics` appear nowhere. At v2.0.1 the surface **shrank to 14** (`WithAutoStyle` and `WithColorProfile` removed) and still contains no graphics option. Implementation-level confirmation: [`ansi/image.go`](https://raw.githubusercontent.com/charmbracelet/glamour/v1.0.0/ansi/image.go) renders `ImageElement` as alt text (`ImageText` style) plus the resolved URL as text — no image payload is ever emitted. Confidence: **high**.

*Nit for downstream prose:* glamour self-describes as targeting "ANSI compatible terminals," which is not itself an exclusion of graphics protocols (kitty/sixel payloads are escape sequences delivered to ANSI-compatible terminals). The option enumeration and `ansi/image.go` carry the finding independently of that phrase.

### glamour v2 is deliberately terminal-blind

v2 removed runtime terminal probing from the renderer: `WithAutoStyle()` and `WithColorProfile()` are gone; color adaptation is delegated to Lip Gloss. The maintainers' framing: *"Glamour is now pure — it always produces the same output for the same input."* Independently verified in code at tag v2.0.1: across `glamour.go`, `styles/styles.go` and all 13 `ansi/*.go`, a grep for `os.Getenv|IsTerminal|term.|Query|ioctl|Winsize|colorprofile|termenv` yields exactly **one** hit — `glamour.go:280 os.Getenv("GLAMOUR_STYLE")`. Zero isatty/TIOCGWINSZ/DA-query calls; `colorprofile` and `x/term` are `// indirect` in go.mod. `defaultWidth = 80` is a hardcoded const and chromaFormatter defaults to the literal `"terminal256"` (`ansi/codeblock.go:18`). Confidence: **high**.

Two precision corrections: (a) "environment-blind" is loose — `GLAMOUR_STYLE` is still read, via the opt-in `WithEnvironmentConfig()` path; **terminal-capability-blind** is the accurate phrase. (b) "default style is hardcoded dark" is true of `getEnvironmentStyle()` (L279-286) but `NewTermRenderer()` with no style option leaves `ansiOptions.Styles` a zero-value `ansi.StyleConfig` — i.e. unstyled, not dark.

**Consequence for a builder:** glamour v2 emits unconditional truecolor ANSI; downsampling is the caller's job. Combined with the 80-column default and the batch string return, glamour is a text-formatting function, not a renderer component.

### Charm rejected streaming markdown in glow on design grounds

[glow PR #823](https://github.com/charmbracelet/glow/pull/823) "feat: add high-performance streaming markdown renderer" (anthonyrisinger, opened 2025-09-12, 46 files, +15,884) was **closed unmerged** 2025-09-22T18:13:27Z by `caarlos0`, with: *"After talking about this with the team, we decided can't move forward with this approach at this time. We have another thing in the works that would make this simpler to implement, but we don't have a timeline for it yet."* Verified via GitHub API (`gh pr view 823`), not the HTML page — WebFetch on the PR URL returns GitHub's error shell, so page-scrape verification of this PR is unreliable.

The mechanism rejected was a `--flow=N` buffer window (author: *"conceptually like a buffer window … the 'smallest possible chunk' we MUST gather before we will consider it a candidate for a 'safe split boundary'"*). The maintainer's pre-close review objected to it specifically: *"it seems that small `--flow` values might cause issues? … it didn't syntax highlight or handle the h2 properly. also, I think having the user decide the buffer size is a bit weird."*

Still true ten months later: `gh api repos/charmbracelet/glow/git/trees/main?recursive=1` returns no path matching `/stream|flow/`; issues #601 and #772 remain open; successor PR #939 (opened 2026-04-27) is still open/unmerged; glamour v2.0.1 ships no streaming API. Confidence: **high**.

Two honest qualifications: "another thing in the works" has **no stated referent** — the attribution to glamour comes from the PR author's speculation, not the maintainer. And `caarlos0`'s `authorAssociation` on this PR is CONTRIBUTOR, not MEMBER; the plural "maintainers" rests on his report of a team decision, not on multiple named maintainers acting.

### Not researched

mdcat, rich, textual/frogmouth, render-markdown.nvim, bat. **No finding either way** on whether any of them retains a layout tree or streams. This is the single largest D1 gap.

---

## D2 — Images and graphics

### kitty graphics protocol — verified properties

| Property | Finding | Source |
|---|---|---|
| **Runtime detection** | Send a query action (`a=q`) followed immediately by a primary Device Attributes request. If DA1 answers and the graphics query does not, the terminal does not support the protocol. Concrete probe: `<ESC>_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA<ESC>\<ESC>[c`. Conformant terminals **must** reply to query actions immediately without processing other input. `a=q` does not store or replace an image, so the probe is non-destructive. | [graphics-protocol.rst](https://raw.githubusercontent.com/kovidgoyal/kitty/master/docs/graphics-protocol.rst) L431-453 |
| **Transmission cost** | Remote/ssh clients can only use direct (escape-code) transmission — the other three mediums (`f` file, `t` temp file, `s` shared memory) require the client on the same machine. Direct mandates base64 then chunking into ≤4096-byte chunks, all but the last a multiple of 4, `m=1` on all but the final chunk. Floor: **4/3 of the *encoded* payload** plus ~11 bytes per 4096-byte chunk (~0.3%). | same, transmission section |
| **Color fidelity** | Terminal **must** understand 24-bit RGB (`f=24`), 32-bit RGBA (`f=32`, the default) and PNG (`f=100`); data **must** be sRGB. No quantization is imposed anywhere in the spec — an implementation can hand off unquantized pixels. | same, `f` key |

Confidence: **high** on all three; primary vendor spec authored by the protocol designer, fetched from master 2026-07-20.

Qualifications that must survive into any design doc:

- **APC safety is "most," not "all."** The spec says *"Most terminal emulators ignore APC codes, making it safe to use"* and explains base64 exists specifically to avoid *"interoperation problems with legacy terminals that get confused by control codes within an APC code."* The claim that non-supporting terminals ignore APC is one notch stronger than the spec.
- **The probe needs a read timeout.** A terminal answering neither DA1 nor the graphics query hangs the probe forever. The spec does not mention this.
- **Compression exists.** zlib deflate (`o=z`) and PNG (`f=100`) are supported, so the 4/3 tax applies to compressed bytes, not raw RGBA. Stating "4/3 of the payload" against raw pixel data would overstate cost by an order of magnitude.
- **Chunking is conditional in the spec, unconditional in practice.** 4096 base64 bytes = 3072 raw = roughly a 32×32 RGBA image, so every real image chunks.
- **Color is lossless, geometry is not.** Bounded at 8 bits/channel sRGB (no 16-bit path documented); wide-gamut sources must be converted, which is itself lossy. And the `c`/`r` keys make the terminal scale to the cell grid with an **unspecified resampling algorithm**.

Sixel comparison (checked adversarially, held up): sixel is genuinely palette/color-register limited — xterm defaults to 16 registers and needs `XTerm*numColorRegisters: 256`; mlterm's console defaults to 16. A portable truecolor sixel escape hatch was searched for and not found. Confidence: **medium** (implementation docs, not a single spec).

### iTerm2 OSC 1337 — detection and geometry

- **Version detection:** `CSI > q` (Extended Device Attributes / XTVERSION, xterm patch 230). iTerm2 replies `ESC P > | iTerm2 [version] ST` — a DCS-wrapped name+version string. Confidence: **high**, [iterm2.com/documentation-escape-codes.html](https://iterm2.com/documentation-escape-codes.html), fetched raw (31,258 bytes) and tag-stripped rather than summarized.
- **Cell geometry:** `OSC 1337 ; ReportCellSize ST` → `OSC 1337 ; ReportCellSize=[height];[width];[scale] ST`, e.g. `17.50;8.00;2.0`. **Height and width are in points, not pixels** — pixels-per-cell is `height × scale`, derived not returned. Older iTerm2 omits `[scale]` entirely, so **two response shapes must be parsed**. WezTerm issue #2085 is a request to implement this, i.e. it is the de facto reference mechanism; iTerm2 issue 9060 reports `CSI 14t`/`18t`-derived math disagreeing with it, with the reporter concluding ReportCellSize is the more accurate path. Confidence: **high**.

### In-band capability reporting covers sixel and OSC-1337-File, not kitty

[iterm2.com/feature-reporting](https://iterm2.com/feature-reporting/) defines exactly 18 boolean feature codes. The only two image-bearing ones are `SIXEL=Sx` and `FILE=F` (the `OSC 1337 File` inline-images protocol, which covers *files including video*, not images only). `grep -c -i "kitty"` over the raw 62,835-byte HTML returns **0** — a renderer cannot learn kitty-graphics support from this mechanism. Confidence: **high**.

Three caveats:
1. **The spec as published assigns code `F` to both `FOCUS_REPORTING` and `FILE`.** That is a real collision in the primary source. A renderer parsing the FeatureString cannot unambiguously distinguish focus reporting from inline-image support from a bare `F`.
2. Kitty has its own in-band detection (the `a=q` probe above), so "not detectable via this mechanism" ≠ undetectable.
3. Adoption is partial — this is an iTerm2 proposal derived from the defunct terminals-wg work; mintty issue #1341 is still an *open request* to implement it. Do not treat it as a universal detection path.

**Sixel conformance is defined behaviorally**, not by declaration: *"printing `snake.six` should cause this image to be rendered … starting at the cursor's location … and produce a graphic substantially similar to `snake.png`."* So `Sx` asserts rendering fidelity, not sequence parsing. Two limits: the spec says *should*, not *must*; and conformance is pinned to one reference image, so `Sx` asserts nothing about transparency, private color registers, DECSDM, or scrolling. Separately, terminals are known to misreport in practice — blessed's `get_sixel_colors()` defaults to 256 when XTSMGRAPHICS goes unanswered. Confidence: **high** on the definition, **medium** on operational trustworthiness.

### tmux: kitty graphics is unmerged proof-of-concept

Verified against GitHub API 2026-07-20 ([tmux#4902](https://github.com/tmux/tmux/issues/4902)):

- Issue state `open` (`state_reason: reopened`), created 2026-03-01, last activity 2026-03-12, 14 comments.
- tmux master image files are `image-sixel.c`, `image.c` **only**. `image-kitty.c` exists solely on branch `ta/kitty-img`.
- `ENABLE_KITTY_IMAGES` occurs **zero** times on master (configure.ac, Makefile.am, image.c, tmux.h all 0); 3/2/4/6 times respectively on the branch. GitHub code search `ENABLE_KITTY_IMAGES repo:tmux/tmux` → `total_count: 0`.
- `compare/master...ta/kitty-img`: status `diverged`, ahead 8, **behind 965**. Branch head dated 2026-03-13; master HEAD 5ed5e360 dated 2026-07-20T17:17:19Z.

Confidence: **high**.

**Critical qualification — do not overstate this.** tmux master *does* ship `allow-passthrough` (verified: `options-table.c:1257`), so kitty graphics can be tunnelled through tmux today via DCS passthrough (`kitten icat --passthrough=tmux`) with tmux unaware of them. "kitty images do not work through tmux at all" is **false**. The correct statement is that tmux does not *manage* them.

### What unmanaged passthrough actually breaks

tmux maintainer nicm, testing the in-development branch (2026-03-12T08:32:14Z): *"I see the image but it seems like it not being managed by tmux: — The image crosses over between multiple panes instead of being cropped to stay within one pane. — `C-b r` does not redraw it (removes the images)."* (`C-b r` is confirmed the default `refresh-client` binding: tmux `key-bindings.c:434`.)

Independently corroborated by the protocol's author: kovidgoyal, [kitty discussion #4021](https://github.com/kovidgoyal/kitty/discussions/4021), states classic placement uses absolute cursor position at transmission time, so *"anything like changing the window layout in tmux or resizing the terminal or scrolling will gum things up royally."* Unicode placeholders (U+10EEEE) exist specifically because classic placement is unmanaged by multiplexers. Further corroboration: kitty#2457 "Images/graphics are not cleared up inside tmux," chafa#195.

Confidence: **high**. Scope caveat: nicm's test used `kitten icat --passthrough=none --transfer-mode=stream` against the in-progress `ta/kitty-img` branch, not stock tmux with `allow-passthrough on`. Cite it as "tmux's in-progress kitty passthrough," not as a measurement on released tmux.

**This is the load-bearing D2 finding for the build decision:** image *lifecycle* under scroll, pane resize and differential repaint is unsolved at the multiplexer layer by anyone, and the remedy nicm named — caching images in `image.c` "more like SIXEL" — is unmerged work on a branch 965 commits behind master.

### Alacritty is a permanent hole

- [PR #4763](https://github.com/alacritty/alacritty/pull/4763) "Support for graphics in the terminal" (ayosec): created 2021-02-05, `state: open`, `merged: false`, `merged_at: null`, 255 comments, 85 commits, last updated 2026-06-26. Verified via `gh api`.
- Absence of support verified independently of the PR: Alacritty's `CHANGELOG.md` on master has **zero** sixel/kitty-graphics/image-protocol entries (only unrelated hits: DEC special graphics line-drawing charset, macOS GPU switching). `extra/man/alacritty-escapes.7.scd` on master (added in 0.17) has no match for sixel, 1337, graphic, image, or APC/`_G`. Latest release 0.17.0, 2026-04-06. Third-party: arewesixelyet.com lists Alacritty "Unsupported"; kitty's own implementer list omits it.
- **The refusal is on design grounds, from the lead maintainer, repeatedly and across years.** chrisduerr, 2023-08-19: *"This PR not being merged is all about image protocols being bad, not about maintainers not having time."* 2024-05-28: *"This is a massive patch for a mediocre protocol… I don't see it being a reasonable upstream addition."* 2024-05-29: *"First of all the protocol is garbage… ~3k lines is a significant portion of Alacritty's code."* 2025-08-29: *"I think I've made it pretty clear that this has little chance of ever getting upstreamed."* Retrieved via `gh api repos/alacritty/alacritty/issues/4763/comments --paginate` — **the rendered HTML page does not surface these comments**; a WebFetch pass falsely reported the quote absent.

Confidence: **high**. Three qualifications: "permanent" is a hardening — his words are probabilistic ("little chance of *ever*"), the PR is deliberately kept open, and he names a condition ("any implementation should be simple"). His objection is partly ongoing-maintenance cost, which is a resource argument of the *burden* kind, not the *queue* kind he explicitly denied. And kchibisov's position is undocumented in the thread, so this is one maintainer's veto, not a recorded project policy. The operational conclusion is unaffected: **a builder cannot plan around Alacritty gaining image support on any forecastable timeline.** Footnote for a matrix: ayosec's fork and distro packages named `alacritty-graphics` (v0.16.1 seen Jan 2026) do add Sixel + iTerm2 protocols.

### Emulator support matrix — INCOMPLETE

Only Alacritty (no support, permanent) and tmux (passthrough-only, unmanaged) were verified end to end. Partial signal from kitty's own implementer list (Ghostty, Konsole, st-patch, Warp, wayst, WezTerm, iTerm2, xterm.js) and from terminfo.dev (6 of 12 surveyed terminals implement kitty graphics: iTerm2 3.6.9, Ghostty 1.3.1, kitty 0.46.2, Warp, Terminal.app, WezTerm; Alacritty explicitly not) — **neither was opened as a primary source in this pass**. Windows Terminal, foot, and GNU screen were **not researched at all**. Treat the matrix as an open item, not a finding.

---

## D3 — Text measurement

This is the best-evidenced dimension and the one that most changes the build decision.

### Display width is empirically not portable

Measuring ~35 terminals with `ucs-detect`, Jeff Quast (maintainer of Python `wcwidth`, `ucs-detect`, `blessed`) found **23 distinct implementations of which codepoints are Wide, 21 of language grapheme support, 19 of ZWJ-joined complex emoji, and 7 and 6 distinct implementations of VS-15 and VS-16 widths**. [Source](https://www.jeffquast.com/post/perfecting-terminal-character-width-using-correction-tables/), published 2026-06-07. Corroborated by the per-terminal table at [ucs-detect.readthedocs.io/results.html](https://ucs-detect.readthedocs.io/results.html) — 40 terminals scored across WIDE/NARROW/LANG/ZWJ/VS16/VS15/SRI/SFZ/RI, spread from kitty at 100 across the board to ConEmu at WIDE 44 / LANG 0 / ZWJ 0. Confidence: **high** (primary measurement by the instrument's author).

**Necessary qualifier:** those counts are the baseline **without** DEC mode 2027. Among 2027-enabled terminals the counts fall to 5 (WIDE) and 6 (LANG) — still >1, so non-portability holds, but quoting 23/21/19 unqualified overstates the present-day worst case. The results page also notes kitty's Text Sizing protocol "allows any application to programmatically set character widths," a newer mechanism that sidesteps measurement entirely.

### DEC mode 2027 is an opt-in draft, and enabling it does not buy agreement

The spec is [contour-terminal/terminal-unicode-core](https://github.com/contour-terminal/terminal-unicode-core), read in full (`spec/terminal-unicode-core.tex`, 211 lines).

**What it mandates when on** (L120-121: *"MUST be adhered to if this VT mode 2027 is enabled"*):
- UAX #29 grapheme clustering (L129-130), so a non-breakable sequence lands in the same grid cell (L133-135) and extending a cluster does not move the cursor (L138-140) — **except for VS16, which may change the width to wide (2 cells)**. That exception is materially relevant and is easy to miss.
- Emoji rendered square, implying East Asian Width **Wide, 2 cells** — derived from UTS #51, with UTS #11 cited only for the width category (L153-155).
- ZWJ emoji **must** render as a single 2-cell image; decomposed sub-image fallback *"must not be used as a fallback as it will break cursor movement guarantees"* (L158-161).

**What it does not do:**
- **Off by default, and undefined when off.** L122-124: *"If the VT mode 2027 is not set, then the behavior is as undefined as if this specification was not implemented at all."*
- **Both DECSTR (soft reset) and RIS (hard reset) revert it** (L204-207). Since mode state is terminal-global, any child process writing a reset silently reverts it for the parent, and the spec defines no notification — an app learns only by re-querying with DECRQM `CSI ? 2027 $ p`. **An application cannot set the mode once and assume it persists.** Corroborated in shipped code: Ghostty `src/terminal/modes.zig:297` lists `grapheme_cluster` = 2027 with **no** `.default = true` (siblings `wraparound` L264 and `cursor_visible` L268 carry it), and `Terminal.zig:4316-4328 fullReset()` calls `self.modes.reset()`. *The DECSTR half rests on spec text alone — no `DECSTR`/`softReset` symbol was found in Ghostty's `Terminal.zig` or `stream.zig`.*
- **No Unicode-version negotiation.** The spec explicitly acknowledges the hazard and defers it, under a section titled *"Future Compatibility and Stability"*: *"Unicode itself had a major breakage at version between version 8 and 9 with regards to some codepoints having their east asian width changed. While this may happen any time again, we do not expect that to happen that soon nor that frequent to address future incompatibilities as of this spec and leave this for a later point."* A grep for `negotiat|version` returns only those two lines. It is worse than that: the macro `\newcommand{\Unicode}{\textbf{Unicode 13}}` is defined at L44 and **never used in the body** — the spec does not normatively pin a Unicode version at all, and cites UTS-29/11/51 without version qualifiers. Two conformant implementations on different UCD releases are both conformant and can disagree.
- **DECRQM reports mode state, never data version.**

**Status of the document:** authored by Christian Parpart, dated 2021-09-06, watermarked "Draft", revision 1, `Changelog.md` reads "0.1.0 (unreleased) — initial draft", only tags are `v0.1.0_prerelease_1/2`, contributors are christianparpart (12), jquast (3), dependabot (1), and there is **no conformance or implementer list anywhere in the repo**. The repo was pushed 2026-06-23 and is still revision 1 after five years. Characterize it as **a draft proposal terminals adopted de facto, not a ratified standard.** Confidence: **high**.

**Empirically, enabling 2027 does not deliver agreement.** From ucs-detect measurement: among mode-2027-enabled terminals, **Contour measures 54 ZWJ emoji as Narrow instead of Wide**, and **foot, WezTerm and Windows Terminal measure standalone Regional Indicators as Narrow instead of Wide**; Contour and WezTerm also miss VS16 for some early emoji, and foot and Windows Terminal measure Fitzpatrick modifiers as width 1 instead of 2. The source scopes these explicitly to 2027-enabled terminals — this is not the report author's inference. The published results table shows Contour ZWJ 96 vs foot/ghostty/WezTerm 100. Confidence: **high**.

> Reading the results table requires care: `sri` (a lone Regional Indicator) and `ri` (flag pairs) are **separate test categories**. RI=100 for foot/WezTerm/Windows Terminal does not contradict the SRI finding. Also, that table pins WezTerm at build 20240203 — per-terminal numbers are version-sensitive and any matrix must cite versions. The "54 emoji" count rests solely on one ucs-detect run; it was not independently re-derived.

### UAX #11: Ambiguous has no context-free width

TR11 is Unicode **17.0.0, revision 44, dated 2025-07-24** — current. It defines Ambiguous (A) as *"All characters that can be sometimes wide and sometimes narrow… require additional information not contained in the character code,"* and prescribes: *"If the context cannot be established reliably, they should be treated as narrow characters by default."*

The affected ranges were verified against the machine-readable UCD (`EastAsianWidth.txt`), not prose: `0391..03A1 ; A`, `03A3..03A9 ; A` (Greek capitals), `0410..044F ; A` (Cyrillic), `2500..254B ; A` (box drawing), `2190..2194 ; A`, `2195..2199 ; A` (arrows), `00A7 ; A`, `2020..2022 ; A`. *Precision:* only U+2500..U+254B of the box-drawing block is Ambiguous; U+254C..U+257F is not. "Most box-drawing characters," not all.

That this produces real disagreement is confirmed by shipping terminals exposing it as a knob: xterm's `cjkWidth` / `-cjk_width` — *"characters with East Asian Ambiguous (A) category in UTR 11 have a column width of 2. Otherwise, they have a column width of 1"* — documented as needed for *"legacy CJK terminal programs that expect box-drawing characters to occupy two columns*"; and VTE's `vte_terminal_set_cjk_ambiguous_width(width)`, "Either 1 (narrow) or 2 (wide)." Confidence: **high**.

Counter-position checked: kitty has no blanket ambiguous-width toggle (only `narrow_symbols` for specific PUA codepoints) and argues configuration is the wrong mechanism — *"as of version 0.40 kitty has innovated a new protocol that allows programs running in the terminal to control how many cells a character is rendered in thereby solving the issue of character width once and for all."* That replaces configuration with explicit app→terminal negotiation, which strengthens rather than refutes the "not a constant" conclusion.

### Emoji width and the regional-indicator trap

TR11 ED4 (East Asian Wide) includes *"characters that have the [UTS51] property Emoji_Presentation, with the exception of characters that have the [UCD] property Regional_Indicator."* Cross-checked against the data files: of 1219 Emoji_Presentation code points, excluding the 26 regional indicators, the count with `East_Asian_Width != W` is **exactly 0**.

The carve-out is load-bearing, not decorative. `emoji-data.txt:486` — `1F1E6..1F1FF ; Emoji_Presentation` (RIs **do** carry the property). `EastAsianWidth.txt:2610` — `1F1E6..1F1FF ; N` (yet they are Neutral). A renderer implementing the naive rule "Emoji_Presentation ⇒ 2 cells" computes **4 cells for a two-RI flag sequence** against an intended 2, and additionally needs UAX #29 GB12/GB13 pair-clustering machinery — separate from the width table. Confidence: **high**.

### Runtime probing is a real, shipped technique

`ucs-detect` measures compliance by cursor-position report, verified in source, not just prose:

```python
def measure_width(term, writer, text, timeout):
    """Measure actual rendered width of text using cursor position reports."""
    _, x1 = get_location_with_retry(term, timeout)
    writer(text)
    _, x2 = get_location_with_retry(term, timeout)
    return x2 - x1
```

Raw `ESC[6n` is emitted at `terminal.py:624` and parsed by `_CPR_RE = re.compile(r"\x1b\[(\d+);(\d+)R")` at `terminal.py:594`; the compliance comparison at `measure.py:367-376` records both `"measured_by_wcwidth"` and `"measured_by_terminal"`. Shipped and current: release 2.3.4 dated 2026-06-12, with 40+ committed per-terminal result files under `data/` (ghostty.yaml, kitty.yaml, wezterm.yaml, tmux.yaml, screen.yaml, zellij.yaml, alacritty.yaml, iterm2.yaml, foot.yaml, konsole.yaml…) — evidence the probe was actually run, not just written. [Source](https://github.com/jquast/ucs-detect). Confidence: **high**.

### Per-terminal correction tables ship today

Python `wcwidth` 0.8.2 (PyPI, uploaded 2026-06-29) exports:

```python
wcstwidth(pwcs: str, n: Optional[int] = None, unicode_version: str = 'auto',
          ambiguous_width: int = 1, term_program: bool | str = True) -> int
```

The correction data is a real shipped module, `wcwidth/table_term_programs.py`, with `KNOWN_TERMINALS = {alacritty, apple_terminal, bobcat, contour, extraterm, foot, ghostty, iterm2, kitty, konsole, mintty, mlterm, pterm, rio, st, terminology, urxvt, vte, warp, wezterm, xterm, xterm.js}`, an ALIASES map, and 22 override/known data files. **Verified by executing the wheel**, not by reading the blog: the 4-person family ZWJ sequence `U+1F468 200D U+1F469 200D U+1F467 200D U+1F466` gives `wcswidth = 2` but `wcstwidth(term_program='vte') = 8` and `'xterm' = 8`, while `'iTerm.app' = 2`. `U+2640 U+FE0F` gives 2 plain, 1 under `'vte'`/`'xterm'`. Version archaeology across 0.7.0/0.8.0/0.8.1/0.8.2 wheels: `wcstwidth` is absent in 0.7.0 and present from **0.8.0 (2026-06-05)**. Confidence: **high**.

> **Correction to the widely-cited blog example.** The post's zombie illustration — `wcwidth.wcwidth('🧟‍♂') → 2`, `wcstwidth(..., term_program='VTE') → 8` — **does not reproduce in any shipped release**. The sequence is `U+1F9DF U+200D U+2642` with no VS16; `wcwidth.wcwidth()` on it raises `TypeError: ord() expected a character, but string of length 3 found` (only `wcswidth()` returns 2), and `wcstwidth(..., term_program='VTE')` returns **2**, not 8, in 0.8.0, 0.8.1 and 0.8.2 alike (3 with VS16 appended). Cite the family sequence or `U+2640 U+FE0F` instead.

**Material limit for a builder:** `table_term_programs.py`'s own header states multiplexers are **deliberately excluded** — *"Terminal multiplexers (tmux, zellij, libvterm, screen) are excluded because their displayed presentation depends on the host terminal; cursor-position reports from ucs-detect testing are not reliable indicators of actual width."* Confirmed empirically: `term_program='tmux'` returns uncorrected values. Also note the API asymmetry: `wcstwidth` defaults to `term_program=True` (auto-detect via `TERM_PROGRAM`/`TERM`), while `width()/ljust()/rjust()/center()/wrap()/clip()` default to `term_program=False`.

### iTerm2's width-table declaration — an in-band mitigation, not a query

`OSC 1337 ; UnicodeVersion=[n] ST` where `[n]` is 8 or 9, with `push`/`pop` and labelled `push mylabel`/`pop mylabel` — the labelled form exists *"if a program crashes or an ssh session ends unexpectedly."* The vendor's own stated rationale is *"Since not all apps will be updated at the same time."* Confidence: **high**, [iTerm2 escape codes](https://iterm2.com/documentation-escape-codes.html).

Two corrections to how this is usually described: (a) it is **not iTerm2-specific** — [WezTerm implements the same sequence](https://wezterm.org/config/lua/config/unicode_version.html) (`unicode_version`, default 9) including push/pop and labelled variants, and supports versions **beyond 9**, so the 8-or-9 range is an iTerm2 limitation, not a property of the sequence. Two independent vendors shipping it strengthens the "vendor-acknowledged problem" reading. (b) It is **not a negotiation** — it is a one-way declaration. There is no documented query/report form letting an app read back the emulator's current width-table version, so an app cannot *discover* skew; it can only assert a table and restore it. **Cursor-position probing remains the only detection path.**

---

## D4 — Incremental parse and streaming render

**No verified findings.** comrak, pulldown-cmark, markdown-it, tree-sitter-markdown, cmark-gfm and mdast were not evaluated. Ink, ratatui, notcurses, Textual and Bubble Tea repaint strategies were not examined. The only adjacent evidence is glow PR #823 (D1) — a rejected buffered-window attempt.

Three specific claims about that PR were put to adversarial verification and **failed to survive** (see *Refuted*): that streaming inside block structures is unachievable on glamour's API; that reference-style link resolution is the only semantic divergence between chunked and whole-document render; and that small `--flow` values empirically broke rendering. Failure to survive verification means **not established** — it does not establish the negation. Treat all three as open.

---

## D5 — Layout and tables

**No verified findings.** taffy, yoga, notcurses and Textual's compositor were not evaluated; no table auto-layout algorithm or overflow strategy was researched. Falsification claim 5 is unresolved.

Only adjacent datum: glamour exposes `WithTableWrap` and `WithInlineTableLinks` options and imports `charm.land/lipgloss/v2` for `lipgloss.Wrap` and `table.New` in `ansi/blockelement.go` and `ansi/table.go` (confidence: high, source read during D1) — i.e. it delegates table layout to Lip Gloss rather than implementing a constraint solver. Not a substitute for D5.

---

## D6 — Feature surface

**No verified findings.** The CommonMark + GFM + extension enumeration, math and mermaid rendering paths, and single-binary implications were not researched. Falsification claim 6 is unresolved.

Only adjacent datum: glamour's `WithEmoji` and `WithChromaFormatter` options place emoji substitution and syntax highlighting (Chroma) in the renderer's surface, with `terminal256` as the hardcoded formatter default (confidence: high). Nothing about math, mermaid, footnotes, alerts, definition lists, frontmatter or OSC 8.

---

## D7 — Ecosystem choice

**No verified findings.** Go, Rust and TypeScript were not compared. Incidental Go-side observations accumulated while researching D1 (glamour v1/v2, goldmark as glamour's parser, Chroma for highlighting, Lip Gloss v2 for layout and color downsampling) and Python-side while researching D3 (`wcwidth` 0.8.2 with `wcstwidth` correction tables, `ucs-detect`, `blessed`) are **not** an ecosystem assessment — notably, Python was not one of the three candidate languages, and the strongest text-measurement tooling found in this pass lives there.

---

## Hard edges

| Edge | Why it is hard | Unsolvable or merely unsolved | Mitigation | Conf. |
|---|---|---|---|---|
| **Image lifecycle under a multiplexer** | tmux does not model kitty-protocol images. Passthrough works but is unmanaged: images bleed across pane boundaries, `C-b r` erases rather than repaints. The protocol's own author says layout change, resize or scroll "will gum things up royally." | **Unsolved.** Native support is PoC-only on `ta/kitty-img`, 965 commits behind master, open since 2026-03-01. | Unicode placeholders (U+10EEEE) exist for exactly this; tmux `allow-passthrough on` gets pixels on screen but not correctness. Degrade to alt text under `$TMUX` is the only safe default. | high |
| **Alacritty has no image path, by design** | Lead maintainer has refused upstreaming on protocol-quality grounds since 2021 across four dated statements; PR #4763 open and unmerged for 5.5 years. | **Unsolvable within stock Alacritty** on any forecastable timeline. | Text fallback; note the `alacritty-graphics` fork exists but is not the thing users install. | high |
| **Display width is not portable** | 23 distinct Wide implementations, 21 LANG, 19 ZWJ across ~35 terminals; 7 and 6 for VS-15/16. Mode 2027 narrows this to 5 and 6 — better, not solved. | **Unsolved, and structurally so:** Unicode does not specify terminal behavior. | Per-terminal correction tables (`wcstwidth`, shipped since 0.8.0) + CPR probing (`ucs-detect`, shipped) + mode 2027 where available + kitty Text Sizing where available. All four are partial. | high |
| **Mode 2027 does not survive a child process** | DECSTR and RIS both revert it to undefined, silently, with no notification. Terminal mode state is global to the tty. | **Unsolvable as specified.** | Re-query DECRQM `CSI ? 2027 $ p` after any subprocess that may reset the terminal. | high |
| **Mode 2027 advertisement ≠ correct measurement** | It is a binary support flag. Contour measures 54 ZWJ emoji Narrow; foot/WezTerm/Windows Terminal measure standalone RIs Narrow — all while advertising 2027. | Unsolved. | Correction tables keyed on `TERM_PROGRAM`, plus probe-and-verify for the specific sequences the renderer emits. | high |
| **Regional indicators break the naive emoji-is-wide rule** | RIs carry `Emoji_Presentation` but are `East_Asian_Width = N`. Naive rule yields 4 cells for a flag instead of 2, and needs UAX #29 GB12/GB13 pair clustering separately from the width table. | Solved *if* implemented correctly; a real trap otherwise. | Special-case `1F1E6..1F1FF` and implement GB12/GB13. | high |
| **Ambiguous-width characters** | Greek, Cyrillic, most box drawing (`2500..254B`), arrows, `00A7`, `2020..2022` are Ambiguous. UAX #11 says default narrow when context is unknown; xterm (`cjkWidth`) and VTE (`set_cjk_ambiguous_width`) let users flip it to 2. | **Unsolvable by lookup** — it is context-dependent by definition. | Expose it as a configurable parameter; prefer kitty Text Sizing where the app can assert width. | high |
| **Multiplexers defeat correction tables** | `wcwidth`'s own table module excludes tmux/zellij/libvterm/screen because "their displayed presentation depends on the host terminal" and CPR reports are not reliable indicators there. | Unsolved. | Detect `$TMUX`/`$STY`, fall back to conservative widths, prefer host-terminal identity where discoverable. | high |
| **Capability detection is fragmented** | kitty uses `a=q` + DA1; iTerm2 uses `CSI > q` (XTVERSION) and `OSC 1337 ; ReportCellSize`; the OSC 1337 feature-report covers only Sx and F and **omits kitty entirely**, and its spec assigns `F` to two different features. Terminals also misreport (blessed's sixel-color default). | Unsolved; no single round-trip exists. | Multiple probes, each with a **mandatory read timeout** (a terminal that answers neither DA1 nor the query hangs the probe forever). | high |
| **Cell geometry is derived, with two response shapes** | `ReportCellSize` returns **points** plus a scale factor; older iTerm2 omits scale. Pixels-per-cell = height × scale. Independent methods (`CSI 14t`/`18t`) disagree with it. | Solved with care. | Parse both shapes; prefer ReportCellSize over 14t/18t math. | high |
| **Direct image transmission tax** | Remote/ssh clients cannot use file or shared-memory mediums. Base64 + ≤4096-byte chunks is mandatory: 4/3 inflation on the encoded payload plus ~0.3% chunk overhead. | Unsolvable — no binary-safe direct mode exists. | `o=z` zlib or `f=100` PNG shrinks what gets inflated; the 4/3 applies to compressed bytes. | high |
| **Terminal-side image scaling is unspecified** | The `c`/`r` keys make the terminal scale to the cell grid with **no specified resampling algorithm**. Color is lossless; geometry is not. | Unsolvable via the protocol. | Pre-scale client-side to exact pixel dimensions derived from ReportCellSize. | high |

---

## Falsification results

| # | Claim | Verdict | Evidence |
|---|---|---|---|
| 1 | Every existing renderer uses `render(markdown, width) -> string`, and that choice forecloses streaming, reflow, and image lifecycle. | **Unresolved — substance confirmed for glamour, form refuted, universality and causality untested** | glamour's actual signature is `Render(in string, stylePath string)` — parameterized by *style*, not width, so the literal form is wrong. The *substance* holds and is stronger than stated: width is frozen at construction (`WithWordWrap`, `glamour.go` L173-175), the top-level entry point cannot set it at all and silently takes `defaultWidth = 80` (L25, L57-65), no exported resize method exists, output is a string/bytes, and no re-layoutable IR is public. But "every" rests on one library — mdcat, rich, textual, render-markdown.nvim, bat were not opened — and the causal clause ("*that choice is what forecloses*") was not tested at all. |
| 2 | glamour is batch-only, has no inline image support, and mis-measures CJK and emoji. | **Two-thirds confirmed, one-third unresolved** | **Batch-only: confirmed.** `Write` only appends to `tr.buf`; the sole parse is `tr.md.Convert(tr.buf.Bytes(), …)` inside `Close`; `Read` before `Close` returns EOF. The `io.ReadWriter` surface is misleading, not streaming. **No inline images: confirmed** at both levels — 16 options at v1.0.0 and 14 at v2.0.1 with no image/kitty/sixel/iterm/graphics option, and `ansi/image.go` emits alt text plus URL. **CJK/emoji mis-measurement: unresolved** — the supporting claim (that maintainers acknowledged defective pre-v2 CJK/emoji wrapping) did **not** survive verification, and no measurement of glamour's width behavior was run. |
| 3 | No open-source library does incremental/streaming markdown rendering to a terminal. | **Unresolved** | Only evidence: glow PR #823 was closed unmerged 2025-09-22 by a Charm maintainer, and glow still has no streaming path ten months later (no `stream`/`flow` in the tree; #601, #772 open; successor #939 open; glamour v2.0.1 has no streaming API). That is a finding about *Charm*, not about the ecosystem. D4 was not researched, so this remains "I did not find one," not "none exists." |
| 4 | There is no portable display-width function, only mitigation via probing. | **Confirmed, with the mitigation set broader than "probing"** | 23 Wide / 21 LANG / 19 ZWJ / 7 VS15 / 6 VS16 distinct implementations across ~35 terminals, from the maintainer of `wcwidth`, with a public 40-terminal results table. Mode 2027 is an unratified 2021 draft (0.1.0 unreleased, no conformer list), off by default, reverted by DECSTR/RIS, with no Unicode-version negotiation — and terminals that enable it still disagree (Contour: 54 ZWJ emoji Narrow; foot/WezTerm/Windows Terminal: standalone RIs Narrow). UAX #11 makes Ambiguous width context-dependent by definition; xterm and VTE ship it as a user knob. **Refinement:** mitigation is not only probing — shipped per-terminal correction tables (`wcstwidth`, wcwidth ≥0.8.0), mode 2027, and kitty's Text Sizing protocol are three further partial mitigations, and 2027 narrows divergence to 5/6 implementations. |
| 5 | Terminal table layout is unsolved-in-practice — everyone overflows. | **Unresolved** | Not researched. Only datum: glamour delegates tables to Lip Gloss (`ansi/table.go`) and exposes `WithTableWrap`/`WithInlineTableLinks`. No algorithm or overflow-strategy survey was conducted. |
| 6 | Mermaid rendering requires a JS runtime or shelling to `mmdc`, killing single-binary distribution. | **Unresolved** | Not researched. No evidence gathered either way. |

---

## Refuted during verification

Fourteen claims failed 3-vote adversarial verification and are excluded from the findings above. Failure means *not established by the evidence offered* — it does not establish the negation. The ones a planner should re-test rather than assume:

- That streaming inside block structures (code fences, tables) is unachievable on glamour's API, and that better-than-line-buffered streaming requires incremental parsing in glamour itself. *(0-3)*
- That reference-style links defined at document end are the only semantic divergence between chunked and whole-document render. *(0-3)*
- That small `--flow` values empirically broke syntax highlighting and H2 rendering in maintainer testing. *(0-3)*
- That glamour maintainers acknowledged defective CJK/emoji wrapping before v2. *(1-2)* — directly relevant to falsification claim 2.
- That iTerm2 transmits images as one base64 blob with no chunking or compression. *(1-2)*
- That iTerm2 images through tmux require the 3.5 multipart variant because of 256-byte / 1 MiB sequence caps. *(0-3)*
- That `OSC 1337 ; Capabilities` gives one round-trip capability discovery, also exposed out-of-band via `TERM_FEATURES`. *(1-2)*
- That kitty-protocol images **cannot** be rendered inside tmux at all. *(0-3)* — this is the one to actively disbelieve; `allow-passthrough` works, it just isn't managed.
- That Alacritty's PR demonstrates image-in-grid bookkeeping necessarily leaks into text selection. *(0-3)*
- That mode 2027 detection is a single DECRQM round-trip requiring no new sequence. *(1-2)*
- That VS16 forces width 2 while VS15 is width-neutral, per the spec. *(0-3)* — note the *confirmed* spec text does carry a VS16 width exception at L138-140; the fuller claim did not survive.
- That no single static width table can be correct across emulators, framed as a consequence of Unicode not specifying terminal behavior. *(1-2)* — the empirical version of this **is** confirmed (see claim 4); the framing failed, not the fact.
- That UAX #11 explicitly disclaims East_Asian_Width as a basis for terminal cell width. *(1-2)* — do not cite TR11 as saying this.

---

## Caveats

1. **Coverage is the dominant caveat.** Four of seven dimensions (D4–D7) have no verified findings, and D1's prior-art survey covers one library of the seven named. This document cannot support a go/no-go or a language choice as it stands. It can support "glamour is not sufficient" and "text measurement is a real engineering cost."
2. **Verification method limits.** Several verifiers exhausted their WebSearch budget (200/200) before running adversarial searches, and fell back to primary-source and source-code reading. For claims that are readings of a single specified document (mode 2027 spec text, TR11 text, glamour source) that is sound. For ecosystem claims it is not, and no ecosystem claim rests on it.
3. **Two tooling traps encountered, both of which produced a false negative before correction.** GitHub's rendered HTML pages did **not** surface the load-bearing comments on Alacritty PR #4763 or the metadata on glow PR #823 — both required `gh api`. And pkg.go.dev's LLM-generated package summary asserted glamour has "an incremental API for streaming," which the source contradicts. Any re-verification should use APIs and raw source, not page fetches.
4. **Version sensitivity is high and the half-life is short.** glamour v2.0.1 (2026-06-12), Alacritty 0.17.0 (2026-04-06), wcwidth 0.8.2 (2026-06-29), ucs-detect 2.3.4 (2026-06-12), tmux master 2026-07-20, TR11 rev 44 (2025-07-24). The `wcstwidth` correction-table API is **six weeks old** — it did not exist before 0.8.0 on 2026-06-05, and a planner should not assume its table coverage or API is stable. The published ucs-detect results table pins WezTerm at build 20240203; per-terminal numbers must be cited with versions.
5. **One blog example is demonstrably wrong and is being cited downstream.** The `wcstwidth` zombie-emoji example (`→ 8` under VTE) does not reproduce in 0.8.0, 0.8.1 or 0.8.2 — it returns 2. Verified by executing the wheels. Use the 4-person family sequence instead.
6. **Where inference was used, it is flagged inline.** The notable ones: "another thing in the works" (glow) has no stated referent and the glamour attribution is a non-maintainer's speculation; "Charm's maintainers" rests on one contributor's report of a team decision; "permanent" for Alacritty is a hardening of probabilistic language; the mode-2027 DECSTR-reset half rests on spec text alone, with only the RIS half confirmed in Ghostty's source.
7. **Source-strength is uneven by design.** D2 protocol properties and D3 Unicode properties rest on vendor specs and UCD data files — the strongest available class. The emulator support matrix rests on partial secondary signal and is explicitly incomplete. Sixel's palette limitation rests on implementation docs, not a spec.

---

## Open questions a planner must answer

1. **Does any of the unexamined prior art (mdcat, rich, textual/frogmouth, render-markdown.nvim, bat) already retain a layout tree or stream?** If one does, both the novelty argument and the ecosystem choice change. This is the cheapest remaining question and the one most likely to change the build decision.
2. **Is there an established technique for committing a stable prefix of a markdown document whose interpretation later input can invalidate** — stable-block commitment, speculative tail, or something else — and does any incremental parser (comrak, pulldown-cmark, tree-sitter-markdown) expose the resumability it would need? Charm rejected the one shipped attempt at a buffered window without naming an alternative, and the specific reasons that attempt was said to fail did not survive verification.
3. **What is the actual image-lifecycle contract a renderer can offer?** Given that tmux does not manage passthrough images, terminal-side scaling is unspecified, and Alacritty will never render them: is the target "images where possible, alt text otherwise," and does that degrade acceptably enough to be worth the protocol work at all?
4. **What does correct-enough width cost to implement per ecosystem?** Python has `wcstwidth` correction tables and `ucs-detect` today; whether Go, Rust or TypeScript has an equivalent — or whether the correction tables would have to be ported and maintained against a moving 40-terminal corpus — is unknown and is a first-order input to the language decision.
5. **Do mermaid and math have a path that preserves single-binary distribution?** Claim 6 is entirely untested, and it is the one falsification claim whose answer could unilaterally constrain the ecosystem choice.
