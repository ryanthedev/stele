//! The `stele` binary: parse the CLI, load the document, enter the terminal,
//! and run the scroll/resize/paint event loop. All decision logic lives in the
//! library (`app`, `painter`, `loader`, `terminal`, `media`, `decor`); this
//! file is thin glue over real crossterm I/O and is not itself unit-tested —
//! it is covered black-box, through the real binary, by
//! `tests/cli_errors.rs`, `tests/document_source.rs`, `tests/quit_restore.rs`
//! and `tests/signal_restore.rs`.

use std::io::{self, Write};
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

use ast::Document;
use clap::Parser;
use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use layout::{IntrinsicSizer, LayoutConfig, layout};
use width::{WidthConfig, WidthEngine};

use stele::app::{AppState, ChromeAction, LayoutContext, Mode, PendingAction, StatusMessage};
use stele::cli::Cli;
use stele::decor::themed::ThemedDecor;
use stele::link::{Followed, Navigator, SystemOpener};
use stele::loader::{DocumentSource, LoadOptions};
use stele::media::{GfxMediaSink, ImageSizer, NoopMediaSink};
use stele::painter::{FrameOverlays, Painter, Size};
use stele::terminal::{
    CellQuery, PanicGuardedWriter, TerminalGuard, install_panic_hook, osc52_copy,
};
use stele::theme_source::ThemeSource;

/// How long a resize burst is drained for before committing to one
/// relayout — long enough to coalesce a storm of SIGWINCH-driven `Resize`
/// events, short enough that a single genuine resize still repaints
/// promptly.
const RESIZE_DEBOUNCE: Duration = Duration::from_millis(50);

/// The longest one resize burst may go on coalescing before it must commit to
/// a relayout and repaint, however many events are still arriving.
///
/// [`RESIZE_DEBOUNCE`] alone is a *quiet* period, re-armed by every arriving
/// event, so it terminates only when the resizes stop. A drag that emits them
/// faster than 50 ms — a live window drag at 60 Hz emits one every ~16 ms —
/// holds the debounce loop open for as long as the drag lasts. Measured: at a
/// 10 ms and a 40 ms resize interval the viewer painted **zero bytes** over
/// six seconds; at 70 ms (past the debounce) it painted normally. The
/// threshold was exactly the debounce.
///
/// So the burst gets a wall-clock ceiling as well as a quiet period. 200 ms
/// puts a sustained drag at ~5 repaints per second instead of none, while
/// still folding ~12 events of a 60 Hz drag into each relayout — the
/// coalescing this loop exists for is preserved, it is simply no longer
/// unbounded. Under `--watch` the ceiling is additionally clamped to the next
/// tick, so a resize storm cannot defer a reload either (DW-2.2).
const RESIZE_BURST_MAX: Duration = Duration::from_millis(200);

