# Conformance ledger — crates/ast

Measured by `tests/conformance.rs` against the vendored suites in
`tests/spec/` (provenance there in `README.md`).

## CommonMark 0.31.2

**652/652 examples pass** in the `ParseOptions::commonmark()` profile.

Documented deviations (cap 3, enforced by `test_dw_2_1_commonmark_conformance`):

*None.*

## GFM extensions (cmark-gfm 0.29.0.gfm.13 spec)

All vendored extension cases pass in the default (all-extensions) profile:

| Suite | Cases |
|---|---|
| Tables (extension) | 8/8 |
| Autolinks (extension) | 11/11 |
| Task list items (extension) | 2/2 |
| Strikethrough (extension) | 2/2 |

## Profiles

Extensions are compile-time-independent, runtime-toggled (`ParseOptions`).
`Document::parse` — the product configuration — enables everything
(GFM + math + footnotes + alerts + frontmatter). The CommonMark suite is
measured with all extensions off, exactly as the reference implementations
measure themselves (extensions change the language: e.g. always-on autolink
literals or frontmatter would contradict CommonMark examples 96/98/608/611/612
by design, not by defect). Both profiles run through the same parser code.
