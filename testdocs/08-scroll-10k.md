# Scroll endurance — 10,000+ lines

Built for DW-5.2 (*full scroll of a 10k-line document: every frame wrapped in
paired mode-2026 markers; manual visual pass confirms no tearing*) and DW-5.3
(*resize storm: final layout correct, topmost visible block preserved*).

**How to use it**

- Hold `j` / arrow-down and let it run the whole way. Watch for tearing, flicker,
  or a half-drawn frame. Nothing should ever show a partially painted row.
- Every checkpoint is numbered 000 to 269. They must arrive strictly in
  order — a skipped or repeated checkpoint means the scroll math is off.
- Resize the window mid-scroll, repeatedly and violently. The checkpoint you were
  looking at should still be the one on screen afterwards (that is the block-
  anchored resize behavior, DW-5.3), and the layout must stay correct.
- The build stamp in the bottom-right corner should never flicker or move.

---

## Checkpoint 000

*expect: checkpoints arrive in strict order; this is 000 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 000, entry one
- list item at checkpoint 000, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 000 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 001

*expect: checkpoints arrive in strict order; this is 001 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 001, entry one
- list item at checkpoint 001, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 001 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 002

*expect: checkpoints arrive in strict order; this is 002 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 002, entry one
- list item at checkpoint 002, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 002 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 003

*expect: checkpoints arrive in strict order; this is 003 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 003, entry one
- list item at checkpoint 003, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 003 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 004

*expect: checkpoints arrive in strict order; this is 004 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 004, entry one
- list item at checkpoint 004, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 004 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 005

*expect: checkpoints arrive in strict order; this is 005 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 005, entry one
- list item at checkpoint 005, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 005 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 006

*expect: checkpoints arrive in strict order; this is 006 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 006, entry one
- list item at checkpoint 006, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 006 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 007

*expect: checkpoints arrive in strict order; this is 007 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 007, entry one
- list item at checkpoint 007, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 007 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 008

*expect: checkpoints arrive in strict order; this is 008 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 008, entry one
- list item at checkpoint 008, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 008 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 009

*expect: checkpoints arrive in strict order; this is 009 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 009, entry one
- list item at checkpoint 009, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 009 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 010

*expect: checkpoints arrive in strict order; this is 010 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 010, entry one
- list item at checkpoint 010, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 010 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 011

*expect: checkpoints arrive in strict order; this is 011 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 011, entry one
- list item at checkpoint 011, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 011 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 012

*expect: checkpoints arrive in strict order; this is 012 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 012, entry one
- list item at checkpoint 012, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 012 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 013

*expect: checkpoints arrive in strict order; this is 013 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 013, entry one
- list item at checkpoint 013, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 013 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 014

*expect: checkpoints arrive in strict order; this is 014 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 014, entry one
- list item at checkpoint 014, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 014 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 015

*expect: checkpoints arrive in strict order; this is 015 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 015, entry one
- list item at checkpoint 015, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 015 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 016

*expect: checkpoints arrive in strict order; this is 016 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 016, entry one
- list item at checkpoint 016, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 016 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 017

*expect: checkpoints arrive in strict order; this is 017 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 017, entry one
- list item at checkpoint 017, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 017 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 018

*expect: checkpoints arrive in strict order; this is 018 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 018, entry one
- list item at checkpoint 018, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 018 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 019

*expect: checkpoints arrive in strict order; this is 019 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 019, entry one
- list item at checkpoint 019, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 019 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 020

*expect: checkpoints arrive in strict order; this is 020 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 020, entry one
- list item at checkpoint 020, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 020 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 021

*expect: checkpoints arrive in strict order; this is 021 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 021, entry one
- list item at checkpoint 021, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 021 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 022

*expect: checkpoints arrive in strict order; this is 022 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 022, entry one
- list item at checkpoint 022, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 022 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 023

*expect: checkpoints arrive in strict order; this is 023 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 023, entry one
- list item at checkpoint 023, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 023 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 024

*expect: checkpoints arrive in strict order; this is 024 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 024, entry one
- list item at checkpoint 024, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 024 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 025

*expect: checkpoints arrive in strict order; this is 025 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 025, entry one
- list item at checkpoint 025, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 025 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 026

*expect: checkpoints arrive in strict order; this is 026 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 026, entry one
- list item at checkpoint 026, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 026 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 027

*expect: checkpoints arrive in strict order; this is 027 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 027, entry one
- list item at checkpoint 027, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 027 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 028

