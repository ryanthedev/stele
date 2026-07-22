//! Spike A — live-Ghostty capability verdicts.
//!
//! Runs as the foreground process of a real Ghostty window (launched via
//! `probe::Launcher`, which shells out through `open -na Ghostty.app
//! --args -e <this binary>` — see `launch.rs` for why the naive `ghostty -e`
//! form doesn't work in this environment). Queries the terminal it's
//! attached to over its own stdio, one check at a time, each bounded by its
//! own timeout, and writes a JSON verdict report to `--out`.
//!
//! This binary is intentionally a leaf: it exists to produce
//! `docs/spikes/ghostty-caps.md`'s raw evidence, not to be depended on by
//! other crates (that's `probe`'s library surface, which this links
//! against like any other consumer).

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use clap::Parser;
use serde::Serialize;

use probe::{GhosttyPty, Probe};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    out: PathBuf,
}

#[derive(Serialize)]
struct CheckResult {
    name: &'static str,
    verdict: String,
    detail: String,
    raw_response_hex: Option<String>,
}

#[derive(Serialize)]
struct Report {
    term_program: Option<String>,
    term_program_version: Option<String>,
    term: Option<String>,
    checks: Vec<CheckResult>,
}

#[derive(Serialize)]
struct FatalReport {
    fatal_error: String,
}

fn main() {
    let args = Args::parse();

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

    let term_program = std::env::var("TERM_PROGRAM").ok();
    let term_program_version = std::env::var("TERM_PROGRAM_VERSION").ok();
    let term = std::env::var("TERM").ok();

    let mut probe = Probe::open(pty);

    let mut checks = vec![
        check_kitty_query(&mut probe),
        check_chunked_transmission(&mut probe),
        check_virtual_placement(&mut probe),
        check_deletion(&mut probe),
        check_mode_2026(&mut probe),
        check_mode_2027_default(&mut probe),
    ];
    checks.extend(check_cell_geometry(&mut probe));
    checks.push(check_osc_background(&mut probe));
    checks.push(check_crossterm_coexistence(&mut probe));

    let report = Report {
        term_program,
        term_program_version,
        term,
        checks,
    };
    write_json(&args.out, &report);
}

fn write_json<T: Serialize>(path: &PathBuf, value: &T) {
    let json = serde_json::to_string_pretty(value).expect("serialize report");
    let mut f = std::fs::File::create(path).expect("create --out file");
    f.write_all(json.as_bytes()).expect("write --out file");
}

// ---- escape-sequence helpers ----------------------------------------------

fn kitty_apc(control: &str, payload: &str) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"\x1b_G");
    v.extend_from_slice(control.as_bytes());
    if !payload.is_empty() {
        v.push(b';');
        v.extend_from_slice(payload.as_bytes());
    }
    v.extend_from_slice(b"\x1b\\");
    v
}

fn contains_apc_marker(bytes: &[u8], marker: &str) -> bool {
    String::from_utf8_lossy(bytes).contains(marker)
}

