//! DW-6.4: *"With `$TMUX` set: alt-text path, zero graphics escapes emitted."*
//!
//! **What the gate was verified by before, and why it could not fail.** The
//! only DW-6.4 test was `media::sizer`'s
//! `test_dw_6_4_disabled_sizer_yields_no_reserved_boxes_and_full_frame_has_no_kitty_escapes`,
//! which constructs `ImageSizer::disabled(..)` by hand and then asserts no
//! `\x1b_G` reaches the wire. That is a true and useful statement about a
//! disabled sizer — and it says nothing whatsoever about `$TMUX`. The variable
//! appears nowhere in it. The one line that connects the two is in `main.rs`:
//!
//! ```ignore
//! let graphics_disabled = cli.no_images || std::env::var_os("TMUX").is_some() || !is_ghostty;
//! ```
//!
//! Misspell that key, invert the `||` chain, or drop the `graphics_disabled`
//! branch entirely, and every DW-6.4 assertion in the repo still passes while
//! every tmux user gets raw kitty escapes painted into their scrollback. An
//! assertion on the effect cannot see the trigger.
//!
//! So these two tests drive the real decision, through the real binary, on a
//! real pty, over a document that really does contain an image — and they come
//! in a matched pair. Without the complement ("with `$TMUX` removed, graphics
//! *are* emitted") the first test degenerates into "stele never emits
//! graphics", which would pass trivially against a viewer whose graphics
//! support had been deleted.

#![cfg(unix)]

use std::io::Write as _;
use std::os::unix::process::CommandExt as _;
use std::process::Stdio;
use std::time::Duration;

mod common;

use common::fixtures::{scratch_dir, write_png};

// The pty is local rather than `common::pty::Pty` for one mechanical reason:
// this test has to read the *master* side to see what the viewer painted, and
// `common::pty::Pty` keeps its master descriptor private with no accessor (its
// own harness reads it from inside the module). Everything else here — the
// `posix_openpt` dance, the fixed 80x24 winsize, `setsid` + `TIOCSCTTY` —
// deliberately mirrors that harness; if it ever grows a `master_fd()`, delete
// all of this and use it.
#[allow(unsafe_code)]
mod tty {
    use std::ffi::CStr;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
    use std::time::{Duration, Instant};

    pub struct Pty {
        master: OwnedFd,
        slave: OwnedFd,
    }

    impl Pty {
        pub fn open() -> Pty {
            // SAFETY: the POSIX pty-allocation dance. `ptsname` returns a
            // pointer into a libc-owned static buffer; it is copied into an
            // owned `CString` before any other libc call can clobber it.
            unsafe {
                let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
                assert!(master >= 0, "posix_openpt failed");
                assert_eq!(libc::grantpt(master), 0, "grantpt failed");
                assert_eq!(libc::unlockpt(master), 0, "unlockpt failed");
                let name_ptr = libc::ptsname(master);
                assert!(!name_ptr.is_null(), "ptsname failed");
                let name = CStr::from_ptr(name_ptr).to_owned();
                let slave = libc::open(name.as_ptr(), libc::O_RDWR | libc::O_NOCTTY);
                assert!(slave >= 0, "open({name:?}) failed");
                // A deterministic viewport; an unsized pty reports 0x0 and the
                // painter has nothing to lay out. The zero pixel fields are
                // also the realistic case for `TIOCGWINSZ`, so the geometry
                // ladder falls through to its documented fallback here.
                let size = libc::winsize {
                    ws_row: 24,
                    ws_col: 80,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                };
                assert_eq!(
                    libc::ioctl(master, libc::TIOCSWINSZ as _, &size),
                    0,
                    "TIOCSWINSZ failed"
                );
                Pty {
                    master: OwnedFd::from_raw_fd(master),
                    slave: OwnedFd::from_raw_fd(slave),
                }
            }
        }

        pub fn master_fd(&self) -> RawFd {
            self.master.as_raw_fd()
        }

        /// A fresh descriptor for the slave, for one of the child's stdio slots.
        pub fn slave_dup(&self) -> OwnedFd {
            // SAFETY: `dup` of a descriptor we own; the result is handed
            // straight to `OwnedFd`, which closes it exactly once.
            unsafe {
                let fd = libc::dup(self.slave.as_raw_fd());
                assert!(fd >= 0, "dup failed");
                OwnedFd::from_raw_fd(fd)
            }
        }
    }

    /// Drain `master` until `needle` shows up or `deadline` elapses; return
    /// everything read. A needle that never arrives makes this a bounded
    /// "read for `deadline`".
    pub fn read_until(master: RawFd, needle: &[u8], deadline: Duration) -> Vec<u8> {
        let start = Instant::now();
        let mut buf: Vec<u8> = Vec::new();
        while start.elapsed() < deadline {
            if contains(&buf, needle) {
                break;
            }
            let mut pfd = libc::pollfd {
                fd: master,
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one initialised `pollfd`, count 1, 50 ms timeout.
            let ready = unsafe { libc::poll(&mut pfd, 1, 50) };
            if ready <= 0 {
                continue;
            }
            let mut chunk = [0u8; 8192];
            // SAFETY: reading into a stack buffer we own, bounded by its length.
            let n = unsafe { libc::read(master, chunk.as_mut_ptr().cast(), chunk.len()) };
            if n > 0 {
                buf.extend_from_slice(&chunk[..n as usize]);
            } else {
                break;
            }
        }
        buf
    }

    pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }
}

use tty::{Pty, contains, read_until};

