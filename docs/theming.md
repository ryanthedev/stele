# Theming stele

A theme is one TOML file. You write it, or you download someone else's, and
stele uses it. There is nothing else to install and no directory to register it
in.

```toml
name = "Ember"
appearance = "dark"

[colors]
text = "#dbcec0"
heading1 = "#ff9147"
```

That file is a complete theme. Every role is optional — anything you leave out
keeps the built-in colour, so a five-line theme is a real theme and not a
half-finished one.

## Using one

```
stele --theme ./ember.toml notes.md      # just this run
```

Or save it as `~/.config/stele/theme.toml` and stele will use it every time.
`$XDG_CONFIG_HOME` is honoured if you set it.

The two paths differ on purpose when something goes wrong. A `--theme` path you
typed must exist and load — if it doesn't, stele exits and says so, because
silently giving you different colours than the ones you asked for is the wrong
kindness. The config path is the opposite: almost nobody has one, so its absence
costs nothing and says nothing.

There is no runtime theme switcher. To change themes, change the file or the
flag and start stele again.

## The four keys

| Key | Meaning |
|---|---|
| `name` | Shown in the status line. Optional. |
| `appearance` | `"dark"` or `"light"`. Decides which built-in your theme lays over, and therefore every colour you did *not* set. Defaults to `dark` with a warning. |
| `[colors]` | Markdown role name → `#rrggbb` or `#rgb`. All optional. |
| `[syntax]` | Syntax role name → colour. All optional, but see [Syntax colours](#syntax-colours) — this table is all-or-nothing in practice. |
| `[layout]` | Geometry, not colour: padding, the gutter, the reading line. All optional — see [The page](#the-page). |

`appearance` is the one people skip and shouldn't. A theme that sets six
colours inherits twenty-seven, and they have to come from the right end of the
page or the six you chose will sit on a background that fights them.

## What `T` does

With no theme file, `T` toggles the built-in dark and light variants, as it
always has.

With a theme file, `T` swaps between your theme and the **built-in of the
opposite appearance**. A single-appearance file has no other half, and `T`
exists for the moment the room gets bright — so it has to lead somewhere
legible rather than nowhere.

## Every role you can set

Roles marked *structural* paint furniture rather than words, and answer to
WCAG's 3:1 non-text floor instead of the 4.5:1 asked of text.

| Role | Colours |
|---|---|
| `text` | Body prose. **Unset by default** — with no theme, prose inherits your terminal's own foreground. |
| `emphasis` | `*italic*` |
| `strong` | `**bold**` |
| `strikethrough` | `~~struck~~` |
| `heading1`–`heading6` | The six heading levels. Setting one also moves that heading's depth markers and its ember rule — they are the same colour by definition, and a theme cannot separate them. |
| `code_inline` | `` `code spans` `` |
| `code_block` | Fenced block text that no syntax highlighter claimed |
| `code_block_bg` | **A background, not a foreground.** The slab a fenced block sits on, painted the full width of every line so the block has an edge and you can see where the code stops. Setting it also moves what `[syntax]` colours are measured against — see below. |
| `link` | Link labels |
| `image_alt` | Alt text shown in place of an image |
| `math` | TeX source shown in place of a formula |
| `list_marker` | *structural* — bullets and ordered numbers |
| `task_marker` | *structural* — `[x]` / `[ ]` |
| `blockquote` | *structural* — the quote gutter bar |
| `alert_note`, `alert_tip`, `alert_important`, `alert_warning`, `alert_caution` | GitHub alert titles |
| `rule` | *structural* — thematic breaks |
| `table_border` | *structural* — cell separators and the header rule |
| `table_header` | Header cell text |
| `footnote_ref` | `[^1]` in running text |
| `footnote_label` | `[^1]` marking a definition |
| `html` | Raw HTML shown as literal text |
| `front_matter` | YAML frontmatter shown as literal text |
| `overflow` | *structural* — the `…` clip indicator |
| `search_match` | Text matching the active search |
| `search_current` | The match you are currently on |
| `line_number` | *structural* — a row's number in the gutter |
| `line_number_current` | *structural* — the number on the reading line |
| `gutter_border` | *structural* — the rule between the gutter and the page |
| `current_line_bg` | **A background, not a foreground.** The band under the reading line — see [The reading line](#the-reading-line). |

## The page

One table, and it is the only part of a theme file that is not a palette:

```toml
[layout]
padding_left = 2
padding_right = 2
line_numbers = true
```

Every key is optional and every default is the frame stele drew before any of
this existed, so a theme that leaves `[layout]` out looks exactly as it always
did.

| Key | Default | Means |
|---|---|---|
| `padding_left`, `padding_right` | `0` | Dead cells either side of the page. Nothing is ever painted in them, and they sit *outside* the reading line's band — desk, not page. |
| `padding_top`, `padding_bottom` | `0` | Blank rows above and below the document. The status row is below all of it either way. |
| `line_numbers` | `false` | A number per rendered row, in a gutter down the left. |
| `gutter_gap` | `1` | Cells between the gutter's separator and the first character of the page. |
| `current_line` | `true` | Paint the band under the reading line. |
| `scrolloff` | `0` | Rows kept between the reading line and the edge of the page before it scrolls. |

Padding is capped at 64 cells a side, `gutter_gap` at 8 and `scrolloff` at 32.
A value outside its range is clamped and reported rather than dropped: someone
who wrote `padding_left = 200` wants a wide margin, and the widest available is
a better reading of that than none.

A terminal too narrow to hold the page plus its furniture gets **none** of the
furniture rather than a squeezed version of it. The threshold is 24 cells of
content, the same floor `--max-width` clamps against.

### Line numbers count rendered rows

Not source lines. stele renders markdown, and a rendered row is what it can
actually address: `scroll` is an index into rendered rows, the position
percentage is computed from them, `g` and `G` move between them. The number in
the gutter names the same thing the rest of the viewer names.

It follows that the numbers *move* when the window is resized, because the rows
did. It also follows that an eight-row image consumes eight numbers — which
looks wasteful until you remember the number is a scroll coordinate, and the
image really does occupy eight rows of scroll.

`#` toggles the gutter for one session, and `--line-numbers` turns it on for
one run whatever the theme says.

### The reading line

A pager has no cursor, so before anything could be highlighted stele had to
grow one. The **reading line** is it: the line every motion addresses, with the
viewport following it rather than the other way round.

That is a real change in how `j` behaves. A line key — `j`, `k`, the arrows —
moves the reader, and the page does not move until the reader reaches its
bottom edge. A page key — `PgDn`, `Ctrl-f`, `Ctrl-d` — moves the page and takes
the reader along, exactly as it always did. `G` now reaches the last *line*
rather than the last screen.

The reading line exists whether or not it is painted. Setting
`current_line = false` hides the band and changes no motion, because a key that
means different things depending on a display setting is worse than either
meaning.

`current_line_bg` is the band's colour, and it wins over everything it covers —
including a code block's own slab. A fence whose middle row loses the slab
colour still reads as a fence; a reading line you cannot find is the feature
not working.

**Over an image it cannot win.** A kitty raster is composited above the text
layer, so on an image row the band covers the gutter, the padding, and whatever
content cells the picture does not fill. That is why the gutter's separator
takes the reading line's colour too — on a row full of picture, the separator
and the number are the only cells left that can say where you are.

## Syntax colours

`[colors]` stops at the edge of a highlighted code block. A fenced block with a
language stele can highlight is re-tagged token by token, and those tokens
answer to `[syntax]` instead:

```toml
[syntax]
keyword = "#f0885f"
string = "#7fc070"
comment = "#8b9c9a"
```

The two tables never overlap. `code_inline` and `code_block` under `[colors]`
still cover inline spans and *unhighlighted* fences — a block with no language,
an unknown language, or one where highlighting timed out.

### Naming one syntax colour claims all of them

This is the one rule here that will surprise you, so it is worth stating
plainly. The moment `[syntax]` contains a single usable colour, stele stops
using its generated syntax palette for that block entirely. Every capture you
did **not** name falls back to `text` — not to the colour it had before.

That is deliberate. The generated palette is a golden-angle sweep through hue
space: colours chosen to be maximally unlike each other, not to be like
anything in particular. Leaving your unnamed tokens on it would put half a code
block in colours you picked and half in colours a machine picked, which reads
as a rendering fault rather than as a theme you haven't finished.

So a partial `[syntax]` table is legal, warns, and gives you a two-colour code
block — your keywords, and everything else in body-text colour. That is a real
thing to want. But if you want a full palette, fill the table in.

Leaving `[syntax]` out entirely changes nothing: your code blocks keep the
generated colours, exactly as they were.

### Every syntax colour you can set

| Role | Colours |
|---|---|
| `keyword` | `fn`, `let`, `class`, `def` |
| `keyword_control` | `if`, `return`, `import`, `try` — the ones that move execution |
| `function` | Function names, at definition and at call |
| `function_macro` | Macro invocations |
| `type` | Type names |
| `type_builtin` | `int`, `str`, `bool` |
| `constructor` | Constructors and enum variants |
| `namespace` | Module and namespace names |
| `variable` | Ordinary identifiers and parameters |
| `variable_builtin` | `self`, `this`, `super` |
| `property` | Struct fields and object members |
| `constant` | Named constants |
| `string` | String and character literals |
| `string_escape` | `\n`, and regex literals |
| `number` | Numeric literals |
| `boolean` | `true` / `false` |
| `comment` | Line and block comments |
| `comment_doc` | Doc comments |
| `operator` | `+`, `=`, `&&` |
| `punctuation` | Brackets, delimiters, separators |
| `attribute` | Annotations and decorators |
| `label` | Loop and goto labels |
| `tag` | HTML and JSX tag names |
| `error` | Text the parser could not make sense of |
| `plain` | Everything inside a highlighted block that matched none of the above |

`plain` is worth setting. It is the filler between tokens, and on a theme that
leaves it out it takes `text` — which is usually right, and occasionally not.

### The slab under it

Every code block is painted on a background, so its extent is visible: a reader
can see where the code stops without inferring it from where the colour changes
on the last line. There is a built-in one; `code_block_bg` replaces it.

This is the one place stele can honestly reproduce an editor theme's *page*. A
terminal owns its own background and stele never sets it — but a code block is a
surface stele actually paints, so `code_block_bg = "#282828"` really does put
gruvbox's page under your code. Every ported theme in `themes/ports/` sets its
upstream's real background for exactly this reason.

Setting it also changes what the contrast warnings mean, and for the better:
once a theme names a slab, `[syntax]` colours are measured against *that*
instead of against the page. A keyword's contrast with a background it never
touches was never the useful number.

Two attributes are not yours to set: `keyword` and `keyword_control` render
bold, `comment` and `comment_doc` render italic. Comments also render *dim* on
the built-in palette, but stele drops the dim for any comment colour you name —
faint blends a colour toward the background, and you picked that hex to be that
hex.

## When something is wrong

Nothing in a theme file is fatal except TOML that will not parse at all. A bad
value costs you that one colour and nothing else — the role falls back to the
built-in, the rest of your file still applies, and stele tells you in the status
line. A typo'd role name will suggest the name you probably meant.

stele will also tell you, and then do it anyway, when:

- a colour is under its contrast floor against the page
- two roles merge into one colour in a 256-colour terminal
- `[syntax]` names some captures and not others

The 256-colour check compares `[colors]` against `[colors]` and `[syntax]`
against `[syntax]`, never across the two. Colours only collide in a way you
care about when a reader sees them side by side, and a `keyword` never appears
next to a `table_border`.

Both are advice. They are hard requirements for stele's *built-in* themes and
deliberately not for yours — it is your terminal.

### About that 256-colour warning

You will probably see some, and the shipped themes all do. The xterm 256-colour
cube offers six steps per channel: 216 colours, of which exactly six are greys.
A palette coherent enough to read as one theme clusters inside a few hue
families, and a low-saturation theme is competing for six cells. Being
thirty-three-ways distinct after quantization is not achievable by hand, and
mostly not worth wanting — truecolor terminals, which is what stele targets,
show every colour exactly as you wrote it.

Where it does matter, the warning names the pair so you can decide.

## Worked examples

`themes/` holds three complete themes, each setting every role in both tables:

| | Appearance | Character |
|---|---|---|
| `ember.toml` | dark | Warm. Coal and firelight; headings fade by saturation, not darkness. |
| `foundry.toml` | light | Ink on paper. Near-black prose, one blue carrying structure. |
| `lichen.toml` | dark | Quiet, low-saturation green-grey. Headings separate by lightness alone. |

Copy one and edit it — they are the fastest way to see what every role does.

## Ports of editor themes

`themes/ports/` holds faithful ports of palettes you probably already run in
your editor:

| | Appearance | Upstream |
|---|---|---|
| `gruvbox-dark.toml` | dark | `morhetz/gruvbox` |
| `gruvbox-light.toml` | light | `morhetz/gruvbox` |
| `nord.toml` | dark | `arcticicestudio/nord` |
| `dracula.toml` | dark | `dracula/dracula-theme` |
| `catppuccin-mocha.toml` | dark | `catppuccin/catppuccin` |
| `tokyo-night.toml` | dark | `folke/tokyonight.nvim` |

```
stele --theme themes/ports/gruvbox-dark.toml notes.md
```

Three things are worth knowing before you use one.

**The `[syntax]` table is a port; the `[colors]` table is not.** Each project
publishes its own scope mapping and they disagree with each other more than you
would guess — Gruvbox paints keywords red, Nord frost blue, Dracula pink,
Catppuccin mauve, Tokyo Night purple. Those mappings are read from each
project's own source and each file cites which. The markdown roles are
different: an editor theme has no opinion about a blockquote gutter or a
footnote label, so those follow one convention across all six, drawn from that
theme's palette.

**They set colours, not a background.** stele paints foregrounds onto whatever
your terminal already is. A port looks like its editor when your terminal is
already set to that theme's background, and looks like that theme's colours on
your own background otherwise. Tokyo Night is the exception and a coincidence:
stele's dark reference background is `#1a1b26`, which *is* Tokyo Night's.

**They are faithful, which means they inherit their flaws.** Several of these
themes ship colours below WCAG AA — Dracula's comment blue, Nord's red, Tokyo
Night's comment grey, Gruvbox's faded yellow — on their own backgrounds as much
as on stele's. The ports keep them, so stele will warn you in the status line
every time you load one. Each file lists its own offenders in an `under-aa:`
header line, and a test holds that line to what the lint actually finds, so a
port cannot go illegible quietly. If you would rather have the contrast than the
fidelity, those lines are the ones to edit.

The three themes in `themes/` proper are held to the stricter bar: they must be
legible, and a test enforces it.

`themes/ports/generate.py` is what produced them, if you want to add another —
the palette-to-role mapping is the only part worth writing by hand.
