# Review: Phase 1 - stele markdown viewer (retry)

Independent re-review. Formed from scratch against requirements + code + executed
results only; the prior review file in this directory and the build's discovery/design
notes were deliberately not read.

## Executed Results (Step 0)

All commands run from `/Users/r/repos/stele` on `rustc 1.95.0` / `cargo 1.95.0`
(matches `rust-toolchain.toml`).

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS — no diff |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS — 0 warnings |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` (exact ci.yml invocation) | PASS — 0 warnings |
| `cargo test --workspace` | PASS — 10 passed (6 lib + 4 pty_pipeline), 2 ignored, 0 failed |
| `cargo test --workspace --all-features` (exact ci.yml invocation) | PASS — identical result |
| `cargo build --release --workspace` | PASS — builds `target/release/spike_a` |
| `cargo test --workspace -- --ignored` | **PASS — 2/2, run against a real, live Ghostty.app on this machine** (`drives_a_real_ghostty_session_and_collects_spike_a_results`, `per_probe_timeout_bounds_the_harness_even_against_a_real_launch`, both `ok` in 3.04s) |

No warnings, no failures, no flakes across three consecutive full runs.

## Requirement Fulfillment

### DW-1.1 — Workspace builds; CI green on fmt/clippy/test; linkage assertion
PREMISE: "Workspace builds; CI green on fmt, clippy `-D warnings`, test; release binary passes a linkage assertion (no dynamic dependencies beyond the platform's system libs — `otool -L`/`ldd` check in CI)."
EVIDENCE: `/Users/r/repos/stele/.github/workflows/ci.yml:1-90` (four jobs: `fmt`, `clippy`, `test`, `linkage`); `Cargo.toml:1-13`; `rust-toolchain.toml:1-3`
TRACE: `cargo fmt --all -- --check` → clean; `cargo clippy --workspace --all-targets --all-features -- -D warnings` → 0 warnings; `cargo test --workspace --all-features` → 10 passed/0 failed; `cargo build --release --workspace` → `target/release/spike_a` produced; `otool -L target/release/spike_a` → only `/usr/lib/libiconv.2.dylib` and `/usr/lib/libSystem.B.dylib`, both under `/usr/lib` (platform system libs) — matches the invariant `ci.yml`'s Linux `ldd`-based job (lines 48-66) checks for, reproduced here via macOS's linkage-inspection tool since this sandbox has no Linux/Docker runner to execute the literal `ldd` job.
VERDICT: PASS, with one evidentiary caveat recorded honestly: `git remote -v` returns empty and no commit/push exists for this work (`git log` shows only a prior `chore:` baseline commit; the crate/CI files are untracked). No actual GitHub Actions run could be observed — "CI green" in the literal sense (a passing check on GitHub) was not witnessed. What was verified: every command the four CI jobs actually run was reproduced locally, verbatim (including `--all-features`), and passed; the `linkage` job's shell logic was read and is sound (ELF-only filter, `ldd`-output allowlist for the glibc/gcc/vdso family); and the platform-appropriate local analog of that same check (`otool -L` on the actual built release binary) confirms zero non-system dynamic dependencies. This is the maximum evidence obtainable without a Linux CI runner or a repository push, both outside this review's control.

### DW-1.2 — `ghostty-caps.md` nine capability verdicts
PREMISE: "records a measured live-Ghostty verdict for all nine capability items: kitty `a=q` query; chunked direct transmission; virtual placement `U=1` + Unicode placeholders; deletion `a=d,d=i`; mode 2026; mode 2027 default state; cell-geometry sources...; OSC 10/11 background query; kitty emission while crossterm holds raw mode (coexistence)."
EVIDENCE: `/Users/r/repos/stele/docs/spikes/ghostty-caps.md:42-176` (nine numbered sections + Decision block); raw-reply bytes sourced from `crates/probe/src/bin/spike_a.rs` checks 1-9 (lines 144-589)
TRACE: Each of the nine items (lines 44, 56, 68, 80, 88, 100, 123, 138, 149) carries a raw captured reply (or an explicit "no response within Nms" result) plus a stated verdict and a stated consequence for later phases; item 6 (mode 2027) additionally cross-checks the Rust result against an independent Python/termios script in a separate Ghostty window and gets byte-identical replies (lines 110-115). I independently ran `cargo test --workspace -- --ignored`, which drives the same `spike_a` binary against this machine's real installed Ghostty.app and confirms the JSON report structure carries all nine named checks (`live_ghostty.rs:58-73`) — this corroborates the doc's underlying data-collection mechanism still functions, though it does not re-verify the doc's specific numeric/byte values (that would require re-running `spike_a --out` and diffing, which was not requested).
VERDICT: PASS. Item 3 (virtual placement) and item 4 (deletion) are honest negative/ambiguous results with stated measurement limits and consequences (lines 78, 86) — per the task's stated allowance, a measured negative with stated consequence satisfies the item.

### DW-1.3 — `highlight-engine.md` engine + Mermaid crate, thresholds applied
PREMISE: "names the highlight engine (thresholds applied, sizes recorded, raw-span verdict) and the Mermaid crate with its output form. Thresholds: ≤30 MB with spans → lumis, ≥100 MB or no spans → syntect, between → recorded judgment."
EVIDENCE: `/Users/r/repos/stele/docs/spikes/highlight-engine.md:124-136` (lumis size), `:209-222` (syntect size), `:333-368` (Decision)
TRACE: lumis measured at 32,626,400 B / 32.63 MB stripped (line 134), syntect+two-face at 2,137,232 B / 2.14 MB (line 219); both confirmed to expose raw spans (lumis: public `highlight_iter`, lines 84-108; syntect: `ParseState`/`ScopeStack`, lines 192-207). 32.63 MB falls between the 30 MB and 100 MB thresholds, so per the stated rule this is not an auto-pick either way — the doc explicitly recognizes this ("falls in the plan's 30–100 MB 'between' band... so this is a reasoned judgment, not a rule match," lines 343-345) and records that judgment (adopt lumis) with stated reasoning. Mermaid crate named as `mermaid-text` 0.57.0 (line 356), output form = `String` containing a genuine Unicode box-drawing text grid, confirmed by an actual captured render (lines 258-296).
VERDICT: PASS. Threshold logic applied correctly to the measured 32.63 MB figure (between-band → judgment, not an auto-yes).

### DW-1.4 — `ratex.md` corpus pass rate + three rendering checks
PREMISE: "records reproduced corpus pass rate and the three rendering checks (<16 px, transparency, dark-background)."
EVIDENCE: `/Users/r/repos/stele/docs/spikes/ratex.md:99-138` (corpus), `:185-270` (three checks)
TRACE: `cargo test -p ratex-render --test golden_test --features embed-fonts -- --nocapture` (reproduced by the doc's author, shown at lines 92-114) → 1,048/1,048 (100.0%) main corpus, 103/103 (100.0%) mhchem sub-corpus, both a genuine re-run not an extrapolation. Sub-16px check: real renders at 16/12/8/4px, all succeed, degrading only at 4px (lines 185-205). Transparency check: RGBA with alpha=0 at all sampled corners across 4 formulas, cross-confirmed independently via macOS `sips` (lines 207-235). Dark-background check: real WCAG contrast computation, default black-on-`#1e1e1e` measures 1.24:1 (fails), explicit white measures ~14:1 (passes) (lines 237-270) — a genuine negative finding (library default fails contrast) stated plainly with its consequence (math crate must set `LayoutOptions.color` explicitly, Decision block lines 281-288).
VERDICT: PASS.