/// How often `--watch` checks the file's mtime, and therefore the worst-case
/// delay between an external write and the repaint (DW-2.2).
///
/// A period on the monotonic clock, not a `poll` timeout. It bounds the event
/// loop's wait ([`Session::until_next_tick`]) *and* is the schedule the reload
/// check runs on ([`Session::watch_tick_due`]) — because a timeout alone only
/// fires when nothing else is happening, which made the bound evaporate the
/// moment the reader held a key.
///
/// Deliberately polled rather than watched by the filesystem: one `stat` four
/// times a second costs nothing, needs no dependency, no second thread, and no
/// inotify/kqueue fallback matrix.
const WATCH_POLL_INTERVAL: Duration = Duration::from_millis(250);

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Resolved before anything is read: `--watch -` is a contradiction
    // (DW-2.3) and must not cost the user a terminal switch to find out.
    let source = match cli.source() {
        Ok(source) => source,
        Err(err) => {
            eprintln!("stele: {err}");
            return ExitCode::FAILURE;
        }
    };
    let options = LoadOptions {
        show_frontmatter: cli.frontmatter,
    };

    // Resolved here, alongside `--watch -` and before the terminal is touched,
    // for the same reason: a `--theme` path that does not load should fail on
    // a plain terminal with a readable message, not after an alt-screen
    // switch that wipes it.
    let mut theme = match ThemeSource::load(
        cli.theme.as_deref(),
        stele::theme_source::default_config_path().as_deref(),
        highlight::Variant::Dark,
        highlight::ColorMode::from_env(),
    ) {
        Ok(theme) => theme,
        Err(err) => {
            eprintln!("stele: {err}");
            return ExitCode::FAILURE;
        }
    };

    // Load and validate *before* touching the terminal at all: a bad path
    // should fail cleanly without ever entering raw mode / the alternate
    // screen. Stamped before the read, not after, so a write that lands while
    // we are reading is still seen as newer than this load.
    let loaded_at = Instant::now();
    let loaded = match source.load_with(options) {
        Ok(loaded) => loaded,
        Err(err) => {
            eprintln!("stele: {err}");
            return ExitCode::FAILURE;
        }
    };
    let doc = Rc::clone(&loaded.doc);
    let file_info = loaded.info;

    let engine = WidthEngine::new(WidthConfig::default());

    // Relative image paths resolve against the document's own directory.
    let base_dir = source.base_dir();

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
    let mut guard = match TerminalGuard::enter(cell_query) {
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

    let sizer: Box<dyn IntrinsicSizer> = if graphics_disabled {
        Box::new(ImageSizer::disabled(&base_dir))
    } else {
        Box::new(ImageSizer::new(&base_dir).with_cell_px(geometry.cell_px))
    };

    let mut session = Session {
        source,
        options,
        watch: cli.watch,
        doc,
        loaded_at,
        config,
        engine,
        sizer,
        last_failure: None,
        last_tick: Instant::now(),
        nav: Navigator::new(Box::new(SystemOpener), options),
        graphics_disabled,
        cell_px: geometry.cell_px,
    };
    let tree = {
        let ctx = session.ctx();
        layout(ctx.doc, size.width, ctx.config, ctx.engine, ctx.sizer)
    };
    let mut state = AppState::new(tree, size, file_info);

    let mut painter = Painter::new(WidthEngine::new(WidthConfig::default()));
    if graphics_disabled {
        painter.register_media(Box::new(NoopMediaSink));
    } else {
        // The sink *shares* the document rather than copying it (DW-2.6): it
        // resolves image paths and math sources by `NodeId` at paint time,
        // which is read-only work.
        painter.register_media(Box::new(
            GfxMediaSink::new(Rc::clone(&session.doc), &base_dir).with_cell_px(geometry.cell_px),
        ));
    }
    // The themed decor provides real syntax highlighting and theme colors.
    // Background is not OSC 11-probed here (that needs a pre-alt-screen query
    // round-trip); the fallback is the dark variant, which a theme file's own
    // `appearance` overrides. `T` (DW-1.5) flips from here on — see
    // `ThemeSource::toggle` for what it means once a user theme is loaded.
    painter.register_decor(Box::new(ThemedDecor::new(theme.theme())));

    // A theme's complaints reach the reader the same way every other
    // transient does. Loading already succeeded by here — these are the
    // things that were wrong but survivable, and saying nothing about them
    // would leave a mistyped role looking like a stele bug.
    if let Some(summary) = theme.status_summary() {
        state.set_status(StatusMessage::new(summary));
    }

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

    let result = run_session(
        &mut session,
        &mut state,
        &mut painter,
        &mut theme,
        &mut guard,
        out.as_mut(),
    );

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

/// Everything a relayout needs that outlives one document, plus what
/// `--watch` needs to decide it is time for a new one.
///
/// This exists because [`LayoutContext`] borrows the document, and since
/// `--watch` the document is no longer fixed for the session: a `LayoutContext`
/// built once in `main` would pin the *first* `Rc` for the whole run. The
/// owned parts live here and a fresh context is minted per relayout
/// ([`Session::ctx`]) — four reference copies, so the borrow lasts exactly as
/// long as the call that needs it.
struct Session {
    source: DocumentSource,
    options: LoadOptions,
    watch: bool,
    doc: Rc<Document>,
    /// When [`Session::doc`] was read, for [`DocumentSource::changed_since`].
    /// Only advanced by a *successful* load: leaving it behind after a failure
    /// is what lets a file that comes back with an unchanged mtime still be
    /// noticed.
    loaded_at: Instant,
    config: LayoutConfig,
    engine: WidthEngine,
    sizer: Box<dyn IntrinsicSizer>,
    /// The last reload failure already shown on the status row, so a file that
    /// stays missing is reported once instead of repainting the same sentence
    /// four times a second.
    last_failure: Option<String>,
    /// When the watch tick last ran, on the monotonic clock. This is what
    /// makes DW-2.2's bound a bound — see [`Session::watch_tick_due`].
    last_tick: Instant,
    /// Link activation and the `Backspace` history (Phase 6). It owns the
    /// document stack and the OS opener; this loop owns installing whatever
    /// comes back.
    nav: Navigator,
    /// Kept so a document opened by following a link can rebuild its sizer and
    /// media sink at the *new* document's directory: both resolve relative
    /// image paths, and a linked document in another folder would otherwise
    /// look for its images beside the file the reader started from.
    graphics_disabled: bool,
    cell_px: (u32, u32),
}

impl Session {
    fn ctx(&self) -> LayoutContext<'_> {
        LayoutContext {
            doc: &self.doc,
            config: &self.config,
            engine: &self.engine,
            sizer: self.sizer.as_ref(),
        }
    }

    /// How long the event loop may block before the next watch tick is owed.
    ///
    /// Not a constant `WATCH_POLL_INTERVAL`: the wait has to *shrink* as the
    /// interval is used up, or a stream of events would reset the clock on
    /// every iteration and the tick would never come due.
    fn until_next_tick(&self) -> Duration {
        WATCH_POLL_INTERVAL.saturating_sub(self.last_tick.elapsed())
    }

    /// How long the resize debounce may wait for its next event, given it
    /// started collecting with a deadline of `collect_until`.
    ///
    /// Three bounds, whichever is soonest: the debounce quantum itself, the
    /// burst ceiling ([`RESIZE_BURST_MAX`]), and — under `--watch` — the
    /// moment the next tick is owed. The third is what makes DW-2.2's latency
    /// bound survive a resize storm. Bounding the *outer* loop's wait was not
    /// enough: the debounce loop re-armed a fixed 50 ms on every arriving
    /// event and sat entirely outside [`Session::until_next_tick`]'s reach, so
    /// a resize stream faster than 50 ms held it open and the tick below was
    /// never reached. Measured before this: no reload at all at a 10 ms or
    /// 40 ms resize interval, against 0.29 s on an idle viewer.
    ///
    /// Returns zero when a deadline has passed, which the caller reads as
    /// "stop collecting" rather than as "poll with no timeout".
    fn debounce_wait(&self, collect_until: Instant) -> Duration {
        let mut wait = RESIZE_DEBOUNCE.min(collect_until.saturating_duration_since(Instant::now()));
        if self.watch {
            wait = wait.min(self.until_next_tick());
        }
        wait
    }

    /// Whether a watch tick is owed now, resetting the clock when it is.
    ///
    /// Checked on a **wall clock**, not on "did `event::poll` time out". The
    /// difference is the whole of DW-2.2's latency bound. Hanging the reload
    /// check off the timeout branch means any pending event skips it, and an
    /// autorepeating key produces a pending event on every iteration — so a
    /// held `j` starved the reload indefinitely. Measured before this was
    /// fixed: at ~14 keys/s (macOS default autorepeat) an external write went
    /// unnoticed for 8 s and counting, while the same edit on an idle viewer
    /// landed in well under a second. A reader holding a scroll key is the
    /// most ordinary thing there is, so the bound has to hold under input,
    /// not merely in its absence.
    fn watch_tick_due(&mut self) -> bool {
        if !self.watch || self.last_tick.elapsed() < WATCH_POLL_INTERVAL {
            return false;
        }
        self.last_tick = Instant::now();
        true
    }

    /// One `--watch` tick: reload if the file moved, and report whether the
    /// caller must repaint.
    ///
    /// Both outcomes are frame-safe. A successful reload parses and lays out
    /// **fully** before `state` is touched, so the swap is a single
    /// assignment between frames rather than a half-updated tree the painter
    /// could catch mid-flight. A failed one changes no tree at all: the last
    /// good render stays on screen and the reader is told why on the status
    /// row (DW-2.4), which is the right trade for a viewer — a missing file
    /// should not cost someone their scroll position.
    fn poll_reload(
        &mut self,
        state: &mut AppState,
        painter: &mut Painter,
        out: &mut dyn Write,
    ) -> bool {
        if !self.source.changed_since(self.loaded_at) {
            return false;
        }
        // The type check before the open, for the reason
        // `link::refuse_unless_regular_file` documents: this runs inside the
        // event loop with the terminal in raw mode, where Ctrl-C is a
        // keystroke rather than a signal, so a `File::open` that blocks on a
        // FIFO leaves a viewer with no way out at all. A watched file
        // replaced by one is all it takes.
        if let Err(err) = stele::link::refuse_unless_regular_file(&self.source) {
            return self.report_reload_failure(state, &err.to_string());
        }
        let attempted_at = Instant::now();
        match self.source.load_with(self.options) {
            Ok(loaded) => {
                // Kept alive one statement past the reassignment below: it is
                // `AppState::reload_document`'s only way to see what fold
                // state was keyed against (DW-5.2).
                let old_doc = Rc::clone(&self.doc);
                self.doc = Rc::clone(&loaded.doc);
                self.loaded_at = attempted_at;
                self.last_failure = None;
                painter.reload_media(loaded.doc, out);
                state.reload_document(&self.ctx(), loaded.info, Some(&old_doc));
                true
            }
            Err(err) => self.report_reload_failure(state, &err.to_string()),
        }
    }

    /// Puts a reload failure on the status row, once. Returns whether the
    /// caller must repaint.
    ///
    /// The de-duplication is the whole of it: a file that stays missing is
    /// reported once rather than repainting the same sentence four times a
    /// second. Factored out so the type-check refusal above and the load's
    /// own error take the identical path — a second copy of this would be a
    /// second place for that rule to drift.
    fn report_reload_failure(&mut self, state: &mut AppState, reason: &str) -> bool {
        let message = format!("reload failed: {reason}");
        if self.last_failure.as_deref() == Some(message.as_str()) {
            return false;
        }
        state.set_status(StatusMessage::new(message.clone()));
        self.last_failure = Some(message);
        true
    }

    /// Swaps a **different** document in: a link followed, or a `Backspace`
    /// back to one. `scroll` is where to land — 0 for a new document, the
    /// remembered offset on the way back (DW-6.2).
    ///
    /// Three things have to move together, and the order matters:
    ///
    /// 1. `reload_media` on the *old* sink first, so every raster the terminal
    ///    is still holding for the old document is deleted while the sink that
    ///    knows their ids is still the registered one.
    /// 2. A fresh sizer and sink at the new document's own directory, because
    ///    both resolve relative image paths and the reader may have hopped
    ///    into another folder entirely.
    /// 3. Only then the state swap, which lays out against the new sizer.
    fn install_document(
        &mut self,
        state: &mut AppState,
        painter: &mut Painter,
        source: DocumentSource,
        loaded: stele::loader::LoadedDocument,
        scroll: usize,
        out: &mut dyn Write,
    ) {
        painter.reload_media(Rc::clone(&loaded.doc), out);
        let base_dir = source.base_dir();
        if self.graphics_disabled {
            self.sizer = Box::new(ImageSizer::disabled(&base_dir));
            painter.register_media(Box::new(NoopMediaSink));
        } else {
            self.sizer = Box::new(ImageSizer::new(&base_dir).with_cell_px(self.cell_px));
            painter.register_media(Box::new(
                GfxMediaSink::new(Rc::clone(&loaded.doc), &base_dir).with_cell_px(self.cell_px),
            ));
        }
        self.source = source;
        self.doc = Rc::clone(&loaded.doc);
        self.loaded_at = Instant::now();
        self.last_failure = None;
        state.open_document(&self.ctx(), loaded.info, scroll);
    }
}

