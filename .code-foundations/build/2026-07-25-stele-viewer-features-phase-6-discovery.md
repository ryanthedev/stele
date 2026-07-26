# Discovery + Design: Phase 6 — Mouse, link following, and clipboard

Worktree: `.code-foundations/wave-worktrees/phase-6`. Built on `f1529e4`
(Phase 3's result), then rebased onto `1979b09` (Phases 1+2+3+4)
then onto `1c4192f` (…+5), and finally onto
`75c536a` (…+5's fix-forward) as those phases landed — see the rebase and
review sections at the end.

## Files Found

| File | Lines | Relevance |
|---|---|---|
| `crates/stele/src/app.rs` | 2835 | `Mode`, `AppState`, key tables, anchor/relayout, `reseat_toc` |
| `crates/stele/src/main.rs` | 632 | event loop, `handle_chrome_key`, `Session`, `paint` |
| `crates/stele/src/painter.rs` | 1018 | `paint_run` (OSC 8 from `Run.aux`), `sanitize`, `clip_to_width` |
| `crates/stele/src/terminal.rs` | 952 | `RESTORE_SEQUENCE`, `ENTER_SEQUENCE`, `TerminalGuard`, signal restore |
| `crates/stele/src/loader.rs` | 533 | `DocumentSource`, `read_bounded`, `MAX_DOCUMENT_BYTES`, `LoadError` |
| `crates/highlight/src/hyperlink.rs` | 163 | `sanitize_url` — OSC-8 *display* allowlist (http/https/file/mailto) |
| `crates/layout/src/inline.rs` | — | `Frag.link` → `Run.aux` for `Semantic::Link` (confirmed at `inline.rs:135`) |
| `crates/stele/tests/common/pty.rs` | 488 | the shared pty harness; `RESTORE` is asserted **byte-exact** |
| `crates/stele/tests/toc_key_routing.rs` | 251 | the template for a real-`main.rs`-routing integration test |
| `crates/stele/tests/dw_7_6_hostile_links.rs` | 71 | the hostile-link corpus to extend |
| `crates/stele/tests/hardening.rs` | — | the source-inspection guard idiom (used below for "no shell") |
| `fixtures/hostile-links.md` | 9 | 4 links: 1 safe https, 3 hostile schemes |

## Current State

- `Mode` is `{ Normal, Toc { selected } }`, matched exhaustively at the top of
  `handle_key_event`. No `LinkSelect`, no mouse handling anywhere in the crate.
- `Run.aux` already carries the raw link destination for `Semantic::Link` runs
  (set in `layout::inline`, consumed by `painter::paint_run` for OSC 8). A link
  spanning a style change or a wrap boundary becomes **several** runs sharing
  one `aux`.
- `LayoutTree::block_at(line) -> Option<NodeId>` maps a viewport line back to
  its top-level source block; `Document::node(id)` resolves it. That is the
  path from "what the reader is looking at" to a `BlockKind::CodeBlock`'s
  `literal`.
- `loader::read_bounded` is the existing size barricade (64 MiB, `take(limit+1)`
  so the refusal is bounded). `LoadError` has `Io / InvalidUtf8 / TooLarge`.
- `terminal::RESTORE_SEQUENCE` is the single source of truth for "restore",
  written by `Drop`, the panic hook, and the async-signal-safe handler.
  `tests/common/pty.rs::RESTORE` is a byte-for-byte copy and
  `assert_restores_the_terminal` asserts **equality**, not containment.
- `base64 = "0.22"` is already a `crates/stele` dependency (kitty payloads), so
  OSC 52 needs no new dependency.
- `crossterm 0.29` with `use-dev-tty`; `libc` on unix.

## Gaps

| Gap | Resolution |
|---|---|
| No `Mode::LinkSelect`, no link enumeration, no selection indicator | New `VisibleLink`/`LinkSpan` + `Mode::LinkSelect { index }` in `app.rs`; reverse-video indicator in `painter.rs` |
| `LinkTarget`/`LinkError`/`DocumentStack` do not exist | New module `crates/stele/src/link.rs` (see *Deviation* below) |
| No mouse capture, no mouse event routing | `terminal.rs` gains enable/disable sequences; `main.rs` routes `Event::Mouse` |
| Mouse capture is terminal state that outlives the process | Disable bytes go **into `RESTORE_SEQUENCE`**, so panic + signal paths clear it too; `tests/common/pty.rs::RESTORE` updated in step |
| `highlight::sanitize_url` allows `file:`/`mailto:` — too wide for *activation* | Activation gets its own, tighter allowlist (`http`/`https` only, DW-6.3). The display allowlist is unchanged: rendering a `mailto:` as an OSC 8 is not the same act as handing it to a process |
| `loader` computes `FileInfo` + preprocess + parse inside `load_with` only | Extract `loader::document_from_text` so the link path reuses the one parse entry point (`counted_parse`) instead of growing a second |
| No `captures_all_keys` gate yet (Phase 4 introduces it) | Added here as an exhaustive `Mode` match, used by `main.rs::handle_chrome_key`. See *Key routing* |

## Code Standards

Applied from `docs/code-standards.md`:

- `#![deny(unsafe_code)]` in `crates/stele`; **no second `allow` opt-out** —
  `tests/hardening.rs` asserts the count is exactly 1. Nothing this phase adds
  needs `unsafe` (mouse capture is escape bytes; the opener is `std::process`).
- Hand-rolled error enums with manual `Display`, no `thiserror` → `LinkError`.
- Exhaustive matches, no wildcard arms → `Mode`, `LinkTarget`, `LinkError`,
  `PendingAction`, `MouseEventKind` dispatch.
- Sentence-style test names; DW-tagged names for plan requirements.
- Shared pty harness under `tests/common/`; never re-implement a pty.
- Never assert on `Run.width` — column arithmetic for hit-testing re-measures
  through the width engine using the painter's own `clip_to_width`.
- Imports in three groups: `std`, external+workspace, `crate::`.

## Test Infrastructure

`cargo test` built-in. Integration tests declare `mod common;` and use
`common::pty` (real pty, real binary), `common::render::render_row` (terminal
cell-grid model), `common::fixtures`. Documented harness hazards, all of which
apply to the new integration test:

- `read_until` reads in 8 KiB blocks and can swallow past its needle →
  sequential waits need `drain_to_quiet` between them (the `press()` helper in
  `toc_key_routing.rs` is the correct idiom; reused verbatim).
- `Esc` immediately followed by `q` parses as `Alt+q` → end sessions with `q`
  after a drain, or Ctrl-C.
- A `try_wait` loop that stops draining the pty deadlocks.
- `render_row` is reported not to terminate on the startup wire → wait for a
  content needle with `read_until` first, then `drain_to_quiet`, then read one
  frame.

## DW Verification

Test names below are the ones that shipped. `A` = `crates/stele/src/app.rs`,
`L` = `src/link.rs`, `P` = `src/painter.rs`, `T` = `src/terminal.rs`,
`I` = `tests/link_interaction.rs` (real binary on a real pty),
`H` = `tests/hardening.rs`, `X` = `tests/dw_7_6_hostile_links.rs`.

| DW-ID | Done-When Item | Status | Test Cases |
|---|---|---|---|
| DW-6.1 | `Tab`/`Shift-Tab` cycle visible links with a visible selection indicator; `Enter` activates | COVERED | A: `..._tab_and_shift_tab_cycle_the_links_in_the_viewport_and_wrap`, `..._only_links_inside_the_viewport_are_offered`, `..._tab_with_no_links_in_view_reports_instead_of_entering_the_mode`, `..._enter_activates_the_selected_link_and_leaves_the_mode`, `..._esc_leaves_link_selection_without_activating_anything`, `..._the_status_row_names_the_destination_of_the_selected_link`; P: `..._the_selected_link_is_painted_in_reverse_video_and_its_neighbour_is_not`, `..._a_selection_on_another_line_does_not_reverse_this_one`, `..._the_indicator_follows_the_line_it_names_across_a_scroll`; I: `..._tab_selects_a_link_through_the_real_binarys_key_path`, `..._a_chrome_key_pressed_under_a_link_selection_does_not_reach_the_document` |
| DW-6.2 | Relative markdown link opens in place; `Backspace` returns at the previous scroll position | COVERED | L: `..._following_a_relative_markdown_link_pushes_the_current_document`, `..._back_pops_to_the_previous_source_and_its_scroll`, `..._back_at_the_root_reports_instead_of_popping`, `..._a_link_to_the_open_document_opens_it_again_and_back_returns`, `..._a_stack_at_its_ceiling_refuses_the_next_hop_and_stays_poppable`; I: `..._enter_opens_a_relative_markdown_link_and_backspace_returns_to_the_scroll_position` |
| DW-6.3 | `http(s)` handed to the OS opener via argv, no shell; other schemes refused with a status message | COVERED | L: `..._an_http_link_reaches_the_opener_as_a_single_argv_element`, `..._the_opener_argv_names_a_real_program_and_never_a_shell`, `..._a_non_http_scheme_is_refused_before_the_opener_is_touched` (7 schemes), `..._http_and_https_are_the_only_schemes_that_reach_the_opener`; H: `..._no_source_file_spawns_a_shell` (mutation-checked); X: `..._the_activation_allowlist_is_tighter_than_the_display_allowlist` |
| DW-6.4 | Missing / unreadable / directory / device / FIFO / oversized / binary refused, document still rendered, **file-type check before any read** | COVERED | L: `..._a_missing_target_is_refused`, `..._a_directory_target_is_refused`, `..._a_fifo_target_is_refused_without_ever_opening_it` (watchdog thread: a hang fails), `..._a_character_device_is_refused_by_type_not_by_read` (`/dev/zero`), `..._an_unreadable_file_is_refused`, `..._an_oversized_file_is_refused_by_its_stat_size`, `..._a_file_exactly_at_the_link_ceiling_still_opens`, `..._a_binary_file_is_refused_before_it_is_parsed`, `..._invalid_utf8_that_carries_no_nul_is_still_refused`, `..._a_refused_target_leaves_the_stack_and_the_document_untouched`; I: `..._a_refused_target_reports_and_leaves_the_document_rendered` |
| DW-6.5 | Shell metacharacters / newlines / `../` handled with no process escape; traversal that resolves still opens | COVERED | L: `..._shell_metacharacters_in_a_target_never_reach_a_process` (8 payloads), `..._a_newline_inside_a_url_is_refused_rather_than_passed_to_the_opener`, `..._a_url_past_the_length_ceiling_is_refused`, `..._no_hostile_destination_ever_reaches_the_opener_seam` (12 payloads, asserted on the opener's own log), `..._a_traversal_path_that_resolves_to_a_readable_file_opens`, `..._a_symlink_pointing_outside_the_base_directory_opens`; X: `..._every_hostile_fixture_destination_is_refused_activation_without_a_process` |
| DW-6.6 | Wheel scrolls; click on a link activates; click on a non-link does nothing; capture toggleable | COVERED | A: `..._a_wheel_scroll_moves_the_viewport_and_clamps_at_both_ends`, `..._a_click_on_a_link_cell_activates_that_link`, `..._a_click_on_a_cell_with_no_link_changes_nothing`, `..._a_click_past_the_end_of_the_document_changes_nothing`, `..._m_toggles_mouse_capture_and_reports_which_way`; P: `..._item_columns_agree_with_the_columns_the_painter_writes_to`; T: `..._every_mouse_mode_enabled_on_entry_is_disabled_by_the_restore_sequence`, `..._the_restore_disables_the_mouse_before_leaving_the_alt_screen`, `..._toggling_capture_writes_the_enable_and_disable_sequences`; I: `..._the_wheel_scrolls_and_a_click_activates_a_link_in_the_real_binary`, `..._m_puts_the_mouse_mode_bytes_on_the_wire_both_ways` |
| DW-6.7 | `y` emits a well-formed OSC 52 carrying exactly the code block's text | COVERED | T: `..._osc_52_wraps_exactly_the_payload_bytes_in_base64`, `..._the_sequence_carries_no_raw_control_bytes`, `..._an_oversized_payload_is_refused_rather_than_truncated`; A: `..._y_yanks_the_code_block_the_reader_is_looking_at`, `..._the_yanked_text_is_the_source_not_the_clipped_paint`, `..._y_finds_a_fence_nested_in_a_list_item`, `..._y_with_no_code_block_in_view_reports_instead`, `..._a_code_block_scrolled_out_of_view_is_not_the_one_yanked`; I: `..._y_puts_the_code_blocks_bytes_on_the_wire_as_osc_52` |
| DW-6.8 | A resolvable **non-markdown** local file opens in place and joins the stack | COVERED | L: `..._a_txt_target_opens_in_place_and_joins_the_document_stack`, `..._a_rs_target_opens_in_place_and_joins_the_document_stack`, `..._an_extensionless_readable_file_opens`, `..._back_from_a_non_markdown_document_returns_to_the_markdown_one`; I: `..._a_non_markdown_target_opens_in_place_and_joins_the_document_stack` |

**All items COVERED:** YES (8 of 8 DW-IDs in the dispatch prompt, 8 rows here.)

**Name collision, pre-existing:** `tests/tmux_graphics.rs` already carries two
`test_dw_6_4_*` names from the *2026-07-21* plan, whose DW-6.4 is about tmux
taking the alt-text path. Same id, different plan. Not introduced here and not
renamed here — renaming another phase's tests is out of this phase's scope —
but a grep for `test_dw_6_4_` returns both sets.

## Design Decisions

### Boundaries (ca-architecture-boundaries)

Three actors, three homes; the dependency arrows point inward:

| Layer | Owns | Never touches |
|---|---|---|
| `app.rs` (`AppState`) | *what a key means* — mode, selection index, scroll, pending action | the filesystem, `std::process`, the terminal |
| `link.rs` (`Navigator`) | *what a link resolves to and whether it may be opened* — the barricade | the terminal, the painter |
| `main.rs` / `terminal.rs` | *the OS* — spawning the opener, capturing the mouse, writing OSC 52 | policy decisions |

`AppState` therefore never opens anything. Key handling records a
`PendingAction` (`OpenLink(String) | Back | CopyCodeBlock | SetMouseCapture(bool)`)
which `main.rs` drains after every event via `AppState::take_action`. That is
the seam that keeps `AppState` unit-testable without a filesystem *and* keeps
the process-spawning code out of the module the tests drive hardest.

`UrlOpener` is a trait defined in `link.rs` (the layer that *uses* it) and
implemented by `SystemOpener` (the layer that *knows the OS*) — the dependency
inversion the skill asks for. Tests inject a `RecordingOpener` and assert on
the exact argv, so DW-6.3 is provable without launching a browser.

### The barricade (cc-defensive-programming)

A link destination is external input from an untrusted document. It crosses the
barricade in `LinkTarget`, and the ordering is load-bearing:

1. **classify** — `LinkTarget::classify(href)`. Scheme-bearing (RFC 3986
   `ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":"`) → `Url` for `http`/`https`,
   `Err(UnsupportedScheme)` for everything else. `#fragment` → `Err(Fragment)`.
   Otherwise a local path → `LocalDoc` for `.md`/`.markdown`, `LocalFile`
   otherwise.
2. **resolve** — `LinkTarget::resolve(&self, base: &Path)`.
   - `Url`: re-validate the scheme, refuse any ASCII control byte (including
     `\n`, `\r`, `\0`) and anything past `MAX_URL_BYTES`. **Refuse, do not
     strip** — silently rewriting a URL and then opening it is worse than
     saying no. (This is the one place the policy differs deliberately from
     `highlight::sanitize_url`, which strips because it is emitting a *display*
     sequence, not invoking a process.)
   - `LocalDoc`/`LocalFile`: join `base`, then `canonicalize` (resolves `..`
     and symlinks, and is the existence check), then `fs::metadata`, then
     `is_file()`, then `len() <= MAX_LINK_FILE_BYTES`. **No byte is read here.**
3. **open** — `link::read_text_target(path)`: `metadata` again (defense in
   depth), `File::open`, then **`file.metadata()` on the open descriptor** and
   `is_file()` again before the first read, then `read_bounded`, then the NUL
   sniff, then UTF-8.

The two `is_file()` checks are not redundant: the first is what stops us
`open()`ing a FIFO at all (an `open(O_RDONLY)` on a FIFO blocks until a writer
appears — the exact hang the plan's edge-case list names); the second is an
`fstat` on the descriptor we actually hold, which closes the TOCTOU window
between stat and read. The residual window — a path swapped for a FIFO between
the stat and the `open` — cannot be closed without `O_NONBLOCK`, which would
mean a platform-specific `custom_flags` constant; it is documented in the code
rather than papered over.

**Binary detection is a NUL sniff over the first 8 KiB, not a UTF-8 check.** A
NUL byte is valid UTF-8, so `String::from_utf8` alone would happily hand a
`.o` file to the parser. Both checks run; either one refuses.

Assertions vs. error handling, per the skill's table: every one of these is
*anticipated bad input at a trust boundary*, so every one is an error return
with a `Display` message that reaches the status row. There are no assertions
on this path.

### Correctness over robustness, in two places

- **OSC 52 refuses rather than truncates.** A code block whose base64 exceeds
  `MAX_CLIPBOARD_BASE64` is refused with a status message. A silently truncated
  shell command on the clipboard is a foot-gun the reader cannot see; "nothing
  was copied" is one they can.
- **The link size cap is 8 MiB, tighter than the CLI's 64 MiB.** Opening the
  file named on the command line is the user's explicit choice; following a
  link is one keystroke against content they did not write. Named as
  `MAX_LINK_FILE_BYTES` with that reason in its doc comment, and it also makes
  DW-6.4's oversize test cost a sparse 8 MiB rather than 64.

### Link enumeration and the selection indicator

`AppState::visible_links()` walks `tree.lines(scroll..scroll+height)` and
groups **consecutive** `Semantic::Link` runs sharing one `aux` into one
`VisibleLink { target, text, spans }`; a link that wraps continues its previous
entry when the previous line ended on it. Nothing is cached — the list is
recomputed from `(tree, scroll)` on every use, so a `--watch` reload cannot
leave `Mode::LinkSelect { index }` addressing a link that no longer exists.
`reseat_link_select`, called from `reload_document` beside the existing
`reseat_toc`, clamps the index and drops back to `Mode::Normal` when the new
document has no visible links.

The indicator is **reverse video** on the selected link's runs, matching the
TOC overlay's precedent. Deliberately *not* a new `Semantic` role: a new role
would owe the "distinct after 256-downsampling" invariant that Phase 4's DW-4.8
covers, for no gain — reverse is legible in both theme variants by
construction.

`Painter::frame_with_selection(tree, scroll, size, status, selected, out)` is
the real implementation; `frame_with_status` is it with an empty selection,
mirroring how `frame` is `frame_with_status` with an empty `StatusLine`. No
existing painter call site changes.

### Mouse

Own sequences rather than crossterm's `EnableMouseCapture`: crossterm enables
`?1003h` (any-event tracking), which reports every pointer *motion* and would
have this viewer waking up and re-polling hundreds of times a second while
someone moves the mouse across the window. `?1000h` (press/release) + `?1006h`
(SGR extended coordinates) is what the two things we act on — wheel and click —
actually need, and crossterm's parser reads SGR reports regardless of which
modes we set.

The disable bytes go into `RESTORE_SEQUENCE`, not into a separate exit path:
mouse capture is state on the *terminal*, so it must come off on the panic and
fatal-signal paths too, and `RESTORE_SEQUENCE` is the one thing all three write.
`tests/common/pty.rs::RESTORE` is updated in the same commit, and a unit test
derives the invariant rather than restating the constant — for every `?Nh` in
`MOUSE_ENABLE`, `?Nl` must appear in `RESTORE_SEQUENCE`.

`m` toggles capture; the status row says which way it went, since the effect
(the terminal's own text selection coming back) is invisible until the reader
tries to drag.

### `y` and "the selection"

The phase text says "the code block under the selection", but stele has no
selection outside `Mode::LinkSelect`, whose selection is a *link*. The
resolution: `y` in `Mode::Normal` copies **the first code block intersecting
the viewport** — the block the reader is looking at — and reports
`no code block in view` when there is none. The lookup goes
`tree.block_at(line)` → `Document::node(id)` → the first `BlockKind::CodeBlock`
in that block's subtree, so a fence nested in a list item or blockquote is
found too. The text copied is the AST's `literal`, **not** the painted line
text, which layout has already clipped with `…`.

### Key routing

`handle_chrome_key`'s existing gate is `state.mode() != Mode::Normal`, which
already makes `T`/`+`/`-` inert under a new mode — the right answer for
`LinkSelect` (relaying out under a live link index would invalidate it) and the
same answer the TOC gets. Rather than leave that as a `PartialEq` accident, the
gate is rewritten to call `Mode::captures_all_keys()`, an exhaustive match with
no wildcard arm, so the next mode is a compile error rather than a silent
default. Phase 4 introduces the same method name for the same reason; at rebase
this is a merge of two identical answers, not a redesign. The behavioural gate
is unchanged either way.

`tests/link_interaction.rs` drives the real binary through a real pty, so the
`main.rs` routing path — not just `AppState` — is what DW-6.1/6.2/6.6 are
proved against.

## Deviations from the plan's `Produces`

| Plan says | Built as | Why |
|---|---|---|
| `DocumentStack::push(DocumentSource)` | `push(&mut self, source: DocumentSource, scroll: usize)` | DW-6.2 requires returning to the *previous scroll position*, which the source alone cannot carry. `pop() -> Option<StackedDocument>` returns both. |
| file scope names `app.rs`, `terminal.rs`, `loader.rs`, `hyperlink.rs` | plus a new `crates/stele/src/link.rs` | `app.rs` is already 2835 lines and the barricade is a distinct actor (see *Boundaries*). Folding ~400 lines of path/scheme/process policy into the viewport-state module would couple two actors in one file, which is the SRP violation `ca-architecture-boundaries` names. `hyperlink.rs` is read but **not modified** — its allowlist governs display, not activation. |

## Prerequisites

- [x] Phase 1 (`relayout_preserving_anchor`, status row) present in the base
- [x] Phase 2 (`DocumentSource`, `LoadOptions`, `--watch`, reload) present
- [x] Phase 3 (`Mode::Toc`, outline) present — the mode/reseat precedent
- [x] `base64` already a `crates/stele` dependency; no new dependency needed
- [x] `libc` available on unix for the FIFO/permission test fixtures
- [ ] Phase 4 not in this base — `Mode::captures_all_keys` is introduced here
      and expected to reconcile at rebase (see *Key routing*)

## Recommendation

**BUILD.** Every DW item is reachable in this base with no plan change. The
work is: `link.rs` (new, the barricade + stack + opener seam), `app.rs`
(`Mode::LinkSelect`, link enumeration, mouse handling, pending actions,
`reseat_link_select`), `painter.rs` (selection indicator + column
hit-testing), `terminal.rs` (mouse sequences into `RESTORE_SEQUENCE`, OSC 52),
`loader.rs` (extract `document_from_text` so the link path reuses the one parse
entry point), `main.rs` (mouse routing + action draining), and tests in
`app.rs`/`painter.rs`/`terminal.rs`/`link.rs` units plus
`tests/link_interaction.rs`, `tests/hardening.rs` and
`tests/dw_7_6_hostile_links.rs`.

---

## Rebase onto Phase 4 (`1979b09`)

Phase 4 landed `Mode::Search`, a bounded highlight cache, and the key-routing
move this phase had anticipated. Three files conflicted: `app.rs`,
`painter.rs`, `main.rs`. `link.rs`, `terminal.rs`, `loader.rs`,
`tests/dw_7_6_hostile_links.rs`, `tests/hardening.rs` and `tests/common/pty.rs`
are **byte-identical** to the pre-rebase commit — the whole barricade, the
hostile-input corpus, the no-shell guard and the restore-sequence pin were
untouched by the merge.

### 1. `captures_all_keys` — two identical answers, merged

Phase 4 introduced `Mode::captures_all_keys()` for the same reason this phase
did, and its doc comment names `Mode::LinkSelect` by name as the variant that
would be a compile error until its author answered. There is now exactly
**one** such method, with an arm for each of `Normal` (false), `Toc` (true),
`Search` (true) and `LinkSelect` (true). This phase's separate copy and the
`state.mode() != Mode::Normal` gate it patched in `main.rs` are both gone:
Phase 4 moved the whole routing decision into `AppState::chrome_action`, which
asks `captures_all_keys` first and rejects chords second, so `handle_chrome_key`
is now pure action.

Two Phase 4 tests enumerate modes in a hand-written array
(`test_only_a_mode_that_owns_the_keyboard_captures_every_key`,
`test_a_mode_that_captures_the_keyboard_is_refused_by_the_chrome_table`).
A wildcard-free `match` makes a new *variant* a compile error, but not a new
*array element* — both were extended with `Mode::LinkSelect`, since otherwise
they would have silently under-tested it.

### 2. One paint path, not two competing wrappers

Phase 4's `frame_with_search` and this phase's `frame_with_selection` had each
become "the real implementation". They are now both one-liners over
`Painter::frame_with_overlays`, which takes a `FrameOverlays { search,
selected }` and projects it per row into `RowOverlays { spans, selected_items }`.
`main.rs::paint` routes `Normal`, `Search` **and** `LinkSelect` through that
single call; only `Toc` still paints something else, because it genuinely is a
different frame.

This was a live defect, not tidiness. Search state outlives `Mode::Search` on
purpose — `n`/`N` traverse from normal mode — so a reader can accept a query
and then press `Tab`. Under a per-mode wrapper the `LinkSelect` arm dropped the
search overlay, and the highlights vanished on a keystroke that was never about
the search. Reverting `paint` to that shape makes
`test_dw_6_1_a_link_selection_over_an_accepted_search_keeps_both_highlights`
fail with the exact SGR that went missing (mutation-checked).

The two overlays compose rather than compete, and the code now says why: a
search span replaces a run's style **role** before any SGR is written, while
the selection is an **attribute** emitted after `write_sgr`. A match inside the
selected link reads as both, pinned byte-for-byte by
`test_a_match_inside_the_selected_link_reads_as_a_match_and_as_selected`.

### 3. Reseating moved onto Phase 4's hook

`reseat_link_select` was called from `reload_document` and `apply_resize_burst`
before the rebase. It now takes `reflowed` and is called from `relayout`,
immediately after `reseat_search` — the one choke point every reload and every
resize passes through, and it skips a theme swap for `reseat_search`'s reason
(the tree is identical, so the index still means what it meant).

### 4. Search state and the document stack — the interaction nobody had run

**This was a real defect.** `SearchState::matches` addresses text by tree line
index and by a byte range into that line's laid-out text. Following a link
replaces the tree in `AppState::open_document`, which does **not** go through
`relayout` — so neither `reseat_search` nor `recompute_matches` fires, and the
match vector would have survived the swap pointing at whatever bytes now sit at
those coordinates. The painter would have highlighted them and `n`/`N` would
have jumped to them.

`open_document` now drops the search outright (`self.search =
SearchState::default()`). Recomputing against the new tree would have been
equally safe; it is rejected on behaviour rather than safety — a reader who
follows a link would land on a document they have never searched, already
covered in highlights for a query typed somewhere else. Dropping it is what a
browser's find-in-page does on navigation. Covered by
`test_opening_a_document_drops_the_search_that_addressed_the_old_one` and, on
the way back with DW-6.2's promise beside it,
`test_dw_6_2_going_back_restores_the_scroll_and_leaves_no_stale_search`.

One related edge, now pinned: a **click** is not a key, so `captures_all_keys`
does not govern it, and a click on a link with a `/` prompt open does follow
the link. `test_a_click_while_a_query_prompt_is_open_leaves_coherent_state`
asserts the state it leaves is coherent — normal mode, one queued action, no
half-typed query still owning the status row.

### 5. Tests added at the rebase

| Test | Claim |
|---|---|
| `test_dw_6_1_a_key_read_during_a_resize_drain_takes_the_same_route_as_an_idle_one` (pty) | `Tab` read during a resize drain is not lost, and `T` read the same way does not reach the document under a selection. Asserts only what holds on **either** path — which side of the 50 ms drain window a keystroke lands on is a race, and asserting the race would be asserting flake. |
| `test_dw_6_1_a_link_selection_over_an_accepted_search_keeps_both_highlights` (pty) | Both overlays survive `Tab` over an accepted query. Mutation-checked. |
| `test_a_search_highlight_and_a_link_indicator_paint_on_the_same_frame` (painter) | Both overlays on one frame, measured through the decor's own resolved SGR. |
| `test_a_match_inside_the_selected_link_reads_as_a_match_and_as_selected` (painter) | The overlapping case: role first, attribute on top. |
| `test_a_link_selection_opened_over_an_accepted_search_keeps_both_overlays`, `test_leaving_link_selection_leaves_an_accepted_search_intact` (app) | Entering and leaving link selection disturbs neither the query nor its matches. |
| `test_opening_a_document_drops_the_search_that_addressed_the_old_one`, `test_dw_6_2_going_back_restores_the_scroll_and_leaves_no_stale_search` (app) | §4 above. |
| `test_a_click_while_a_query_prompt_is_open_leaves_coherent_state` (app) | §4's edge. |

**Post-rebase gate:** 637 passed, 0 failed (`script -q /dev/null cargo test
--workspace`); `cargo clippy --workspace --all-targets` and `cargo fmt --all --
--check` clean. 71 DW-6-tagged tests, up from 68. The no-shell guard still
fails when `/bin/sh` is injected into `src/link.rs` (re-checked after the
rebase).

---

## Rebase onto Phase 5 (`1c4192f`)

Phase 5 landed section folding as a **chrome action rather than a fourth
mode** — `z`/`R`/`M` routed through `AppState::chrome_action` alongside
`+`/`-`/`T`, `FoldState { collapsed: HashSet<NodeId> }` consulted by
`layout_with_folds`, and a `pending_fold_snap` viewport override inside
`relayout`. Only `app.rs` conflicted. `link.rs`, `terminal.rs`, `loader.rs`,
`painter.rs`, `tests/dw_7_6_hostile_links.rs`, `tests/hardening.rs` and
`tests/common/pty.rs` are **byte-identical** to the previous commit — the
barricade, the hostile corpus, the no-shell guard and the restore pin were
untouched, and the `/bin/sh` mutation check was re-run against this tree.

### 1. Folding versus link selection — the guard would have been wrong

`reseat_link_select` hung off `relayout`'s `reflowed` flag. **A fold makes
that flag false**: `reflowed` means "the width changed or the document was
replaced", and a fold is neither — it relays out at the same width on the same
document and removes lines. The guard would have skipped precisely the case
that changes which links exist.

It is now **unconditional**, and the asymmetry with `reseat_search` is the
point: `reseat_search` guards on `reflowed` because it *overwrites*
`Mode::Search { origin }` and would throw away a good answer; there is nothing
cached here to throw away, because the link list is recomputed from
`(tree, scroll)` on every call. It is a pure clamp against fresh truth, and a
no-op exactly when the tree did not change.

Behaviour pinned: a fold that swallows every visible link **dismisses**
`Mode::LinkSelect` with the same `no links in view` message `Tab` gives, and a
fold that removes only some **clamps** the index onto a link that still
exists. Both tests fail when the `reflowed` guard is restored
(mutation-checked).

### 2. `z`/`R`/`M` versus `Mode::LinkSelect` — inert, and pinned three ways

Inert **by construction**: they are chrome, and `chrome_action` returns `None`
for every key while `captures_all_keys()` is true. Nothing was needed beyond
the answer `Mode::LinkSelect` already gives. What was needed was tests, because
Phase 5 added three keys after this mode was written:

- `test_a_mode_that_captures_the_keyboard_is_refused_by_the_chrome_table` had
  a **hand-written `['+', '-', 'T']`** that Phase 5 did not grow — the three
  newest chrome keys went untested there. It now *derives* the chrome set by
  asking `Mode::Normal` what it accepts, so a seventh key is covered
  automatically. (Third instance of the hand-written-array pattern in this
  build.)
- `test_dw_6_1_the_fold_keys_are_inert_while_a_link_is_selected` asserts the
  three keys directly and then that `Tab`/`Enter` still work — the opposite
  failure, a gate that swallowed navigation too.
- `tests/link_interaction.rs`'s routing test now presses `z`, `M` and `R`
  under the selection through the real binary. `M` is the sharpest: an ungated
  collapse-all would take the selected link off the screen on a keystroke that
  was never about links.

Also added `test_no_chrome_key_shadows_a_phase_6_normal_mode_binding`: chrome
runs *before* the normal key table, so a collision would shadow a binding
silently — no compile error, no failing mode test, just a key that stopped
working. `M` (collapse-all) and `m` (mouse capture) differ only in case.

### 3. Fold state versus the document stack — a real leak, fixed

**`open_document` carried `FoldState` into the linked document.** This is the
same shape as the `SearchState` defect from the previous rebase but strictly
worse: a stale `NodeId` is not merely wrong, it is *silently valid*. Ids are
dense positional indices into one document, so `NodeId(7)` names some block in
the new document too, and `layout_with_folds` would have collapsed whatever
section that turned out to head. A reader follows a link and finds a section
of a document they have never folded already collapsed, keyed to a heading in
a different file. `reseat_folds` exists because a *reload* invalidates these
ids; following a link does not go through `reload_document` at all.

**Implemented: fold state is per-document and does not travel the stack.**
Cleared on the way in and — because `Backspace` returns through the same
method — on the way back. The alternative (stashing a `FoldState` per
`StackedDocument` and re-keying it by content the way `reseat_folds` does)
is a real feature with real machinery, not a line of this one. The cost is
stated in the code rather than hidden: fold a section, follow a link, come
back, and the section is open again.

Ordering is load-bearing — the clear happens *before* the `layout_with_folds`
that installs the new tree. `pending_fold_snap` is cleared with it, for the
same reason: it holds a `NodeId` armed for the next relayout of a tree that is
being replaced. Both are mutation-checked.

### Post-rebase gate

664 passed, 0 failed (`script -q /dev/null cargo test --workspace`); clippy
`--all-targets` and `cargo fmt --all -- --check` clean. The no-shell guard
still fails when `/bin/sh` is injected into `src/link.rs`, re-verified against
this tree.

---

## Security review — blocker fixed: `back()` bypassed the barricade

The three-sample review passed all 8 DW items, confirmed no jail-narrowing,
no panics across degenerate viewports, and verified the spawn argv
byte-for-byte with a PATH shim (every hostile URL arrived as `argc=1` verbatim,
no expansion, no `/tmp/pwned`). It returned **FAIL on one blocker**, and it was
right.

### What was wrong

`Navigator::follow` routed every target through `resolve_regular_file` +
`read_text_target`. `Navigator::back` routed through **neither** — it called
`DocumentSource::load_with`, the command-line loader, which goes straight to
`File::open`. Reproduced on the real binary: open `index.md`, follow to
`second.md`, replace `index.md` with a FIFO, press `Backspace`, and the event
loop never returns. No key read, no frame painted, `q` and Ctrl-C inert, 888 of
888 stack samples in `open()`. The process had to be killed from outside.

Two entry points had diverged. That is the shape of the defect, and the whole
fix is to make them converge.

### The audit — there was a third

`grep` over `crates/stele/src/**` found exactly three callers of the loader:

| Caller | In the event loop? | Verdict |
|---|---|---|
| `Navigator::back` | yes | **the blocker** |
| `Session::poll_reload` (`--watch`) | yes | **same hang, same severity** — a watched file replaced by a FIFO blocks `File::open` inside the loop, where raw mode has made Ctrl-C an ordinary keystroke |
| `main()` startup | no | left alone, deliberately |

The reload path was the third instance the coordinator warned about. Startup
is genuinely different: it runs *before* raw mode, so Ctrl-C is still a signal
and a blocking open is escapable — and `stele <(curl …)` is a process
substitution, which really is a FIFO, and a legitimate way to open a document
that a blanket guard would take away. That reasoning is in the code, not just
here.

### The fix

- **`link::reread_document(source, limit, options)`** — one seam for
  re-reading a document the session already has open. Re-*resolves* rather than
  merely re-reading, because the path was canonicalized when it was pushed but
  the object at it may have been replaced since, and that replacement is
  exactly the attack. `Navigator::back` now goes through it.
- **`link::refuse_unless_regular_file(source)`** — the narrowest guard that
  closes the reload door. Type check only, so every message `--watch` already
  produces for a missing, unreadable or oversized file is unchanged.
- **The size ceiling became a property of provenance, not of the entry point.**
  `StackedDocument` carries the `limit` it was admitted under: entry 0 is the
  command-line document and is worth `MAX_DOCUMENT_BYTES`, everything above it
  was reached by a link and is worth `MAX_LINK_FILE_BYTES`. Naively routing
  `back()` through the link ceiling would have *refused a reader their own
  20 MiB file* after one hop — a fix that traded a hang for a lockout.
- **A stdin root now refuses honestly.** `back()` to a `DocumentSource::Stdin`
  entry would have `read_to_end` a drained pipe and render a blank screen.
  `LinkError::StreamNotRereadable` says the true thing instead, matching the
  refusal `--watch -` already gives at CLI parse for the same reason (DW-2.3).

The reviewer's whole table, re-measured after the fix:

| Parent becomes | `follow()` | `back()` before | `back()` after |
|---|---|---|---|
| FIFO | refused | **blocks forever** | `not a regular file`, no open |
| symlink → `/dev/zero` | refused | read to 64 MiB | `not a regular file`, no read |
| 20 MiB text | refused at 8 MiB | opened | **opened** — correct: entry 0 is worth 64 MiB |
| binary, NUL at byte 0 | refused | opened | `refusing to open binary content` |

### Pinned, and mutation-checked

- `test_dw_6_4_a_parent_replaced_by_a_fifo_between_follow_and_back_is_refused_not_opened`
  — the sequence the reviewer ran, behind a 5 s watchdog thread so a
  regression *fails* rather than wedging the suite.
- The device, binary and oversize rows, each as its own test.
- `test_the_command_line_document_is_still_reachable_above_the_link_ceiling`
  and `test_a_linked_document_keeps_the_link_ceiling_on_the_way_back` — the two
  halves of the provenance rule.
- `test_going_back_to_a_stdin_document_reports_rather_than_rendering_nothing`.
- `test_dw_6_4_a_parent_replaced_by_a_fifo_does_not_wedge_the_viewer_on_backspace`
  and `test_dw_6_4_a_watched_path_replaced_by_a_fifo_does_not_wedge_the_viewer`
  — both doors, on the **real binary over a pty**, asserting not just "it
  refused" but "it was still alive to refuse": status row answers, child
  document still rendered, `j` still paints, `q` still quits.

Restoring the old `back()` seam makes the two unit watchdogs fire at exactly
5.00 s with the message naming this blocker; removing the reload guard fails
the `--watch` pty test. Both verified.

## Rebase onto Phase 5's fix-forward (`75c536a`)

Only `app.rs` conflicted, all three hunks mechanical: the `layout` import came
back alongside `layout_with_folds`, `recompute_matches` gained its `ctx`, and
the two test tails concatenated.

The seam the coordinator flagged holds. `open_document` still drops the search
on a document swap, and it does so by assigning a whole `SearchState::default()`
rather than clearing fields — which is why the fix-forward's **new**
`hidden_by_folds` field was carried for free. That is now explicit in the code
and pinned by `test_opening_a_document_also_drops_the_folded_match_count`: a
count derived from a fold-free relayout of the *previous* document is exactly
as document-bound as `matches`, and a leftover would have `n` reporting folded
matches in a file the reader has left.

**Final gate:** 685 passed, 0 failed (`script -q /dev/null cargo test
--workspace`); clippy `--all-targets` and `cargo fmt --all -- --check` clean.
`terminal.rs`, `loader.rs`, `painter.rs`, the hostile corpus, the no-shell
guard and the restore pin are byte-identical to the pre-fix commit; `link.rs`
is where the fix lives. The `/bin/sh` mutation check fails the guard against
this final tree.

**Known, out of scope, recorded elsewhere:** the reviewer found a U+202E leak
in `crates/highlight`'s OSC 8 sequence — outside this diff and correctly
attributed to that crate. Left for follow-up.

---

## Security review, round 2 — blocker fixed: the image decode path

The review confirmed the `back()` and `--watch` fixes (refused in 0.29 s and
0.24 s, viewer alive after; provenance ceiling correct both directions on the
real binary; hostile-URL argv proved end-to-end) and found a **fourth
instance of the same defect**, on a path I had not audited: images.

### What was wrong

`crates/gfx/src/decode.rs` opened a path taken verbatim from the document with
no `stat`. Reached from `media/sizer.rs` (the layout probe) and
`media/sink.rs` (paint), both inside the event loop under raw mode. A document
containing `![x](pipe.png)` where `pipe.png` is a writerless FIFO wedged the
process — 801 of 801 stack samples in `open`, `q` and Ctrl-C inert, killed
from outside. The route that matters is new to this phase: `install_document`
lays out a document **a stranger chose**, so one author controls both the link
and the image path behind it.

### Placement — where a fifth becomes impossible

I put the check in **`gfx::decode::opened`**, not at the two `media/` call
sites, and that choice is the substance of the fix. `opened` is the only
function in the workspace that turns a document-supplied path into an open
file. Both of `decode`'s public entry points reach it; both `media/` call
sites reach those; any future caller of the crate reaches it too. Fixing the
two visible call sites is exactly what produced instances 1–4 — each was fixed
where it was found, and the next caller inherited nothing.

Both call sites already degraded correctly on `Err` (`ok()?` and
`degrade_to_text`), so the new `DecodeError::NotAFile` needed no call-site
change at all: a FIFO image now falls through to alt text.

The full audit, `grep` over every path-open in the workspace, now reads:

| Site | In the event loop? | Status |
|---|---|---|
| `Navigator::back` | yes | fixed (round 1) |
| `Session::poll_reload` | yes | fixed (round 1) |
| `gfx::decode::opened` | yes | **fixed here, at the owning function** |
| `main()` startup | no | exempt, deliberately — pre-raw-mode, so `SIGINT` still kills, and `<(…)` process substitution is a legitimate FIFO document |
| `math`, `width`, `probe` | n/a | test/dev-only fixture reads |

### The reason no gate caught it — fixed

`tests/common/pty.rs` called `env_remove("TERM_PROGRAM")`, and stele gates
graphics on exactly that variable. So the harness did not merely hide
pictures: it made `ImageSizer`, `GfxMediaSink` and everything under them
**unreachable from the whole test suite**. Four rounds of review ran with the
media interface never executing once.

`Graphics::{Off, On}` now selects it, `Off` still the default — flipping it
wholesale would put base64 APC payloads into frames the text tests assert on
and add the 250 ms startup geometry query to every spawn. `spawn_viewer_with`
takes the setting; `spawn_viewer` is unchanged for all 30 existing call sites.

`test_dw_6_4_a_fifo_image_in_a_linked_document_does_not_wedge_the_viewer` runs
with `Graphics::On` and carries **a real PNG alongside the FIFO**, asserting a
kitty `\x1b_G` payload reaches the wire — without that, the test would pass
with graphics off and prove nothing.

### Test-infrastructure fix: orphaned children

`std::process::Child` does not kill on drop, so every pty test that failed an
assertion left a live `stele` holding a pty open (nine counted on one
machine). `ViewerProcess` now wraps the child and kills + reaps on `Drop`,
deref-ing to `Child` so every existing `child.wait()` and `&mut child` call
site is unchanged. Measured: a test panicking before its `quit_and_reap`
leaves **1** orphan without the guard and **0** with it; the full suite now
ends with 0.

### Mutation checks (this tree)

| Removed | Result |
|---|---|
| the `stat` in `gfx::decode::opened` | both gfx watchdogs fire at 5.00 s; the graphics pty test fails |
| the `ViewerProcess::drop` body | a panicking test leaves an orphan |
| `OPENER_PROGRAM` → `/bin/sh` | `test_dw_6_3_no_source_file_spawns_a_shell` fails |

**Final gate:** 690 passed, 0 failed (`script -q /dev/null cargo test
--workspace`); clippy `--all-targets` and `cargo fmt --all -- --check` clean;
0 orphaned processes after the run. `link.rs`, `terminal.rs`, `loader.rs`,
`painter.rs`, `app.rs`, the hostile corpus and the no-shell guard are
byte-identical to the previous commit — this round touches only
`crates/gfx/src/decode.rs` and the two test files.

