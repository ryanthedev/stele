//! Where a source rewrite may **not** go.
//!
//! Every preprocessor in this directory works on raw text, before the parse,
//! so none of them can ask the AST whether a line is code. They have to
//! decide for themselves — and getting that wrong is not a cosmetic bug: a
//! ` ```markdown ` fence in a tutorial about d2l or Quarto contains the very
//! constructs those passes rewrite, and rewriting them turns a document
//! *about* the syntax into a document that has silently lost it.
//!
//! [`Scan`] answers the one question they all need — "may line `n` be
//! rewritten?" — from two sources:
//!
//! * **Fenced code blocks**, honouring what CommonMark actually says: either
//!   fence character, a closing run at least as long as the opening one (this
//!   is what makes ` ```` `-wrapped ` ``` ` examples work, and the corpus is
//!   full of them), an info string on the opener only, and indentation.
//! * **Pandoc grid tables**, wholesale. Not because a grid table is code, but
//!   because its columns are *positional*: a rewrite that changes a line's
//!   byte length inside one moves every `|` after it out of alignment and the
//!   table stops being a table. A grid table is also where the fence scanner
//!   is blindest — a cell containing `` | ``` markdown `` opens a fence that
//!   starts nowhere near column 0, so the fence machinery below cannot see
//!   it. Protecting the whole table covers both problems with one rule.
//!
//! ## Unclosed constructs leave the document alone
//!
//! [`Scan::balanced`] is false when a fence is still open at end of file, and
//! every pass declines to touch a document that reports false. Under
//! `--watch` this is not a hypothetical: editors save in two steps, and for a
//! few milliseconds the file on disk ends mid-fence. Classifying the tail of
//! a half-written file as "code" (or, worse, as "not code") would reshuffle
//! the whole document for one frame and then reshuffle it back. Declining is
//! the only answer that is stable across the save.
//!
//! ## Which direction this errs in
//!
//! [`delimiter`] looks for a fence after stripping list and blockquote
//! markers, so it sees `- - ``` markdown` — a real shape in the corpus, and
//! one a column-0 scanner misses. The cost is that a ` ``` ` sitting inside a
//! four-space *indented* code block also reads as a fence. That is the
//! harmless direction twice over: at worst it protects lines that could have
//! been rewritten (a missed rewrite), or it leaves a fence unclosed at EOF
//! and the whole document is returned untouched. Neither can corrupt text.
//! The opposite error — failing to notice a fence and rewriting its contents
//! — is the one that destroys a document, so every judgement call here is
//! made in favour of protecting more.

/// One fenced code block, as [`Scan`] found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fence {
    /// Zero-based index of the opening delimiter line.
    pub open: usize,
    /// Zero-based index of the closing delimiter line, or `None` when the
    /// fence runs to end of file — the case that makes [`Scan::balanced`]
    /// false.
    pub close: Option<usize>,
    /// The opening line's info string, trimmed; empty when there is none.
    pub info: String,
    /// Whether the opening delimiter sits at the document's top level —
    /// at most three leading spaces, no list or blockquote marker in front
    /// of it. A pass that rewrites a fence's *own* lines (rather than merely
    /// avoiding them) should only touch these: the indentation of a fence
    /// nested in a list item is load-bearing, and reproducing it correctly
    /// is a different job from recognising the fence.
    pub top_level: bool,
}

/// Which lines of a document are off limits to a rewrite.
///
/// Line indices are into `source.split_inclusive('\n')`, so index `n` here is
/// the same line every pass in this directory iterates — including the last
/// one when the file has no trailing newline.
#[derive(Debug, Clone)]
pub struct Scan {
    protected: Vec<bool>,
    fences: Vec<Fence>,
    balanced: bool,
}

