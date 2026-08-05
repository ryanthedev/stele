# stele

A terminal markdown viewer for [Ghostty](https://ghostty.org).

Markdown in a pager is usually a compromise: the images become alt text, the
diagrams become a wall of arrow syntax, the math becomes TeX source. stele
renders them, as actual pixels, in the terminal — and when it can't, it falls
back to text on purpose rather than by accident.

```
stele README.md
```

## What it renders

| | |
|---|---|
| **Markdown** | CommonMark 0.31.2 plus GFM — tables, task lists, strikethrough, autolinks |
| **Code** | Syntax highlighting, with a copy-to-clipboard key |
| **Images** | PNG, JPEG, GIF, WebP, BMP, ICO, TIFF and SVG, scaled to the cell grid |
| **Math** | `$…$` and `$$…$$` via RaTeX, degrading to a Unicode text grid and then to the TeX source |
| **Diagrams** | Mermaid fences, as Unicode box drawing |
| **Themes** | One TOML file, no installer — see [docs/theming.md](docs/theming.md) |
| **The page** | Line numbers, margins and a highlighted reading line, all set in that same file |

## Requirements

Graphics need **Ghostty**. stele checks `TERM_PROGRAM=ghostty` and turns
images off anywhere else, including under `tmux`, which does not pass the
kitty graphics protocol through. That is deliberate: streaming megabytes of
base64 at a terminal that cannot decode it is worse than showing alt text.

Everything else — text, tables, highlighting, folding, search, links — works
in any terminal. In a non-Ghostty one you get a working viewer with alt text
where the pictures would be.

## Install

### Prebuilt binaries

Download the tarball for your platform from the
[latest release](https://github.com/ryanthedev/stele/releases/latest), then:

```sh
tar xzf stele-*.tar.gz
sudo install stele-*/stele /usr/local/bin/
```

macOS ships binaries unsigned, so the first run may need
`xattr -d com.apple.quarantine /usr/local/bin/stele`.

### From source

```sh
cargo install --git https://github.com/ryanthedev/stele stele
```

Needs Rust **1.95.0** or newer — the workspace is edition 2024 and pins the
toolchain in `rust-toolchain.toml`. `cargo install` will use your default
toolchain, so upgrade with `rustup update` first if it is older.

## Usage

```
stele <file.md>          open a file
stele -                  read the document from stdin
```

| Flag | Effect |
|---|---|
| `--watch` | Reload on change, keeping the scroll anchor. Rejected with `-`. |
| `--theme <FILE>` | Use this theme instead of the built-in colors |
| `--max-width <N>` | Clamp content width to N cells (default 100) |
| `--no-images` | Alt text and TeX source instead of graphics |
| `--frontmatter` | Show YAML frontmatter as content rather than hiding it |
| `--no-rewrite` | Read the file as written: no d2l, Quarto or CodeCogs rewriting |
| `--fetch-remote` | Download `![alt](https://…)` images and draw them. **Off by default** — see [Remote images](#remote-images). |

Save a theme at `~/.config/stele/theme.toml` and it is picked up every run.
`$XDG_CONFIG_HOME` is honoured.

## Remote images

An image whose destination is a URL renders as its alt text, and stele makes
**no network request at all** unless you pass `--fetch-remote`.

That default is a stance, not caution. A markdown file whose images are URLs
is a file that tells whoever wrote it who read it, when, and from what address
— and opening a document is not consent to be counted. So the network stays
shut until you say otherwise, per run, on the command line.

When you do say otherwise:

| | |
|---|---|
| **Schemes** | `http` and `https` only. Checked on the URL *and* on every redirect hop, so a link that bounces to `file:///etc/passwd` is refused at the bounce. |
| **Timeouts** | 5 s to connect, 10 s per request, and a 20 s budget for the whole document — so a page of forty dead images costs about half a minute once, not ten seconds forty times. (The budget is checked between requests, so the true ceiling is 20 s + one request: 30 s.) |
| **Size** | A response is capped at the same ceiling `gfx` already applies to a decoded image, and the bytes are validated by the same decoder a local file goes through. |
| **Redirects** | Four hops, then the image gives up. |
| **Cache** | `$XDG_CACHE_HOME/stele` (or `$HOME/.cache/stele`), keyed by a hash of the URL, capped at 128 MiB and evicted least-recently-used first. A second read of the same document fetches nothing. |
| **Failure** | Always alt text. A timeout, a 404, an oversized body, a redirect loop or a file that is not an image leaves the picture as the words the author wrote, and never stops the document opening. |

Nothing downstream knows a network exists: the fetch happens before the parse
and hands the rest of the viewer an ordinary local file, so a downloaded PNG
or SVG is drawn by exactly the code that draws one off your disk.

`--fetch-remote` and `--no-rewrite` **compose**. `--no-rewrite` is about what
the document says — it stops the d2l, Quarto and CodeCogs passes from
reshaping the source — while `--fetch-remote` is about whether its pictures
are drawn. Typing both gets you both. (One consequence: without the CodeCogs
rewrite, a `latex.codecogs.com` equation stays an image, so it is fetched and
drawn as a picture rather than typeset as maths.)

If you would rather ship a binary that *cannot* make a request, build with
`cargo build --release --no-default-features`: that compiles out the HTTP
client, the TLS stack and the flag itself.

## Keys

| Key | Action |
|---|---|
| `q`, `Ctrl-C` | Quit |
| `j` / `k`, `↓` / `↑` | Move the reading line |
| `Ctrl-D` / `Ctrl-U` | Half page |
| `Ctrl-F` / `Ctrl-B`, `PgDn` / `PgUp` | Full page |
| `g` / `G`, `Home` / `End` | First / last line |
| `[[` / `]]` | Previous / next heading |
| `/`, then `n` / `N` | Search, next / previous match |
| `t` | Table of contents; `Enter` to jump |
| `Tab` / `Shift-Tab` | Cycle links; `Enter` to follow |
| `Backspace` | Back to the previous document |
| `y` | Copy the code block at the cursor |
| `z` | Fold the heading at the cursor |
| `R` / `M` | Expand / collapse all folds |
| `T` | Toggle theme |
| `#` | Toggle line numbers |
| `m` | Toggle mouse capture |
| `Ctrl-G` | File info |

## Building

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

The workspace is nine crates: `ast` (parser), `layout`, `width`, `highlight`,
`gfx` (image and SVG decoding), `math`, `mermaid`, `probe` (a Ghostty
capability harness) and `stele` (the binary).

The one network dependency is [`ureq`](https://crates.io/crates/ureq) over
rustls, behind the default-on `remote-images` feature; it is what
`--fetch-remote` uses and it costs 17 crates. `--no-default-features` builds
without it.

Some findings about Ghostty's terminal behaviour cannot be reproduced in CI —
they were measured against a live session and committed as artifacts under
`docs/spikes/`. CI asserts they exist rather than re-deriving them.

## Security

stele renders untrusted documents, so the decode path is bounded on purpose:
byte caps, a node cap, a wall-clock cap, and a guard that refuses SVG
documents declaring their own XML entities. The network is bounded the same
way and is additionally *off* until asked — see
[Remote images](#remote-images). The reasoning behind each is in
the module docs — `crates/gfx/src/svg.rs` and `crates/gfx/src/decode.rs` — and
where a bound belongs to a dependency rather than to stele, that is said
explicitly.

Found a hole? Open an issue.

## License

MIT — see [LICENSE](LICENSE).
