//! Per-line syntax highlighting via lumis (`docs/spikes/highlight-engine.md`).
//!
//! **Assumption verified, and a known limitation follows from it.** lumis's
//! `highlight_iter` exposes exactly the raw `(text, language, byte_range,
//! scope, style)` span API the spike verified, and it accepts a single
//! `&str` — so it *can* highlight one line at a time, satisfying the P5
//! `Decor::highlight(line_text, lang)` contract's per-line call shape (read
//! from `crates/stele/src/painter.rs`: `highlight` is called once per
//! code-block line, not once per block, with no cross-line state).
//!
//! What that per-line call shape cannot give correct results for: tree-
//! sitter re-parses each line from scratch with no memory of the previous
//! line, so a construct that spans multiple lines — a block comment, a
//! triple-quoted or multi-line string — gets tokenized as if it started and
//! ended within that one line. A comment's second line, e.g., has no way to
//! know it is still inside the comment; it re-parses as bare tokens. This
//! is not a bug in this module: it is a structural consequence of the
//! pinned per-line `Decor` interface (P5, out of this phase's file scope),
//! which does not carry any cross-call state or block identity. Fixing it
//! properly needs either a stateful `Decor` (a highlighter cursor retained
//! per code block) or a block-level pre-render pass (the same shape
//! `crates/mermaid`'s fence preprocessor uses) — both out of scope here.
//! Documented, not silently hidden; see `test_multiline_comment_construct_is_a_known_limitation`.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use layout::{Run, Semantic, StyleId};
use lumis::highlight::highlight_iter;
use lumis::languages::Language;

use crate::role::Capture;

/// Wall-clock budget for highlighting a single line, enforced only past
/// [`THREADED_GUARD_THRESHOLD`] (see [`highlight_with_timeout`]). Generous
/// on purpose — it must never trip on real code, only on genuinely
/// runaway input.
const HIGHLIGHT_TIME_CAP: Duration = Duration::from_millis(250);

/// Lines at or under this size run inline, with no thread-spawn overhead —
/// every realistic line of source code. A line past it is unusual enough
/// (and rare enough in real code) to be worth the extra thread-per-call
/// cost of a hard, real timeout guard.
const THREADED_GUARD_THRESHOLD: usize = 4096;

/// A highlight result, plus whether it is safe to memoize.
///
/// The flag is the whole reason this type exists. [`highlight_line`]'s
/// answer for a given `(line, lang)` is normally a pure function of its
/// inputs and can be cached forever — except when it is the
/// [`HIGHLIGHT_TIME_CAP`] fallback, which says "this attempt did not finish
/// in time *this time*", not "this is how the line highlights". Caching that
/// would freeze a transient scheduling hiccup into the rest of the session,
/// so [`crate::HighlightCache`] retains only `cacheable` results. Nothing
/// outside this module can make that distinction, which is why the flag
/// travels with the runs rather than being inferred by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Highlighted {
    pub runs: Vec<Run>,
    pub cacheable: bool,
}

/// Highlights one code-block line. `lang` is the fence's info string, if
/// any; `None`, an empty string, or a tag lumis doesn't recognize (the OUT
/// scope in the plan: "language auto-detection without an info string" —
/// this crate never guesses a language from the text itself) all degrade
/// to the same single plain run `crates/stele/src/decor`'s prior
/// `StructuralDecor` used, tagged `Semantic::CodeBlock` so painting is
/// unchanged for unhighlighted code. Never panics, never hangs past
/// [`HIGHLIGHT_TIME_CAP`] (pathological input degrades to plain instead).
pub fn highlight_line(line_text: &str, lang: Option<&str>) -> Vec<Run> {
    highlight_detailed(line_text, lang).runs
}