*expect: checkpoints arrive in strict order; this is 028 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 028, entry one
- list item at checkpoint 028, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 028 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 029

*expect: checkpoints arrive in strict order; this is 029 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 029, entry one
- list item at checkpoint 029, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 029 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 030

*expect: checkpoints arrive in strict order; this is 030 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 030, entry one
- list item at checkpoint 030, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 030 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 031

*expect: checkpoints arrive in strict order; this is 031 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 031, entry one
- list item at checkpoint 031, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 031 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 032

*expect: checkpoints arrive in strict order; this is 032 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 032, entry one
- list item at checkpoint 032, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 032 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 033

*expect: checkpoints arrive in strict order; this is 033 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 033, entry one
- list item at checkpoint 033, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 033 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 034

*expect: checkpoints arrive in strict order; this is 034 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 034, entry one
- list item at checkpoint 034, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 034 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 035

*expect: checkpoints arrive in strict order; this is 035 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 035, entry one
- list item at checkpoint 035, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 035 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 036

*expect: checkpoints arrive in strict order; this is 036 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 036, entry one
- list item at checkpoint 036, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 036 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 037

*expect: checkpoints arrive in strict order; this is 037 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 037, entry one
- list item at checkpoint 037, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 037 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 038

*expect: checkpoints arrive in strict order; this is 038 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 038, entry one
- list item at checkpoint 038, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 038 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 039

*expect: checkpoints arrive in strict order; this is 039 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 039, entry one
- list item at checkpoint 039, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 039 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 040

*expect: checkpoints arrive in strict order; this is 040 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 040, entry one
- list item at checkpoint 040, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 040 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 041

*expect: checkpoints arrive in strict order; this is 041 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 041, entry one
- list item at checkpoint 041, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 041 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 042

*expect: checkpoints arrive in strict order; this is 042 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 042, entry one
- list item at checkpoint 042, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 042 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 043

*expect: checkpoints arrive in strict order; this is 043 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 043, entry one
- list item at checkpoint 043, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 043 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 044

*expect: checkpoints arrive in strict order; this is 044 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 044, entry one
- list item at checkpoint 044, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 044 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 045

*expect: checkpoints arrive in strict order; this is 045 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 045, entry one
- list item at checkpoint 045, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 045 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 046

*expect: checkpoints arrive in strict order; this is 046 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 046, entry one
- list item at checkpoint 046, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 046 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 047

*expect: checkpoints arrive in strict order; this is 047 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 047, entry one
- list item at checkpoint 047, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 047 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 048

*expect: checkpoints arrive in strict order; this is 048 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 048, entry one
- list item at checkpoint 048, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 048 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 049

*expect: checkpoints arrive in strict order; this is 049 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 049, entry one
- list item at checkpoint 049, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 049 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 050

*expect: checkpoints arrive in strict order; this is 050 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 050, entry one
- list item at checkpoint 050, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 050 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 051

*expect: checkpoints arrive in strict order; this is 051 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 051, entry one
- list item at checkpoint 051, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 051 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 052

*expect: checkpoints arrive in strict order; this is 052 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 052, entry one
- list item at checkpoint 052, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 052 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 053

*expect: checkpoints arrive in strict order; this is 053 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 053, entry one
- list item at checkpoint 053, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 053 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 054

*expect: checkpoints arrive in strict order; this is 054 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 054, entry one
- list item at checkpoint 054, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 054 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 055

*expect: checkpoints arrive in strict order; this is 055 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 055, entry one
- list item at checkpoint 055, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 055 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 056

*expect: checkpoints arrive in strict order; this is 056 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 056, entry one
- list item at checkpoint 056, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 056 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 057

*expect: checkpoints arrive in strict order; this is 057 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 057, entry one
- list item at checkpoint 057, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 057 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 058

*expect: checkpoints arrive in strict order; this is 058 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 058, entry one
- list item at checkpoint 058, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 058 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 059

*expect: checkpoints arrive in strict order; this is 059 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 059, entry one
- list item at checkpoint 059, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 059 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 060

*expect: checkpoints arrive in strict order; this is 060 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 060, entry one
- list item at checkpoint 060, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 060 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 061

*expect: checkpoints arrive in strict order; this is 061 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 061, entry one
- list item at checkpoint 061, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 061 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 062

*expect: checkpoints arrive in strict order; this is 062 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 062, entry one
- list item at checkpoint 062, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 062 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 063

