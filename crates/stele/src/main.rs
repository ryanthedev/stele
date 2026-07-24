//! The `stele` binary: parse the CLI, load the document, enter the
//! terminal, and run the scroll/resize/paint event loop. All decision logic
//! lives in the library (`app`, `painter`, `loader`, `terminal`); this file
//! is thin glue over real crossterm I/O and is not itself unit-tested.

use std::io;
use std::process::ExitCode;
use std::time::Duration;

use ast::Document;
use clap::Parser;
use crossterm::event::{self, Event, KeyEventKind};
use layout::{LayoutConfig, layout};
use width::{WidthConfig, WidthEngine};

use stele::app::{AppState, LayoutContext};
use stele::cli::Cli;
use stele::decor::StructuralDecor;
use stele::loader;
use stele::media::{GfxMediaSink, ImageSizer, NoopMediaSink};
use stele::painter::{Painter, Size};
use stele::terminal::{TerminalGuard, install_panic_hook};

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
    let sizer: Box<dyn layout::IntrinsicSizer> = if graphics_disabled {
        Box::new(ImageSizer::disabled(&base_dir))
    } else {
        Box::new(ImageSizer::new(&base_dir))
    };

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
    let size = Size {
        width: cols,
        height: rows,
    };

    install_panic_hook();
    let guard = match TerminalGuard::enter() {
        Ok(guard) => guard,
        Err(err) => {
            eprintln!("stele: could not enter raw mode: {err}");
            return ExitCode::FAILURE;
        }
    };

    let ctx = LayoutContext {
        doc: &doc,
        config: &config,
        engine: &engine,
        sizer: sizer.as_ref(),
    };
    let tree = layout(ctx.doc, size.width, ctx.config, ctx.engine, ctx.sizer);
    let mut state = AppState::new(tree, size);

    let mut painter = Painter::new(WidthEngine::new(WidthConfig::default()));
    if graphics_disabled {
        painter.register_media(Box::new(NoopMediaSink));
    } else {
        // The sink keeps its own copy of the document: it resolves image
        // paths and math sources by `NodeId` at paint time.
        painter.register_media(Box::new(GfxMediaSink::new(doc.clone(), &base_dir)));
    }
    painter.register_decor(Box::new(StructuralDecor));

    let result = run_session(&ctx, &mut state, &mut painter);

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

/// The interactive session: initial paint, then the scroll/resize/paint event
/// loop. Returns `Ok(())` on a clean quit and any terminal I/O error to the
/// caller, which restores the terminal before surfacing it.
fn run_session(ctx: &LayoutContext, state: &mut AppState, painter: &mut Painter) -> io::Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    painter.frame(state.tree(), state.scroll(), state.size(), &mut out)?;

    loop {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if state.handle_key(key.code) {
                    break;
                }
            }
            Event::Resize(width, height) => {
                let mut sizes = vec![Size { width, height }];
                let mut quit = false;
                while event::poll(RESIZE_DEBOUNCE).unwrap_or(false) {
                    match event::read() {
                        Ok(Event::Resize(w, h)) => sizes.push(Size {
                            width: w,
                            height: h,
                        }),
                        // A keypress mid-storm (notably `q`) must not be
                        // swallowed by the debounce drain — honor a quit,
                        // otherwise fall through and repaint.
                        Ok(Event::Key(key))
                            if key.kind == KeyEventKind::Press && state.handle_key(key.code) =>
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

        painter.frame(state.tree(), state.scroll(), state.size(), &mut out)?;
    }

    Ok(())
}
