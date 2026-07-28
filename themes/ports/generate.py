#!/usr/bin/env python3
"""Emit stele theme files from published editor palettes.

The mapping is the work; emitting TOML is not.

Two maps per theme, and the split is deliberate. `COLORS` is shared, because
markdown roles — headings, alerts, a blockquote gutter — have no upstream
equivalent to be faithful to; an editor theme has no opinion about them.
`syntax` is per-theme, because every one of these projects publishes its own
scope mapping and they disagree with each other far more than you would guess.
Gruvbox paints keywords red, Nord frost blue, Dracula pink, Catppuccin mauve,
Tokyo Night purple. An earlier version of this file used one shared syntax map
for all six and produced six themes that were recognisably the same picture in
different paint — which is the bug this structure exists to prevent, and the
`distinct syntax mappings` line at the bottom of a run is what watches for it.

Each theme cites where its mapping came from:
  gruvbox      morhetz/gruvbox colors/gruvbox.vim  (the `hi! link` lines)
  nord         nordtheme.com/docs/colors-and-palettes  (per-colour usage)
  dracula      spec.draculatheme.com
  catppuccin   catppuccin/nvim groups/syntax.lua + groups/treesitter.lua
  tokyonight   folke/tokyonight.nvim groups/base.lua + groups/treesitter.lua

Checks every emitted file against the same WCAG floors crates/highlight uses.
"""
import pathlib

DARK_BG = (0x1a, 0x1b, 0x26)
LIGHT_BG = (0xff, 0xff, 0xff)
AA_TEXT, AA_NONTEXT = 4.5, 3.0
STRUCTURAL = {"rule", "table_border", "blockquote", "list_marker", "task_marker", "overflow"}


def rgb(h):
    h = h.lstrip("#")
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))


def hexs(c):
    return "#%02x%02x%02x" % c


def lum(c):
    def ch(v):
        v /= 255.0
        return v / 12.92 if v <= 0.03928 else ((v + 0.055) / 1.055) ** 2.4
    return 0.2126 * ch(c[0]) + 0.7152 * ch(c[1]) + 0.0722 * ch(c[2])


def ratio(a, b):
    x, y = lum(a), lum(b)
    return (max(x, y) + 0.05) / (min(x, y) + 0.05)


def darken(h, amount):
    return hexs(tuple(round(v * (1 - amount)) for v in rgb(h)))


def blend_bg(h, amount, bg="#1a1b26"):
    """tokyonight's Util.blend_bg: `amount` of the colour, the rest background."""
    a, b = rgb(h), rgb(bg)
    return hexs(tuple(round(a[i] * amount + b[i] * (1 - amount)) for i in range(3)))


def ramp(start, end, n=6):
    a, b = rgb(start), rgb(end)
    return [hexs(tuple(round(a[i] + (b[i] - a[i]) * s / (n - 1)) for i in range(3)))
            for s in range(n)]


# Markdown roles. Shared on purpose — see the module docstring.
COLORS = {
    "text": "fg", "emphasis": "fg_dim", "strong": "fg_bright", "strikethrough": "gray",
    "code_inline": "aqua", "code_block": "fg_dim",
    "link": "blue_bright", "image_alt": "gray_light", "math": "purple",
    "list_marker": "orange", "task_marker": "green_bright", "blockquote": "gray_light",
    "alert_note": "blue_bright", "alert_tip": "green_bright",
    "alert_important": "purple_bright", "alert_warning": "yellow_bright",
    "alert_caution": "red_bright",
    "rule": "gray_light", "table_border": "gray_light", "table_header": "yellow",
    "footnote_ref": "aqua", "footnote_label": "gray_light",
    "html": "gray_light", "front_matter": "gray_light", "overflow": "gray_light",
    "search_match": "yellow_bright", "search_current": "orange_bright",
}