/// The kitty graphics APC introducer. Every command the emitter produces —
/// transmit (`a=t`), place (`a=p`), delete (`a=d`) — starts with these bytes,
/// so "zero graphics escapes" is exactly "this byte string never appears".
const KITTY_APC: &[u8] = b"\x1b_G";

/// The synchronized-update *end* that closes every `Painter::frame`. Reading
/// until this appears means one whole frame has been composed and flushed,
/// which is the point at which "did the frame contain graphics" is answerable.
const FRAME_END: &[u8] = b"\x1b[?2026l";

/// The alt text of the image in the fixture below, and therefore what a reader
/// on the degraded path must actually see.
const ALT_TEXT: &str = "a picture of a badger";

/// Runs the real binary on a real pty against a document containing one local
/// PNG, with `TMUX` either set or removed, and returns everything it wrote up
/// to the end of the first frame.
///
/// `TERM_PROGRAM=ghostty` in both cases on purpose: it is the *other* input to
/// `graphics_disabled`, and holding it fixed is what makes `TMUX` the only
/// thing that differs between the two tests.
fn wire_for(tag: &str, tmux: Option<&str>) -> Vec<u8> {
    let dir = scratch_dir(&format!("dw-6-4-{tag}"));
    // 240x480 px is exactly 10x10 cells at the fallback 24x48 geometry, so the
    // image is comfortably inside an 80x24 viewport and really does get a box.
    write_png(&dir, "pic.png", 240, 480);
    let doc = dir.join("doc.md");
    std::fs::File::create(&doc)
        .and_then(|mut f| write!(f, "# heading\n\n![{ALT_TEXT}](pic.png)\n\nbody text\n"))
        .expect("write fixture document");

    let pty = Pty::open();
    let mut cmd = common::viewer_command();
    cmd.arg(&doc)
        .env("TERM", "xterm-256color")
        .env("TERM_PROGRAM", "ghostty")
        .stdin(Stdio::from(pty.slave_dup()))
        .stdout(Stdio::from(pty.slave_dup()))
        .stderr(Stdio::from(pty.slave_dup()));
    match tmux {
        Some(value) => cmd.env("TMUX", value),
        None => cmd.env_remove("TMUX"),
    };
    // SAFETY: `pre_exec` runs between fork and exec, so it may only call
    // async-signal-safe functions. `setsid` and `ioctl(TIOCSCTTY)` both are;
    // together they make the pty the child's controlling terminal, which is
    // what lets crossterm read a terminal size at all.
    #[allow(unsafe_code)]
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            libc::ioctl(0, libc::TIOCSCTTY as _, 0);
            Ok(())
        });
    }
    let mut child = cmd.spawn().expect("spawn stele");

    let master = pty.master_fd();
    // Read past the first frame's end marker. The startup `CSI 16t` query
    // (`terminal::query_cell_px`) goes out before the first frame and a bare
    // pty never answers it, so this also covers the fallback-geometry path.
    let mut wire = read_until(master, FRAME_END, Duration::from_secs(20));
    // One more short read: `\x1b[?2026l` also *opens* nothing, and on a slow
    // machine the frame's tail can lag the marker we matched on.
    wire.extend_from_slice(&read_until(
        master,
        b"\x00never\x00",
        Duration::from_millis(300),
    ));

    // SIGKILL rather than typing `q`. The viewer is mid-session with a pty
    // whose buffer this test has stopped draining, so a `q` keystroke can
    // arrive at a process blocked in `write` and never be read — a deadlock
    // that has nothing to do with what is under test. The clean-quit and
    // signal-restore paths are DW-5.1's business and are asserted in
    // `quit_restore.rs` / `signal_restore.rs`; here the frame is already
    // captured and the process only has to stop existing.
    let _ = child.kill();
    let _ = child.wait();
    std::fs::remove_dir_all(&dir).ok();
    wire
}

#[test]
fn test_dw_6_4_tmux_takes_the_alt_text_path_and_emits_zero_graphics_escapes() {
    let wire = wire_for("tmux-set", Some("/private/tmp/tmux-501/default,12345,0"));
    let text = String::from_utf8_lossy(&wire);
    assert!(
        !contains(&wire, KITTY_APC),
        "with $TMUX set, not one kitty graphics escape may reach the wire — \
         tmux does not pass them through, so they land in the user's \
         scrollback as garbage. Wire was: {text:?}"
    );
    assert!(
        text.contains(ALT_TEXT),
        "the reader must get the image's alt text instead of a blank box. \
         Wire was: {text:?}"
    );
}

/// The complement. Same binary, same document, same `TERM_PROGRAM`, `$TMUX`
/// removed — graphics must now be emitted. Without this the test above is
/// satisfied by a viewer that cannot draw images at all.
#[test]
fn test_dw_6_4_complement_without_tmux_the_same_document_does_emit_graphics() {
    let wire = wire_for("tmux-unset", None);
    let text = String::from_utf8_lossy(&wire);
    assert!(
        contains(&wire, KITTY_APC),
        "with $TMUX removed and TERM_PROGRAM=ghostty, this document must emit \
         graphics — otherwise the $TMUX test above proves nothing. Wire was: \
         {text:?}"
    );
    assert!(
        contains(&wire, b"\x1b_Ga=t,f=100,i="),
        "expected a real PNG transmission, not just some graphics command. \
         Wire was: {text:?}"
    );
    assert!(
        contains(&wire, b"\x1b_Ga=p,i="),
        "the transmitted image must also be placed. Wire was: {text:?}"
    );
}