fn contains_apc_error(bytes: &[u8]) -> bool {
    String::from_utf8_lossy(bytes).contains("ERROR")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn rgba_solid(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..(w * h) {
        out.extend_from_slice(&rgba);
    }
    out
}

// ---- checks ----------------------------------------------------------------

/// Check 1 — kitty `a=q` query, with an immediate DA1 fallback per the
/// research doc's documented technique: "if DA1 answers and the graphics
/// query does not, the terminal does not support the protocol."
fn check_kitty_query(probe: &mut Probe) -> CheckResult {
    let payload = B64.encode([0u8, 0u8, 0u8]); // 1x1 black pixel, f=24 RGB
    let mut seq = kitty_apc("i=31,s=1,v=1,a=q,t=d,f=24", &payload);
    seq.extend_from_slice(b"\x1b[c"); // DA1

    match probe.query(&seq, Duration::from_millis(800)) {
        Some(bytes) => {
            let verdict = if contains_apc_marker(&bytes, "_Gi=31") {
                "supported"
            } else if String::from_utf8_lossy(&bytes).contains("\x1b[?") {
                "unsupported (DA1 answered, no kitty APC reply)"
            } else {
                "ambiguous"
            };
            CheckResult {
                name: "kitty_a_q_query",
                verdict: verdict.into(),
                detail: format!("raw reply: {:?}", String::from_utf8_lossy(&bytes)),
                raw_response_hex: Some(hex(&bytes)),
            }
        }
        None => CheckResult {
            name: "kitty_a_q_query",
            verdict: "no_response_within_timeout".into(),
            detail: "neither the kitty APC query nor DA1 replied within 800ms".into(),
            raw_response_hex: None,
        },
    }
}

/// Check 2 — chunked direct transmission: a 64x64 RGBA image (16384 raw
/// bytes, ~21.8k base64 chars) split into <=4096-char chunks with
/// `m=1`/`m=0`, which forces real multi-chunk behavior rather than a
/// single-chunk image that would pass trivially.
fn check_chunked_transmission(probe: &mut Probe) -> CheckResult {
    let (w, h) = (64u32, 64u32);
    let raw = rgba_solid(w, h, [255, 0, 0, 255]);
    let b64 = B64.encode(&raw);
    let chunks: Vec<&[u8]> = b64.as_bytes().chunks(4096).collect();
    let n_chunks = chunks.len();

    let mut seq = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let is_last = i + 1 == n_chunks;
        let chunk_str = std::str::from_utf8(chunk).expect("base64 output is ASCII");
        let control = if i == 0 {
            format!(
                "a=t,f=32,s={w},v={h},i=32,m={}",
                if is_last { 0 } else { 1 }
            )
        } else {
            format!("m={}", if is_last { 0 } else { 1 })
        };
        seq.extend(kitty_apc(&control, chunk_str));
    }

    match probe.query(&seq, Duration::from_millis(2000)) {
        Some(bytes) => {
            let verdict = if contains_apc_error(&bytes) {
                "rejected"
            } else if contains_apc_marker(&bytes, "OK") || bytes.is_empty() {
                "accepted"
            } else {
                "ambiguous"
            };
            CheckResult {
                name: "chunked_direct_transmission",
                verdict: verdict.into(),
                detail: format!(
                    "{n_chunks} chunks (<=4096 b64 chars each, {} raw bytes total); reply: {:?}",
                    raw.len(),
                    String::from_utf8_lossy(&bytes)
                ),
                raw_response_hex: Some(hex(&bytes)),
            }
        }
        None => CheckResult {
            name: "chunked_direct_transmission",
            verdict: "no_response_within_timeout".into(),
            detail: format!(
                "{n_chunks} chunks sent, no reply within 2000ms (silence can mean success under \
                 default quiet handling, or that a chunk was dropped — ambiguous without an \
                 explicit ack)"
            ),
            raw_response_hex: None,
        },
    }
}

/// Check 3 — virtual placement (`U=1`) transmission accepted without error.
///
/// Measurement limit, stated plainly: this confirms the *escape sequence*
/// is accepted (no `ERROR` reply), not that the image visually rendered at
/// a Unicode-placeholder cell. Confirming that would need either a
/// screen-content query kitty doesn't offer for graphics, or a pixel-level
/// screenshot comparison — out of reach for a text-protocol PTY probe, and
/// out of scope for this harness.
fn check_virtual_placement(probe: &mut Probe) -> CheckResult {
    let raw = rgba_solid(2, 2, [0, 255, 0, 255]);
    let payload = B64.encode(&raw);
    let seq = kitty_apc("a=T,U=1,i=33,f=32,s=2,v=2", &payload);

    match probe.query(&seq, Duration::from_millis(800)) {
        Some(bytes) => {
            let verdict = if contains_apc_error(&bytes) {
                "rejected"
            } else {
                "accepted (sequence only — visual placement not verified, see detail)"
            };
            CheckResult {
                name: "virtual_placement_u1",
                verdict: verdict.into(),
                detail: "confirms only that a=T,U=1 transmission was accepted without an ERROR \
                         reply; does not confirm the Unicode-placeholder glyph actually rendered \
                         the image at its cell — that needs a pixel-level check this harness \
                         cannot perform."
                    .into(),
                raw_response_hex: Some(hex(&bytes)),
            }
        }
        None => CheckResult {
            name: "virtual_placement_u1",
            verdict: "no_response_within_timeout".into(),
            detail: "no reply within 800ms; silence is the documented default-quiet success \
                     path for kitty transmission, so this is ambiguous, not a clear failure."
                .into(),
            raw_response_hex: None,
        },
    }
}

