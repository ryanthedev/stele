# Vendored spec assets — provenance

Vendored 2026-07-22 so conformance runs offline in CI. Do not edit by hand.

| File | Source | Version/tag | sha256 |
|---|---|---|---|
| `commonmark-spec.txt` | github.com/commonmark/commonmark-spec `spec.txt` | tag `0.31.2` | `257c41ad946f7a1414a499aca402a1aa8fdac3678532266611348c1cf54f4b80` |
| `gfm-spec.txt` | github.com/github/cmark-gfm `test/spec.txt` | tag `0.29.0.gfm.13` | `7d8e5814befec287ac116786d81ff14e0adc9b13295b4494649e995408fd871c` |
| `entities.json` | html.spec.whatwg.org/entities.json | living standard (fetched 2026-07-22) | `d741d877ac77c4194c4ad526b5b4a19aef8dfe411ab840a466891cdbb9f362e6` |
| `spec_tests.py`, `normalize.py` | commonmark-spec `test/` | tag `0.31.2` | (reference originals) |

## Generated files

- `commonmark-0.31.2.json` — all 652 examples, extracted from
  `commonmark-spec.txt` by `extract_tests.py` (this directory), which
  replicates `spec_tests.py --dump-tests` (that original imports a `cmark`
  module at top level and cannot run standalone). Regenerate with
  `python3 extract_tests.py`.
- `gfm-extensions.json` — the 23 examples of the four extension sections
  (Tables, Task list items, Strikethrough, Autolinks) extracted from
  `gfm-spec.txt` the same way. The Disallowed-Raw-HTML extension is out of
  scope (stele preserves raw HTML as literal text).
- `../../src/parser/tables.rs`, `../../src/parser/entities.rs` — generated
  by `gen_tables.py` (in this directory) from Python `unicodedata`
  (UCD 16.0.0) and `entities.json`. Regenerate with
  `python3 gen_tables.py && cargo fmt`.
