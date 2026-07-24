# Unicode Width Test Doc

This file exercises `crates/width`'s grapheme-width engine (`crates/width/src/lib.rs`, `engine.rs`, `correction.rs`) against real terminal bytes. Open it in stele running inside Ghostty and eyeball each section: CJK/Hangul/full-width should read as clean 2-cell blocks, emoji (including ZWJ sequences, flags, and skin tones) as single 2-cell glyphs, combining marks and zero-width classes as contributing nothing, and no cluster should ever visually eat more than 2 columns. The alignment tables are the sharpest test: any column-border drift is the bug class this engine exists to prevent.

## CJK

*Chinese (Simplified):*

你好，世界。中文测试文本。

*Chinese (Traditional):*

你好，世界。繁體中文測試文本。

*Japanese — kanji, hiragana, katakana:*

漢字テスト。ひらがなのテキストです。カタカナのテキストです。

*Korean Hangul — precomposed syllable blocks:*

한글 테스트 (precomposed, should be 2 cells per syllable)

*Korean Hangul — decomposed conjoining Jamo (choseong+jungseong+jongseong spelling the same word 'han-geul'); falls through to the engine's general max-per-codepoint rule, not a special case, so it may render differently than the precomposed form above depending on font composition:*

한글

*Full-width Latin letters, digits, and punctuation (should each be 2 cells, twice as wide as their ASCII counterparts below them):*

Ｈｅｌｌｏ，Ｗｏｒｌｄ！１２３４５６７８９０

Hello, World! 1234567890

*Half-width katakana (should be 1 cell each, narrower than the full-width katakana above):*

ｶﾀｶﾅ ﾃｽﾄ ﾊﾝｶｸ

## Emoji

*Simple emoji (each should measure 2 cells):*

😀 🎉 🚀

*Skin-tone (Fitzpatrick) modifiers — modifier attaches without adding width; all three should measure 2 cells, same as the plain thumbs-up:*

👍🏻 👍🏽 👍🏿

*ZWJ sequences — family, professions, couple-with-heart. Each whole sequence is one glyph and should measure 2 cells, not the sum of its parts:*

Family: 👨‍👩‍👧‍👦

Professions: 👩‍💻 🧑‍🚒

Couple (contains an embedded VS16 heart mid-sequence): 👩‍❤️‍👨

*Flags — regional-indicator pairs (GB12/GB13), 2 cells each, not 4:*

🇺🇸 🇯🇵 🇩🇪

*Subdivision (tag-sequence) flags — England, Scotland, Wales. These use invisible Unicode tag characters after the black-flag base; each should still measure 2 cells:*

🏴󠁧󠁢󠁥󠁮󠁧󠁿 🏴󠁧󠁢󠁳󠁣󠁴󠁿 🏴󠁧󠁢󠁷󠁬󠁳󠁿

*Keycap sequences (digit/symbol + VS16 + combining enclosing keycap), 2 cells each:*

1️⃣ #️⃣ *️⃣

*VS15 (text) vs VS16 (emoji) presentation — grounded in this engine's own test corpus (`crates/width/src/correction.rs`). Left of each `vs` should look visibly different in width from the right:*

- Watch, default (wide, 2 cells): ⌚  vs  Watch + VS15 (narrow, 1 cell): ⌚︎
- Heavy black heart, default (narrow): ❤  vs  + VS16 (promoted to wide, 2 cells): ❤️
- White heart suit (Emoji=NO), default (1 cell): ♡  vs  + VS16 (**no effect**, still 1 cell — the selector has nothing to select): ♡️

## Combining Marks

*Stacked diacritics on one base — should still measure 1 cell total, not 1 cell per mark:*

é̀̂̋

*Zalgo-style deep combining-mark stacking — extreme case of the same rule; however deep the stack, the base cluster is still 1 cell:*

Ź̸̧̰̻̀̃̍͘ḁ̶́̄́̍́ḷ̶̸̢̈́ǵ͐͗o̷̊͐

*Devanagari conjuncts (virama-joined consonant clusters):*

क्षत्रिय (kshatriya)  श्री (shri)

*Thai combining vowels and tone marks stacking above/below the base consonant:*

สวัสดี (sawatdee)  เกี๊ยว (kiaew)

*Hebrew niqqud (vowel points) stacked on consonants:*

שָׁלוֹם (shalom, fully pointed)

*Arabic tashkeel (fatha/kasra/sukun/shadda diacritics):*

بِسْمِ اللّهِ

## RTL

*Hebrew:*

שלום עולם

*Arabic:*

مرحبا بالعالم

*Bidi mixed with Latin — natural mixed-direction text, no explicit controls:*

The greeting שלום עולם means "hello world" and مرحبا بالعالم means the same in Arabic.

*Explicit bidi control characters — RLO/PDF and an LRI/PDI isolate. stele's painter (`crates/stele/src/painter.rs`, function `sanitize`) strips U+202A-202E and U+2066-2069 as a terminal-injection barricade, so both lines below should render as PLAIN, un-reversed left-to-right text with the control characters simply gone — if you see reversed or reordered text, the barricade failed:*

user‮txt.exe‬ (should read: usertxt.exe)

Price: ⁦100⁩ dollars (should read: Price: 100 dollars)

*By contrast, plain directional marks (RLM/LRM, not overrides or isolates) are NOT stripped and may still influence local shaping — this line should NOT look reversed, just possibly subtly re-shaped at the mark:*

mostly latin text‏ with an RLM mark inline‎ and an LRM mark

## Zero-Width

