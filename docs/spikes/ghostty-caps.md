# Spike A — Ghostty capabilities

**What this is.** Nine live verdicts, each obtained by actually querying a real Ghostty session over its own PTY — not read from documentation or inferred from source. Every raw reply below is the literal bytes Ghostty sent back, captured by `crates/probe`'s `spike_a` binary.

**Environment measured against:**

| | |
|---|---|
| Ghostty version | 1.3.1 (`TERM_PROGRAM_VERSION`, self-reported) |
| `TERM_PROGRAM` | `ghostty` |
| `TERM` | `xterm-ghostty` |
| Config | `~/.config/ghostty/config` — `font-family = "BerkeleyMono Nerd Font"`, `theme = TokyoNight Night`; no `grapheme-width-method` or other unicode/mode override present |
| Platform | macOS (Darwin 25.2.0, arm64) |
| Date measured | 2026-07-21/22 |

Every capability item's result below should be re-verified if the pinned Ghostty version, or a viewer user's config, changes — see the mode-2027 finding, which is precisely a case of measured behavior disagreeing with a documentation-derived assumption.

## Launch pattern — verified, and it is not the naive one

The obvious pattern, "run the probe binary inside a Ghostty window as its child," has two possible implementations. Only one works in this environment:

| Attempt | Result |
|---|---|
| `ghostty -e <bin>` (direct exec of the `ghostty` CLI binary) | **Hangs indefinitely.** Confirmed three ways: foreground with a 15s timeout, backgrounded with `nohup` (process sat alive, 0% CPU, never executed the child command, for 20+s), and via `launchctl asuser $(id -u) ghostty -e <bin>` (same hang). The process starts but never gets past whatever handshake it performs before exec'ing the child — plausibly Ghostty's macOS single-instance IPC not completing when invoked as a bare Mach-O rather than through Launch Services. |
| `open -na /Applications/Ghostty.app --args -e <bin> <args...>` | **Works.** `open` returns immediately; a real Ghostty window opens, runs `<bin>` as its foreground process with real stdio, and the process's own file writes land on disk. Verified first with a trivial `/bin/sh -c 'echo ... > file'` child, then with the actual `spike_a` binary end to end. |

`crates/probe::Launcher` is built on the second form. This is recorded because the dispatch brief's own suggested pattern (`ghostty -e <path-to-probe-binary>`) does not work as literally stated, and a future phase re-deriving this from scratch would otherwise burn the same debugging cycle.

## Commands run

```
cargo build --release -p probe          # produces target/release/spike_a
open -na /Applications/Ghostty.app --args -e target/release/spike_a --out <path>
# spike_a: GhosttyPty::from_current_process() validates stdio, then runs all
# nine checks sequentially over its own stdin/stdout, writing JSON to --out.
```

Full raw JSON output is reproduced inline per item below (every `raw_response_hex` value is the literal reply bytes; nothing here is paraphrased from memory).

---

## The nine capability items

### 1. kitty `a=q` query

**Verdict: supported.**

