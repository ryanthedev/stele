# Discovery + Design: Phase 1b - Close the DOCTYPE-scan bypass

## Files Found
- `crates/gfx/src/svg.rs` — `refuse_internal_subset` at 94-130 (doc 94-112, body 113-130), called from `parse` at 361. Tests module holds `test_a_wide_entity_bomb_is_refused` (742), `test_a_plain_doctype_is_still_accepted` (767), `test_an_entity_bomb_is_refused_rather_than_expanded` (656).
- `crates/gfx/src/decode.rs:249` — `probe_dimensions(path, limits)`, the public path for the DW-1b.6 reproduction; it routes SVG through `svg::read_source` → `svg::probe`.
- `docs/code-standards.md` — present.

## Current State
The scan ends the DOCTYPE at the first `>` anywhere in the declaration:

```rust
let rest = &prologue[doctype..];
let end = rest.find('>').unwrap_or(rest.len());
if rest[..end].contains('[') { /* refuse */ }
```

Quote-blind. It also silently accepts a DOCTYPE with no `>` at all (`unwrap_or(rest.len())`).

## Gaps
| # | Gap | Consequence |
|---|---|---|
| 1 | `>` inside a `SystemLiteral` truncates the examined slice before `[` | Internal subset never seen — the reproduced bypass |
| 2 | `[` inside a literal would be read as a subset opening | False refusal of a legal `SYSTEM "a[b"` |
| 3 | Missing `>` is treated as "no subset" | Accepts a declaration the scan cannot actually parse |
| 4 | Doc says "refusing that construct refuses the entire class" but says nothing about literals | Claim is false pre-fix; must be made true and explained |

## Code Standards
Test names are sentences (`test_a_...`), DW-tracing tests carry the DW id (`test_dw_1b_1_...`). `#![forbid(unsafe_code)]` workspace-wide; clippy `-D warnings`.

## Test Infrastructure
`#[cfg(test)] mod tests` inline at the bottom of `svg.rs`; fixtures are inline `&str` constants; assertions go through the module-public `probe`/`rasterize` rather than the private guard, so the tests exercise the real path. `cargo test -p gfx`, then `--workspace --all-features`.

## Design Decisions