*expect: checkpoints arrive in strict order; this is 063 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 063, entry one
- list item at checkpoint 063, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 063 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 064

*expect: checkpoints arrive in strict order; this is 064 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 064, entry one
- list item at checkpoint 064, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 064 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 065

*expect: checkpoints arrive in strict order; this is 065 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 065, entry one
- list item at checkpoint 065, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 065 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 066

*expect: checkpoints arrive in strict order; this is 066 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 066, entry one
- list item at checkpoint 066, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 066 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 067

*expect: checkpoints arrive in strict order; this is 067 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 067, entry one
- list item at checkpoint 067, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 067 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 068

*expect: checkpoints arrive in strict order; this is 068 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 068, entry one
- list item at checkpoint 068, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 068 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 069

*expect: checkpoints arrive in strict order; this is 069 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 069, entry one
- list item at checkpoint 069, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 069 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 070

*expect: checkpoints arrive in strict order; this is 070 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 070, entry one
- list item at checkpoint 070, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 070 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 071

*expect: checkpoints arrive in strict order; this is 071 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 071, entry one
- list item at checkpoint 071, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 071 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 072

*expect: checkpoints arrive in strict order; this is 072 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 072, entry one
- list item at checkpoint 072, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 072 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 073

*expect: checkpoints arrive in strict order; this is 073 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 073, entry one
- list item at checkpoint 073, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 073 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 074

*expect: checkpoints arrive in strict order; this is 074 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 074, entry one
- list item at checkpoint 074, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 074 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 075

*expect: checkpoints arrive in strict order; this is 075 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 075, entry one
- list item at checkpoint 075, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 075 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 076

*expect: checkpoints arrive in strict order; this is 076 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 076, entry one
- list item at checkpoint 076, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 076 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 077

*expect: checkpoints arrive in strict order; this is 077 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 077, entry one
- list item at checkpoint 077, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 077 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 078

*expect: checkpoints arrive in strict order; this is 078 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 078, entry one
- list item at checkpoint 078, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 078 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 079

*expect: checkpoints arrive in strict order; this is 079 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 079, entry one
- list item at checkpoint 079, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 079 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 080

*expect: checkpoints arrive in strict order; this is 080 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 080, entry one
- list item at checkpoint 080, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 080 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 081

*expect: checkpoints arrive in strict order; this is 081 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 081, entry one
- list item at checkpoint 081, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 081 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 082

*expect: checkpoints arrive in strict order; this is 082 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 082, entry one
- list item at checkpoint 082, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 082 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 083

*expect: checkpoints arrive in strict order; this is 083 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 083, entry one
- list item at checkpoint 083, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 083 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 084

*expect: checkpoints arrive in strict order; this is 084 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 084, entry one
- list item at checkpoint 084, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 084 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 085

*expect: checkpoints arrive in strict order; this is 085 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 085, entry one
- list item at checkpoint 085, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 085 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 086

*expect: checkpoints arrive in strict order; this is 086 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 086, entry one
- list item at checkpoint 086, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 086 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 087

*expect: checkpoints arrive in strict order; this is 087 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 087, entry one
- list item at checkpoint 087, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 087 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 088

*expect: checkpoints arrive in strict order; this is 088 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 088, entry one
- list item at checkpoint 088, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 088 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 089

*expect: checkpoints arrive in strict order; this is 089 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 089, entry one
- list item at checkpoint 089, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 089 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 090

*expect: checkpoints arrive in strict order; this is 090 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 090, entry one
- list item at checkpoint 090, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 090 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 091

*expect: checkpoints arrive in strict order; this is 091 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 091, entry one
- list item at checkpoint 091, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 091 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 092

*expect: checkpoints arrive in strict order; this is 092 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 092, entry one
- list item at checkpoint 092, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 092 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 093

*expect: checkpoints arrive in strict order; this is 093 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 093, entry one
- list item at checkpoint 093, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 093 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 094

*expect: checkpoints arrive in strict order; this is 094 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 094, entry one
- list item at checkpoint 094, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 094 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 095

*expect: checkpoints arrive in strict order; this is 095 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 095, entry one
- list item at checkpoint 095, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 095 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 096

*expect: checkpoints arrive in strict order; this is 096 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 096, entry one
- list item at checkpoint 096, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 096 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 097

*expect: checkpoints arrive in strict order; this is 097 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 097, entry one
- list item at checkpoint 097, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 097 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 098