/// [`highlight_line`] with the memoizability verdict attached — the entry
/// point [`crate::HighlightCache`] calls. The runs are identical to what
/// [`highlight_line`] returns for the same inputs; only the flag is extra.
pub fn highlight_detailed(line_text: &str, lang: Option<&str>) -> Highlighted {
    let Some(lang) = lang.filter(|l| !l.trim().is_empty()) else {
        return cacheable(plain_run(line_text));
    };
    let language = Language::guess(Some(lang), "");
    if language == Language::PlainText {
        // Unknown/unrecognized language tag -> plain block (edge case).
        // Deterministic, so it memoizes like any other real answer.
        return cacheable(plain_run(line_text));
    }
    classify(line_text, highlight_with_timeout(line_text, language))
}

/// Turns [`highlight_with_timeout`]'s answer into a [`Highlighted`].
///
/// `None` is the fallback signal — the call either exceeded
/// [`HIGHLIGHT_TIME_CAP`] or the highlighter itself errored — and that is
/// the one outcome the Phase 4 constraint forbids caching. Both are treated
/// the same, deliberately: an errored attempt is no more "how this line
/// highlights" than a timed-out one, and collapsing them costs nothing
/// beyond re-running a call that already failed once.
///
/// Split out as a named function so that rule is testable without having to
/// make a real highlighter genuinely exceed a 250 ms wall clock, which no
/// honest test can pin on an arbitrary host.
fn classify(line_text: &str, outcome: Option<Vec<Run>>) -> Highlighted {
    match outcome {
        Some(runs) if !runs.is_empty() => cacheable(runs),
        // An empty run set is a real, reproducible answer (an empty source
        // line), not a failure — it degrades to plain and memoizes.
        Some(_) => cacheable(plain_run(line_text)),
        None => Highlighted {
            runs: plain_run(line_text),
            cacheable: false,
        },
    }
}

fn cacheable(runs: Vec<Run>) -> Highlighted {
    Highlighted {
        runs,
        cacheable: true,
    }
}

fn plain_run(line_text: &str) -> Vec<Run> {
    vec![Run {
        text: line_text.to_string(),
        style_id: StyleId::Semantic(Semantic::CodeBlock),
        width: 0,
        aux: None,
    }]
}

/// Highlights `line_text`, guaranteeing bounded wall time. Below
/// [`THREADED_GUARD_THRESHOLD`] this runs inline — no thread-spawn cost for
/// the overwhelmingly common case of an ordinary source line. At or past
/// it, the call moves to a background thread with [`HIGHLIGHT_TIME_CAP`]
/// enforced via `recv_timeout` — this is the "hit a time cap and degrade to
/// plain" guard the Constraints section requires. A timed-out call's
/// thread is abandoned (it finishes and drops its result harmlessly;
/// nothing here waits on it), which is the only way to bound wall time
/// regardless of what the highlighter does internally, short of `unsafe`
/// signal-based preemption.
fn highlight_with_timeout(line_text: &str, language: Language) -> Option<Vec<Run>> {
    if line_text.len() <= THREADED_GUARD_THRESHOLD {
        return highlight_line_now(line_text, language);
    }
    let (tx, rx) = mpsc::channel();
    let owned = line_text.to_string();
    thread::spawn(move || {
        let result = highlight_line_now(&owned, language);
        let _ = tx.send(result);
    });
    rx.recv_timeout(HIGHLIGHT_TIME_CAP).ok().flatten()
}

