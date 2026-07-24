# Media

Manual test doc for image rendering via the kitty graphics protocol. Covers
the inline-baseline-vs-own-rows layout boundary, scaling, containers, alt
text, and every path stele's sizer is supposed to reject.

All image paths below are relative to this file (`img/...`).

## One-row image inline mid-sentence

*expect: rides the text baseline inline, like a glyph — this is the new
behavior for any image whose natural size is one row tall.*

Here is a small icon ![a small red square](img/small.png) sitting right in
the middle of a sentence, with ordinary text continuing on both sides of it
as if it were just another character on the line.

And another one, twice in one line: ![red square](img/small.png) then some
words, then ![red square again](img/small.png) near the end.

## A tall image in a paragraph

*expect: too tall for one row — claims its own rows and pushes surrounding
text to separate lines, not inline.*

A tall narrow banner:

![a tall green banner](img/tall.png)

Text resumes here as its own paragraph after the image's reserved rows.

## A wide image that must scale to content width

*expect: scaled down to fit the content column; never overflows or gets
clipped at the terminal edge.*

![a very wide yellow strip, wider than any reasonable terminal column](img/wide.png)

## An image bigger than any terminal

*expect: still scales down to fit — this one is 6000x6000px, larger than
any real terminal window, and must not crash the sizer or blow past the
layout engine's reserved-box cap.*

![a huge purple square, larger than any terminal viewport](img/huge.png)

## Multiple images in one paragraph

*expect: three medium images, each multi-row, laid out one after another
without corrupting each other's rows.*

![blue square one](img/medium.png) ![blue square two](img/medium.png) ![blue square three](img/medium.png)

## Images inside containers

### Inside a list item

*expect: image renders (or falls back to alt text) inside the list item's
indentation, without breaking the list marker/indent.*

- First item, plain text
- Second item with an inline icon ![list icon](img/small.png) mid-line
- Third item

### Inside a nested list

*expect: same as above, one level deeper — indentation should still track
correctly around the reserved box.*

- Outer item
  - Nested item with an image ![nested icon](img/small.png) here
  - Another nested item
- Outer item two

### Inside a blockquote

*expect: image renders inside the quote's gutter/indent, same as any other
quoted content.*

> A blockquote with an inline icon ![quoted icon](img/small.png) in the
> middle of quoted prose.

### Inside a table cell

*expect: alt text ONLY, never an image — media in table cells is disabled
by design, regardless of the image's validity or size.*

| Name | Preview |
|------|---------|
| Small | ![alt text for table preview](img/small.png) |
| Medium | ![another table preview](img/medium.png) |

## An image as an entire paragraph alone

*expect: own paragraph, own reserved rows, no surrounding text on the same
line.*

![a solo medium blue square, alone in its own paragraph](img/medium.png)

## Alt text variations

### No alt text

*expect: empty alt text is valid — image (or fallback) renders with no
label.*

![](img/small.png)

### Very long alt text

*expect: long alt text must not break layout when used as a fallback label
(e.g. if the sizer/decoder ever fails); wraps like ordinary text.*

![this is a deliberately very long piece of alt text meant to exercise line wrapping and layout robustness in case the image cannot be sized or painted and the viewer must fall back to rendering this description as plain text instead of a picture, which should still read cleanly across however many terminal columns are available without corrupting the surrounding paragraph](img/medium.png)

### Alt text containing markup

*expect: the `*`, `_`, and `` ` `` characters inside alt text are literal —
not parsed as emphasis or code, since alt text is a plain string, not nested
inline content.*

![alt text with *asterisks*, _underscores_, `backticks`, and [brackets]](img/small.png)

## Rejected paths — must always fall back to alt text

Every case in this section must render as plain alt text and never as an
image, by design (see `is_local_image_path` in `crates/stele/src/media/sizer.rs`).

### A remote URL

*expect: alt text only — `scheme://` URLs are explicitly rejected, no
network fetch is ever attempted.*

![a remote image that must never be fetched](https://example.com/does-not-matter.png)

### An SVG file

*expect: alt text only — `.svg` is explicitly out of scope regardless of
whether the file exists.*

![an svg diagram, rejected purely by extension](img/diagram.svg)

### A path that does not exist

*expect: alt text only — the sizer's dimension probe fails to open a
missing file and returns None.*

![a path that does not exist on disk](img/does-not-exist.png)

### A path pointing at a directory

*expect: alt text only — `img/` is a directory, not an image file; the
probe fails to decode it.*

![a directory, not a file](img/)

### A path with `..` traversal

*expect: alt text only — this either escapes the doc's base directory to a
nonexistent file, or the ambiguity itself is enough to justify treating it
as untrusted; either way it must not silently render.*

![a path that tries to escape the testdocs directory](../not-a-real-image.png)

## A malformed / corrupt image file

*expect: alt text only — `corrupt.png` has a `.png` extension but is
actually plain text; the decoder must fail gracefully rather than crash.*

![a file with a .png extension that is not actually a valid PNG](img/corrupt.png)

## Scroll test: two images close together

*expect: scroll slowly through this section (line by line, then by page).
Watch for the recently-fixed ghost-trail bug — scrolling must never leave a
duplicate/stale copy of either image behind at the old scroll position.
Each image should appear exactly once, cleanly, as the viewport moves.*

First scroll marker — line A.

![blue scroll marker one](img/scroll-a.png)

A short paragraph of plain text between the two images, just long enough
to force a bit of vertical scrolling distance between them so the two
reserved boxes don't overlap in the same viewport at small terminal
heights.

![red scroll marker two](img/scroll-b.png)

Last scroll marker — line B. Scroll up and down repeatedly across this
whole section and confirm nothing is left behind.
