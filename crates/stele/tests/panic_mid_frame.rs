//! DW-1.6: a panic that lands mid-frame — after real frame bytes are
//! already sitting unflushed inside `run_session`'s `BufWriter` — must still
//! restore the terminal, and nothing that reaches the wire *after* the
//! restore sequence may be an escape sequence. That second half is the
//! actual bug this phase's `BufWriter`/`PanicGuardedWriter` pairing exists
//! to prevent: `BufWriter::drop` best-effort-flushes its buffer regardless
//! of why the stack is unwinding, and without `PanicGuardedWriter` that
//! flush would land the frame's stale escape bytes on a terminal
//! `terminal::on_panic` has already restored.
//!
//! `STELE_TEST_PANIC_AFTER_BYTES` (`main.rs`'s `PanicAfterBytes`) is the
//! fault-injection hook this test spawns the real binary against — the only
//! way to put the panic inside the exact writer stack under test, in the
//! subprocess the pty drives, is across the process boundary an env var
//! crosses.
//!
//! Run: `cargo test -p stele --test panic_mid_frame -- --nocapture`

#![cfg(unix)]

mod common;

use std::io::Write as _;
use std::os::unix::process::ExitStatusExt;
use std::process::Stdio;
use std::time::Duration;

use common::pty::{ENTER_ALT, Pty, RESTORE, contains, read_until};

#[test]
fn test_dw_1_6_panic_mid_frame_restores_the_terminal_with_no_stale_frame_bytes_after() {
    let doc = std::env::temp_dir().join(format!("stele-panic-mid-frame-{}.md", std::process::id()));
    // A large first frame — many small `write` calls — so a threshold well
    // past `SYNC_BEGIN` (8 bytes) still fires while genuine frame content is
    // sitting unflushed in the `BufWriter`, long before that frame's own
    // final flush would have run.
    let body: String = (0..200)
        .map(|i| format!("paragraph {i} carries a few words of its own\n\n"))
        .collect();
    std::fs::File::create(&doc)
        .and_then(|mut f| f.write_all(body.as_bytes()))
        .expect("write fixture");

    let pty = Pty::open();
    let mut cmd = common::viewer_command();
    cmd.arg(&doc)
        .env("TERM", "xterm-256color")
        // Graphics off (not Ghostty): skips the CSI 16t round trip and its
        // timeout, which this test has no use for and would only slow down.
        .env_remove("TERM_PROGRAM")
        .env_remove("TMUX")
        .env("STELE_TEST_PANIC_AFTER_BYTES", "80")
        .stdin(Stdio::from(pty.slave_dup()))
        .stdout(Stdio::from(pty.slave_dup()))
        .stderr(Stdio::from(pty.slave_dup()));
    // SAFETY: `pre_exec` runs between fork and exec, so it may only call
    // async-signal-safe functions; `setsid`/`ioctl(TIOCSCTTY)` both are.
    unsafe {
        std::os::unix::process::CommandExt::pre_exec(&mut cmd, || {
            libc::setsid();
            libc::ioctl(0, libc::TIOCSCTTY as _, 0);
            Ok(())
        });
    }
    let mut child = cmd.spawn().expect("spawn stele");

    let master = pty.master_fd();
    let before = read_until(master, ENTER_ALT, Duration::from_secs(10));
    assert!(
        contains(&before, ENTER_ALT),
        "stele never entered the alternate screen; wire was {before:?}"
    );

    let after = read_until(master, RESTORE, Duration::from_secs(10));
    let restore_at = after
        .windows(RESTORE.len())
        .position(|w| w == RESTORE)
        .unwrap_or_else(|| {
            panic!("the injected panic never produced the restore sequence: {after:?}")
        });

    // Drain a little longer: the default panic hook's message (and the
    // shell prompt returning) follow the restore sequence and are expected;
    // an escape sequence among them is not — that would be the stale
    // half-frame `BufWriter::drop` leaking past `PanicGuardedWriter`.
    let mut trailing = after[restore_at + RESTORE.len()..].to_vec();
    trailing.extend(read_until(
        master,
        b"\xff-a-needle-that-never-appears-\xff",
        Duration::from_millis(500),
    ));
    assert!(
        !trailing.contains(&0x1b),
        "an escape byte reached the wire after the restore sequence — a stale, unflushed frame \
         leaked through: {trailing:?}"
    );

    let status = child.wait().expect("wait");
    assert_ne!(
        status.code(),
        Some(0),
        "a panicking process must not report a clean exit"
    );
    assert_eq!(
        status.signal(),
        None,
        "the injected panic must not turn into a signal-based death: {status:?}"
    );

    let _ = std::fs::remove_file(&doc);
}