*expect: checkpoints arrive in strict order; this is 098 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 098, entry one
- list item at checkpoint 098, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 098 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 099

*expect: checkpoints arrive in strict order; this is 099 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 099, entry one
- list item at checkpoint 099, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 099 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 100

*expect: checkpoints arrive in strict order; this is 100 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 100, entry one
- list item at checkpoint 100, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 100 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 101

*expect: checkpoints arrive in strict order; this is 101 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 101, entry one
- list item at checkpoint 101, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 101 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 102

*expect: checkpoints arrive in strict order; this is 102 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 102, entry one
- list item at checkpoint 102, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 102 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 103

*expect: checkpoints arrive in strict order; this is 103 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 103, entry one
- list item at checkpoint 103, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 103 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 104

*expect: checkpoints arrive in strict order; this is 104 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 104, entry one
- list item at checkpoint 104, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 104 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 105

*expect: checkpoints arrive in strict order; this is 105 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 105, entry one
- list item at checkpoint 105, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 105 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 106

*expect: checkpoints arrive in strict order; this is 106 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 106, entry one
- list item at checkpoint 106, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 106 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 107

*expect: checkpoints arrive in strict order; this is 107 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 107, entry one
- list item at checkpoint 107, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 107 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 108

*expect: checkpoints arrive in strict order; this is 108 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 108, entry one
- list item at checkpoint 108, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 108 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 109

*expect: checkpoints arrive in strict order; this is 109 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 109, entry one
- list item at checkpoint 109, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 109 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 110

*expect: checkpoints arrive in strict order; this is 110 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 110, entry one
- list item at checkpoint 110, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 110 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 111

*expect: checkpoints arrive in strict order; this is 111 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 111, entry one
- list item at checkpoint 111, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 111 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 112

*expect: checkpoints arrive in strict order; this is 112 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 112, entry one
- list item at checkpoint 112, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 112 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 113

*expect: checkpoints arrive in strict order; this is 113 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 113, entry one
- list item at checkpoint 113, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 113 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 114

*expect: checkpoints arrive in strict order; this is 114 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 114, entry one
- list item at checkpoint 114, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 114 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 115

*expect: checkpoints arrive in strict order; this is 115 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 115, entry one
- list item at checkpoint 115, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 115 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 116

*expect: checkpoints arrive in strict order; this is 116 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 116, entry one
- list item at checkpoint 116, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 116 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 117

*expect: checkpoints arrive in strict order; this is 117 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 117, entry one
- list item at checkpoint 117, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 117 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 118

*expect: checkpoints arrive in strict order; this is 118 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 118, entry one
- list item at checkpoint 118, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 118 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 119

*expect: checkpoints arrive in strict order; this is 119 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 119, entry one
- list item at checkpoint 119, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 119 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 120

*expect: checkpoints arrive in strict order; this is 120 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 120, entry one
- list item at checkpoint 120, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 120 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 121

*expect: checkpoints arrive in strict order; this is 121 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 121, entry one
- list item at checkpoint 121, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 121 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 122

*expect: checkpoints arrive in strict order; this is 122 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 122, entry one
- list item at checkpoint 122, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 122 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 123

*expect: checkpoints arrive in strict order; this is 123 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 123, entry one
- list item at checkpoint 123, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 123 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 124

*expect: checkpoints arrive in strict order; this is 124 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 124, entry one
- list item at checkpoint 124, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 124 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 125

*expect: checkpoints arrive in strict order; this is 125 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 125, entry one
- list item at checkpoint 125, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 125 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 126

*expect: checkpoints arrive in strict order; this is 126 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 126, entry one
- list item at checkpoint 126, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 126 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 127

*expect: checkpoints arrive in strict order; this is 127 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 127, entry one
- list item at checkpoint 127, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 127 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 128

*expect: checkpoints arrive in strict order; this is 128 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 128, entry one
- list item at checkpoint 128, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 128 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 129

*expect: checkpoints arrive in strict order; this is 129 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 129, entry one
- list item at checkpoint 129, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 129 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 130

*expect: checkpoints arrive in strict order; this is 130 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 130, entry one
- list item at checkpoint 130, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 130 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 131

*expect: checkpoints arrive in strict order; this is 131 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 131, entry one
- list item at checkpoint 131, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 131 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 132

*expect: checkpoints arrive in strict order; this is 132 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 132, entry one
- list item at checkpoint 132, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 132 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 133

