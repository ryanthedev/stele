//! The `stele` binary: parse the CLI, load the document, enter the terminal,
//! and run the scroll/resize/paint event loop. All decision logic lives in the
//! library (`app`, `painter`, `loader`, `terminal`, `media`, `decor`); this
//! file is thin glue over real crossterm I/O and is not itself unit-tested —
//! it is covered black-box, through the real binary, by
//! `tests/cli_errors.rs`, `tests/quit_restore.rs` and `tests/signal_restore.rs`.

use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Duration;

use ast::Document;
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use layout::{LayoutConfig, layout};
use width::{WidthConfig, WidthEngine};

use stele::app::{AppState, FileInfo, LayoutContext};
use stele::cli::Cli;
use stele::decor::themed::ThemedDecor;
use stele::loader;
use stele::media::{GfxMediaSink, ImageSizer, NoopMediaSink};
use stele::painter::{Painter, Size};
use stele::terminal::{CellQuery, PanicGuardedWriter, TerminalGuard, install_panic_hook};

/// How long a resize burst is drained for before committing to one
/// relayout — long enough to coalesce a storm of SIGWINCH-driven `Resize`
/// events, short enough that a single genuine resize still repaints
/// promptly.
const RESIZE_DEBOUNCE: Duration = Duration::from_millis(50);

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Load and validate the file *before* touching the terminal at all: a
    // bad path should fail cleanly without ever entering raw mode / the
    // alternate screen.
    let source = match loader::load_document(&cli.file) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("stele: {err}");
            return ExitCode::FAILURE;
        }
    };

    // Captured from the raw file, before preprocessing touches it (DW-1.3):
    // `Ctrl-G`'s byte size and line count describe the file on disk, not the
    // frontmatter-stripped / mermaid-rendered text the layout engine sees.
    let file_info = FileInfo {
        name: cli.file.display().to_string(),
        byte_size: source.len() as u64,
        line_count: source.lines().count(),
    };

    // Source preprocessing before parse: hide a leading frontmatter block
    // unless --frontmatter, then render top-level mermaid fences to grids.
    let source = stele::decor::frontmatter::apply(&source, cli.frontmatter).into_owned();
    let source = stele::decor::mermaid::preprocess(&source).into_owned();

    let doc = Document::parse(&source);
    let engine = WidthEngine::new(WidthConfig::default());

    // Relative image paths resolve against the document's own directory.
    let base_dir = cli
        .file
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    // Graphics are off under tmux (which does not pass kitty sequences
    // through), when the user asks, and on any terminal that isn't Ghostty —
    // stele targets Ghostty only, and streaming megabytes of base64 APC at a
    // terminal that cannot decode it is worse than showing alt text. A
    // disabled sizer reserves no boxes at all, so layout falls through to the
    // alt-text path and the media sink is never invoked — the structural
    // guarantee behind DW-6.4.
    let is_ghostty = std::env::var("TERM_PROGRAM").is_ok_and(|t| t == "ghostty");
    let graphics_disabled = cli.no_images || std::env::var_os("TMUX").is_some() || !is_ghostty;

    let config = LayoutConfig {
        min_width: 24,
        max_width: cli.max_width.unwrap_or(100),
    };

    let (cols, rows) = match crossterm::terminal::size() {
        Ok(dims) => dims,
        Err(err) => {
            eprintln!("stele: could not read terminal size: {err}");
            return ExitCode::FAILURE;
        }
    };
    // One row reserved for the status line (DW-1.1): `size` here is the
    // *content* viewport `AppState`/`layout` see, always one row shorter
    // than the real terminal. `Painter::frame_with_status` paints the
    // status row immediately below it.
    let size = Size {
        width: cols,
        height: rows.saturating_sub(1),
    };

    install_panic_hook();
    // The guard asks the terminal how big a cell is on its way in — after raw
    // mode (the reply has no newline) and before the alternate-screen switch
    // (so the wait cannot be mistaken for "the first frame is done"). Skipped
    // when graphics are off: nothing would consume the answer.
    let cell_query = if graphics_disabled {
        CellQuery::Skip
    } else {
        CellQuery::Ask
    };
    let guard = match TerminalGuard::enter(cell_query) {
        Ok(guard) => guard,
        Err(err) => {
            eprintln!("stele: could not enter raw mode: {err}");
            return ExitCode::FAILURE;
        }
    };

    // The geometry the guard resolved on its way in feeds two stages that use
    // it for different things, and both want the same number. `ImageSizer`
    // divides a probed pixel size by it to decide how many *cells* a box
    // occupies — the box's aspect ratio, which is what the reader sees.
    // `GfxMediaSink` multiplies a cell count by it to decide how many *pixels*
    // to rasterize into — resolution only.
    let geometry = guard.cell_geometry();

    let sizer: Box<dyn layout::IntrinsicSizer> = if graphics_disabled {
        Box::new(ImageSizer::disabled(&base_dir))
    } else {
        Box::new(ImageSizer::new(&base_dir).with_cell_px(geometry.cell_px))
    };

    let ctx = LayoutContext {
        doc: &doc,
        config: &config,
        engine: &engine,
        sizer: sizer.as_ref(),
    };
    let tree = layout(ctx.doc, size.width, ctx.config, ctx.engine, ctx.sizer);
    let mut state = AppState::new(tree, size, file_info);

    let mut painter = Painter::new(WidthEngine::new(WidthConfig::default()));
    if graphics_disabled {
        painter.register_media(Box::new(NoopMediaSink));
    } else {
        // The sink keeps its own copy of the document: it resolves image
        // paths and math sources by `NodeId` at paint time.
        painter.register_media(Box::new(
            GfxMediaSink::new(doc.clone(), &base_dir).with_cell_px(geometry.cell_px),
        ));
    }
    // The themed decor provides real syntax highlighting and theme colors.
    // Background is not OSC 11-probed here (that needs a pre-alt-screen query
    // round-trip); the fallback is the dark variant, and `T` (DW-1.5) flips
    // between it and light from here on. Kept as an explicit `ThemeState`
    // rather than `ThemedDecor::detect(None)` (equivalent for this initial
    // frame) so `run_session` has the variant/color-mode pair to flip.
    let mut theme = ThemeState {
        variant: highlight::Variant::Dark,
        color_mode: highlight::ColorMode::from_env(),
    };
    painter.register_decor(Box::new(ThemedDecor::new(highlight::Theme::new(
        theme.variant,
        theme.color_mode,
    ))));

    // stdout, wrapped for two reasons (DW-1.6): `BufWriter` so a frame's many
    // small writes cost one syscall instead of dozens, and
    // `PanicGuardedWriter` so a panic mid-frame can never let that buffer's
    // own best-effort flush leak stale escape bytes onto a terminal
    // `install_panic_hook` has already restored. See `terminal::PanicGuardedWriter`.
    let buffered = io::BufWriter::new(PanicGuardedWriter::new(
        io::stdout().lock(),
        stele::terminal::frame_poison_flag(),
    ));
    let mut out: Box<dyn Write> = match test_panic_after_bytes() {
        Some(remaining) => Box::new(PanicAfterBytes {
            inner: buffered,
            remaining,
        }),
        None => Box::new(buffered),
    };

    let result = run_session(&ctx, &mut state, &mut painter, &mut theme, out.as_mut());

    // Leave the alternate screen (drop restores the terminal) BEFORE printing
    // any error — an `eprintln!` while the alt screen is active is wiped out
    // by the restore, leaving the user with a bare nonzero exit and no
    // message.
    drop(guard);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("stele: {err}");
            ExitCode::FAILURE
        }
    }
}

