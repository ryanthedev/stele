# Stele Manual Test Suite: CommonMark + GFM

This file is for **manual visual testing** in a real terminal. Scroll through slowly. Each section
below is self-contained and carries an *italic note* right above anything subtle, telling you what
correct rendering should look like. If what you see doesn't match the note, that's a bug — write
down the section name and keep going.

---

## Headings: h1-h6, empty headings, inline markup

*expect: six distinct visual weights/sizes from H1 (largest) down to H6 (smallest), each clearly
different from body text.*

# H1: The Weather Balloon Incident

## H2: A Ship's Log, Rediscovered

### H3: Notes from the Radio Room

#### H4: Appendix C — Tide Tables

##### H5: Footnote to Appendix C

###### H6: Errata (Printed Upside Down)

Below are two empty headings — a level-1 and a level-3, each with nothing but the marker and a
trailing space (`# ` and `### `).

*expect: an empty heading still occupies heading-sized vertical space and heading styling; it should
not simply vanish or collapse to zero height.*

# 

### 

Headings can carry inline markup. This one mixes **bold**, *italic*, `inline code`, and a
[link](https://ghostty.org "Ghostty"):

## The **Captain's** Orders Were `clear`, if [poorly timed](https://ghostty.org)

*expect: the bold/italic/code/link styling survives inside the heading, layered on top of the
heading's own size and weight rather than overriding it.*

---

## Emphasis: nesting, intraword underscores, escaped markers

Plain forms first, for a baseline: *italic text*, **bold text**, and ***bold italic text***.

*expect: italic is slanted or colored distinctly, bold is heavier weight, and the combined form
shows both traits simultaneously — not just one of them.*

Nesting bold inside italic and italic inside bold, deliberately using mixed delimiters so the
parser can't cheat by just counting matching runs:

- *This whole clause is italic, but **this middle part is also bold**, and then it goes back to
  plain italic.*
- **This whole clause is bold, but *this middle part is also italic*, and then it goes back to
  plain bold.**
- ***Everything here is both bold and italic at once, no plain segments at all.***

Intraword underscores should **not** trigger emphasis — this rule exists specifically so that
identifiers like `some_variable_name` and `my_other_thing` don't get mangled:

Snake case survives intact: some_variable_name_here and another_one_like_it, and even
file_name_v2_final.txt should stay plain, unformatted text.

*expect: none of the underscores above turned into italics; the words read as plain text with
underscores intact.*

Asterisks in the middle of a word *do* trigger emphasis by the CommonMark rule (unlike
underscores), so this next line is a real test of that asymmetry:

un*frigging*believable should render with "frigging" in italics, while un_frigging_believable
(same word, underscores) should not.

Escaped emphasis markers — these should render as literal punctuation, not as formatting:

Use \*asterisks\* and \_underscores\_ literally when you want to write \*\*this exact text\*\*
without triggering bold, and a literal backslash itself: \\ then more text.

*expect: every asterisk and underscore above is a plain visible character; nothing is italicized
or bolded, and the double backslash shows as a single literal backslash followed by "then more
text."*

---

## Lists: tight, loose, ordered variants, deep nesting

A **tight** list (no blank lines between items — items render compactly, no extra paragraph
spacing):

- First, gather the equipment.
- Second, check the weather.
- Third, don't forget the thermos.

A **loose** list (blank line between each item — CommonMark wraps each item in its own paragraph,
so expect visibly more vertical space between items than the tight list above):

- First, gather the equipment.

- Second, check the weather. This item has enough text that it reads like its own paragraph, which
  is exactly the point of a loose list.

- Third, don't forget the thermos.

*expect: clearly more line spacing between items in the loose list than in the tight list directly
above it.*

Ordered list starting at a number other than 1:

5. Fifth item (the list should start counting at 5, not renumber to 1).
6. Sixth item.
7. Seventh item.

Ordered list using the `)` delimiter instead of `.`:

1) First item with parenthesis delimiter.
2) Second item with parenthesis delimiter.
3) Third item with parenthesis delimiter.

*expect: both ordered lists above show real sequential numbers (5, 6, 7 and 1, 2, 3) with their
respective delimiter style intact — no renumbering to 1, 2, 3 in the first list.*