/// Carries out the one thing `AppState` decided but deliberately cannot do
/// itself — touch the filesystem, spawn the OS opener, or write to the
/// terminal outside a frame. Returns whether the screen must be repainted.
///
/// This is the whole of the interaction layer's contact with the OS, in one
/// function, which is what makes the rest of it testable without one.
fn perform_action(
    action: PendingAction,
    session: &mut Session,
    state: &mut AppState,
    painter: &mut Painter,
    guard: &mut TerminalGuard,
    out: &mut dyn Write,
) -> bool {
    match action {
        PendingAction::OpenLink(href) => {
            let current = session.source.clone();
            match session.nav.follow(&href, &current, state.scroll()) {
                Ok(Followed::Opened { source, loaded }) => {
                    session.install_document(state, painter, source, loaded, 0, out);
                }
                Ok(Followed::Handed) => {
                    // The opener is a detached child; there is nothing to wait
                    // for and no window to observe. Saying so is the only
                    // feedback the reader gets that the keystroke landed.
                    state.set_status(StatusMessage::new(format!("opened {href} externally")));
                }
                Err(err) => state.set_status(StatusMessage::new(err.to_string())),
            }
            true
        }
        PendingAction::Back => match session.nav.back() {
            Ok((source, loaded, scroll)) => {
                session.install_document(state, painter, source, loaded, scroll, out);
                true
            }
            Err(err) => {
                state.set_status(StatusMessage::new(err.to_string()));
                true
            }
        },
        PendingAction::CopyCodeBlock => {
            let doc = Rc::clone(&session.doc);
            let Some(text) = state.code_block_in_view(&doc) else {
                // `code_block_in_view` has already put the reason on the row.
                return true;
            };
            let bytes = text.len();
            match osc52_copy(&text) {
                Some(sequence) => {
                    // Written straight through the frame writer, inside no
                    // frame: OSC 52 paints nothing, and holding it until the
                    // next synchronized-update block would tie the clipboard
                    // to a repaint that may not come.
                    let wrote = out
                        .write_all(sequence.as_bytes())
                        .and_then(|()| out.flush());
                    state.set_status(StatusMessage::new(match wrote {
                        Ok(()) => format!("copied {bytes} bytes to the clipboard"),
                        Err(err) => format!("clipboard write failed: {err}"),
                    }));
                }
                None => state.set_status(StatusMessage::new(
                    "code block is too large for an OSC 52 clipboard write",
                )),
            }
            true
        }
        PendingAction::SetMouseCapture(on) => {
            if let Err(err) = guard.set_mouse_capture(on) {
                state.set_status(StatusMessage::new(format!("mouse capture failed: {err}")));
            }
            true
        }
    }
}