/// The running theme toggle state (DW-1.5): which built-in variant is
/// active, and the color mode resolved once at startup (`NO_COLOR`,
/// truecolor vs. 256-color) that stays fixed across a `T` press.
struct ThemeState {
    variant: highlight::Variant,
    color_mode: highlight::ColorMode,
}

/// The interactive session: initial paint, then the scroll/resize/paint event
/// loop. Returns `Ok(())` on a clean quit and any terminal I/O error to the
/// caller, which restores the terminal before surfacing it.
fn run_session(
    ctx: &LayoutContext,
    state: &mut AppState,
    painter: &mut Painter,
    theme: &mut ThemeState,
    out: &mut dyn Write,
) -> io::Result<()> {
    let status = state.status();
    painter.frame_with_status(state.tree(), state.scroll(), state.size(), &status, out)?;

    loop {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if !handle_chrome_key(key, ctx, state, painter, theme)
                    && state.handle_key_event(key)
                {
                    break;
                }
            }
            Event::Resize(width, height) => {
                // Same reservation as the initial `Size` in `main` — see its
                // comment. `height` here is the raw terminal row count
                // crossterm reports on a resize.
                let mut sizes = vec![Size {
                    width,
                    height: height.saturating_sub(1),
                }];
                let mut quit = false;
                while event::poll(RESIZE_DEBOUNCE).unwrap_or(false) {
                    match event::read() {
                        Ok(Event::Resize(w, h)) => sizes.push(Size {
                            width: w,
                            height: h.saturating_sub(1),
                        }),
                        // A keypress mid-storm (notably `q` or Ctrl-C) must
                        // not be swallowed by the debounce drain — honor a
                        // quit, otherwise fall through and repaint.
                        Ok(Event::Key(key))
                            if key.kind == KeyEventKind::Press && state.handle_key_event(key) =>
                        {
                            quit = true;
                            break;
                        }
                        Ok(_) => break,
                        Err(_) => break,
                    }
                }
                state.apply_resize_burst(ctx, &sizes);
                if quit {
                    break;
                }
            }
            _ => continue,
        }

        let status = state.status();
        painter.frame_with_status(state.tree(), state.scroll(), state.size(), &status, out)?;
    }

    Ok(())
}