Deeply nested unordered list, five levels deep:

- Level 1: the outer expedition
  - Level 2: the base camp
    - Level 3: the supply cache
      - Level 4: the emergency kit
        - Level 5: the single match

*expect: five distinct indentation steps, each bullet clearly further right than its parent, and
each bullet's text aligned under itself (not drifting left/right relative to its own marker).*

Mixed ordered/unordered nesting — an ordered list containing an unordered sublist containing
another ordered sub-sublist:

1. Plan the route.
   - Check the map.
   - Check the compass.
     1. Confirm magnetic declination.
     2. Confirm the compass isn't near a magnet.
2. Pack the bags.
   - Food for three days.
   - A spare pair of socks.
3. Leave a note for whoever finds the cabin empty.

*expect: the marker style changes appropriately at each nesting transition (numbers, then bullets,
then numbers again), each level indented relative to its parent.*

---

## Task lists

Unchecked and checked items, plus nesting:

- [ ] Buy a new tent (unchecked)
- [x] Buy a new tent (checked — this line is a duplicate on purpose, to compare states side by side)
- [x] Renew the fishing license
- [ ] Repair the canoe
  - [x] Patch the left side
  - [ ] Patch the right side
  - [ ] Repaint the hull

*expect: checked items show a filled/checked box glyph, unchecked show an empty box glyph, and both
are visually distinct from a plain bulleted list item.*

A task list inside a blockquote:

> Trip prep, quoted from last year's notebook:
>
> - [x] Confirm the campsite reservation
> - [ ] Test the water filter
> - [ ] Charge the headlamps

*expect: the checkbox rendering survives inside the blockquote, with the quote's left border/indent
still visible alongside the checkboxes.*

---

## Tables

Alignment: left, center, right, and default (none):

| Left        | Center      | Right       | Default  |
| :---------- | :---------: | ----------: | -------- |
| short       | short       | short       | short    |
| a longer bit of text | a longer bit of text | a longer bit of text | a longer bit of text |
| x           | y           | z           | w        |

*expect: left-aligned text hugs the left edge of its column, centered text is visually centered,
right-aligned text hugs the right edge, and the default column matches whatever the renderer's
default is (typically left) — the four columns should look distinguishably different from each
other.*

A ragged table — rows with fewer cells than the header declares (missing trailing cells should be
treated as empty):

| Name    | Role       | Notes |
| ------- | ---------- | ----- |
| Alvarez | Navigator  | Reliable in fog |
| Reyes   | Cook       |
| Tanaka  |

*expect: the missing cells render as blank, and the table's column widths/borders stay consistent
across all rows rather than collapsing or misaligning.*

A table with a very wide cell that must wrap, next to normal cells:

| Field       | Description |
| ----------- | ----------- |
| summary     | This description is intentionally long enough that it should not fit on a single terminal line at any reasonable width, forcing the renderer to wrap it within the cell while keeping the table's borders and the neighboring column intact. |
| id          | short |

*expect: the long cell wraps onto multiple lines inside its own column, table borders stay straight
top-to-bottom, and the `id` row's short cell isn't stretched to match.*

A table with inline code and links inside cells:

