//! DW-1.5, end to end: `Launcher` actually drives a real Ghostty session
//! via a PTY (the `spike_a` binary running as Ghostty's foreground child),
//! bounded by a per-probe timeout.
//!
//! **Ignored by default.** CI has no Ghostty (the plan's own stated edge
//! case), and this needs a real macOS GUI session with Ghostty installed —
//! see `docs/spikes/ghostty-caps.md` for the launch pattern this exercises
//! and why the naive `ghostty -e` form doesn't work. Run locally:
//!
//! ```sh
//! cargo test -p probe --test live_ghostty -- --ignored
//! ```

use std::path::PathBuf;
use std::time::{Duration, Instant};

use probe::{LaunchError, Launcher};

fn scratch_out_path(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "stele-probe-live-ghostty-{label}-{}.json",
        std::process::id()
    ));
    p
}

/// The positive path: a real Ghostty window, driven end to end, produces
/// the full Spike-A report.
#[test]
#[ignore = "requires a real Ghostty install and an interactive GUI session"]
fn drives_a_real_ghostty_session_and_collects_spike_a_results() {
    let launcher = Launcher::default_macos();
    let probe_bin = PathBuf::from(env!("CARGO_BIN_EXE_spike_a"));
    let out_path = scratch_out_path("results");
    let _ = std::fs::remove_file(&out_path);

    let bytes = launcher
        .run_probe(
            &probe_bin,
            &["--out", out_path.to_str().unwrap()],
            &out_path,
            Duration::from_secs(20),
        )
        .expect("Launcher should drive a real Ghostty session to completion within 20s");

    let json: serde_json::Value =
        serde_json::from_slice(&bytes).expect("spike_a must emit valid JSON");
    let checks = json["checks"]
        .as_array()
        .expect("report must have a top-level `checks` array");
    assert!(
        checks.len() >= 9,
        "expected at least the plan's nine Spike-A capability items, got {}: {json:#}",
        checks.len()
    );

    let names: Vec<&str> = checks.iter().filter_map(|c| c["name"].as_str()).collect();
    for expected in [
        "kitty_a_q_query",
        "chunked_direct_transmission",
        "virtual_placement_u1",
        "deletion_a_d_d_i",
        "mode_2026_synchronized_output",
        "mode_2027_default_state",
        "osc_10_11_fg_bg_query",
        "crossterm_raw_mode_coexistence",
    ] {
        assert!(
            names.contains(&expected),
            "missing expected check {expected:?} among {names:?}"
        );
    }

    let _ = std::fs::remove_file(&out_path);
}

/// The defensive edge case DW-1.5 exists to prove: even a real Ghostty
/// launch is bounded by the harness's own timeout. An unrealistically
/// short budget (1ms — no real Ghostty window can open, run nine escape-
/// sequence round trips, and write a result file in that time) must
/// produce a prompt `LaunchError::Timeout`, not a hang.
#[test]
#[ignore = "requires a real Ghostty install and an interactive GUI session"]
fn per_probe_timeout_bounds_the_harness_even_against_a_real_launch() {
    let launcher = Launcher::default_macos();
    let probe_bin = PathBuf::from(env!("CARGO_BIN_EXE_spike_a"));
    let out_path = scratch_out_path("timeout");
    let _ = std::fs::remove_file(&out_path);

    let start = Instant::now();
    let result = launcher.run_probe(
        &probe_bin,
        &["--out", out_path.to_str().unwrap()],
        &out_path,
        Duration::from_millis(1),
    );
    let elapsed = start.elapsed();

    assert!(
        matches!(result, Err(LaunchError::Timeout(_))),
        "expected Err(LaunchError::Timeout) with a 1ms budget, got {result:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "the harness must bail out near the requested timeout rather than hang; took {elapsed:?}"
    );

    let _ = std::fs::remove_file(&out_path);
}
