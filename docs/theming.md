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

## The three keys

| Key | Meaning |
|---|---|
| `name` | Shown in the status line. Optional. |
| `appearance` | `"dark"` or `"light"`. Decides which built-in your theme lays over, and therefore every colour you did *not* set. Defaults to `dark` with a warning. |
| `[colors]` | Role name → `#rrggbb` or `#rgb`. All optional. |

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

Syntax-highlighting colours (keywords, strings, comments) are **not** themeable.
They are generated rather than chosen, and naming them is a separate job.

## When something is wrong

Nothing in a theme file is fatal except TOML that will not parse at all. A bad
value costs you that one colour and nothing else — the role falls back to the
built-in, the rest of your file still applies, and stele tells you in the status
line. A typo'd role name will suggest the name you probably meant.

stele will also tell you, and then do it anyway, when:

- a colour is under its contrast floor against the page
- two roles merge into one colour in a 256-colour terminal

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

`themes/` holds three complete themes, each setting every role:

| | Appearance | Character |
|---|---|---|
| `ember.toml` | dark | Warm. Coal and firelight; headings fade by saturation, not darkness. |
| `foundry.toml` | light | Ink on paper. Near-black prose, one blue carrying structure. |
| `lichen.toml` | dark | Quiet, low-saturation green-grey. Headings separate by lightness alone. |

Copy one and edit it — they are the fastest way to see what every role does.