COLOR_ORDER = ["text", "emphasis", "strong", "strikethrough",
               "heading1", "heading2", "heading3", "heading4", "heading5", "heading6",
               "code_inline", "code_block", "link", "image_alt", "math",
               "list_marker", "task_marker", "blockquote",
               "alert_note", "alert_tip", "alert_important", "alert_warning",
               "alert_caution", "rule", "table_border", "table_header",
               "footnote_ref", "footnote_label", "html", "front_matter", "overflow",
               "search_match", "search_current"]

SYNTAX_ORDER = ["plain", "comment", "comment_doc", "punctuation", "operator",
                "keyword", "keyword_control", "label", "attribute",
                "function", "function_macro", "type", "type_builtin", "constructor",
                "namespace", "tag", "property",
                "string", "string_escape", "number", "boolean", "constant",
                "variable", "variable_builtin", "error"]

PALETTES = {}


def theme(slug, name, appearance, upstream, source, blurb, ramp_ends, palette, syntax,
          notes=()):
    PALETTES[slug] = dict(name=name, appearance=appearance, upstream=upstream,
                          source=source, blurb=blurb, ramp=ramp(*ramp_ends),
                          palette=palette, syntax=syntax, notes=list(notes))


# ---------------------------------------------------------------- gruvbox ---
# colors/gruvbox.vim: Statement/Conditional/Repeat/Label/Exception/Keyword ->
# Red; Identifier -> Blue; Function -> GreenBold; PreProc/Include/Define/Macro
# -> Aqua; Constant/Character/Boolean/Number/Float -> Purple; Type/Typedef ->
# Yellow; StorageClass -> Orange; Structure -> Aqua; Operator -> Normal.
GRUVBOX_SYNTAX = {
    "plain": "fg", "comment": "gray", "comment_doc": "gray_light",
    "punctuation": "fg_dim",
    "operator": "fg",             # hi! link Operator Normal — gruvbox leaves these plain
    "keyword": "red", "keyword_control": "red", "label": "red",
    "attribute": "aqua",          # PreProc family
    "function": "green", "function_macro": "aqua",       # Macro -> Aqua
    "type": "yellow", "type_builtin": "yellow", "constructor": "yellow",
    "namespace": "aqua",          # Include -> Aqua
    "tag": "blue", "property": "blue",
    "variable": "blue",           # Identifier -> Blue, which is classic gruvbox
    "variable_builtin": "orange",  # StorageClass -> Orange
    "string": "green", "string_escape": "orange",        # SpecialChar -> Special -> Orange
    "number": "purple", "boolean": "purple", "constant": "purple",
    "error": "red",
}

theme(
    "gruvbox-dark", "Gruvbox Dark", "dark",
    "morhetz/gruvbox", "colors/gruvbox.vim",
    "Retro groove: warm, low-blue, high-contrast. The most-ported terminal\n"
    "palette there is.",
    ("#fe8019", "#fabd2f"),
    dict(fg="#ebdbb2", fg_bright="#fbf1c7", fg_dim="#d5c4a1",
         gray="#928374", gray_light="#a89984",
         red="#fb4934", red_bright="#fb4934",
         green="#b8bb26", green_bright="#b8bb26",
         yellow="#fabd2f", yellow_bright="#fabd2f",
         blue="#83a598", blue_bright="#83a598",
         purple="#d3869b", purple_bright="#d3869b",
         aqua="#8ec07c", aqua_bright="#8ec07c",
         orange="#fe8019", orange_bright="#fe8019"),
    GRUVBOX_SYNTAX,
    notes=["gruvbox dark draws syntax from its `bright_*` tier; `neutral_*` is "
           "what the light variant uses."],
)