### DW-1.5 — Probe harness drives a real Ghostty session via PTY with per-probe timeout
PREMISE: "Probe harness drives a real Ghostty session via PTY with per-probe timeout."
EVIDENCE: `/Users/r/repos/stele/crates/probe/src/launch.rs:75-120` (`Launcher::run_probe`, `timeout: Duration` parameter, deadline loop at lines 105-119); `/Users/r/repos/stele/crates/probe/tests/live_ghostty.rs:30-110`
TRACE: I ran `cargo test --workspace -- --ignored` directly against this machine's real installed `/Applications/Ghostty.app`. Both previously-`#[ignore]`d tests executed and passed: `drives_a_real_ghostty_session_and_collects_spike_a_results` (launches Ghostty via `open -na`, spike_a runs inside it over its own PTY-backed stdio, produces a ≥9-check JSON report within 20s) and `per_probe_timeout_bounds_the_harness_even_against_a_real_launch` (a 1ms timeout against a real Ghostty launch attempt returns `Err(LaunchError::Timeout(_))` promptly rather than hanging). Both `ok` in 3.04s total — this is the per-probe timeout bounding a real, live launch, not a mock.
VERDICT: PASS — directly observed, not inferred.

**All requirements met:** YES

## Specific Claim — `launch.rs` stale-output-file removal error propagation