**Verified against the XML 1.0 spec** (fetched from https://www.w3.org/TR/xml/, productions quoted verbatim):

- `[28] doctypedecl ::= '<!DOCTYPE' S Name (S ExternalID)? S? ('[' intSubset ']' S?)? '>'`
- `[75] ExternalID ::= 'SYSTEM' S SystemLiteral | 'PUBLIC' S PubidLiteral S SystemLiteral`
- `[11] SystemLiteral ::= ('"' [^"]* '"') | ("'" [^']* "'")`
- `[12] PubidLiteral ::= '"' PubidChar* '"' | "'" (PubidChar - "'")* "'"`
- `[13] PubidChar ::= #x20 | #xD | #xA | [a-zA-Z0-9] | [-'()+,./:=?;!*#@$_%]`

Two consequences the design rests on:

1. `SystemLiteral` is `[^"]*` / `[^']*` — it may hold `>` and `[`. `PubidChar` lists neither, so a `PubidLiteral` legally cannot. **I treat both literals uniformly** and do not rely on that asymmetry: a quote-aware scan that skips any quoted run is correct for both, and a hostile document is not obliged to be legal — a `>` inside what claims to be a `PubidLiteral` must not be allowed to end the declaration either.
2. Between `<!DOCTYPE` and the internal subset's `[`, production 28 admits only `S`, `Name` and `ExternalID`. `Name` cannot contain `>` or a quote, and `ExternalID`'s only free-form regions are the two quoted literals. **So no unquoted `>` can legally precede the `[`.** A single left-to-right pass that tracks quote state and stops at whichever of `[` or `>` it meets first outside a quote is therefore exact, not approximate.

**Chosen implementation:** replace the two-line body with one left-to-right byte scan over the declaration, carrying `quote: Option<u8>`:
- inside a quote, only the matching delimiter is significant;
- outside, `"` or `'` opens a quote, `[` refuses, `>` accepts.

Byte-wise is safe on `&str` here because every delimiter is ASCII and no UTF-8 continuation byte is ASCII — the same reasoning `contains_element` already relies on.

**Alternative rejected:** regex or a DOCTYPE mini-parser. Both add a dependency or a parser to sit in front of a parser, for a decision that is one flag and four characters wide.

**Running off the end refuses, with its own message.** An unterminated literal (or a declaration with no `>` at all) means the scan cannot tell what the document declares. Per the phase brief's standing instruction — prefer refusing an input you cannot parse confidently — that refuses. It gets a *different* message ("svg: DOCTYPE does not close before the root element") rather than reusing the entity message, because claiming the document "declares its own XML entities" when we could not read that far would be exactly the kind of false claim this plan exists to remove. DW-1b.4 asks only that it be refused rather than accepted, which this satisfies.

**Doc rewrite:** the doc must now state how quoted literals are handled and cite the productions that make the single pass exact, replacing the bare "refusing that construct refuses the entire class".

## DW Verification

| DW-ID | Done-When Item | Status | Test Cases |
|-------|---------------|--------|------------|
| DW-1b.1 | `SYSTEM "x>y" [ <!ENTITY ...> ]` refused, same error as unquoted; fails pre-fix | COVERED | `test_dw_1b_1_a_subset_behind_a_quoted_gt_is_refused_like_the_unquoted_twin` — asserts the message contains `own XML entities` **and** that it is byte-identical to the unquoted twin's |
| DW-1b.2 | Single-quoted `SYSTEM 'x>y'` and two-literal `PUBLIC "…>…" "…>…"` refused, each its own test | COVERED | `test_dw_1b_2_a_single_quoted_literal_hides_no_subset_either`, `test_dw_1b_2_both_public_literals_are_walked_for_a_hidden_subset` |
| DW-1b.3 | `SYSTEM "a[b"` still parses | COVERED | `test_dw_1b_3_a_bracket_inside_a_literal_is_not_an_internal_subset` — asserts `probe` returns the declared size |
| DW-1b.4 | Unterminated literal refused, no panic / OOB | COVERED | `test_dw_1b_4_an_unterminated_literal_is_refused_rather_than_walked_past` plus `test_dw_1b_4_no_prefix_of_a_hostile_doctype_walks_out_of_bounds` (every byte prefix of a hostile DOCTYPE) |
| DW-1b.5 | `test_a_plain_doctype_is_still_accepted` passes unchanged | COVERED | that test, byte-for-byte unchanged, re-run |
| DW-1b.6 | `evil.svg` reproduction inverted through `probe_dimensions` | COVERED | `test_dw_1b_6_the_reproduced_bypass_is_refused_through_probe_dimensions` (writes the fixture to a tempdir, drives `crate::decode::probe_dimensions`); output recorded below |
| DW-1b.7 | Workspace suite green ≥806/0, clippy silent, fmt clean | COVERED | the three commands, run and recorded |

**All items COVERED:** YES

## Demonstrated Failure (pre-fix body restored, then reverted)

`cargo test -p gfx --all-features --lib dw_1b` with the two-line pre-fix scan in place: **1 passed, 6 failed.**

| Test | Pre-fix outcome |
|---|---|
| `..._1b_1_a_subset_behind_a_quoted_gt_...` | FAILED — `an internal subset is refused: (10, 10)` |
| `..._1b_2_a_single_quoted_literal_...` | FAILED — `an internal subset is refused: (10, 10)` |
| `..._1b_2_both_public_literals_...` | FAILED — `an internal subset is refused: (10, 10)` |
| `..._1b_3_a_bracket_inside_a_literal_...` | FAILED — `parses: Malformed("svg: declares its own XML entities…")` (the false-positive half) |
| `..._1b_4_an_unterminated_literal_...` | FAILED — `Malformed("svg: expected '[' or '>' not 'h' at 3:13")`: the guard passed it through and **roxmltree** rejected it downstream, so it was not silently accepted pre-fix, but it was not this guard that refused it |
| `..._1b_4_no_prefix_..._out_of_bounds` | **passed** — a no-panic property, not a bypass pin; it holds under both bodies |
| `..._1b_6_..._through_probe_dimensions` | FAILED — `the hidden subset is refused on the public path: (10, 10)` |

Measured on the same fixture against the pre-fix body, through `decode::probe_dimensions`:

```
evil.svg   SYSTEM "x>y" (24759 bytes) -> Ok((10, 10))                    in 11.976833ms
benign.svg SYSTEM "xy"  (24758 bytes) -> Err(Malformed("svg: declares    in   123.667µs
                                            its own XML entities…"))
entity text expanded by the accepted file: 262144 bytes
```

Post-fix, `evil.svg` returns the same `Err(Malformed("svg: declares its own XML entities, which this refuses to expand"))` as its twin.

## Prerequisites
- [x] Phase 1 committed (`e3d2a80`); working tree clean
- [x] `probe_dimensions` reachable from `gfx`'s own test module as `crate::decode::probe_dimensions`
- [x] `tempfile` available for the DW-1b.6 fixture — to confirm in `crates/gfx/Cargo.toml` dev-dependencies; falls back to `std::env::temp_dir` if absent

## Recommendation
BUILD. Replace the guard body with a quote-aware single pass, rewrite its doc to state and cite what makes that pass exact, add the six regression tests, and demonstrate the DW-1b.1 test failing against the pre-fix body.
