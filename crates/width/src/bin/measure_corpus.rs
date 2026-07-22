//! The corpus-measurement tool for DW-3.1 — separate from the `width`
//! engine (which never touches a terminal): this binary runs as the
//! foreground process of a real Ghostty window (via `probe::Launcher`,
//! exactly like `probe`'s own `spike_a`), asks Ghostty itself how wide
//! each cluster in `corpus/cases.json` actually renders, and writes the
//! pinned, committed verdict artifact.
//!
//! Only built under the `corpus-tool` feature — see `Cargo.toml` — so a
//! normal `width` build never compiles or links `probe`.
//!
//! Usage (run once per Ghostty version, from outside any live Ghostty
//! session — `tests/live_ghostty_corpus.rs` drives this via
//! `probe::Launcher`, following the existing `crates/probe` convention):
//!
//! ```sh
//! cargo run -p width --bin measure_corpus --features corpus-tool -- \
//!     --cases corpus/cases.json --out /tmp/measured.json
//! ```

use std::io::Write as _;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use probe::{GhosttyPty, Probe};

#[derive(Deserialize)]
struct CaseInput {
    id: String,
    category: String,
    codepoints: Vec<String>,
    cluster: String,
}

#[derive(Deserialize)]
struct CasesFile {
    cases: Vec<CaseInput>,
}

#[derive(Serialize)]
struct MeasuredCase {
    id: String,
    category: String,
    codepoints: Vec<String>,
    cluster: String,
    measured_width: u16,
}

#[derive(Serialize)]
struct MeasuredCorpus {
    ghostty_version: Option<String>,
    term_program: Option<String>,
    term: Option<String>,
    cases: Vec<MeasuredCase>,
}

#[derive(Serialize)]
struct FatalReport {
    fatal_error: String,
}

/// Bare-bones flag parsing (`--cases <path> --out <path>`) — this tool has
/// exactly two required arguments, so a dependency on `clap` would be
/// over-general for it.
struct Args {
    cases: PathBuf,
    out: PathBuf,
}

fn parse_args() -> Args {
    let argv: Vec<String> = std::env::args().collect();
    let mut cases = None;
    let mut out = None;
    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--cases" => {
                cases = argv.get(i + 1).cloned();
                i += 2;
            }
            "--out" => {
                out = argv.get(i + 1).cloned();
                i += 2;
            }
            _ => i += 1,
        }
    }
    Args {
        cases: PathBuf::from(cases.expect("--cases <path> is required")),
        out: PathBuf::from(out.expect("--out <path> is required")),
    }
}

fn write_json<T: Serialize>(path: &std::path::Path, value: &T) {
    let bytes = serde_json::to_vec_pretty(value).expect("serialize report");
    let mut f = std::fs::File::create(path).expect("create output file");
    f.write_all(&bytes).expect("write output file");
}

fn main() {
    let args = parse_args();

    let pty = match GhosttyPty::from_current_process() {
        Ok(pty) => pty,
        Err(e) => {
            write_json(
                &args.out,
                &FatalReport {
                    fatal_error: e.to_string(),
                },
            );
            std::process::exit(1);
        }
    };

    let cases_bytes = std::fs::read(&args.cases).expect("read --cases file");
    let cases_file: CasesFile = serde_json::from_slice(&cases_bytes).expect("parse --cases JSON");

    let term_program = std::env::var("TERM_PROGRAM").ok();
    let term_program_version = std::env::var("TERM_PROGRAM_VERSION").ok();
    let term = std::env::var("TERM").ok();

    let mut probe = Probe::open(pty);

    let measured: Vec<MeasuredCase> = cases_file
        .cases
        .into_iter()
        .map(|c| {
            // Reset to column 0 before every measurement so a long corpus
            // run never wraps the line — `Probe::measured_width` reports a
            // delta, and a delta across a wrap point is meaningless (see
            // its own doc comment on single-cluster/short-run intent).
            // Its own return value is discarded: only the cursor-reset
            // side effect matters here.
            let _ = probe.measured_width("\r");
            let width = probe.measured_width(&c.cluster);
            MeasuredCase {
                id: c.id,
                category: c.category,
                codepoints: c.codepoints,
                cluster: c.cluster,
                measured_width: width,
            }
        })
        .collect();

    write_json(
        &args.out,
        &MeasuredCorpus {
            ghostty_version: term_program_version,
            term_program,
            term,
            cases: measured,
        },
    );
}