theme(
    "gruvbox-light", "Gruvbox Light", "light",
    "morhetz/gruvbox", "colors/gruvbox.vim",
    "The same retro groove inverted for paper.",
    ("#af3a03", "#9d0006"),
    dict(fg="#3c3836", fg_bright="#282828", fg_dim="#504945",
         gray="#7c6f64", gray_light="#665c54",
         red="#9d0006", red_bright=darken("#9d0006", 0.15),
         green="#79740e", green_bright=darken("#79740e", 0.15),
         yellow="#b57614", yellow_bright=darken("#b57614", 0.25),
         blue="#076678", blue_bright=darken("#076678", 0.15),
         purple="#8f3f71", purple_bright=darken("#8f3f71", 0.15),
         aqua="#427b58", aqua_bright=darken("#427b58", 0.15),
         orange="#af3a03", orange_bright=darken("#af3a03", 0.15)),
    GRUVBOX_SYNTAX,
    notes=["on paper louder means closer to the ink, so `*_bright` is a darkened "
           "`faded_*` rather than a `neutral_*` — the neutral tier is tuned for a "
           "dark page and washes out on a white one."],
)

# ------------------------------------------------------------------- nord ---
# nordtheme.com states a usage per colour: nord7 classes/types/primitives,
# nord8 functions and methods, nord9 keywords/operators/tags/punctuation,
# nord11 errors, nord12 annotations and decorators, nord13 escape characters
# and regex, nord14 strings, nord15 numbers, nord4 variables/constants/
# attributes/fields.
theme(
    "nord", "Nord", "dark",
    "arcticicestudio/nord", "nordtheme.com/docs/colors-and-palettes",
    "An arctic, north-bluish palette. Deliberately low-saturation, and the one\n"
    "port here whose keywords are *blue* — Nord spends its warm colours on\n"
    "meaning (errors, annotations, strings) and its cool ones on syntax.",
    ("#88c0d0", "#81a1c1"),
    dict(fg="#d8dee9", fg_bright="#eceff4", fg_dim="#e5e9f0",
         gray="#7b88a1", gray_light="#8f9ab5",
         red="#bf616a", red_bright="#bf616a",
         green="#a3be8c", green_bright="#a3be8c",
         yellow="#ebcb8b", yellow_bright="#ebcb8b",
         blue="#81a1c1", blue_bright="#88c0d0",
         purple="#b48ead", purple_bright="#b48ead",
         aqua="#8fbcbb", aqua_bright="#8fbcbb",
         orange="#d08770", orange_bright="#d08770"),
    {
        "plain": "#d8dee9",             # nord4
        "comment": "#616e88",           # brightened nord3 — see NOTE
        "comment_doc": "#7b88a1",
        "punctuation": "#81a1c1",       # nord9
        "operator": "#81a1c1",          # nord9
        "keyword": "#81a1c1", "keyword_control": "#81a1c1", "label": "#81a1c1",
        "attribute": "#d08770",         # nord12
        "function": "#88c0d0", "function_macro": "#88c0d0",   # nord8
        "type": "#8fbcbb", "type_builtin": "#8fbcbb", "constructor": "#8fbcbb",
        "namespace": "#8fbcbb",         # nord7
        "tag": "#81a1c1",               # nord9
        "property": "#d8dee9",          # nord4, fields
        "variable": "#d8dee9", "variable_builtin": "#d8dee9",  # nord4
        "string": "#a3be8c",            # nord14
        "string_escape": "#ebcb8b",     # nord13
        "number": "#b48ead", "boolean": "#b48ead",             # nord15
        "constant": "#d8dee9",          # nord4
        "error": "#bf616a",             # nord11
    },
    notes=["Nord specifies nord3 (#4c566a) for comments, which is 1.9:1 against "
           "this page and unreadable. This uses #616e88, the brightened comment "
           "every real Nord editor port ships instead."],
)