impl Scan {
    /// Classifies every line of `source`.
    pub fn of(source: &str) -> Scan {
        let lines: Vec<&str> = source.split_inclusive('\n').collect();
        let mut protected = vec![false; lines.len()];
        let mut fences: Vec<Fence> = Vec::new();
        let mut balanced = true;

        let mut index = 0usize;
        while index < lines.len() {
            let line = strip_eol(lines[index]);

            // A grid table swallows its whole extent, borders included. It is
            // checked before the fence probe because a border line can never
            // be a fence delimiter, and because the `|` rows inside it must
            // not be handed to the fence machinery at all.
            if let Some(end) = grid_table_end(&lines, index) {
                for entry in &mut protected[index..=end] {
                    *entry = true;
                }
                index = end + 1;
                continue;
            }

            let Some((fence_char, run)) = delimiter(line) else {
                index += 1;
                continue;
            };

            // An opener's info string is whatever follows the run. A fence
            // whose "info" is empty still opens a block — CommonMark has no
            // notion of a delimiter that can only close.
            let info = delimiter_tail(line, fence_char, run).trim().to_string();
            let open = index;
            protected[open] = true;

            let mut close = None;
            let mut cursor = open + 1;
            while cursor < lines.len() {
                protected[cursor] = true;
                let candidate = strip_eol(lines[cursor]);
                if let Some((candidate_char, candidate_run)) = delimiter(candidate)
                    && candidate_char == fence_char
                    && candidate_run >= run
                    && delimiter_tail(candidate, candidate_char, candidate_run)
                        .trim()
                        .is_empty()
                {
                    close = Some(cursor);
                    break;
                }
                cursor += 1;
            }
            if close.is_none() {
                balanced = false;
            }

            fences.push(Fence {
                open,
                close,
                info,
                top_level: top_level_delimiter(line),
            });
            index = close.map_or(lines.len(), |c| c + 1);
        }

        Scan {
            protected,
            fences,
            balanced,
        }
    }

    /// Whether line `line` may not be rewritten.
    ///
    /// An index past the end reports `true`. A caller asking about a line
    /// that does not exist has already lost track of the document, and the
    /// fail-safe answer to "may I edit this?" is no.
    pub fn is_protected(&self, line: usize) -> bool {
        self.protected.get(line).copied().unwrap_or(true)
    }

    /// Every fenced code block, in source order.
    pub fn fences(&self) -> &[Fence] {
        &self.fences
    }

    /// False when a fence was still open at end of file. Passes return the
    /// source untouched in that case; see the module doc.
    pub fn balanced(&self) -> bool {
        self.balanced
    }
}

/// A line with its `\n` (and any `\r` before it) removed, for analysis.
///
/// The passes rejoin `split_inclusive` slices verbatim, so the terminator is
/// never edited — but it must not be *measured* either, or a CRLF document
/// would see a stray `\r` at the end of every info string and class name.
pub fn strip_eol(line: &str) -> &str {
    line.trim_end_matches('\n').trim_end_matches('\r')
}

/// `line` with leading whitespace and any run of block-container markers
/// (list bullets, ordered-list numbers, blockquote `>`) removed.
///
/// This is what lets the scanner see `- - ``` markdown`, and what lets
/// `quarto` see a `:::` closer indented four spaces inside a list item.
/// Both are real shapes in the corpus and both are invisible to a scanner
/// that only looks at column 0.
pub fn container_content(line: &str) -> &str {
    let mut rest = line.trim_start_matches([' ', '\t']);
    loop {
        let stripped = strip_one_marker(rest);
        match stripped {
            Some(next) => rest = next.trim_start_matches([' ', '\t']),
            None => return rest,
        }
    }
}

/// One list bullet, ordered-list number, or blockquote marker off the front
/// of `rest`, or `None` when it does not start with one.
///
/// A bullet must be followed by whitespace, so `*emphasis*` and `-5 degrees`
/// are not mistaken for markers. `>` needs no following space: `>>quoted` is
/// two levels of quote.
fn strip_one_marker(rest: &str) -> Option<&str> {
    let bytes = rest.as_bytes();
    if bytes.first() == Some(&b'>') {
        return Some(&rest[1..]);
    }
    if matches!(bytes.first(), Some(b'-' | b'*' | b'+'))
        && matches!(bytes.get(1), Some(b' ' | b'\t'))
    {
        return Some(&rest[1..]);
    }
    // `12.` / `12)` — CommonMark caps an ordered marker at nine digits.
    let digits = bytes
        .iter()
        .take(10)
        .take_while(|b| b.is_ascii_digit())
        .count();
    if (1..=9).contains(&digits)
        && matches!(bytes.get(digits), Some(b'.' | b')'))
        && matches!(bytes.get(digits + 1), Some(b' ' | b'\t'))
    {
        return Some(&rest[digits + 1..]);
    }
    None
}