*expect: checkpoints arrive in strict order; this is 133 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 133, entry one
- list item at checkpoint 133, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 133 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 134

*expect: checkpoints arrive in strict order; this is 134 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 134, entry one
- list item at checkpoint 134, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 134 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 135

*expect: checkpoints arrive in strict order; this is 135 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 135, entry one
- list item at checkpoint 135, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 135 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 136

*expect: checkpoints arrive in strict order; this is 136 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 136, entry one
- list item at checkpoint 136, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 136 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 137

*expect: checkpoints arrive in strict order; this is 137 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 137, entry one
- list item at checkpoint 137, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 137 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 138

*expect: checkpoints arrive in strict order; this is 138 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 138, entry one
- list item at checkpoint 138, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 138 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 139

*expect: checkpoints arrive in strict order; this is 139 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 139, entry one
- list item at checkpoint 139, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 139 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 140

*expect: checkpoints arrive in strict order; this is 140 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 140, entry one
- list item at checkpoint 140, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 140 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 141

*expect: checkpoints arrive in strict order; this is 141 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 141, entry one
- list item at checkpoint 141, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 141 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 142

*expect: checkpoints arrive in strict order; this is 142 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 142, entry one
- list item at checkpoint 142, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 142 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 143

*expect: checkpoints arrive in strict order; this is 143 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 143, entry one
- list item at checkpoint 143, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 143 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 144

*expect: checkpoints arrive in strict order; this is 144 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 144, entry one
- list item at checkpoint 144, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 144 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 145

*expect: checkpoints arrive in strict order; this is 145 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 145, entry one
- list item at checkpoint 145, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 145 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 146

*expect: checkpoints arrive in strict order; this is 146 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 146, entry one
- list item at checkpoint 146, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 146 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 147

*expect: checkpoints arrive in strict order; this is 147 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 147, entry one
- list item at checkpoint 147, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 147 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 148

*expect: checkpoints arrive in strict order; this is 148 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 148, entry one
- list item at checkpoint 148, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 148 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 149

*expect: checkpoints arrive in strict order; this is 149 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 149, entry one
- list item at checkpoint 149, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 149 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 150

*expect: checkpoints arrive in strict order; this is 150 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 150, entry one
- list item at checkpoint 150, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 150 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 151

*expect: checkpoints arrive in strict order; this is 151 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 151, entry one
- list item at checkpoint 151, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 151 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 152

*expect: checkpoints arrive in strict order; this is 152 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 152, entry one
- list item at checkpoint 152, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 152 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 153

*expect: checkpoints arrive in strict order; this is 153 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 153, entry one
- list item at checkpoint 153, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 153 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 154

*expect: checkpoints arrive in strict order; this is 154 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 154, entry one
- list item at checkpoint 154, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 154 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 155

*expect: checkpoints arrive in strict order; this is 155 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 155, entry one
- list item at checkpoint 155, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 155 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 156

*expect: checkpoints arrive in strict order; this is 156 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 156, entry one
- list item at checkpoint 156, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 156 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 157

*expect: checkpoints arrive in strict order; this is 157 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 157, entry one
- list item at checkpoint 157, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 157 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 158

*expect: checkpoints arrive in strict order; this is 158 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 158, entry one
- list item at checkpoint 158, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 158 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 159

*expect: checkpoints arrive in strict order; this is 159 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 159, entry one
- list item at checkpoint 159, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 159 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 160

*expect: checkpoints arrive in strict order; this is 160 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 160, entry one
- list item at checkpoint 160, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 160 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 161

*expect: checkpoints arrive in strict order; this is 161 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 161, entry one
- list item at checkpoint 161, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 161 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 162

*expect: checkpoints arrive in strict order; this is 162 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 162, entry one
- list item at checkpoint 162, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 162 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 163

*expect: checkpoints arrive in strict order; this is 163 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 163, entry one
- list item at checkpoint 163, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 163 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 164

*expect: checkpoints arrive in strict order; this is 164 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 164, entry one
- list item at checkpoint 164, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 164 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 165

*expect: checkpoints arrive in strict order; this is 165 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 165, entry one
- list item at checkpoint 165, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 165 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 166

*expect: checkpoints arrive in strict order; this is 166 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 166, entry one
- list item at checkpoint 166, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 166 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 167

*expect: checkpoints arrive in strict order; this is 167 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 167, entry one
- list item at checkpoint 167, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 167 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 168

