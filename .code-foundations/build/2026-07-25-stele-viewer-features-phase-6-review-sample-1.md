# Review: Phase 6 — mouse, link following, and clipboard

Worktree `.code-foundations/wave-worktrees/phase-6`, change under review `git diff 1c4192f..HEAD` (single commit `c8e35f1`).

## Executed Results (Step 0)

Run from inside a controlling terminal (`script -q /dev/null`).

- Test suite: `cargo test --workspace` → **664 passed, 0 failed, 5 ignored**, exit 0
- Typecheck/lint: `cargo clippy --workspace --all-targets` → clean, no warnings
- Format: `cargo fmt --all -- --check` → clean

All 8 DW items carry executed automated tests (stele-scoped names; note the cross-crate DW-ID collision recorded under Notes): DW-6.1 ×15, 6.2 ×14, 6.3 ×15, 6.4 ×14, 6.5 ×9, 6.6 ×11, 6.7 ×9, 6.8 ×5.

## Requirement Fulfillment

### DW-6.1
PREMISE:  "`Tab`/`Shift-Tab` cycle the links visible in the viewport with a visible selection indicator; `Enter` activates the selected one."
EVIDENCE: `app.rs:1065` (`visible_links`), `:1125` (`enter_link_select`), `:1144` (`cycle_link`), `:1188` (`activate_selection`), `:1472` (`handle_link_select_key`); `painter.rs:97` `SGR_REVERSE`, `:759` emission.
TRACE:    `Tab` → `enter_link_select(true)` → `Mode::LinkSelect{index:0}` → painter writes `\x1b[7m` around the link's runs → `Shift-Tab`/`BackTab` → `cycle_link(index,false)` → wraps `(index+count-1)%count` → `Enter` → `activate_selection` leaves the mode *then* queues `PendingAction::OpenLink`.
EXECUTED: `test_dw_6_1_tab_selects_a_link_through_the_real_binarys_key_path` drives the real binary over a pty and asserts the reverse-video run really contains the link glyphs (not merely that the attribute was emitted). My own probe cycled a 373-link viewport 751 times in each direction plus 200 click columns with no panic.
VERDICT:  PASS

### DW-6.2
PREMISE:  "A relative markdown link opens in place and `Backspace` returns to the previous document at its previous scroll position."
EVIDENCE: `link.rs:552` (`follow`), `:589` (`back`), `:426` (`DocumentStack`); `main.rs:466`, `:480`, `:421` (`install_document`).
TRACE:    `Enter` → `follow("second.md", current, scroll())` → push `(current, scroll)` **after** the target loads → `install_document(..., 0)`. `Backspace` → `back()` → `pop()` → reload → `install_document(..., entry.scroll)`.
EXECUTED: `test_dw_6_2_enter_opens_a_relative_markdown_link_and_backspace_returns_to_the_scroll_position` compares the returned screen byte-for-byte against the departure screen, from a genuinely scrolled position. Root `Backspace` reports "already at the first document".
VERDICT:  **PASS on the stated behaviour, but see Issue 1** — the same `back()` call is where the blocking defect lives. The requirement's own sentence is satisfied; the code path it names hangs on a hostile file type.

### DW-6.3
PREMISE:  "An `http`/`https` link is handed to the OS opener via argv with no shell involved; a non-`http(s)` scheme is refused with a status message."
EVIDENCE: `link.rs:483` (`opener_argv`), `:491` (`SystemOpener::open`), `:321` (`validated_url`), `:74` (`ACTIVATABLE_SCHEMES`).
TRACE:    `Command::new(OPENER_PROGRAM).args([url])` — `execvp`, no interpreter anywhere on the path. Non-allowlisted scheme → `LinkError::UnsupportedScheme` → `main.rs:476` → status row.
EXECUTED: **Recorded bytes, not intent.** I put a logging shim named `open`/`xdg-open` first on `PATH` and ran the real `SystemOpener`. Every hostile URL arrived as `argc=1` with the URL verbatim:
```
argv[1]=<http://x/$(id)>
argv[1]=<http://x/;id>
argv[1]=<http://x/`id`>
argv[1]=<https://x/a b&&touch /tmp/pwned-by-stele>
argv[1]=<https://x/|tee /tmp/pwned2>
```
No expansion, no word-splitting, and `/tmp/pwned-by-stele` and `/tmp/pwned2` do not exist afterwards. `http://x/\nid` never reached the opener at all (refused upstream). `tests/hardening.rs::test_dw_6_3_no_source_file_spawns_a_shell` guards the absence of the other path across 2000+ lines of shipped code.
VERDICT:  PASS