/// The fence delimiter `line` carries — `(character, run length)` — or `None`.
///
/// A backtick fence whose info string contains a backtick is not a fence
/// (CommonMark forbids it, and the rule is what keeps a line of prose full of
/// code spans from opening a block that swallows the rest of the file).
pub fn delimiter(line: &str) -> Option<(u8, usize)> {
    let body = container_content(line);
    let first = *body.as_bytes().first()?;
    if first != b'`' && first != b'~' {
        return None;
    }
    let run = body.bytes().take_while(|&b| b == first).count();
    if run < 3 {
        return None;
    }
    if first == b'`' && body[run..].contains('`') {
        return None;
    }
    Some((first, run))
}

/// Whatever follows the delimiter run on `line` — the info string on an
/// opener, and necessarily blank on a closer.
fn delimiter_tail(line: &str, fence_char: u8, run: usize) -> &str {
    let body = container_content(line);
    debug_assert!(body.as_bytes().first() == Some(&fence_char));
    &body[run..]
}

/// Whether `line`'s delimiter is at the document's top level: at most three
/// leading spaces and no container marker in front of it.
fn top_level_delimiter(line: &str) -> bool {
    let indent = line.bytes().take_while(|&b| b == b' ').count();
    indent <= 3 && line[indent..].starts_with(['`', '~'])
}

/// Whether `line` is a Pandoc grid-table rule: `+---+---+` or `+===+===+`.
///
/// No minimum length rides along with these four clauses. The shortest thing
/// that can satisfy all of them is `+-+`: a shorter body would have to be
/// `++` or `+`, and neither carries the `-` or `=` the last clause insists
/// on. A length check would therefore be a branch no line can take.
fn is_grid_rule(line: &str) -> bool {
    let indent = line.bytes().take_while(|&b| b == b' ').count();
    if indent > 3 {
        return false;
    }
    let body = &line[indent..];
    body.starts_with('+')
        && body.ends_with('+')
        && body.bytes().all(|b| matches!(b, b'+' | b'-' | b'='))
        && body.bytes().any(|b| matches!(b, b'-' | b'='))
}

/// Whether `line` could be a grid-table row: `| … |`, or a rule.
fn is_grid_row(line: &str) -> bool {
    let indent = line.bytes().take_while(|&b| b == b' ').count();
    indent <= 3 && line[indent..].starts_with('|')
}