*expect: checkpoints arrive in strict order; this is 168 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 168, entry one
- list item at checkpoint 168, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 168 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 169

*expect: checkpoints arrive in strict order; this is 169 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 169, entry one
- list item at checkpoint 169, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 169 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 170

*expect: checkpoints arrive in strict order; this is 170 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 170, entry one
- list item at checkpoint 170, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 170 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 171

*expect: checkpoints arrive in strict order; this is 171 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 171, entry one
- list item at checkpoint 171, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 171 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 172

*expect: checkpoints arrive in strict order; this is 172 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 172, entry one
- list item at checkpoint 172, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 172 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 173

*expect: checkpoints arrive in strict order; this is 173 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 173, entry one
- list item at checkpoint 173, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 173 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 174

*expect: checkpoints arrive in strict order; this is 174 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 174, entry one
- list item at checkpoint 174, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 174 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 175

*expect: checkpoints arrive in strict order; this is 175 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 175, entry one
- list item at checkpoint 175, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 175 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 176

*expect: checkpoints arrive in strict order; this is 176 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 176, entry one
- list item at checkpoint 176, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 176 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 177

*expect: checkpoints arrive in strict order; this is 177 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 177, entry one
- list item at checkpoint 177, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 177 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 178

*expect: checkpoints arrive in strict order; this is 178 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 178, entry one
- list item at checkpoint 178, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 178 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 179

*expect: checkpoints arrive in strict order; this is 179 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 179, entry one
- list item at checkpoint 179, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 179 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 180

*expect: checkpoints arrive in strict order; this is 180 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 180, entry one
- list item at checkpoint 180, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 180 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 181

*expect: checkpoints arrive in strict order; this is 181 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 181, entry one
- list item at checkpoint 181, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 181 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 182

*expect: checkpoints arrive in strict order; this is 182 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 182, entry one
- list item at checkpoint 182, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 182 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 183

*expect: checkpoints arrive in strict order; this is 183 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 183, entry one
- list item at checkpoint 183, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 183 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 184

*expect: checkpoints arrive in strict order; this is 184 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 184, entry one
- list item at checkpoint 184, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 184 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 185

*expect: checkpoints arrive in strict order; this is 185 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 185, entry one
- list item at checkpoint 185, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 185 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 186

*expect: checkpoints arrive in strict order; this is 186 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 186, entry one
- list item at checkpoint 186, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 186 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 187

*expect: checkpoints arrive in strict order; this is 187 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 187, entry one
- list item at checkpoint 187, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 187 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 188

*expect: checkpoints arrive in strict order; this is 188 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 188, entry one
- list item at checkpoint 188, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 188 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 189

*expect: checkpoints arrive in strict order; this is 189 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 189, entry one
- list item at checkpoint 189, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 189 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 190

*expect: checkpoints arrive in strict order; this is 190 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 190, entry one
- list item at checkpoint 190, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 190 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 191

*expect: checkpoints arrive in strict order; this is 191 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 191, entry one
- list item at checkpoint 191, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 191 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 192

*expect: checkpoints arrive in strict order; this is 192 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 192, entry one
- list item at checkpoint 192, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 192 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 193

*expect: checkpoints arrive in strict order; this is 193 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 193, entry one
- list item at checkpoint 193, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 193 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 194

*expect: checkpoints arrive in strict order; this is 194 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 194, entry one
- list item at checkpoint 194, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 194 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 195

*expect: checkpoints arrive in strict order; this is 195 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 195, entry one
- list item at checkpoint 195, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 195 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 196

*expect: checkpoints arrive in strict order; this is 196 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 196, entry one
- list item at checkpoint 196, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 196 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 197

*expect: checkpoints arrive in strict order; this is 197 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 197, entry one
- list item at checkpoint 197, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 197 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 198

*expect: checkpoints arrive in strict order; this is 198 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 198, entry one
- list item at checkpoint 198, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 198 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 199

*expect: checkpoints arrive in strict order; this is 199 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 199, entry one
- list item at checkpoint 199, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 199 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 200

*expect: checkpoints arrive in strict order; this is 200 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 200, entry one
- list item at checkpoint 200, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 200 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 201

*expect: checkpoints arrive in strict order; this is 201 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 201, entry one
- list item at checkpoint 201, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 201 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 202

*expect: checkpoints arrive in strict order; this is 202 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 202, entry one
- list item at checkpoint 202, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 202 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 203