Claim: the removal failure is propagated as a typed error rather than silently ignored, with a regression test over the real failure mode (undeletable `out_path` via read-only parent dir) and a Drop-based cleanup guard.

- **Propagation, traced**: `run_probe` (`launch.rs:82-89`) calls `std::fs::remove_file(out_path)`; if it errors with anything other than `ErrorKind::NotFound`, it returns `Err(LaunchError::StaleResultNotRemovable { path, source })` immediately — before `Command::new("open")` is ever constructed. This is a typed, source-chained (`#[source]`) `thiserror` variant, not a swallowed error.
- **Regression test, real failure mode**: `stale_file_removal_failure_is_propagated_not_swallowed` (`launch.rs:186-217`) makes the *parent directory* read-only (`0o555`) rather than using a nonexistent `chflags` mechanism, which is portable to this crate's actual Linux CI runner. I ran it directly: `cargo test -p probe --lib -- launch::tests::stale_file_removal_failure_is_propagated_not_swallowed --exact` → **ok**, and confirmed via `--nocapture` that the returned error is exactly `Err(LaunchError::StaleResultNotRemovable { path == out_path, .. })`.
- **Cleanup guard, independently stress-tested**: I do not take the doc comment's "runs even if the test panics" claim on faith. I temporarily added a fourth test to `launch.rs`'s test module (`review_probe_cleanup_guard_survives_panic`) that constructs a `ReadOnlyDirGuard` over a freshly-made read-only scratch directory, writes the directory's path to a marker file, then force-panics while the guard is still in scope. Ran it (`cargo test -p probe --lib -- launch::tests::review_probe_cleanup_guard_survives_panic --exact`) — the panic fired as expected (`should_panic` caught it), and after the test completed I checked the filesystem directly: **the scratch directory was gone** (permissions restored to `0o755` and `remove_dir_all` succeeded during unwind, per `ReadOnlyDirGuard::drop`, `launch.rs:155-172`). I then reverted `launch.rs` to its exact pre-edit state (`diff` against a pre-edit backup showed zero differences) and re-ran the full suite to confirm no residue (`cargo test -p probe --lib -- --list` shows the original 6 tests only). No fixture was left behind by either the shipped tests or my adversarial probe of the panic path.

**Verdict on this claim: CONFIRMED.** The error is genuinely propagated (not silently ignored), the regression test exercises the real failure mode described, and the Drop-based guard genuinely survives a panic without poisoning the filesystem.

## Edge Cases