### DW-6.4
PREMISE:  "A link target that is missing, unreadable, a directory, a device file or FIFO, oversized, or binary is refused with a status message and leaves the current document rendered — with the file-type check performed before any read."
EVIDENCE: `link.rs:343` (`resolve_regular_file`: `canonicalize` → `metadata` → `is_file` → size, **no open**), `:383` (`read_text_target`: pre-open `metadata`, post-open `fstat`, bounded read, NUL sniff).
TRACE:    `follow` → `classify` → `resolve` (stat-only) → depth check → `read_text_target` → `document_from_text`. Nothing is pushed and no document is replaced until the load succeeds (`link.rs:576`), so a refusal leaves the screen untouched.
EXECUTED: real binary over a pty, ten hostile targets, every one refused with the right message and the viewer responsive afterwards:
```
FIFO (no writer)        status 'not a regular file'  responsive_after=True
FIFO (writer attached)  status 'not a regular file'  responsive_after=True
/dev/zero /dev/random /dev/tty   'not a regular file'  responsive_after=True
symlink loop            'no such file'               responsive_after=True
directory               'not a regular file'         responsive_after=True
9 MiB file              'larger than'                responsive_after=True
missing                 'no such file'               responsive_after=True
```
Library-level timings: every refusal returned in 41–284 µs — no read occurred. RSS was flat (15.5 MB) across the 9 MiB refusal, confirming the stat-size gate runs before any allocation.
VERDICT:  PASS (for the link-target path this item names; the unguarded second door is Issue 1)

### DW-6.5
PREMISE:  "A crafted link containing shell metacharacters, newlines, or `../` traversal is handled without executing anything or escaping into an unintended process invocation. This is a no-process-escape guarantee only; traversal targets that resolve to readable files still open, per the chosen policy."
EVIDENCE: `link.rs:203` (`classify` — a metachar string has no RFC 3986 scheme, so it is a *path*, never a process argument), `:325` (control-byte refusal), `:343` (`canonicalize`, deliberately not a containment check).
TRACE:    `"; rm -rf /"` → no scheme → `LocalFile` → joined + canonicalized → ENOENT → `Missing`. No process is involved at any point.
EXECUTED: probe confirms both halves. Metacharacters refused as missing paths; **traversal and symlinks still open**, as the policy requires:
```
"../outside.md"                      -> OPENED .../outside.md
"../../../../../../../../etc/hosts"  -> OPENED /private/etc/hosts
"%2e%2e/outside.md"                  -> OPENED .../outside.md
symlink out of base dir              -> OPENED (canonical target)
```
The implementation has **not** been narrowed into a directory jail — checked explicitly, since that would itself be a finding.
VERDICT:  PASS

### DW-6.6
PREMISE:  "Mouse wheel scrolls the viewport; a click on a link activates it; a click on a non-link cell does nothing; mouse capture can be toggled off."
EVIDENCE: `app.rs:1230` (`handle_mouse_event`), `:1258` (`click`), `:1273` (`link_at`), `:1213` (`toggle_mouse_capture`); `painter.rs:1069` (`item_columns`); `terminal.rs:46/50` (`MOUSE_ENABLE`/`MOUSE_DISABLE`), `:453` (`set_mouse_capture`).
TRACE:    SGR report → `ScrollUp/Down` → `scroll_by(±3)`; `Down(Left)` → `click` → `link_at` re-measures columns through `sanitize`+`clip_to_width` (not `Run.width`, per `docs/code-standards.md`) → hit → `OpenLink`; miss → `false`, no repaint. `m` → `SetMouseCapture` → guard writes the mode bytes to its own writer and flushes.
EXECUTED: `test_dw_6_6_the_wheel_scrolls_and_a_click_activates_a_link_in_the_real_binary` finds the link's real painted coordinates and clicks them; `test_dw_6_6_m_puts_the_mouse_mode_bytes_on_the_wire_both_ways` asserts `\x1b[?1000l`/`\x1b[?1006l` and back on the actual wire, not on a status string. `MOUSE_DISABLE` is a substring of `RESTORE_SEQUENCE`, so a panic or `SIGTERM` also turns reporting off.
VERDICT:  PASS