/// The index of the last rule line of the grid table starting at `start`, or
/// `None` when no table starts there.
///
/// The table runs while lines are rules or `|` rows, and *ends at the last
/// rule* — so a GFM pipe table written directly under a grid table is not
/// swallowed into it.
fn grid_table_end(lines: &[&str], start: usize) -> Option<usize> {
    if !is_grid_rule(strip_eol(lines[start])) {
        return None;
    }
    let mut end = start;
    let mut cursor = start + 1;
    while cursor < lines.len() {
        let line = strip_eol(lines[cursor]);
        if is_grid_rule(line) {
            end = cursor;
        } else if !is_grid_row(line) {
            break;
        }
        cursor += 1;
    }
    Some(end)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The set of protected line indices, as an easy thing to assert on.
    fn protected(source: &str) -> Vec<usize> {
        let scan = Scan::of(source);
        (0..source.split_inclusive('\n').count())
            .filter(|&i| scan.is_protected(i))
            .collect()
    }

    #[test]
    fn test_a_plain_document_protects_nothing() {
        assert_eq!(protected("# Title\n\nSome prose.\n"), Vec::<usize>::new());
        assert!(Scan::of("# Title\n\nSome prose.\n").balanced());
    }

    #[test]
    fn test_a_backtick_fence_protects_its_delimiters_and_its_body() {
        let src = "before\n```rust\nfn main() {}\n```\nafter\n";
        assert_eq!(protected(src), vec![1, 2, 3]);
        let scan = Scan::of(src);
        assert_eq!(scan.fences().len(), 1);
        assert_eq!(scan.fences()[0].info, "rust");
        assert_eq!(scan.fences()[0].open, 1);
        assert_eq!(scan.fences()[0].close, Some(3));
        assert!(scan.fences()[0].top_level);
    }

    #[test]
    fn test_a_tilde_fence_is_a_fence_too_and_backticks_do_not_close_it() {
        let src = "~~~\n```\nstill inside\n~~~\nout\n";
        assert_eq!(protected(src), vec![0, 1, 2, 3]);
        let scan = Scan::of(src);
        assert_eq!(scan.fences().len(), 1, "{:?}", scan.fences());
        assert_eq!(scan.fences()[0].close, Some(3));
    }

    /// The rule that makes file 06's ` ```` `-wrapped ` ``` ` examples work:
    /// a closing run must be at least as long as the opening one, so the
    /// inner three-backtick pair is content, not a close-then-reopen.
    #[test]
    fn test_a_shorter_run_inside_a_longer_fence_does_not_close_it() {
        let src = "````markdown\n```python\nx = 1\n```\n````\nafter\n";
        assert_eq!(protected(src), vec![0, 1, 2, 3, 4]);
        let scan = Scan::of(src);
        assert_eq!(scan.fences().len(), 1);
        assert_eq!(scan.fences()[0].close, Some(4));
        // The oracle nobody can fake: the real parser agrees there is one
        // code block here, and its literal holds the inner fence.
        let doc = ast::Document::parse(src);
        let ast::BlockKind::CodeBlock { literal, .. } = &doc.blocks()[0].kind else {
            panic!("expected a code block, got {:?}", doc.blocks()[0].kind);
        };
        assert!(literal.contains("```python"));
    }

    /// The opposite direction: a *longer* closing run does close a shorter
    /// fence, which is why the closer's run is compared with `>=` and not
    /// `==`.
    #[test]
    fn test_a_longer_closing_run_still_closes_a_shorter_fence() {
        let scan = Scan::of("```\nbody\n`````\nafter\n");
        assert_eq!(scan.fences()[0].close, Some(2));
        assert!(!scan.is_protected(3));
    }

    #[test]
    fn test_a_delimiter_carrying_an_info_string_never_closes_a_fence() {
        let scan = Scan::of("```\nbody\n``` rust\nstill body\n```\n");
        assert_eq!(scan.fences().len(), 1);
        assert_eq!(scan.fences()[0].close, Some(4));
    }

    #[test]
    fn test_a_fence_indented_up_to_three_spaces_is_still_top_level() {
        let scan = Scan::of("   ```rust\n   fn f() {}\n   ```\n");
        assert_eq!(scan.fences().len(), 1);
        assert!(scan.fences()[0].top_level);
        assert_eq!(scan.fences()[0].info, "rust");
    }

    /// R8's list half, verbatim from `fixtures/preprocessor-lookalikes.md`
    /// and from file 06 of the corpus: the fence opens *after* two list
    /// markers, so its backticks are at column four and a column-0 scanner
    /// sees nothing at all.
    #[test]
    fn test_a_fence_opened_after_list_markers_is_found_and_is_not_top_level() {
        let src = "- - ``` markdown\n    ::: {.callout-note}\n    ```\nafter\n";
        assert_eq!(protected(src), vec![0, 1, 2]);
        let scan = Scan::of(src);
        assert_eq!(scan.fences().len(), 1);
        assert_eq!(scan.fences()[0].info, "markdown");
        assert!(
            !scan.fences()[0].top_level,
            "a list-nested fence must not be offered up for its own line to be rewritten"
        );
    }

    #[test]
    fn test_a_fence_inside_a_blockquote_is_found() {
        let src = "> ```\n> secret\n> ```\nout\n";
        assert_eq!(protected(src), vec![0, 1, 2]);
    }

    /// R8's grid-table half. The fence lives inside a cell, so its backticks
    /// are behind a `|` and the fence machinery is blind to it — the table
    /// protection is what saves the cell contents.
    #[test]
    fn test_a_pandoc_grid_table_is_protected_whole_including_its_cell_fences() {
        let src = concat!(
            "intro\n",
            "+--------------+--------------+\n",
            "| Markdown     | Output       |\n",
            "+==============+==============+\n",
            "| ``` markdown | :::note      |\n",
            "| :::note      |              |\n",
            "| ```          |              |\n",
            "+--------------+--------------+\n",
            "after\n",
        );
        assert_eq!(protected(src), vec![1, 2, 3, 4, 5, 6, 7]);
        assert!(Scan::of(src).balanced(), "the cell fence must not leak out");
    }

    /// The table must end at its last rule, not run on into whatever follows.
    /// Without this, a GFM pipe table written under a grid table would be
    /// swallowed and silently exempted from every rewrite.
    #[test]
    fn test_a_grid_table_ends_at_its_last_rule_not_at_the_next_pipe_row() {
        let src = concat!(
            "+-----+\n",
            "| a   |\n",
            "+-----+\n",
            "| b | c |\n",
            "|---|---|\n",
            "| d | e |\n",
        );
        assert_eq!(protected(src), vec![0, 1, 2]);
    }

    #[test]
    fn test_a_thematic_break_is_not_mistaken_for_a_grid_rule() {
        assert_eq!(protected("a\n\n---\n\nb\n"), Vec::<usize>::new());
        assert_eq!(protected("a\n\n+++\n\nb\n"), Vec::<usize>::new());
    }

    /// R10: half a save is not a document. An unclosed fence reports
    /// unbalanced, and every pass declines rather than reclassifying the
    /// whole tail of the file for one `--watch` frame.
    #[test]
    fn test_an_unclosed_fence_reports_unbalanced_and_protects_to_end_of_file() {
        let src = "# Title\n\n```rust\nfn main() {\n";
        let scan = Scan::of(src);
        assert!(!scan.balanced());
        assert_eq!(scan.fences()[0].close, None);
        assert_eq!(protected(src), vec![2, 3]);
    }

    /// A file whose final line has no newline still has that line, and
    /// `split_inclusive` is what makes the index of it agree with every
    /// pass's own iteration.
    #[test]
    fn test_a_file_without_a_trailing_newline_still_classifies_its_last_line() {
        let src = "```\nbody\n```";
        assert_eq!(protected(src), vec![0, 1, 2]);
        assert!(Scan::of(src).balanced());
    }

    #[test]
    fn test_a_crlf_document_does_not_see_the_carriage_return_as_info_text() {
        let scan = Scan::of("```rust\r\nfn main() {}\r\n```\r\n");
        assert_eq!(scan.fences()[0].info, "rust");
        assert_eq!(scan.fences()[0].close, Some(2));
        assert!(scan.balanced());
    }

    #[test]
    fn test_an_index_past_the_end_is_reported_protected() {
        let scan = Scan::of("plain\n");
        assert!(!scan.is_protected(0));
        assert!(scan.is_protected(1), "out of range must fail safe");
        assert!(scan.is_protected(usize::MAX));
    }

    #[test]
    fn test_an_empty_document_never_panics() {
        let scan = Scan::of("");
        assert!(scan.balanced());
        assert!(scan.fences().is_empty());
    }

    /// The no-backtick-in-the-info rule belongs to *backtick* fences only.
    /// CommonMark has it so a line of prose full of code spans cannot open a
    /// block; a tilde fence has no such ambiguity, and its info string may
    /// say whatever it likes.
    #[test]
    fn test_a_tilde_fence_may_carry_a_backtick_in_its_info_string() {
        let src = "~~~ `code`\nbody\n~~~\nafter\n";
        assert_eq!(delimiter("~~~ `code`"), Some((b'~', 3)));
        assert_eq!(protected(src), vec![0, 1, 2]);
        assert_eq!(Scan::of(src).fences()[0].info, "`code`");
    }

    #[test]
    fn test_a_line_of_prose_full_of_code_spans_does_not_open_a_fence() {
        // Three separate one-backtick spans, not a fence; and the
        // backtick-in-info rule is what rejects the ``` case below it.
        assert_eq!(delimiter("use `a`, `b` or `c`"), None);
        assert_eq!(delimiter("```not a fence`"), None);
        assert_eq!(delimiter("``"), None);
        assert_eq!(delimiter("~~struck~~"), None);
    }

    #[test]
    fn test_container_stripping_takes_markers_but_not_lookalikes() {
        assert_eq!(container_content("  - - ``` md"), "``` md");
        assert_eq!(container_content("> > text"), "text");
        assert_eq!(container_content("3. 1) item"), "item");
        // Not markers: emphasis, a negative number, a bare digit run.
        assert_eq!(container_content("*emphasis*"), "*emphasis*");
        assert_eq!(container_content("-5 degrees"), "-5 degrees");
        assert_eq!(container_content("1234567890. x"), "1234567890. x");
    }

    /// The marker rules have edges, and each edge is a decision CommonMark
    /// already made: nine digits is the longest ordered marker, `.` and `)`
    /// are both delimiters, a bullet needs whitespace after it but `>` does
    /// not, and a tab counts as that whitespace. Getting any of them wrong
    /// changes which lines the scanner can see a fence on.
    #[test]
    fn test_every_container_marker_edge_is_the_one_commonmark_draws() {
        // Nine digits is a marker; ten is not. This is the boundary itself,
        // from both sides.
        assert_eq!(container_content("123456789. x"), "x");
        assert_eq!(container_content("1234567890. x"), "1234567890. x");
        // Both ordered delimiters, and both whitespace kinds after them.
        assert_eq!(container_content("1) x"), "x");
        assert_eq!(container_content("1.\tx"), "x");
        assert_eq!(container_content("-\tx"), "x");
        // A digit run with no delimiter, and a delimiter with nothing after.
        assert_eq!(container_content("12 x"), "12 x");
        assert_eq!(container_content("12.x"), "12.x");
        // `>` needs no space, and stacks.
        assert_eq!(container_content(">>>quoted"), "quoted");
        // All three bullet characters.
        for bullet in ["- x", "* x", "+ x"] {
            assert_eq!(container_content(bullet), "x", "for {bullet:?}");
        }
        // A bullet with nothing after it is not a marker to strip.
        assert_eq!(container_content("-"), "-");
    }

    /// Three spaces of indent is still the document's top level and four is
    /// not — the line where CommonMark stops seeing a fence and starts seeing
    /// an indented code block.
    ///
    /// The parser is the oracle for the *`top_level`* half: at four spaces it
    /// reads an indented code block, so the delimiter is no longer a
    /// delimiter to anybody but this scanner. That this scanner still calls
    /// it a fence is the documented conservatism — both answers protect the
    /// same lines, and `top_level` is what stops a pass from *rewriting* the
    /// line on top of that.
    #[test]
    fn test_four_spaces_of_indent_is_no_longer_a_top_level_fence() {
        let top = "   ```rust\n   x\n   ```\n";
        let indented = "    ```rust\n    x\n    ```\n";

        assert!(matches!(
            ast::Document::parse(top).blocks()[0].kind,
            ast::BlockKind::CodeBlock { .. }
        ));
        let scan = Scan::of(top);
        assert!(scan.fences()[0].top_level);
        assert_eq!(scan.fences()[0].info, "rust");

        // At four spaces the parser sees an indented code block whose
        // *content* is the backticks — no info string anywhere.
        let indented_doc = ast::Document::parse(indented);
        let ast::BlockKind::CodeBlock { info, literal, .. } = &indented_doc.blocks()[0].kind else {
            panic!("expected an indented code block");
        };
        assert_eq!(info.as_deref(), None);
        assert!(literal.starts_with("```rust"));

        let scan = Scan::of(indented);
        assert!(
            !scan.fences()[0].top_level,
            "a four-space-indented delimiter is not a top-level fence"
        );
        assert_eq!(
            protected(indented),
            vec![0, 1, 2],
            "and it is still protected"
        );
    }

    /// A grid rule has to be `+`, then at least one `-` or `=`, then `+`, at
    /// no more than three spaces of indent. Each clause is load-bearing:
    /// dropping any one of them turns some ordinary line of prose or ASCII
    /// art into the start of a protected region that runs to the next `+`.
    #[test]
    fn test_a_grid_rule_is_recognised_only_in_the_exact_shape_pandoc_writes() {
        // The smallest real rule, and the header form.
        assert_eq!(protected("+-+\n"), vec![0]);
        assert_eq!(protected("+=+\n"), vec![0]);
        assert_eq!(protected("   +---+\n"), vec![0]);
        // Not rules: too short, no dash or equals, foreign characters, and
        // indented past the point a block can start.
        for line in [
            "++\n",
            "+++\n",
            "+--x--+\n",
            "+---\n",
            "---+\n",
            "    +---+\n",
            "|---|\n",
        ] {
            assert_eq!(protected(line), Vec::<usize>::new(), "for {line:?}");
        }
    }

    /// A table's rows have the same three-space ceiling as its rules. A row
    /// past it ends the table, so the rule above it is the last protected
    /// line rather than the first of many.
    #[test]
    fn test_a_row_indented_past_three_spaces_does_not_continue_the_table() {
        assert_eq!(
            protected("+---+\n    | a |\nprose\n"),
            vec![0],
            "an over-indented row is not a row, so the table is its rule alone"
        );
        // And when a rule *does* follow such a row, it opens a new table of
        // its own rather than joining the one above it — which is the same
        // rule read from the other end, and is why the scan cannot simply
        // run to the last `+` line it can find.
        assert_eq!(protected("+---+\n    | a |\n+---+\n"), vec![0, 2]);
    }

    /// Two fences back to back, and a grid table straight after one. Both
    /// exercise where the scan resumes: one line too far and the second
    /// construct's opener is swallowed as content, one line too short and the
    /// closer re-opens a fence that runs to end of file.
    #[test]
    fn test_the_scan_resumes_correctly_after_each_construct_it_closes() {
        let src = concat!(
            "```a\n",
            "one\n",
            "```\n",     // 0..=2
            "between\n", // 3
            "```b\n",
            "two\n",
            "```\n", // 4..=6
            "+---+\n",
            "| c |\n",
            "+---+\n", // 7..=9
            "after\n", // 10
        );
        let scan = Scan::of(src);
        assert!(scan.balanced());
        assert_eq!(scan.fences().len(), 2, "{:?}", scan.fences());
        assert_eq!(scan.fences()[0].info, "a");
        assert_eq!(scan.fences()[1].info, "b");
        assert_eq!(scan.fences()[1].open, 4);
        assert_eq!(protected(src), vec![0, 1, 2, 4, 5, 6, 7, 8, 9]);
    }

    /// A closing run must be *at least* the opening one, so a run one shorter
    /// is content and a run of exactly the same length closes. Both sides of
    /// the comparison, with the parser agreeing on each.
    #[test]
    fn test_the_closing_run_comparison_is_exact_on_both_sides_of_the_boundary() {
        // Equal: closes.
        let equal = "````\nbody\n````\nafter\n";
        assert_eq!(Scan::of(equal).fences()[0].close, Some(2));
        assert_eq!(ast::Document::parse(equal).blocks().len(), 2);

        // One shorter: does not close, and the fence runs to end of file.
        let shorter = "````\nbody\n```\nafter\n";
        let scan = Scan::of(shorter);
        assert_eq!(scan.fences()[0].close, None);
        assert!(!scan.balanced());
        assert_eq!(
            ast::Document::parse(shorter).blocks().len(),
            1,
            "the parser agrees the shorter run did not close it"
        );
    }
}