/// The interactive session: initial paint, then the scroll/resize/paint event
/// loop. Returns `Ok(())` on a clean quit and any terminal I/O error to the
/// caller, which restores the terminal before surfacing it.
fn run_session(
    session: &mut Session,
    state: &mut AppState,
    painter: &mut Painter,
    theme: &mut ThemeSource,
    guard: &mut TerminalGuard,
    out: &mut dyn Write,
) -> io::Result<()> {
    paint(state, painter, out)?;

    loop {
        let mut repaint = false;

        // Without `--watch` this is the blocking `event::read` it always was.
        // With it, the wait is capped at whatever is left of the current tick,
        // so the loop reaches the reload check on schedule whether or not any
        // events showed up in the meantime.
        let event_ready = if session.watch {
            event::poll(session.until_next_tick())?
        } else {
            true
        };

        if event_ready {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if !handle_chrome_key(key, session, state, painter, theme)
                        && state.handle_key_event(key)
                    {
                        break;
                    }
                    repaint = true;
                }
                Event::Resize(width, height) => {
                    // Same reservation as the initial `Size` in `main` — see
                    // its comment. `height` here is the raw terminal row count
                    // crossterm reports on a resize.
                    let mut sizes = vec![Size {
                        width,
                        height: height.saturating_sub(1),
                    }];
                    let mut quit = false;
                    let collect_until = Instant::now() + RESIZE_BURST_MAX;
                    loop {
                        let wait = session.debounce_wait(collect_until);
                        // A zero wait means a deadline arrived rather than the
                        // resizes stopping — the burst cap, or the moment a
                        // watch tick is owed. Either way, stop collecting and
                        // let the loop body run; the next iteration picks the
                        // stream back up where it left off.
                        if wait.is_zero() || !event::poll(wait).unwrap_or(false) {
                            break;
                        }
                        match event::read() {
                            Ok(Event::Resize(w, h)) => sizes.push(Size {
                                width: w,
                                height: h.saturating_sub(1),
                            }),
                            // A keypress mid-storm ends the burst either way.
                            // What matters is that it is *acted on* first, in
                            // the same order the main loop uses — chrome keys,
                            // then navigation. Testing only `handle_key_event`
                            // here meant `T` was read off the queue, offered
                            // to a handler that does not know it, and dropped:
                            // a theme toggle pressed while dragging a window
                            // edge did nothing at all. (`+`/`-` were dropped
                            // too, but they are overridden a few lines below
                            // regardless — `apply_resize_burst` resyncs
                            // `content_width` to the new terminal width, which
                            // is the documented "a resize always wins over a
                            // stale toggle" rule, not a swallowed key.)
                            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                                if !handle_chrome_key(key, session, state, painter, theme)
                                    && state.handle_key_event(key)
                                {
                                    quit = true;
                                }
                                break;
                            }
                            Ok(_) => break,
                            Err(_) => break,
                        }
                    }
                    state.apply_resize_burst(&session.ctx(), &sizes);
                    if quit {
                        break;
                    }
                    repaint = true;
                }
                // Mouse reports (DW-6.6): the wheel scrolls, a left click
                // activates the link under it, and everything else is inert.
                // `handle_mouse_event` says whether anything changed, so a
                // click on empty text does not cost a repaint.
                Event::Mouse(mouse) => {
                    repaint = state.handle_mouse_event(mouse, &session.engine);
                }
                // Anything else (focus, paste) changes nothing on screen, so
                // it does not earn a frame — but it must still fall through to
                // the watch tick below rather than `continue`, or a stream of
                // them would starve the reload just as keys did.
                _ => {}
            }
        }

        // Whatever the event decided, it may have left one thing for the OS.
        // Drained here — once, after the event branch and before the watch
        // tick — so a link that swaps the document does so between frames
        // rather than underneath one.
        if let Some(action) = state.take_action() {
            repaint |= perform_action(action, session, state, painter, guard, out);
        }

        // Independent of what the event branch did, and of whether it ran at
        // all: the reload is owed on a clock, not on an idle keyboard.
        if session.watch_tick_due() {
            repaint |= session.poll_reload(state, painter, out);
        }

        if repaint {
            paint(state, painter, out)?;
        }
    }

    Ok(())
}