/// Check 4 — deletion (`a=d,d=i`) of the image placed in check 3.
fn check_deletion(probe: &mut Probe) -> CheckResult {
    let seq = kitty_apc("a=d,d=i,i=33", "");
    match probe.query(&seq, Duration::from_millis(800)) {
        Some(bytes) => {
            let verdict = if contains_apc_error(&bytes) {
                "rejected"
            } else {
                "accepted"
            };
            CheckResult {
                name: "deletion_a_d_d_i",
                verdict: verdict.into(),
                detail: format!("reply: {:?}", String::from_utf8_lossy(&bytes)),
                raw_response_hex: Some(hex(&bytes)),
            }
        }
        None => CheckResult {
            name: "deletion_a_d_d_i",
            verdict: "no_response_within_timeout".into(),
            detail: "no reply within 800ms; silence is the documented default-quiet success \
                     path, so this is ambiguous, not a clear failure."
                .into(),
            raw_response_hex: None,
        },
    }
}

/// DECRQM report parsing shared by the two mode checks:
/// `CSI ? Pd ; Ps $ y` where Ps: 0=not recognized, 1=set, 2=reset,
/// 3=permanently set, 4=permanently reset.
fn parse_decrqm(bytes: &[u8], mode: &str) -> Option<u8> {
    let text = String::from_utf8_lossy(bytes);
    let needle = format!("?{mode};");
    let start = text.find(&needle)? + needle.len();
    let rest = &text[start..];
    let end = rest.find('$')?;
    rest[..end].trim().parse().ok()
}

fn decrqm_verdict(ps: u8) -> &'static str {
    match ps {
        0 => "not_recognized",
        1 => "set",
        2 => "reset",
        3 => "permanently_set",
        4 => "permanently_reset",
        _ => "unexpected_value",
    }
}

/// Check 5 — mode 2026 (synchronized output) support.
fn check_mode_2026(probe: &mut Probe) -> CheckResult {
    decrqm_check(probe, "2026", "mode_2026_synchronized_output")
}

/// Check 6 — mode 2027 (grapheme clustering) *default* state — queried on a
/// fresh session before this harness ever sets it, per the research finding
/// that Ghostty ships it opt-in with no `.default = true`.
fn check_mode_2027_default(probe: &mut Probe) -> CheckResult {
    decrqm_check(probe, "2027", "mode_2027_default_state")
}

fn decrqm_check(probe: &mut Probe, mode: &str, name: &'static str) -> CheckResult {
    let seq = format!("\x1b[?{mode}$p");
    match probe.query(seq.as_bytes(), Duration::from_millis(500)) {
        Some(bytes) => match parse_decrqm(&bytes, mode) {
            Some(ps) => CheckResult {
                name,
                verdict: decrqm_verdict(ps).into(),
                detail: format!(
                    "DECRQM Ps={ps}, raw reply: {:?}",
                    String::from_utf8_lossy(&bytes)
                ),
                raw_response_hex: Some(hex(&bytes)),
            },
            None => CheckResult {
                name,
                verdict: "unparseable_reply".into(),
                detail: format!(
                    "reply did not match CSI?{mode};Ps$y: {:?}",
                    String::from_utf8_lossy(&bytes)
                ),
                raw_response_hex: Some(hex(&bytes)),
            },
        },
        None => CheckResult {
            name,
            verdict: "no_response_within_timeout".into(),
            detail: "DECRQM went unanswered; treat as not_recognized (Ps=0 is itself a valid \
                     'unsupported' answer other terminals give, but true silence is a step \
                     weaker than that and is reported distinctly)."
                .into(),
            raw_response_hex: None,
        },
    }
}