### DW-6.7
PREMISE:  "`y` emits a well-formed OSC 52 sequence carrying exactly the selected code block's text."
EVIDENCE: `terminal.rs:85` (`osc52_copy`), `:63` (`MAX_CLIPBOARD_BASE64`); `app.rs:1316` (`code_block_in_view`), `:2456` (`first_code_literal`); `main.rs:490`.
TRACE:    `y` → `CopyCodeBlock` → first code block any viewport line belongs to → AST `literal` (not the clipped paint) → base64 → `ESC ] 52 ; c ; <b64> ESC \` → written straight through, outside any frame, then flushed.
EXECUTED: `test_dw_6_7_y_puts_the_code_blocks_bytes_on_the_wire_as_osc_52` decodes the payload off the pty and asserts byte equality with the fence source. My probe confirms the bound is exact (48 KiB text → 65 545-byte sequence, 48 KiB + 1 → `None` → "too large" status, never a truncated write) and that a payload of raw `ESC`/`BEL`/`;` round-trips with **zero** stray bytes outside the base64 alphabet in the emitted body.
VERDICT:  PASS

### DW-6.8
PREMISE:  "A link to a resolvable local file that is NOT markdown (e.g. a `.txt` or `.rs` file) opens in place and joins the document stack, exactly as a markdown target does."
EVIDENCE: `link.rs:99` (`LocalFile`), `:564` (both variants share one arm — the distinction is what the destination *claims*, not what is allowed).
TRACE:    `notes.txt` → `LocalFile` → identical resolve/read/push as `LocalDoc`.
EXECUTED: `test_dw_6_8_a_non_markdown_target_opens_in_place_and_joins_the_document_stack` (pty, `.txt`), plus unit tests for `.rs` and an extensionless `LICENSE`, and a `Backspace` round trip out of a `.txt`.
VERDICT:  PASS

**All requirements met:** YES for the stated behaviour of all 8 items. See Issue 1 for a demonstrated defect on DW-6.2's code path that the item's own sentence does not describe.

## Test-DW Coverage

- [x] All 8 DW items have corresponding automated tests that ran in Step 0
- [x] Coverage matches the stated 100% level: every item is covered both at unit level (`src/link.rs`, `src/app.rs`, `src/terminal.rs`) and end-to-end through the real binary over a pty
- [x] Tests are DW-tagged and sentence-named per `docs/code-standards.md`

**Gap:** no test exercises `Navigator::back()` against a hostile file type. `test_a_back_whose_file_vanished_keeps_the_entry_for_another_try` covers a *deleted* parent (which errors cleanly) but not a parent that has become a FIFO, a device, oversized, or binary — which is the case that fails. This is the coverage gap that let Issue 1 through.

## Dead Code

None found. No unused imports, no unreachable code after early returns, no debug statements, no commented-out blocks. `clippy --all-targets` is clean. `link.rs` shipped code contains **zero** wildcard match arms, per the project convention.

## Correctness Dimensions

| Dimension | Status | Evidence |
|-----------|--------|----------|
| Concurrency | N/A | Single-threaded event loop; no shared mutable state, no async, no background tasks. The one ordering hazard — swapping the document underneath a frame — is handled by draining `take_action` between frames (`main.rs:637`). |
| Error Handling | PASS | Hand-rolled `LinkError` with manual `Display`, every variant reaching the status row and leaving the document rendered. No empty catches: the only `let _ =` sites are terminal-restore best-effort writes and one documented re-push that cannot fail. |
| Resources | **FAIL** | `Navigator::back` blocks permanently in `open(2)`. Demonstrated below with a sampled stack. |
| Boundaries | PASS | Probed 0×0, 1×1, 0×40, 40×0 and 65535×65535 viewports; clicks at columns/rows 0, 1, 79, `u16::MAX`; 373 visible links cycled 751 times each way; wide/zero-width/RTL link labels. No panic. Stack bounded at 64 (`MAX_STACK_DEPTH`), URL at 4096, link file at 8 MiB, OSC 52 at 64 KiB base64. |
| Security | **FAIL** | Same defect: a crafted link plus a writable directory wedges the viewer indefinitely on a hostile file type — named verbatim in the brief's in-scope list. Everything else in the security surface is sound: argv verified at the byte level, no shell, scheme allowlist, no jail-narrowing, and the status row is sanitized (C0/C1/DEL/bidi stripped) before it reaches the wire. |

## Loaded-Skill Criteria

| Skill | Criterion | Status | Evidence |
|-------|-----------|--------|----------|
| cc-defensive-programming | External input validated at entry | **FAIL** | `link.rs:591` `Navigator::back` calls `DocumentSource::load_with` — the *unvalidated* loader — on a path named by an untrusted document, bypassing the barricade the module's own header declares ("File type before file content… because binary-detection-by-read on a FIFO or a character device does not return"). Demonstrated below. |
| cc-defensive-programming | Barricade design (validate at the boundary; inside, assume validated) | **FAIL** | The barricade has two doors. `follow` goes through `resolve` + `read_text_target`; `back` goes around both. Same root cause as above. |
| cc-defensive-programming | No empty catch blocks / swallowed errors | PASS | All `let _ =` occurrences are restore-path best-effort (`terminal.rs:490-527`) or a documented infallible re-push (`link.rs:597`). Every `LinkError` surfaces to the reader. |
| cc-defensive-programming | Assertions for bugs only, not anticipated runtime errors | PASS | No production assertions on external input; all runtime conditions are `Result`-carried. |
| cc-defensive-programming | Correctness-vs-robustness posture | PASS | Robustness posture is right for a viewer: a refusal keeps the last good render and tells the reader why, rather than exiting. |
| ca-architecture-boundaries | Dependency rule / DIP | PASS | `UrlOpener` is declared in `link.rs` (the layer that decides *whether*) and implemented by `SystemOpener` (the layer that knows *how*) — arrow points inward, and it is what makes DW-6.3 assertable on argv rather than on a browser window. |
| ca-architecture-boundaries | Business logic runs without infrastructure | PASS | The whole interaction layer is drivable with no terminal, no process and no document on disk; `PendingAction` keeps `std::process`/`std::fs` out of `app.rs` entirely. |
| ca-architecture-boundaries | SRP by actor | PASS | `AppState` decides, `main.rs::perform_action` performs, `Navigator` owns policy + history. One reason to change each. |
| ca-architecture-boundaries | Boundary integrity — no bypass around the declared seam | **FAIL** | `read_text_target` is `link.rs`'s declared read seam; `back()` reaches past it into `crate::loader` directly. The architectural shape *is* the bug. |

## Issues

### 1. `Backspace` hangs the viewer permanently on a hostile file type — the link barricade has an unguarded second door

- **File:** `crates/stele/src/link.rs:589-601` (`Navigator::back`), reaching `crates/stele/src/loader.rs:164-178` (`DocumentSource::load_with`)
- **Severity:** High (denial of service — unrecoverable without killing the process) · **Confidence:** Certain (executed reproduction with a sampled stack)

**Demonstrated by:** real `target/debug/stele` driven over a pty. Open `index.md`, `Tab`+`Enter` into `second.md`, replace `index.md` with a FIFO, press `Backspace`:

```
Backspace onto FIFO   opened_child=True  backspace->paint 3.00s  responsive_after=False  alive=True
  … after 3 more seconds                                          responsive=False        alive=True