/// One frame, of whichever kind the current [`Mode`] calls for.
///
/// The status row is asked for exactly once per painted frame either way,
/// because that call is what ages a transient message toward its TTL (see
/// `AppState::status`) — an overlay frame that skipped it would freeze a
/// `Ctrl-G` message on screen for as long as the TOC stayed open.
///
/// `Mode::Search` paints the *document*, not an overlay: incremental search
/// is read against what is on screen, so the reader has to keep seeing it.
/// The query itself lives in the status row and the matches are painted into
/// the document by the search overlay, so the two modes share this arm.
fn paint(state: &mut AppState, painter: &mut Painter, out: &mut dyn Write) -> io::Result<()> {
    let status = state.status();
    match state.mode() {
        // The TOC is the one mode that paints something other than the
        // document: a full-screen list, over the whole content viewport.
        Mode::Toc { .. } => {
            let rows = state.toc_rows(state.size().height);
            painter.frame_overlay(&rows, state.size(), &status, out)
        }
        // Every other mode is the document with zero, one, or both overlays
        // on it, so all three go through **one** call rather than a wrapper
        // each. That matters and is not tidiness: search state outlives
        // `Mode::Search` on purpose (`n`/`N` traverse from normal mode), so a
        // reader who searches and then presses `Tab` is in `LinkSelect` *with
        // matches to highlight*. A per-mode wrapper would have silently
        // dropped one overlay or the other depending on which mode won the
        // match — and the mode that wins is the one the reader is not
        // thinking about.
        Mode::Normal | Mode::Search { .. } | Mode::LinkSelect { .. } => {
            let selected = state.selection_spans();
            let overlays = FrameOverlays {
                search: state.search_overlay(),
                selected: &selected,
            };
            painter.frame_with_overlays(
                state.tree(),
                state.scroll(),
                state.size(),
                &status,
                overlays,
                out,
            )
        }
    }
}