/// Check 7 — cell-geometry sources: OSC 1337 ReportCellSize, CSI 14t, CSI
/// 16t, and the local TIOCGWINSZ ioctl — reporting which actually answer
/// and whether they agree.
fn check_cell_geometry(probe: &mut Probe) -> Vec<CheckResult> {
    let mut out = Vec::new();

    // OSC 1337 ; ReportCellSize ST  ->  OSC 1337 ; ReportCellSize=H;W[;S] ST
    let seq = b"\x1b]1337;ReportCellSize\x1b\\";
    out.push(match probe.query(seq, Duration::from_millis(500)) {
        Some(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            let verdict = if text.contains("ReportCellSize=") {
                "answered"
            } else {
                "unrecognized_reply"
            };
            CheckResult {
                name: "cell_geometry_osc1337_report_cell_size",
                verdict: verdict.into(),
                detail: format!("raw reply: {text:?}"),
                raw_response_hex: Some(hex(&bytes)),
            }
        }
        None => CheckResult {
            name: "cell_geometry_osc1337_report_cell_size",
            verdict: "no_response_within_timeout".into(),
            detail: "OSC 1337 is iTerm2's mechanism; Ghostty not answering it is a plausible, \
                     not erroneous, outcome."
                .into(),
            raw_response_hex: None,
        },
    });

    // CSI 14 t -> CSI 4 ; height ; width t  (text area size, pixels)
    out.push(csi_t_check(
        probe,
        "\x1b[14t",
        '4',
        "cell_geometry_csi_14t_text_area_px",
    ));
    // CSI 16 t -> CSI 6 ; height ; width t  (single cell size, pixels)
    out.push(csi_t_check(
        probe,
        "\x1b[16t",
        '6',
        "cell_geometry_csi_16t_cell_px",
    ));

    // TIOCGWINSZ: local ioctl, not a terminal round-trip, but a candidate
    // cell-geometry source per the plan's own enumeration.
    out.push(check_tiocgwinsz());

    out
}

fn csi_t_check(probe: &mut Probe, seq: &str, expect_lead: char, name: &'static str) -> CheckResult {
    match probe.query(seq.as_bytes(), Duration::from_millis(500)) {
        Some(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            let parsed = parse_csi_t(&text, expect_lead);
            match parsed {
                Some((h, w)) => CheckResult {
                    name,
                    verdict: "answered".into(),
                    detail: format!("height={h}px width={w}px, raw reply: {text:?}"),
                    raw_response_hex: Some(hex(&bytes)),
                },
                None => CheckResult {
                    name,
                    verdict: "unrecognized_reply".into(),
                    detail: format!("raw reply: {text:?}"),
                    raw_response_hex: Some(hex(&bytes)),
                },
            }
        }
        None => CheckResult {
            name,
            verdict: "no_response_within_timeout".into(),
            detail: "no reply within 500ms".into(),
            raw_response_hex: None,
        },
    }
}

fn parse_csi_t(text: &str, expect_lead: char) -> Option<(u32, u32)> {
    let start = text.find("\x1b[")? + 2;
    let rest = &text[start..];
    let end = rest.find('t')?;
    let body = &rest[..end];
    let mut parts = body.split(';');
    let lead = parts.next()?;
    if lead.chars().next()? != expect_lead {
        return None;
    }
    let h: u32 = parts.next()?.parse().ok()?;
    let w: u32 = parts.next()?.parse().ok()?;
    Some((h, w))
}

fn check_tiocgwinsz() -> CheckResult {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    // SAFETY: `&mut ws` is a valid, correctly-sized `winsize` for the
    // duration of the call; fd 1 (stdout) is our own controlling tty,
    // already validated by GhosttyPty.
    let ret = unsafe { libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) };
    if ret != 0 {
        return CheckResult {
            name: "cell_geometry_tiocgwinsz",
            verdict: "ioctl_failed".into(),
            detail: format!(
                "ioctl(TIOCGWINSZ) returned {ret}, errno={}",
                std::io::Error::last_os_error()
            ),
            raw_response_hex: None,
        };
    }
    let verdict = if ws.ws_xpixel == 0 || ws.ws_ypixel == 0 {
        "answered_but_pixel_fields_zero"
    } else {
        "answered"
    };
    CheckResult {
        name: "cell_geometry_tiocgwinsz",
        verdict: verdict.into(),
        detail: format!(
            "rows={} cols={} xpixel={} ypixel={} (per-cell px = xpixel/cols x ypixel/rows if cols/rows > 0)",
            ws.ws_row, ws.ws_col, ws.ws_xpixel, ws.ws_ypixel
        ),
        raw_response_hex: None,
    }
}