*expect: checkpoints arrive in strict order; this is 203 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 203, entry one
- list item at checkpoint 203, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 203 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 204

*expect: checkpoints arrive in strict order; this is 204 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 204, entry one
- list item at checkpoint 204, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 204 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 205

*expect: checkpoints arrive in strict order; this is 205 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 205, entry one
- list item at checkpoint 205, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 205 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 206

*expect: checkpoints arrive in strict order; this is 206 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 206, entry one
- list item at checkpoint 206, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 206 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 207

*expect: checkpoints arrive in strict order; this is 207 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 207, entry one
- list item at checkpoint 207, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 207 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 208

*expect: checkpoints arrive in strict order; this is 208 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 208, entry one
- list item at checkpoint 208, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 208 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 209

*expect: checkpoints arrive in strict order; this is 209 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 209, entry one
- list item at checkpoint 209, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 209 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 210

*expect: checkpoints arrive in strict order; this is 210 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 210, entry one
- list item at checkpoint 210, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 210 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 211

*expect: checkpoints arrive in strict order; this is 211 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 211, entry one
- list item at checkpoint 211, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 211 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 212

*expect: checkpoints arrive in strict order; this is 212 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 212, entry one
- list item at checkpoint 212, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 212 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 213

*expect: checkpoints arrive in strict order; this is 213 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 213, entry one
- list item at checkpoint 213, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 213 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 214

*expect: checkpoints arrive in strict order; this is 214 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 214, entry one
- list item at checkpoint 214, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 214 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 215

*expect: checkpoints arrive in strict order; this is 215 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 215, entry one
- list item at checkpoint 215, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 215 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 216

*expect: checkpoints arrive in strict order; this is 216 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 216, entry one
- list item at checkpoint 216, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 216 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 217

*expect: checkpoints arrive in strict order; this is 217 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 217, entry one
- list item at checkpoint 217, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 217 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 218

*expect: checkpoints arrive in strict order; this is 218 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 218, entry one
- list item at checkpoint 218, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 218 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 219

*expect: checkpoints arrive in strict order; this is 219 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 219, entry one
- list item at checkpoint 219, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 219 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 220

*expect: checkpoints arrive in strict order; this is 220 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 220, entry one
- list item at checkpoint 220, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 220 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 221

*expect: checkpoints arrive in strict order; this is 221 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 221, entry one
- list item at checkpoint 221, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 221 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 222

*expect: checkpoints arrive in strict order; this is 222 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 222, entry one
- list item at checkpoint 222, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 222 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 223

*expect: checkpoints arrive in strict order; this is 223 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 223, entry one
- list item at checkpoint 223, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 223 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 224

*expect: checkpoints arrive in strict order; this is 224 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 224, entry one
- list item at checkpoint 224, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 224 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 225

*expect: checkpoints arrive in strict order; this is 225 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 225, entry one
- list item at checkpoint 225, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 225 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 226

*expect: checkpoints arrive in strict order; this is 226 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 226, entry one
- list item at checkpoint 226, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 226 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 227

*expect: checkpoints arrive in strict order; this is 227 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 227, entry one
- list item at checkpoint 227, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 227 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 228

*expect: checkpoints arrive in strict order; this is 228 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 228, entry one
- list item at checkpoint 228, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 228 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 229

*expect: checkpoints arrive in strict order; this is 229 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 229, entry one
- list item at checkpoint 229, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 229 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 230

*expect: checkpoints arrive in strict order; this is 230 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 230, entry one
- list item at checkpoint 230, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 230 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 231

*expect: checkpoints arrive in strict order; this is 231 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 231, entry one
- list item at checkpoint 231, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 231 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 232

*expect: checkpoints arrive in strict order; this is 232 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 232, entry one
- list item at checkpoint 232, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 232 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 233

*expect: checkpoints arrive in strict order; this is 233 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 233, entry one
- list item at checkpoint 233, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 233 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 234

*expect: checkpoints arrive in strict order; this is 234 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 234, entry one
- list item at checkpoint 234, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 234 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 235

*expect: checkpoints arrive in strict order; this is 235 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 235, entry one
- list item at checkpoint 235, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 235 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 236

*expect: checkpoints arrive in strict order; this is 236 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 236, entry one
- list item at checkpoint 236, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 236 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 237

*expect: checkpoints arrive in strict order; this is 237 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 237, entry one
- list item at checkpoint 237, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 237 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 238