```

`sample(1)` — 888 of 888 samples in one place:

```
stele::run_session                    main.rs:638
stele::perform_action                 main.rs:480
stele::link::Navigator::back
stele::loader::DocumentSource::load_with
std::fs::File::open
open  (in libsystem_kernel.dylib)     ← blocked
```

The event loop never returns. No further key is read, no frame is painted, `q` and `Ctrl-C` do nothing (raw mode has made them keystrokes, and nothing is reading them). The reader must kill the process from another terminal — and because the loop never reaches the restore, the terminal is left in raw mode + alternate screen until the signal handler fires.

**Why it happens:** `follow()` routes every target through `resolve_regular_file` (stat → `is_file` → size, no open) and then `read_text_target` (pre-open stat, post-open `fstat`, bounded read, NUL sniff). `back()` routes through neither — it calls `entry.source.load_with(self.options)`, which is the command-line loader and goes straight to `File::open`. `open(2)` on a FIFO with no writer blocks in the kernel; there is no check in front of it.

**The same missing check has three further consequences**, all measured against the identical mutation reached through `follow()` for contrast:

| Parent mutated to | `follow()` (guarded) | `back()` (unguarded) |
|---|---|---|
| FIFO | refused by type in 84 µs | **blocks forever in `open(2)`** |
| symlink → `/dev/zero` | `not a regular file` (84 µs, no read) | reads the device until the 64 MiB ceiling, then `larger than 64 MiB` — the exact "binary-detection-by-read on a character device" hazard DW-6.4 exists to prevent, arriving through the back door |
| 20 MiB of text | `larger than the 8 MiB stele will open` | **opens it** — the link ceiling is bypassed for the 64 MiB document ceiling |
| binary, NUL at byte 0 | `refusing to open binary content` | **opens it** as a document — the NUL sniff never runs |

**Scope justification.** The brief names this in-scope verbatim: "any path that … blocks indefinitely on a hostile file type". The listed edge case states the rule unconditionally — "the file-type check must happen BEFORE any read, since binary-detection-by-read can block forever on a character device or FIFO" — and `back()` performs a read with no file-type check. Stack entries above the root are literally former link targets: paths chosen by an untrusted document.

**Preconditions, stated honestly:** an attacker must be able to replace a path the reader has already visited while the reader is in a child document. That is a *wider* window than the residual TOCTOU `read_text_target`'s doc comment documents and accepts (which is "an attacker who can already write into the document's directory at the instant of a keystroke") — here the window is the entire time the reader spends away. Realistic carriers: an untrusted archive unpacked into a shared or world-writable directory, or a documentation tree on a mount another process controls. It is not reachable by a crafted document alone.

**Fix:** route `back()` through the same seam `follow()` uses — resolve/type-check the stacked path and read it with `read_text_target`, then `document_from_text` — instead of `DocumentSource::load_with`. That closes the hang and restores the 8 MiB ceiling and the binary sniff on the return path in one change. Keep the existing "put the entry back on failure" behaviour. Add a test that `Backspace` onto a FIFO returns promptly (the `mpsc::recv_timeout` watchdog shape `test_dw_6_4_a_fifo_target_is_refused_without_ever_opening_it` already uses).

## Notes (non-blocking)

1. **U+202E reaches the wire inside an OSC 8 hyperlink — pre-existing, not this phase.** `[x](http://e.com/‮dm.exe)` paints `ESC]8;;http://e.com/<U+202E>dm.exe ESC\` into the document body. Traced to `crates/highlight`'s `sanitize_url`/`hyperlink_open`, which the diff does not touch (`git diff --stat 1c4192f..HEAD -- crates/highlight` is empty). Phase 6's own new surface is clean: the status row correctly strips it via `painter.rs:450` `sanitize`, verified byte-by-byte for bidi overrides, C1 CSI (`U+009B`), DEL and C0. Confidence: high. Severity: low (a spoofed URI in a terminal hover/click target). Belongs to whoever owns `highlight`, not to this phase.
2. **Cross-crate DW-ID collision.** `test_dw_6_2_*`, `test_dw_6_3_*` etc. exist in `crates/gfx` and `crates/math` for entirely different requirements, so a bare `cargo test dw_6_3` mixes phases. Traceability is still recoverable per-crate, but the DW tag is not globally unique. Confidence: certain. Severity: low, process-level.
3. **`https://` (scheme, `//`, no host) is handed to the opener.** `validated_url`'s emptiness check is `url.len() <= scheme.len() + 1`, which `https://` (8 vs 6) passes. Harmless in practice — `open "https://"` does nothing useful — but it is a "URL with no target" the check was written to catch. Confidence: certain. Severity: very low.
4. **NUL past the 8 KiB sniff window opens.** A file with its first NUL at byte 9000 is accepted and the NUL enters the document text. Documented and tested as a deliberate heuristic bound (`test_the_binary_sniff_looks_only_at_a_bounded_prefix`), and the painter's `sanitize` strips it before the wire, so there is no terminal impact. Recorded because it is the kind of bound worth knowing. Severity: informational.
5. **Two different links to the same URL on adjacent lines merge into one `Tab` stop.** Documented at `app.rs:1061-1064` as a cosmetic miscount that is never a misnavigation (both halves open the same place). Agreed — not a defect. Severity: informational.
6. **On the reported "confirmed a hang" lead.** Chased first, as instructed. The follow path does **not** hang: FIFO with and without a writer, symlink→FIFO, `/dev/zero`, `/dev/random`, `/dev/tty`, `/dev/null`, `/dev/stdin`, symlink loop and directory are all refused in 41–284 µs at library level and stay responsive through the real binary. A plausible explanation for a truncated earlier probe is a bug in the probe rather than in stele — my own first probe run died on an unbounded `fs::read("/dev/urandom")` that **I** wrote, which looks exactly like "confirmed a hang" from the outside. The real hang is on `back()`, one door over, and is Issue 1.
7. **Scratch artifacts** (`sample-1-scratch/`, including the probe crate, PATH shim and pty driver) were created under the worktree and deleted before finishing. No mutating commands were run against the repository.

**Verdict: FAIL** — blocker: Issue 1, `Navigator::back` (`link.rs:589`) reads a stacked document through the unvalidated `DocumentSource::load_with` instead of the link barricade, blocking the whole viewer permanently in `open(2)` on a FIFO and additionally bypassing the 8 MiB ceiling, the regular-file check and the binary sniff on the return path. Every one of the eight Done-When items passes on its stated behaviour; the failure is a demonstrated defect on DW-6.2's code path, a listed edge case ("the file-type check must happen BEFORE any read") left unhandled on that path, and a violation of the loaded `cc-defensive-programming` barricade criterion and the `ca-architecture-boundaries` seam-integrity criterion.