# ---------------------------------------------------------------- dracula ---
# spec.draculatheme.com: keyword/storage/operator/tag/punctuation-operator ->
# pink; function/method/attribute -> green; class/type -> cyan; string ->
# yellow; number/constant/boolean -> purple; variable -> foreground;
# parameter -> orange; error -> red.
theme(
    "dracula", "Dracula", "dark",
    "dracula/dracula-theme", "spec.draculatheme.com",
    "High-saturation neon on near-black. The loudest port here, and the one\n"
    "that most rewards a truecolor terminal.",
    ("#bd93f9", "#ff79c6"),
    dict(fg="#f8f8f2", fg_bright="#ffffff", fg_dim="#c5c5c0",
         gray="#6272a4", gray_light="#8b9ac4",
         red="#ff5555", red_bright="#ff5555",
         green="#50fa7b", green_bright="#50fa7b",
         yellow="#f1fa8c", yellow_bright="#f1fa8c",
         blue="#8be9fd", blue_bright="#8be9fd",
         purple="#bd93f9", purple_bright="#bd93f9",
         aqua="#8be9fd", aqua_bright="#8be9fd",
         orange="#ffb86c", orange_bright="#ffb86c"),
    {
        "plain": "#f8f8f2",
        "comment": "#6272a4", "comment_doc": "#8b9ac4",
        "punctuation": "#ff79c6",       # the spec puts punctuation operators on pink
        "operator": "#ff79c6",
        "keyword": "#ff79c6", "keyword_control": "#ff79c6", "label": "#ff79c6",
        "attribute": "#50fa7b",
        "function": "#50fa7b", "function_macro": "#50fa7b",
        "type": "#8be9fd", "type_builtin": "#8be9fd", "constructor": "#8be9fd",
        "namespace": "#8be9fd",
        "tag": "#ff79c6",
        "property": "#f8f8f2",
        "variable": "#f8f8f2", "variable_builtin": "#ffb86c",   # parameter orange
        "string": "#f1fa8c", "string_escape": "#ff79c6",
        "number": "#bd93f9", "boolean": "#bd93f9", "constant": "#bd93f9",
        "error": "#ff5555",
    },
)

# ------------------------------------------------------------- catppuccin ---
# groups/syntax.lua: Keyword/Statement/Conditional/Repeat/Exception -> mauve;
# Function -> blue; Type/Structure/StorageClass -> yellow; String -> green;
# Constant/Number/Boolean -> peach; Operator -> sky; Delimiter/Comment ->
# overlay2; Label -> sapphire; PreProc/Special -> pink; Tag -> lavender.
# groups/treesitter.lua: @property -> lavender; @variable -> text;
# @variable.builtin -> red; @module -> yellow; @constructor -> yellow;
# @function.macro -> pink; @string.escape -> pink; @type.builtin -> mauve;
# @attribute -> Constant; @punctuation.bracket -> overlay2.
theme(
    "catppuccin-mocha", "Catppuccin Mocha", "dark",
    "catppuccin/catppuccin", "catppuccin/nvim groups/syntax.lua + treesitter.lua",
    "Pastel on a soft charcoal base. Every accent is held at a similar\n"
    "lightness, which is why its code reads as calm with twelve hues in play.",
    ("#cba6f7", "#89b4fa"),
    dict(fg="#cdd6f4", fg_bright="#f5e0dc", fg_dim="#bac2de",
         gray="#7f849c", gray_light="#9399b2",
         red="#f38ba8", red_bright="#eba0ac",
         green="#a6e3a1", green_bright="#94e2d5",
         yellow="#f9e2af", yellow_bright="#fab387",
         blue="#89b4fa", blue_bright="#74c7ec",
         purple="#cba6f7", purple_bright="#b4befe",
         aqua="#94e2d5", aqua_bright="#89dceb",
         orange="#fab387", orange_bright="#f5c2e7"),
    {
        "plain": "#cdd6f4",             # text
        "comment": "#9399b2", "comment_doc": "#9399b2",         # overlay2
        "punctuation": "#9399b2",       # Delimiter / @punctuation.bracket
        "operator": "#89dceb",          # sky
        "keyword": "#cba6f7", "keyword_control": "#cba6f7",     # mauve
        "label": "#74c7ec",             # sapphire
        "attribute": "#fab387",         # @attribute -> Constant -> peach
        "function": "#89b4fa",          # blue
        "function_macro": "#f5c2e7",    # pink
        "type": "#f9e2af", "constructor": "#f9e2af",            # yellow
        "type_builtin": "#cba6f7",      # mauve
        "namespace": "#f9e2af",         # @module -> yellow
        "tag": "#b4befe",               # lavender
        "property": "#b4befe",          # lavender
        "variable": "#cdd6f4",          # text
        "variable_builtin": "#f38ba8",  # red
        "string": "#a6e3a1",            # green
        "string_escape": "#f5c2e7",     # pink
        "number": "#fab387", "boolean": "#fab387", "constant": "#fab387",  # peach
        "error": "#f38ba8",
    },
)