/// Bottom-row chrome and layout-affecting toggles that need resources
/// `AppState::handle_key_event` does not have: `ctx` for a relayout, and
/// `painter`/`theme` for the theme swap. (`Ctrl-g`'s file info needs
/// neither — it lives entirely inside `AppState::handle_control_chord`,
/// since `FileInfo` is static per-session data baked into `AppState` at
/// construction.) Returns whether `key` was one of these — when `true`,
/// the caller must not also pass `key` to `AppState::handle_key_event`.
fn handle_chrome_key(
    key: KeyEvent,
    ctx: &LayoutContext,
    state: &mut AppState,
    painter: &mut Painter,
    theme: &mut ThemeState,
) -> bool {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return false;
    }
    match key.code {
        KeyCode::Char('+') => state.widen(ctx),
        KeyCode::Char('-') => state.narrow(ctx),
        KeyCode::Char('T') => {
            theme.variant = toggled_variant(theme.variant);
            painter.register_decor(Box::new(ThemedDecor::new(highlight::Theme::new(
                theme.variant,
                theme.color_mode,
            ))));
            state.relayout_preserving_anchor(ctx, *ctx.config);
        }
        _ => return false,
    }
    true
}

/// The pure half of `T`'s action. Exhaustive match — no wildcard arm — so a
/// third built-in `Variant` fails to compile here instead of silently
/// leaving `T` a no-op.
fn toggled_variant(variant: highlight::Variant) -> highlight::Variant {
    match variant {
        highlight::Variant::Dark => highlight::Variant::Light,
        highlight::Variant::Light => highlight::Variant::Dark,
    }
}

/// Test-only fault injection (DW-1.6): a writer that panics once
/// `remaining` bytes have passed through it, deliberately mid-frame, with
/// content already sitting unflushed inside the `BufWriter`/
/// `PanicGuardedWriter` wrapped around it. Configured via env var — set only
/// by `tests/panic_mid_frame.rs` — rather than a method call, because the
/// only way to inject this into the exact writer stack under test is across
/// the process boundary the pty test spawns `stele` over; existing pty tests
/// configure the child the same way, through `Command::env`.
struct PanicAfterBytes<W> {
    inner: W,
    remaining: usize,
}

impl<W: Write> Write for PanicAfterBytes<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.len() >= self.remaining {
            panic!("stele: test-injected panic mid-frame (STELE_TEST_PANIC_AFTER_BYTES)");
        }
        self.remaining -= buf.len();
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn test_panic_after_bytes() -> Option<usize> {
    std::env::var("STELE_TEST_PANIC_AFTER_BYTES")
        .ok()
        .and_then(|s| s.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toggled_variant_swaps_dark_and_light_and_is_its_own_inverse() {
        assert_eq!(
            toggled_variant(highlight::Variant::Dark),
            highlight::Variant::Light
        );
        assert_eq!(
            toggled_variant(highlight::Variant::Light),
            highlight::Variant::Dark
        );
        assert_eq!(
            toggled_variant(toggled_variant(highlight::Variant::Dark)),
            highlight::Variant::Dark
        );
    }
}