Sent `ESC _G i=31,s=1,v=1,a=q,t=d,f=24;AAAA ESC \` immediately followed by DA1 (`ESC[c`), per the research doc's documented synchronization technique.

```
raw reply: "\u{1b}_Gi=31;OK\u{1b}\\\u{1b}[?62;22;52c"
```

The kitty APC reply (`_Gi=31;OK`) arrived before the DA1 reply, both in the same read. Unambiguous positive.

### 2. Chunked direct transmission

**Verdict: accepted.**

A 64×64 RGBA image (16384 raw bytes) was base64-encoded (~21.8k chars) and split into 6 chunks of ≤4096 base64 chars each, sent as `a=t,f=32,s=64,v=64,i=32,m=1` (first chunk) → `m=1` (middle chunks) → `m=0` (final chunk), per spec.

```
raw reply: "\u{1b}_Gi=32;OK\u{1b}\\"
```

A single `OK` after the last chunk — real multi-chunk transmission works end to end, not just a single-chunk pass-through.

### 3. Virtual placement `U=1`

**Verdict: accepted at the sequence level — visual placement not independently confirmed.**

Sent `a=T,U=1,i=33,f=32,s=2,v=2;<payload>` (transmit-and-display, virtual placement flag set).

```
raw reply: "\u{1b}_Gi=33;OK\u{1b}\\"
```

**Stated measurement limit:** this confirms Ghostty *accepted* the `U=1` transmission without an `ERROR` reply. It does **not** confirm that writing a Unicode-placeholder codepoint (U+10EEEE + diacritics) elsewhere on the grid actually renders the image at that cell — kitty's protocol offers no screen-content query for graphics, and confirming the visual result would require a pixel-level screenshot comparison, which is out of reach for a text-protocol PTY probe and out of scope for this harness. P6 should budget one manual visual pass before treating virtual placement as load-bearing.

### 4. Deletion `a=d,d=i`

**Verdict: no response within 800ms — ambiguous, not a confirmed failure.**

Sent `a=d,d=i,i=33` to delete the image placed in item 3. No reply arrived (contrast with items 1–3, which all got an explicit `OK`).

**What this means, stated carefully:** we did not observe an acknowledgment either way. This harness cannot distinguish "delete succeeded silently" from "delete was ignored" from this evidence alone — no counter-query exists in the base protocol to ask "does image 33 still exist." **Consequence for P6:** DW-6.1's plan to assert "create/delete balance" from an emission log cannot rely on delete-side terminal acks (there may not be any); balance bookkeeping needs to be driven from the client's own send-log, not confirmed terminal-side.

### 5. Mode 2026 (synchronized output)

**Verdict: recognized, reset (off) by default.**

`CSI ? 2026 $ p` → `CSI ? 2026 ; 2 $ y` (DECRQM Ps=2 = reset).

```
raw reply: "\u{1b}[?2026;2$y"
```

Ps=2, not Ps=0 — the mode **is** recognized (an unsupported mode reports Ps=0, "not recognized"), just off until a client explicitly begins a synchronized-update block. This is the expected, unsurprising shape.

### 6. Mode 2027 (grapheme clustering) default state

**Verdict: recognized, set (on) by default — this disagrees with the grounding research.**

`CSI ? 2027 $ p` → `CSI ? 2027 ; 1 $ y` (DECRQM Ps=1 = **set**).

```
raw reply: "\u{1b}[?2027;1$y"
```

**This was cross-checked independently of `crates/probe`'s own parsing**, to rule out a bug in the Rust code before reporting it: a standalone Python script, using raw `termios`/`select` with no shared code path, was launched the same way (`open -na Ghostty.app --args -e python3 <script>`) in a *separate* fresh Ghostty window and sent the identical two DECRQM queries. Result: byte-identical —

```
mode 2026: b'\x1b[?2026;2$y'
mode 2027: b'\x1b[?2027;1$y'
```

`~/.config/ghostty/config` on the reference machine carries no grapheme/unicode-width override (only `font-family`, `font-size`, `theme`), so this is not a local config artifact as far as this file can show.

**This contradicts part 1 research's finding** (`2026-07-20-tui-markdown-renderer.md`, D3), which read Ghostty's `src/terminal/modes.zig:297` and reported `grapheme_cluster` (mode 2027) carries no `.default = true`, unlike sibling modes. That was a source-reading claim about *some* commit of Ghostty; this is a live measurement against the shipped 1.3.1 binary. They disagree, and the live measurement is authoritative for this project's purposes — possible explanations (not distinguished by this spike): the shipped default changed between the commit read and the 1.3.1 release, or the static per-mode default table doesn't tell the whole story and something in Ghostty's startup path enables it dynamically. Either way, the measured fact stands.

**Consequence.** The plan's own scope for P3 states: *"no mode 2027 negotiation — the viewer renders against Ghostty's measured default behavior"* and the Decision Log records *"Measured default Ghostty behavior is the contract."* Read literally, both already anticipate exactly this outcome — the *contract* was always "whatever Ghostty actually defaults to," not "off." What changes is the concrete value P3 must build against: **P3's live-Ghostty width corpus (DW-3.1) is measuring cell widths with mode 2027 ON**, not off, on this reference install. P3 must pin the same Ghostty version and config alongside its corpus (the plan already requires pinning the Ghostty version; this finding is why that requirement matters in practice), and should re-run this specific DECRQM check as a canary before trusting a stale corpus if Ghostty is ever upgraded.

### 7. Cell-geometry sources

Four sources queried; the plan asks "which answer, and agreement."

| Source | Verdict | Detail |
|---|---|---|
| `OSC 1337 ; ReportCellSize` | no response within 500ms | iTerm2-specific mechanism; Ghostty does not answer it. |
| `CSI 14 t` (text-area size, px) | answered | `\x1b[4;2016;3720t` → height=2016px, width=3720px |
| `CSI 16 t` (single cell size, px) | answered | `\x1b[6;48;24t` → height=48px, width=24px |
| `TIOCGWINSZ` (local ioctl, not a terminal round-trip) | answered | rows=42 cols=155 xpixel=3740 ypixel=2048 |

**Agreement check:** `CSI 14t`'s text-area size ÷ the window's rows/cols (155×42, from `TIOCGWINSZ`) gives 3720/155 = **24.0px** and 2016/42 = **48.0px** — an *exact* match to `CSI 16t`'s direct per-cell answer (24×48px). `TIOCGWINSZ`'s own pixel fields (3740×2048) are ~0.5–1.6% larger than `CSI 14t`'s text-area size (3720×2016) — consistent with `TIOCGWINSZ` measuring the whole window including padding/chrome, while `CSI 14t`/`16t` measure the character grid itself.

**Consequence.** OSC 1337 is a dead end on Ghostty — do not implement it. `CSI 16t` is the direct, agreeing source and should be P6's primary cell-geometry query; `CSI 14t` ÷ (rows,cols) is a working cross-check but adds no information `16t` doesn't already give more directly. `TIOCGWINSZ` pixel fields are usable as a fast local fallback but carry a small systematic bias from window chrome and should not be treated as pixel-exact.

### 8. OSC 10/11 background/foreground query

**Verdict: answered.**

```
OSC 10 (fg): "\u{1b}]10;rgb:c0c0/caca/f5f5\u{1b}\\"
OSC 11 (bg): "\u{1b}]11;rgb:1a1a/1b1b/2626\u{1b}\\"
```

fg ≈ `#c0caf5` (light lavender), bg ≈ `#1a1b26` (near-black, slightly blue) — consistent with the configured Tokyo Night dark theme. Both queries answered promptly and unambiguously.

### 9. kitty emission while crossterm holds raw mode (coexistence)

**Verdict: coexists.**

`Probe::open` holds crossterm raw mode for the process's entire lifetime (all nine checks ran under it), but that alone doesn't exercise the real interference hazard — two independent readers on the same fd. This check additionally ran `crossterm::event::poll(1ms)` in a tight loop on a background thread, concurrently with sending a fresh kitty `a=q` query (id=41) and reading its reply on the main thread via the probe's own raw fd read.

```
raw reply: "\u{1b}_Gi=41;OK\u{1b}\\"
```

The reply arrived intact and correctly tagged (`i=41`) despite the concurrent `crossterm::event::poll` calls — no evidence of one reader stealing bytes meant for the other.

---

## Decision

Each line: `capability → verdict → consequence`.

1. **kitty `a=q` query → supported → P6 can use the `a=q` + DA1 synchronization technique for runtime graphics-protocol detection as documented in the research; no fallback needed.**
2. **Chunked direct transmission → accepted (6-chunk, 16384-byte payload) → P6 can implement real multi-chunk base64 direct transmission (`t=d`, `m=1`/`m=0`) as specified; no fallback needed.**
3. **Virtual placement `U=1` → accepted at the sequence level, visual result unverified by this harness → P6 may proceed with `U=1` + Unicode-placeholder placement as primary, but must add one manual visual pass before shipping it as load-bearing; if that visual pass fails, fall back to direct placement + repaint-behind per the plan's own assumption-table fallback.**
4. **Deletion `a=d,d=i` → accepted-but-silent (no ack observed) → P6's DW-6.1 create/delete-balance assertion must be driven from the client's own send-log, not a terminal-side ack, since none was observed for delete.**
5. **Mode 2026 → recognized, reset by default → approach C (retained layout, immediate paint, synchronized-update frames, no differ) remains viable; no escalation to the approach-A owned-differ fallback is triggered by this measurement.**
6. **Mode 2027 default state → recognized, SET by default (disagrees with the source-reading in the grounding research) → P3 must build its width correction corpus against mode-2027-ON behavior on this Ghostty version/config, must pin the Ghostty version and config alongside the corpus (as the plan already requires), and should re-run this DECRQM check as a canary on any Ghostty upgrade.**
7. **Cell-geometry sources → `CSI 16t` and `CSI 14t`÷(rows,cols) agree exactly (24×48px/cell); `TIOCGWINSZ` pixel fields are ~0.5–1.6% high (window chrome); `OSC 1337 ReportCellSize` unanswered → P6 should query `CSI 16t` as the primary cell-geometry source, treat `TIOCGWINSZ` pixel fields as an approximate fallback only, and must not implement OSC 1337 for Ghostty.**
   *Implemented 2026-07-25 (`ccd56a6`), as recommended: `terminal::query_cell_px` tries `CSI 16t` with a 250 ms deadline, then `TIOCGWINSZ`÷grid, then a labelled fallback; OSC 1337 is not implemented. The parser is pinned to the exact reply this spike recorded (`\x1b[6;48;24t`), and the round trip is proven against a scripted pty — **not** yet against live Ghostty, which is the one thing this doc measured and the test suite cannot.*
8. **OSC 10/11 background/foreground query → answered promptly, both fg and bg → P7's OSC-10/11-based theme selection (Decision Log) is viable as specified; no fallback needed.**
9. **kitty emission while crossterm holds raw mode → coexists, verified under concurrent `crossterm::event::poll` → P5's crossterm-adoption approach note is confirmed; the rustix + owned-input-parsing fallback is not required on this basis.**