| Command | Docs |
| ------- | ---- |
| `stele render file.md` | See the [Stele repo](https://github.com) for details |
| `ghostty +show-config` | Part of the [Ghostty](https://ghostty.org) docs |

*expect: inline code spans keep their code styling inside table cells, and links keep their link
styling (and stay clickable/underlined if that's how this renderer shows links elsewhere).*

A one-column table:

| Single Column |
| ------------- |
| Just one row here |
| And another one here |

*expect: renders as a proper table with visible borders even though there's only one column — not
silently downgraded to a plain list.*

A table wider than any reasonable terminal (ten columns) — this is a stress case:

| C1 | C2 | C3 | C4 | C5 | C6 | C7 | C8 | C9 | C10 |
| -- | -- | -- | -- | -- | -- | -- | -- | -- | --- |
| a1 | b1 | c1 | d1 | e1 | f1 | g1 | h1 | i1 | j1 |
| a2 | b2 | c2 | d2 | e2 | f2 | g2 | h2 | i2 | j2 |

*expect: the renderer picks a reasonable strategy for overflow — horizontal scroll, column
truncation, or reflow — but doesn't crash, corrupt other content below it, or silently drop columns
without any visual indication.*

---

## Blockquotes

Nested blockquotes, four levels deep:

> Level 1: an old sailor's warning.
>
> > Level 2: what the first mate added in the margin.
> >
> > > Level 3: a second annotation, in different ink.
> > >
> > > > Level 4: barely legible, added much later.

*expect: four visually distinct indentation/border levels, each one nested further right than its
parent, all the way down.*

A blockquote containing a list, which itself contains a code block:

> Deployment checklist, quoted from the runbook:
>
> - Confirm the branch is green.
> - Run the build:
>
>   ```bash
>   make build && make test
>   ```
>
> - Only then tag the release.

*expect: the code block keeps its monospace/code-block styling while still nested inside both the
list item and the blockquote — border or indent from the blockquote should remain visible beside
the code block.*

Lazy continuation — CommonMark allows a blockquote's paragraph to continue on a following line even
if that line omits the `>` marker, as long as it's not interrupted by something else:

> This paragraph starts with a proper blockquote marker on the first line,
but this second line has no marker at all and should still be swallowed
into the same quoted paragraph, by the lazy-continuation rule.

*expect: both lines above render as a single quoted paragraph with consistent blockquote styling —
the second line should not look like plain unquoted text.*

---

## GitHub alerts

> [!NOTE]
> A note calls out information the reader should be aware of, even when skimming.

> [!TIP]
> A tip offers helpful advice for doing things better or more easily.

> [!IMPORTANT]
> Important information the user needs to know to achieve their goal.

> [!WARNING]
> Urgent info that needs immediate attention to avoid problems.

> [!CAUTION]
> Advises about risks or negative outcomes of certain actions.

*expect: each of the five alert types above shows its own distinct icon and color/label (Note, Tip,
Important, Warning, Caution) — they should not all look identical to each other or to a plain
blockquote.*

An alert with a multi-paragraph body:

> [!WARNING]
> The first paragraph explains that the migration script is destructive and cannot be undone once
> it starts writing to disk.
>
> This second paragraph, still inside the same alert, adds that you should take a full backup
> before running it, and that support tickets citing "I didn't back up" will be closed as
> won't-fix.

*expect: both paragraphs stay inside the same warning-styled block, with normal paragraph spacing
between them but no break in the alert's border/background.*

An alert containing a list:

> [!IMPORTANT]
> Before filing a bug against this renderer, please confirm:
>
> - You're on the latest tagged release, not a stale build.
> - The input file is valid CommonMark/GFM (check it elsewhere first).
> - You can reproduce it with a minimal example, not just "sometimes it happens."

*expect: the list renders normally (bullets, indentation) while still contained inside the
important-styled alert block.*

---

## Links

Inline link: [the Stele repository](https://github.com/example/stele).

Inline link with a title attribute: [Ghostty](https://ghostty.org "A fast, native terminal emulator").

*expect: a title attribute shouldn't print as visible text next to the link — it's metadata, often
surfaced only as a tooltip or not at all in a terminal.*

Reference-style link: this is [the reference link][repo-ref], defined elsewhere in the document.

Collapsed reference link: [Ghostty][] (uses its own link text as the implicit reference label).

Shortcut reference link: [Ghostty] (no brackets at all after the label; relies purely on a matching
definition).

[repo-ref]: https://github.com/example/stele
[ghostty]: https://ghostty.org

Autolink using angle brackets: <https://ghostty.org/docs>.

Bare URL autolink (GFM extension — no angle brackets, no markdown syntax at all, just a naked URL
sitting in prose): check out https://ghostty.org/docs for more, or email support@example.com.

*expect: both the angle-bracket autolink and the bare URL above render as clickable-looking links,
not as plain text — this is a GFM extension, not baseline CommonMark, so it's worth double-checking.*

A link whose text contains nested emphasis: [a **bold** and *italic* and `coded` link
label](https://ghostty.org).

*expect: bold/italic/code styling survives inside the link text, layered under the link's own
underline/color styling.*

---

## Strikethrough

Plain strikethrough: ~~this text has been struck through~~.

Strikethrough nested with bold, in both orders:

- ~~This entire clause is struck through, and **this part is also bold**, then back to plain
  strikethrough.~~
- **This entire clause is bold, and ~~this part is also struck through~~, then back to plain
  bold.**

*expect: in both bullets, the strikethrough line runs through the whole clause, and the bold
portion is additionally heavier-weight within it — the two effects should stack, not override each
other.*

---

## Footnotes

Here's a claim that needs a source.[^survey] And here's the same source cited again a moment
later, to confirm both references point at one shared definition.[^survey]

This next claim uses a footnote whose *definition appears earlier in the document* than its
reference — order of definition shouldn't matter for correctness.[^early-defined]

[^early-defined]: This footnote was defined up here, before the paragraph that references it below
    ever calls `[^early-defined]`. Numbering should still follow reference order in the rendered
    text, not definition order in the source.

And a footnote whose body has multiple paragraphs.[^multipara]

[^survey]: Internal usage survey, conducted across the beta cohort, n=214 respondents.

[^multipara]: This is the first paragraph of a multi-paragraph footnote.

    This is the second paragraph of the same footnote, indented so it's recognized as a
    continuation rather than a new top-level block.

*expect: footnote markers render as small superscript-style numbers/labels in the body text, the
two references to `[^survey]` share one number, and clicking or navigating to a footnote jumps to a
definitions area (often at the document's end) where both paragraphs of `[^multipara]` appear
together under one entry.*

---

## Thematic breaks

Three different source syntaxes, all of which must render as the same horizontal rule:

Hyphens:

---

Asterisks:

***

Underscores:

___

*expect: all three horizontal rules above look identical to each other, and each clearly separates
the text above it from the text below.*

---

## Hard breaks, soft breaks

Hard break using two trailing spaces (invisible in most editors — trust the markup): this line ends with two real trailing spaces before the newline, so it forces a break.  
This line should start directly below, with no blank paragraph gap.

Hard break using a trailing backslash:\
this line should also start on a new line directly below, no blank gap.

Soft break — just a single newline in the source, no trailing spaces or backslash:
this line is a plain continuation and per CommonMark should be joined onto the same
visual line as the text above it, typically shown with just a single space between them.

*expect: the two hard-break examples each force a genuine line break with no paragraph spacing,
while the soft-break example collapses onto one flowing line (or wraps only due to terminal width,
not due to the source line break).*

---

## HTML blocks and inline HTML

This renderer targets a terminal, so raw HTML should not be interpreted — it should show up as
literal text, tags and all.

An HTML block:

<div class="callout">
  <p>This entire div, including its tags, should appear as literal visible text.</p>
</div>

Inline HTML mixed into a sentence: here is some text with an <strong>inline bold tag</strong> and a
<a href="https://ghostty.org">literal anchor tag</a> sitting right in the middle of it, plus a
self-closing tag like <br/> for good measure.

*expect: every angle-bracketed tag above (`<div>`, `<p>`, `<strong>`, `<a href=...>`, `<br/>`, and
their closing tags) is visible as plain literal text in the output — none of it should actually
render as bold, as a link, or as a line break. If tags vanish or actually apply formatting, that's
the bug this section is designed to catch.*

---

## Character entities and numeric references

Named entities: A &amp; B, 5 &lt; 10, 10 &gt; 5, a "quoted" word via &quot;quoted&quot;, the
&copy; symbol, an em dash via &mdash; between clauses, and a non-breaking space via &nbsp;between
these two words.

Numeric character references, decimal and hex: &#65;&#66;&#67; (should read as "ABC"), and an emoji
via a hex reference: &#x1F600;.

*expect: every entity and numeric reference above resolves to its actual character (ampersand,
less-than, greater-than, quote marks, copyright symbol, em dash, non-breaking space, the letters
"ABC", and a grinning-face emoji) — none of them should print as the raw `&name;` or `&#code;`
escape sequence.*

---

*End of test document. If every section above matched its note, the renderer is in good shape.*
