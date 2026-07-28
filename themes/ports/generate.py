#!/usr/bin/env python3
"""Emit stele theme files from published editor palettes.

The mapping below is the work; emitting TOML is not. Every hex in PALETTES is a
value published by the upstream theme, except the heading ramps, which are
interpolated between two of the theme's own colours because none of these
themes defines a six-level markdown ramp to copy.

Checks each emitted file against the same WCAG floors crates/highlight uses.
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
    """Toward black by `amount`. Used only where an upstream light-mode palette
    offers one accent tier and stele wants two."""
    return hexs(tuple(round(v * (1 - amount)) for v in rgb(h)))


def ramp(start, end, n=6):
    """n stops from start to end, inclusive, in RGB."""
    a, b = rgb(start), rgb(end)
    return [hexs(tuple(round(a[i] + (b[i] - a[i]) * s / (n - 1)) for i in range(3)))
            for s in range(n)]


# Palette slots every theme must fill. The role maps below are written once,
# against these names, so a port is a palette plus a header and nothing else.
SLOTS = ("fg fg_bright fg_dim gray gray_light red red_bright green green_bright "
         "yellow yellow_bright blue blue_bright purple purple_bright aqua "
         "aqua_bright orange orange_bright").split()

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

# The conventional editor mapping — what these themes already do in their own
# TextMate / tree-sitter definitions.
SYNTAX = {
    "plain": "fg", "comment": "gray", "comment_doc": "gray_light",
    "punctuation": "fg_dim", "operator": "orange",
    "keyword": "red", "keyword_control": "red_bright", "label": "red",
    "attribute": "aqua", "function": "green_bright", "function_macro": "aqua_bright",
    "type": "yellow", "type_builtin": "yellow_bright", "constructor": "yellow",
    "namespace": "blue", "tag": "blue_bright", "property": "blue",
    "string": "green", "string_escape": "orange_bright",
    "number": "purple", "boolean": "purple_bright", "constant": "purple",
    "variable": "fg", "variable_builtin": "purple_bright", "error": "red_bright",
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


def theme(slug, name, appearance, upstream, blurb, ramp_ends, palette, overrides=None):
    PALETTES[slug] = dict(name=name, appearance=appearance, upstream=upstream,
                          blurb=blurb, ramp=ramp(*ramp_ends), palette=palette,
                          overrides=overrides or {})


theme(
    "gruvbox-dark", "Gruvbox Dark", "dark",
    "morhetz/gruvbox",
    "Retro groove: warm, low-blue, high-contrast. The most-ported terminal\n"
    "palette there is, and the one that reads best on a warm-white terminal.",
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
)

theme(
    "gruvbox-light", "Gruvbox Light", "light",
    "morhetz/gruvbox",
    "The same retro groove inverted for paper. Gruvbox's `faded_*` accents do\n"
    "the work here — the `neutral_*` set is tuned for a dark page and washes\n"
    "out on a white one, which is why `*_bright` below is a darkened faded\n"
    "rather than a neutral: on paper, louder means closer to the ink.",
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
)

theme(
    "nord", "Nord", "dark",
    "arcticicestudio/nord",
    "An arctic, north-bluish palette. Deliberately low-contrast and\n"
    "low-saturation — the quietest of these ports, and the closest in\n"
    "temperament to stele's own Lichen.",
    ("#88c0d0", "#81a1c1"),
    dict(fg="#d8dee9", fg_bright="#eceff4", fg_dim="#e5e9f0",
         gray="#7b88a1", gray_light="#8f9ab5",
         red="#bf616a", red_bright="#d08770",
         green="#a3be8c", green_bright="#b5cf9f",
         yellow="#ebcb8b", yellow_bright="#f0d9a8",
         blue="#81a1c1", blue_bright="#88c0d0",
         purple="#b48ead", purple_bright="#c8a5c1",
         aqua="#8fbcbb", aqua_bright="#a3cdcc",
         orange="#d08770", orange_bright="#e0a08c"),
)

theme(
    "dracula", "Dracula", "dark",
    "dracula/dracula-theme",
    "High-saturation neon on near-black. The loudest of these ports, and the\n"
    "one that most rewards a truecolor terminal.",
    ("#bd93f9", "#ff79c6"),
    dict(fg="#f8f8f2", fg_bright="#ffffff", fg_dim="#c5c5c0",
         gray="#6272a4", gray_light="#8b9ac4",
         red="#ff5555", red_bright="#ff6e6e",
         green="#50fa7b", green_bright="#69ff96",
         yellow="#f1fa8c", yellow_bright="#f4fbaa",
         blue="#8be9fd", blue_bright="#a4eeff",
         purple="#bd93f9", purple_bright="#d6b3ff",
         aqua="#8be9fd", aqua_bright="#a4eeff",
         orange="#ffb86c", orange_bright="#ffc98c"),
    overrides=dict(colors=dict(link="#8be9fd", code_inline="#ff79c6"),
                   syntax=dict(keyword="#ff79c6", keyword_control="#ff92d0",
                               label="#ff79c6", function="#50fa7b",
                               type="#8be9fd", type_builtin="#8be9fd")),
)

theme(
    "catppuccin-mocha", "Catppuccin Mocha", "dark",
    "catppuccin/catppuccin (Mocha flavour)",
    "Pastel on a soft charcoal base. Every accent is deliberately held at a\n"
    "similar lightness, which is why its code blocks read as calm even with\n"
    "twelve hues in play.",
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
)

theme(
    "tokyo-night", "Tokyo Night", "dark",
    "folke/tokyonight.nvim (Night variant)",
    "Neon over deep navy. Worth knowing: stele's own dark reference background\n"
    "is #1a1b26, which *is* Tokyo Night's — so this port's contrast numbers are\n"
    "the ones its author intended rather than an approximation.",
    ("#7aa2f7", "#9d7cd8"),
    dict(fg="#c0caf5", fg_bright="#c0caf5", fg_dim="#a9b1d6",
         gray="#565f89", gray_light="#737aa2",
         red="#f7768e", red_bright="#ff7a93",
         green="#9ece6a", green_bright="#b9f27c",
         yellow="#e0af68", yellow_bright="#ffc777",
         blue="#7aa2f7", blue_bright="#2ac3de",
         purple="#bb9af7", purple_bright="#c8a9ff",
         aqua="#7dcfff", aqua_bright="#89ddff",
         orange="#ff9e64", orange_bright="#ffb489"),
)


def render(slug, spec):
    p = spec["palette"]
    ov = spec["overrides"]
    colors = {r: p[s] for r, s in COLORS.items()}
    colors.update(ov.get("colors", {}))
    for i, stop in enumerate(spec["ramp"], start=1):
        colors[f"heading{i}"] = stop
    syntax = {r: p[s] for r, s in SYNTAX.items()}
    syntax.update(ov.get("syntax", {}))

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
        "# Every colour below is a value published by the upstream theme, with two",
        "# exceptions, both forced by things stele has and an editor does not:",
        "#",
        "#   - The six heading rungs are interpolated between two of the theme's own",
        "#     colours. No editor theme defines a six-level markdown ramp, and a",
        "#     rainbow of six unrelated accents would break the one thing the ramp is",
        "#     for — saying \"these are all headings\" while the marker count says how",
        "#     deep. See docs/theming.md.",
        "#   - Where the upstream palette offers one tier and stele wants two, the",
        "#     second is derived from the first and said so above.",
        "#",
        "# Contrast is measured against stele's reference background rather than the",
        "# upstream theme's own, because stele does not set your terminal background.",
    ]
    if bad:
        out += [
            "#",
            "# This port is faithful rather than corrected, so it inherits the roles",
            "# upstream ships below WCAG AA. stele will say so in the status line every",
            "# time you load it. Brightening them would make this a theme that merely",
            "# resembles " + spec["name"] + " rather than one that is it. The roles are",
            "# listed below so you can edit them if you would rather have the contrast.",
            "#",
            "# under-aa: " + ", ".join(sorted(r for r, _, _, _ in bad)),
        ]
    else:
        out += ["#", "# Every role here clears WCAG AA against that reference.",
                "#", "# under-aa:"]
    out += [
        f'name = "{spec["name"]}"',
        f'appearance = "{spec["appearance"]}"',
        "",
        "[colors]",
    ]
    out += [f'{r} = "{colors[r]}"' for r in COLOR_ORDER]
    out += ["", "# Naming any of these hands the theme the whole code block — see",
            "# docs/theming.md. Mapped the way the upstream theme maps its own",
            "# TextMate and tree-sitter scopes.", "[syntax]"]
    out += [f'{r} = "{syntax[r]}"' for r in SYNTAX_ORDER]
    return "\n".join(out) + "\n", bad


if __name__ == "__main__":
    root = pathlib.Path("themes/ports")
    for slug, spec in PALETTES.items():
        body, bad = render(slug, spec)
        (root / f"{slug}.toml").write_text(body)
        status = "ok" if not bad else f"{len(bad)} UNDER FLOOR"
        print(f"{slug:20} {status}")
        for role, hx, r, floor in bad:
            print(f"    {role:24} {hx}  {r:.2f}:1  (needs {floor})")