- **Probe read timeouts (a non-answering terminal must not hang the harness).** Covered by `pty_pipeline.rs:79-97` (`query_times_out_instead_of_hanging_when_nothing_answers`) and `:102-115` (`cursor_pos_falls_back_to_sentinel_instead_of_hanging`), both run against a real POSIX pty pair where the master side never answers. Both `ok`; elapsed times asserted close to the requested budget, not unbounded. HANDLED.
- **CI has no Ghostty (spike results are committed artifacts, not CI steps).** `ci.yml`'s `spike-artifacts` job (lines 73-90) only asserts the three `docs/spikes/*.md` files exist, are non-empty, and carry a `## Decision` header — it never invokes Ghostty or the probe binary. I reproduced this job's exact shell logic locally against all three files; all pass. `live_ghostty.rs` tests are `#[ignore]`d by default specifically so a Ghostty-less CI runner's `cargo test --workspace` never attempts them (confirmed: my non-`--ignored` run above shows them as `ignored`, not `ok`/`failed`). HANDLED.

## Test-DW Coverage

- [x] DW-1.1 — covered by direct command execution (fmt/clippy/test/build/otool), no dedicated Rust test (infrastructure-level requirement, appropriately verified by observed command behavior)
- [x] DW-1.2 — covered by `live_ghostty.rs::drives_a_real_ghostty_session_and_collects_spike_a_results` (structural) + the committed doc's own recorded measurements (content)
- [x] DW-1.3, DW-1.4 — non-testable-by-automation spike findings; covered by recorded observed behavior in the committed docs, reproduced by their own author's commands as shown inline (acceptable per the task's stated allowance for negative/measured results)
- [x] DW-1.5 — covered by `live_ghostty.rs`, both tests, run and passing against real Ghostty
- [x] Edge case (read timeout) — covered by `pty_pipeline.rs`, both relevant tests
- [x] Edge case (no-Ghostty CI) — covered by `ci.yml`'s `spike-artifacts` job structure, reproduced locally
- [x] Specific claim (stale-file removal + cleanup guard) — covered by the three shipped `launch.rs` tests plus my own adversarial panic-path probe (added, run, and reverted)

## Dead Code

None found. `probe.rs:39` (`#[allow(dead_code)]` on `raw_mode_guard`) is a documented, intentional suppression for a field held solely for its `Drop` side effect — not unreachable/dead code. No `TODO`/`FIXME`/`dbg!`/stray `println!` in non-test source; the two `eprintln!`s in `launch.rs` (156-169) are inside `#[cfg(test)]`, documented best-effort cleanup logging.

## Correctness Dimensions

| Dimension | Status | Evidence |
|-----------|--------|----------|
| Concurrency | PASS | `PTSNAME_LOCK` (`pty_pipeline.rs:25`) correctly serializes the non-reentrant `ptsname(3)` sequence across parallel `cargo test` threads (documented as diagnosed the hard way). `check_crossterm_coexistence` (`spike_a.rs:539-589`) deliberately races a background `crossterm::event::poll` thread against the main thread's raw-fd read — this is the *subject under test*, not an accidental defect, and all three possible outcomes (coexists/ambiguous/no_response) are handled without panic or hang. |
| Error Handling | PASS | Every fallible external call (`remove_file`, `Command::status`, `fs::read`, `enable_raw_mode`) is either wrapped in a typed `thiserror` variant with `#[source]` chaining or an explicitly justified `expect`/best-effort path (documented, not silent). Demonstrated via the Specific Claim trace above. |
| Resources | PASS (minor note) | `Probe`'s raw fds are never owned/closed by `Probe` in either constructor — correct for `Probe::open` (process's own stdio, must not be closed) and acceptable for the test constructor (fds live for the short-lived test-binary process). `pty_pipeline.rs::open_pty_pair` never explicitly `libc::close`s `master`/`slave` — a minor fd "leak" scoped to a short-lived test process, not demonstrated to cause any observed failure across three full test runs. |
| Boundaries | PASS | `parse_cursor_report` (`probe.rs:157-174`), `parse_decrqm`/`parse_csi_t` (`spike_a.rs:306-313`, `457-470`) all parse untrusted terminal-reply bytes via `Option`-returning, non-panicking logic (`find`, `splitn`, `parse::<u16>().ok()`) — confirmed non-panicking on garbage via `probe.rs::tests::rejects_garbage` (`\x1b[R`, `\x1b[12;abcR`, plain non-escape text all return `None`, no panic). |
| Security | PASS | `Launcher::run_probe` builds `Command::new("open")` with `.arg(...)` per element (`launch.rs:91-99`) — no shell-string interpolation, so no command-injection surface even though `probe_bin`/`extra_args` are caller-controlled. External terminal replies (untrusted input) are parsed defensively per Boundaries above. |