# ------------------------------------------------------------- tokyonight ---
# groups/base.lua: Keyword -> cyan; Statement/Identifier -> magenta; Function
# -> blue; Type/Special -> blue1; String/Character -> green; Constant ->
# orange; Operator -> blue5; PreProc -> cyan; Comment -> comment.
# groups/treesitter.lua: @keyword -> purple; @keyword.function -> magenta;
# @constructor -> magenta; @property -> green1; @operator -> blue5;
# @punctuation.bracket -> fg_dark; @string.escape -> magenta; @label -> blue;
# @tag -> Label; @type.builtin -> blend_bg(blue1, 0.8); @module -> Include.
theme(
    "tokyo-night", "Tokyo Night", "dark",
    "folke/tokyonight.nvim", "tokyonight.nvim groups/base.lua + treesitter.lua",
    "Neon over deep navy. Worth knowing: stele's dark reference background is\n"
    "#1a1b26, which *is* Tokyo Night's — so this port's contrast numbers are the\n"
    "ones its author intended rather than an approximation.",
    ("#7aa2f7", "#9d7cd8"),
    dict(fg="#c0caf5", fg_bright="#c0caf5", fg_dim="#a9b1d6",
         gray="#565f89", gray_light="#737aa2",
         red="#f7768e", red_bright="#f7768e",
         green="#9ece6a", green_bright="#9ece6a",
         yellow="#e0af68", yellow_bright="#e0af68",
         blue="#7aa2f7", blue_bright="#2ac3de",
         purple="#bb9af7", purple_bright="#9d7cd8",
         aqua="#7dcfff", aqua_bright="#89ddff",
         orange="#ff9e64", orange_bright="#ff9e64"),
    {
        "plain": "#c0caf5",             # fg
        "comment": "#565f89", "comment_doc": "#737aa2",
        "punctuation": "#a9b1d6",       # fg_dark, @punctuation.bracket
        "operator": "#89ddff",          # blue5
        "keyword": "#9d7cd8",           # @keyword -> purple
        "keyword_control": "#bb9af7",   # Statement/Conditional -> magenta
        "label": "#7aa2f7",             # @label -> blue
        "attribute": "#7dcfff",         # @attribute -> PreProc -> cyan
        "function": "#7aa2f7",          # Function -> blue
        "function_macro": "#7dcfff",    # Macro -> PreProc -> cyan
        "type": "#2ac3de",              # Type -> blue1
        "type_builtin": blend_bg("#2ac3de", 0.8),
        "constructor": "#bb9af7",       # @constructor -> magenta
        "namespace": "#7dcfff",         # @module -> Include -> PreProc -> cyan
        "tag": "#7aa2f7",               # @tag -> Label -> blue
        "property": "#73daca",          # green1
        "variable": "#c0caf5",
        "variable_builtin": "#f7768e",  # red, like `self` / `this`
        "string": "#9ece6a", "string_escape": "#bb9af7",
        "number": "#ff9e64", "boolean": "#ff9e64", "constant": "#ff9e64",
        "error": "#f7768e",
    },
)


