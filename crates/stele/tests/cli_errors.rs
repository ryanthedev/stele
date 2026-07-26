//! Black-box tests over the real `stele` binary's error paths (DW-5.4, DW-2.3,
//! DW-2.1's routing half).
//!
//! **The rule this file lives by: every test here must exercise a path that
//! ends before `main` touches the terminal.** Argument validation and document
//! loading both happen before `crossterm::terminal::size()` in `main.rs`, so a
//! child on that path exits on its own and can be run from a plain subprocess.
//!
//! That rule is not cosmetic tidiness, and it is written here because breaking
//! it cost a suite hang. A test in this file used to pipe a *valid* document to
//! `stele -` and assume "no controlling terminal here, so the viewer fails at
//! raw mode". That assumption held only while stele used crossterm's default
//! event source. With `use-dev-tty` (see `crates/stele/Cargo.toml`), a child
//! whose stdin is a pipe resolves keys against `/dev/tty` — which, when the
//! suite is run from a terminal, is *the developer's own terminal*. The child
//! entered raw mode and the alternate screen on the real terminal and blocked
//! in `event::read()` forever, wedging `cargo test` and leaving the terminal in
//! raw mode. It passed in CI only because CI had no controlling terminal.
//!
//! So: anything that needs the viewer to actually reach the terminal belongs in
//! `tests/document_source.rs`, which gives the child a pty of its own through
//! `common::pty`. And every child spawned here goes through [`run_bounded`], so
//! a future regression that reaches terminal entry *fails on a deadline*
//! instead of hanging the run.

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn stele_bin() -> &'static str {
    env!("CARGO_BIN_EXE_stele")
}

/// Every path exercised here exits in milliseconds; this is three orders of
/// magnitude of headroom, so exceeding it means the child is blocked, not slow.
const EXIT_DEADLINE: Duration = Duration::from_secs(10);

/// One `stele` run's captured output.
struct Run {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl Run {
    fn stderr(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.stderr)
    }
}

/// Runs `stele args`, optionally with `stdin_bytes` piped in, and **fails
/// rather than hangs** if the child has not exited within [`EXIT_DEADLINE`].
///
/// The deadline is the point. `Child::wait_with_output` waits forever, so a
/// child that unexpectedly reaches terminal entry takes the whole test run with
/// it — see this module's doc comment. Here the child is killed and the test
/// reports what it was doing.
fn run_bounded(args: &[&str], stdin_bytes: Option<&[u8]>) -> Run {
    let mut child = Command::new(stele_bin())
        .args(args)
        .stdin(if stdin_bytes.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stele");

    if let Some(bytes) = stdin_bytes {
        let mut pipe = child.stdin.take().expect("piped stdin");
        pipe.write_all(bytes).expect("write document to stdin");
        // The EOF is what lets a stdin-sourced viewer finish reading.
        drop(pipe);
    }

    let deadline = Instant::now() + EXIT_DEADLINE;
    loop {
        match child.try_wait().expect("try_wait") {
            Some(_) => break,
            None if Instant::now() >= deadline => {
                child.kill().ok();
                child.wait().ok();
                panic!(
                    "`stele {}` did not exit within {EXIT_DEADLINE:?}. Every test in \
                     this file must stay on a path that ends before the terminal is \
                     touched — a child that blocks here is one that reached \
                     `event::read()`, and on a developer's machine that means it \
                     captured their terminal. Move the test to \
                     tests/document_source.rs and give it a pty.",
                    args.join(" ")
                );
            }
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }

    let output = child.wait_with_output().expect("wait_with_output");
    Run {
        status: output.status,
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

#[test]
fn test_dw_5_4_missing_file_exits_nonzero_with_clean_stderr() {
    let run = run_bounded(&["/nonexistent/does-not-exist.md"], None);

    assert!(!run.status.success());
    assert_ne!(run.status.code(), Some(0));
    let stderr = run.stderr();
    assert!(stderr.starts_with("stele:"), "stderr was: {stderr}");
    assert!(stderr.contains("could not read file"));
}

#[test]
fn test_dw_5_4_invalid_utf8_exits_nonzero_with_clean_stderr() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("stele-cli-invalid-utf8-{}.md", std::process::id()));
    std::fs::write(&path, [0x48, 0x65, 0x6c, 0x6c, 0x6f, 0xff, 0xfe]).unwrap();

    let run = run_bounded(&[path.to_str().expect("utf-8 temp path")], None);
    std::fs::remove_file(&path).ok();

    assert!(!run.status.success());
    assert_ne!(run.status.code(), Some(0));
    let stderr = run.stderr();
    assert!(stderr.starts_with("stele:"), "stderr was: {stderr}");
    assert!(stderr.contains("not valid UTF-8"));
}

/// DW-2.3: `--watch -` is rejected before anything else happens. The exit
/// status alone would not prove that — asserted on the message, which must
/// name both halves so the reader knows which one to drop.
#[test]
fn test_dw_2_3_watch_with_stdin_exits_nonzero_before_touching_the_terminal() {
    let run = run_bounded(&["-", "--watch"], None);

    assert!(!run.status.success());
    assert_ne!(run.status.code(), Some(0));
    let stderr = run.stderr();
    assert!(stderr.starts_with("stele:"), "stderr was: {stderr}");
    assert!(stderr.contains("--watch"), "stderr was: {stderr}");
    assert!(stderr.contains("stdin"), "stderr was: {stderr}");
    // Nothing may have been painted: the rejection happens before raw mode
    // and before the alternate screen.
    assert!(
        run.stdout.is_empty(),
        "stdout was: {:?}",
        String::from_utf8_lossy(&run.stdout)
    );
}

/// DW-2.1's routing half: `-` is accepted, and the bytes it reads are the ones
/// on **stdin** — not a file whose name happens to be `-`.
///
/// The document piped in is deliberately invalid UTF-8, which makes this
/// provable without ever reaching the terminal: the load fails at the
/// barricade, so the process reports and exits on its own. `not valid UTF-8`
/// can only be reached by having *read the pipe*, and `could not read file`
/// would be the signature of `-` having been opened as a path — so one message
/// distinguishes the two outcomes this test exists to tell apart. (Whether a
/// *valid* piped document then renders and answers keys is DW-2.1's other
/// half, proved on a real pty by
/// `tests/document_source.rs::test_dw_2_1_piped_stdin_renders_and_still_answers_keys_from_the_tty`.)
#[test]
fn test_dw_2_1_a_bare_dash_reads_the_document_from_stdin_not_from_a_file() {
    let run = run_bounded(&["-"], Some(&[b'#', b' ', b'h', b'i', 0xff, 0xfe, b'\n']));

    assert!(!run.status.success());
    let stderr = run.stderr();
    assert!(stderr.starts_with("stele:"), "stderr was: {stderr}");
    assert!(
        stderr.contains("not valid UTF-8"),
        "the piped bytes must have been read and validated as the document: {stderr}"
    );
    assert!(
        !stderr.contains("could not read file"),
        "`-` must be read as stdin, not opened as a file named `-`: {stderr}"
    );
    assert!(
        !stderr.contains("--watch"),
        "`-` alone must not trip the --watch conflict: {stderr}"
    );
}
