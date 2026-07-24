//! Stamps the binary with the commit it was built from.
//!
//! Exists for one reason: telling "the fix isn't working" apart from "you are
//! running yesterday's binary." A viewer whose whole output is pixels in a
//! terminal gives you nothing to grep, so the build has to say what it is.
//!
//! `STELE_BUILD` ends up as `<sha>[-dirty] <utc build time>`, or
//! `unknown-build` if git is unavailable (a source tarball, a vendored build)
//! — never a build failure.

use std::process::Command;

fn main() {
    // Re-stamp when HEAD moves (new commit, branch switch) or the index
    // changes (staging edits flips the dirty flag).
    for path in [
        "../../.git/HEAD",
        "../../.git/index",
        "../../.git/refs/heads",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }

    let sha = run(&["rev-parse", "--short=7", "HEAD"]);
    let dirty = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .ok()
        .is_some_and(|o| !o.stdout.is_empty());

    let stamp = match sha {
        Some(sha) if dirty => format!("{sha}-dirty"),
        Some(sha) => sha,
        None => "unknown-build".to_string(),
    };

    // Build time, so two builds of the same commit are still tellable apart.
    // `date` rather than a crate — this is a stamp, not a dependency.
    let when = run(&[]).unwrap_or_default();
    let when = if when.is_empty() {
        Command::new("date")
            .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    } else {
        when
    };

    println!("cargo:rustc-env=STELE_BUILD={stamp} {when}");
    println!("cargo:rustc-env=STELE_BUILD_SHA={stamp}");
}

/// Runs `git <args>`, returning trimmed stdout on success. An empty `args`
/// means "no git call" — used to keep the build-time path separate.
fn run(args: &[&str]) -> Option<String> {
    if args.is_empty() {
        return None;
    }
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let s = s.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}