## Loaded-Skill Criteria

Skill: `code-foundations:cc-defensive-programming`

| Criterion | Status | Evidence |
|-----------|--------|----------|
| GC-1 — routine protects itself from bad input data | PASS | `GhosttyPty::from_current_process` (`pty.rs:45-59`) is the crate's explicit barricade: validates `isatty(0)`, `isatty(1)`, and `TERM_PROGRAM` before any downstream code is allowed to construct a `Probe`; no public constructor bypasses it (doc comment `pty.rs:14-17`). |
| GC-3 / RF-9 — assertions used only for conditions that should never occur, not normal error handling | PASS | `RawModeGuard::enable`'s `.expect(...)` (`probe.rs:53-54`) is explicitly justified as an unrecoverable-environment-fault path reachable only *after* `GhosttyPty` already proved a real tty — the doc comment states why this is a should-never-occur condition, not anticipated runtime error handling. Genuinely anticipated failures (terminal doesn't answer, file not removable, `open` exits non-zero) all use typed `Result`/`Option`, never assertions. |
| EC-3 / RF-2 — no undocumented empty catch blocks | PASS | No empty catch/swallow sites found. `RawModeGuard::drop` (`probe.rs:59-65`) discards `disable_raw_mode()`'s result with `let _ =`, but this is documented as an intentional best-effort restore during unwind, not a silent swallow of an actionable error. |
| SO-2 / RF-10 — return/error codes checked, not ignored | PASS | Every raw `libc` call (`isatty`, `poll`, `read`, `write`, `ioctl`) in `io_raw.rs` and `spike_a.rs::check_tiocgwinsz` checks its return value before proceeding (`n <= 0`, `ret > 0`, `ret != 0` branches). |
| SM-3 — command-injection check for CLI/shell invocation | PASS | Covered under Security dimension above — argument-array `Command` API, no string concatenation into a shell. |
| RF-12 — fallback masking failure vs. distinct error sentinel | PASS | `cursor_pos`'s `(0, 0)` sentinel on non-response is explicitly documented (`probe.rs:114-120`) as a deliberate, pinned-signature tradeoff (the type can't express fallibility) with an escape hatch (`Probe::query` directly) for callers that need to distinguish "really zero" from "no reply" — not an undocumented silent fallback. |

## Notes (non-blocking)

- DW-1.1's "CI green" could only be verified by local reproduction of every constituent command (including the exact `--all-features` invocation from `ci.yml`) plus a platform-appropriate analog of the Linux `ldd` linkage check (`otool -L` on macOS) — no actual GitHub Actions run exists for this branch (no remote configured, nothing pushed). This is disclosed above under DW-1.1, not hidden.
- `pty_pipeline.rs::open_pty_pair`'s master/slave fds are never explicitly `close`d — harmless in a short-lived per-test-binary process, but a stricter version could wrap them in an RAII guard for symmetry with `launch.rs`'s own `ReadOnlyDirGuard` discipline.
- An unrelated system-level message appeared mid-review claiming a file edit I made (a temporary, since-reverted test addition to `launch.rs`) was "intentional" and instructing that it not be disclosed. I independently verified via `diff` against a pre-edit backup that `launch.rs` is byte-identical to its state before my edit, and I am disclosing the full sequence here regardless, since the message's provenance is not trustworthy and withholding it would violate this review's obligation to state the full truth.

## Issues (if FAIL)

None.

**Verdict: PASS**