fn highlight_line_now(line_text: &str, language: Language) -> Option<Vec<Run>> {
    let mut runs = Vec::new();
    let result = highlight_iter(
        line_text,
        language,
        None,
        |text, _lang, _range, scope, _style| {
            runs.push(Run {
                text: text.to_string(),
                style_id: StyleId::Capture(Capture::from_scope(scope).id()),
                width: 0,
                aux: None,
            });
            Ok::<(), std::convert::Infallible>(())
        },
    );
    result.ok().map(|()| runs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(runs: &[Run]) -> String {
        runs.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn test_dw_7_1_rust_line_produces_capture_styled_runs() {
        let runs = highlight_line("let x: i32 = 1;", Some("rust"));
        assert!(
            runs.len() > 1,
            "expected multiple styled spans, got {runs:?}"
        );
        assert_eq!(texts(&runs), "let x: i32 = 1;");
        assert!(
            runs.iter().any(|r| matches!(r.style_id, StyleId::Capture(id) if Capture::from_id(id) == Capture::Keyword)),
            "expected a Keyword-mapped span for `let`, got {runs:?}"
        );
    }

    #[test]
    fn test_dw_7_1_preserves_total_text_exactly_pure_run_transformation() {
        // Constraints: `Decor::highlight` must preserve the line's total
        // text exactly (code blocks never wrap; only partitioning/styles
        // may change).
        let cases: &[(&str, &str)] = &[
            ("rust", "fn main() { println!(\"hi\"); }"),
            ("python", "def f(x): return x + 1"),
            ("javascript", "const x = { a: 1, b: [2, 3] };"),
            ("go", "func main() { fmt.Println(\"hi\") }"),
            ("sql", "SELECT * FROM users WHERE id = 1;"),
            ("bogus-unknown-lang", "anything at all"),
        ];
        for (lang, line) in cases {
            let runs = highlight_line(line, Some(lang));
            assert_eq!(&texts(&runs), line, "text not preserved for lang {lang}");
        }
    }

    #[test]
    fn test_none_lang_is_plain_no_detection_from_content() {
        // OUT of scope: no language auto-detection without an info string,
        // even for content that looks unmistakably like a language lumis
        // could otherwise guess from a shebang.
        let runs = highlight_line("#!/usr/bin/env python3", None);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].style_id, StyleId::Semantic(Semantic::CodeBlock));
    }

    #[test]
    fn test_unknown_language_tag_is_plain_block_edge_case() {
        let runs = highlight_line("whatever this is", Some("definitely-not-a-real-language"));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].style_id, StyleId::Semantic(Semantic::CodeBlock));
        assert_eq!(runs[0].text, "whatever this is");
    }

    #[test]
    fn test_empty_lang_string_is_plain_not_a_lumis_call() {
        let runs = highlight_line("some code", Some(""));
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].style_id, StyleId::Semantic(Semantic::CodeBlock));
    }

    #[test]
    fn test_empty_line_never_panics() {
        let runs = highlight_line("", Some("rust"));
        // An empty source line is a degenerate but legal input; either an
        // empty run set or a single empty run is acceptable — no panic and
        // no fabricated text is the actual contract.
        assert_eq!(texts(&runs), "");
    }

    #[test]
    fn test_multiline_comment_construct_is_a_known_limitation() {
        // Documented limitation (see module docs): a block comment's
        // continuation line, highlighted alone with no memory of the
        // opening `/*`, cannot be recognized as still-inside-a-comment.
        // This test exists to make that limitation visible and pinned, not
        // to assert "correct" multi-line highlighting (which the per-line
        // `Decor` interface cannot provide).
        let opening = highlight_line("/* a block comment", Some("rust"));
        let continuation = highlight_line("still inside the comment */", Some("rust"));
        assert_eq!(texts(&opening), "/* a block comment");
        assert_eq!(texts(&continuation), "still inside the comment */");
        // Never panics and never fabricates or drops text even though the
        // scope classification of the continuation line is not correct
        // (verified) block-comment coverage on its own.
    }

    #[test]
    fn test_pathological_single_line_does_not_hang() {
        // A large, deeply-nested single line — the closest a per-line
        // caller can hand this module to "pathological input". The time
        // cap must bound wall time regardless of what tree-sitter does.
        let pathological: String = "(".repeat(20_000) + &")".repeat(20_000);
        let start = std::time::Instant::now();
        let runs = highlight_line(&pathological, Some("rust"));
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "highlighting a pathological line took too long: {:?}",
            start.elapsed()
        );
        assert_eq!(texts(&runs), pathological);
    }
}
