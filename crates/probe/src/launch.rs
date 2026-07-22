//! Drives a real Ghostty session from *outside* it: spawns a probe binary
//! as Ghostty's child (the only workable direction — Ghostty is a GUI app,
//! not something you can hand a PTY master and drive from the far end) and
//! waits, with a hard timeout, for that binary to write its results.
//!
//! Launch-pattern note, recorded here because it cost real debugging time:
//! invoking the `ghostty` CLI binary directly (`ghostty -e <bin>`, as
//! naively suggested by "run it inside a Ghostty window as its child")
//! **hangs indefinitely** in this environment — verified by three separate
//! attempts (direct exec, backgrounded, and via `launchctl asuser`), all of
//! which left the invoking process alive but stuck, never executing the
//! child command. The working pattern is going through macOS's `open(1)`,
//! which properly round-trips through Launch Services / the app's
//! single-instance handshake: `open -na /Applications/Ghostty.app --args -e
//! <bin> <args...>`. That is what [`Launcher`] shells out to.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Why [`Launcher::run_probe`] didn't produce a result.
#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("failed to spawn `open`: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("`open -na <Ghostty> --args -e ...` exited non-zero: {0}")]
    OpenFailed(std::process::ExitStatus),
    #[error(
        "timed out after {0:?} waiting for the probe's result file — Ghostty likely never \
         answered, or the probe binary itself hung"
    )]
    Timeout(Duration),
    #[error("result file appeared but could not be read: {0}")]
    ReadResult(#[source] std::io::Error),
    #[error("stale result file at {path} could not be removed before launching: {source}")]
    StaleResultNotRemovable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Launches probe binaries as children of a real Ghostty window.
pub struct Launcher {
    ghostty_app: PathBuf,
}

impl Launcher {
    pub fn new(ghostty_app: impl Into<PathBuf>) -> Self {
        Launcher {
            ghostty_app: ghostty_app.into(),
        }
    }

    /// The default Ghostty.app install location on macOS.
    pub fn default_macos() -> Self {
        Self::new("/Applications/Ghostty.app")
    }

    /// Spawns `probe_bin` as the foreground process of a fresh Ghostty
    /// window (`open -na <app> --args -e <probe_bin> <extra_args>`), then
    /// polls for `out_path` to appear, up to `timeout`. This is the
    /// per-probe timeout at the harness layer: independent of whatever
    /// internal timeouts the probe binary applies to individual escape-
    /// sequence queries, a hung or never-launched Ghostty window cannot
    /// hang the caller past `timeout`.
    ///
    /// Any stale file already at `out_path` is removed first so a prior
    /// run's leftover output can't be misread as this run's result. If the
    /// removal itself fails for any reason other than the file already
    /// being absent (immutable flag, read-only mount, permission
    /// mismatch...), that is reported as [`LaunchError::StaleResultNotRemovable`]
    /// rather than silently proceeding — proceeding here would risk
    /// returning the stale file's contents as this run's result.
    pub fn run_probe(
        &self,
        probe_bin: &Path,
        extra_args: &[&str],
        out_path: &Path,
        timeout: Duration,
    ) -> Result<Vec<u8>, LaunchError> {
        if let Err(source) = std::fs::remove_file(out_path)
            && source.kind() != std::io::ErrorKind::NotFound
        {
            return Err(LaunchError::StaleResultNotRemovable {
                path: out_path.to_path_buf(),
                source,
            });
        }

        let mut cmd = Command::new("open");
        cmd.arg("-na")
            .arg(&self.ghostty_app)
            .arg("--args")
            .arg("-e")
            .arg(probe_bin);
        for arg in extra_args {
            cmd.arg(arg);
        }
        let status = cmd.status().map_err(LaunchError::Spawn)?;
        if !status.success() {
            return Err(LaunchError::OpenFailed(status));
        }

        let deadline = Instant::now() + timeout;
        loop {
            if out_path.exists() {
                // The probe binary may still be mid-write when the file
                // first appears (create-then-write, not an atomic rename);
                // give it a brief moment before reading. Bounded, not a
                // second unbounded wait.
                std::thread::sleep(Duration::from_millis(150));
                return std::fs::read(out_path).map_err(LaunchError::ReadResult);
            }
            if Instant::now() >= deadline {
                return Err(LaunchError::Timeout(timeout));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Unique-per-call scratch directory, safe under `cargo test`'s
    /// parallel threads (a shared counter, not just the pid, disambiguates
    /// concurrent calls within this one test binary).
    fn scratch_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!(
            "stele-probe-launch-test-{label}-{}-{n}",
            std::process::id()
        ));
        p
    }

    /// A bogus app bundle path so any test that reaches the `open` spawn
    /// step fails fast (`open` reports "no such file" in well under a
    /// second, exit non-zero, no window shown — verified by hand before
    /// relying on it here) instead of touching a real Ghostty install.
    const BOGUS_GHOSTTY_APP: &str = "/nonexistent/NotGhostty.app";

    /// Restores write permission on, then deletes, a directory a test
    /// deliberately made read-only — runs even if the test panics via a
    /// failed assertion, so a permission-locked fixture directory can
    /// never survive a test run to poison a later one.
    struct ReadOnlyDirGuard(PathBuf);

    impl Drop for ReadOnlyDirGuard {
        fn drop(&mut self) {
            // Cleanup, not assertions: best-effort, errors are reported
            // but not propagated (there is no meaningful recovery from a
            // failed Drop).
            if let Err(e) =
                std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755))
            {
                eprintln!(
                    "test cleanup: failed to restore permissions on {:?}: {e}",
                    self.0
                );
            }
            if let Err(e) = std::fs::remove_dir_all(&self.0) {
                eprintln!("test cleanup: failed to remove {:?}: {e}", self.0);
            }
        }
    }

    /// DW-1.5 regression (review finding): a pre-existing, undeletable
    /// `out_path` must be reported as an error, never silently misread as
    /// this run's result. Against the pre-fix code this reproduces the
    /// reviewer's finding — `run_probe` fell through to attempting the
    /// launch instead of failing on the removal error.
    ///
    /// Undeletability is achieved via a read-only parent directory rather
    /// than `chflags uchg`: `chflags` doesn't exist on Linux, which is
    /// this crate's actual CI platform (`.github/workflows/ci.yml` runs
    /// `ubuntu-latest`), and removing a file is a directory-write
    /// operation on every Unix filesystem, so this is portable and needs
    /// no elevated privileges.
    #[test]
    fn stale_file_removal_failure_is_propagated_not_swallowed() {
        let dir = scratch_dir("undeletable");
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let out_path = dir.join("out.json");
        std::fs::write(&out_path, b"STALE_CONTENT_FROM_PRIOR_RUN").expect("seed stale file");

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555))
            .expect("make parent dir read-only");
        // From this point on, cleanup must happen even if an assertion
        // below panics.
        let _guard = ReadOnlyDirGuard(dir.clone());

        let launcher = Launcher::new(BOGUS_GHOSTTY_APP);
        let result = launcher.run_probe(
            Path::new("/nonexistent/probe_bin"),
            &[],
            &out_path,
            Duration::from_millis(50),
        );

        match result {
            Err(LaunchError::StaleResultNotRemovable { path, .. }) => {
                assert_eq!(path, out_path);
            }
            other => panic!(
                "expected Err(LaunchError::StaleResultNotRemovable) for an undeletable stale \
                 result file, got {other:?} — a prior run's leftover output must never be \
                 misread as this run's result"
            ),
        }
    }

    /// The fix's other half: a stale file that *can* be removed still
    /// gets removed (the guard isn't a no-op on the happy path), and its
    /// removal doesn't itself surface as a `StaleResultNotRemovable`
    /// error.
    #[test]
    fn removable_stale_file_is_actually_removed_before_launch_attempt() {
        let dir = scratch_dir("removable");
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let out_path = dir.join("out.json");
        std::fs::write(&out_path, b"STALE_CONTENT_FROM_PRIOR_RUN").expect("seed stale file");

        let launcher = Launcher::new(BOGUS_GHOSTTY_APP);
        let result = launcher.run_probe(
            Path::new("/nonexistent/probe_bin"),
            &[],
            &out_path,
            Duration::from_millis(50),
        );

        assert!(
            !matches!(result, Err(LaunchError::StaleResultNotRemovable { .. })),
            "an ordinarily-removable stale file must not surface as a removal failure, got \
             {result:?}"
        );
        assert!(
            !out_path.exists(),
            "the stale file should have been removed before the launch attempt"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The no-op case the fix must not regress: no stale file at all
    /// (`remove_file` returns `NotFound`) must not be reported as a
    /// removal failure — the file's absence is the desired post-state.
    #[test]
    fn absent_out_path_is_not_treated_as_a_removal_failure() {
        let dir = scratch_dir("absent");
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        let out_path = dir.join("out.json"); // deliberately never created

        let launcher = Launcher::new(BOGUS_GHOSTTY_APP);
        let result = launcher.run_probe(
            Path::new("/nonexistent/probe_bin"),
            &[],
            &out_path,
            Duration::from_millis(50),
        );

        assert!(
            !matches!(result, Err(LaunchError::StaleResultNotRemovable { .. })),
            "a nonexistent out_path (nothing to remove) must not be reported as a removal \
             failure, got {result:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
