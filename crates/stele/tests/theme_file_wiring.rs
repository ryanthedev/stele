//! A user theme has to change what reaches the terminal, and no theme at all
//! has to change nothing.
//!
//! Both claims are asserted on the real painted bytes rather than on a
//! resolved `Style`. The unit tests in `stele::theme_source` already prove
//! the right `Theme` comes back; what they cannot prove is that the theme
//! reaches the wire, which is the only place a reader ever sees it. A
//! regression that resolved the user's colour perfectly and then painted the
//! built-in would pass every one of those tests and fail here.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use ast::Document;
use highlight::{ColorMode, Variant};
use layout::{LayoutConfig, NullSizer, layout};
use stele::decor::themed::ThemedDecor;
use stele::painter::{Painter, Size};
use stele::theme_source::ThemeSource;

use common::fixtures::engine;

/// A document exercising the roles the themes below actually set.
const DOC: &str = "# Title\n\nBody prose with a [link](https://example.com).\n";

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("stele-theme-wiring-{tag}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// A document with all three kinds of code in it: an inline span, a fence the
/// highlighter claims, and a fence it does not.
const CODE_DOC: &str = "Prose with `a span`.\n\n```rust\nfn main() { let x = 1; }\n```\n\n```\nno language here\n```\n";

/// Paints one frame of `markdown` through a real `Painter` and returns the
/// wire bytes.
fn paint_doc(source: &ThemeSource, markdown: &str, height: u16) -> String {
    let doc = Document::parse(markdown);
    let engine = engine();
    let config = LayoutConfig {
        min_width: 24,
        max_width: 80,
    };
    let tree = layout(&doc, 80, &config, &engine, &NullSizer);

    let mut painter = Painter::new(engine);
    painter.register_decor(Box::new(ThemedDecor::new(source.theme())));

    let mut frame = Vec::new();
    painter
        .frame(&tree, 0, Size { width: 80, height }, &mut frame)
        .expect("paint");
    String::from_utf8_lossy(&frame).into_owned()
}

fn paint_with(source: &ThemeSource) -> String {
    paint_doc(source, DOC, 12)
}

/// The SGR foreground the Rust fence's `fn` keyword is actually painted in.
///
/// Found by position rather than by searching for a colour, because the point
/// of every caller is to discover which colour won — a search would only ever
/// confirm the colour it was handed.
fn keyword_colour(wire: &str) -> &str {
    let keyword_at = wire.find("fn").expect("the rust block is painted");
    let before = &wire[..keyword_at];
    let last_fg = before.rfind("38;2;").expect("the keyword carries a colour");
    let tail = &before[last_fg..];
    let end = tail.find('m').unwrap_or(tail.len());
    &tail[..end]
}

fn write_theme(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("theme.toml");
    fs::write(&path, body).expect("write theme");
    path
}

/// DW-3.1: the colour in the file is the colour on the wire.
///
/// `text` is the role under test on purpose — it is the one with no palette
/// slot, so before theme files there was no way for *any* colour to reach the
/// wire for body prose. Finding this SGR proves the override path end to end.
#[test]
fn test_a_theme_files_colour_reaches_the_wire() {
    let dir = scratch("applied");
    let path = write_theme(
        &dir,
        "name = \"Wire\"\nappearance = \"dark\"\n\n[colors]\ntext = \"#a1b2c3\"\n",
    );
    let source = ThemeSource::load(Some(&path), None, Variant::Dark, ColorMode::Truecolor)
        .expect("theme loads");
    let wire = paint_with(&source);

    assert!(
        wire.contains("38;2;161;178;195"),
        "the themed body colour never reached the terminal: {wire:?}"
    );
}

/// DW-3.2: with nothing configured, every byte is what it was before theme
/// files existed. This is the promise to everyone who never writes a theme,
/// and it is a byte-for-byte claim rather than a spot check because "the
/// colours look the same" is exactly how a regression here would hide.
#[test]
fn test_no_theme_paints_exactly_what_the_built_in_paints() {
    let missing = scratch("absent").join("nope.toml");
    let configured = ThemeSource::load(None, Some(&missing), Variant::Dark, ColorMode::Truecolor)
        .expect("an absent config is not an error");
    let built_in = ThemeSource::built_in(Variant::Dark, ColorMode::Truecolor);

    assert_eq!(
        paint_with(&configured),
        paint_with(&built_in),
        "an absent theme file changed the painted frame"
    );
}

/// A theme that sets nothing must also paint the built-in exactly. The
/// override map is empty, so this is the seam's inertness proven through the
/// painter rather than through `Theme::resolve`.
#[test]
fn test_an_empty_theme_paints_exactly_what_the_built_in_paints() {
    let dir = scratch("empty");
    let path = write_theme(
        &dir,
        "name = \"Empty\"\nappearance = \"dark\"\n\n[colors]\n",
    );
    let source = ThemeSource::load(Some(&path), None, Variant::Dark, ColorMode::Truecolor)
        .expect("theme loads");
    let built_in = ThemeSource::built_in(Variant::Dark, ColorMode::Truecolor);

    assert_eq!(
        paint_with(&source),
        paint_with(&built_in),
        "a theme setting no colours changed the painted frame"
    );
}

/// DW-3.5: `NO_COLOR` with a theme file emits no colour at all. A theme is a
/// set of colours and a file must not be a way around asking for none — this
/// checks the wire, where it counts, not the resolved style.
#[test]
fn test_no_color_with_a_theme_file_puts_no_colour_on_the_wire() {
    let dir = scratch("nocolor");
    let path = write_theme(
        &dir,
        "appearance = \"dark\"\n\n[colors]\ntext = \"#a1b2c3\"\nheading1 = \"#ff0000\"\n",
    );
    let source = ThemeSource::load(Some(&path), None, Variant::Dark, ColorMode::NoColor)
        .expect("theme loads");
    let wire = paint_with(&source);

    assert!(
        !wire.contains("38;2;"),
        "NO_COLOR emitted a truecolor foreground: {wire:?}"
    );
    assert!(
        !wire.contains("48;2;"),
        "NO_COLOR emitted a truecolor background: {wire:?}"
    );
}

/// Theming a heading must move its depth markers and its ember rule with it.
/// The markers paint through `HeadingRung`, which a theme cannot name — so
/// this is the one place the `canonical_role` fold is visible from outside
/// `highlight`, and a regression there would show up as a title in the user's
/// colour with markers still in the built-in's.
#[test]
fn test_theming_a_heading_moves_its_markers_on_the_wire() {
    let dir = scratch("markers");
    let path = write_theme(
        &dir,
        "appearance = \"dark\"\n\n[colors]\nheading1 = \"#ff8800\"\n",
    );
    let source = ThemeSource::load(Some(&path), None, Variant::Dark, ColorMode::Truecolor)
        .expect("theme loads");
    let wire = paint_with(&source);

    let themed = "38;2;255;136;0";
    assert!(
        wire.contains(themed),
        "the themed heading colour never reached the wire: {wire:?}"
    );
    // The marker is emitted before the title text, so the themed colour must
    // appear at least twice: once for the `▌` and once for the words.
    assert!(
        wire.matches(themed).count() >= 2,
        "the heading's marker did not follow its title into the theme colour: {wire:?}"
    );
}

/// Where `[colors]` stops: the edge of the highlighter.
///
/// A fenced block with a known language is re-tagged into `StyleId::Capture`
/// runs by the highlighter, and `resolve_capture` does not consult the
/// *semantic* overlay at all. So `code_block` reaches only code the
/// highlighter did not claim, and `code_inline` reaches every inline span
/// because those are never highlighted. A `[syntax]` table is the only way
/// into a claimed block — this theme has none, so the keyword stays on the
/// generated palette.
#[test]
fn test_colors_reaches_unhighlighted_code_but_never_a_syntax_capture() {
    let dir = scratch("captures");
    let path = write_theme(
        &dir,
        "appearance = \"dark\"\n\n[colors]\ncode_block = \"#ff00ff\"\ncode_inline = \"#00ff00\"\n",
    );
    let source = ThemeSource::load(Some(&path), None, Variant::Dark, ColorMode::Truecolor)
        .expect("theme loads");
    let wire = paint_doc(&source, CODE_DOC, 20);

    assert!(
        wire.contains("38;2;0;255;0"),
        "an inline code span is a Semantic role and must take the theme: {wire:?}"
    );
    assert!(
        wire.contains("38;2;255;0;255"),
        "a fenced block with no language stays Semantic::CodeBlock and must \
         take the theme: {wire:?}"
    );
    assert_ne!(
        keyword_colour(&wire),
        "38;2;255;0;255",
        "a syntax capture took the theme's code_block colour — `[colors]` \
         does not reach inside a highlighted block"
    );
}

/// And where `[syntax]` starts. The same `fn` keyword, the same document, one
/// extra table in the file — this is the whole feature, asserted on the bytes
/// the terminal receives rather than on a resolved `Style`.
#[test]
fn test_a_syntax_tables_colour_reaches_a_highlighted_keyword() {
    let dir = scratch("syntax-keyword");
    let path = write_theme(
        &dir,
        "appearance = \"dark\"\n\n[colors]\ntext = \"#c8d0e0\"\n\n[syntax]\nkeyword = \"#ff00aa\"\n",
    );
    let source = ThemeSource::load(Some(&path), None, Variant::Dark, ColorMode::Truecolor)
        .expect("theme loads");
    let wire = paint_doc(&source, CODE_DOC, 20);

    assert_eq!(
        keyword_colour(&wire),
        "38;2;255;0;170",
        "the themed keyword colour never reached the wire"
    );
}

/// The fallback on the wire: `main` is a `Function` capture this theme did not
/// name, so it takes `text` — not the golden-angle colour it wears on an
/// unthemed build, and not the keyword's colour either.
#[test]
fn test_an_unnamed_capture_paints_in_text_once_syntax_is_themed() {
    let dir = scratch("syntax-fallback");
    let path = write_theme(
        &dir,
        "appearance = \"dark\"\n\n[colors]\ntext = \"#c8d0e0\"\n\n[syntax]\nkeyword = \"#ff00aa\"\n",
    );
    let source = ThemeSource::load(Some(&path), None, Variant::Dark, ColorMode::Truecolor)
        .expect("theme loads");
    let wire = paint_doc(&source, CODE_DOC, 20);

    let built_in = ThemeSource::built_in(Variant::Dark, ColorMode::Truecolor);
    let built_in_wire = paint_doc(&built_in, CODE_DOC, 20);
    let generated = keyword_colour(&built_in_wire);

    let main_at = wire.find("main").expect("the rust block is painted");
    let before = &wire[..main_at];
    let last_fg = before
        .rfind("38;2;")
        .expect("the function carries a colour");
    assert!(
        before[last_fg..].starts_with("38;2;200;208;224"),
        "an unnamed capture must fall back to `text`, not the palette: {:?}",
        &before[last_fg..]
    );
    assert_ne!(
        keyword_colour(&wire),
        generated,
        "the themed block is still wearing generated colours"
    );
}

/// A theme with no `[syntax]` table paints a highlighted block byte-identically
/// to the built-in, however much of the page it repaints. The promise to every
/// theme that already exists.
#[test]
fn test_a_colors_only_theme_leaves_a_highlighted_block_untouched() {
    let dir = scratch("syntax-absent");
    let path = write_theme(
        &dir,
        "appearance = \"dark\"\n\n[colors]\ntext = \"#c8d0e0\"\nheading1 = \"#ff8800\"\n",
    );
    let source = ThemeSource::load(Some(&path), None, Variant::Dark, ColorMode::Truecolor)
        .expect("theme loads");
    let built_in = ThemeSource::built_in(Variant::Dark, ColorMode::Truecolor);

    assert_eq!(
        keyword_colour(&paint_doc(&source, CODE_DOC, 20)),
        keyword_colour(&paint_doc(&built_in, CODE_DOC, 20)),
        "a theme with no [syntax] table repainted a syntax capture"
    );
}
