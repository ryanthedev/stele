# Discovery + Design: Phase 3 - Width engine

## Files Found
- `crates/ast`, `crates/probe` — existing workspace members (Phases 1-2). No `crates/width` existed before this phase.
- `crates/probe/src/{lib.rs,probe.rs,launch.rs,pty.rs,io_raw.rs}` — the pinned `Probe`/`GhosttyPty`/`Launcher` seam. `Probe::measured_width(&mut self, s: &str) -> u16` (cursor-position-delta measurement) and `probe::Launcher::run_probe` (drives a real Ghostty window as a subprocess via `open -na Ghostty.app --args -e <bin>`) are exactly the primitives DW-3.1's corpus needs — no second harness was written.
- `crates/probe/tests/live_ghostty.rs` — the `#[ignore]` convention this phase's live-Ghostty test follows.
- `docs/spikes/ghostty-caps.md` — item 6: mode 2027 (grapheme clustering) is live-measured ON by default on the pinned Ghostty 1.3.1 binary, contradicting the source-reading research. This phase's corpus had to be measured against that ON state.
- `.code-foundations/plans/2026-07-21-stele-markdown-viewer.md` — Phase 3 section (scope, Produces, Done-When, approach notes).
- No `docs/code-standards.md` in the repo.

## Current State
Greenfield: `crates/width` did not exist. The workspace `Cargo.toml` listed only `ast` and `probe`.

## Gaps
None blocking. The plan's `Produces` signature (`WidthEngine::new`, `cluster_width`, `display_width`, free `graphemes`) was implementable as specified with no redesign needed. The one real gap was epistemic, not structural: the plan flagged the "`unicode-width` + owned correction layer reaches 100% agreement" assumption as MEDIUM confidence, and that could only be resolved by actually measuring — see Assumption Verification below.

## Code Standards
No `docs/code-standards.md` found. Followed the conventions already established by `crates/ast` and `crates/probe`: `#![forbid(unsafe_code)]` crate-level, doc comments that state *why* not just *what*, `#[ignore]`d live-Ghostty tests with a doc comment giving the exact local run command, workspace-inherited `edition`/`license`/`rust-version`.

## Test Infrastructure
`cargo test` per crate, `#[ignore = "..."]` for anything needing a live Ghostty GUI session (established by `probe`'s `live_ghostty.rs`, followed here in `width`'s `live_ghostty_corpus.rs`). No `proptest` or `quickcheck` used elsewhere in the workspace yet; added `proptest` as a `width`-local dev-dependency for DW-3.2 — the natural tool for an arbitrary-string identity property, and self-contained to this crate.

## Assumption Verification

**Plan assumption (MEDIUM confidence):** "`unicode-width` + an owned correction layer can reach 100% agreement with live Ghostty over the corpus."

