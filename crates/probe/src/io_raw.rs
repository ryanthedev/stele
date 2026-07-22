//! Fd-level read/write with a hard wall-clock timeout.
//!
//! This is the module the defensive-programming pass most cares about: the
//! plan's own edge-case note is "probe read timeouts (a non-answering
//! terminal must not hang the harness)." Every blocking call in this file
//! is bounded by `poll(2)` before ever touching `read`/`write`, so a
//! terminal (or a dead PTY) that never answers can only ever produce a
//! `None`, never a hang.

use std::io;
use std::os::fd::RawFd;
use std::time::{Duration, Instant};

/// Waits up to `timeout` for `fd` to become readable, then reads whatever is
/// available (up to 4096 bytes in one call). Returns `None` on timeout, EOF,
/// or a transient error — callers that need to distinguish those cases use
/// [`try_read_once`] directly; [`Probe::query`](crate::Probe::query) only
/// needs "did an answer arrive."
pub(crate) fn try_read_once(fd: RawFd, timeout: Duration) -> Option<Vec<u8>> {
    if !poll_readable(fd, timeout) {
        return None;
    }
    let mut buf = [0u8; 4096];
    // SAFETY: `fd` is a valid, open fd for the lifetime of the caller (owned
    // by `Probe`); `buf` is a live, correctly-sized stack buffer.
    let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
    if n <= 0 {
        return None;
    }
    Some(buf[..n as usize].to_vec())
}

/// `poll(2)` on `fd` for readability, bounded by `timeout`. Never blocks
/// longer than `timeout` regardless of what the far end of the PTY does.
pub(crate) fn poll_readable(fd: RawFd, timeout: Duration) -> bool {
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
    // SAFETY: `&mut pfd` is a valid pointer to one `pollfd` for the
    // duration of the call, matching the `nfds = 1` argument.
    let ret = unsafe { libc::poll(&mut pfd, 1, ms) };
    ret > 0 && (pfd.revents & libc::POLLIN) != 0
}

/// Writes `data` to `fd` in full, looping over short writes. Used for
/// sending escape sequences — these are always small (well under a PIPE_BUF
/// write's atomicity guarantee), so this is expected to complete in one
/// syscall in practice, but the loop makes that an optimization rather than
/// an assumption.
pub(crate) fn write_all(fd: RawFd, data: &[u8]) -> io::Result<()> {
    let mut written = 0;
    while written < data.len() {
        // SAFETY: `data[written..]` is a valid slice for its own length.
        let n = unsafe { libc::write(fd, data[written..].as_ptr().cast(), data.len() - written) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        written += n as usize;
    }
    Ok(())
}

/// Reads repeatedly until either the overall `timeout` elapses with zero
/// bytes received (→ `None`, the terminal never answered), or at least one
/// byte has arrived and a `quiet` window passes with no further bytes (→
/// `Some(buf)`, the response is presumed complete). This collects
/// multi-chunk responses (e.g. a kitty APC reply split across reads)
/// without waiting the full `timeout` once data is flowing.
pub(crate) fn read_until_quiet(fd: RawFd, timeout: Duration, quiet: Duration) -> Option<Vec<u8>> {
    let deadline = Instant::now() + timeout;
    let mut buf = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let wait = if buf.is_empty() {
            remaining
        } else {
            quiet.min(remaining)
        };
        match try_read_once(fd, wait) {
            Some(chunk) => buf.extend_from_slice(&chunk),
            None => {
                if buf.is_empty() {
                    return None;
                }
                break;
            }
        }
    }
    if buf.is_empty() { None } else { Some(buf) }
}