def render(slug, spec):
    p = spec["palette"]
    colors = {r: p[s] for r, s in COLORS.items()}
    for i, stop in enumerate(spec["ramp"], start=1):
        colors[f"heading{i}"] = stop
    # A syntax value is either a literal hex or a name in this theme's palette.
    syntax = {r: (v if v.startswith("#") else p[v]) for r, v in spec["syntax"].items()}
    missing = set(SYNTAX_ORDER) - set(syntax)
    assert not missing, f"{slug} is missing syntax roles: {sorted(missing)}"

    bg = DARK_BG if spec["appearance"] == "dark" else LIGHT_BG
    bad = []
    for role, hx in colors.items():
        floor = AA_NONTEXT if role in STRUCTURAL else AA_TEXT
        r = ratio(rgb(hx), bg)
        if r < floor:
            bad.append((role, hx, r, floor))
    for role, hx in syntax.items():
        r = ratio(rgb(hx), bg)
        if r < AA_TEXT:
            bad.append((f"syntax.{role}", hx, r, AA_TEXT))

    out = [f"# {spec['name']} — a port of {spec['upstream']}.", "#"]
    out += ["# " + line for line in spec["blurb"].split("\n")]
    out += [
        "#",
        "# The [syntax] table is mapped from the upstream project's own scope",
        f"# definitions ({spec['source']}), not from a house convention. These themes",
        "# disagree with each other about what colour a keyword is far more than you",
        "# would expect, and flattening that would make six ports that were the same",
        "# picture in different paint.",
        "#",
        "# The [colors] table is not a port. Markdown roles — headings, alerts, a",
        "# blockquote gutter — have no upstream equivalent to be faithful to, so they",
        "# follow one convention across every theme here, drawn from this palette. The",
        "# six heading rungs are interpolated between two of the theme's own colours:",
        "# no editor defines a six-level ramp, and six unrelated accents would break",
        "# the one thing the ramp is for. See docs/theming.md.",
    ]
    for note in spec["notes"]:
        out += ["#", "# NOTE: " + note]
    out += [
        "#",
        "# Contrast is measured against stele's reference background rather than the",
        "# upstream theme's own, because stele does not set your terminal background.",
    ]
    if bad:
        out += [
            "#",
            "# This port is faithful rather than corrected, so it inherits the roles",
            "# upstream ships below WCAG AA. stele says so in the status line every",
            "# time you load it. Brightening them would make this a theme that merely",
            f"# resembles {spec['name']} rather than one that is it. They are listed",
            "# here so you can edit them if you would rather have the contrast.",
            "#",
            "# under-aa: " + ", ".join(sorted(r for r, _, _, _ in bad)),
        ]
    else:
        out += ["#", "# Every role here clears WCAG AA against that reference.",
                "#", "# under-aa:"]
    out += [
        f'# upstream: {spec["upstream"]}',
        f'name = "{spec["name"]}"',
        f'appearance = "{spec["appearance"]}"',
        "",
        "[colors]",
    ]
    out += [f'{r} = "{colors[r]}"' for r in COLOR_ORDER]
    out += ["", "# Naming any of these hands the theme the whole code block — see",
            "# docs/theming.md.", "[syntax]"]
    out += [f'{r} = "{syntax[r]}"' for r in SYNTAX_ORDER]
    return "\n".join(out) + "\n", bad


if __name__ == "__main__":
    root = pathlib.Path(__file__).parent
    shapes = {}
    for slug, spec in PALETTES.items():
        body, bad = render(slug, spec)
        (root / f"{slug}.toml").write_text(body)
        status = "ok" if not bad else f"{len(bad)} under floor"
        print(f"{slug:20} {status}")
        for role, hx, r, floor in bad:
            print(f"    {role:24} {hx}  {r:.2f}:1  (needs {floor})")
        # Which roles share a colour — the *shape* of the mapping, independent
        # of the hues. Two upstreams landing on one shape means somebody reused
        # a convention instead of reading the source.
        groups = {}
        for role, value in spec["syntax"].items():
            groups.setdefault(value, set()).add(role)
        key = frozenset(frozenset(g) for g in groups.values())
        shapes.setdefault(key, set()).add(spec["upstream"])

    upstreams = {s["upstream"] for s in PALETTES.values()}
    print(f"\ndistinct syntax shapes: {len(shapes)} across {len(upstreams)} upstreams")
    for ups in shapes.values():
        if len(ups) > 1:
            print("  SHARED SHAPE across upstreams:", sorted(ups))