*Each pair below should measure as exactly 2 cells total (A + B); the character between them contributes 0:*

- ZWJ:  A‍B
- ZWNJ: A‌B
- ZWSP: A​B
- Soft hyphen: A­B
- Word joiner: A⁠B

*A standalone ZWJ with nothing to join (its own cluster) — should measure 0 cells, not 2 (this is a live-Ghostty-measured regression case, `zerowidth_2` in the corpus):*

[‍]

## Ambiguous-Width Characters

*Box drawing — under this engine's default (non-CJK-locale) policy these should be narrow (1 cell), keeping the box's straight edges aligned with plain-ASCII text around it:*

```
┌─────┬─────┐
│ one │ two │
├─────┼─────┤
│ 三  │ 四  │
└─────┴─────┘
```

*Greek:*

αβγδεζηθ ΑΒΓΔΕΖΗΘ

*Cyrillic:*

АБВГДЕЖЗ абвгдежз

*Arrows:*

→ ← ↑ ↓ ↔ ⇒ ⇔ ⇑ ⇓

*Math symbols:*

± × ÷ ≤ ≥ ≈ ≠ ∑ ∫ √ ∞ ∂

## Alignment Tests

Ruler (count columns against this to spot any drift below):

```
1         2         3         4         5         6
123456789012345678901234567890123456789012345678901234567890
```

### CJK-only column

*Every sample below is exactly 2 CJK characters = 4 cells; the `Sample` column border must land on the identical position in every row:*

```
|----------|--------|----------------|
```

| Language | Sample | Width (cells) |
|----------|--------|----------------|
| Chinese  | 中文   | 4              |
| Japanese | 漢字   | 4              |
| Korean   | 한글   | 4              |

### Emoji-only column

*Every emoji below is a single cluster measuring 2 cells despite wildly different codepoint counts (1 codepoint vs. a 7-codepoint ZWJ family); the `Emoji` column border must still land on the identical position in every row:*

```
|-------|------------------|
```

| Emoji | Description       |
|-------|--------------------|
| 😀    | simple             |
| 👨‍👩‍👧‍👦 | ZWJ family         |
| 🇯🇵    | flag pair          |
| 👍🏽    | skin-tone modifier |

### Combining-marks column

*Every row below is 1 base letter = 1 cell, regardless of how many combining marks are stacked on it (0, 1, or 4):*

```
|-------|-------|
```

| Marks | Count |
|-------|-------|
| e     | 0     |
| é    | 1     |
| é̀̂̋ | 4     |

### Mixed column (CJK + emoji + combining marks together)

*The most important table in this document. One column, three totally different cluster classes, three different expected widths (2, 4, and 10 cells) — the column border after `Content` must still land on the identical position in every single row:*

```
|--------------|-------|
```

| Content        | Cells |
|----------------|-------|
| 中             | 2     |
| 😀             | 2     |
| é̀̂̋      | 1     |
| 한글           | 4     |
| 👨‍👩‍👧‍👦 | 2     |
| Ｈｅｌｌｏ     | 10    |

## Long Unbreakable Runs of Wide Characters

*A long run of CJK characters with no spaces — the layout engine must break at grapheme-cluster boundaries when this wraps, never mid-cluster (which would be visually impossible anyway for these single-codepoint clusters, but sets the baseline for the ZWJ case below):*

测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试测试

*A long run of multi-codepoint ZWJ emoji sequences with no separating spaces — wrapping must occur between whole sequences, never splitting a sequence's codepoints across the wrap (which would break the emoji into orphaned components):*

👨‍👩‍👧‍👦👩‍💻🧑‍🚒👩‍❤️‍👨👨‍👩‍👧‍👦👩‍💻🧑‍🚒👩‍❤️‍👨👨‍👩‍👧‍👦👩‍💻🧑‍🚒👩‍❤️‍👨👨‍👩‍👧‍👦👩‍💻🧑‍🚒👩‍❤️‍👨👨‍👩‍👧‍👦👩‍💻🧑‍🚒👩‍❤️‍👨👨‍👩‍👧‍👦👩‍💻🧑‍🚒👩‍❤️‍👨

## CJK Paragraph (Wraps Several Times)

*Natural-language Chinese, no spaces between words (as is normal for the script) — long enough that it must wrap multiple times at any reasonable terminal width; every wrapped line should still start and end on a clean 2-cell character boundary:*

这是一段很长的中文段落，用来测试终端在换行时是否能够正确处理宽字符的显示宽度。每一个汉字在等宽终端中都应当占据两个字符的宽度，而不是一个。当这段文字达到终端的最大列数时，渲染引擎必须在字符边界处正确换行，不能把一个字符从中间切断，也不能因为宽度计算错误而导致文字重叠或错位。这段话会被反复延长，直到确保它在标准的八十列终端里至少能够换行三到五次，从而充分验证长文本的自动换行逻辑与宽字符宽度测量之间协同工作的正确性。如果宽度引擎出现任何偏差，这里的换行位置就会立刻显得不整齐，非常容易被人眼发现。

## Wide Character at the Exact Wrap Boundary

*Assumes an 80-column terminal. The line below is 79 ASCII dots (79 cells) immediately followed by one CJK character (needs 2 cells) and then `X`. At exactly 80 columns, only 1 cell remains when the CJK character is reached — it must wrap whole to the next line (appearing first, followed by `X`), never get split or rendered half-off-screen. If your terminal isn't exactly 80 columns, resize it to 80 to trigger this exact case; at any other width the same rule — never split a wide cluster across the boundary — still applies wherever the wrap actually falls:*

...............................................................................中X

