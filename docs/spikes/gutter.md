# Spike: the gutter, line numbers, and a current line

What the mocks answered before any of this was built. Four treatments were
painted into a real Ghostty pane with Ember's palette and photographed; the
screenshots are gone but the conclusions they forced are here, because every
one of them is a thing the code now does for a reason that is not obvious from
the code.

## Line numbers count *rendered* rows, not source lines

The tempting reading of "line numbers" is the source file's, the way `less -N`
does it. stele cannot honestly do that, and the reason is structural rather
than a matter of effort: `LayoutTree::line_blocks` tags each row with the
**top-level** block that produced it (`crates/layout/src/block.rs` — only
`depth == 1` sets `current_block`). A twenty-row list is one top-level block,
so a source-line gutter would print the same number twenty times down the side
of it, and a nested blockquote would print its container's.

Rendered rows are the thing stele actually addresses. `scroll` is an index into
them, the position percentage is computed from them, `g`/`G` move between them.
Numbering them means the number in the gutter names the same thing every other
part of the viewer already names. It also means the numbers move when the
viewport reflows, which is honest: the rows themselves moved.

## An image row is a numbered row

An eight-row image consumes eight numbers. That looks wasteful until you
remember what the number is for — it is a scroll coordinate, and the image
really does occupy eight rows of scroll. Skipping them would make the gutter
disagree with `G`.

## The current-line band stops at the page, not at the terminal

Mocked both ways. stele's content column is capped at 100 cells, so on a wide
terminal a band running to the right edge is a hundred cells of colour with
nothing in it — it reads as a rendering fault. A band that ends where the page
ends reads as a page. The band therefore spans
`[padding_left, padding_left + gutter + content + padding_right)`.

## The band wins over the code slab

A fenced block paints its own background. When the reading line is inside one,
two backgrounds want the same cells. The band takes them. A code block whose
middle row loses the slab colour still reads as a code block — the syntax
colours carry it — whereas a reading line you cannot find is the feature not
working. This is also what every editor does.

## Over an image, the band is whatever the raster leaves

Kitty places a raster at `z=0`, which the protocol defines as above the text
layer. Nothing painted into those cells survives. So on an image row the band
covers the gutter, the padding, and any content cells the box does not fill,
and the raster covers the rest. The gutter's separator glyph turning accent-
coloured is what makes the reading line findable there, which is most of why
the separator exists at all.

## Padding sits outside the band

Mocked both ways. With `padding_left` outside, the band's left edge is the
number column and the padding reads as desk around a page. With it inside, the
band starts at the terminal edge and the page loses its edge. Outside won.

## The gutter's width is derived, and the derivation can chase its own tail

The number column is as wide as the document's line count needs. But the gutter
narrows the content column, which rewraps the document, which changes the line
count, which can change the width.

`AppState::layout_fitting_the_gutter` lays out once at the width the caller
budgeted, then asks what gutter the resulting line count actually needs. If it
grew, it lays out again with the content narrowed by exactly that growth. Two
passes at most, and only ever a second one when the gutter got wider — which is
the only direction that can clip a number.

It terminates because the pair is monotone in the direction that matters:
narrowing the content can only *add* lines, so the digit count cannot shrink
between the two passes. A third pass could differ only if a single cell of
content crossed a power-of-ten boundary in line count, which costs a gutter one
cell wider than it strictly needed — invisible, and much cheaper than laying a
ten-thousand-line document out until a fixed point falls out.