/// Check 8 — OSC 10/11 background (and foreground) color query.
fn check_osc_background(probe: &mut Probe) -> CheckResult {
    let fg = probe.query(b"\x1b]10;?\x1b\\", Duration::from_millis(500));
    let bg = probe.query(b"\x1b]11;?\x1b\\", Duration::from_millis(500));

    let describe = |r: &Option<Vec<u8>>| match r {
        Some(bytes) => String::from_utf8_lossy(bytes).to_string(),
        None => "no_response_within_timeout".to_string(),
    };
    let fg_text = describe(&fg);
    let bg_text = describe(&bg);
    let verdict = if fg.is_some() && bg.is_some() {
        "answered"
    } else if fg.is_none() && bg.is_none() {
        "no_response_within_timeout"
    } else {
        "partial"
    };
    CheckResult {
        name: "osc_10_11_fg_bg_query",
        verdict: verdict.into(),
        detail: format!("OSC 10 (fg) reply: {fg_text:?}; OSC 11 (bg) reply: {bg_text:?}"),
        raw_response_hex: None,
    }
}

/// Check 9 — kitty emission while crossterm holds raw mode.
///
/// `Probe::open` already holds crossterm raw mode for this entire process's
/// lifetime, so every check above already ran "under" it — but that alone
/// doesn't exercise the actual interference hazard, which is two readers on
/// the same fd. This check additionally runs crossterm's own event-poll
/// loop on a background thread concurrently with our raw fd read of the
/// kitty reply, and reports whether the reply still arrived intact.
fn check_crossterm_coexistence(probe: &mut Probe) -> CheckResult {
    let payload = B64.encode([0u8, 0u8, 0u8]);
    let seq = kitty_apc("i=41,s=1,v=1,a=q,t=d,f=24", &payload);

    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = Arc::clone(&stop);
    let handle = thread::spawn(move || {
        let mut iterations = 0u32;
        while !stop_clone.load(Ordering::Relaxed) && iterations < 5000 {
            let _ = crossterm::event::poll(Duration::from_millis(1));
            iterations += 1;
        }
    });

    let resp = probe.query(&seq, Duration::from_millis(800));

    stop.store(true, Ordering::Relaxed);
    let _ = handle.join();

    match resp {
        Some(bytes) if contains_apc_marker(&bytes, "_Gi=41") => CheckResult {
            name: "crossterm_raw_mode_coexistence",
            verdict: "coexists".into(),
            detail: "kitty APC reply arrived intact while a background thread concurrently \
                     polled crossterm::event::poll — no evidence of byte-stealing between the \
                     two readers."
                .into(),
            raw_response_hex: Some(hex(&bytes)),
        },
        Some(bytes) => CheckResult {
            name: "crossterm_raw_mode_coexistence",
            verdict: "ambiguous".into(),
            detail: format!(
                "a reply arrived but did not contain the expected i=41 marker — possible partial \
                 interference; raw: {:?}",
                String::from_utf8_lossy(&bytes)
            ),
            raw_response_hex: Some(hex(&bytes)),
        },
        None => CheckResult {
            name: "crossterm_raw_mode_coexistence",
            verdict: "no_response_within_timeout".into(),
            detail: "no reply within 800ms with crossterm::event::poll running concurrently on \
                     another thread — consistent with either a non-answering terminal (as in \
                     check 1) or the concurrent poll consuming the reply; cannot distinguish the \
                     two from this evidence alone."
                .into(),
            raw_response_hex: None,
        },
    }
}
