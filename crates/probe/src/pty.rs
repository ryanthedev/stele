//! The barricade: proves the current process is actually attached to a
//! Ghostty-hosted PTY before anything downstream is allowed to treat stdio
//! as a live terminal channel.
//!
//! This is the one place in the crate that trusts the *environment* rather
//! than a byte a subprocess sent us. Everything past construction of a
//! [`GhosttyPty`] assumes the checks below already ran.

use std::env;

/// Proof that the current process's stdin/stdout are a real tty and the
/// controlling terminal identifies itself as Ghostty.
///
/// Constructed only via [`GhosttyPty::from_current_process`], which performs
/// the validation. There is deliberately no public constructor that skips
/// the checks — every [`crate::Probe`] is built from one of these, so every
/// probe session is guaranteed to have passed the barricade.
#[derive(Debug)]
pub struct GhosttyPty {
    _private: (),
}

/// Why [`GhosttyPty::from_current_process`] refused to vouch for the
/// environment. All three are "the external world doesn't look like what we
/// need," not internal bugs — hence `Result`, not an assertion.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GhosttyPtyError {
    #[error("stdin (fd 0) is not a tty — this process was not launched interactively")]
    StdinNotTty,
    #[error("stdout (fd 1) is not a tty — this process was not launched interactively")]
    StdoutNotTty,
    #[error("TERM_PROGRAM is {0:?}, expected Some(\"ghostty\")")]
    WrongTermProgram(Option<String>),
}

impl GhosttyPty {
    /// Validates that this process's stdio is a real tty and that the
    /// controlling terminal reports itself as Ghostty via `TERM_PROGRAM`.
    ///
    /// Ghostty sets `TERM_PROGRAM=ghostty` on every child it launches
    /// (confirmed empirically for both direct `ghostty -e` and
    /// `open -na Ghostty.app --args -e` invocation — see
    /// `docs/spikes/ghostty-caps.md`), so this is a real environmental
    /// signal, not a self-report we merely hope is honest.
    pub fn from_current_process() -> Result<Self, GhosttyPtyError> {
        // SAFETY: isatty(2) on a constant, valid fd number (0, 1) is always
        // safe to call; it never dereferences a pointer we own.
        if unsafe { libc::isatty(0) } == 0 {
            return Err(GhosttyPtyError::StdinNotTty);
        }
        if unsafe { libc::isatty(1) } == 0 {
            return Err(GhosttyPtyError::StdoutNotTty);
        }
        let term_program = env::var("TERM_PROGRAM").ok();
        if term_program.as_deref() != Some("ghostty") {
            return Err(GhosttyPtyError::WrongTermProgram(term_program));
        }
        Ok(GhosttyPty { _private: () })
    }
}