This could not be honestly marked COVERED from reasoning alone — it required actually measuring. A real Ghostty 1.3.1 GUI session was available in this environment (`/Applications/Ghostty.app`, confirmed `CFBundleShortVersionString` = `1.3.1`, matching `docs/spikes/ghostty-caps.md`'s pinned version — re-verified rather than copied blind, as instructed). So the corpus tool (`src/bin/measure_corpus.rs`, gated behind a `corpus-tool` feature) was built first, driven via `probe::Launcher` against a real Ghostty window, and it measured all 206 corpus cases live (`cargo test -p width --test live_ghostty_corpus --features corpus-tool -- --ignored --nocapture`, 42.5s, real GUI window opened and closed).

**Result: the assumption held, but only after one correction-layer fix found by the live measurement itself** — reported here in full rather than glossed over:

- First measurement pass: **205/206 agreement**. The one disagreement: `vs16_17` — U+2661 WHITE HEART SUIT + VS16 measured **width 1** in Ghostty, not the 2 my first-draft correction layer predicted (my draft rule was "any base char + VS16 ⇒ width 2, unconditionally"). Root cause: U+2661 has `Emoji=NO` in Unicode's own character property data — VS16 selects "emoji presentation" on a character, and that selection is a no-op if the character has no emoji-presentation form to select. My draft rule didn't gate on that; live Ghostty does. Fixed by adding `unicode-properties` (UCD `Emoji`/`Emoji_Component` table lookup, not a hand-maintained list) and gating the VS15/VS16 override on `base_char.is_emoji_char()`.
- Re-running that fix surfaced a second, related disagreement: a **standalone U+200D (ZWJ) with nothing to join** measured width 0 in Ghostty, but my "cluster contains ZWJ ⇒ 2" rule fired even on a lone ZWJ (its own single-char grapheme cluster, since there's no adjacent emoji to join). Fixed by gating the ZWJ/Fitzpatrick override on the cluster having more than one codepoint — a real *sequence*, not a bare joiner/modifier.
- After both fixes: **206/206 agreement**, verified by `test_dw_3_1_engine_agrees_with_live_ghostty_over_the_full_corpus` against the committed `corpus/ghostty-1.3.1-widths.json`.

**Conclusion: assumption CONFIRMED, not invalidated** — but the two specific disagreements above are exactly the kind of Unicode trap the plan's Approach notes anticipated ("hiding every Unicode trap"), and are recorded here so a future re-measurement (Ghostty upgrade) knows what to watch for.

## DW Verification

| DW-ID | Done-When Item | Status | Test Cases |
|-------|---------------|--------|------------|
| DW-3.1 | 100% agreement with live-Ghostty measured widths over a ≥200-case corpus (CJK, ZWJ emoji, flags, VS15/16, combining, Hangul), committed as an artifact pinned to the Ghostty version. | COVERED | `test_dw_3_1_engine_agrees_with_live_ghostty_over_the_full_corpus`, `every_corpus_category_named_by_dw_3_1_is_represented` (`crates/width/tests/corpus_agreement.rs`), backed by the committed, live-measured `crates/width/corpus/ghostty-1.3.1-widths.json` (206 cases, 12 categories) |
| DW-3.2 | Property test: `display_width` equals the sum of `cluster_width` over `graphemes` for arbitrary strings. | COVERED | `test_dw_3_2_display_width_equals_sum_of_cluster_widths_narrow`, `test_dw_3_2_display_width_equals_sum_of_cluster_widths_wide` (proptest, both ambiguous-width policies), `test_dw_3_2_holds_for_a_string_of_mixed_multi_codepoint_clusters` (`crates/width/tests/property_display_width.rs`) |
| DW-3.3 | No cluster reports width >2; zero-width classes report 0. | COVERED | `test_dw_3_3_no_cluster_reports_width_over_two` (proptest), `test_dw_3_3_zero_width_classes_report_exactly_zero`, `test_dw_3_3_wide_classes_cap_at_two_not_higher` (`crates/width/tests/property_display_width.rs`); structurally enforced in `correction.rs` (`.min(2)`, explicit 0/1/2 returns only) |

**All items COVERED:** YES

## Design Decisions

### Design: WidthEngine correction layer

#### Approaches Considered
1. **Pure per-codepoint summation** — `display_width` sums `unicode-width`'s answer for every codepoint in the string, with grapheme clustering used only to iterate. No cluster-level correction at all.
2. **Precomputed cluster lookup table** — generate a hash map of every "interesting" cluster string (every flag pair, every VS15/16 emoji sequence, every family ZWJ combination) to its measured width, built entirely from the corpus/Unicode data files; `cluster_width` becomes a table lookup with a fallback to (1).
3. **Rule-based correction layer over structural classification** (chosen) — classify a cluster by structural pattern (flag pair, VS15/VS16-with-Emoji-gate, ZWJ/Fitzpatrick sequence) and apply a small, explicit override for each; everything else falls through to the max per-codepoint `unicode-width` answer.

#### Comparison
| Criterion | A: Pure summation | B: Lookup table | C: Rule-based (chosen) |
|-----------|---|---|---|
| Interface simplicity | Trivial (no correction module) | Simple call site, huge hidden table | Simple call site, small hidden rule set |
| Information hiding | N/A — but wrong answers leak into every caller | Hides the table, but the table itself is an unbounded, unmaintainable surface (every new emoji sequence is a cache miss) | Hides UCD tables + structural rules; generalizes to inputs never seen in the corpus |
| Caller ease of use | Trivial | Trivial | Trivial |
| Correctness on the corpus | Fails ZWJ sequences (sums instead of collapsing), flag pairs (4 cells not 2), VS15/16 (no override at all) | Correct only for table hits; any emoji sequence outside the table silently falls back to (A)'s wrong answer | 206/206 on the live corpus, and correct *by construction* for any ZWJ/flag/VS15/16 cluster not in the corpus too |
| Maintenance under a Ghostty/Unicode version bump | N/A | Table must be regenerated and is unbounded in size | A handful of rules; re-running the corpus test is the regression check |

#### Choice: C (rule-based structural classification)
Rationale: (A) is disqualified outright — it fails the plan's own named edge cases (flag pairs, ZWJ, VS15/16) by construction, not just on hard corner cases. (B) trades a shallow interface for an unbounded, ever-growing hidden table that still degrades silently outside its coverage — a classic false-abstraction risk (APOSD): it looks complete but isn't, and gives no signal when it's wrong. (C) is a genuinely deep module: the three public functions hide the UCD width tables, the `Emoji`/`Emoji_Component` property lookup (via `unicode-properties`, not a hand-maintained list), and every structural correction rule, while generalizing correctly to clusters the corpus never enumerated. What's sacrificed: (C) required discovering the VS16-emoji-gate and lone-ZWJ edge cases empirically (see Assumption Verification) rather than getting them right on the first pass — a real cost, paid once, during this phase.

#### Depth Check
- Interface methods: 3 (`WidthEngine::new`, `cluster_width`, `display_width`) + 1 free fn (`graphemes`) — exactly the plan's pinned signature, no more.
- Hidden details: UCD East-Asian-Width + general-category tables (`unicode-width`); UCD `Emoji`/`Emoji_Component` property tables (`unicode-properties`); grapheme boundary algorithm (UAX #29, `unicode-segmentation`); the five structural correction rules (flag pair, VS16-if-emoji, VS15-if-emoji, ZWJ-sequence, Fitzpatrick-sequence); the ambiguous-width policy switch.
- Common case complexity: simple — `engine.cluster_width(cluster)` or `engine.display_width(s)`, no configuration beyond the one `ambiguous_wide` bool at construction.

### Corpus-tool separation
The plan's Approach notes required the engine to stay I/O-free while the measurement harness stays separate. Implemented as a `corpus-tool` Cargo feature, off by default: `probe`/`serde`/`serde_json` are optional dependencies gated by that feature, and `measure_corpus` (the bin) and `live_ghostty_corpus.rs` (the ignored test that drives it via `probe::Launcher`) both carry `required-features = ["corpus-tool"]`. Verified concretely: `cargo build --release --workspace` (default features, what CI's linkage job runs) produces `libwidth.rlib` with **no** `probe`/`crossterm` in its dependency closure — confirmed by inspecting the release target directory (only `libast.rlib`, `libprobe.rlib`, `libwidth.rlib`, `spike_a` present; no `measure_corpus`). `cargo clippy --workspace --all-targets --all-features` (CI's actual clippy invocation) does enable the feature and lints the corpus tool too.

## Prerequisites
- [x] Required files exist (created this phase): `crates/width/**`
- [x] Dependencies available (crates.io reachable; `unicode-segmentation`, `unicode-width`, `unicode-properties`, `proptest` added)
- [x] Phase 1 (`crates/probe`) available and used as-is, no changes
- [x] A real Ghostty 1.3.1 GUI session was available in this environment to produce the live corpus (verified via `open -na Ghostty.app` during Phase 1's own spikes and re-confirmed here via `defaults read .../Info.plist CFBundleShortVersionString` = `1.3.1`)

## Recommendation
BUILD (already executed — see Implementation below). No plan deviation: the pinned `Produces` signature was implementable exactly as specified, and the one MEDIUM-confidence assumption was verified true (after two correction-layer fixes surfaced by the live measurement itself, both documented above and in `correction.rs`'s doc comments).