/// Bottom-row chrome and layout-affecting toggles that need resources
/// `AppState::handle_key_event` does not have: `ctx` for a relayout, and
/// `painter`/`theme` for the theme swap. (`Ctrl-g`'s file info needs
/// neither — it lives entirely inside `AppState::handle_control_chord`,
/// since `FileInfo` is static per-session data baked into `AppState` at
/// construction.) Returns whether `key` was one of these — when `true`,
/// the caller must not also pass `key` to `AppState::handle_key_event`.
///
/// **These are document-reading keys, so a mode that owns the keyboard gets
/// them first — which here means it gets them at all.** This function runs
/// *before* `AppState::handle_key_event` in both call sites, and returning
/// `true` is what stops the key reaching it. That is a key-stealing shape:
/// any binding added here silently outranks every mode `AppState` has, and
/// the mode cannot even see that it happened. Three phases found it
/// independently — Phase 2 from the resize drain (a `T` read mid-drag was
/// offered only to `handle_key_event`, which does not know it, and
/// vanished), Phase 3 from the TOC (`+`/`-`/`T` relaying out a document the
/// overlay was not showing), Phase 4 from the search prompt (`/The` searched
/// for `he` and swapped the theme on the way).
///
/// **So the decision of whether `key` is chrome is not made here at all.**
/// It is `AppState::chrome_action`'s, for two reasons: it depends on the
/// mode, and this file is not unit-tested (see the module doc), which is
/// precisely why the same defect could be found three times without any of
/// the three being caught by a test. Phase 3's `state.mode() != Mode::Normal`
/// gate fixed the instance correctly; asking `Mode::captures_all_keys`
/// instead makes the *next* mode a compile error rather than a fourth
/// rediscovery. What is left in this function is the part that genuinely
/// cannot move: performing the action against a painter and a layout context.
fn handle_chrome_key(
    key: KeyEvent,
    session: &Session,
    state: &mut AppState,
    painter: &mut Painter,
    theme: &mut ThemeSource,
) -> bool {
    let Some(action) = state.chrome_action(key) else {
        return false;
    };
    let ctx = session.ctx();
    match action {
        ChromeAction::Widen => state.widen(&ctx),
        ChromeAction::Narrow => state.narrow(&ctx),
        ChromeAction::ToggleTheme => {
            theme.toggle();
            painter.register_decor(Box::new(ThemedDecor::new(theme.theme())));
            state.relayout_preserving_anchor(&ctx, *ctx.config);
        }
        // Zero-arg by design (Phase 5's `Produces`): each of these only
        // decides *what* changed in `AppState::folds`; the relayout that
        // applies it needs `ctx`, which they do not take, so it happens here
        // — the same split `Widen`/`Narrow` already draw between deciding
        // and relaying out.
        ChromeAction::ToggleFold => {
            state.toggle_fold();
            state.relayout_preserving_anchor(&ctx, *ctx.config);
        }
        ChromeAction::ExpandAllFolds => {
            state.expand_all();
            state.relayout_preserving_anchor(&ctx, *ctx.config);
        }
        ChromeAction::CollapseAllFolds => {
            state.collapse_all();
            state.relayout_preserving_anchor(&ctx, *ctx.config);
        }
    }
    true
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

// `T`'s dark/light flip moved into `stele::theme_source`, which now owns what
// the key means once a user theme is loaded — with no theme it is the same
// flip it always was, with one it reaches the built-in at the opposite end.
// DW-1.5's assertion lives there as
// `test_toggling_without_a_theme_is_the_original_dark_light_flip`, alongside
// `test_toggling_a_user_theme_reaches_the_opposite_built_in_and_back`.