*expect: checkpoints arrive in strict order; this is 238 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 238, entry one
- list item at checkpoint 238, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 238 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 239

*expect: checkpoints arrive in strict order; this is 239 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 239, entry one
- list item at checkpoint 239, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 239 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 240

*expect: checkpoints arrive in strict order; this is 240 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 240, entry one
- list item at checkpoint 240, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 240 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 241

*expect: checkpoints arrive in strict order; this is 241 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 241, entry one
- list item at checkpoint 241, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 241 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 242

*expect: checkpoints arrive in strict order; this is 242 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 242, entry one
- list item at checkpoint 242, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 242 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 243

*expect: checkpoints arrive in strict order; this is 243 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 243, entry one
- list item at checkpoint 243, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 243 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 244

*expect: checkpoints arrive in strict order; this is 244 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 244, entry one
- list item at checkpoint 244, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 244 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 245

*expect: checkpoints arrive in strict order; this is 245 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 245, entry one
- list item at checkpoint 245, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 245 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 246

*expect: checkpoints arrive in strict order; this is 246 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 246, entry one
- list item at checkpoint 246, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 246 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 247

*expect: checkpoints arrive in strict order; this is 247 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 247, entry one
- list item at checkpoint 247, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 247 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 248

*expect: checkpoints arrive in strict order; this is 248 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 248, entry one
- list item at checkpoint 248, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 248 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 249

*expect: checkpoints arrive in strict order; this is 249 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 249, entry one
- list item at checkpoint 249, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 249 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 250

*expect: checkpoints arrive in strict order; this is 250 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 250, entry one
- list item at checkpoint 250, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 250 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 251

*expect: checkpoints arrive in strict order; this is 251 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 251, entry one
- list item at checkpoint 251, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 251 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 252

*expect: checkpoints arrive in strict order; this is 252 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 252, entry one
- list item at checkpoint 252, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 252 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 253

*expect: checkpoints arrive in strict order; this is 253 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 253, entry one
- list item at checkpoint 253, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 253 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 254

*expect: checkpoints arrive in strict order; this is 254 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 254, entry one
- list item at checkpoint 254, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 254 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 255

*expect: checkpoints arrive in strict order; this is 255 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 255, entry one
- list item at checkpoint 255, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 255 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 256

*expect: checkpoints arrive in strict order; this is 256 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 256, entry one
- list item at checkpoint 256, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 256 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 257

*expect: checkpoints arrive in strict order; this is 257 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 257, entry one
- list item at checkpoint 257, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 257 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 258

*expect: checkpoints arrive in strict order; this is 258 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 258, entry one
- list item at checkpoint 258, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 258 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 259

*expect: checkpoints arrive in strict order; this is 259 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 259, entry one
- list item at checkpoint 259, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 259 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 260

*expect: checkpoints arrive in strict order; this is 260 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 260, entry one
- list item at checkpoint 260, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 260 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 261

*expect: checkpoints arrive in strict order; this is 261 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 261, entry one
- list item at checkpoint 261, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 261 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 262

*expect: checkpoints arrive in strict order; this is 262 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 262, entry one
- list item at checkpoint 262, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 262 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 263

*expect: checkpoints arrive in strict order; this is 263 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 263, entry one
- list item at checkpoint 263, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 263 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 264

*expect: checkpoints arrive in strict order; this is 264 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 264, entry one
- list item at checkpoint 264, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 264 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 265

*expect: checkpoints arrive in strict order; this is 265 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 265, entry one
- list item at checkpoint 265, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 265 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 266

*expect: checkpoints arrive in strict order; this is 266 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 266, entry one
- list item at checkpoint 266, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 266 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 267

*expect: checkpoints arrive in strict order; this is 267 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 267, entry one
- list item at checkpoint 267, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 267 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 268

*expect: checkpoints arrive in strict order; this is 268 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 268, entry one
- list item at checkpoint 268, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 268 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

## Checkpoint 269

*expect: checkpoints arrive in strict order; this is 269 of 269.*

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

- list item at checkpoint 269, entry one
- list item at checkpoint 269, entry two
  - nested under entry two

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

| column A | column B | column C |
| --- | :---: | ---: |
| block 269 | centered | right |
| alignment must hold at every scroll offset | x | 42 |

Scrolling a retained line-box tree should be a pure index shift: the same
lines, painted at a different offset, with no reflow and no reparse. If a
line's content changes as it moves up the screen, that is a bug.

